use super::{
    decode_hex, hex, json_response, parse_hex32, response, upstream_json, Config, OutgoingResponse,
};
use layerx_proof::inclusion::{verify_receipt, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_wire::batch_maintenance::decode_maintenance;
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode, decode_merkle_proof, BatchHeader};

const MAX_RECEIPT_BYTES: usize = 524_288;
const MAX_HEADER_BYTES: usize = 65_536;
const MAX_PROOF_BYTES: usize = 65_536;
const RETRY_AFTER_SECONDS: u64 = 30;

#[derive(serde::Deserialize)]
struct HeadEvidence {
    header_hex: String,
    header_signature: String,
    receipt_proof_hex: String,
}

#[derive(serde::Deserialize)]
struct HeadDocument {
    current: bool,
    receipt_hex: String,
    receipt_digest: String,
    state_root: String,
    observed_sequence: u64,
    observed_at: u64,
    batch_evidence: HeadEvidence,
}

/// The principal-state facts that the pinned sequencer authority actually proves.
struct VerifiedState {
    canonical_state_root: [u8; 32],
    receipt_state_root: [u8; 32],
    receipt_digest: [u8; 32],
    batch_number: u64,
    observed_sequence: u64,
    timestamp_ms: u64,
}

/// Returns the head sequence, timestamp and resulting state root the node
/// publishes, taken from the receipt bytes the signed batch header commits to.
fn head_claims(receipt: &[u8], header: &BatchHeader) -> Result<(u64, u64, [u8; 32]), ()> {
    if let Ok(record) = decode_maintenance(receipt) {
        record.verify_header(header).map_err(|_| ())?;
        let occupancy = record.occupancy();
        return Ok((
            occupancy.global_sequence,
            header.timestamp_ms(),
            occupancy.resulting_state_root,
        ));
    }
    let decoded = decode(receipt).map_err(|_| ())?;
    let protocol = decoded.protocol().ok_or(())?;
    if protocol.global_sequence() < header.first_sequence()
        || protocol.global_sequence() > header.last_sequence()
    {
        return Err(());
    }
    Ok((
        protocol.global_sequence(),
        protocol.timestamp(),
        protocol.resulting_state_root(),
    ))
}

fn verified_state(
    head: &HeadDocument,
    authorization: &SequencerAuthorization,
) -> Result<VerifiedState, ()> {
    let receipt = decode_hex(&head.receipt_hex, MAX_RECEIPT_BYTES).map_err(|_| ())?;
    let header_bytes =
        decode_hex(&head.batch_evidence.header_hex, MAX_HEADER_BYTES).map_err(|_| ())?;
    let signature: [u8; 64] = decode_hex(&head.batch_evidence.header_signature, 64)
        .map_err(|_| ())?
        .try_into()
        .map_err(|_| ())?;
    let encoded_proof =
        decode_hex(&head.batch_evidence.receipt_proof_hex, MAX_PROOF_BYTES).map_err(|_| ())?;
    let path = decode_merkle_proof(&encoded_proof).map_err(|_| ())?;
    let proof = Proof::new(
        path.leaf_index(),
        path.leaf_count(),
        path.siblings().to_vec(),
    )
    .map_err(|_| ())?;
    let evidence = verify_receipt(&receipt, &proof, &header_bytes, &signature, authorization)
        .map_err(|_| ())?;
    let header = evidence.header().header();
    let digest = receipt_digest(&receipt).map_err(|_| ())?;
    if parse_hex32(&head.receipt_digest).map_err(|_| ())? != digest {
        return Err(());
    }
    let (observed_sequence, observed_at, canonical_state_root) = head_claims(&receipt, header)?;
    if parse_hex32(&head.state_root).map_err(|_| ())? != canonical_state_root {
        return Err(());
    }
    if head.observed_sequence != observed_sequence || head.observed_at != observed_at {
        return Err(());
    }
    Ok(VerifiedState {
        canonical_state_root,
        receipt_state_root: header.receipt_merkle_root(),
        receipt_digest: digest,
        batch_number: header.batch_number(),
        observed_sequence,
        timestamp_ms: observed_at,
    })
}

fn document(state: &VerifiedState) -> serde_json::Value {
    serde_json::json!({
        "network_mode": "hosted",
        "canonical_state_root": hex(&state.canonical_state_root),
        "state_root": hex(&state.canonical_state_root),
        "receipt_state_root": hex(&state.receipt_state_root),
        "receipt_digest": hex(&state.receipt_digest),
        "batch_number": state.batch_number,
        "observed_sequence": state.observed_sequence,
        "timestamp_ms": state.timestamp_ms,
        "verification": "sequencer-signed-batch-header-and-receipt-inclusion"
    })
}

pub(super) fn read(config: &Config, trace_id: &str) -> OutgoingResponse {
    let upstream = match upstream_json(
        config,
        &config.component,
        config.component_token.as_str(),
        "GET",
        "/v1/state",
        None,
        &[],
    ) {
        Ok(upstream) => upstream,
        Err(error) => return error,
    };
    if upstream.status == 503 {
        return response(
            503,
            "principal_state_proof_unavailable",
            Some(RETRY_AFTER_SECONDS),
        );
    }
    if upstream.status != 200 || upstream.content_type != "application/json" {
        return response(503, "component_invalid", Some(5));
    }
    let body: serde_json::Value = match serde_json::from_slice(&upstream.body) {
        Ok(value) => value,
        Err(_) => return response(502, "state_document_invalid", None),
    };
    let value = body.get("result").unwrap_or(&body);
    let head: HeadDocument = match serde_json::from_value(value.clone()) {
        Ok(value) => value,
        Err(_) => return response(502, "state_document_invalid", None),
    };
    if !head.current {
        return response(
            503,
            "principal_state_proof_unavailable",
            Some(RETRY_AFTER_SECONDS),
        );
    }
    let Ok(state) = verified_state(&head, &config.sequencer_authorization) else {
        return response(502, "state_proof_unverified", None);
    };
    json_response(
        200,
        &serde_json::json!({ "ok": true, "result": document(&state), "trace": trace_id }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn required<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
        value.unwrap_or_else(|error| panic!("{error:?}"))
    }

    fn text(value: &serde_json::Value, name: &str) -> String {
        value[name]
            .as_str()
            .unwrap_or_else(|| panic!("fixture field {name}"))
            .to_owned()
    }

    fn fixture(relative: &str) -> serde_json::Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        required(serde_json::from_slice(&required(std::fs::read(path))))
    }

    /// Rebuilds the exact `GET /v1/protocol/account-state/head` envelope the node
    /// serves from the captured authority evidence in the fixture.
    fn head_envelope() -> (serde_json::Value, SequencerAuthorization) {
        let capture = fixture("tests/fixtures/maintained-authority.json");
        let evidence = &capture["authority"]["batch_evidence"];
        let receipt = required(decode_hex(
            &text(&capture, "receipt_hex"),
            MAX_RECEIPT_BYTES,
        ));
        let header_bytes = required(decode_hex(&text(evidence, "header_hex"), MAX_HEADER_BYTES));
        let header = required(layerx_wire::receipt::decode_batch_header(&header_bytes));
        let (observed_sequence, observed_at, resulting_state_root) =
            required(head_claims(&receipt, &header));
        let authorization = SequencerAuthorization::new(
            required(parse_hex32(&text(&capture, "sequencer_id"))),
            required(parse_hex32(&text(&capture, "sequencer_public_key"))),
            required(text(&capture, "first_batch").parse()),
            required(text(&capture, "last_batch").parse()),
        );
        let envelope = serde_json::json!({
            "current": true,
            "receipt_hex": text(&capture, "receipt_hex"),
            "receipt_digest": hex(&required(receipt_digest(&receipt))),
            "state_root": hex(&resulting_state_root),
            "observed_sequence": observed_sequence,
            "observed_at": observed_at,
            "batch_evidence": {
                "header_hex": text(evidence, "header_hex"),
                "header_signature": text(evidence, "header_signature"),
                "receipt_proof_hex": text(evidence, "receipt_proof_hex"),
                "batch_identity": evidence["batch_identity"].clone()
            }
        });
        (envelope, authorization)
    }

    #[test]
    fn state_reads_publish_only_the_sequencer_signed_head_facts() {
        let (envelope, authorization) = head_envelope();
        let head: HeadDocument = required(serde_json::from_value(envelope.clone()));
        let state = required(verified_state(&head, &authorization).map_err(|()| "unverified"));
        let header_bytes = required(decode_hex(
            &text(&envelope["batch_evidence"], "header_hex"),
            MAX_HEADER_BYTES,
        ));
        let header = required(layerx_wire::receipt::decode_batch_header(&header_bytes));
        let receipt = required(decode_hex(
            &text(&envelope, "receipt_hex"),
            MAX_RECEIPT_BYTES,
        ));
        let (sequence, timestamp, resulting_state_root) = required(head_claims(&receipt, &header));
        assert_eq!(state.canonical_state_root, resulting_state_root);
        assert_eq!(state.receipt_state_root, header.receipt_merkle_root());
        assert_eq!(state.batch_number, header.batch_number());
        assert_eq!(state.timestamp_ms, timestamp);
        assert_eq!(state.observed_sequence, sequence);
        let published = document(&state);
        assert_eq!(
            published["canonical_state_root"],
            serde_json::json!(hex(&resulting_state_root))
        );
        assert_eq!(published["state_root"], published["canonical_state_root"]);
        assert_eq!(published["timestamp_ms"], serde_json::json!(timestamp));
        assert_eq!(
            published["observed_sequence"],
            serde_json::json!(state.observed_sequence)
        );
        assert_eq!(
            published["receipt_digest"],
            serde_json::json!(hex(&state.receipt_digest))
        );
        for omitted in ["accounts", "cells", "next_sequence"] {
            assert!(published.get(omitted).is_none(), "{omitted}");
        }
    }

    #[test]
    fn tampered_or_unpinned_head_documents_are_refused() {
        let (envelope, authorization) = head_envelope();
        for field in [
            "receipt_hex",
            "receipt_digest",
            "state_root",
            "observed_sequence",
            "observed_at",
        ] {
            let mut changed = envelope.clone();
            changed[field] = match field {
                "observed_sequence" | "observed_at" => {
                    serde_json::json!(envelope[field].as_u64().unwrap_or_default().wrapping_add(1))
                }
                "receipt_hex" => {
                    let mut bytes = required(decode_hex(
                        &text(&envelope, "receipt_hex"),
                        MAX_RECEIPT_BYTES,
                    ));
                    let last = bytes.len() - 1;
                    bytes[last] ^= 0x01;
                    serde_json::json!(hex(&bytes))
                }
                _ => serde_json::json!("aa".repeat(32)),
            };
            let head: HeadDocument = required(serde_json::from_value(changed));
            assert!(verified_state(&head, &authorization).is_err(), "{field}");
        }
        for field in ["header_hex", "header_signature", "receipt_proof_hex"] {
            let mut changed = envelope.clone();
            let original = text(&envelope["batch_evidence"], field);
            let mut bytes = required(decode_hex(&original, MAX_RECEIPT_BYTES));
            let last = bytes.len() - 1;
            bytes[last] ^= 0x01;
            changed["batch_evidence"][field] = serde_json::json!(hex(&bytes));
            let head: HeadDocument = required(serde_json::from_value(changed));
            assert!(verified_state(&head, &authorization).is_err(), "{field}");
        }
        let head: HeadDocument = required(serde_json::from_value(envelope.clone()));
        let foreign = fixture("../authority/tests/fixtures/real-program-deploy-receipt.json");
        let unpinned = SequencerAuthorization::new(
            required(parse_hex32(&text(&foreign, "sequencer_id_hex"))),
            required(parse_hex32(&text(&foreign, "sequencer_public_key_hex"))),
            1,
            u64::MAX,
        );
        assert!(verified_state(&head, &unpinned).is_err());
    }

    #[test]
    fn a_head_that_is_not_current_stays_a_retryable_refusal() {
        let (envelope, _) = head_envelope();
        let mut stale = envelope;
        stale["current"] = serde_json::json!(false);
        let head: HeadDocument = required(serde_json::from_value(stale));
        assert!(!head.current);
    }
}
