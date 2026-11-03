//! The arena on the RV32 executor: parity with native and crank gas.
//! `thruput test -- --nocapture` prints the gas lines.

mod common;

use arena::ArenaServerClient;
use common::*;
use engine::arena::{MAX_PLAYERS, MAX_STEPS};
use engine::input::{FIRE, FORWARD, TURN_LEFT, TURN_RIGHT};
use thruput_sdk::*;

/// Gas of one crank, measured inside the transaction.
fn crank_gas(world: &World) -> u64 {
    let arena = world.arena;
    world.call(&ContractAddress::named("cranker"), move |_| {
        let before = thruput_sdk::stats::gas_remaining();
        ArenaServerClient::new(&arena).crank();
        before - thruput_sdk::stats::gas_remaining()
    })
}

/// Peak stack of one crank, from the executor's RV32 stack trace.
fn crank_stack(world: &World) -> u64 {
    let arena = world.arena;
    let result = world.env.transact(|tx| {
        tx.call(&ContractAddress::named("cranker")).run(move || ArenaServerClient::new(&arena).crank())
    });
    result.assert_success();
    world.next_block();
    let trace = format!("{result:?}");
    let field = |name: &str| -> u64 {
        let at = trace.find(name).expect("rv32 stack usage in trace") + name.len();
        trace[at..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
    };
    field("sp_initial: ") - field("sp_min: ")
}

/// Everyone runs and guns: turning, walking, holding fire.
fn mayhem(world: &World, players: u8, start: u32) {
    for n in 0..players {
        let pattern = [FORWARD | FIRE | TURN_RIGHT, FORWARD | FIRE, FIRE | TURN_LEFT, FORWARD];
        let buttons: Vec<u8> = (0..arena::MAX_BATCH).map(|i| pattern[(i / 20 + n as usize) % 4]).collect();
        world.send(&player(n), start, &buttons);
    }
}

fn full_room(world: &World, players: u8) {
    world.crank();
    for n in 0..players {
        world.join(&player(n));
    }
    world.next_block();
    world.play(8);
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_matches_native_state() {
    let native = setup_native();
    let rv32 = setup_rv32();
    for world in [&native, &rv32] {
        full_room(world, 6);
        let start = world.state().tic + 5;
        mayhem(world, 6, start);
        world.next_block();
        world.play(6);
    }
    assert_eq!(state_hash(&native.state()), state_hash(&rv32.state()));
}

#[test]
#[ignore = "requires RV32 build — run via `thruput test`"]
fn rv32_crank_gas() {
    let world = setup_rv32();
    println!("gas: first crank (opens arena)    {:>11}", crank_gas(&world));
    println!("gas: idle crank, empty arena      {:>11}", crank_gas(&world));

    full_room(&world, MAX_PLAYERS as u8);
    println!("gas: 16 players standing, 1 block {:>11}", crank_gas(&world));

    let start = world.state().tic + 5;
    mayhem(&world, MAX_PLAYERS as u8, start);
    world.next_block();
    let mut worst = 0;
    let mut deepest = 0;
    for _ in 0..4 {
        worst = worst.max(crank_gas(&world));
        deepest = deepest.max(crank_stack(&world));
    }
    println!("stack: deepest crank              {:>11} of 20480 bytes", deepest);
    println!("gas: 16 players mayhem, worst     {:>11}", worst);

    // Nobody cranked for a while: the catch-up is capped at MAX_STEPS tics.
    world.wait_ns(60_000_000_000);
    let catch_up = crank_gas(&world);
    println!("gas: catch-up crank ({MAX_STEPS} tics)     {:>11}", catch_up);
    assert!(catch_up < 1_000_000_000, "a crank must fit maxTxGas");
}
