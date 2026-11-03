#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use arena::{ArenaServerClient, CrankReport};
use engine::arena::{Arena, KillLog, SIM_DELAY, STATE_LEN, tic_at};
use thruput_sdk::*;

static NONCE: SHA256Hash = thruput_sdk::sha256_static!("ARENA_TEST_NONCE");

pub const T0_NS: u64 = 1_800_000_000 * 1_000_000_000;
pub const BLOCK_NS: u64 = 2_000_000_000;

pub fn player(n: u8) -> ContractAddress {
    ContractAddress::named(&format!("player-{n}"))
}

pub struct World {
    pub env: TestEnvironmentHandle,
    pub arena: ContractAddress,
    now_ns: std::cell::Cell<u64>,
}

/// Futurenet states `maxBytestringLen` 4096 (`getChainConfig`); the mock's
/// default is the protocol fallback of 1024.
fn futurenet_env() -> TestEnvironmentHandle {
    TestEnvironment::builder()
        .no_cost()
        .with_max_bytestring_len(engine::arena::CHUNK_LEN as u32)
        .build()
}

pub fn setup_native() -> World {
    let env = futurenet_env();
    let hash = *arena::register_mock_contract(&env).as_hash();
    setup_with(env, hash)
}

pub fn setup_rv32() -> World {
    let artifact = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"))
        .join("thruput")
        .join("arena.rv32a");
    let env = futurenet_env();
    let hash = env.setup_create_contract(&load_rv32_contract(artifact));
    setup_with(env, hash)
}

fn setup_with(env: TestEnvironmentHandle, hash: SHA256Hash) -> World {
    env.advance_block_at(TimePoint::from(T0_NS));
    let out: Arc<Mutex<Option<ContractAddress>>> = Arc::new(Mutex::new(None));
    let out_inner = out.clone();
    env.transact(|tx| {
        tx.call(&ContractAddress::named("deployer")).run(move || {
            *out_inner.lock().unwrap() = Some(deploy_contract(&NONCE, &hash));
        })
    })
    .assert_success();
    let arena = out.lock().unwrap().unwrap();
    // The deploy only becomes visible to later transactions once a new block opens.
    env.advance_block_at(TimePoint::from(T0_NS));
    World {
        env,
        arena,
        now_ns: std::cell::Cell::new(T0_NS),
    }
}

impl World {
    pub fn now_ns(&self) -> u64 {
        self.now_ns.get()
    }

    /// Opens the next block `ns` later; pending writes become visible.
    pub fn wait_ns(&self, ns: u64) {
        self.now_ns.set(self.now_ns.get() + ns);
        self.env.advance_block_at(TimePoint::from(self.now_ns.get()));
    }

    pub fn next_block(&self) {
        self.wait_ns(BLOCK_NS);
    }

    /// Runs `f` as `caller` in the current block, without closing it.
    pub fn transact<R: Send + 'static>(
        &self,
        caller: &ContractAddress,
        f: impl FnOnce(ArenaServerClient) -> R + std::panic::UnwindSafe + Send + 'static,
    ) -> (TransactResult, Option<R>) {
        let arena = self.arena;
        let out: Arc<Mutex<Option<R>>> = Arc::new(Mutex::new(None));
        let out_inner = out.clone();
        let result = self.env.transact(|tx| {
            tx.call(caller).run(move || {
                *out_inner.lock().unwrap() = Some(f(ArenaServerClient::new(&arena)));
            })
        });
        let value = out.lock().unwrap().take();
        (result, value)
    }

    /// Runs `f` as `caller`, asserts success, and closes the block.
    pub fn call<R: Send + 'static>(
        &self,
        caller: &ContractAddress,
        f: impl FnOnce(ArenaServerClient) -> R + std::panic::UnwindSafe + Send + 'static,
    ) -> R {
        let (result, value) = self.transact(caller, f);
        result.assert_success();
        self.next_block();
        value.unwrap()
    }

    /// A read-only call: runs in the current block and leaves the clock alone.
    pub fn view<R: Send + 'static>(
        &self,
        f: impl FnOnce(ArenaServerClient) -> R + std::panic::UnwindSafe + Send + 'static,
    ) -> R {
        let (result, value) = self.transact(&ContractAddress::named("viewer"), f);
        result.assert_success();
        value.unwrap()
    }

    pub fn join(&self, who: &ContractAddress) {
        self.transact(who, |a| a.join()).0.assert_success();
    }

    pub fn send(&self, who: &ContractAddress, start: u32, buttons: &[u8]) {
        let buttons = buttons.to_vec();
        self.transact(who, move |a| a.send_inputs(start, &buttons)).0.assert_success();
    }

    /// Cranks in the current block and closes it.
    pub fn crank(&self) -> (TransactResult, CrankReport) {
        let arena = self.arena;
        let result = self.env.transact(|tx| {
            tx.call(&ContractAddress::named("cranker"))
                .run(move || ArenaServerClient::new(&arena).crank())
        });
        result.assert_success();
        let report = result.assert_return_as::<CrankReport>();
        self.next_block();
        (result, report)
    }

    /// Lets `seconds` pass, one block and one crank at a time.
    pub fn play(&self, seconds: u64) {
        for _ in 0..seconds * 1_000_000_000 / BLOCK_NS {
            self.crank();
        }
    }

    /// The arena as a browser would read it.
    pub fn state(&self) -> Arena {
        let mut bytes = Vec::new();
        while bytes.len() < STATE_LEN {
            let offset = bytes.len() as u32;
            let slice = self.view(move |a| a.state_slice(offset));
            assert!(!slice.is_empty(), "state ended early at {offset}");
            bytes.extend(slice);
        }
        Arena::decode(&bytes).expect("state decodes")
    }

    /// The tic a crank in the current block simulates up to.
    pub fn crank_target(&self, genesis_ns: u64) -> u32 {
        tic_at(genesis_ns, self.now_ns()).saturating_sub(SIM_DELAY)
    }
}

/// Mirrors one crank on a local copy: apply queued inputs, then simulate.
pub fn local_crank(arena: &mut Arena, target: u32, inputs: &[(usize, u32, Vec<u8>)]) -> KillLog {
    for (slot, start, buttons) in inputs {
        arena.apply_inputs(*slot, *start, buttons);
    }
    let mut kills = KillLog::new();
    arena.advance_to(target, &mut kills);
    kills
}

pub fn state_hash(arena: &Arena) -> u32 {
    let mut scratch = vec![0u8; engine::arena::STATE_LEN];
    arena.state_hash(&mut scratch)
}
