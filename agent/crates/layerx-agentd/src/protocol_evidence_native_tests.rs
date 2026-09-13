use super::{RawReceiptEvidence, VerifiedReceiptEvidence};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::receipt::{AuthorizedBatch, MaintainedOutcomeEvidence};

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("native terminal evidence: {error:?}"))
}

#[test]
fn actual_daemon_terminal_requires_the_complete_maintained_transition() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/custody/daemon-credit-receipt");
    let read = |name: &str| checked(std::fs::read(root.join(name)));
    let header_bytes = read("header");
    let header = checked(layerx_wire::receipt::decode_batch_header(&header_bytes));
    let receipt = read("credit.receipt");
    let decoded = checked(layerx_wire::receipt::decode(&receipt));
    let protocol = decoded
        .protocol()
        .unwrap_or_else(|| panic!("protocol credit"));
    let public = checked(read("sequencer.public").try_into());
    let authority = AuthorizedBatch::new(
        protocol.batch_id(),
        [0; 32],
        header.previous_state_root(),
        protocol.resulting_state_root(),
        public,
    );
    let raw = RawReceiptEvidence::new(
        receipt,
        checked(layerx_proof::merkle::decode_proof(&read("receipt.proof"))),
        header_bytes,
        checked(read("header.signature").try_into()),
    );
    let maintenance = read("maintenance.receipt");
    let proof = checked(layerx_proof::merkle::decode_proof(&read(
        "maintenance.proof",
    )));
    let authorization = SequencerAuthorization::new(header.sequencer_id(), public, 1, 1);
    let signature = raw.header_signature();
    let evidence = MaintainedOutcomeEvidence {
        header: raw.canonical_header(),
        header_signature: &signature,
        activity_proof: raw.proof(),
        maintenance: &maintenance,
        maintenance_proof: &proof,
        authorization: &authorization,
    };
    let terminal = checked(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &authority,
        &evidence,
        &[raw.canonical_receipt().to_vec()],
        header.protocol_version(),
        header.network_id(),
    ));
    assert_eq!(terminal.activity_id(), protocol.activity_id());
    assert_eq!(terminal.result_code(), 0);
    prove_terminal_recovery(&read("activity"), &read("credit"), &terminal);
    assert_eq!(terminal.global_sequence(), 1);
    assert_eq!(
        terminal.level(),
        layerx_types::verify::VerificationLevel::BATCH_INCLUDED
    );
    assert!(VerifiedReceiptEvidence::verify_authorized(
        &raw,
        &authority,
        header.protocol_version(),
        header.network_id()
    )
    .is_err());
    let wrong = AuthorizedBatch::new(
        authority.batch_id(),
        authority.asset(),
        authority.previous_state_root(),
        header.resulting_state_root(),
        public,
    );
    assert!(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &wrong,
        &evidence,
        &[raw.canonical_receipt().to_vec()],
        header.protocol_version(),
        header.network_id()
    )
    .is_err());
    assert!(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &authority,
        &evidence,
        &[raw.canonical_receipt().to_vec()],
        header.protocol_version(),
        header.network_id() + 1
    )
    .is_err());
    let mut corrupted = maintenance.clone();
    corrupted[50] ^= 1;
    let bad_evidence = MaintainedOutcomeEvidence {
        maintenance: &corrupted,
        ..evidence
    };
    assert!(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &authority,
        &bad_evidence,
        &[raw.canonical_receipt().to_vec()],
        header.protocol_version(),
        header.network_id()
    )
    .is_err());
}

fn actual_submission(bytes: &[u8], public: &[u8; 32]) -> crate::sign::VerifiedSubmission {
    use crate::prepare::{PreparationAuditEntry, Prepared};
    use layerx_types::activity::{Authority, EnvelopeBuilder, TimestampBound};
    use layerx_types::payload::{
        ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload,
    };
    let kind = checked(ActivityType::new(ModuleId::Bridge, 1));
    let registration = checked(ModuleRegistration::new(ModuleId::Bridge, &[kind]));
    let registry = checked(ModuleRegistry::new(&[registration]));
    let activity = checked(layerx_wire::activity::decode_signed(bytes, &registry));
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(activity.protocol_version()));
    checked(builder.network_id(activity.network_id()));
    checked(builder.activity_type(kind));
    checked(builder.actor_did(checked(layerx_types::ids::Did::new(activity.actor_did()))));
    checked(builder.authority(checked(Authority::owner(activity.authority()))));
    checked(builder.account_sequence(activity.account_sequence()));
    let bound = activity.timestamp_bound();
    checked(builder.timestamp_bound(checked(TimestampBound::new(
        bound.not_before,
        bound.not_after,
    ))));
    checked(
        builder.idempotency_key(layerx_types::ids::IdempotencyKey::new(
            activity.idempotency_key(),
        )),
    );
    checked(builder.fee_limit(layerx_types::amount::Amount::from_u128(
        activity.fee_limit(),
    )));
    checked(builder.payload_hash(activity.payload_hash()));
    checked(builder.payload(checked(Payload::new(&registry, kind, activity.payload()))));
    let envelope = checked(builder.build());
    let canonical_bytes = checked(layerx_wire::activity::encode_unsigned_envelope(&envelope));
    assert_eq!(
        canonical_bytes,
        checked(layerx_wire::activity::encode_unsigned(&activity))
    );
    let disclosed = checked(crate::prepare::disclose(&canonical_bytes, &registry));
    let prepared = Prepared {
        signing_preimage: *checked(layerx_wire::sign::preimage_unsigned(&envelope)).as_bytes(),
        envelope,
        canonical_bytes,
        observed_head_sequence: 0,
        disclosure: disclosed.disclosure,
        disclosure_digest: disclosed.digest,
        audit: PreparationAuditEntry {
            idempotency_key: activity.idempotency_key(),
            observed_head_sequence: 0,
            disclosure_digest: disclosed.digest,
        },
    };
    checked(crate::sign::verify_before_submit(
        bytes, &prepared, public, &registry,
    ))
}

fn prove_terminal_recovery(activity: &[u8], credit: &[u8], terminal: &VerifiedReceiptEvidence) {
    use crate::outbox::{Outbox, SubmissionState};
    use crate::store::{Store, TenantId};
    let verified = actual_submission(activity, &checked(credit[139..171].try_into()));
    assert_eq!(verified.activity_id(), terminal.activity_id());
    let id = verified.idempotency_key();
    let root = std::env::temp_dir().join(format!("native-terminal-{}", std::process::id()));
    let tenant = checked(TenantId::new("native-terminal"));
    let mut store = checked(Store::open(&root));
    let mut outbox = Outbox::default();
    checked(outbox.enqueue(&mut store, tenant.clone(), id, verified));
    checked(outbox.transition(
        &mut store,
        id,
        SubmissionState::Submitted,
        "submitted exact native bytes",
        None,
    ));
    checked(outbox.transition(
        &mut store,
        id,
        SubmissionState::Unknown,
        "receipt resolution after restart",
        None,
    ));
    drop(outbox);
    drop(store);
    let mut store = checked(Store::open(&root));
    let mut restored = Outbox::default();
    checked(restored.restore(&store, tenant.clone(), id));
    assert_eq!(
        restored.status(id).map(|status| status.state),
        Some(SubmissionState::Unknown)
    );
    checked(restored.transition(
        &mut store,
        id,
        SubmissionState::Executed,
        "authenticated maintained outcome",
        Some(terminal.clone()),
    ));
    drop(restored);
    drop(store);
    let store = checked(Store::open(&root));
    let mut restored = Outbox::default();
    checked(restored.restore(&store, tenant, id));
    let status = restored
        .status(id)
        .unwrap_or_else(|| panic!("restored terminal status"));
    assert_eq!(status.state, SubmissionState::Executed);
    assert_eq!(
        status
            .evidence
            .map(crate::outbox::ReceiptEvidence::receipt_ref),
        Some(terminal.receipt_ref())
    );
    assert_eq!(checked(restored.exact_signed_bytes(id)), activity);
    assert!(restored.bytes_for_transmission(id).is_err());
}
