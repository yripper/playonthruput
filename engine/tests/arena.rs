//! Multiplayer arena: lockstep inputs, lag-compensated shots, persistence.

use engine::arena::*;
use engine::fx::ONE;
use engine::input;
use engine::render::{FB_BYTES, frame_hash};
use engine::rle;

const T0: u64 = 1_800_000_000_000_000_000;

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

fn step_n(arena: &mut Arena, n: u32) -> KillLog {
    let mut kills = KillLog::new();
    for _ in 0..n {
        arena.step(&mut kills);
    }
    kills
}

/// Shooter at (2.5, 13.5) facing +x, target 6 tiles down the same open row,
/// both standing long enough that their rewound positions are where they are.
fn duel() -> (Arena, usize, usize) {
    let mut arena = Arena::new(T0, 7);
    let a = arena.join(&id(1)).unwrap();
    let b = arena.join(&id(2)).unwrap();
    step_n(&mut arena, 1);
    place(&mut arena, a, 2, 13, 0);
    place(&mut arena, b, 8, 13, 512);
    step_n(&mut arena, REWIND + 2);
    (arena, a, b)
}

fn place(arena: &mut Arena, slot: usize, tx: i32, ty: i32, angle: u32) {
    let p = &mut arena.players[slot];
    p.x = tx * ONE + ONE / 2;
    p.y = ty * ONE + ONE / 2;
    p.angle = angle;
}

fn fire_now(arena: &mut Arena, slot: usize) -> KillLog {
    let t = arena.tic;
    arena.apply_inputs(slot, t, &[input::FIRE]);
    step_n(arena, 1)
}

/// Inputs keep players alive past the idle kick.
fn keepalive(arena: &mut Arena, slots: &[usize]) {
    let t = arena.tic;
    for &s in slots {
        arena.apply_inputs(s, t, &[0]);
    }
}

#[test]
fn tic_clock_is_35_per_second() {
    assert_eq!(tic_at(T0, T0), 0);
    assert_eq!(tic_at(T0, T0 + 1_000_000_000), 35);
    assert_eq!(tic_at(T0, T0 + 4_000_000_000), SIM_DELAY);
    assert_eq!(tic_at(T0, T0 - 5), 0, "time before genesis clamps to 0");
}

#[test]
fn join_assigns_slots_and_is_idempotent() {
    let mut arena = Arena::new(T0, 1);
    assert_eq!(arena.join(&id(1)), Some(0));
    assert_eq!(arena.join(&id(2)), Some(1));
    assert_eq!(arena.join(&id(1)), Some(0));
    assert_eq!(arena.slot_of(&id(2)), Some(1));
    assert_eq!(arena.slot_of(&id(3)), None);
}

#[test]
fn full_arena_rejects_joins() {
    let mut arena = Arena::new(T0, 1);
    for n in 0..MAX_PLAYERS as u8 {
        assert!(arena.join(&id(n + 1)).is_some());
    }
    assert_eq!(arena.join(&id(200)), None);
}

#[test]
fn players_spawn_on_the_next_tic() {
    let mut arena = Arena::new(T0, 1);
    let a = arena.join(&id(1)).unwrap();
    assert!(!arena.players[a].alive);
    step_n(&mut arena, 1);
    let p = arena.players[a];
    assert!(p.alive);
    assert_eq!(p.hp, MAX_HP);
}

#[test]
fn inputs_outside_the_window_are_dropped() {
    let mut arena = Arena::new(T0, 1);
    let a = arena.join(&id(1)).unwrap();
    step_n(&mut arena, 10);
    arena.apply_inputs(a, 5, &[input::FIRE; 4]);
    assert_eq!(arena.input_at(a, 5), 0, "past tics can't be rewritten");
    arena.apply_inputs(a, 10 + INPUT_RING as u32, &[input::FIRE]);
    assert_eq!(arena.players[a].last_input, 0, "too far ahead is dropped");
    // A batch straddling the window keeps only the in-window part.
    arena.apply_inputs(a, 8, &[input::FORWARD; 4]);
    assert_eq!(arena.input_at(a, 10), input::FORWARD);
    assert_eq!(arena.input_at(a, 11), input::FORWARD);
    assert_eq!(arena.players[a].last_input, 11);
}

#[test]
fn buffered_inputs_move_the_player_on_their_tic() {
    let mut arena = Arena::new(T0, 1);
    let a = arena.join(&id(1)).unwrap();
    step_n(&mut arena, 1);
    place(&mut arena, a, 2, 13, 0);
    let x0 = arena.players[a].x;
    let t = arena.tic;
    arena.apply_inputs(a, t + 5, &[input::FORWARD; 10]);
    step_n(&mut arena, 5);
    assert_eq!(arena.players[a].x, x0, "nothing moves before the buffered tic");
    step_n(&mut arena, 10);
    assert!(arena.players[a].x > x0 + ONE / 2);
}

#[test]
fn shot_hits_a_target_in_line() {
    let (mut arena, a, b) = duel();
    fire_now(&mut arena, a);
    assert!(arena.players[b].hp < MAX_HP);
}

#[test]
fn shot_is_checked_against_the_rewound_position() {
    let (mut arena, a, b) = duel();
    // The target has since stepped out of line, but the shooter still sees
    // them where they were REWIND tics ago: favor the shooter.
    place(&mut arena, b, 8, 16, 512);
    fire_now(&mut arena, a);
    assert!(arena.players[b].hp < MAX_HP, "shot should land on the rewound position");
}

#[test]
fn shot_misses_a_target_that_only_just_stepped_into_line() {
    let (mut arena, a, b) = duel();
    place(&mut arena, b, 8, 16, 512);
    step_n(&mut arena, REWIND + 2);
    // Now in line for real, but the shooter's view lags REWIND tics behind.
    place(&mut arena, b, 8, 13, 512);
    keepalive(&mut arena, &[a, b]);
    fire_now(&mut arena, a);
    assert_eq!(arena.players[b].hp, MAX_HP);
}

#[test]
fn walls_block_shots() {
    let (mut arena, a, b) = duel();
    // Column x=10 is a pillar on rows 8..=12; put the target behind it.
    place(&mut arena, a, 6, 10, 0);
    place(&mut arena, b, 14, 10, 512);
    step_n(&mut arena, REWIND + 2);
    keepalive(&mut arena, &[a, b]);
    fire_now(&mut arena, a);
    assert_eq!(arena.players[b].hp, MAX_HP);
}

#[test]
fn freshly_spawned_players_cannot_be_shot() {
    let mut arena = Arena::new(T0, 7);
    let a = arena.join(&id(1)).unwrap();
    let b = arena.join(&id(2)).unwrap();
    step_n(&mut arena, 1);
    place(&mut arena, a, 2, 13, 0);
    place(&mut arena, b, 8, 13, 512);
    step_n(&mut arena, 10);
    fire_now(&mut arena, a);
    assert_eq!(arena.players[b].hp, MAX_HP, "the shooter can't have seen them yet");
}

/// Fires until the target dies, returning the kill.
fn kill(arena: &mut Arena, a: usize, b: usize) -> Kill {
    for _ in 0..10 {
        let log = fire_now(arena, a);
        if let Some(k) = log.iter().next() {
            return *k;
        }
        keepalive(arena, &[a, b]);
        step_n(arena, 12);
    }
    panic!("target never died");
}

#[test]
fn kill_scores_and_respawns() {
    let (mut arena, a, b) = duel();
    let k = kill(&mut arena, a, b);
    assert_eq!((k.killer as usize, k.victim as usize), (a, b));
    assert_eq!(arena.players[a].frags, 1);
    assert_eq!(arena.players[b].deaths, 1);
    assert!(!arena.players[b].alive);
    step_n(&mut arena, RESPAWN_TICS);
    assert!(arena.players[b].alive);
    assert_eq!(arena.players[b].hp, MAX_HP);
}

#[test]
fn dead_players_cannot_shoot() {
    let (mut arena, a, b) = duel();
    kill(&mut arena, a, b);
    place(&mut arena, a, 8, 13, 0);
    let hp = arena.players[a].hp;
    fire_now(&mut arena, b);
    assert_eq!(arena.players[a].hp, hp);
}

#[test]
fn idle_players_are_kicked() {
    let mut arena = Arena::new(T0, 1);
    let a = arena.join(&id(1)).unwrap();
    let b = arena.join(&id(2)).unwrap();
    let mut kills = KillLog::new();
    let mut t = 0;
    while t < IDLE_KICK_TICS + 10 {
        arena.apply_inputs(b, arena.tic, &[0]);
        t += 100;
        arena.advance_to(t, &mut kills);
    }
    assert!(!arena.players[a].active, "silent player should be kicked");
    assert!(arena.players[b].active);
    assert_eq!(arena.join(&id(3)), Some(a), "the slot is reusable");
}

#[test]
fn long_gaps_are_skipped_not_simulated() {
    let mut arena = Arena::new(T0, 1);
    arena.join(&id(1));
    let mut kills = KillLog::new();
    arena.advance_to(10_000, &mut kills);
    assert_eq!(arena.tic, 10_000);
}

fn scripted(arena: &mut Arena, slots: &[usize], tics: u32) {
    let mut kills = KillLog::new();
    for _ in 0..tics {
        let t = arena.tic;
        for (i, &s) in slots.iter().enumerate() {
            let b = engine::game::scripted_input(t.wrapping_mul(7).wrapping_add(i as u32 * 131));
            arena.apply_inputs(s, t, &[b]);
        }
        arena.step(&mut kills);
    }
}

#[test]
fn simulation_is_deterministic() {
    let mut scratch = [0u8; STATE_LEN];
    let run = |scratch: &mut [u8; STATE_LEN]| {
        let mut arena = Arena::new(T0, 99);
        let slots: Vec<usize> = (1..=6).map(|n| arena.join(&id(n)).unwrap()).collect();
        scripted(&mut arena, &slots, 2_000);
        arena.state_hash(scratch)
    };
    assert_eq!(run(&mut scratch), run(&mut scratch));
}

#[test]
fn encode_decode_round_trips() {
    let mut arena = Arena::new(T0, 5);
    let slots: Vec<usize> = (1..=4).map(|n| arena.join(&id(n)).unwrap()).collect();
    scripted(&mut arena, &slots, 500);
    let t = arena.tic;
    arena.apply_inputs(slots[0], t + 3, &[input::FORWARD; 20]);

    let mut bytes = vec![0u8; STATE_LEN];
    arena.encode(&mut bytes);
    let decoded = Arena::decode(&bytes).unwrap();
    assert_eq!(decoded.players, arena.players);
    assert_eq!(decoded.tic, arena.tic);
    assert_eq!(decoded.input_at(slots[0], t + 3), input::FORWARD);

    // Both copies continue identically.
    let (mut x, mut y) = (arena, decoded);
    scripted(&mut x, &slots, 300);
    scripted(&mut y, &slots, 300);
    let mut s = vec![0u8; STATE_LEN];
    assert_eq!(x.state_hash(&mut s), y.state_hash(&mut s));
}

#[test]
fn decode_rejects_bad_input() {
    assert_eq!(Arena::decode(&[1, 2, 3]).err(), Some(DecodeError::TooShort));
    let mut bytes = vec![0u8; STATE_LEN];
    Arena::new(T0, 1).encode(&mut bytes);
    bytes[0] = 99;
    assert_eq!(Arena::decode(&bytes).err(), Some(DecodeError::BadVersion));
}

#[test]
fn state_fits_in_four_kilobyte_chunks() {
    assert!(STATE_CHUNKS <= 4, "{STATE_LEN} bytes in {STATE_CHUNKS} chunks");
}

#[test]
fn kill_cam_renders_and_compresses() {
    let (mut arena, a, b) = duel();
    let k = kill(&mut arena, a, b);
    let mut fb = [0u8; FB_BYTES];
    arena.render_kill(&k, &mut fb);
    // The victim's figure is centered in the shooter's view.
    assert_eq!(engine::render::pixel(&fb, 80, 50), PLAYER_COLORS[b]);

    let mut enc = [0u8; rle::MAX_ENCODED];
    let n = rle::encode(&fb, &mut enc);
    let mut back = [0u8; FB_BYTES];
    rle::decode(&enc[..n], &mut back).unwrap();
    assert_eq!(frame_hash(&back), frame_hash(&fb));
    assert!(n <= 4 * 1000, "kill cam should fit in 4 events, got {n} bytes");
    eprintln!("kill cam: {n} bytes");
}

#[test]
fn rle_rejects_malformed_frames() {
    let mut fb = [0u8; FB_BYTES];
    assert_eq!(rle::decode(&[0x0f], &mut fb), Err(rle::RleError::Truncated));
    assert_eq!(rle::decode(&[0x00], &mut fb), Err(rle::RleError::Underflow));
    assert_eq!(rle::decode(&[0x0f, 255].repeat(100), &mut fb), Err(rle::RleError::Overflow));
}
