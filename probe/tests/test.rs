mod common;

use engine::game::scripted_input;
use engine::{FB_BYTES, Game, render};
use probe::register_mock_contract;
use thruput_sdk::*;

#[test]
fn test_deploy() {
    let env = TestEnvironment::no_cost();
    let contract = register_mock_contract(&env);
    common::initializes(&env, contract.as_hash());
}

#[test]
fn contract_matches_engine_run_natively() {
    let mut game = Game::new(1, 16);
    for t in 0..350 {
        game.tick(scripted_input(t));
    }
    let expected_state = game.state_hash();
    let mut fb = [0u8; FB_BYTES];
    render::render(&game, &mut fb);
    let expected_frame = render::frame_hash(&fb);

    let env = TestEnvironment::no_cost();
    let contract = register_mock_contract(&env);
    common::with_probe(&env, contract.as_hash(), "probe_native", move |probe| {
        assert_eq!(probe.sim(350, 16), expected_state);
        assert_eq!(probe.render(350, 16, 1), expected_frame);
    });
}
