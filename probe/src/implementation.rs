//! Measurement contract: runs the shared engine on the RV32 VM so tests can
//! read real gas (1 per instruction) and guest memory use.

use engine::game::scripted_input;
use engine::{FB_BYTES, Game, render};
use thruput_sdk::*;

const SEED: u32 = 1;

#[derive(Debug)]
#[thruput_sdk::storage]
pub struct Contract {}

fn simulate(tics: u32, actors: u32) -> Game {
    let mut game = Game::new(SEED, actors as usize);
    for t in 0..tics {
        game.tick(scripted_input(t));
    }
    game
}

#[contract]
impl Contract {
    pub fn initialize(&mut self) {}

    /// Baseline: call overhead with the same arguments and no work.
    pub fn noop(&self, tics: u32, actors: u32) -> u32 {
        tics ^ actors
    }

    /// Runs `tics` scripted simulation tics with `actors` creatures.
    /// Returns the final state hash.
    pub fn sim(&self, tics: u32, actors: u32) -> u32 {
        simulate(tics, actors).state_hash()
    }

    /// Simulates `tics`, then renders `frames` 160×100 frames, advancing one
    /// tic between frames. Returns the last frame's hash.
    pub fn render(&self, tics: u32, actors: u32, frames: u32) -> u32 {
        let mut game = simulate(tics, actors);
        let mut fb = [0u8; FB_BYTES];
        for f in 0..frames {
            render::render(&game, &mut fb);
            game.tick(scripted_input(tics + f));
        }
        render::frame_hash(&fb)
    }

    /// Same as `render` with one frame, plus guest memory use. Returns
    /// `[frame_hash, heap_used, stack_used, heap_base, stack_top]` (zeros on host).
    pub fn memory_probe(&self, tics: u32, actors: u32) -> [u32; 5] {
        let painted = mem::paint_stack();
        let hash = self.render(tics, actors, 1);
        let stack_used = mem::stack_used(painted);
        let (heap_used, heap_base) = mem::heap_used();
        [hash, heap_used, stack_used, heap_base, mem::stack_top()]
    }
}

// Stack painting and bump-allocator probing, adapted from Kalien's gas probe
// (kalepail/kalien-thruput, asteroids-core/gas-probe).
#[cfg(target_arch = "riscv32")]
mod mem {
    const PAINT: u8 = 0xa5;
    /// Bytes left untouched just below the live stack pointer.
    const GUARD: usize = 256;

    unsafe extern "C" {
        static __stack_bottom: u8;
        static __stack_top: u8;
        static __heap_base: u8;
    }

    fn sp() -> usize {
        let sp: usize;
        unsafe { core::arch::asm!("mv {0}, sp", out(reg) sp) };
        sp
    }

    pub fn stack_top() -> u32 {
        (&raw const __stack_top) as u32
    }

    #[inline(never)]
    pub fn paint_stack() -> usize {
        // The stack reserve begins at address 0; skip the null guard.
        let bottom = core::cmp::max((&raw const __stack_bottom) as usize, 16);
        let top = sp() - GUARD;
        for addr in bottom..top {
            unsafe { core::ptr::write_volatile(addr as *mut u8, PAINT) };
        }
        bottom
    }

    #[inline(never)]
    pub fn stack_used(bottom: usize) -> u32 {
        let top = (&raw const __stack_top) as usize;
        let mut addr = bottom;
        while addr < top && unsafe { core::ptr::read_volatile(addr as *const u8) } == PAINT {
            addr += 1;
        }
        (top - addr) as u32
    }

    /// The bump allocator never frees, so the next allocation's offset is the peak.
    pub fn heap_used() -> (u32, u32) {
        let base = (&raw const __heap_base) as usize;
        let probe = unsafe { alloc::alloc::alloc(core::alloc::Layout::new::<u8>()) } as usize;
        ((probe - base) as u32, base as u32)
    }
}

#[cfg(not(target_arch = "riscv32"))]
mod mem {
    pub fn paint_stack() -> usize {
        0
    }
    pub fn stack_used(_bottom: usize) -> u32 {
        0
    }
    pub fn heap_used() -> (u32, u32) {
        (0, 0)
    }
    pub fn stack_top() -> u32 {
        0
    }
}
