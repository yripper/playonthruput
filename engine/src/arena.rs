//! The multiplayer arena: an endless deathmatch in one room.
//!
//! The chain is the server. It simulates `SIM_DELAY` tics behind real time so
//! every player's inputs have landed before their tic is simulated (lockstep,
//! like DOOM's original netcode). Browsers predict the present from the last
//! confirmed state, and render other players `REWIND` tics in the past, where
//! their inputs are already known. Shots are checked against those same
//! rewound positions ("favor the shooter"), so what you saw is what you hit.

use crate::fx::{self, Fx, ONE};
use crate::game::{input, slide, tile_center};
use crate::map::ARENA;
use crate::ray;
use crate::render::{self, FB_BYTES, Sprite, View};
use crate::rng::Rng;
use crate::trig;

pub const MAX_PLAYERS: usize = 16;
pub const TICS_PER_SECOND: u64 = 35;
/// Tics of buffered future inputs per player.
pub const INPUT_RING: usize = 256;
/// Tics of position history per player (must exceed `REWIND`).
pub const HISTORY_RING: usize = 128;
/// The chain simulates this far behind real time (4 s).
pub const SIM_DELAY: u32 = 140;
/// Other players are seen, and shot, this many tics in the past (3 s).
pub const REWIND: u32 = 105;
/// Most tics one crank simulates; longer gaps are skipped.
pub const MAX_STEPS: u32 = 280;
pub const RESPAWN_TICS: u32 = 70;
/// Players who send no inputs for a minute leave the arena.
pub const IDLE_KICK_TICS: u32 = 35 * 60;
pub const MAX_HP: i16 = 100;
pub const MAX_KILLS_PER_STEP_LOG: usize = 8;

const PLAYER_RADIUS: Fx = ONE / 4;
const MOVE_SPEED: Fx = ONE / 10;
const TURN_SPEED: u32 = 12;
const FIRE_COOLDOWN: u8 = 12;
const HIT_HALF_WIDTH: Fx = ONE * 4 / 10;
const BASE_DAMAGE: i16 = 30;

/// Body colors per slot (palette indices).
pub const PLAYER_COLORS: [u8; MAX_PLAYERS] = [12, 10, 5, 3, 13, 9, 15, 6, 12, 10, 5, 3, 13, 9, 15, 6];

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Player {
    /// The player's account address; all zeros when the slot is free.
    pub id: [u8; 32],
    pub active: bool,
    pub alive: bool,
    pub x: Fx,
    pub y: Fx,
    pub angle: u32,
    pub hp: i16,
    pub cooldown: u8,
    pub frags: u16,
    pub deaths: u16,
    /// Tic the player (re)spawns at while dead.
    pub respawn_at: u32,
    /// Tic of the latest spawn; shots can't rewind to before it.
    pub spawned_at: u32,
    /// Highest tic the player has sent input for.
    pub last_input: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kill {
    pub tic: u32,
    pub killer: u8,
    pub victim: u8,
    /// The killer's eyes when firing: the kill cam's camera.
    pub shooter: View,
    /// Where the victim was in the killer's (rewound) view.
    pub target_x: Fx,
    pub target_y: Fx,
}

/// Kills from one crank. Overflow is counted, not stored.
pub struct KillLog {
    pub kills: [Option<Kill>; MAX_KILLS_PER_STEP_LOG],
    pub count: usize,
    pub dropped: u32,
}

impl KillLog {
    pub const fn new() -> Self {
        KillLog {
            kills: [None; MAX_KILLS_PER_STEP_LOG],
            count: 0,
            dropped: 0,
        }
    }

    fn push(&mut self, kill: Kill) {
        if self.count < MAX_KILLS_PER_STEP_LOG {
            self.kills[self.count] = Some(kill);
            self.count += 1;
        } else {
            self.dropped += 1;
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Kill> {
        self.kills[..self.count].iter().flatten()
    }
}

impl Default for KillLog {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Arena {
    /// Unix time (ns) of tic 0.
    pub genesis_ns: u64,
    /// Next tic to simulate; everything before it is final.
    pub tic: u32,
    rng: Rng,
    pub players: [Player; MAX_PLAYERS],
    /// `inputs[slot][t % INPUT_RING]`: buttons for tic t, for t in `tic..tic + INPUT_RING`.
    inputs: [[u8; INPUT_RING]; MAX_PLAYERS],
    /// `history[slot][t % HISTORY_RING]`: quantized position after tic t.
    history: [[[u8; 2]; HISTORY_RING]; MAX_PLAYERS],
}

/// Tic number at `now_ns` for an arena started at `genesis_ns`.
pub fn tic_at(genesis_ns: u64, now_ns: u64) -> u32 {
    let elapsed = now_ns.saturating_sub(genesis_ns) as u128;
    (elapsed * TICS_PER_SECOND as u128 / 1_000_000_000) as u32
}

/// Positions are stored at 1/8-tile precision: 32 tiles fit in a byte.
fn quantize(v: Fx) -> u8 {
    (v >> 13).clamp(0, 255) as u8
}

fn dequantize(q: u8) -> Fx {
    ((q as Fx) << 13) + (1 << 12)
}

impl Arena {
    pub fn new(genesis_ns: u64, seed: u32) -> Self {
        Arena {
            genesis_ns,
            tic: 0,
            rng: Rng::new(seed),
            players: [Player::default(); MAX_PLAYERS],
            inputs: [[0; INPUT_RING]; MAX_PLAYERS],
            history: [[[0; 2]; HISTORY_RING]; MAX_PLAYERS],
        }
    }

    /// `new`, in place. The arena is ~9 KiB, and a contract can't afford the
    /// stack copies a by-value constructor makes.
    pub fn reset(&mut self, genesis_ns: u64, seed: u32) {
        self.genesis_ns = genesis_ns;
        self.tic = 0;
        self.rng = Rng::new(seed);
        self.players = [Player::default(); MAX_PLAYERS];
        for ring in self.inputs.iter_mut() {
            ring.fill(0);
        }
        for ring in self.history.iter_mut() {
            ring.fill([0; 2]);
        }
    }

    pub fn slot_of(&self, id: &[u8; 32]) -> Option<usize> {
        self.players.iter().position(|p| p.active && &p.id == id)
    }

    /// Takes a free slot for `id` (or returns its existing one). The player
    /// spawns on the next simulated tic. `None` when the arena is full.
    pub fn join(&mut self, id: &[u8; 32]) -> Option<usize> {
        if let Some(slot) = self.slot_of(id) {
            return Some(slot);
        }
        let slot = self.players.iter().position(|p| !p.active)?;
        self.players[slot] = Player {
            id: *id,
            active: true,
            respawn_at: self.tic,
            last_input: self.tic,
            ..Player::default()
        };
        self.inputs[slot] = [0; INPUT_RING];
        Some(slot)
    }

    /// Buffers `buttons` for tics `start..start + len`. Tics already simulated,
    /// or too far ahead to buffer, are dropped.
    pub fn apply_inputs(&mut self, slot: usize, start: u32, buttons: &[u8]) {
        let window = self.tic..self.tic + INPUT_RING as u32;
        let mut latest = None;
        for (i, &b) in buttons.iter().enumerate() {
            let t = start.wrapping_add(i as u32);
            if window.contains(&t) {
                self.inputs[slot][t as usize % INPUT_RING] = b;
                latest = Some(t);
            }
        }
        if let Some(t) = latest {
            let p = &mut self.players[slot];
            p.last_input = p.last_input.max(t);
        }
    }

    /// Buffered buttons for `slot` at tic `t` (0 if none or out of window).
    pub fn input_at(&self, slot: usize, t: u32) -> u8 {
        if t >= self.tic && t < self.tic + INPUT_RING as u32 {
            self.inputs[slot][t as usize % INPUT_RING]
        } else {
            0
        }
    }

    /// Where `slot` stood after tic `t`, if `t` is still in the history.
    pub fn position_at(&self, slot: usize, t: u32) -> Option<(Fx, Fx)> {
        if t >= self.tic || self.tic - t > HISTORY_RING as u32 {
            return None;
        }
        let [qx, qy] = self.history[slot][t as usize % HISTORY_RING];
        Some((dequantize(qx), dequantize(qy)))
    }

    /// Simulates up to (not including) `target`, skipping ahead first if the
    /// arena sat idle longer than `MAX_STEPS`.
    pub fn advance_to(&mut self, target: u32, kills: &mut KillLog) {
        if target > self.tic + MAX_STEPS {
            self.skip_to(target - MAX_STEPS);
        }
        while self.tic < target {
            self.step(kills);
        }
    }

    /// Jumps forward without simulating: nobody moves, buffered inputs for
    /// skipped tics are discarded, and history holds still.
    fn skip_to(&mut self, target: u32) {
        let skipped = target - self.tic;
        for slot in 0..MAX_PLAYERS {
            let (x, y) = (self.players[slot].x, self.players[slot].y);
            for i in 0..skipped.min(INPUT_RING as u32) {
                self.inputs[slot][(self.tic + i) as usize % INPUT_RING] = 0;
            }
            for i in 0..skipped.min(HISTORY_RING as u32) {
                self.history[slot][(self.tic + i) as usize % HISTORY_RING] = [quantize(x), quantize(y)];
            }
        }
        self.tic = target;
    }

    pub fn step(&mut self, kills: &mut KillLog) {
        let t = self.tic;
        let mut buttons = [0u8; MAX_PLAYERS];
        let alive_at_start: [bool; MAX_PLAYERS] = core::array::from_fn(|s| self.players[s].alive);

        for s in 0..MAX_PLAYERS {
            buttons[s] = self.inputs[s][t as usize % INPUT_RING];
            self.inputs[s][t as usize % INPUT_RING] = 0;
            if !self.players[s].active {
                continue;
            }
            if !self.players[s].alive && t >= self.players[s].respawn_at {
                self.spawn(s, t);
            }
            if self.players[s].alive {
                Self::move_player(&mut self.players[s], buttons[s]);
            }
        }

        for s in 0..MAX_PLAYERS {
            let p = &self.players[s];
            self.history[s][t as usize % HISTORY_RING] = [quantize(p.x), quantize(p.y)];
        }

        for s in 0..MAX_PLAYERS {
            let p = &mut self.players[s];
            if !p.active || !alive_at_start[s] {
                continue;
            }
            if p.cooldown > 0 {
                p.cooldown -= 1;
            } else if buttons[s] & input::FIRE != 0 {
                p.cooldown = FIRE_COOLDOWN;
                self.fire(s, t, kills);
            }
        }

        for p in self.players.iter_mut() {
            if p.active && t > p.last_input + IDLE_KICK_TICS {
                *p = Player::default();
            }
        }
        self.tic += 1;
    }

    fn spawn(&mut self, s: usize, t: u32) {
        let spawns = ARENA.spawns();
        let (tx, ty) = spawns[self.rng.below(spawns.len() as u32) as usize];
        let p = &mut self.players[s];
        p.alive = true;
        p.hp = MAX_HP;
        p.x = tile_center(tx);
        p.y = tile_center(ty);
        // Face the room's center from any spawn.
        p.angle = if tx < 16 { 0 } else { 512 };
        p.spawned_at = t;
        p.cooldown = 0;
    }

    fn move_player(p: &mut Player, b: u8) {
        if b & input::TURN_LEFT != 0 {
            p.angle = p.angle.wrapping_sub(TURN_SPEED) % trig::ANGLES;
        }
        if b & input::TURN_RIGHT != 0 {
            p.angle = (p.angle + TURN_SPEED) % trig::ANGLES;
        }
        let (cos, sin) = (trig::cos(p.angle), trig::sin(p.angle));
        let forward = match (b & input::FORWARD != 0, b & input::BACK != 0) {
            (true, false) => MOVE_SPEED,
            (false, true) => -MOVE_SPEED,
            _ => 0,
        };
        let strafe = match (b & input::STRAFE_RIGHT != 0, b & input::STRAFE_LEFT != 0) {
            (true, false) => MOVE_SPEED,
            (false, true) => -MOVE_SPEED,
            _ => 0,
        };
        let dx = fx::mul(cos, forward) - fx::mul(sin, strafe);
        let dy = fx::mul(sin, forward) + fx::mul(cos, strafe);
        slide(&ARENA, &mut p.x, &mut p.y, dx, dy, PLAYER_RADIUS);
    }

    /// Where `target` appeared to a shooter firing at tic `t`, or `None` if
    /// they hadn't spawned yet in that view (spawn protection).
    fn seen_position(&self, target: usize, t: u32) -> Option<(Fx, Fx)> {
        let seen = t.checked_sub(REWIND)?;
        if seen < self.players[target].spawned_at {
            return None;
        }
        let [qx, qy] = self.history[target][seen as usize % HISTORY_RING];
        Some((dequantize(qx), dequantize(qy)))
    }

    fn fire(&mut self, s: usize, t: u32, kills: &mut KillLog) {
        let shooter = self.players[s];
        let (cos, sin) = (trig::cos(shooter.angle), trig::sin(shooter.angle));
        let wall = ray::cast(&ARENA, shooter.x, shooter.y, cos, sin).dist;
        let mut best: Option<(usize, Fx, Fx, Fx)> = None;
        for o in 0..MAX_PLAYERS {
            let target = &self.players[o];
            if o == s || !target.active || !target.alive {
                continue;
            }
            let Some((tx, ty)) = self.seen_position(o, t) else {
                continue;
            };
            let (dx, dy) = (tx - shooter.x, ty - shooter.y);
            let along = fx::mul(dx, cos) + fx::mul(dy, sin);
            let across = fx::mul(dy, cos) - fx::mul(dx, sin);
            let nearer = best.is_none_or(|(_, d, _, _)| along < d);
            if along > 0 && along < wall && across.abs() < HIT_HALF_WIDTH && nearer {
                best = Some((o, along, tx, ty));
            }
        }
        let Some((o, _, tx, ty)) = best else {
            return;
        };
        let damage = BASE_DAMAGE + self.rng.below(10) as i16;
        let victim = &mut self.players[o];
        victim.hp -= damage;
        if victim.hp > 0 {
            return;
        }
        victim.alive = false;
        victim.deaths += 1;
        victim.respawn_at = t + RESPAWN_TICS;
        self.players[s].frags += 1;
        kills.push(Kill {
            tic: t,
            killer: s as u8,
            victim: o as u8,
            shooter: View {
                x: shooter.x,
                y: shooter.y,
                angle: shooter.angle,
            },
            target_x: tx,
            target_y: ty,
        });
    }

    /// Other living players as `viewer` sees them at tic `t`: rewound by
    /// `REWIND`, exactly where shots against them are checked.
    pub fn sprites_seen_by(&self, viewer: Option<usize>, t: u32, out: &mut [Sprite; MAX_PLAYERS]) -> usize {
        let mut n = 0;
        for o in 0..MAX_PLAYERS {
            if Some(o) == viewer || !self.players[o].active || !self.players[o].alive {
                continue;
            }
            if let Some((x, y)) = self.seen_position(o, t + 1) {
                out[n] = Sprite {
                    x,
                    y,
                    color: PLAYER_COLORS[o],
                };
                n += 1;
            }
        }
        n
    }

    /// The kill cam: the killer's view at the fatal shot, with the victim where
    /// the killer saw them.
    pub fn render_kill(&self, kill: &Kill, fb: &mut [u8; FB_BYTES]) {
        let mut sprites = [Sprite { x: 0, y: 0, color: 0 }; MAX_PLAYERS];
        sprites[0] = Sprite {
            x: kill.target_x,
            y: kill.target_y,
            color: PLAYER_COLORS[kill.victim as usize],
        };
        render::render_view(&ARENA, &kill.shooter, &sprites[..1], fb);
    }
}

// ---------------------------------------------------------------------------
// Fixed binary layout, so the contract can store the arena as bytestrings and
// browsers can decode it. All integers little-endian.

const PLAYER_LEN: usize = 32 + 1 + 1 + 4 + 4 + 2 + 2 + 1 + 2 + 2 + 4 + 4 + 4;
const HEADER_LEN: usize = 1 + 8 + 4 + 4;
pub const STATE_VERSION: u8 = 1;
pub const STATE_LEN: usize =
    HEADER_LEN + MAX_PLAYERS * PLAYER_LEN + MAX_PLAYERS * INPUT_RING + MAX_PLAYERS * HISTORY_RING * 2;
pub const CHUNK_LEN: usize = 4096;
pub const STATE_CHUNKS: usize = STATE_LEN.div_ceil(CHUNK_LEN);
/// The state as stored: zero-padded to whole chunks, so every chunk has the
/// same length and can be read into a fixed-size buffer.
pub const STORED_LEN: usize = STATE_CHUNKS * CHUNK_LEN;

struct Writer<'a> {
    buf: &'a mut [u8],
    at: usize,
}

impl Writer<'_> {
    fn bytes(&mut self, b: &[u8]) {
        self.buf[self.at..self.at + b.len()].copy_from_slice(b);
        self.at += b.len();
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let mut out = [0; N];
        out.copy_from_slice(&self.buf[self.at..self.at + N]);
        self.at += N;
        out
    }
    fn u8(&mut self) -> u8 {
        self.take::<1>()[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_le_bytes(self.take())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take())
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.take())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeError {
    TooShort,
    BadVersion,
}

impl Arena {
    /// Writes the arena into `out[..STATE_LEN]`.
    pub fn encode(&self, out: &mut [u8]) {
        let mut w = Writer { buf: out, at: 0 };
        w.bytes(&[STATE_VERSION]);
        w.bytes(&self.genesis_ns.to_le_bytes());
        w.bytes(&self.tic.to_le_bytes());
        w.bytes(&self.rng.state().to_le_bytes());
        for p in &self.players {
            w.bytes(&p.id);
            w.bytes(&[p.active as u8, p.alive as u8]);
            w.bytes(&p.x.to_le_bytes());
            w.bytes(&p.y.to_le_bytes());
            w.bytes(&(p.angle as u16).to_le_bytes());
            w.bytes(&p.hp.to_le_bytes());
            w.bytes(&[p.cooldown]);
            w.bytes(&p.frags.to_le_bytes());
            w.bytes(&p.deaths.to_le_bytes());
            w.bytes(&p.respawn_at.to_le_bytes());
            w.bytes(&p.spawned_at.to_le_bytes());
            w.bytes(&p.last_input.to_le_bytes());
        }
        for ring in &self.inputs {
            w.bytes(ring);
        }
        for ring in &self.history {
            for pos in ring {
                w.bytes(pos);
            }
        }
        debug_assert_eq!(w.at, STATE_LEN);
    }

    /// Reads an arena written by `encode`.
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut arena = Arena::new(0, 0);
        arena.decode_into(bytes)?;
        Ok(arena)
    }

    /// `decode`, in place (see `reset`).
    pub fn decode_into(&mut self, bytes: &[u8]) -> Result<(), DecodeError> {
        if bytes.len() < STATE_LEN {
            return Err(DecodeError::TooShort);
        }
        let mut r = Reader { buf: bytes, at: 0 };
        if r.u8() != STATE_VERSION {
            return Err(DecodeError::BadVersion);
        }
        self.genesis_ns = u64::from_le_bytes(r.take());
        self.tic = r.u32();
        self.rng = Rng::new(r.u32());
        for p in self.players.iter_mut() {
            p.id = r.take();
            p.active = r.u8() != 0;
            p.alive = r.u8() != 0;
            p.x = r.i32();
            p.y = r.i32();
            p.angle = r.u16() as u32;
            p.hp = r.u16() as i16;
            p.cooldown = r.u8();
            p.frags = r.u16();
            p.deaths = r.u16();
            p.respawn_at = r.u32();
            p.spawned_at = r.u32();
            p.last_input = r.u32();
        }
        for ring in self.inputs.iter_mut() {
            ring.copy_from_slice(&bytes[r.at..r.at + INPUT_RING]);
            r.at += INPUT_RING;
        }
        for ring in self.history.iter_mut() {
            for pos in ring.iter_mut() {
                *pos = r.take();
            }
        }
        Ok(())
    }

    /// Hash of the full state, for parity checks between chain and browser.
    pub fn state_hash(&self, scratch: &mut [u8]) -> u32 {
        self.encode(scratch);
        crate::fnv1a(crate::FNV_OFFSET, &scratch[..STATE_LEN])
    }
}
