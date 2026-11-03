//! Grid DDA raycast, shared by the renderer (one ray per column) and the
//! hitscan weapon (one ray per shot).

use crate::fx::{self, Fx, ONE};
use crate::map::{self, Level};

/// The map border is solid, so a ray crosses at most ~64 cells.
const MAX_STEPS: u32 = 64;
/// Caps 1/|dir| for near-axis-aligned rays so sums stay inside i32.
const MAX_DELTA: i64 = 1 << 28;
const MIN_DIST: Fx = ONE / 64;

pub struct Hit {
    /// Perpendicular distance, in units of the ray direction's length.
    pub dist: Fx,
    /// 0 = hit a vertical (x) face, 1 = horizontal (y) face.
    pub side: u8,
    pub tile: u8,
    /// Fractional position along the wall face, for texturing.
    pub wall_x: Fx,
}

fn delta(dir: Fx) -> Fx {
    if dir == 0 {
        MAX_DELTA as Fx
    } else {
        ((1i64 << 32) / (dir.unsigned_abs() as i64)).min(MAX_DELTA) as Fx
    }
}

pub fn cast(level: &Level, px: Fx, py: Fx, dir_x: Fx, dir_y: Fx) -> Hit {
    let mut map_x = fx::to_int(px);
    let mut map_y = fx::to_int(py);
    let delta_x = delta(dir_x);
    let delta_y = delta(dir_y);

    let (step_x, mut side_x) = if dir_x < 0 {
        (-1, fx::mul(px - fx::from_int(map_x), delta_x))
    } else {
        (1, fx::mul(fx::from_int(map_x + 1) - px, delta_x))
    };
    let (step_y, mut side_y) = if dir_y < 0 {
        (-1, fx::mul(py - fx::from_int(map_y), delta_y))
    } else {
        (1, fx::mul(fx::from_int(map_y + 1) - py, delta_y))
    };

    let mut side = 0;
    let mut tile = map::BRICK;
    for _ in 0..MAX_STEPS {
        if side_x < side_y {
            side_x += delta_x;
            map_x += step_x;
            side = 0;
        } else {
            side_y += delta_y;
            map_y += step_y;
            side = 1;
        }
        tile = level.tile(map_x, map_y);
        if tile != map::EMPTY {
            break;
        }
    }

    let dist = if side == 0 { side_x - delta_x } else { side_y - delta_y }.max(MIN_DIST);
    let along = if side == 0 {
        py + fx::mul(dist, dir_y)
    } else {
        px + fx::mul(dist, dir_x)
    };
    Hit {
        dist,
        side,
        tile,
        wall_x: along & (ONE - 1),
    }
}

/// Coarse line of sight: samples the segment every quarter tile. Exactness
/// doesn't matter for gameplay; determinism does, and this is deterministic.
pub fn line_clear(level: &Level, x0: Fx, y0: Fx, x1: Fx, y1: Fx) -> bool {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let span = dx.unsigned_abs().max(dy.unsigned_abs()) as i32;
    let steps = (span / (ONE / 4)).max(1);
    for i in 1..steps {
        let x = x0 + (dx as i64 * i as i64 / steps as i64) as Fx;
        let y = y0 + (dy as i64 * i as i64 / steps as i64) as Fx;
        if level.tile(fx::to_int(x), fx::to_int(y)) != map::EMPTY {
            return false;
        }
    }
    true
}
