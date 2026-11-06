# playonthruput (repo: doom-lite)

An **original** retro first-person deathmatch where **the chain is the game server**. It uses our own engine, map and art; it is not a port or clone of DOOM. There is no backend: a static site talks to Thruput Futurenet directly.

- **Arena contract (Futurenet):** `6kJjYNByNncx8oMjWipewY3AVNbFofK96RgPCThHCQtc`
- **Run the client:** `cd web && npm install && npm run dev`. `npm run build` rebuilds the wasm engine and the static site into `web/dist`.

## How the arena works

```
browser (wasm engine)                     Thruput (arena contract, RV32)
 predict present from last state ──┐
 send_inputs(start_tic, buttons) ──┼──► RCQ queue (fixed 256 B entries, any # of txs/block)
 crank() every block (1/n odds) ───┘──► crank: drain queue → simulate to now − 4 s
 getStorage ×3 (4 KiB chunks)  ◄──────── state: 9,217 B arena in 3 bytestrings
 getEvents: Frag, KillCam      ◄──────── kill cam rendered + RLE-coded on chain
```

- **Lockstep with a delay.** The chain simulates `SIM_DELAY` = 140 tics (4 s) behind its block clock, so every player's inputs for a tic land before that tic is simulated. Browsers replay their own unconfirmed inputs on top of the last confirmed state to show the present.
- **Favor the shooter.** Other players are drawn `REWIND` = 105 tics (3 s) in the past, where their inputs are already known, and shots are checked against those same rewound positions (128-tic position history). What you saw is what you hit.
- **One engine, three targets.** `engine/` is `no_std` integer-only Rust; it compiles to RV32 for the contract and to a 16 KB wasm module for browsers, so chain/browser parity is by construction (tests check state hashes match byte for byte).
- **Kill cam.** For every kill the crank renders the killer's 160×100 view at the fatal shot and emits it RLE-coded (~580 B) in a `KillCam` event. The victim's browser shows the chain's frame.
- **Session keys.** Each browser makes a throwaway Ed25519 key, funds it from the Futurenet faucet (`faucet_requestAirdrop`), and signs its own transactions.

| Arena measurement (RV32) | Result |
|---|---|
| Crank, 16 players fighting, kill cams included (worst) | **3.0M gas** (0.3% of the 1e9 cap) |
| Crank catching up 280 tics | 0.6–0.9M gas |
| Peak crank stack | 13.7 KiB of a 20 KiB stack |
| Live crank on Futurenet, 1 player, 785 tics | 650k gas |
| Browser prediction, 600 tics / render one frame | 4.5 ms / 1.4 ms (Node, cold) |

**Chain facts this design rests on** (verified against source or the live RPC):
- Contract return values are capped at **1,024 B** when the chain config doesn't state `maxReturnBufferLen`, and Futurenet doesn't (`thruput-xdr` `network_config.rs:292-295`). So state is read with `getStorage`, not view calls.
- Futurenet states `maxBytestringLen` 4096 (`getChainConfig`); the mock test env defaults to 1024, so tests set it explicitly.
- The RV32 guest heap is a bump allocator that never frees (`thruput-rv32-runtime` `allocator.rs`). The crank consumes queue entries with `try_consume_sized` into the stack and builds the arena in place on the heap.
- The public RPC allows any CORS origin and the faucet needs no captcha, which is what makes "no backend" possible.

## Earlier work in this repo

The engine started as a single-player campaign with verified runs (Kalien's tape-replay pattern), kept in `contract/` (`runs`) and `probe/`. That history is below.

Research inputs:
- `../../resources/kalien-thruput`: Asteroids, with tapes replayed on-chain.
- `../../resources/thruput-spray-pray`: a high-throughput tx pipeline.
- `../../resources/thruput-examples`
- `../../open-source-survey.md`

## Constraints we design around

| Limit | Value | Source |
|---|---|---|
| Gas | 1 per RV32 instruction, 1e9 max per tx | `thruput-sdk-rs/crates/rv32-runtime/src/mem.rs:17` |
| Guest memory | 64 KiB (stack + heap + calldata) | chain config |
| Calldata | >4 KiB needs the SDK `alloc` feature; Kalien ships 18 KB tapes | `kalien_score/Cargo.toml:29-33` |
| View-call gas | 10M, so a full replay can't be previewed by simulation | Kalien `PORT-PLAN.md:61` |
| Bytestring | 4 KiB each | chain config |
| Latency | ~2 s blocks; 2–10 s from send to receipt under load | spray-pray `README.md:416-424` |
| Crypto | Ed25519, ML-DSA-65, SHA-256 only | `thruput-xdr/src/types.rs:608-612` |
| Randomness | none native; derive from block fields | Kalien `implementation.rs:228-249` |

**Kalien numbers we calibrate against:**
- Asteroids runs at ~2k gas per frame, so a 10-minute (36,000-frame) game costs ~74M gas.
- Peak memory is ~41 KiB of 64.
- Contract size is 55 KB.

Memory is the binding constraint, not gas.

## Design: three modes, built in order

### Mode A: Verified runs (build first, proven pattern)

The Kalien model applied to an FPS:
1. Play in the browser.
2. Record one input byte per tic: forward/back, strafe, turn, fire, use.
3. Submit the tape.
4. The contract replays the whole run with the same deterministic engine, checks the claimed result (time / kills / secrets), records the best, and mints or ranks.

- **Tape:** 35 tics/s × 1 B = 35 B/s, so a 5-minute level is ~10.5 KB of calldata. That fits.
- **Gas budget:** 1e9 / (35 × 300 tics) ≈ **95k instructions per tic** for a 5-minute level. Target ≤30k/tic so there's 3× headroom. A grid-based world makes collision and line-of-sight a few dozen DDA steps each, so ~30 enemies should fit. *To measure.*
- **Memory:**
  - The level is a grid (64×64 = 4 KB) plus a thing list.
  - Dynamic state (player, ~64 actors, doors, pickups) is ~4–8 KB in fixed arrays, with no allocation in the step.
  - Embed the levels in the contract binary as read-only data (*to verify whether rodata counts against the 64 KiB*), or store them as 4 KB bytestrings.
- **No rendering on-chain.** The chain only replays the simulation. That's what makes it fit.

### Mode B: Chain as game server (multiplayer, experimental)

1. Players insert inputs into an `RCQ` (≤250 per block, no conflicts).
2. One permissionless crank per block advances the shared world one tick for everyone and emits per-player state as events.

At ~0.5 ticks/s this is a **simultaneous-turn tactical** shooter, not a twitch one. It reuses the swap-suite queue + crank pattern, so it doubles as practice for Soroswap on Thruput.

### Mode C: Stunt, the chain renders a frame

A contract raycasts one 160-column view and emits ~320 bytes: wall distance + texture id per column, plus sprites. The browser paints it. Good for a demo video; not needed for A or B.

## Lessons taken from Kalien and Spray & Pray

- **Share one engine crate.** Kalien hand-wrote a TS copy of the Rust engine and needed a parity tool (0 mismatches over 59.3M frames). We compile the same `no_std` Rust core to **wasm for the browser** and **RV32 for the contract**, so parity comes by construction. We keep golden state-hash tests anyway.
- **Integer math only:** fixed point (16.16 or smaller), angles as integers, trig via lookup tables, xorshift RNG seeded per run.
- **Seeds come from the chain** (`sha256(tx_hash‖id‖height‖timepoint)`) per epoch. Admin seeding gets locked off. A copycat-tape fix (per-player seed) goes in from day one.
- **Memory hygiene:** a 16 KiB guest stack via `--defsym=__stack_size=16384`, read inputs straight from the packed tape, no allocation in the step, and `-Z trap-unreachable=false`.
- **Pre-check off-chain:** a backend replays the tape before relaying, because view calls are capped at 10M gas.
- **Submission:** server-side relay keys with explicit random nonces and expiry heights, and batches (spray-pray). The sender needs free native tokens ≥ the declared gas. Always check `receipt.execution.result`, because a failed tx still commits.
- **Futurenet resets:** read the network hash at runtime and keep a redeploy script.

## Plan

1. **Measure** with a gas-probe contract (copy Kalien's `gas-probe` idea): instructions per tic for a grid world with N actors, a DDA line-of-sight cost, and the memory peak. This decides the actor and level budget.
2. **Engine core** (`engine/`, `no_std`): map grid, player movement + collision, doors, pickups, a simple actor AI, hitscan weapon, tic loop, tape format, golden tests.
3. **Contract** (`contract/`): `submit_run(tape, claim)`, which does the checks → replay → best score → event.
4. **Browser client:** the wasm engine + a canvas raycaster renderer + a tape recorder.
5. **Relay + leaderboard:** a small Worker. Kalien's Worker/D1/R2 layout is the reference.
6. Then Mode B, then the Mode C demo.

## Status

- 2026-10-31: repo created and research done. Thruput CLI thruput 0.42.0 (commit e422d971) installed (`cargo +1.98.0 install thruput --locked`; it needs rustc ≥1.96 and the default stable here is 1.94.1).
- 2026-11-03: **engine + probe working.** The `engine/` crate is `no_std`, integer-only, 12 native tests. The `probe/` contract is built to RV32 and measured with `thruput test -- --nocapture` (Thruput 0.42.0, SDK 0.58.0, test executor = real RV32 VM):

  | Measurement | Result |
  |---|---|
  | Sim, 0 / 8 / 16 / 32 / 64 creatures | 343 / 1,815 / 2,797 / 6,463 / **8,854 gas per tic** |
  | Render one 160×100 16-color frame (walls + sprites) | **~590k–670k gas** (0–64 creatures) |
  | Peak guest memory (64 creatures + 1 frame) | **15.7 KB of 64 KB** (stack 11.6 KB, heap 4 KB) |
  | RV32 vs native parity (700 tics + frame hash) | ✅ identical |

  What that means at 1e9 gas per tx:
  - **~113k tics ≈ 54 min of play with 64 creatures** verified in one tx (target was ≤30k gas per tic).
  - Rendering: ~1,600 frames of compute per tx. The real cap is events (≤1,000 × 1 KB per tx → ~125 frames).
- 2026-11-03: **verified runs (Mode A) working.**
  - **Tape format** (`engine/src/tape.rs`): 20-byte header + 1 byte per tic + 8-byte footer. A 10-minute run is 21,028 bytes.
  - **Shared rules** (`engine/src/verify.rs`): 24 creatures, 1,000 points per kill, plus a speed bonus for clearing the level.
  - **`contract/` (`runs`):**
    - per-epoch seeds (Kalien's scheme) plus a per-player run seed, so tapes can't be copied between players;
    - `submit_run` replays the tape and keeps the best score per player per epoch.
  - **Tests:** 23 engine, 13 native contract and 3 RV32, all passing.

  | Run length | Tape | Gas (RV32, aim bot, 24 creatures) |
  |---|---|---|
  | 2.5 min (5,250 tics) | 5,278 B | 8.7M |
  | 5 min (10,500 tics) | 10,528 B | 30.9M |
  | 10 min (21,000 tics, max) | 21,028 B | **70.9M (7% of the 1e9 cap)** |
- 2026-11-03: **multiplayer arena live on Futurenet.**
  - `engine/src/arena.rs`: 16-player lockstep deathmatch, input and position rings, lag-compensated hitscan, spawn protection, respawn, idle kick, fixed binary state. `engine/src/rle.rs`: kill-cam frame coding.
  - `arena/`: the contract (`join`, `send_inputs`, `crank`, `state_keys`, `state_slice`). Tests: 10 native (including chain/local state-hash parity and a chain-rendered kill cam matching the local render pixel for pixel), 2 RV32 (parity and gas).
  - `wasm/`: the engine for browsers. `web/`: the Vite client.
  - Verified with two headless Chrome players on Futurenet: seated within ~8 s, shots and kills confirmed on chain, kill cam shown on the victim's screen.
  - Bug found live: the crank that seats the first player in a sleeping arena also skips ahead, and the idle kick removed that player at once. Fixed in `skip_to`.
- 2026-11-06: **netcode: no more self-snaps.** Measured with two headless players moving for 90 s (`?debug` or the `` ` `` key shows the same readout). Contract redeployed as `6kJjYNBy…`.
  - **Clock (main cause).** Blocks reach the RPC 2.5–3.3 s after their stamp, so a clock read from the newest block ran ~2.5 s slow and inputs reached the chain about when their tics were simulated. Players now clock from their own transactions (stamp of the block each landed in minus send time, median of 9), seeded from the join tx, and the game clock eases toward new estimates (≤5 % fast/slow) instead of jumping. Jumps had left unsampled or stalled tics. Result: 0 of ~40 confirmed states disagree with the player's prediction (was ~9 in 10).
  - **Cranks.** Every seated browser cranks once per 2.03 s block. Only one crank per block drains the queue; extras are deferred to the next block, so no block goes uncranked (state every 2.05 s, was 2.85 s; worst gap ~3.5 s, was 7.8 s). Duplicate kill reports from cranks sharing a block are filtered.
  - **Late cranks.** Inputs more than 256 tics past where the arena stopped are held in a per-crank overflow (`engine::arena::Overflow`) and moved in as the arena advances, instead of being dropped.
  - **Reads.** State chunks read at different heights are rejected; a crank landing between reads had mixed two states.
  - **Opponents.** Drawn easing toward corrected positions (≤3 tics of running per frame; respawns jump). Snaps 13 and 2 per 90 s, was 24–39. Guessing opponents' moves was tried and made it worse.
  - Still true: opponents' inputs reach other browsers ~5 s after they happen (send + block + RPC visibility), longer than the 3 s `REWIND`, so their drawn position is often a correction in progress.
