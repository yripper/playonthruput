//! The same contract on the RV32 executor: parity with native, a max-length
//! tape (calldata + memory limits), and gas. `thruput test -- --nocapture`
//! prints the gas lines.

mod common;

use common::*;
use engine::tape::MAX_TICS;
use engine::verify;
use runs::{RunError, RunReceipt, RunsClient};
use thruput_sdk::*;

fn alice() -> ContractAddress {
    ContractAddress::named("alice")
}

/// Gas of one `submit_run`, measured inside the transaction.
fn submit_gas(world: &World, player: &ContractAddress, tape: Vec<u8>) -> (RunReceipt, u64) {
    let runs = world.runs;
    world.call(player, move |_| {
        let before = thruput_sdk::stats::gas_remaining();
        let receipt = RunsClient::new(&runs).submit_run(&tape);
        (receipt, before - thruput_sdk::stats::gas_remaining())
    })
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_verifies_an_honest_run_like_native() {
    let world = setup_rv32();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let inputs = bot_inputs(seed, 3_000);
    let expected = verify::play(seed, &inputs);
    let (_, receipt) = world.submit_ok(&alice(), honest_tape(info.seed_id, seed, &inputs));
    assert_eq!(receipt.score, expected.score);
    assert_eq!(receipt.kills, expected.kills);
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_rejects_an_inflated_score() {
    let world = setup_rv32();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let inputs = bot_inputs(seed, 1_000);
    let real = verify::play(seed, &inputs);
    let tape = tape_with_footer(
        info.seed_id,
        seed,
        &inputs,
        engine::tape::Footer {
            score: real.score + 1,
            state_hash: real.state_hash,
        },
    );
    assert_eq!(world.submit_err(&alice(), tape), RunError::ScoreMismatch);
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_gas_per_run_length() {
    let world = setup_rv32();
    let info = world.ensure_seed();
    println!("| tics | tape bytes | kills | gas | gas/tic |");
    println!("|---|---|---|---|---|");
    // A fresh player per length, so every run is that player's first best.
    for tics in [1_050, 5_250, 10_500, MAX_TICS] {
        let player = ContractAddress::named(&format!("player_{tics}"));
        let seed = world.run_seed(info.seed_id, &player);
        let inputs = bot_inputs(seed, tics);
        if verify::play(seed, &inputs).score == 0 {
            println!("| {tics} | - | 0 | (zero score, not submittable) | - |");
            continue;
        }
        let tape = honest_tape(info.seed_id, seed, &inputs);
        let bytes = tape.len();
        let (receipt, gas) = submit_gas(&world, &player, tape);
        println!("| {tics} | {bytes} | {} | {gas} | {} |", receipt.kills, gas / tics as u64);
    }
}
