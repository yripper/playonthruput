#![cfg_attr(not(feature = "dev"), no_std)]

#[cfg(target_arch = "riscv32")]
extern crate alloc;

thruput_sdk::declare_mock_contract!();

mod implementation;

pub use implementation::{
    ACTIVE_EPOCHS, EPOCH_SECONDS, RunError, RunReceipt, RunVerified, Runs, RunsClient, SeedCreated,
    SeedInfo, current_seed_id, derive_run_seed, is_active,
};
