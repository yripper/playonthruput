//! Run tapes: one input byte per tic between a fixed header and footer.
//!
//! ```text
//! header (20 B): "DLT1" | version u8 | level u8 | reserved u16 = 0
//!                | seed_id u32 | seed u32 | tics u32          (all LE)
//! inputs (tics B): one `input::*` bitmask per tic
//! footer (8 B):  claimed score u32 | claimed final state hash u32
//! ```
//!
//! A 10-minute run is 21,028 bytes, which fits one transaction's calldata.

pub const MAGIC: [u8; 4] = *b"DLT1";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 20;
pub const FOOTER_LEN: usize = 8;
/// 10 minutes at 35 tics per second.
pub const MAX_TICS: u32 = 21_000;
pub const MAX_TAPE_LEN: usize = HEADER_LEN + MAX_TICS as usize + FOOTER_LEN;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    pub level: u8,
    pub seed_id: u32,
    pub seed: u32,
    pub tics: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Footer {
    pub score: u32,
    pub state_hash: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TapeError {
    TooShort,
    BadMagic,
    BadVersion,
    ReservedNonZero,
    TicsOutOfRange,
    LengthMismatch,
}

pub struct Tape<'a> {
    pub header: Header,
    pub inputs: &'a [u8],
    pub footer: Footer,
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Container checks only; no simulation. Cheap enough to run before any
/// storage reads.
pub fn parse(tape: &[u8]) -> Result<Tape<'_>, TapeError> {
    if tape.len() < HEADER_LEN + FOOTER_LEN {
        return Err(TapeError::TooShort);
    }
    if tape[..4] != MAGIC {
        return Err(TapeError::BadMagic);
    }
    if tape[4] != VERSION {
        return Err(TapeError::BadVersion);
    }
    if tape[6] != 0 || tape[7] != 0 {
        return Err(TapeError::ReservedNonZero);
    }
    let header = Header {
        level: tape[5],
        seed_id: u32_at(tape, 8),
        seed: u32_at(tape, 12),
        tics: u32_at(tape, 16),
    };
    if header.tics == 0 || header.tics > MAX_TICS {
        return Err(TapeError::TicsOutOfRange);
    }
    let inputs_end = HEADER_LEN + header.tics as usize;
    if tape.len() != inputs_end + FOOTER_LEN {
        return Err(TapeError::LengthMismatch);
    }
    Ok(Tape {
        header,
        inputs: &tape[HEADER_LEN..inputs_end],
        footer: Footer {
            score: u32_at(tape, inputs_end),
            state_hash: u32_at(tape, inputs_end + 4),
        },
    })
}

/// Writes a tape into `out` and returns its length. `out` must hold
/// `HEADER_LEN + inputs.len() + FOOTER_LEN` bytes.
pub fn encode(header: &Header, inputs: &[u8], footer: &Footer, out: &mut [u8]) -> usize {
    let len = HEADER_LEN + inputs.len() + FOOTER_LEN;
    out[..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = header.level;
    out[6] = 0;
    out[7] = 0;
    out[8..12].copy_from_slice(&header.seed_id.to_le_bytes());
    out[12..16].copy_from_slice(&header.seed.to_le_bytes());
    out[16..20].copy_from_slice(&(inputs.len() as u32).to_le_bytes());
    out[HEADER_LEN..HEADER_LEN + inputs.len()].copy_from_slice(inputs);
    let f = HEADER_LEN + inputs.len();
    out[f..f + 4].copy_from_slice(&footer.score.to_le_bytes());
    out[f + 4..f + 8].copy_from_slice(&footer.state_hash.to_le_bytes());
    len
}
