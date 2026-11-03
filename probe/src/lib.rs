#![cfg_attr(not(feature = "dev"), no_std)]

#[cfg(target_arch = "riscv32")]
extern crate alloc;

thruput_sdk::declare_mock_contract!();

mod implementation;

pub use implementation::Contract;
pub use implementation::ContractClient;
