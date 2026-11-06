//! The arena server, with the chain as the server.
//!
//! Players never write game state. `join` and `send_inputs` only drop entries
//! into a retrocausal queue, which takes inserts from any number of
//! transactions in the same block. `crank` is the server tick: it drains the
//! queue, simulates up to `SIM_DELAY` tics behind the block's clock, writes
//! the state back and emits kills with a chain-rendered kill cam. Anyone may
//! crank; one crank per block wins (the queue allows one consumer per block)
//! and the rest are excluded.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use engine::arena::{self, Arena, CHUNK_LEN, KillLog, Overflow, STATE_CHUNKS, STATE_LEN, STORED_LEN};
use engine::render::FB_BYTES;
use engine::rle;
use thruput_sdk::storage::{Bytestring, Flaggable, Map, RCQ};
use thruput_sdk::*;

/// Every queue entry is exactly this long (the protocol's cap), so the crank
/// can consume entries into a stack buffer. The guest heap is a bump
/// allocator that never frees: one boxed entry per consume would exhaust it.
pub const ENTRY_LEN: usize = 256;
/// Most buttons in one `send_inputs` call: what's left of an entry after the
/// 38-byte header. That's 6 seconds of play.
pub const MAX_BATCH: usize = ENTRY_LEN - INPUT_HEADER_LEN;
/// Bounds a crank's queue work; anything left waits for the next block.
pub const MAX_ENTRIES_PER_CRANK: u32 = 400;
/// Kill-cam frames ride in one event each, which is capped at 1 KB with topics.
pub const MAX_FRAME_BYTES: usize = 900;
/// A `state_slice` plus its length prefix stays under the 1,024-byte cap on
/// return values (`kConfigParamMaxReturnBufferLen`, unstated on Futurenet).
pub const SLICE_LEN: usize = 960;

const ENTRY_JOIN: u8 = 0;
const ENTRY_INPUT: u8 = 1;
const INPUT_HEADER_LEN: usize = 1 + 32 + 4 + 1;

#[thruput_sdk::storage]
#[cfg_attr(feature = "dev", derive(Debug))]
pub struct ArenaServer {
    queue: RCQ<()>,
    /// The encoded `engine::arena::Arena`, zero-padded to `STORED_LEN` and
    /// split into `CHUNK_LEN` bytestrings.
    chunks: Map<u32, Bytestring>,
}

#[contract_exception]
#[derive(Clone, Copy)]
pub enum ArenaError {
    EmptyBatch = 1,
    BatchTooLong = 2,
}

#[contract_type]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "dev", derive(Debug))]
pub struct CrankReport {
    /// The next tic to simulate; everything before it is final.
    pub tic: u32,
    pub entries: u32,
    pub kills: u32,
}

/// A player killed another. `tic` is when the fatal shot was simulated.
#[derive(Event, Clone, Debug)]
pub struct Frag {
    #[topic]
    pub tic: u32,
    pub killer: ContractAddress,
    pub victim: ContractAddress,
    pub killer_slot: u32,
    pub victim_slot: u32,
}

/// The killer's view at the fatal shot, rendered on-chain and run-length
/// coded (`engine::rle`). Absent when the frame didn't compress small enough.
#[derive(Event, Clone, Debug)]
pub struct KillCam {
    #[topic]
    pub tic: u32,
    pub killer_slot: u32,
    pub victim_slot: u32,
    pub frame: Vec<u8>,
}

/// The arena (re)started: no state existed yet.
#[derive(Event, Clone, Debug)]
pub struct ArenaOpened {
    pub genesis_ns: u64,
    pub seed: u32,
}

fn seed_from(hash: &[u8; 32]) -> u32 {
    let word = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    if word == 0 { 0x9e37_79b9 } else { word }
}

fn address(id: &[u8; 32]) -> ContractAddress {
    ContractAddress::from(*id)
}

/// Applies one queue entry. Malformed entries are skipped, never thrown:
/// aborting here would leave the entry at the queue's head and jam the arena.
fn apply_entry(arena: &mut Arena, overflow: &mut Overflow, entry: &[u8; ENTRY_LEN]) {
    let mut id = [0u8; 32];
    id.copy_from_slice(&entry[1..33]);
    match entry[0] {
        ENTRY_JOIN => {
            arena.join(&id);
        }
        ENTRY_INPUT => {
            let start = u32::from_le_bytes([entry[33], entry[34], entry[35], entry[36]]);
            let len = (entry[37] as usize).min(MAX_BATCH);
            if let Some(slot) = arena.slot_of(&id) {
                arena.apply_inputs_overflowing(slot, start, &entry[INPUT_HEADER_LEN..INPUT_HEADER_LEN + len], overflow);
            }
        }
        _ => {}
    }
}

/// A heap arena built without a stack temporary: at ~9 KiB, every by-value
/// move of an `Arena` costs stack the contract doesn't have.
fn boxed_arena() -> Box<Arena> {
    // SAFETY: `Arena` is integers, bools and arrays of them (no references,
    // no niches beyond bool), and all-zero bytes are a valid value of each.
    unsafe { Box::<Arena>::new_zeroed().assume_init() }
}

/// The crank's input overflow (4 KiB), on the heap for the same reason.
fn boxed_overflow(arena: &Arena) -> Box<Overflow> {
    // SAFETY: `Overflow` is integers and arrays of them; all-zero is valid.
    let mut overflow = unsafe { Box::<Overflow>::new_zeroed().assume_init() };
    overflow.open(arena);
    overflow
}

impl ArenaServer {
    /// Loads the stored arena into `arena`; false if there is none yet.
    fn _load(&self, arena: &mut Arena, buf: &mut [u8]) -> bool {
        if !self.chunks.at(&0).exists() {
            return false;
        }
        for (i, out) in buf.chunks_mut(CHUNK_LEN).enumerate() {
            out.copy_from_slice(&self.chunks.at(&(i as u32)).get_sized::<CHUNK_LEN>());
        }
        arena.decode_into(buf).is_ok()
    }

    fn _store(&mut self, arena: &Arena, buf: &mut [u8]) {
        buf.fill(0);
        arena.encode(buf);
        for (i, chunk) in buf.chunks(CHUNK_LEN).enumerate() {
            self.chunks.at_mut(&(i as u32)).set(chunk);
        }
    }

    fn _open(&self, arena: &mut Arena) {
        let now: u64 = *get_block_timepoint();
        let source: [u8; 32] = *get_source_transaction_hash();
        let seed = seed_from(&source);
        log_event(&ArenaOpened { genesis_ns: now, seed });
        arena.reset(now, seed);
    }

    fn _drain(&mut self, arena: &mut Arena, overflow: &mut Overflow) -> u32 {
        let mut entries = 0;
        while entries < MAX_ENTRIES_PER_CRANK {
            match self.queue.try_consume_sized::<ENTRY_LEN>() {
                Ok(entry) => apply_entry(arena, overflow, &entry),
                // Only this contract inserts, always ENTRY_LEN bytes; discard
                // anything else rather than let it block the head.
                Err(LibraryExn::RetrocausalQueueEntryWrongSize { .. }) => {
                    let _ = self.queue.try_consume_raw();
                }
                // Empty queue or the per-transaction read limit: done for now.
                Err(_) => break,
            }
            entries += 1;
        }
        entries
    }
}

fn emit_kills(arena: &Arena, kills: &KillLog) {
    let mut fb = [0u8; FB_BYTES];
    let mut frame = [0u8; MAX_FRAME_BYTES];
    for kill in kills.iter() {
        let killer = &arena.players[kill.killer as usize];
        let victim = &arena.players[kill.victim as usize];
        log_event(&Frag {
            tic: kill.tic,
            killer: address(&killer.id),
            victim: address(&victim.id),
            killer_slot: kill.killer as u32,
            victim_slot: kill.victim as u32,
        });
        arena.render_kill(kill, &mut fb);
        if let Some(n) = rle::encode(&fb, &mut frame) {
            log_event(&KillCam {
                tic: kill.tic,
                killer_slot: kill.killer as u32,
                victim_slot: kill.victim as u32,
                frame: frame[..n].to_vec(),
            });
        }
    }
}

#[contract(client = ArenaServerClient, throws = ArenaError, emits(Frag, KillCam, ArenaOpened))]
impl ArenaServer {
    /// Asks for a slot. The next crank seats the caller if the arena has room;
    /// joining again while seated is a no-op.
    pub fn join(&mut self) {
        let mut entry = [0u8; ENTRY_LEN];
        entry[0] = ENTRY_JOIN;
        entry[1..33].copy_from_slice(&*get_caller());
        self.queue.insert_raw(&entry);
    }

    /// Queues the caller's buttons for tics `start_tic..start_tic + len`.
    /// Tics the arena has already simulated when the crank applies them are
    /// dropped, so send ahead: the arena runs `SIM_DELAY` tics behind the clock.
    pub fn send_inputs(&mut self, start_tic: u32, buttons: &[u8]) {
        ensure(!buttons.is_empty(), ArenaError::EmptyBatch);
        ensure(buttons.len() <= MAX_BATCH, ArenaError::BatchTooLong);
        let mut entry = [0u8; ENTRY_LEN];
        entry[0] = ENTRY_INPUT;
        entry[1..33].copy_from_slice(&*get_caller());
        entry[33..37].copy_from_slice(&start_tic.to_le_bytes());
        entry[37] = buttons.len() as u8;
        entry[INPUT_HEADER_LEN..INPUT_HEADER_LEN + buttons.len()].copy_from_slice(buttons);
        self.queue.insert_raw(&entry);
    }

    /// The server tick. Permissionless; browsers call it every block.
    pub fn crank(&mut self) -> CrankReport {
        let mut arena = boxed_arena();
        let mut buf = vec![0u8; STORED_LEN];
        if !self._load(&mut arena, &mut buf) {
            self._open(&mut arena);
        }
        // A late crank finds players' newest inputs past the ring; the
        // overflow holds them until the arena advances far enough.
        let mut overflow = boxed_overflow(&arena);
        let entries = self._drain(&mut arena, &mut overflow);
        let now = arena::tic_at(arena.genesis_ns, *get_block_timepoint());
        let mut kills = KillLog::new();
        arena.advance_with_overflow(now.saturating_sub(arena::SIM_DELAY), &mut kills, &mut overflow);
        emit_kills(&arena, &kills);
        self._store(&arena, &mut buf);
        CrankReport {
            tic: arena.tic,
            entries,
            kills: kills.count as u32 + kills.dropped,
        }
    }

    /// Storage-key hashes of the state chunks. Browsers resolve these once,
    /// then poll the chunks with the `getStorage` RPC, which returns raw
    /// bytes without the 1 KiB cap on contract return values.
    pub fn state_keys(&self) -> Vec<[u8; 32]> {
        (0..STATE_CHUNKS as u32)
            .map(|i| **self.chunks.at(&i).storage_key())
            .collect()
    }

    /// Up to `SLICE_LEN` bytes of the encoded arena from `offset`, never
    /// crossing a chunk boundary (empty before the first crank or past the
    /// end). Return values are capped at 1 KiB, so reading the whole state
    /// this way takes about ten calls; `state_keys` is the cheap path.
    pub fn state_slice(&self, offset: u32) -> Vec<u8> {
        let offset = offset as usize;
        if offset >= STATE_LEN {
            return Vec::new();
        }
        let Some(chunk) = self.chunks.at(&((offset / CHUNK_LEN) as u32)).try_get().none_if_missing() else {
            return Vec::new();
        };
        // Chunks are padded to CHUNK_LEN; never hand out the padding.
        let from = (offset % CHUNK_LEN).min(chunk.len());
        let end = (from + SLICE_LEN).min(chunk.len()).min(from + STATE_LEN - offset);
        chunk[from..end].to_vec()
    }

    pub fn state_len(&self) -> u32 {
        STATE_LEN as u32
    }
}
