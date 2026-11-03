//! The simulation. One `tick` per input byte, 35 tics per second. Nothing
//! here allocates; all state lives in fixed arrays (~1.3 KB).

use crate::fx::{self, Fx, ONE};
use crate::map;
use crate::ray;
use crate::rng::Rng;
use crate::trig;
use crate::{FNV_OFFSET, fnv1a};

pub const MAX_ACTORS: usize = 64;
pub const TICS_PER_SECOND: u32 = 35;

/// One byte per tic: the tape format.
pub mod input {
    pub const FORWARD: u8 = 1 << 0;
    pub const BACK: u8 = 1 << 1;
    pub const STRAFE_LEFT: u8 = 1 << 2;
    pub const STRAFE_RIGHT: u8 = 1 << 3;
    pub const TURN_LEFT: u8 = 1 << 4;
    pub const TURN_RIGHT: u8 = 1 << 5;
    pub const FIRE: u8 = 1 << 6;
    pub const USE: u8 = 1 << 7;
}

const PLAYER_RADIUS: Fx = ONE / 4;
const ACTOR_RADIUS: Fx = ONE / 4;
const MOVE_SPEED: Fx = ONE / 10;
const ACTOR_SPEED: Fx = ONE / 25;
const TURN_SPEED: u32 = 12;
const FIRE_COOLDOWN: u8 = 10;
const ACTOR_COOLDOWN: u8 = 30;
const HIT_HALF_WIDTH: Fx = ONE * 35 / 100;
const SIGHT_RANGE: Fx = fx::from_int(12);
const ATTACK_RANGE: Fx = fx::from_int(8);
const SPAWN_MIN_DISTANCE: i32 = 6;
const SPAWN_ATTEMPTS: u32 = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ActorState {
    Dead = 0,
    Idle = 1,
    Chase = 2,
}

#[derive(Clone, Copy, Debug)]
pub struct Actor {
    pub x: Fx,
    pub y: Fx,
    pub hp: i16,
    pub state: ActorState,
    pub cooldown: u8,
}

impl Actor {
    const DEAD: Actor = Actor {
        x: 0,
        y: 0,
        hp: 0,
        state: ActorState::Dead,
        cooldown: 0,
    };
}

#[derive(Debug)]
pub struct Player {
    pub x: Fx,
    pub y: Fx,
    pub angle: u32,
    pub hp: i32,
    pub cooldown: u8,
}

pub struct Game {
    pub player: Player,
    pub actors: [Actor; MAX_ACTORS],
    pub actor_count: usize,
    pub tic: u32,
    pub kills: u32,
    /// Tic on which the last creature died; 0 while any are alive.
    pub cleared_at: u32,
    rng: Rng,
}

fn tile_center(t: i32) -> Fx {
    fx::from_int(t) + ONE / 2
}

fn blocked(x: Fx, y: Fx, radius: Fx) -> bool {
    let x0 = fx::to_int(x - radius);
    let x1 = fx::to_int(x + radius);
    let y0 = fx::to_int(y - radius);
    let y1 = fx::to_int(y + radius);
    map::tile(x0, y0) != map::EMPTY
        || map::tile(x1, y0) != map::EMPTY
        || map::tile(x0, y1) != map::EMPTY
        || map::tile(x1, y1) != map::EMPTY
}

/// Axis-separated movement so walls stop one axis and let you slide along them.
fn slide(x: &mut Fx, y: &mut Fx, dx: Fx, dy: Fx, radius: Fx) {
    if !blocked(*x + dx, *y, radius) {
        *x += dx;
    }
    if !blocked(*x, *y + dy, radius) {
        *y += dy;
    }
}

impl Game {
    pub fn new(seed: u32, actors: usize) -> Self {
        let (sx, sy) = map::PLAYER_START;
        let mut game = Game {
            player: Player {
                x: tile_center(sx),
                y: tile_center(sy),
                angle: 0,
                hp: 100,
                cooldown: 0,
            },
            actors: [Actor::DEAD; MAX_ACTORS],
            actor_count: actors.min(MAX_ACTORS),
            tic: 0,
            kills: 0,
            cleared_at: 0,
            rng: Rng::new(seed),
        };
        game.spawn_actors(sx, sy);
        game
    }

    fn spawn_actors(&mut self, sx: i32, sy: i32) {
        for i in 0..self.actor_count {
            for _ in 0..SPAWN_ATTEMPTS {
                let tx = self.rng.below(map::MAP_W as u32) as i32;
                let ty = self.rng.below(map::MAP_H as u32) as i32;
                let far = (tx - sx).abs() + (ty - sy).abs() >= SPAWN_MIN_DISTANCE;
                if far && map::tile(tx, ty) == map::EMPTY {
                    self.actors[i] = Actor {
                        x: tile_center(tx),
                        y: tile_center(ty),
                        hp: 20,
                        state: ActorState::Idle,
                        cooldown: 0,
                    };
                    break;
                }
            }
        }
    }

    pub fn alive(&self) -> bool {
        self.player.hp > 0
    }

    pub fn tick(&mut self, buttons: u8) {
        if !self.alive() {
            return;
        }
        self.tic += 1;
        self.move_player(buttons);
        if self.player.cooldown > 0 {
            self.player.cooldown -= 1;
        } else if buttons & input::FIRE != 0 {
            self.fire();
            self.player.cooldown = FIRE_COOLDOWN;
        }
        self.update_actors();
    }

    fn move_player(&mut self, buttons: u8) {
        let p = &mut self.player;
        if buttons & input::TURN_LEFT != 0 {
            p.angle = p.angle.wrapping_sub(TURN_SPEED) % trig::ANGLES;
        }
        if buttons & input::TURN_RIGHT != 0 {
            p.angle = (p.angle + TURN_SPEED) % trig::ANGLES;
        }
        let (cos, sin) = (trig::cos(p.angle), trig::sin(p.angle));
        let mut forward = 0;
        let mut strafe = 0;
        if buttons & input::FORWARD != 0 {
            forward += MOVE_SPEED;
        }
        if buttons & input::BACK != 0 {
            forward -= MOVE_SPEED;
        }
        if buttons & input::STRAFE_RIGHT != 0 {
            strafe += MOVE_SPEED;
        }
        if buttons & input::STRAFE_LEFT != 0 {
            strafe -= MOVE_SPEED;
        }
        let dx = fx::mul(cos, forward) - fx::mul(sin, strafe);
        let dy = fx::mul(sin, forward) + fx::mul(cos, strafe);
        slide(&mut p.x, &mut p.y, dx, dy, PLAYER_RADIUS);
    }

    /// Hitscan: the nearest actor inside a narrow corridor along the view
    /// direction, closer than the first wall, takes the shot.
    fn fire(&mut self) {
        let p = &self.player;
        let (cos, sin) = (trig::cos(p.angle), trig::sin(p.angle));
        let wall = ray::cast(p.x, p.y, cos, sin).dist;
        let mut target = None;
        let mut nearest = wall;
        for (i, a) in self.actors[..self.actor_count].iter().enumerate() {
            if a.state == ActorState::Dead {
                continue;
            }
            let (dx, dy) = (a.x - p.x, a.y - p.y);
            let along = fx::mul(dx, cos) + fx::mul(dy, sin);
            let across = fx::mul(dy, cos) - fx::mul(dx, sin);
            if along > 0 && along < nearest && across.abs() < HIT_HALF_WIDTH {
                nearest = along;
                target = Some(i);
            }
        }
        if let Some(i) = target {
            let damage = 10 + self.rng.below(10) as i16;
            let a = &mut self.actors[i];
            a.hp -= damage;
            a.state = ActorState::Chase;
            if a.hp <= 0 {
                a.state = ActorState::Dead;
                self.kills += 1;
                if self.kills as usize == self.actor_count {
                    self.cleared_at = self.tic;
                }
            }
        }
    }

    fn update_actors(&mut self) {
        let (px, py) = (self.player.x, self.player.y);
        for i in 0..self.actor_count {
            let mut a = self.actors[i];
            if a.state == ActorState::Dead {
                continue;
            }
            if a.cooldown > 0 {
                a.cooldown -= 1;
            }
            let (dx, dy) = (px - a.x, py - a.y);
            let distance = dx.abs().max(dy.abs());
            // Stagger expensive sight checks across tics by actor index.
            let phase = (self.tic + i as u32) % 4 == 0;
            match a.state {
                ActorState::Idle => {
                    if phase && distance < SIGHT_RANGE && ray::line_clear(a.x, a.y, px, py) {
                        a.state = ActorState::Chase;
                    }
                }
                ActorState::Chase => {
                    let step_x = dx.clamp(-ACTOR_SPEED, ACTOR_SPEED);
                    let step_y = dy.clamp(-ACTOR_SPEED, ACTOR_SPEED);
                    if distance > ONE {
                        slide(&mut a.x, &mut a.y, step_x, step_y, ACTOR_RADIUS);
                    }
                    let can_attack = phase && a.cooldown == 0 && distance < ATTACK_RANGE;
                    if can_attack && ray::line_clear(a.x, a.y, px, py) {
                        a.cooldown = ACTOR_COOLDOWN;
                        if self.rng.below(256) < 96 {
                            self.player.hp -= 3 + self.rng.below(5) as i32;
                        }
                    }
                }
                ActorState::Dead => {}
            }
            self.actors[i] = a;
        }
    }

    /// Hash of everything that affects future tics. Equal hashes on browser
    /// and chain mean the replay matched.
    pub fn state_hash(&self) -> u32 {
        let p = &self.player;
        let mut h = FNV_OFFSET;
        for v in [p.x, p.y, p.angle as i32, p.hp, p.cooldown as i32] {
            h = fnv1a(h, &v.to_le_bytes());
        }
        for v in [self.tic, self.kills, self.cleared_at, self.rng.state()] {
            h = fnv1a(h, &v.to_le_bytes());
        }
        for a in &self.actors[..self.actor_count] {
            h = fnv1a(h, &a.x.to_le_bytes());
            h = fnv1a(h, &a.y.to_le_bytes());
            h = fnv1a(h, &[a.hp as u8, (a.hp >> 8) as u8, a.state as u8, a.cooldown]);
        }
        h
    }
}

/// Deterministic stand-in for a player, used by probes and tests: walks,
/// turns, strafes and fires on a fixed rhythm.
pub fn scripted_input(tic: u32) -> u8 {
    let mut buttons = match (tic / TICS_PER_SECOND) % 6 {
        0 | 1 => input::FORWARD,
        2 => input::FORWARD | input::TURN_LEFT,
        3 => input::STRAFE_RIGHT,
        4 => input::TURN_RIGHT,
        _ => input::BACK | input::TURN_LEFT,
    };
    if tic % 7 == 0 {
        buttons |= input::FIRE;
    }
    buttons
}
