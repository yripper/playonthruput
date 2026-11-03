//! Verified runs: replays a whole play-through on-chain with the shared engine
//! and keeps each player's best score per seed epoch.
//!
//! Seeds follow Kalien's scheme (kalepail/kalien-thruput): one seed per
//! 10-minute epoch, derived from block fields the caller can't predict. Each
//! player then gets their own run seed from it, so a tape recorded by one
//! player can't be resubmitted by another.

use engine::tape;
use engine::verify::{self, VerifyError};
use thruput_sdk::crypto::sha256;
use thruput_sdk::storage::{Map, Value};
use thruput_sdk::*;

pub const EPOCH_SECONDS: u64 = 600;
/// A seed stays playable for the current epoch and the previous 143 (24 h).
pub const ACTIVE_EPOCHS: u32 = 144;
/// The engine's RNG can't take 0.
pub const ZERO_SEED_FALLBACK: u32 = 0xdead_beef;

const NANOS_PER_SECOND: u64 = 1_000_000_000;
const RUN_SEED_DOMAIN: &[u8; 16] = b"doom-lite-run-v1";

#[thruput_sdk::storage]
#[cfg_attr(feature = "dev", derive(Debug))]
pub struct Runs {
    seeds: Map<u32, Value<u32>>,
    best: Map<ContractAddress, Map<u32, Value<u32>>>,
}

#[contract_type]
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "dev", derive(Debug))]
pub struct SeedInfo {
    pub seed_id: u32,
    pub seed: u32,
}

#[contract_type]
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(feature = "dev", derive(Debug))]
pub struct RunReceipt {
    pub player: ContractAddress,
    pub seed_id: u32,
    pub tics: u32,
    pub kills: u32,
    pub cleared_at: u32,
    pub score: u32,
    pub previous_best: u32,
    pub tape_hash: [u8; 32],
}

#[contract_exception]
#[derive(Clone, Copy)]
pub enum RunError {
    UnknownSeed = 1,
    SeedInactive = 2,
    SeedMismatch = 3,
    NotImproved = 4,
    TapeTooShort = 10,
    TapeBadMagic = 11,
    TapeBadVersion = 12,
    TapeReservedNonZero = 13,
    TapeTicsOutOfRange = 14,
    TapeLengthMismatch = 15,
    UnknownLevel = 16,
    ScoreMismatch = 20,
    StateMismatch = 21,
}

impl From<tape::TapeError> for RunError {
    fn from(err: tape::TapeError) -> Self {
        match err {
            tape::TapeError::TooShort => RunError::TapeTooShort,
            tape::TapeError::BadMagic => RunError::TapeBadMagic,
            tape::TapeError::BadVersion => RunError::TapeBadVersion,
            tape::TapeError::ReservedNonZero => RunError::TapeReservedNonZero,
            tape::TapeError::TicsOutOfRange => RunError::TapeTicsOutOfRange,
            tape::TapeError::LengthMismatch => RunError::TapeLengthMismatch,
        }
    }
}

impl From<VerifyError> for RunError {
    fn from(err: VerifyError) -> Self {
        match err {
            VerifyError::Tape(e) => e.into(),
            VerifyError::UnknownLevel => RunError::UnknownLevel,
            VerifyError::SeedMismatch => RunError::SeedMismatch,
            VerifyError::ScoreMismatch => RunError::ScoreMismatch,
            VerifyError::StateMismatch => RunError::StateMismatch,
        }
    }
}

/// A run beat the player's previous best for its epoch.
#[derive(Event, Clone, Debug)]
pub struct RunVerified {
    #[topic]
    pub player: ContractAddress,
    #[topic]
    pub seed_id: u32,
    pub tics: u32,
    pub kills: u32,
    pub cleared_at: u32,
    pub score: u32,
    pub previous_best: u32,
    pub tape_hash: [u8; 32],
}

#[derive(Event, Clone, Debug)]
pub struct SeedCreated {
    #[topic]
    pub seed_id: u32,
    pub seed: u32,
}

/// The epoch containing the current block.
pub fn current_seed_id() -> u32 {
    let nanos: u64 = *get_block_timepoint();
    (nanos / NANOS_PER_SECOND / EPOCH_SECONDS) as u32
}

pub fn is_active(seed_id: u32, current: u32) -> bool {
    seed_id <= current && current - seed_id < ACTIVE_EPOCHS
}

fn first_word(hash: &[u8; 32]) -> u32 {
    let word = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    if word == 0 { ZERO_SEED_FALLBACK } else { word }
}

/// The caller can grind their own transaction hash, but not the block height
/// and nanosecond timepoint the proposer stamps when the transaction lands.
fn derive_epoch_seed(source_tx: &[u8; 32], seed_id: u32, height: i64, timepoint_ns: u64) -> u32 {
    let mut preimage = [0u8; 52];
    preimage[..32].copy_from_slice(source_tx);
    preimage[32..36].copy_from_slice(&seed_id.to_le_bytes());
    preimage[36..44].copy_from_slice(&height.to_le_bytes());
    preimage[44..].copy_from_slice(&timepoint_ns.to_le_bytes());
    first_word(&sha256(&preimage))
}

/// `sha256("doom-lite-run-v1" || epoch_seed LE || player)`, first word.
pub fn derive_run_seed(epoch_seed: u32, player: &ContractAddress) -> u32 {
    let mut preimage = [0u8; 52];
    preimage[..16].copy_from_slice(RUN_SEED_DOMAIN);
    preimage[16..20].copy_from_slice(&epoch_seed.to_le_bytes());
    preimage[20..].copy_from_slice(&**player);
    first_word(&sha256(&preimage))
}

impl Runs {
    fn _stored_seed(&self, seed_id: u32) -> Option<u32> {
        self.seeds.at(&seed_id).try_get().none_if_missing()
    }

    fn _best(&self, player: &ContractAddress, seed_id: u32) -> u32 {
        self.best
            .at(player)
            .at(&seed_id)
            .try_get()
            .none_if_missing()
            .unwrap_or(0)
    }
}

#[contract(client = RunsClient, throws = RunError, emits(RunVerified, SeedCreated))]
impl Runs {
    /// Creates the current epoch's seed if missing and returns it. Anyone may
    /// call it; the first player of an epoch usually does.
    pub fn ensure_current_seed(&mut self) -> SeedInfo {
        let seed_id = current_seed_id();
        if let Some(seed) = self._stored_seed(seed_id) {
            return SeedInfo { seed_id, seed };
        }
        let source: [u8; 32] = *get_source_transaction_hash();
        let seed = derive_epoch_seed(&source, seed_id, *get_block_height(), *get_block_timepoint());
        self.seeds.at_mut(&seed_id).set(&seed);
        log_event(&SeedCreated { seed_id, seed });
        SeedInfo { seed_id, seed }
    }

    /// The current epoch's seed, if it has been created.
    pub fn current_seed(&self) -> Option<SeedInfo> {
        let seed_id = current_seed_id();
        self._stored_seed(seed_id).map(|seed| SeedInfo { seed_id, seed })
    }

    /// The seed `player` must record a run with for `seed_id`.
    pub fn run_seed(&self, seed_id: u32, player: &ContractAddress) -> u32 {
        let epoch_seed = self._stored_seed(seed_id).unwrap_or_else(|| throw(RunError::UnknownSeed));
        derive_run_seed(epoch_seed, player)
    }

    /// Verifies a run by replaying its tape and records it if it beats the
    /// caller's best for the epoch. Cheap checks run first so bad tapes are
    /// rejected before the replay.
    pub fn submit_run(&mut self, tape_bytes: &[u8]) -> RunReceipt {
        let player = get_caller();
        let parsed = tape::parse(tape_bytes).unwrap_or_else(|e| throw(RunError::from(e)));
        let seed_id = parsed.header.seed_id;

        let epoch_seed = self._stored_seed(seed_id).unwrap_or_else(|| throw(RunError::UnknownSeed));
        ensure(is_active(seed_id, current_seed_id()), RunError::SeedInactive);
        let expected_seed = derive_run_seed(epoch_seed, &player);

        let previous_best = self._best(&player, seed_id);
        ensure(parsed.footer.score > previous_best, RunError::NotImproved);

        let outcome = verify::verify(tape_bytes, expected_seed).unwrap_or_else(|e| throw(RunError::from(e)));

        self.best.at_mut(&player).at_mut(&seed_id).set(&outcome.score);
        let tape_hash: [u8; 32] = *sha256(tape_bytes);
        log_event(&RunVerified {
            player,
            seed_id,
            tics: outcome.tics,
            kills: outcome.kills,
            cleared_at: outcome.cleared_at,
            score: outcome.score,
            previous_best,
            tape_hash,
        });
        RunReceipt {
            player,
            seed_id,
            tics: outcome.tics,
            kills: outcome.kills,
            cleared_at: outcome.cleared_at,
            score: outcome.score,
            previous_best,
            tape_hash,
        }
    }

    /// `player`'s best score for `seed_id` (0 if none).
    pub fn best_score(&self, player: &ContractAddress, seed_id: u32) -> u32 {
        self._best(player, seed_id)
    }

    /// The epoch containing the current block.
    pub fn current_seed_id(&self) -> u32 {
        current_seed_id()
    }
}
