use engine::game::{input, scripted_input};
use engine::tape::{self, Footer, Header, MAX_TICS, TapeError};
use engine::verify::{self, Outcome, VerifyError};

const SEED: u32 = 0x5eed;

fn inputs(tics: u32) -> Vec<u8> {
    (0..tics).map(scripted_input).collect()
}

fn make_tape(seed: u32, inputs: &[u8], footer: Option<Footer>) -> Vec<u8> {
    let outcome = verify::play(seed, inputs);
    let footer = footer.unwrap_or(Footer {
        score: outcome.score,
        state_hash: outcome.state_hash,
    });
    let header = Header {
        level: 0,
        seed_id: 7,
        seed,
        tics: inputs.len() as u32,
    };
    let mut out = vec![0u8; tape::HEADER_LEN + inputs.len() + tape::FOOTER_LEN];
    let len = tape::encode(&header, inputs, &footer, &mut out);
    assert_eq!(len, out.len());
    out
}

#[test]
fn honest_tape_verifies() {
    let tape = make_tape(SEED, &inputs(2_000), None);
    let outcome = verify::verify(&tape, SEED).unwrap();
    assert_eq!(outcome, verify::play(SEED, &inputs(2_000)));
}

#[test]
fn parse_round_trips_header_and_footer() {
    let ins = inputs(10);
    let footer = Footer {
        score: 42,
        state_hash: 0xabcd,
    };
    let tape = make_tape(SEED, &ins, Some(footer));
    let parsed = tape::parse(&tape).unwrap();
    assert_eq!(parsed.header.seed_id, 7);
    assert_eq!(parsed.header.seed, SEED);
    assert_eq!(parsed.header.tics, 10);
    assert_eq!(parsed.inputs, &ins[..]);
    assert_eq!(parsed.footer, footer);
}

#[test]
fn inflated_score_is_rejected() {
    let ins = inputs(500);
    let real = verify::play(SEED, &ins);
    let tape = make_tape(
        SEED,
        &ins,
        Some(Footer {
            score: real.score + 1_000,
            state_hash: real.state_hash,
        }),
    );
    assert_eq!(verify::verify(&tape, SEED), Err(VerifyError::ScoreMismatch));
}

#[test]
fn wrong_state_hash_is_rejected() {
    let ins = inputs(500);
    let real = verify::play(SEED, &ins);
    let tape = make_tape(
        SEED,
        &ins,
        Some(Footer {
            score: real.score,
            state_hash: real.state_hash ^ 1,
        }),
    );
    assert_eq!(verify::verify(&tape, SEED), Err(VerifyError::StateMismatch));
}

#[test]
fn tape_for_another_seed_is_rejected() {
    let tape = make_tape(SEED, &inputs(100), None);
    assert_eq!(verify::verify(&tape, SEED + 1), Err(VerifyError::SeedMismatch));
}

#[test]
fn malformed_containers_are_rejected() {
    let good = make_tape(SEED, &inputs(100), None);

    assert_eq!(tape::parse(&good[..10]).err(), Some(TapeError::TooShort));

    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::BadMagic));

    let mut bad = good.clone();
    bad[4] = 2;
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::BadVersion));

    let mut bad = good.clone();
    bad[6] = 1;
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::ReservedNonZero));

    let mut bad = good.clone();
    bad.push(0);
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::LengthMismatch));

    let mut bad = good.clone();
    bad[16..20].copy_from_slice(&(MAX_TICS + 1).to_le_bytes());
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::TicsOutOfRange));

    let mut bad = good.clone();
    bad[16..20].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(tape::parse(&bad).err(), Some(TapeError::TicsOutOfRange));
}

#[test]
fn unknown_level_is_rejected() {
    let mut tape = make_tape(SEED, &inputs(100), None);
    tape[5] = 1;
    assert_eq!(verify::verify(&tape, SEED), Err(VerifyError::UnknownLevel));
}

#[test]
fn max_length_tape_verifies() {
    let tape = make_tape(SEED, &inputs(MAX_TICS), None);
    assert_eq!(tape.len(), tape::MAX_TAPE_LEN);
    assert!(verify::verify(&tape, SEED).is_ok());
}

#[test]
fn clearing_scores_more_than_the_same_kills_without_clearing() {
    assert!(verify::score(24, 5_000) > verify::score(24, 0));
    assert!(verify::score(24, 4_000) > verify::score(24, 5_000));
    assert!(verify::score(10, 0) > verify::score(9, 0));
}

#[test]
fn idle_run_scores_zero_kills() {
    let outcome: Outcome = verify::play(SEED, &vec![0u8; 100]);
    assert_eq!(outcome.kills, 0);
    assert_eq!(outcome.cleared_at, 0);
    assert_eq!(outcome.score, 0);
}

#[test]
fn killing_every_creature_records_the_clear_tic() {
    let mut game = engine::Game::new(1, 1);
    game.actors[0].x = 5 << 16 | 0x8000;
    game.actors[0].y = 3 << 16 | 0x8000;
    let mut tic = 0;
    while game.kills == 0 && tic < 200 {
        game.tick(input::FIRE);
        tic += 1;
    }
    assert_eq!(game.kills, 1);
    assert_eq!(game.cleared_at, game.tic);
}
