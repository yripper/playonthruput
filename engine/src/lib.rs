//! Deterministic game core. Integer math only: the same build must produce
//! bit-identical results natively, in wasm, and on Thruput's RV32IM VM
//! (which has no floating-point unit).

#![no_std]

pub mod arena;
pub mod fx;
pub mod game;
pub mod map;
pub mod ray;
pub mod render;
pub mod rle;
pub mod rng;
pub mod tape;
pub mod trig;
pub mod verify;

pub use game::{Game, input};
pub use render::{FB_BYTES, H, PALETTE, Sprite, View, W};

/// FNV-1a over bytes. Cheap checksum used for state and frame hashes.
pub fn fnv1a(hash: u32, bytes: &[u8]) -> u32 {
    let mut h = hash;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub const FNV_OFFSET: u32 = 0x811c_9dc5;
