//! 160×100, 16-color (4 bits per pixel) software raycaster. The framebuffer
//! is 8,000 bytes, small enough for the contract's 64 KiB guest memory.
//! Textures are procedural, so no texture data has to be stored or loaded.

use crate::fx::{self, Fx, ONE};
use crate::game::{ActorState, Game, MAX_ACTORS};
use crate::map;
use crate::ray;
use crate::trig;
use crate::{FNV_OFFSET, fnv1a};

pub const W: usize = 160;
pub const H: usize = 100;
pub const FB_BYTES: usize = W * H / 2;

/// Field of view: camera plane length relative to the view direction (~66°).
const PLANE_SCALE: Fx = ONE * 66 / 100;
const MAX_LINE: i64 = 8 * H as i64;
const NEAR_CLIP: Fx = ONE / 5;
const TEX: i32 = 16;

pub const PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],       // 0 black
    [40, 40, 48],    // 1 ceiling
    [70, 56, 40],    // 2 floor
    [150, 60, 40],   // 3 brick
    [125, 48, 32],   // 4 brick alt
    [120, 120, 110], // 5 stone
    [95, 95, 88],    // 6 stone alt
    [60, 55, 50],    // 7 mortar
    [90, 35, 24],    // 8 brick shaded
    [40, 70, 80],    // 9 tech frame
    [60, 110, 120],  // 10 tech panel
    [65, 65, 60],    // 11 stone shaded
    [170, 30, 30],   // 12 creature
    [230, 220, 120], // 13 light / eyes
    [100, 15, 15],   // 14 creature shaded
    [240, 240, 240], // 15 white
];

/// Darker variant per color, applied to y-facing walls for depth cues.
const SHADE: [u8; 16] = [0, 0, 0, 8, 8, 11, 11, 0, 8, 0, 9, 11, 14, 6, 14, 5];

const CEILING: u8 = 1;
const FLOOR: u8 = 2;

#[inline]
fn put(fb: &mut [u8; FB_BYTES], x: usize, y: usize, color: u8) {
    let i = y * W + x;
    let byte = &mut fb[i / 2];
    *byte = if i % 2 == 0 {
        (*byte & 0xf0) | color
    } else {
        (*byte & 0x0f) | (color << 4)
    };
}

pub fn pixel(fb: &[u8; FB_BYTES], x: usize, y: usize) -> u8 {
    let i = y * W + x;
    if i % 2 == 0 { fb[i / 2] & 0x0f } else { fb[i / 2] >> 4 }
}

fn brick(u: i32, v: i32) -> u8 {
    let row = v / 4;
    let offset = if row % 2 == 1 { 4 } else { 0 };
    if v % 4 == 0 || (u + offset) % 8 == 0 {
        7
    } else if ((u + offset) / 8 + row) % 2 == 0 {
        3
    } else {
        4
    }
}

fn stone(u: i32, v: i32) -> u8 {
    if u % 8 == 0 || v % 8 == 0 {
        11
    } else if (u / 8 + v / 8) % 2 == 0 {
        5
    } else {
        6
    }
}

fn tech(u: i32, v: i32) -> u8 {
    if u == 0 || v == 0 || u == TEX - 1 {
        9
    } else if v == 8 && u % 3 == 0 {
        13
    } else {
        10
    }
}

fn wall_texel(tile: u8, u: i32, v: i32, side: u8) -> u8 {
    let color = match tile {
        map::STONE => stone(u, v),
        map::TECH => tech(u, v),
        _ => brick(u, v),
    };
    if side == 1 { SHADE[color as usize] } else { color }
}

/// A floating creature: an oval body with two glowing eyes. 0 = transparent.
fn creature_texel(u: i32, v: i32) -> u8 {
    let du = u - TEX / 2;
    let dv = v - 9;
    if du * du * 4 + dv * dv * 3 > 200 {
        0
    } else if (5..7).contains(&v) && matches!(u, 5 | 6 | 10 | 11) {
        13
    } else if v > 12 {
        14
    } else {
        12
    }
}

struct Camera {
    x: Fx,
    y: Fx,
    dir_x: Fx,
    dir_y: Fx,
    plane_x: Fx,
    plane_y: Fx,
}

impl Camera {
    fn from(game: &Game) -> Self {
        let p = &game.player;
        let (dir_x, dir_y) = (trig::cos(p.angle), trig::sin(p.angle));
        Camera {
            x: p.x,
            y: p.y,
            dir_x,
            dir_y,
            plane_x: fx::mul(-dir_y, PLANE_SCALE),
            plane_y: fx::mul(dir_x, PLANE_SCALE),
        }
    }
}

/// Renders one frame and returns the per-column wall depth (for callers that
/// want to composite more sprites).
pub fn render(game: &Game, fb: &mut [u8; FB_BYTES]) -> [Fx; W] {
    let cam = Camera::from(game);
    let mut zbuf = [0; W];
    draw_walls(&cam, fb, &mut zbuf);
    draw_actors(game, &cam, fb, &zbuf);
    zbuf
}

fn draw_walls(cam: &Camera, fb: &mut [u8; FB_BYTES], zbuf: &mut [Fx; W]) {
    for x in 0..W {
        let camera_x = ((2 * x as i32 - W as i32) * ONE) / W as i32;
        let ray_x = cam.dir_x + fx::mul(cam.plane_x, camera_x);
        let ray_y = cam.dir_y + fx::mul(cam.plane_y, camera_x);
        let hit = ray::cast(cam.x, cam.y, ray_x, ray_y);
        zbuf[x] = hit.dist;

        let line = ((((H as i64) << 16) / hit.dist as i64).min(MAX_LINE)) as i32;
        let top = H as i32 / 2 - line / 2;
        let u = fx::to_int(hit.wall_x * TEX);
        for y in 0..H {
            let yi = y as i32;
            let color = if yi < top {
                CEILING
            } else if yi >= top + line {
                FLOOR
            } else {
                let v = ((yi - top) * TEX / line.max(1)).clamp(0, TEX - 1);
                wall_texel(hit.tile, u, v, hit.side)
            };
            put(fb, x, y, color);
        }
    }
}

fn draw_actors(game: &Game, cam: &Camera, fb: &mut [u8; FB_BYTES], zbuf: &[Fx; W]) {
    // Painter's order: far to near, so nearer creatures overdraw farther ones.
    let mut order = [0u8; MAX_ACTORS];
    let mut depth = [0 as Fx; MAX_ACTORS];
    let mut count = 0;
    for (i, a) in game.actors[..game.actor_count].iter().enumerate() {
        if a.state == ActorState::Dead {
            continue;
        }
        let (dx, dy) = (a.x - cam.x, a.y - cam.y);
        let d = fx::mul(dx, dx) + fx::mul(dy, dy);
        let mut j = count;
        while j > 0 && depth[j - 1] < d {
            depth[j] = depth[j - 1];
            order[j] = order[j - 1];
            j -= 1;
        }
        depth[j] = d;
        order[j] = i as u8;
        count += 1;
    }

    let det = fx::mul(cam.plane_x, cam.dir_y) - fx::mul(cam.dir_x, cam.plane_y);
    for &i in &order[..count] {
        let a = &game.actors[i as usize];
        let (sx, sy) = (a.x - cam.x, a.y - cam.y);
        // Camera-space coordinates: lateral offset and depth.
        let tx = fx::div(fx::mul(cam.dir_y, sx) - fx::mul(cam.dir_x, sy), det);
        let ty = fx::div(fx::mul(cam.plane_x, sy) - fx::mul(cam.plane_y, sx), det);
        if ty < NEAR_CLIP {
            continue;
        }
        let half_w = W as i64 / 2;
        let screen_x = (half_w + half_w * tx as i64 / ty as i64) as i32;
        let size = ((((H as i64) << 16) / ty as i64).min(2 * H as i64)) as i32;
        let left = screen_x - size / 2;
        let top = H as i32 / 2 - size / 2;
        for col in left.max(0)..(left + size).min(W as i32) {
            if ty >= zbuf[col as usize] {
                continue;
            }
            let u = (col - left) * TEX / size;
            for row in top.max(0)..(top + size).min(H as i32) {
                let v = (row - top) * TEX / size;
                let color = creature_texel(u, v);
                if color != 0 {
                    put(fb, col as usize, row as usize, color);
                }
            }
        }
    }
}

pub fn frame_hash(fb: &[u8; FB_BYTES]) -> u32 {
    fnv1a(FNV_OFFSET, fb)
}
