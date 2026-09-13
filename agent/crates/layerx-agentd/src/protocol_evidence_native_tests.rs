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
        header.protocol_version(),
        header.network_id(),
    ));
    assert_eq!(terminal.activity_id(), protocol.activity_id());
    assert_eq!(terminal.result_code(), 0);
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
        header.protocol_version(),
        header.network_id()
    )
    .is_err());
    assert!(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &authority,
        &evidence,
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
        header.protocol_version(),
        header.network_id()
    )
    .is_err());
}
