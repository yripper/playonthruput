//! The arena engine for browsers, as plain `extern "C"` exports over static
//! buffers (no wasm-bindgen): JS writes bytes into a buffer, calls a function,
//! and reads the result out of another buffer.
//!
//! The browser runs the exact simulation the chain runs. It keeps two arenas:
//! `confirmed`, decoded from chain storage, and `predicted`, which replays the
//! player's own not-yet-confirmed inputs on a copy of it up to the present.

#![no_std]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use engine::arena::{self, Arena, KillLog, MAX_PLAYERS, PLAYER_COLORS, STORED_LEN};
use engine::map::ARENA;
use engine::render::{self, FB_BYTES, H, PALETTE, Sprite, View, W};
use engine::rle;

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

/// Mutable global state. Wasm here is single-threaded and exports never
/// re-enter each other, so handing out `&mut` from a shared static is sound.
struct Global<T>(UnsafeCell<T>);

unsafe impl<T> Sync for Global<T> {}

impl<T> Global<T> {
    #[allow(clippy::mut_from_ref)]
    fn get(&self) -> &mut T {
        unsafe { &mut *self.0.get() }
    }
}

/// All-zero bytes are a valid `Arena` (integers, bools, arrays of them).
const fn zeroed<T>() -> Global<T> {
    Global(UnsafeCell::new(unsafe { MaybeUninit::zeroed().assume_init() }))
}

const RGBA_LEN: usize = W * H * 4;
/// Own inputs a prediction can replay: SIM_DELAY plus a few seconds of lag.
pub const INPUTS_CAP: usize = 1024;
pub const KILLCAM_CAP: usize = 2048;
/// Fields per player in `players`: active, alive, x, y, angle, hp, frags,
/// deaths, respawn_at, color.
pub const PLAYER_FIELDS: usize = 10;
/// Prediction never runs more than this many tics in one frame.
const MAX_PREDICT_STEPS: u32 = 600;

static CONFIRMED: Global<Arena> = zeroed();
static PREDICTED: Global<Arena> = zeroed();
static STATE: Global<[u8; STORED_LEN]> = zeroed();
static INPUTS: Global<[u8; INPUTS_CAP]> = zeroed();
static ID: Global<[u8; 32]> = zeroed();
static FB: Global<[u8; FB_BYTES]> = zeroed();
static RGBA: Global<[u8; RGBA_LEN]> = zeroed();
static KILLCAM: Global<[u8; KILLCAM_CAP]> = zeroed();
static KILLCAM_RGBA: Global<[u8; RGBA_LEN]> = zeroed();
static PLAYERS: Global<[i32; MAX_PLAYERS * PLAYER_FIELDS]> = zeroed();

#[unsafe(no_mangle)]
pub extern "C" fn state_ptr() -> *mut u8 {
    STATE.get().as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn state_len() -> u32 {
    STORED_LEN as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn inputs_ptr() -> *mut u8 {
    INPUTS.get().as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn id_ptr() -> *mut u8 {
    ID.get().as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn rgba_ptr() -> *const u8 {
    RGBA.get().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn killcam_ptr() -> *mut u8 {
    KILLCAM.get().as_mut_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn killcam_rgba_ptr() -> *const u8 {
    KILLCAM_RGBA.get().as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn players_ptr() -> *const i32 {
    PLAYERS.get().as_ptr()
}

/// Tunables the client must agree with the chain on.
#[unsafe(no_mangle)]
pub extern "C" fn constant(which: u32) -> u32 {
    match which {
        0 => arena::SIM_DELAY,
        1 => arena::REWIND,
        2 => arena::TICS_PER_SECOND as u32,
        3 => MAX_PLAYERS as u32,
        4 => arena::RESPAWN_TICS,
        5 => arena::MAX_HP as u32,
        6 => INPUTS_CAP as u32,
        7 => PLAYER_FIELDS as u32,
        _ => 0,
    }
}

/// Decodes `STATE` into the confirmed arena. 0 on success.
#[unsafe(no_mangle)]
pub extern "C" fn load_state() -> i32 {
    match CONFIRMED.get().decode_into(STATE.get()) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn confirmed_tic() -> u32 {
    CONFIRMED.get().tic
}

#[unsafe(no_mangle)]
pub extern "C" fn predicted_tic() -> u32 {
    PREDICTED.get().tic
}

/// Genesis time in ms (fits an f64 exactly for millennia).
#[unsafe(no_mangle)]
pub extern "C" fn genesis_ms() -> f64 {
    (CONFIRMED.get().genesis_ns / 1_000_000) as f64
}

/// The confirmed slot of the address in `ID`, or -1.
#[unsafe(no_mangle)]
pub extern "C" fn slot_of() -> i32 {
    CONFIRMED.get().slot_of(ID.get()).map_or(-1, |s| s as i32)
}

/// Copies the confirmed arena, replays `slot`'s inputs `INPUTS[..len]` from
/// tic `start`, and simulates up to `target`. Other players' inputs are the
/// ones the chain has already queued for them.
#[unsafe(no_mangle)]
pub extern "C" fn predict(slot: i32, start: u32, len: u32, target: u32) {
    let predicted = PREDICTED.get();
    predicted.clone_from(CONFIRMED.get());
    if slot >= 0 && (slot as usize) < MAX_PLAYERS {
        let len = (len as usize).min(INPUTS_CAP);
        predicted.apply_inputs(slot as usize, start, &INPUTS.get()[..len]);
    }
    let mut kills = KillLog::new();
    let end = target.min(predicted.tic + MAX_PREDICT_STEPS);
    while predicted.tic < end {
        predicted.step(&mut kills);
    }
}

/// Fills `PLAYERS` from the predicted (`which` = 1) or confirmed arena.
#[unsafe(no_mangle)]
pub extern "C" fn snapshot(which: u32) {
    let source = if which == 1 { PREDICTED.get() } else { CONFIRMED.get() };
    let out = PLAYERS.get();
    for (s, p) in source.players.iter().enumerate() {
        let row = &mut out[s * PLAYER_FIELDS..(s + 1) * PLAYER_FIELDS];
        row[0] = p.active as i32;
        row[1] = p.alive as i32;
        row[2] = p.x;
        row[3] = p.y;
        row[4] = p.angle as i32;
        row[5] = p.hp as i32;
        row[6] = p.frags as i32;
        row[7] = p.deaths as i32;
        row[8] = p.respawn_at as i32;
        row[9] = PLAYER_COLORS[s] as i32;
    }
}

/// Renders into `RGBA`. Playing (`spectate` = 0): the predicted present from
/// `slot`'s eyes, others where the chain will check shots against them.
/// Spectating: the confirmed arena from `slot`'s eyes, everyone exact.
#[unsafe(no_mangle)]
pub extern "C" fn render(slot: u32, spectate: u32) {
    let slot = slot as usize % MAX_PLAYERS;
    let mut sprites = [Sprite { x: 0, y: 0, color: 0 }; MAX_PLAYERS];
    let (source, count) = if spectate == 0 {
        let a = PREDICTED.get();
        let n = a.sprites_seen_by(Some(slot), a.tic.saturating_sub(1), &mut sprites);
        (&*a, n)
    } else {
        let a = CONFIRMED.get();
        let mut n = 0;
        for (o, p) in a.players.iter().enumerate() {
            if o != slot && p.active && p.alive {
                sprites[n] = Sprite { x: p.x, y: p.y, color: PLAYER_COLORS[o] };
                n += 1;
            }
        }
        (&*a, n)
    };
    let p = &source.players[slot];
    let view = View { x: p.x, y: p.y, angle: p.angle };
    render::render_view(&ARENA, &view, &sprites[..count], FB.get());
    to_rgba(FB.get(), RGBA.get());
}

/// Decodes a `KillCam` event frame from `KILLCAM[..len]` into `KILLCAM_RGBA`.
#[unsafe(no_mangle)]
pub extern "C" fn decode_killcam(len: u32) -> i32 {
    let len = (len as usize).min(KILLCAM_CAP);
    let mut fb = [0u8; FB_BYTES];
    match rle::decode(&KILLCAM.get()[..len], &mut fb) {
        Ok(()) => {
            to_rgba(&fb, KILLCAM_RGBA.get());
            0
        }
        Err(_) => -1,
    }
}

fn to_rgba(fb: &[u8; FB_BYTES], out: &mut [u8; RGBA_LEN]) {
    for y in 0..H {
        for x in 0..W {
            let [r, g, b] = PALETTE[render::pixel(fb, x, y) as usize];
            let i = (y * W + x) * 4;
            out[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }
}
