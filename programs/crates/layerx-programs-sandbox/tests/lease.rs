#[path = "../../layerx-programs-registry/tests/support/mod.rs"]
mod protocol_support;

use layerx_programs_sandbox::{
    usage_observation_digest, Lease, LeaseActivity, LeaseBook, LeaseId, LeaseLimits,
    LeaseRefusal, LeaseState, LeaseTransition, LeaseUsage, TransitionEvidence, UsageOutcome,
    MAX_CONCURRENT_LEASES_PER_PRINCIPAL,
};
use layerx_programs_runtime::{AuthorizationContext, CapabilitySet, FeeSchedule, Meter, PrincipalId,
    ProgramId, ResourceBudget, Storage};

fn storage_meter() -> Meter {
    Meter::new(ResourceBudget::declared(), FeeSchedule::declared())
}

const ENTRYPOINT: &[u8] = b"sandbox_transition";
const CALL_PREFIX: usize = 106;

fn candidate(id: u8, tenant: u8, opened: u64, expiry: u64) -> Lease {
    Lease::request(
        LeaseId::new([id; 32]).unwrap_or_else(|error| panic!("lease id: {error}")),
        PrincipalId::new([tenant; 32]).unwrap_or_else(|error| panic!("tenant: {error}")),
        ProgramId::new([3; 32]).unwrap_or_else(|error| panic!("host: {error}")),
        [4; 32], [5; 32], 100,
        LeaseLimits { cpu_fuel: 10, memory_bytes: 10, storage_read_bytes: 10,
            storage_write_bytes: 10, output_values: 10, output_bytes: 10,
            table_elements: 10, namespace_bytes: 10 },
        opened, expiry,
    ).unwrap_or_else(|error| panic!("lease: {error}"))
}

fn calldata(transition: LeaseTransition) -> Vec<u8> {
    let mut bytes = vec![0; 101];
    bytes[..2].copy_from_slice(&1u16.to_be_bytes());
    bytes[2..34].copy_from_slice(&transition.lease.bytes());
    bytes[34..66].copy_from_slice(&transition.tenant.bytes());
    bytes[66] = transition.activity as u8;
    bytes[67] = transition.from as u8;
    bytes[68] = transition.to as u8;
    bytes[69..].copy_from_slice(&transition.usage_observation_digest);
    bytes
}

fn call_payload(host: ProgramId, transition: LeaseTransition) -> Vec<u8> {
    let calldata = calldata(transition);
    let mut payload = vec![0; CALL_PREFIX];
    payload[..32].copy_from_slice(&host.bytes());
    payload[32..34].copy_from_slice(&2u16.to_be_bytes());
    payload[34..36].copy_from_slice(&(ENTRYPOINT.len() as u16).to_be_bytes());
    payload[36..40].copy_from_slice(&(calldata.len() as u32).to_be_bytes());
    payload[46..50].copy_from_slice(&1u32.to_be_bytes());
    payload.extend_from_slice(ENTRYPOINT);
    payload.extend_from_slice(&calldata);
    payload
}

fn verified(
    lease: &Lease, mut transition: LeaseTransition, batch: u64,
) -> (LeaseTransition, TransitionEvidence) {
    let fixture = protocol_support::programs_call_fixture(
        call_payload(lease.host_program(), transition), batch, protocol_support::NOW,
    );
    let verifier = protocol_support::verifier_for_fixture(
        &fixture, batch, batch, None, 1_000,
    );
    let state = &fixture.proof.state;
    let head = verifier.verify_current_protocol_head(
        &state.receipt, &state.receipt_proof, &state.header,
        &state.header_signature, protocol_support::NOW,
    ).unwrap_or_else(|error| panic!("verified head: {error}"));
    transition.activity_id = head.activity_id();
    let evidence = TransitionEvidence::verify_call(
        &head, lease, transition,
        &AuthorizationContext::new(lease.tenant(), CapabilitySet::empty()),
        &fixture.proof.activity,
        &fixture.proof.activity_proof, &state.header,
    ).unwrap_or_else(|error| panic!("transition evidence: {error}"));
    (transition, evidence)
}

fn wrong_principal_refusal(lease: &Lease, mut transition: LeaseTransition, batch: u64) -> LeaseRefusal {
    let fixture = protocol_support::programs_call_fixture(
        call_payload(lease.host_program(), transition), batch, protocol_support::NOW,
    );
    let verifier = protocol_support::verifier_for_fixture(&fixture, batch, batch, None, 1_000);
    let state = &fixture.proof.state;
    let head = verifier.verify_current_protocol_head(&state.receipt, &state.receipt_proof,
        &state.header, &state.header_signature, protocol_support::NOW)
        .unwrap_or_else(|error| panic!("verified head: {error}"));
    transition.activity_id = head.activity_id();
    TransitionEvidence::verify_call(&head, lease, transition,
        &AuthorizationContext::new(
            PrincipalId::new([0x77; 32]).unwrap_or_else(|error| panic!("wrong principal: {error}")),
            CapabilitySet::empty()),
        &fixture.proof.activity, &fixture.proof.activity_proof, &state.header)
        .err().unwrap_or_else(|| panic!("wrong principal was accepted"))
}

fn transition(
    lease: &Lease, activity: LeaseActivity, from: LeaseState, to: LeaseState,
    observation: [u8; 32], batch: u64,
) -> (LeaseTransition, TransitionEvidence) {
    verified(lease, LeaseTransition { lease: lease.id(), tenant: lease.tenant(), activity,
        from, to, activity_id: [0; 32], usage_observation_digest: observation }, batch)
}

#[test]
fn real_receipts_drive_every_lifecycle_edge_and_destroyed_never_revives() {
    let lease = candidate(1, 2, 10, 20);
    let request_digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("request digest: {error}"));
    let unverified_request = LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
        activity: LeaseActivity::Request, from: LeaseState::Requested, to: LeaseState::Requested,
        activity_id: [0; 32], usage_observation_digest: request_digest };
    assert_eq!(wrong_principal_refusal(&lease, unverified_request, 10), LeaseRefusal::TenantMismatch);
    let (request, request_evidence) = transition(&lease, LeaseActivity::Request,
        LeaseState::Requested, LeaseState::Requested, request_digest, 10);
    let mut book = LeaseBook::new();
    book.insert_requested(lease, request, request_evidence)
        .unwrap_or_else(|error| panic!("request: {error}"));
    for (activity, from, to, batch) in [
        (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded, 11),
        (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active, 12),
        (LeaseActivity::BeginSettlement, LeaseState::Active, LeaseState::Settling, 13),
        (LeaseActivity::Expire, LeaseState::Settling, LeaseState::Expired, 14),
        (LeaseActivity::Destroy, LeaseState::Expired, LeaseState::Destroyed, 15),
    ] {
        let current = book.get(LeaseId::new([1; 32]).unwrap_or_else(|error| panic!("id: {error}")))
            .unwrap_or_else(|| panic!("stored lease")).clone();
        let (declared, evidence) = transition(&current, activity, from, to, [0; 32], batch);
        if activity == LeaseActivity::Destroy {
            book.destroy(current.id(), &mut Storage::new(), &mut storage_meter(), declared, evidence)
                .unwrap_or_else(|error| panic!("destroy: {error}"));
        } else {
            book.transition(current.id(), declared, evidence)
                .unwrap_or_else(|error| panic!("transition: {error}"));
        }
        if activity == LeaseActivity::Destroy {
            assert_eq!(book.destroy(current.id(), &mut Storage::new(), &mut storage_meter(),
                declared, evidence), Err(LeaseRefusal::ReplayedEvidence));
        } else {
            assert_eq!(book.transition(current.id(), declared, evidence), Err(LeaseRefusal::ReplayedEvidence));
        }
    }
    let destroyed = book.get(LeaseId::new([1; 32]).unwrap_or_else(|error| panic!("id: {error}")))
        .unwrap_or_else(|| panic!("destroyed lease")).clone();
    let (revive, evidence) = transition(&destroyed, LeaseActivity::Fund,
        LeaseState::Destroyed, LeaseState::Funded, [0; 32], 16);
    assert_eq!(book.transition(destroyed.id(), revive, evidence), Err(LeaseRefusal::InvalidTransition));
    let encoded = destroyed.canonical_state_bytes().unwrap_or_else(|error| panic!("encode: {error}"));
    assert_eq!(Lease::decode_state(&encoded), Ok(destroyed));
    let mut altered = encoded.clone();
    altered.push(0);
    assert_eq!(Lease::decode_state(&altered), Err(LeaseRefusal::InvalidStateEncoding));
}

#[test]
fn real_bound_receipt_closes_intrinsically_and_refuses_mismatch_and_regression() {
    let lease = candidate(2, 3, 10, 20);
    let request_digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("request digest: {error}"));
    let (request, request_evidence) = transition(&lease, LeaseActivity::Request,
        LeaseState::Requested, LeaseState::Requested, request_digest, 10);
    let mut book = LeaseBook::new();
    book.insert_requested(lease, request, request_evidence).unwrap_or_else(|error| panic!("request: {error}"));
    for (activity, from, to, batch) in [
        (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded, 11),
        (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active, 12),
    ] {
        let current = book.get(LeaseId::new([2; 32]).unwrap_or_else(|error| panic!("id: {error}"))).unwrap_or_else(|| panic!("lease")).clone();
        if current.state() == LeaseState::Funded {
            let (expire, expire_evidence) = transition(&current, LeaseActivity::Expire,
                LeaseState::Funded, LeaseState::Expired, [0; 32], 12);
            assert!(matches!(book.transition(current.id(), expire, expire_evidence),
                Err(LeaseRefusal::NotExpired { .. })));
        }
        let (declared, evidence) = transition(&current, activity, from, to, [0; 32], batch);
        if activity == LeaseActivity::Activate {
            let mut altered = declared;
            altered.to = LeaseState::Expired;
            assert_eq!(book.transition(current.id(), altered, evidence), Err(LeaseRefusal::ActivityReceiptMismatch));
        }
        book.transition(current.id(), declared, evidence).unwrap_or_else(|error| panic!("advance: {error}"));
    }
    let id = LeaseId::new([2; 32]).unwrap_or_else(|error| panic!("id: {error}"));
    assert_eq!(book.record_usage(id, LeaseUsage { cpu_fuel: 5, ..LeaseUsage::default() }, 5, 13, None),
        Ok(UsageOutcome::Recorded(LeaseUsage { cpu_fuel: 5, ..LeaseUsage::default() })));
    assert_eq!(book.record_usage(id, LeaseUsage { cpu_fuel: 4, ..LeaseUsage::default() }, 4, 13, None),
        Err(LeaseRefusal::UsageRegression));
    let current = book.get(id).unwrap_or_else(|| panic!("active lease")).clone();
    let usage = LeaseUsage { cpu_fuel: 11, ..LeaseUsage::default() };
    let digest = usage_observation_digest(id, usage, 5, 14).unwrap_or_else(|error| panic!("usage digest: {error}"));
    let (close, evidence) = transition(&current, LeaseActivity::CloseBoundExceeded,
        LeaseState::Active, LeaseState::Settling, digest, 14);
    assert_eq!(book.transition(id, close, evidence), Err(LeaseRefusal::IntrinsicActivityRequired));
    let (close, evidence) = transition(&current, LeaseActivity::CloseBoundExceeded,
        LeaseState::Active, LeaseState::Settling, digest, 14);
    let before = book.get(id).unwrap_or_else(|| panic!("before refusal")).state_digest()
        .unwrap_or_else(|error| panic!("before digest: {error}"));
    let mut altered = close;
    altered.to = LeaseState::Expired;
    assert_eq!(book.record_usage(id, usage, 5, 14, Some((altered, evidence))),
        Err(LeaseRefusal::ActivityReceiptMismatch));
    assert_eq!(book.get(id).unwrap_or_else(|| panic!("after refusal")).state_digest(), Ok(before));
    assert!(matches!(book.record_usage(id, usage, 5, 14, Some((close, evidence))),
        Ok(UsageOutcome::ClosedByBound { .. })));
}

#[test]
fn real_request_evidence_enforces_principal_concurrency_and_expiry() {
    let mut book = LeaseBook::new();
    for index in 1..=MAX_CONCURRENT_LEASES_PER_PRINCIPAL as u8 {
        let lease = candidate(index, 9, 10, 20);
        let digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("digest: {error}"));
        let (request, evidence) = transition(&lease, LeaseActivity::Request,
            LeaseState::Requested, LeaseState::Requested, digest, 10);
        book.insert_requested(lease, request, evidence).unwrap_or_else(|error| panic!("admit: {error}"));
    }
    let lease = candidate(33, 9, 10, 20);
    let digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("digest: {error}"));
    let (request, evidence) = transition(&lease, LeaseActivity::Request,
        LeaseState::Requested, LeaseState::Requested, digest, 10);
    assert_eq!(book.insert_requested(lease, request, evidence), Err(LeaseRefusal::PrincipalLeaseLimit));

    let requested = book.get(LeaseId::new([2; 32]).unwrap_or_else(|error| panic!("requested id: {error}")))
        .unwrap_or_else(|| panic!("requested lease"));
    let mut corrupt = requested.canonical_state_bytes().unwrap_or_else(|error| panic!("state: {error}"));
    let usage_offset = b"LayerX/programs/sandbox/lease-state/v1\0".len() + 224 + 16 + 64 + 16;
    corrupt[usage_offset + 7] = 1;
    assert_eq!(Lease::decode_state(&corrupt), Err(LeaseRefusal::InvalidStateEncoding));

    let first = LeaseId::new([1; 32]).unwrap_or_else(|error| panic!("first id: {error}"));
    let current = book.get(first).unwrap_or_else(|| panic!("first lease")).clone();
    let (expire, evidence) = transition(&current, LeaseActivity::Expire,
        LeaseState::Requested, LeaseState::Expired, [0; 32], 20);
    book.transition(first, expire, evidence).unwrap_or_else(|error| panic!("expire: {error}"));
    let current = book.get(first).unwrap_or_else(|| panic!("expired lease")).clone();
    let (destroy, evidence) = transition(&current, LeaseActivity::Destroy,
        LeaseState::Expired, LeaseState::Destroyed, [0; 32], 21);
    book.destroy(first, &mut Storage::new(), &mut storage_meter(), destroy, evidence)
        .unwrap_or_else(|error| panic!("destroy: {error}"));
    let lease = candidate(33, 9, 22, 30);
    let digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("digest: {error}"));
    let (request, evidence) = transition(&lease, LeaseActivity::Request,
        LeaseState::Requested, LeaseState::Requested, digest, 22);
    book.insert_requested(lease, request, evidence).unwrap_or_else(|error| panic!("replacement: {error}"));
}

#[test]
fn lease_prefix_is_host_accessible_isolated_and_addressable_as_one_unit() {
    let lease = candidate(40, 8, 10, 20);
    let namespace = lease.namespace();
    let storage_namespace = namespace.storage_namespace()
        .unwrap_or_else(|error| panic!("storage namespace: {error}"));
    assert_eq!(storage_namespace.program(), lease.host_program());
    let adjacent = candidate(41, 8, 10, 20).namespace();
    assert_ne!(namespace.key_prefix(), adjacent.key_prefix());
    assert_eq!(namespace.key_prefix(), namespace.bytes());
}

#[test]
fn altered_real_activity_and_header_evidence_are_refused() {
    let lease = candidate(41, 8, 10, 20);
    let digest = lease.request_binding_digest().unwrap_or_else(|error| panic!("digest: {error}"));
    let mut transition = LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
        activity: LeaseActivity::Request, from: LeaseState::Requested, to: LeaseState::Requested,
        activity_id: [0; 32], usage_observation_digest: digest };
    let fixture = protocol_support::programs_call_fixture(
        call_payload(lease.host_program(), transition), 10, protocol_support::NOW,
    );
    let verifier = protocol_support::verifier_for_fixture(&fixture, 10, 10, None, 1_000);
    let state = &fixture.proof.state;
    let head = verifier.verify_current_protocol_head(&state.receipt, &state.receipt_proof,
        &state.header, &state.header_signature, protocol_support::NOW)
        .unwrap_or_else(|error| panic!("head: {error}"));
    transition.activity_id = head.activity_id();
    let authorization = AuthorizationContext::new(lease.tenant(), CapabilitySet::empty());
    let mut activity = fixture.proof.activity.clone();
    let last = activity.len().checked_sub(1).unwrap_or_else(|| panic!("activity bytes"));
    activity[last] ^= 1;
    assert_eq!(TransitionEvidence::verify_call(&head, &lease, transition, &authorization,
        &activity, &fixture.proof.activity_proof, &state.header),
        Err(LeaseRefusal::InvalidCanonicalEvidence));
    let mut header = state.header.clone();
    let last = header.len().checked_sub(1).unwrap_or_else(|| panic!("header bytes"));
    header[last] ^= 1;
    assert_eq!(TransitionEvidence::verify_call(&head, &lease, transition, &authorization,
        &fixture.proof.activity, &fixture.proof.activity_proof, &header),
        Err(LeaseRefusal::InvalidCanonicalEvidence));
}
