//! Gas and memory measurements on the real RV32 executor. Run with
//! `thruput test -- --nocapture` to see the tables.

mod common;

use engine::game::scripted_input;
use engine::{FB_BYTES, Game, render};
use std::path::PathBuf;
use thruput_sdk::*;

fn thruput_artifact(file_name: &str) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"))
        .join("thruput")
        .join(file_name)
}

fn deployed() -> (TestEnvironmentHandle, SHA256Hash) {
    let contract = load_rv32_contract(thruput_artifact("probe.rv32a"));
    let env = TestEnvironment::no_cost();
    let hash = env.setup_create_contract(&contract);
    (env, hash)
}

/// Gas used by `call`, read from the transaction's remaining gas.
fn gas_of<T>(call: impl FnOnce() -> T) -> (T, u64) {
    let before = thruput_sdk::stats::gas_remaining();
    let value = call();
    (value, before - thruput_sdk::stats::gas_remaining())
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn test_deploy_rv32() {
    let (env, hash) = deployed();
    common::initializes(&env, &hash);
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_matches_native_engine() {
    let mut game = Game::new(1, 32);
    for t in 0..700 {
        game.tick(scripted_input(t));
    }
    let expected_state = game.state_hash();
    let mut fb = [0u8; FB_BYTES];
    render::render(&game, &mut fb);
    let expected_frame = render::frame_hash(&fb);

    let (env, hash) = deployed();
    common::with_probe(&env, &hash, "probe_rv32_parity", move |probe| {
        assert_eq!(probe.sim(700, 32), expected_state, "sim diverged on RV32");
        assert_eq!(probe.render(700, 32, 1), expected_frame, "frame diverged on RV32");
    });
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_sim_gas_table() {
    let (env, hash) = deployed();
    common::with_probe(&env, &hash, "probe_rv32_sim", |probe| {
        println!("| actors | tics | sim gas | gas/tic (minus baseline) |");
        println!("|---|---|---|---|");
        for actors in [0, 8, 16, 32, 64] {
            let tics = 1_050; // 30 s of play
            let (_, base) = gas_of(|| probe.noop(tics, actors));
            let (_, sim) = gas_of(|| probe.sim(tics, actors));
            println!(
                "| {actors} | {tics} | {sim} | {} |",
                (sim - base) / tics as u64
            );
        }
    });
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_render_gas_table() {
    let (env, hash) = deployed();
    common::with_probe(&env, &hash, "probe_rv32_render", |probe| {
        println!("| actors | frames | render gas | gas/frame (minus sim) |");
        println!("|---|---|---|---|");
        for actors in [0, 16, 64] {
            let frames = 20;
            let (_, sim) = gas_of(|| probe.sim(300 + frames, actors));
            let (_, total) = gas_of(|| probe.render(300, actors, frames));
            println!(
                "| {actors} | {frames} | {total} | {} |",
                total.saturating_sub(sim) / frames as u64
            );
        }
    });
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_memory_table() {
    let (env, hash) = deployed();
    common::with_probe(&env, &hash, "probe_rv32_memory", |probe| {
        let [_, heap, stack, heap_base, stack_top] = probe.memory_probe(300, 64);
        println!(
            "memory (64 actors, 1 frame): stack {stack} B, heap {heap} B, heap_base {heap_base}, stack_top {stack_top}, total {} of 65536",
            stack + heap
        );
    });
}
