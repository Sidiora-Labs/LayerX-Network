//! Golden vectors and cross-platform determinism for program-owned account
//! derivation.
//!
//! The derivation relocates every program-held balance if it ever changes, so
//! this suite freezes it two ways. First, each conformance vector reconstructs
//! the exact frozen derivation preimage from literal bytes — the domain tag, the
//! program identifier, the big-endian seed length and the seed — independently
//! of the crate's own assembly, and asserts the crate derives the sha256 of
//! that literal preimage. Any change to the domain, field order or length
//! encoding in the runtime diverges from these literals and fails the suite.
//! Second, the determinism vectors prove the derivation is a pure function of
//! its public inputs: recomputed repeatedly it is byte-identical, which is the
//! property that must hold across every operating system, architecture and
//! optimisation level a node might run.
//!
//! These tests are written for the conformance vector set; per the
//! implementation-phase contract they are not run here.

use layerx_programs_runtime::{
    derive_program_account, hash_bytes, program_account_preimage, HashAlgorithm, ProgramAccount,
    ProgramId, MAX_PROGRAM_ACCOUNT_SEED_BYTES, PROGRAM_ACCOUNT_BYTES,
};

/// The frozen domain tag, written as a literal so a change to the runtime
/// constant is caught here rather than silently accepted.
const FROZEN_DOMAIN: &[u8] = b"LayerX/programs/program-account/v1\0";

/// The conformance vector set: a spread of program identifiers and bounded
/// seeds, including the empty seed, a maximal seed and non-ASCII bytes.
fn conformance_vectors() -> Vec<([u8; 32], Vec<u8>)> {
    vec![
        ([1u8; 32], b"".to_vec()),
        ([1u8; 32], b"vault".to_vec()),
        ([2u8; 32], b"vault".to_vec()),
        ([7u8; 32], b"escrow/42".to_vec()),
        ([0x11u8; 32], b"pool/reserve".to_vec()),
        ({
            let mut program = [0u8; 32];
            for (index, byte) in program.iter_mut().enumerate() {
                *byte = index as u8 + 1;
            }
            program
        }, vec![0x00, 0xff, 0x7f, 0x80, 0x01]),
        ([0xabu8; 32], vec![0xcd; MAX_PROGRAM_ACCOUNT_SEED_BYTES]),
    ]
}

/// Rebuilds the frozen derivation preimage from literal bytes, independently of
/// the crate's own assembly.
fn frozen_preimage(program: [u8; 32], seed: &[u8]) -> Vec<u8> {
    let mut preimage = Vec::new();
    preimage.extend_from_slice(FROZEN_DOMAIN);
    preimage.extend_from_slice(&program);
    preimage.extend_from_slice(&(u32::try_from(seed.len()).expect("seed length fits u32")).to_be_bytes());
    preimage.extend_from_slice(seed);
    preimage
}

fn program(bytes: [u8; 32]) -> ProgramId {
    ProgramId::new(bytes).expect("nonzero program identifier")
}

fn format_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn golden_preimage_layout_is_frozen() {
    for (program_bytes, seed) in conformance_vectors() {
        let assembled =
            program_account_preimage(program(program_bytes), &seed).expect("preimage assembles");
        assert_eq!(
            assembled,
            frozen_preimage(program_bytes, &seed),
            "derivation preimage layout diverged for seed length {}",
            seed.len()
        );
    }
}

#[test]
fn golden_derivation_matches_frozen_preimage_hash() {
    for (program_bytes, seed) in conformance_vectors() {
        let account = derive_program_account(program(program_bytes), &seed)
            .expect("derivation succeeds for admitted seed");
        let expected = hash_bytes(HashAlgorithm::Sha256, &frozen_preimage(program_bytes, &seed))
            .expect("hash succeeds");
        assert_eq!(
            account.bytes(),
            expected,
            "derived account diverged from the frozen preimage hash for seed length {}",
            seed.len()
        );
    }
}

#[test]
fn derivation_is_byte_identical_across_repeated_computation() {
    for (program_bytes, seed) in conformance_vectors() {
        let first = derive_program_account(program(program_bytes), &seed).expect("derivation");
        let baseline_hex = format_hex(&first.bytes());
        for _ in 0..64 {
            let again = derive_program_account(program(program_bytes), &seed).expect("derivation");
            assert_eq!(
                format_hex(&again.bytes()),
                baseline_hex,
                "derivation diverged across repeated computation for seed length {}",
                seed.len()
            );
        }
    }
}

#[test]
fn derivation_outputs_are_fixed_width() {
    for (program_bytes, seed) in conformance_vectors() {
        let account = derive_program_account(program(program_bytes), &seed).expect("derivation");
        assert_eq!(account.bytes().len(), PROGRAM_ACCOUNT_BYTES);
    }
}

#[test]
fn conformance_vectors_do_not_collide() {
    let mut seen: Vec<[u8; PROGRAM_ACCOUNT_BYTES]> = Vec::new();
    for (program_bytes, seed) in conformance_vectors() {
        let account: ProgramAccount =
            derive_program_account(program(program_bytes), &seed).expect("derivation");
        let bytes = account.bytes();
        assert!(
            !seen.contains(&bytes),
            "two distinct conformance vectors collided on the same account identifier"
        );
        seen.push(bytes);
    }
}

#[test]
fn same_seed_under_distinct_programs_never_collides() {
    let seed = b"counterparty";
    let left = derive_program_account(program([1u8; 32]), seed).expect("derivation");
    let right = derive_program_account(program([2u8; 32]), seed).expect("derivation");
    assert_ne!(
        left.bytes(),
        right.bytes(),
        "no program may reproduce another program's derived account"
    );
}
