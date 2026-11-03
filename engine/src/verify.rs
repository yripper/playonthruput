//! Replay and scoring rules. The contract, the relay pre-check and the
//! browser all call `verify`, so they can never disagree on a score.

use crate::game::Game;
use crate::tape::{self, MAX_TICS, TapeError};

/// Level 0 always spawns this many creatures.
pub const LEVEL_CREATURES: usize = 24;
pub const POINTS_PER_KILL: u32 = 1_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub tics: u32,
    pub kills: u32,
    /// 0 if the level was not cleared.
    pub cleared_at: u32,
    pub alive: bool,
    pub score: u32,
    pub state_hash: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyError {
    Tape(TapeError),
    UnknownLevel,
    SeedMismatch,
    ScoreMismatch,
    StateMismatch,
}

/// Kills dominate; clearing the level adds a bonus that shrinks the longer it
/// took, so among full clears the fastest run wins.
pub fn score(kills: u32, cleared_at: u32) -> u32 {
    let clear_bonus = if cleared_at > 0 { MAX_TICS - cleared_at } else { 0 };
    kills * POINTS_PER_KILL + clear_bonus
}

/// Simulates a run from its seed and inputs.
pub fn play(seed: u32, inputs: &[u8]) -> Outcome {
    let mut game = Game::new(seed, LEVEL_CREATURES);
    for &buttons in inputs {
        game.tick(buttons);
    }
    Outcome {
        tics: inputs.len() as u32,
        kills: game.kills,
        cleared_at: game.cleared_at,
        alive: game.alive(),
        score: score(game.kills, game.cleared_at),
        state_hash: game.state_hash(),
    }
}

/// Full check: container, expected seed, replay, and both footer claims.
pub fn verify(tape_bytes: &[u8], expected_seed: u32) -> Result<Outcome, VerifyError> {
    let tape = tape::parse(tape_bytes).map_err(VerifyError::Tape)?;
    if tape.header.level != 0 {
        return Err(VerifyError::UnknownLevel);
    }
    if tape.header.seed != expected_seed {
        return Err(VerifyError::SeedMismatch);
    }
    let outcome = play(tape.header.seed, tape.inputs);
    if outcome.score != tape.footer.score {
        return Err(VerifyError::ScoreMismatch);
    }
    if outcome.state_hash != tape.footer.state_hash {
        return Err(VerifyError::StateMismatch);
    }
    Ok(outcome)
}
