//! Keccak-256 as used by the EVM for event topics and function selectors.

const RATE: usize = 136;

const ROUND_CONSTANTS: [u64; 24] = [
    0x0000_0000_0000_0001,
    0x0000_0000_0000_8082,
    0x8000_0000_0000_808a,
    0x8000_0000_8000_8000,
    0x0000_0000_0000_808b,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8009,
    0x0000_0000_0000_008a,
    0x0000_0000_0000_0088,
    0x0000_0000_8000_8009,
    0x0000_0000_8000_000a,
    0x0000_0000_8000_808b,
    0x8000_0000_0000_008b,
    0x8000_0000_0000_8089,
    0x8000_0000_0000_8003,
    0x8000_0000_0000_8002,
    0x8000_0000_0000_0080,
    0x0000_0000_0000_800a,
    0x8000_0000_8000_000a,
    0x8000_0000_8000_8081,
    0x8000_0000_0000_8080,
    0x0000_0000_8000_0001,
    0x8000_0000_8000_8008,
];

const ROTATIONS: [u32; 25] = [
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
];

fn permute(state: &mut [u64; 25]) {
    for constant in ROUND_CONSTANTS {
        let columns: [u64; 5] = core::array::from_fn(|x| {
            state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20]
        });
        let mix: [u64; 5] =
            core::array::from_fn(|x| columns[(x + 4) % 5] ^ columns[(x + 1) % 5].rotate_left(1));
        for (index, lane) in state.iter_mut().enumerate() {
            *lane ^= mix[index % 5];
        }
        let mut moved = [0_u64; 25];
        for (index, lane) in state.iter().enumerate() {
            let (x, y) = (index % 5, index / 5);
            moved[y + 5 * ((2 * x + 3 * y) % 5)] = lane.rotate_left(ROTATIONS[index]);
        }
        *state = core::array::from_fn(|index| {
            let (x, row) = (index % 5, index - index % 5);
            moved[index] ^ (!moved[row + (x + 1) % 5] & moved[row + (x + 2) % 5])
        });
        state[0] ^= constant;
    }
}

fn absorb(state: &mut [u64; 25], block: &[u8]) {
    for (lane, chunk) in state.iter_mut().zip(block.chunks_exact(8)) {
        let mut word = [0_u8; 8];
        word.copy_from_slice(chunk);
        *lane ^= u64::from_le_bytes(word);
    }
    permute(state);
}

/// Returns the Keccak-256 digest of `input`.
#[must_use]
pub fn keccak256(input: &[u8]) -> [u8; 32] {
    let mut state = [0_u64; 25];
    let mut blocks = input.chunks_exact(RATE);
    for block in &mut blocks {
        absorb(&mut state, block);
    }
    let rest = blocks.remainder();
    let mut last = [0_u8; RATE];
    last[..rest.len()].copy_from_slice(rest);
    last[rest.len()] ^= 0x01;
    last[RATE - 1] ^= 0x80;
    absorb(&mut state, &last);
    let mut digest = [0_u8; 32];
    for (out, lane) in digest.chunks_exact_mut(8).zip(state) {
        out.copy_from_slice(&lane.to_le_bytes());
    }
    digest
}
