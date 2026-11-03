//! 16.16 fixed point. The map is 32 tiles wide, so world coordinates stay far
//! below i32 range; products go through i64.

pub type Fx = i32;

pub const FRAC_BITS: u32 = 16;
pub const ONE: Fx = 1 << FRAC_BITS;

#[inline]
pub const fn from_int(i: i32) -> Fx {
    i << FRAC_BITS
}

#[inline]
pub const fn to_int(a: Fx) -> i32 {
    a >> FRAC_BITS
}

#[inline]
pub fn mul(a: Fx, b: Fx) -> Fx {
    ((a as i64 * b as i64) >> FRAC_BITS) as Fx
}

/// `b` must be non-zero; callers guard it (division by zero aborts the guest).
#[inline]
pub fn div(a: Fx, b: Fx) -> Fx {
    (((a as i64) << FRAC_BITS) / b as i64) as Fx
}
