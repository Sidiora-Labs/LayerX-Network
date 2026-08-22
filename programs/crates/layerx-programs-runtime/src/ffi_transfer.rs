//! Scalar-only C bridge into the programs monetary-law validator.

use crate::{PrincipalId, ProgramId};

const RESULT_OK: i32 = 0;
const RESULT_NON_CANONICAL: i32 = -3;

fn bytes(words: [u64; 4]) -> [u8; 32] {
    let mut result = [0; 32];
    for (index, word) in words.into_iter().enumerate() {
        result[index * 8..index * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    result
}

/// Authorizes one 402LXP leg without exposing a balance mutation surface.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn layerx_programs_authorize_402lxp_leg(
    p0: u64,
    p1: u64,
    p2: u64,
    p3: u64,
    r0: u64,
    r1: u64,
    r2: u64,
    r3: u64,
    h0: u64,
    h1: u64,
    h2: u64,
    h3: u64,
    a0: u64,
    a1: u64,
    a2: u64,
    a3: u64,
    t0: u64,
    t1: u64,
    t2: u64,
    t3: u64,
    amount_hi: u64,
    amount_lo: u64,
) -> i32 {
    let Ok(program) = ProgramId::new(bytes([p0, p1, p2, p3])) else {
        return RESULT_NON_CANONICAL;
    };
    let Ok(principal) = PrincipalId::new(bytes([r0, r1, r2, r3])) else {
        return RESULT_NON_CANONICAL;
    };
    let amount = (u128::from(amount_hi) << 64) | u128::from(amount_lo);
    // The C transition has already authenticated `principal` against its
    // resolved authority and rejects a forged payer before reaching this ABI.
    // Ordinal-5 remains that direct authenticated kernel path; it must not
    // mint a guest capability or manufacture a second settlement authority.
    if program.bytes() == [0; 32]
        || principal.bytes() == [0; 32]
        || bytes([h0, h1, h2, h3]) == [0; 32]
        || bytes([a0, a1, a2, a3]) == [0; 32]
        || bytes([t0, t1, t2, t3]) == [0; 32]
        || amount == 0
    {
        return RESULT_NON_CANONICAL;
    }
    RESULT_OK
}
