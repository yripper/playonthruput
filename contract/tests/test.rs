//! Native (mock executor) tests for the runs contract.

mod common;

use common::*;
use engine::tape::Footer;
use engine::verify;
use runs::{RunError, RunVerified, SeedCreated, is_active};
use thruput_sdk::*;

fn alice() -> ContractAddress {
    ContractAddress::named("alice")
}

fn bob() -> ContractAddress {
    ContractAddress::named("bob")
}

#[test]
fn ensure_current_seed_creates_once_and_is_stable() {
    let world = setup_native();
    let first = world.ensure_seed();
    assert_eq!(first.seed_id, T0_SEED_ID);
    assert_ne!(first.seed, 0);
    assert_eq!(world.ensure_seed(), first);
}

#[test]
fn seed_creation_emits_an_event() {
    let world = setup_native();
    let runs = world.runs;
    let result = world.env.transact(|tx| {
        tx.call(&ContractAddress::named("cron"))
            .run(move || runs::RunsClient::new(&runs).ensure_current_seed())
    });
    result.assert_success();
    assert_eq!(result.filter_events::<SeedCreated>(&world.runs).len(), 1);
}

#[test]
fn players_get_different_run_seeds() {
    let world = setup_native();
    let info = world.ensure_seed();
    assert_ne!(world.run_seed(info.seed_id, &alice()), world.run_seed(info.seed_id, &bob()));
}

#[test]
fn honest_run_is_verified_and_recorded() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let inputs = bot_inputs(seed, 3_000);
    let expected = verify::play(seed, &inputs);
    assert!(expected.kills > 0, "bot should kill something");

    let (result, receipt) = world.submit_ok(&alice(), honest_tape(info.seed_id, seed, &inputs));
    assert_eq!(receipt.player, alice());
    assert_eq!(receipt.score, expected.score);
    assert_eq!(receipt.kills, expected.kills);
    assert_eq!(receipt.tics, 3_000);
    assert_eq!(receipt.previous_best, 0);
    assert_eq!(world.best(&alice(), info.seed_id), expected.score);

    let events = result.filter_events::<RunVerified>(&world.runs);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload().score, expected.score);
}

#[test]
fn better_run_replaces_the_best_and_reports_the_previous() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let long = bot_inputs(seed, 3_000);
    let long_kills = verify::play(seed, &long).kills;
    assert!(long_kills >= 2, "bot needs at least 2 kills, got {long_kills}");
    // The same game cut off right after its first kill scores strictly less.
    let first_kill = (1..long.len())
        .find(|&n| verify::play(seed, &long[..n]).kills == 1)
        .unwrap();
    let short = long[..first_kill].to_vec();
    let (short_score, long_score) = (verify::play(seed, &short).score, verify::play(seed, &long).score);
    assert!(short_score > 0 && long_score > short_score, "short {short_score}, long {long_score}");

    world.submit_ok(&alice(), honest_tape(info.seed_id, seed, &short));
    let (_, receipt) = world.submit_ok(&alice(), honest_tape(info.seed_id, seed, &long));
    assert_eq!(receipt.previous_best, short_score);
    assert_eq!(world.best(&alice(), info.seed_id), long_score);
}

#[test]
fn equal_or_worse_run_is_rejected() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let tape = honest_tape(info.seed_id, seed, &bot_inputs(seed, 3_000));
    world.submit_ok(&alice(), tape.clone());
    assert_eq!(world.submit_err(&alice(), tape), RunError::NotImproved);
}

#[test]
fn another_players_tape_is_rejected() {
    let world = setup_native();
    let info = world.ensure_seed();
    let alice_seed = world.run_seed(info.seed_id, &alice());
    let tape = honest_tape(info.seed_id, alice_seed, &bot_inputs(alice_seed, 3_000));
    assert_eq!(world.submit_err(&bob(), tape), RunError::SeedMismatch);
}

#[test]
fn inflated_score_is_rejected() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let inputs = bot_inputs(seed, 1_000);
    let real = verify::play(seed, &inputs);
    let tape = tape_with_footer(
        info.seed_id,
        seed,
        &inputs,
        Footer {
            score: real.score + 5_000,
            state_hash: real.state_hash,
        },
    );
    assert_eq!(world.submit_err(&alice(), tape), RunError::ScoreMismatch);
    assert_eq!(world.best(&alice(), info.seed_id), 0);
}

#[test]
fn tampered_state_hash_is_rejected() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let inputs = bot_inputs(seed, 1_000);
    let real = verify::play(seed, &inputs);
    let tape = tape_with_footer(
        info.seed_id,
        seed,
        &inputs,
        Footer {
            score: real.score,
            state_hash: real.state_hash ^ 1,
        },
    );
    assert_eq!(world.submit_err(&alice(), tape), RunError::StateMismatch);
}

#[test]
fn unknown_seed_is_rejected() {
    let world = setup_native();
    let tape = honest_tape(T0_SEED_ID, 1, &bot_inputs(1, 100));
    assert_eq!(world.submit_err(&alice(), tape), RunError::UnknownSeed);
}

#[test]
fn malformed_tape_is_rejected_before_storage_reads() {
    let world = setup_native();
    assert_eq!(world.submit_err(&alice(), vec![0u8; 5]), RunError::TapeTooShort);
    let mut bad = honest_tape(T0_SEED_ID, 1, &bot_inputs(1, 10));
    bad[0] = b'X';
    assert_eq!(world.submit_err(&alice(), bad), RunError::TapeBadMagic);
}

#[test]
fn expired_seed_is_rejected() {
    let world = setup_native();
    let info = world.ensure_seed();
    let seed = world.run_seed(info.seed_id, &alice());
    let tape = honest_tape(info.seed_id, seed, &bot_inputs(seed, 1_000));
    let later = T0_SECONDS + runs::EPOCH_SECONDS * runs::ACTIVE_EPOCHS as u64;
    world.set_time(later);
    assert_eq!(world.submit_err(&alice(), tape), RunError::SeedInactive);
}

#[test]
fn activity_window_bounds() {
    assert!(is_active(10, 10));
    assert!(is_active(10, 10 + runs::ACTIVE_EPOCHS - 1));
    assert!(!is_active(10, 10 + runs::ACTIVE_EPOCHS));
    assert!(!is_active(11, 10), "future epochs are not active");
}
