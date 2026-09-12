use std::path::Path;
use std::process::Command;

#[test]
fn real_weth_protocol_three_custody_evidence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root"));
    let output = root
        .join("qual-logs")
        .join(format!("custody-evidence-{}", std::process::id()));
    let status = Command::new("python3")
        .arg(root.join("tests/bridge/local_credit_evidence.py"))
        .arg("--output")
        .arg(&output)
        .status()
        .unwrap_or_else(|error| panic!("custody evidence producer: {error}"));
    assert!(status.success(), "real custody evidence failed: {status}");
    let read = |name| std::fs::read(output.join(name)).unwrap_or_else(|error| panic!("{error}"));
    let profile = read("custody.profile");
    let credit = read("custody.credit");
    assert_eq!(profile.len(), 207);
    assert_eq!(credit.len(), 427);
    let identity =
        String::from_utf8(read("identity.json")).unwrap_or_else(|error| panic!("{error}"));
    let identity =
        layerx_paxeer_client::parse_json(&identity).unwrap_or_else(|error| panic!("{error:?}"));
    let did = identity
        .member("did")
        .and_then(layerx_paxeer_client::Json::as_text)
        .unwrap_or_else(|| panic!("actor DID"));
    let account = layerx_types::account::AccountId::parse(&format!("agent:{did}:main"))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let beneficiary = layerx_paxeer_client::account_address_for_protocol(&account, 3)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&credit[107..139], beneficiary.as_slice());
    let public: [u8; 32] = profile[65..97]
        .try_into()
        .unwrap_or_else(|error| panic!("{error}"));
    let authority =
        ed25519_dalek::VerifyingKey::from_bytes(&public).unwrap_or_else(|error| panic!("{error}"));
    let signature = ed25519_dalek::Signature::from_slice(&credit[363..])
        .unwrap_or_else(|error| panic!("{error}"));
    let mut message = b"LX:CUSTODY:CREDIT:v1".to_vec();
    message.extend_from_slice(&credit[..363]);
    authority
        .verify_strict(&message, &signature)
        .unwrap_or_else(|error| panic!("{error}"));
    let decoded = layerx_paxeer_client::AttestedNativeCustodyCredit::verify(
        &profile,
        &credit,
        layerx_paxeer_client::NativeCustodyExpectation {
            network_id: u32::from_be_bytes(
                profile[201..205]
                    .try_into()
                    .unwrap_or_else(|_| panic!("network")),
            ),
            beneficiary,
            owner_key: credit[139..171]
                .try_into()
                .unwrap_or_else(|_| panic!("owner")),
        },
    )
    .unwrap_or_else(|error| panic!("native custody decoder: {error:?}"));
    assert!(matches!(
        decoded.evidence(),
        layerx_paxeer_client::NativeCustodyEvidence::EthereumReceipt { .. }
    ));
    assert_eq!(decoded.canonical_bytes().as_slice(), credit.as_slice());
    message[20] ^= 1;
    assert!(authority.verify_strict(&message, &signature).is_err());
}

#[test]
fn real_comet_state_credit_preserves_typed_evidence_and_refusals() {
    use ed25519_dalek::{Signer as _, SigningKey};
    use layerx_paxeer_client::{
        AttestedNativeCustodyCredit, NativeCustodyEvidence, NativeCustodyExpectation,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap_or_else(|| panic!("repository root"));
    let fixtures = root.join("tests/fixtures/custody/paxeer-state-v2");
    let profile =
        std::fs::read(fixtures.join("custody.profile")).unwrap_or_else(|error| panic!("{error}"));
    let credit =
        std::fs::read(fixtures.join("custody.credit")).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(&profile[..5], b"LXBC2");
    assert_eq!(&credit[..5], b"LXDC2");
    let expected = NativeCustodyExpectation {
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
    };
    let decoded = AttestedNativeCustodyCredit::verify(&profile, &credit, expected)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(matches!(
        decoded.evidence(),
        NativeCustodyEvidence::CometState { .. }
    ));
    assert_eq!(decoded.canonical_bytes().as_slice(), credit.as_slice());
    for index in 0..credit.len() {
        let mut changed = credit.clone();
        changed[index] ^= 1;
        assert!(
            AttestedNativeCustodyCredit::verify(&profile, &changed, expected).is_err(),
            "credit {index}"
        );
    }
    for index in 0..profile.len() {
        let mut changed = profile.clone();
        changed[index] ^= 1;
        assert!(
            AttestedNativeCustodyCredit::verify(&changed, &credit, expected).is_err(),
            "profile {index}"
        );
    }
    let signer = SigningKey::from_bytes(&[0x55; 32]);
    assert_eq!(signer.verifying_key().as_bytes(), &profile[65..97]);
    for offset in [4, 215, 287, 359, 362] {
        let mut changed = credit.clone();
        changed[offset] ^= 1;
        let mut message = b"LX:CUSTODY:CREDIT:v2".to_vec();
        message.extend_from_slice(&changed[..363]);
        changed[363..].copy_from_slice(&signer.sign(&message).to_bytes());
        assert!(
            AttestedNativeCustodyCredit::verify(&profile, &changed, expected).is_err(),
            "signed field {offset}"
        );
    }
}
