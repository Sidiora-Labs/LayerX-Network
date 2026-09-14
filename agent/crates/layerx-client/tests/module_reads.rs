use layerx_client::evidence::{verify_module_evidence, AccountEvidencePolicy, RootSelector};
use layerx_types::json::{parse, JsonValue};
use layerx_types::verify::VerificationLevel;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fixture() -> Result<(JsonValue, AccountEvidencePolicy), Box<dyn std::error::Error>> {
    let vector = parse(include_str!(
        "../../../../tests/vectors/native-module-evidence.json"
    ))?;
    let policy = AccountEvidencePolicy {
        expected_network_id: 42,
        expected_protocol_version: 3,
        handshake_sequencer_key: vector.hex_array_at("sequencer")?,
        root_selector: RootSelector::Latest,
    };
    Ok((vector, policy))
}

#[test]
fn native_module_proof_is_independently_verified() -> TestResult {
    let (vector, policy) = fixture()?;
    let value = vector.hex_at("value")?;
    let key = vector.hex_at("key")?;
    let proof = vector.hex_at("proof")?;
    let verified = verify_module_evidence(&value, &proof, 0, &key, policy)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(verified.state_root(), vector.hex_array_at("root")?);
    assert_eq!(verified.level(), VerificationLevel::STATE_PROVEN);
    assert_eq!(verified.checkpoint_id(), None);
    let header =
        layerx_wire::receipt::decode_batch_header(&verified.signed_header().canonical_bytes)
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(header.network_id(), 42);
    assert_eq!(header.protocol_version(), 3);
    assert_eq!(
        u64::from_be_bytes(value.as_slice().try_into()?),
        header.last_sequence() + 1
    );
    Ok(())
}

#[test]
fn native_module_evidence_refuses_malformed_or_substituted_fields() -> TestResult {
    let (vector, policy) = fixture()?;
    let value = vector.hex_at("value")?;
    let key = vector.hex_at("key")?;
    let proof = vector.hex_at("proof")?;
    for length in 0..proof.len() {
        assert!(verify_module_evidence(&value, &proof[..length], 0, &key, policy).is_err());
    }
    let mut altered = proof.clone();
    altered.push(0);
    assert!(verify_module_evidence(&value, &altered, 0, &key, policy).is_err());
    for position in [0, 1, 2, 3, 4, 5, 6, 7, 8, proof.len() - 2, proof.len() - 1] {
        altered = proof.clone();
        altered[position] ^= 1;
        assert!(verify_module_evidence(&value, &altered, 0, &key, policy).is_err());
    }
    for position in 0..value.len() {
        let mut changed = value.clone();
        changed[position] ^= 1;
        assert!(verify_module_evidence(&changed, &proof, 0, &key, policy).is_err());
    }
    for module in 1..=10 {
        assert!(verify_module_evidence(&value, &proof, module, &key, policy).is_err());
    }
    assert!(verify_module_evidence(&value, &proof, 0, b"other", policy).is_err());
    for selector in [RootSelector::Batch(1), RootSelector::Checkpoint([1; 32])] {
        assert!(verify_module_evidence(
            &value,
            &proof,
            0,
            &key,
            AccountEvidencePolicy {
                root_selector: selector,
                ..policy
            }
        )
        .is_err());
    }
    Ok(())
}

#[test]
fn native_module_evidence_refuses_foreign_network_version_or_signer() -> TestResult {
    let (vector, policy) = fixture()?;
    let value = vector.hex_at("value")?;
    let key = vector.hex_at("key")?;
    let proof = vector.hex_at("proof")?;
    for network in [0, 41, 43] {
        assert!(verify_module_evidence(
            &value,
            &proof,
            0,
            &key,
            AccountEvidencePolicy {
                expected_network_id: network,
                ..policy
            }
        )
        .is_err());
    }
    for version in [0, 1, 2, 4] {
        assert!(verify_module_evidence(
            &value,
            &proof,
            0,
            &key,
            AccountEvidencePolicy {
                expected_protocol_version: version,
                ..policy
            }
        )
        .is_err());
    }
    for signer in [[0; 32], [1; 32]] {
        assert!(verify_module_evidence(
            &value,
            &proof,
            0,
            &key,
            AccountEvidencePolicy {
                handshake_sequencer_key: signer,
                ..policy
            }
        )
        .is_err());
    }
    Ok(())
}
