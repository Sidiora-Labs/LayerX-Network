use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::receipt::{
    verify_outcome, verify_sequencer_signature, withdrawal, AuthorizedBatch,
};

const RECEIPT: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/receipt");
const ACTIVITY: &[u8] =
    include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/activity.lxa");
const KEY: [u8; 32] =
    *include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/sequencer.public");

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|error| format!("{error:?}"))
}

fn activity_facts(key: [u8; 32]) -> Result<AuthorizedBatch, String> {
    let receipt = checked(layerx_wire::receipt::decode(RECEIPT))?;
    let protocol = receipt.protocol().ok_or("protocol receipt required")?;
    Ok(AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key,
    ))
}

fn proof(bytes: &[u8]) -> Result<layerx_proof::merkle::Proof, String> {
    let path = checked(layerx_wire::receipt::decode_merkle_proof(bytes))?;
    checked(layerx_proof::merkle::Proof::new(
        path.leaf_index(),
        path.leaf_count(),
        path.siblings().to_vec(),
    ))
}

#[test]
fn actual_paid_withdrawal_binds_original_request_and_complete_batch() -> Result<(), String> {
    let header_bytes =
        include_bytes!("../../../../tests/fixtures/asset/bound-native-withdrawal/header");
    let header = checked(layerx_wire::receipt::decode_batch_header(header_bytes))?;
    let receipt = checked(verify_sequencer_signature(RECEIPT, KEY))?;
    let protocol = receipt.protocol().ok_or("protocol receipt required")?;
    assert_eq!(
        (
            protocol.module_id(),
            protocol.operation(),
            protocol.result_code()
        ),
        (1, 9, 0)
    );
    assert_eq!((protocol.amount(), protocol.fee_charged()), (1, 17));
    assert_eq!(protocol.effects().len(), 2);
    let authorization = layerx_proof::inclusion::SequencerAuthorization::new(
        header.sequencer_id(),
        KEY,
        header.batch_number(),
        header.batch_number(),
    );
    let receipt_proof = proof(include_bytes!(
        "../../../../tests/fixtures/asset/bound-native-withdrawal/receipt.proof"
    ))?;
    let maintenance_proof = proof(include_bytes!(
        "../../../../tests/fixtures/asset/bound-native-withdrawal/maintenance.proof"
    ))?;
    let evidence = layerx_proof::receipt::MaintainedOutcomeEvidence {
        header: header_bytes,
        header_signature: include_bytes!(
            "../../../../tests/fixtures/asset/bound-native-withdrawal/header.signature"
        ),
        activity_proof: &receipt_proof,
        maintenance: include_bytes!(
            "../../../../tests/fixtures/asset/bound-native-withdrawal/maintenance.receipt"
        ),
        maintenance_proof: &maintenance_proof,
        authorization: &authorization,
    };
    let batch = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        KEY,
    );
    let selected = checked(
        layerx_proof::receipt::authorized_maintained_activity_batch_chain(
            RECEIPT,
            &batch,
            &evidence,
            &[RECEIPT.to_vec()],
        ),
    )?;
    assert!(verify_outcome(RECEIPT, &selected).is_ok());
    assert!(withdrawal::verify(RECEIPT, &selected, ACTIVITY, header.network_id()).is_ok());
    assert!(withdrawal::verify(RECEIPT, &selected, ACTIVITY, header.network_id() ^ 1).is_err());
    let mut changed = ACTIVITY.to_vec();
    *changed.last_mut().ok_or("activity signature required")? ^= 1;
    assert!(withdrawal::verify(RECEIPT, &selected, &changed, header.network_id()).is_err());
    assert!(
        layerx_proof::receipt::authorized_maintained_activity_batch_chain(
            RECEIPT,
            &batch,
            &evidence,
            &[],
        )
        .is_err()
    );
    Ok(())
}

fn changed_event(offset: usize) -> Result<Vec<u8>, String> {
    let decoded = checked(layerx_wire::receipt::decode(RECEIPT))?;
    let protocol = decoded.protocol().ok_or("protocol receipt required")?;
    let body = protocol
        .effects()
        .get(1)
        .ok_or("withdrawal event required")?
        .body();
    assert_eq!(body.len(), 254);
    let mut unsigned = checked(layerx_wire::receipt::encode_unsigned(&decoded))?;
    let locations = unsigned
        .windows(body.len())
        .enumerate()
        .filter_map(|(position, value)| (value == body).then_some(position))
        .collect::<Vec<_>>();
    let [start] = locations.as_slice() else {
        return Err("unique event required".to_owned());
    };
    unsigned[start + offset] ^= 1;
    if offset == 130 {
        let mut payload = [0; 108];
        payload[..32].copy_from_slice(&body[70..102]);
        payload[32..48].copy_from_slice(&body[102..118]);
        payload[48..68].copy_from_slice(&unsigned[start + 130..start + 150]);
        payload[68..100].copy_from_slice(&body[150..182]);
        payload[100..].copy_from_slice(&body[246..]);
        let kind = checked(layerx_types::payload::ActivityType::new(
            layerx_types::payload::ModuleId::Asset,
            9,
        ))?;
        let registry = checked(withdrawal::registry())?;
        let value = checked(layerx_types::payload::Payload::new(
            &registry, kind, &payload,
        ))?;
        let digest = checked(layerx_wire::hash::payload_hash_for(&value))?;
        unsigned[start + 182..start + 214].copy_from_slice(&digest);
    }
    let digest = checked(layerx_wire::hash::receipt_digest(&unsigned))?;
    let key = SigningKey::from_bytes(&[0x39; 32]);
    assert_eq!(unsigned.pop(), Some(0));
    let mut signature = layerx_wire::encode::Encoder::new(69);
    checked(signature.u8(1))?;
    checked(signature.bytes(&key.sign(&digest).to_bytes(), 64))?;
    unsigned.extend_from_slice(&signature.finish());
    Ok(unsigned)
}

#[test]
fn signed_codec_mutations_cannot_rebind_a_native_withdrawal() -> Result<(), String> {
    let header = checked(layerx_wire::receipt::decode_batch_header(include_bytes!(
        "../../../../tests/fixtures/asset/bound-native-withdrawal/header"
    )))?;
    let key = SigningKey::from_bytes(&[0x39; 32]);
    let facts = activity_facts(key.verifying_key().to_bytes())?;
    for offset in [2, 6, 38, 70, 102, 118, 130, 150, 182, 214, 253] {
        let changed = changed_event(offset)?;
        assert!(verify_sequencer_signature(&changed, key.verifying_key().to_bytes()).is_ok());
        assert!(
            withdrawal::verify(&changed, &facts, ACTIVITY, header.network_id()).is_err(),
            "withdrawal body offset {offset}"
        );
        if matches!(offset, 130 | 214) {
            assert!(
                verify_outcome(&changed, &facts).is_ok(),
                "original request must bind an otherwise valid signed event at {offset}"
            );
        }
    }
    Ok(())
}

#[test]
fn historical_withdrawal_without_ledger_binding_is_not_verified() -> Result<(), String> {
    let receipt =
        include_bytes!("../../../../tests/fixtures/asset/unbound-native-withdrawal/receipt");
    let activity =
        include_bytes!("../../../../tests/fixtures/asset/unbound-native-withdrawal/activity.lxa");
    let key = *include_bytes!(
        "../../../../tests/fixtures/asset/unbound-native-withdrawal/sequencer.public"
    );
    let header = layerx_wire::receipt::decode_batch_header(include_bytes!(
        "../../../../tests/fixtures/asset/unbound-native-withdrawal/header"
    ))
    .map_err(|error| format!("original native header: {error:?}"))?;
    let signed = verify_sequencer_signature(receipt, key)
        .map_err(|error| format!("original native signature: {error:?}"))?;
    let protocol = signed.protocol().ok_or("native receipt required")?;
    assert_eq!(
        (
            protocol.module_id(),
            protocol.operation(),
            protocol.result_code()
        ),
        (1, 0, 0)
    );
    assert_eq!(protocol.fee_charged(), 17);
    assert!(protocol.effects().is_empty());
    let authorized = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        key,
    );
    assert!(verify_outcome(receipt, &authorized).is_err());
    assert!(withdrawal::verify(receipt, &authorized, activity, header.network_id()).is_err());
    Ok(())
}
