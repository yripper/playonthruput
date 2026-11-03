//! Angles are integers, 1024 per turn. Sine comes from a quarter-wave table
//! computed at compile time, so no floating point exists at runtime.

use crate::fx::Fx;

pub const ANGLES: u32 = 1024;
const QUARTER: usize = (ANGLES / 4) as usize;

const fn taylor_sin(x: f64) -> f64 {
    let x2 = x * x;
    let mut term = x;
    let mut sum = x;
    let mut n = 1;
    while n < 12 {
        term = -term * x2 / ((2 * n) as f64 * (2 * n + 1) as f64);
        sum += term;
        n += 1;
    }
    sum
}

static SIN_TABLE: [Fx; QUARTER + 1] = {
    let mut table = [0; QUARTER + 1];
    let mut i = 0;
    while i <= QUARTER {
        let x = i as f64 * core::f64::consts::FRAC_PI_2 / QUARTER as f64;
        table[i] = (taylor_sin(x) * 65536.0 + 0.5) as Fx;
        i += 1;
    }
    table
};

pub fn sin(angle: u32) -> Fx {
    let a = (angle % ANGLES) as usize;
    let r = a % QUARTER;
    match a / QUARTER {
        0 => SIN_TABLE[r],
        1 => SIN_TABLE[QUARTER - r],
        2 => -SIN_TABLE[r],
        _ => -SIN_TABLE[QUARTER - r],
    }
}

pub fn cos(angle: u32) -> Fx {
    sin(angle.wrapping_add(QUARTER as u32))
}
