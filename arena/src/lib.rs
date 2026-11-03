#![cfg_attr(not(feature = "dev"), no_std)]

thruput_sdk::declare_mock_contract!();

mod implementation;

pub use implementation::{
    ArenaError, ArenaOpened, ArenaServer, ArenaServerClient, CrankReport, Frag, KillCam, MAX_BATCH,
    MAX_ENTRIES_PER_CRANK, MAX_FRAME_BYTES, SLICE_LEN,
};
