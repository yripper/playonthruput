//! Run-length coding for 4-bit frames, so a chain-rendered kill cam fits in a
//! few 1 KB events. Ceiling and floor rows compress to a handful of bytes.
//!
//! Each run is one byte `color << 4 | n`: `n < 15` means a run of `n + 1`
//! pixels; `n == 15` means `16 + next byte` pixels (16..=271).

use crate::render::{FB_BYTES, H, W, pixel};

const PIXELS: usize = W * H;
const LONG: u8 = 15;
const MAX_RUN: usize = 16 + 255;
/// Worst case: every pixel differs from its neighbour.
pub const MAX_ENCODED: usize = PIXELS;

/// Encodes `fb` into `out`, returning the encoded length, or `None` if it
/// doesn't fit (callers size `out` to what they can ship).
pub fn encode(fb: &[u8; FB_BYTES], out: &mut [u8]) -> Option<usize> {
    let mut len = 0;
    let mut i = 0;
    while i < PIXELS {
        let color = pixel(fb, i % W, i / W);
        let mut run = 1;
        while i + run < PIXELS && run < MAX_RUN && pixel(fb, (i + run) % W, (i + run) / W) == color {
            run += 1;
        }
        if run <= LONG as usize {
            *out.get_mut(len)? = color << 4 | (run - 1) as u8;
            len += 1;
        } else {
            *out.get_mut(len)? = color << 4 | LONG;
            *out.get_mut(len + 1)? = (run - 16) as u8;
            len += 2;
        }
        i += run;
    }
    Some(len)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RleError {
    Truncated,
    Overflow,
    Underflow,
}

/// Decodes a frame written by `encode`.
pub fn decode(bytes: &[u8], fb: &mut [u8; FB_BYTES]) -> Result<(), RleError> {
    let mut i = 0;
    let mut at = 0;
    while at < bytes.len() {
        let color = bytes[at] >> 4;
        let n = bytes[at] & 0x0f;
        let run = if n == LONG {
            let extra = *bytes.get(at + 1).ok_or(RleError::Truncated)?;
            at += 2;
            16 + extra as usize
        } else {
            at += 1;
            n as usize + 1
        };
        if i + run > PIXELS {
            return Err(RleError::Overflow);
        }
        for p in i..i + run {
            let byte = &mut fb[p / 2];
            *byte = if p % 2 == 0 { (*byte & 0xf0) | color } else { (*byte & 0x0f) | (color << 4) };
        }
        i += run;
    }
    if i == PIXELS { Ok(()) } else { Err(RleError::Underflow) }
}
