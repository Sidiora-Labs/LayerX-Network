use super::{
    authorized_batch_by_activity, decode, hex, verify_authorized_receipt, AuthorizedBatch,
    BatchEvidence, BatchIdentityEvidence, EvidenceRefusal, ReceiptCheck, SequencerAuthorization,
};
use ed25519_dalek::{Signer as _, SigningKey};
use layerx_proof::merkle::{encode_proof, Proof};
use layerx_wire::receipt::{encode_unsigned, ProtocolReceipt};
use serde::Deserialize;
use std::fmt::Debug;

fn must<T, E: Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("original native evidence: {error:?}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    network_id: u32,
    receipts: Vec<Fixture>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    batch_number: u64,
    activity_id: String,
    module_id: u16,
    result_code: i32,
    sequencer_public_key: String,
    header: String,
    header_signature: String,
    receipt: String,
    receipt_proof: String,
    maintenance: String,
    maintenance_proof: String,
}

fn corpus() -> Corpus {
    must(serde_json::from_str(include_str!(
        "../fixtures/native-module-outcomes.json"
    )))
}

fn stored_proof(encoded: &str) -> Vec<u8> {
    let bytes = must(hex::decode(encoded));
    assert_eq!(bytes.len(), 9 + usize::from(bytes[0]) * 32);
    let index = u32::from_be_bytes(must(bytes[1..5].try_into()));
    let count = u32::from_be_bytes(must(bytes[5..9].try_into()));
    let siblings = bytes[9..]
        .chunks_exact(32)
        .map(|value| must(value.try_into()))
        .collect();
    encode_proof(&must(Proof::new(index, count, siblings)))
}

impl Fixture {
    fn evidence(&self) -> BatchEvidence {
        BatchEvidence {
            header: must(hex::decode(&self.header)),
            header_signature: must(must(hex::decode(&self.header_signature)).try_into()),
            receipt_proof: stored_proof(&self.receipt_proof),
            batch_identity: BatchIdentityEvidence::BatchMaintenanceV1 {
                receipt: must(hex::decode(&self.maintenance)),
                proof: stored_proof(&self.maintenance_proof),
                activity_receipts: vec![must(hex::decode(&self.receipt))],
            },
        }
    }

    fn authorization(&self) -> SequencerAuthorization {
        let header = must(layerx_wire::receipt::decode_batch_header(&must(
            hex::decode(&self.header),
        )));
        let public = must(hex::decode32(&self.sequencer_public_key));
        assert_eq!(
            must(layerx_wire::handover::sequencer_id(&public)),
            header.sequencer_id()
        );
        SequencerAuthorization::new(
            header.sequencer_id(),
            public,
            self.batch_number,
            self.batch_number,
        )
    }
}

fn activity_batch(receipt: &ProtocolReceipt, public_key: [u8; 32]) -> AuthorizedBatch {
    AuthorizedBatch::new(
        receipt.batch_id(),
        receipt.asset(),
        receipt.previous_state_root(),
        receipt.resulting_state_root(),
        public_key,
    )
}

#[test]
fn original_native_module_successes_refusals_and_handover_verify_with_maintenance() {
    let corpus = corpus();
    assert_eq!(corpus.network_id, 77);
    assert_eq!(corpus.receipts.len(), 15);
    let mut results = Vec::new();
    for fixture in &corpus.receipts {
        let bytes = must(hex::decode(&fixture.receipt));
        let receipt = must(decode(&bytes));
        let receipt = receipt
            .protocol()
            .unwrap_or_else(|| panic!("protocol receipt"));
        assert_eq!(receipt.module_id(), fixture.module_id);
        assert_eq!(receipt.result_code(), fixture.result_code);
        let evidence = fixture.evidence();
        let header = must(layerx_wire::receipt::decode_batch_header(&evidence.header));
        assert_eq!(header.network_id(), corpus.network_id);
        let authorization = fixture.authorization();
        let facts = must(authorized_batch_by_activity(
            must(hex::decode32(&fixture.activity_id)),
            &bytes,
            &evidence,
            &authorization,
        ));
        assert_eq!(facts.batch_number, fixture.batch_number);
        assert_eq!(facts.resulting_state_root, header.resulting_state_root());
        assert_eq!(facts.global_sequence, fixture.batch_number * 2 - 1);
        let batch = activity_batch(receipt, authorization.public_key());
        assert_eq!(verify_authorized_receipt(&bytes, &batch), Ok(()));
        if (2..=7).contains(&fixture.module_id) {
            let refusal = layerx_proof::receipt::verify_outcome(&bytes, &batch)
                .err()
                .unwrap_or_else(|| panic!("generic operation zero must remain refused"));
            assert_eq!(refusal.check, ReceiptCheck::Operation);
        }
        results.push((fixture.module_id, fixture.result_code));
    }
    assert_eq!(results[1..3], [(2, -409), (2, -400)]);
    assert_eq!(results[13..], [(7, 0), (3, 0)]);
    assert_ne!(
        corpus.receipts[0].sequencer_public_key,
        corpus.receipts[14].sequencer_public_key
    );
}

#[test]
fn original_native_evidence_refuses_other_activity_pin_header_and_maintenance() {
    for fixture in corpus().receipts {
        let bytes = must(hex::decode(&fixture.receipt));
        let activity = must(hex::decode32(&fixture.activity_id));
        let evidence = fixture.evidence();
        let authorization = fixture.authorization();
        assert_eq!(
            authorized_batch_by_activity([8; 32], &bytes, &evidence, &authorization),
            Err(EvidenceRefusal::ActivityMismatch)
        );
        let wrong_pin = SequencerAuthorization::new(
            authorization.sequencer_id(),
            [4; 32],
            fixture.batch_number,
            fixture.batch_number,
        );
        assert!(authorized_batch_by_activity(activity, &bytes, &evidence, &wrong_pin).is_err());
        let mut changed = evidence.clone();
        changed.header_signature[0] ^= 1;
        assert!(authorized_batch_by_activity(activity, &bytes, &changed, &authorization).is_err());
        let BatchIdentityEvidence::BatchMaintenanceV1 { receipt, .. } = &mut changed.batch_identity
        else {
            panic!("native maintenance")
        };
        receipt[0] ^= 1;
        changed.header_signature = evidence.header_signature;
        assert!(authorized_batch_by_activity(activity, &bytes, &changed, &authorization).is_err());
        let mut changed = bytes.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(
            authorized_batch_by_activity(activity, &changed, &evidence, &authorization).is_err()
        );
    }
}

fn resign(bytes: &mut [u8]) -> [u8; 32] {
    let key = SigningKey::from_bytes(&[61; 32]);
    let receipt = must(decode(bytes));
    let digest = must(layerx_wire::hash::receipt_digest(&must(encode_unsigned(
        &receipt,
    ))));
    let offset = bytes.len() - 64;
    bytes[offset..].copy_from_slice(&key.sign(&digest).to_bytes());
    key.verifying_key().to_bytes()
}

fn module_offset(bytes: &[u8], receipt: &ProtocolReceipt) -> usize {
    let positions: Vec<_> = bytes
        .windows(32)
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == receipt.batch_id()).then_some(index))
        .collect();
    assert_eq!(positions.len(), 1);
    positions[0] + 32
}

#[test]
fn signed_native_state_projection_and_event_substitutions_are_refused() {
    for fixture in corpus()
        .receipts
        .into_iter()
        .filter(|v| (2..=7).contains(&v.module_id))
    {
        let original = must(hex::decode(&fixture.receipt));
        let decoded = must(decode(&original));
        let receipt = decoded
            .protocol()
            .unwrap_or_else(|| panic!("protocol receipt"));
        let module = module_offset(&original, receipt);
        let asset = module + 2 + 4 + 4 + 1 + 4;
        let amount = asset + 32;
        let from = amount + 16 + 4;
        let debit = from + 32;
        let sequence = debit + 32;
        let to = sequence + 8 + 4;
        let credit = to + 32;
        let transfer = credit + 32 + 4;
        let authorization = transfer + 32 + 4;
        let context = authorization + 32 + 4;
        for offset in [
            asset,
            amount,
            from,
            debit,
            debit + 16,
            sequence,
            to,
            credit,
            credit + 16,
            transfer,
            authorization,
            context,
        ] {
            assert_eq!(original[offset], 0);
            let mut changed = original.clone();
            changed[offset] = 1;
            let public = resign(&mut changed);
            let batch = activity_batch(receipt, public);
            assert_eq!(
                verify_authorized_receipt(&changed, &batch),
                Err(EvidenceRefusal::Receipt(ReceiptCheck::ReceiptShape))
            );
        }
        if let Some(effect) = receipt.effects().first() {
            let positions: Vec<_> = original
                .windows(effect.body().len())
                .enumerate()
                .filter_map(|(index, body)| (body == effect.body()).then_some(index))
                .collect();
            assert_eq!(positions.len(), 1);
            let mut changed = original.clone();
            changed[positions[0]..positions[0] + 32].fill(0);
            let public = resign(&mut changed);
            assert_eq!(
                verify_authorized_receipt(&changed, &activity_batch(receipt, public)),
                Err(EvidenceRefusal::Receipt(ReceiptCheck::ReceiptShape))
            );
        }
    }
}
