# doom-lite (working name)

An **original** retro first-person shooter, in the spirit of early-90s corridor shooters, whose runs are verified on Thruput. It uses our own engine, levels and art. It is not a port or clone of DOOM.

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
