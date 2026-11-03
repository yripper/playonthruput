#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use engine::tape::{self, Footer, Header};
use engine::verify;
use runs::{RunError, RunReceipt, RunsClient};
use thruput_sdk::*;

static NONCE: SHA256Hash = thruput_sdk::sha256_static!("RUNS_TEST_NONCE");

/// A fixed block time on an epoch boundary, so tests know the seed id.
pub const T0_SECONDS: u64 = 1_800_000_000 - (1_800_000_000 % runs::EPOCH_SECONDS);
pub const T0_SEED_ID: u32 = (T0_SECONDS / runs::EPOCH_SECONDS) as u32;

pub fn at_seconds(seconds: u64) -> TimePoint {
    TimePoint::from(seconds * 1_000_000_000)
}

pub struct World {
    pub env: TestEnvironmentHandle,
    pub runs: ContractAddress,
    now: std::cell::Cell<u64>,
}

pub fn setup_native() -> World {
    let env = TestEnvironment::no_cost();
    let hash = *runs::register_mock_contract(&env).as_hash();
    setup_with(env, hash)
}

pub fn setup_rv32() -> World {
    let artifact = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"))
        .join("thruput")
        .join("runs.rv32a");
    let env = TestEnvironment::no_cost();
    let hash = env.setup_create_contract(&load_rv32_contract(artifact));
    setup_with(env, hash)
}

fn setup_with(env: TestEnvironmentHandle, hash: SHA256Hash) -> World {
    env.advance_block_at(at_seconds(T0_SECONDS));
    let out: Arc<Mutex<Option<ContractAddress>>> = Arc::new(Mutex::new(None));
    let out_inner = out.clone();
    env.transact(|tx| {
        tx.call(&ContractAddress::named("deployer")).run(move || {
            *out_inner.lock().unwrap() = Some(deploy_contract(&NONCE, &hash));
        })
    })
    .assert_success();
    let runs = out.lock().unwrap().unwrap();
    // The deploy only becomes visible to later transactions once a new block opens.
    env.advance_block_at(at_seconds(T0_SECONDS));
    World {
        env,
        runs,
        now: std::cell::Cell::new(T0_SECONDS),
    }
}

impl World {
    /// Commits pending writes by opening a new block at the current time.
    fn next_block(&self) {
        self.env.advance_block_at(at_seconds(self.now.get()));
    }

    pub fn set_time(&self, seconds: u64) {
        self.now.set(seconds);
        self.next_block();
    }

    /// Runs `f` as `caller` in its own transaction and returns its value.
    pub fn call<R: Send + 'static>(
        &self,
        caller: &ContractAddress,
        f: impl FnOnce(RunsClient) -> R + std::panic::UnwindSafe + Send + 'static,
    ) -> R {
        let runs = self.runs;
        let out: Arc<Mutex<Option<R>>> = Arc::new(Mutex::new(None));
        let out_inner = out.clone();
        self.env
            .transact(|tx| {
                tx.call(caller).run(move || {
                    *out_inner.lock().unwrap() = Some(f(RunsClient::new(&runs)));
                })
            })
            .assert_success();
        self.next_block();
        let value = out.lock().unwrap().take().unwrap();
        value
    }

    pub fn ensure_seed(&self) -> runs::SeedInfo {
        self.call(&ContractAddress::named("cron"), |r| r.ensure_current_seed())
    }

    pub fn run_seed(&self, seed_id: u32, player: &ContractAddress) -> u32 {
        let player = *player;
        self.call(&ContractAddress::named("viewer"), move |r| r.run_seed(seed_id, &player))
    }

    pub fn best(&self, player: &ContractAddress, seed_id: u32) -> u32 {
        let player = *player;
        self.call(&ContractAddress::named("viewer"), move |r| r.best_score(&player, seed_id))
    }

    pub fn submit(&self, player: &ContractAddress, tape: Vec<u8>) -> TransactResult {
        let runs = self.runs;
        let result = self.env.transact(|tx| {
            tx.call(player)
                .run(move || RunsClient::new(&runs).submit_run(&tape))
        });
        self.next_block();
        result
    }

    pub fn submit_ok(&self, player: &ContractAddress, tape: Vec<u8>) -> (TransactResult, RunReceipt) {
        let result = self.submit(player, tape);
        result.assert_success();
        let receipt = result.assert_return_as::<RunReceipt>();
        (result, receipt)
    }

    pub fn submit_err(&self, player: &ContractAddress, tape: Vec<u8>) -> RunError {
        self.submit(player, tape).assert_abort_as::<RunError>()
    }
}

/// Builds an honest tape: plays `inputs` on `seed` and claims the true result.
pub fn honest_tape(seed_id: u32, seed: u32, inputs: &[u8]) -> Vec<u8> {
    let outcome = verify::play(seed, inputs);
    tape_with_footer(
        seed_id,
        seed,
        inputs,
        Footer {
            score: outcome.score,
            state_hash: outcome.state_hash,
        },
    )
}

pub fn tape_with_footer(seed_id: u32, seed: u32, inputs: &[u8], footer: Footer) -> Vec<u8> {
    let header = Header {
        level: 0,
        seed_id,
        seed,
        tics: inputs.len() as u32,
    };
    let mut out = vec![0u8; tape::HEADER_LEN + inputs.len() + tape::FOOTER_LEN];
    tape::encode(&header, inputs, &footer, &mut out);
    out
}

/// A simple aim bot that plays a real game: turns toward the nearest visible
/// creature and fires when lined up, otherwise walks and turns off walls.
/// It only produces inputs; the tape it yields is as legitimate as a human's.
pub fn bot_inputs(seed: u32, tics: u32) -> Vec<u8> {
    use engine::game::ActorState;
    use engine::verify::LEVEL_CREATURES;
    use engine::input::{FIRE, FORWARD, TURN_LEFT, TURN_RIGHT};
    let mut game = engine::Game::new(seed, LEVEL_CREATURES);
    let mut inputs = Vec::with_capacity(tics as usize);
    let mut last_pos = (game.player.x, game.player.y);
    for _ in 0..tics {
        let p = &game.player;
        let target = game.actors[..game.actor_count]
            .iter()
            .filter(|a| a.state != ActorState::Dead)
            .filter(|a| engine::ray::line_clear(&engine::map::CAMPAIGN, p.x, p.y, a.x, a.y))
            .min_by_key(|a| (a.x - p.x).abs().max((a.y - p.y).abs()));
        let buttons = match target {
            Some(a) => {
                let (dx, dy) = ((a.x - p.x) as f64, (a.y - p.y) as f64);
                let want = (dy.atan2(dx) / std::f64::consts::TAU * 1024.0).round() as i32;
                let diff = (want - p.angle as i32).rem_euclid(1024);
                let diff = if diff > 512 { diff - 1024 } else { diff };
                let turn = if diff > 6 { TURN_RIGHT } else if diff < -6 { TURN_LEFT } else { 0 };
                if diff.abs() <= 10 { turn | FIRE } else { turn }
            }
            None if (p.x, p.y) == last_pos => FORWARD | TURN_RIGHT,
            None => FORWARD,
        };
        last_pos = (game.player.x, game.player.y);
        inputs.push(buttons);
        game.tick(buttons);
    }
    inputs
}
