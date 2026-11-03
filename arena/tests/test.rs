//! Native (mock executor) tests for the arena contract.

mod common;

use arena::{ArenaError, ArenaOpened, Frag, KillCam, MAX_BATCH, SLICE_LEN};
use common::*;
use engine::arena::{Arena, CHUNK_LEN, MAX_PLAYERS, SIM_DELAY, STATE_CHUNKS, STATE_LEN, tic_at};
use engine::input::{FIRE, FORWARD, TURN_LEFT, TURN_RIGHT};
use thruput_sdk::ContractAddress;
use engine::render::{FB_BYTES, frame_hash};

fn copy(arena: &Arena) -> Arena {
    let mut bytes = vec![0u8; STATE_LEN];
    arena.encode(&mut bytes);
    Arena::decode(&bytes).unwrap()
}

/// Two seated, spawned players, long enough that spawn protection is over.
fn two_players() -> World {
    let world = setup_native();
    world.crank();
    world.join(&player(1));
    world.join(&player(2));
    world.next_block();
    world.play(8);
    world
}

#[test]
fn first_crank_opens_the_arena() {
    let world = setup_native();
    assert_eq!(
        world.view(|a| a.state_slice(0)),
        Vec::<u8>::new(),
        "no state before the first crank"
    );
    let (result, report) = world.crank();
    assert_eq!(report.tic, 0);
    let opened = result.filter_events::<ArenaOpened>(&world.arena);
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].payload().genesis_ns, T0_NS);

    let (second, _) = world.crank();
    assert!(second.filter_events::<ArenaOpened>(&world.arena).is_empty());
}

#[test]
fn crank_simulates_sim_delay_behind_the_clock() {
    let world = setup_native();
    world.crank();
    let genesis = world.state().genesis_ns;
    world.wait_ns(10_000_000_000);
    let (_, report) = world.crank();
    assert_eq!(report.tic, tic_at(genesis, world.now_ns() - BLOCK_NS) - SIM_DELAY);
    assert_eq!(world.state().tic, report.tic);
}

#[test]
fn join_seats_the_player_on_the_next_crank() {
    let world = setup_native();
    world.crank();
    world.join(&player(1));
    world.next_block();
    let (_, report) = world.crank();
    assert_eq!(report.entries, 1);
    let state = world.state();
    assert!(state.slot_of(&player(1)).is_some());
    assert!(state.slot_of(&player(2)).is_none());
}

#[test]
fn many_players_join_in_the_same_block() {
    let world = setup_native();
    world.crank();
    for n in 0..MAX_PLAYERS as u8 + 2 {
        world.join(&player(n));
    }
    world.next_block();
    let (_, report) = world.crank();
    assert_eq!(report.entries, MAX_PLAYERS as u32 + 2);
    let state = world.state();
    let seated = (0..MAX_PLAYERS as u8 + 2).filter(|&n| state.slot_of(&player(n)).is_some()).count();
    assert_eq!(seated, MAX_PLAYERS, "the arena seats 16; the rest are turned away");
}

#[test]
fn inputs_from_unseated_players_are_ignored() {
    let world = setup_native();
    world.crank();
    world.send(&player(1), 200, &[FORWARD; 10]);
    world.next_block();
    let (_, report) = world.crank();
    assert_eq!(report.entries, 1);
    assert!(world.state().players.iter().all(|p| !p.active));
}

#[test]
fn send_inputs_validates_the_batch() {
    let world = setup_native();
    let (result, _) = world.transact(&player(1), |a| a.send_inputs(0, &[]));
    assert_eq!(result.assert_abort_as::<ArenaError>(), ArenaError::EmptyBatch);
    let (result, _) = world.transact(&player(1), |a| a.send_inputs(0, &[0; MAX_BATCH + 1]));
    assert_eq!(result.assert_abort_as::<ArenaError>(), ArenaError::BatchTooLong);
    world.transact(&player(1), |a| a.send_inputs(0, &[0; MAX_BATCH])).0.assert_success();
}

#[test]
fn state_keys_are_distinct_per_chunk() {
    let world = setup_native();
    let keys = world.view(|a| a.state_keys());
    assert_eq!(keys.len(), STATE_CHUNKS);
    assert!(keys.iter().all(|k| *k != [0; 32]));
    assert_ne!(keys[0], keys[1]);
}

#[test]
fn state_slices_respect_the_return_cap() {
    let world = setup_native();
    world.crank();
    let first = world.view(|a| a.state_slice(0));
    assert_eq!(first.len(), SLICE_LEN);
    let tail = world.view(|a| a.state_slice(CHUNK_LEN as u32 - 10));
    assert_eq!(tail.len(), 10, "slices stop at the chunk boundary");
    assert!(world.view(|a| a.state_slice(STATE_LEN as u32)).is_empty());
}

/// The browser's job in miniature: predict the chain from the last state and
/// the inputs it knows about, and land on the exact same bytes.
#[test]
fn chain_and_local_simulation_agree() {
    let world = two_players();
    let state = world.state();
    let a = state.slot_of(&player(1)).unwrap();
    let b = state.slot_of(&player(2)).unwrap();
    let start = state.tic + 5;
    let walk_a = [FORWARD | TURN_RIGHT; 120].to_vec();
    let walk_b = [FORWARD | TURN_LEFT; 90].to_vec();
    world.send(&player(1), start, &walk_a);
    world.send(&player(2), start + 30, &walk_b);
    world.next_block();

    let mut local = copy(&state);
    local_crank(&mut local, world.crank_target(state.genesis_ns), &[(a, start, walk_a), (b, start + 30, walk_b)]);
    world.crank();
    for _ in 0..3 {
        local_crank(&mut local, world.crank_target(state.genesis_ns), &[]);
        world.crank();
    }
    let chain = world.state();
    assert_eq!(chain.tic, local.tic);
    assert_ne!(chain.players[a].x, state.players[a].x, "player 1 should have moved");
    assert_eq!(state_hash(&chain), state_hash(&local));
}

/// Searches for a shooter among the seated players whose buttons (turn k
/// tics, then hold fire) kill someone, by playing each candidate on a local
/// copy of the arena: the same engine the chain runs.
fn find_kill(state: &Arena, start: u32) -> Option<(usize, Vec<u8>)> {
    for shooter in (0..MAX_PLAYERS).filter(|&s| state.players[s].alive) {
        for turn in [TURN_RIGHT, TURN_LEFT] {
            for k in 0..=86 {
                let mut buttons = vec![turn; k];
                buttons.resize(MAX_BATCH, FIRE);
                let mut local = copy(state);
                let kills = local_crank(&mut local, start + MAX_BATCH as u32, &[(shooter, start, buttons.clone())]);
                if kills.count > 0 {
                    return Some((shooter, buttons));
                }
            }
        }
    }
    None
}

#[test]
fn a_kill_emits_a_frag_and_a_chain_rendered_kill_cam() {
    let world = setup_native();
    world.crank();
    for n in 0..8 {
        world.join(&player(n));
    }
    world.next_block();
    world.play(8);
    let state = world.state();
    let start = state.tic + 5;
    let (a, buttons) = find_kill(&state, start).expect("someone in a full room should have a shot");
    let shooter = ContractAddress::from(state.players[a].id);

    let mut local = copy(&state);
    let expected = local_crank(&mut local, start + MAX_BATCH as u32, &[(a, start, buttons.clone())]);
    let kill = *expected.iter().next().unwrap();
    let b = kill.victim as usize;
    let victim = ContractAddress::from(state.players[b].id);

    world.send(&shooter, start, &buttons);
    world.next_block();
    let mut frags = Vec::new();
    let mut cams = Vec::new();
    for _ in 0..6 {
        let (result, _) = world.crank();
        for e in result.filter_events::<Frag>(&world.arena) {
            let f = e.payload();
            frags.push((f.killer, f.victim, f.killer_slot, f.victim_slot));
        }
        for e in result.filter_events::<KillCam>(&world.arena) {
            cams.push(e.payload().frame.to_vec());
        }
    }

    assert_eq!(frags.len(), 1);
    assert_eq!(frags[0], (shooter, victim, a as u32, b as u32));

    assert_eq!(cams.len(), 1);
    let mut chain_frame = [0u8; FB_BYTES];
    engine::rle::decode(&cams[0], &mut chain_frame).unwrap();
    let mut local_frame = [0u8; FB_BYTES];
    local.render_kill(&kill, &mut local_frame);
    assert_eq!(frame_hash(&chain_frame), frame_hash(&local_frame), "the chain drew what the browser would");

    let after = world.state();
    assert_eq!(after.players[a].frags, 1);
    assert_eq!(after.players[b].deaths, 1);
}
