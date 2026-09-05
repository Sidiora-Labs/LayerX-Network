use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::receipt::{
    canonical_protocol_facts, verify, AuthorizedBatch, ReceiptCheck, VerificationFailure,
};
use layerx_types::verify::VerificationLevel;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::receipt_digest;
use layerx_wire::limits::{LEGACY_PROTOCOL_VERSION, PROTOCOL_VERSION};

#[derive(Clone)]
struct Fields {
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    batch_id: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    from_before: u128,
    from_after: u128,
    to: [u8; 32],
    to_before: u128,
    to_after: u128,
}

fn fields() -> Fields {
    Fields {
        activity_id: [1; 32],
        previous_state_root: [2; 32],
        resulting_state_root: [3; 32],
        batch_id: [4; 32],
        asset: [5; 32],
        amount: 25,
        from: [6; 32],
        from_before: 100,
        from_after: 75,
        to: [7; 32],
        to_before: 10,
        to_after: 35,
    }
}

fn encode_fields_version(
    fields: &Fields,
    signature: Option<[u8; 64]>,
    protocol_version: u16,
) -> Vec<u8> {
    let mut encoder = Encoder::new(4096);
    assert_eq!(
        encoder.structure_header_version(0x5201, protocol_version),
        Ok(())
    );
    assert_eq!(encoder.u16(protocol_version), Ok(()));
    assert_eq!(encoder.bytes(&fields.activity_id, 32), Ok(()));
    assert_eq!(encoder.u64(9), Ok(()));
    assert_eq!(encoder.bytes(&fields.previous_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&fields.resulting_state_root, 32), Ok(()));
    assert_eq!(encoder.bytes(&[8; 32], 32), Ok(()));
    assert_eq!(encoder.i32(0), Ok(()));
    assert_eq!(encoder.sequence_length(0, 512), Ok(()));
    assert_eq!(encoder.u128(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.batch_id, 32), Ok(()));
    assert_eq!(encoder.u16(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u32(1), Ok(()));
    assert_eq!(encoder.u8(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.asset, 32), Ok(()));
    assert_eq!(encoder.u128(fields.amount), Ok(()));
    assert_eq!(encoder.bytes(&fields.from, 32), Ok(()));
    assert_eq!(encoder.u128(fields.from_before), Ok(()));
    assert_eq!(encoder.u128(fields.from_after), Ok(()));
    assert_eq!(encoder.u64(1), Ok(()));
    assert_eq!(encoder.bytes(&fields.to, 32), Ok(()));
    assert_eq!(encoder.u128(fields.to_before), Ok(()));
    assert_eq!(encoder.u128(fields.to_after), Ok(()));
    assert_eq!(encoder.bytes(&[9; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[10; 32], 32), Ok(()));
    assert_eq!(encoder.bytes(&[11; 32], 32), Ok(()));
    assert_eq!(encoder.u64(1_000), Ok(()));
    assert_eq!(encoder.u8(u8::from(signature.is_some())), Ok(()));
    if let Some(value) = signature {
        assert_eq!(encoder.bytes(&value, 64), Ok(()));
    }
    encoder.finish()
}

fn encode_fields(fields: &Fields, signature: Option<[u8; 64]>) -> Vec<u8> {
    encode_fields_version(fields, signature, PROTOCOL_VERSION)
}

fn sign(fields: &Fields, signing_key: &SigningKey) -> Vec<u8> {
    let unsigned = encode_fields(fields, None);
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt hashing failed: {error:?}"));
    encode_fields(fields, Some(signing_key.sign(&digest).to_bytes()))
}

fn sign_version(fields: &Fields, signing_key: &SigningKey, protocol_version: u16) -> Vec<u8> {
    let unsigned = encode_fields_version(fields, None, protocol_version);
    let digest = receipt_digest(&unsigned)
        .unwrap_or_else(|error| panic!("receipt hashing failed: {error:?}"));
    encode_fields_version(
        fields,
        Some(signing_key.sign(&digest).to_bytes()),
        protocol_version,
    )
}

fn authorised(fields: &Fields, signing_key: &SigningKey) -> AuthorizedBatch {
    AuthorizedBatch::new(
        fields.batch_id,
        fields.asset,
        fields.previous_state_root,
        fields.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    )
}

#[test]
fn verifies_from_bytes_with_no_ambient_capabilities() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let fields = fields();
    let bytes = sign(&fields, &signing_key);
    let verified = verify(&bytes, &authorised(&fields, &signing_key))
        .unwrap_or_else(|error| panic!("valid core receipt rejected: {error:?}"));
    assert_eq!(verified.canonical_bytes(), bytes);
    assert_eq!(verified.level(), VerificationLevel::SEQUENCER_SIGNED);
    let facts = canonical_protocol_facts(verified.canonical_bytes())
        .unwrap_or_else(|error| panic!("verified receipt facts rejected: {error:?}"));
    assert_eq!(facts.result_code(), 0);
    assert_eq!(facts.asset(), fields.asset);
    assert_eq!(facts.amount(), fields.amount);
    assert_eq!(facts.fee_charged(), 1);
}

#[test]
fn verifies_current_occupancy_protocol_receipts() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let fields = fields();
    let bytes = sign_version(&fields, &signing_key, PROTOCOL_VERSION);
    let verified = verify(&bytes, &authorised(&fields, &signing_key))
        .unwrap_or_else(|error| panic!("valid protocol v2 receipt rejected: {error:?}"));
    assert_eq!(verified.canonical_bytes(), bytes);
    assert_eq!(
        verified
            .receipt()
            .protocol()
            .map(layerx_wire::receipt::ProtocolReceipt::protocol_version),
        Some(PROTOCOL_VERSION)
    );
}

#[test]
fn legacy_receipts_decode_but_are_not_accepted_as_beta_evidence() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let fields = fields();
    let bytes = sign_version(&fields, &signing_key, LEGACY_PROTOCOL_VERSION);
    assert_eq!(
        verify(&bytes, &authorised(&fields, &signing_key)),
        Err(VerificationFailure {
            check: ReceiptCheck::ProtocolVersion,
        })
    );
}

#[test]
fn rejects_altered_values_and_wrong_authority() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let wrong_key = SigningKey::from_bytes(&[4; 32]);
    let original = fields();
    let original_signature = {
        let unsigned = encode_fields(&original, None);
        let digest = receipt_digest(&unsigned)
            .unwrap_or_else(|error| panic!("receipt hashing failed: {error:?}"));
        signing_key.sign(&digest).to_bytes()
    };

    let mut altered_amount = original.clone();
    altered_amount.amount = 24;
    altered_amount.from_after = 76;
    altered_amount.to_after = 34;
    assert_eq!(
        verify(
            &encode_fields(&altered_amount, Some(original_signature)),
            &authorised(&original, &signing_key),
        ),
        Err(VerificationFailure {
            check: ReceiptCheck::SequencerSignature,
        })
    );

    let mut altered_recipient = original.clone();
    altered_recipient.to[0] ^= 1;
    assert_eq!(
        verify(
            &encode_fields(&altered_recipient, Some(original_signature)),
            &authorised(&original, &signing_key),
        ),
        Err(VerificationFailure {
            check: ReceiptCheck::SequencerSignature,
        })
    );

    assert_eq!(
        verify(
            &sign(&original, &wrong_key),
            &authorised(&original, &signing_key)
        ),
        Err(VerificationFailure {
            check: ReceiptCheck::SequencerSignature,
        })
    );
}

#[test]
fn names_balance_asset_and_state_chain_failures() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let original = fields();

    let mut bad_debit = original.clone();
    bad_debit.from_after = 76;
    assert_eq!(
        verify(
            &sign(&bad_debit, &signing_key),
            &authorised(&bad_debit, &signing_key)
        ),
        Err(VerificationFailure {
            check: ReceiptCheck::DebitBalance,
        })
    );

    let mut bad_credit = original.clone();
    bad_credit.to_after = 36;
    assert_eq!(
        verify(
            &sign(&bad_credit, &signing_key),
            &authorised(&bad_credit, &signing_key)
        ),
        Err(VerificationFailure {
            check: ReceiptCheck::CreditBalance,
        })
    );

    let bytes = sign(&original, &signing_key);
    let wrong_asset = AuthorizedBatch::new(
        original.batch_id,
        [99; 32],
        original.previous_state_root,
        original.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    );
    assert_eq!(
        verify(&bytes, &wrong_asset),
        Err(VerificationFailure {
            check: ReceiptCheck::Asset,
        })
    );
    let wrong_roots = AuthorizedBatch::new(
        original.batch_id,
        original.asset,
        [98; 32],
        original.resulting_state_root,
        signing_key.verifying_key().to_bytes(),
    );
    assert_eq!(
        verify(&bytes, &wrong_roots),
        Err(VerificationFailure {
            check: ReceiptCheck::PreviousStateRoot,
        })
    );
}

#[test]
fn verifies_state_commitment_receipt_and_keeps_root_and_signature_checks() {
    let signing_key = SigningKey::from_bytes(&[3; 32]);
    let fields = fields();
    let version = layerx_wire::limits::STATE_COMMITMENT_PROTOCOL_VERSION;
    let bytes = sign_version(&fields, &signing_key, version);
    let Ok(verified) = verify(&bytes, &authorised(&fields, &signing_key)) else {
        panic!("version three signed receipt rejected");
    };
    assert_eq!(verified.canonical_bytes(), bytes);
    let mut wrong_root = fields.clone();
    wrong_root.resulting_state_root[0] ^= 1;
    assert_eq!(
        verify(&bytes, &authorised(&wrong_root, &signing_key)),
        Err(VerificationFailure {
            check: ReceiptCheck::ResultingStateRoot
        })
    );
    let mut altered = bytes;
    let last = altered.len() - 1;
    altered[last] ^= 1;
    assert_eq!(
        verify(&altered, &authorised(&fields, &signing_key)),
        Err(VerificationFailure {
            check: ReceiptCheck::SequencerSignature
        })
    );
}
