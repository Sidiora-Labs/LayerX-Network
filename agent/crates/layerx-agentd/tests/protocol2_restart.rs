mod support;

#[test]
fn protocol2_recovers_identical_signed_program_call_evidence() {
    let signing_seed = [0x4c; 32];
    let records = [support::TestAuthorityRecord {
        signing_seed,
        epoch: 2,
        first_batch: 1,
        last_batch: 3,
        revoked_at_batch: None,
    }];
    let policy = support::TestAuthorityPolicy {
        protocol_version: 2,
        network_id: 42,
        records: &records,
        handshake_signing_seed: signing_seed,
        handshake_batch: 3,
    };
    support::try_evidence_authority_after_restart(policy)
        .unwrap_or_else(|error| panic!("recovered protocol 2 authority: {error:?}"));
}
