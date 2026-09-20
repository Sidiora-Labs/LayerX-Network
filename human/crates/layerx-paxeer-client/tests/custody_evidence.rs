use std::path::{Path, PathBuf};

use layerx_paxeer_client::{
    NativeCustodyCredit, NativeCustodyExpectation, NATIVE_CUSTODY_CREDIT_HEAD_BYTES,
    NATIVE_CUSTODY_PROFILE_BYTES,
};
use sha2::{Digest as _, Sha256};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root"))
        .join("tests/fixtures/custody/paxeer-light-v1")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn expectation(profile: &[u8], credit: &[u8]) -> NativeCustodyExpectation {
    NativeCustodyExpectation {
        network_id: u32::from_be_bytes(
            profile[201..205]
                .try_into()
                .unwrap_or_else(|_| panic!("network")),
        ),
        beneficiary: credit[107..139]
            .try_into()
            .unwrap_or_else(|_| panic!("beneficiary")),
        owner_key: credit[139..171]
            .try_into()
            .unwrap_or_else(|_| panic!("owner")),
    }
}

#[test]
fn real_light_credit_preserves_typed_evidence_and_refusals() {
    let profile = read("custody.profile");
    let credit = read("custody.credit");
    assert_eq!(profile.len(), NATIVE_CUSTODY_PROFILE_BYTES);
    assert_eq!(&profile[..5], b"LXBC3");
    assert_eq!(&credit[..5], b"LXDC3");
    assert_eq!(
        &credit[NATIVE_CUSTODY_CREDIT_HEAD_BYTES..NATIVE_CUSTODY_CREDIT_HEAD_BYTES + 5],
        b"LXLB1"
    );
    let did = String::from_utf8(read("did.txt")).unwrap_or_else(|error| panic!("{error}"));
    let did = did.trim();
    let owner = &credit[139..171];
    let owner_hex: String = owner.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(did, format!("did:layerx:{owner_hex}"));
    let account = layerx_types::account::AccountId::parse(&format!("agent:{did}:main"))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let beneficiary = layerx_paxeer_client::account_address_for_protocol(&account, 3)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&credit[107..139], beneficiary.as_slice());
    let expected = expectation(&profile, &credit);
    let decoded = NativeCustodyCredit::verify(&profile, &credit, expected)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let evidence = decoded.evidence();
    assert_eq!(evidence.header_height, evidence.state_height + 1);
    assert!(
        evidence.state_height
            >= u64::from_be_bytes(
                profile[161..169]
                    .try_into()
                    .unwrap_or_else(|_| panic!("trusted height"))
            )
    );
    assert_eq!(
        evidence.proof_bundle_hash,
        <[u8; 32]>::from(Sha256::digest(decoded.bundle_bytes()))
    );
    assert_eq!(decoded.canonical_bytes(), credit.as_slice());
    assert_eq!(
        decoded.head_bytes(),
        &credit[..NATIVE_CUSTODY_CREDIT_HEAD_BYTES]
    );
    let nullifier = String::from_utf8(read("custody.credit.nullifier"))
        .unwrap_or_else(|error| panic!("{error}"));
    let decoded_nullifier: String = decoded
        .nullifier()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(nullifier.trim(), decoded_nullifier);
    for index in (0..credit.len()).filter(|index| !(223..255).contains(index)) {
        let mut changed = credit.clone();
        changed[index] ^= 1;
        assert!(
            NativeCustodyCredit::verify(&profile, &changed, expected).is_err(),
            "credit {index}"
        );
    }
    for index in 0..profile.len() {
        let mut changed = profile.clone();
        changed[index] ^= 1;
        assert!(
            NativeCustodyCredit::verify(&changed, &credit, expected).is_err(),
            "profile {index}"
        );
    }
    assert!(NativeCustodyCredit::verify(
        &profile,
        &credit[..NATIVE_CUSTODY_CREDIT_HEAD_BYTES],
        expected
    )
    .is_err());
    let mut trailing = credit.clone();
    trailing.push(0);
    assert!(NativeCustodyCredit::verify(&profile, &trailing, expected).is_err());
    let mut wrong_owner = expected;
    wrong_owner.owner_key[0] ^= 1;
    assert!(NativeCustodyCredit::verify(&profile, &credit, wrong_owner).is_err());
    let mut wrong_network = expected;
    wrong_network.network_id += 1;
    assert!(NativeCustodyCredit::verify(&profile, &credit, wrong_network).is_err());
    for retired in [b"LXDC1", b"LXDC2"] {
        let mut changed = credit.clone();
        changed[..5].copy_from_slice(retired);
        assert!(NativeCustodyCredit::verify(&profile, &changed, expected).is_err());
    }
    for retired in ["paxeer-state-v2", "paxeer-state-v2/history-window"] {
        let directory = fixtures().join("..").join(retired);
        let old_profile = std::fs::read(directory.join("custody.profile"))
            .unwrap_or_else(|error| panic!("{error}"));
        let old_credit = std::fs::read(directory.join("custody.credit"))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(NativeCustodyCredit::verify(
            &old_profile,
            &old_credit,
            expectation(&old_profile, &old_credit)
        )
        .is_err());
    }
}

#[test]
fn real_light_credit_profiles_and_later_proofs_stay_bound() {
    let profile = read("custody.profile");
    let credit = read("custody.credit");
    let adjacent_profile = read("custody-adjacent.profile");
    let adjacent = read("custody-adjacent.credit");
    let later = read("custody-later.credit");
    let first = NativeCustodyCredit::verify(&profile, &credit, expectation(&profile, &credit))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let adjacent_decoded = NativeCustodyCredit::verify(
        &adjacent_profile,
        &adjacent,
        expectation(&adjacent_profile, &adjacent),
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    let later_decoded =
        NativeCustodyCredit::verify(&profile, &later, expectation(&profile, &later))
            .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(first.custody(), adjacent_decoded.custody());
    assert_eq!(first.custody(), later_decoded.custody());
    assert_eq!(first.nullifier(), later_decoded.nullifier());
    assert_eq!(first.bundle_bytes(), adjacent_decoded.bundle_bytes());
    assert_eq!(
        u64::from_be_bytes(
            adjacent_profile[161..169]
                .try_into()
                .unwrap_or_else(|_| panic!("trusted height"))
        ),
        adjacent_decoded.evidence().state_height
    );
    assert!(later_decoded.evidence().header_height > first.evidence().header_height);
    assert!(NativeCustodyCredit::verify(
        &adjacent_profile,
        &credit,
        expectation(&profile, &credit)
    )
    .is_err());
    assert!(
        NativeCustodyCredit::verify(&profile, &adjacent, expectation(&profile, &adjacent)).is_err()
    );
}
