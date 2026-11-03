use engine::game::{ActorState, MAX_ACTORS, input, scripted_input};
use engine::{FB_BYTES, Game, render, trig};

fn run(seed: u32, actors: usize, tics: u32) -> Game {
    let mut game = Game::new(seed, actors);
    for t in 0..tics {
        game.tick(scripted_input(t));
    }
    game
}

#[test]
fn same_seed_and_inputs_replay_identically() {
    let a = run(7, 32, 2_000);
    let b = run(7, 32, 2_000);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn different_seeds_diverge() {
    assert_ne!(run(1, 32, 500).state_hash(), run(2, 32, 500).state_hash());
}

#[test]
fn rendering_is_deterministic() {
    let game = run(3, 16, 300);
    let (mut fb1, mut fb2) = ([0u8; FB_BYTES], [0u8; FB_BYTES]);
    render::render(&game, &mut fb1);
    render::render(&game, &mut fb2);
    assert_eq!(render::frame_hash(&fb1), render::frame_hash(&fb2));
}

#[test]
fn actor_count_is_capped() {
    assert_eq!(Game::new(1, 1_000).actor_count, MAX_ACTORS);
    assert_eq!(Game::new(1, 0).actor_count, 0);
}

#[test]
fn every_actor_spawns_alive() {
    let game = Game::new(9, MAX_ACTORS);
    assert!(game.actors.iter().all(|a| a.state == ActorState::Idle));
}

#[test]
fn player_cannot_walk_through_walls() {
    let mut game = Game::new(1, 0);
    for _ in 0..1_000 {
        game.tick(input::FORWARD);
    }
    let (tx, ty) = (game.player.x >> 16, game.player.y >> 16);
    assert_eq!(engine::map::tile(tx, ty), engine::map::EMPTY);
}

#[test]
fn dead_player_freezes_the_simulation() {
    let mut game = Game::new(1, 0);
    game.player.hp = 0;
    let before = game.state_hash();
    game.tick(input::FORWARD | input::FIRE);
    assert_eq!(before, game.state_hash());
}

#[test]
fn trig_is_unit_length_at_cardinals() {
    assert_eq!(trig::sin(0), 0);
    assert_eq!(trig::cos(0), 1 << 16);
    assert_eq!(trig::sin(256), 1 << 16);
    assert_eq!(trig::sin(512), 0);
    assert_eq!(trig::sin(768), -(1 << 16));
}

fn creature_columns(game: &Game) -> Vec<usize> {
    let mut fb = [0u8; FB_BYTES];
    render::render(game, &mut fb);
    (0..engine::W)
        .filter(|&x| (0..engine::H).any(|y| render::pixel(&fb, x, y) == 12))
        .collect()
}

/// The player starts at tile (3,3) facing +x down an open corridor.
fn game_with_actor_at(x: i32, y: i32) -> Game {
    let mut game = Game::new(1, 1);
    game.actors[0].x = x;
    game.actors[0].y = y;
    game
}

#[test]
fn creature_ahead_projects_to_screen_center() {
    let game = game_with_actor_at(5 << 16 | 0x8000, 3 << 16 | 0x8000);
    let cols = creature_columns(&game);
    assert!(!cols.is_empty(), "creature should be visible");
    let mid = (cols[0] + cols[cols.len() - 1]) / 2;
    assert!((70..=90).contains(&mid), "centered, got {mid}");
}

#[test]
fn creature_toward_negative_y_appears_on_the_left() {
    let game = game_with_actor_at(5 << 16 | 0x8000, 3 << 16);
    let cols = creature_columns(&game);
    let mid = (cols[0] + cols[cols.len() - 1]) / 2;
    assert!(mid < 80, "left half, got {mid}");
}

#[test]
fn creature_behind_a_wall_is_hidden() {
    // Tile (7,3) is a wall; (8,3) is behind it.
    let game = game_with_actor_at(8 << 16 | 0x8000, 3 << 16 | 0x8000);
    assert!(creature_columns(&game).is_empty());
}

#[test]
fn shooting_a_creature_ahead_hits_it() {
    let mut game = game_with_actor_at(5 << 16 | 0x8000, 3 << 16 | 0x8000);
    let hp = game.actors[0].hp;
    game.tick(input::FIRE);
    assert!(game.actors[0].hp < hp);
}
