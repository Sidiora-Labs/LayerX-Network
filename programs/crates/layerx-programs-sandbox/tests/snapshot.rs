#[path = "../../layerx-programs-registry/tests/support/mod.rs"]
mod protocol_support;

use layerx_programs_runtime::{Abi, AuthorizationContext, CapabilitySet, FeeSchedule, Meter,
    PrincipalId, ProgramId, ResourceBudget, ValidationLimits, UnavailableReceiptOracle,
    WasmEngine, Storage, WasmValue, ABI_V1_VERSION};
use layerx_programs_runtime::test_support::{code_section, func_body, function_section, module,
    raw_section, type_section, unsigned_leb, OP_END, OP_I32_ADD, OP_I32_CONST, TYPE_I32};
use layerx_programs_sandbox::{restore, Lease,
    LeaseActivity, LeaseId, LeaseLimits, LeaseState, LeaseTransition,
    SandboxState, Snapshot, SnapshotRefusal, TransitionEvidence};

const ENTRYPOINT: &[u8] = b"sandbox_transition";
const CALL_PREFIX: usize = 106;

fn candidate(id: u8, tenant: u8, opened: u64, expiry: u64) -> Lease {
    Lease::request(
        LeaseId::new([id; 32]).unwrap_or_else(|error| panic!("lease id: {error}")),
        PrincipalId::new([tenant; 32]).unwrap_or_else(|error| panic!("tenant: {error}")),
        ProgramId::new([3; 32]).unwrap_or_else(|error| panic!("host: {error}")),
        layerx_programs_runtime::hash_bytes(layerx_programs_runtime::HashAlgorithm::Sha256,
            &continuation_module()).unwrap_or_else(|error| panic!("hash: {error}")), [4; 32], 10_000,
        LeaseLimits { cpu_fuel: 10_000, memory_bytes: 1 << 20,
            storage_read_bytes: 1 << 20, storage_write_bytes: 1 << 20,
            output_values: 1_000, output_bytes: 1 << 20, table_elements: 1_000,
            namespace_bytes: 1 << 20 }, opened, expiry,
    ).unwrap_or_else(|error| panic!("lease: {error}"))
}

fn candidate_namespace(id: u8, tenant: u8, namespace_bytes: u64) -> Lease {
    Lease::request(
        LeaseId::new([id; 32]).unwrap_or_else(|error| panic!("lease id: {error}")),
        PrincipalId::new([tenant; 32]).unwrap_or_else(|error| panic!("tenant: {error}")),
        ProgramId::new([3; 32]).unwrap_or_else(|error| panic!("host: {error}")),
        layerx_programs_runtime::hash_bytes(layerx_programs_runtime::HashAlgorithm::Sha256,
            &continuation_module()).unwrap_or_else(|error| panic!("hash: {error}")), [4; 32], 10_000,
        LeaseLimits { cpu_fuel: 10_000, memory_bytes: 1 << 20,
            storage_read_bytes: 1 << 20, storage_write_bytes: 1 << 20,
            output_values: 1_000, output_bytes: 1 << 20, table_elements: 1_000,
            namespace_bytes }, 10, 30,
    ).unwrap_or_else(|error| panic!("lease: {error}"))
}

fn call_payload(host: ProgramId, transition: LeaseTransition) -> Vec<u8> {
    let mut calldata = vec![0; 101];
    calldata[..2].copy_from_slice(&1u16.to_be_bytes());
    calldata[2..34].copy_from_slice(&transition.lease.bytes());
    calldata[34..66].copy_from_slice(&transition.tenant.bytes());
    calldata[66] = transition.activity as u8;
    calldata[67] = transition.from as u8;
    calldata[68] = transition.to as u8;
    calldata[69..].copy_from_slice(&transition.usage_observation_digest);
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

fn evidence(lease: &Lease, mut transition: LeaseTransition, batch: u64)
    -> (LeaseTransition, TransitionEvidence) {
    let fixture = protocol_support::programs_call_fixture(
        call_payload(lease.host_program(), transition), batch, protocol_support::NOW);
    let verifier = protocol_support::verifier_for_fixture(&fixture, batch, batch, None, 1_000);
    let proof = &fixture.proof.state;
    let head = verifier.verify_current_protocol_head(&proof.receipt, &proof.receipt_proof,
        &proof.header, &proof.header_signature, protocol_support::NOW)
        .unwrap_or_else(|error| panic!("head: {error}"));
    transition.activity_id = head.activity_id();
    let verified = TransitionEvidence::verify_call(&head, lease, transition,
        &AuthorizationContext::new(lease.tenant(), CapabilitySet::empty()),
        &fixture.proof.activity, &fixture.proof.activity_proof, &proof.header)
        .unwrap_or_else(|error| panic!("evidence: {error}"));
    (transition, verified)
}

fn advance(mut lease: Lease, edges: &[(LeaseActivity, LeaseState, LeaseState, u64)]) -> Lease {
    let request = LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
        activity: LeaseActivity::Request, from: LeaseState::Requested,
        to: LeaseState::Requested, activity_id: [0; 32],
        usage_observation_digest: lease.request_binding_digest()
            .unwrap_or_else(|error| panic!("request digest: {error}")) };
    let (request, proof) = evidence(&lease, request, lease.opened_at());
    let mut book = layerx_programs_sandbox::LeaseBook::new();
    book.insert_requested(lease, request, proof)
        .unwrap_or_else(|error| panic!("request: {error}"));
    for &(activity, from, to, batch) in edges {
        lease = book.get(request.lease).unwrap_or_else(|| panic!("lease")).clone();
        let transition = LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
            activity, from, to, activity_id: [0; 32], usage_observation_digest: [0; 32] };
        let (transition, proof) = evidence(&lease, transition, batch);
        book.transition(lease.id(), transition, proof)
            .unwrap_or_else(|error| panic!("advance: {error}"));
    }
    book.get(request.lease).unwrap_or_else(|| panic!("advanced lease")).clone()
}

fn advance_existing(mut lease: Lease, edges: &[(LeaseActivity, LeaseState, LeaseState, u64)]) -> Lease {
    for &(activity, from, to, batch) in edges {
        let transition = LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
            activity, from, to, activity_id: [0; 32], usage_observation_digest: [0; 32] };
        let (transition, proof) = evidence(&lease, transition, batch);
        lease.transition(transition, proof)
            .unwrap_or_else(|error| panic!("advance existing: {error}"));
    }
    lease
}

fn active(id: u8, tenant: u8) -> Lease {
    advance(candidate(id, tenant, 10, 30), &[
        (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded, 11),
        (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active, 12),
    ])
}

fn funded(id: u8, tenant: u8) -> Lease {
    advance(candidate(id, tenant, 20, 40), &[
        (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded, 21),
    ])
}

fn activation(lease: &Lease, batch: u64) -> (LeaseTransition, TransitionEvidence) {
    evidence(lease, LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
        activity: LeaseActivity::Activate, from: LeaseState::Funded, to: LeaseState::Active,
        activity_id: [0; 32], usage_observation_digest: [0; 32] }, batch)
}

fn meter() -> Meter {
    Meter::new(ResourceBudget::new_complete(1_000, 1 << 20, 1 << 20, 1 << 20,
        100, 1 << 20, 100), FeeSchedule::declared())
}

fn continuation_module() -> Vec<u8> {
    continuation_module_with_global(true)
}

fn continuation_module_with_global(export_global: bool) -> Vec<u8> {
    let global_section = raw_section(6, &[1, TYPE_I32, 1, OP_I32_CONST, 0, OP_END]);
    let memory_section = raw_section(5, &[1, 0, 1]);
    let declarations = if export_global {
        vec![("seed", 0, 0), ("continue", 0, 1), ("memory", 2, 0), ("counter", 3, 0)]
    } else {
        vec![("seed", 0, 0), ("continue", 0, 1), ("memory", 2, 0)]
    };
    let mut exports = unsigned_leb(declarations.len() as u64);
    for (name, kind, index) in declarations {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.push(kind);
        exports.extend(unsigned_leb(index));
    }
    module(&[
        type_section(&[(&[], &[TYPE_I32])]),
        function_section(&[0, 0]),
        memory_section,
        global_section,
        raw_section(7, &exports),
        code_section(&[
            func_body(&[], &[OP_I32_CONST, 41, 0x24, 0, OP_I32_CONST, 41, OP_END]),
            func_body(&[], &[0x23, 0, OP_I32_CONST, 1, OP_I32_ADD, 0x24, 0,
                0x23, 0, OP_END]),
        ]),
    ])
}

fn validated_continuation() -> layerx_programs_runtime::ValidatedModule {
    WasmEngine::new(ValidationLimits::declared())
        .unwrap_or_else(|error| panic!("engine: {error}"))
        .validate(&continuation_module())
        .unwrap_or_else(|error| panic!("validation: {error}"))
}

fn running_instance(lease: &Lease, module: &layerx_programs_runtime::ValidatedModule,
    storage: Storage) -> layerx_programs_runtime::ProgramInstance {
    let authorization = AuthorizationContext::new(lease.tenant(), CapabilitySet::empty());
    let abi = Abi::new(ABI_V1_VERSION, lease.host_program(), authorization, storage,
        &UnavailableReceiptOracle).unwrap_or_else(|error| panic!("abi: {error}"));
    module.instantiate_sandbox(meter(), abi)
        .unwrap_or_else(|error| panic!("sandbox instance: {error}"))
}

fn snapshot_transition(lease: &Lease, digest: [u8; 32], batch: u64)
    -> (LeaseTransition, TransitionEvidence) {
    evidence(lease, LeaseTransition { lease: lease.id(), tenant: lease.tenant(),
        activity: LeaseActivity::Snapshot, from: LeaseState::Active, to: LeaseState::Active,
        activity_id: [0; 32], usage_observation_digest: digest }, batch)
}

#[test]
fn snapshot_destroy_restore_and_continue_preserves_exact_execution_state() {
    let mut source = active(1, 9);
    let module = validated_continuation();
    let mut source_storage = Storage::new();
    let namespace = source.namespace().storage_namespace()
        .unwrap_or_else(|error| panic!("namespace: {error}"));
    let live_key = b"counter".to_vec();
    let mut transaction = source_storage.transaction(namespace);
    transaction.write(&live_key, &41u64.to_be_bytes())
        .unwrap_or_else(|error| panic!("live write: {error}"));
    transaction.commit();
    let mut uninterrupted_instance = running_instance(&source, &module, source_storage);
    assert_eq!(uninterrupted_instance.call("seed", &[]), Ok(vec![WasmValue::I32(41)]));
    let uninterrupted_capture = Snapshot::capture(&source, &mut uninterrupted_instance, "continue", &[])
        .unwrap_or_else(|error| panic!("sandbox state: {error}"));
    let uninterrupted = uninterrupted_capture.state().clone();
    let (snapshot_edge, snapshot_evidence) = snapshot_transition(&source,
        uninterrupted_capture.digest().unwrap_or_else(|error| panic!("digest: {error}")), 13);
    let snapshot = Snapshot::commit(&mut source, &mut uninterrupted_instance, uninterrupted_capture,
        snapshot_edge, snapshot_evidence)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let snapshot_usage = uninterrupted_instance.meter().finish().unwrap_or_else(|error| panic!("usage: {error}"));
    assert!(snapshot_usage.storage_write_bytes >= snapshot.storage_bytes());
    let mut source_storage = uninterrupted_instance.storage_snapshot()
        .unwrap_or_else(|| panic!("runtime storage"));
    assert_eq!(source.snapshot_records().len(), 1);
    assert_eq!(source.snapshot_records()[0].digest(), snapshot.digest());
    assert_eq!(source.snapshot_records()[0].namespace(), source.namespace());
    assert_eq!(source.snapshot_records()[0].byte_length(), snapshot.byte_length());
    assert!(source.snapshot_records()[0].chunk_count() > 0);
    assert_eq!(source.snapshot_records()[0].owner(), source.tenant());
    assert_eq!(source.snapshot_records()[0].source_lease(), source.id());
    assert_eq!(source.snapshot_records()[0].host_program(), source.host_program());
    assert_eq!(source.snapshot_records()[0].image_code_hash(), source.image_code_hash());
    let restarted_source = Lease::decode_state(&source.canonical_state_bytes()
        .unwrap_or_else(|error| panic!("source state: {error}")))
        .unwrap_or_else(|error| panic!("restart lease: {error}"));
    assert_eq!(restarted_source.snapshot_records(), source.snapshot_records());
    let restarted_snapshot = SandboxState::from_canonical(&restarted_source,
        &snapshot.state().canonical_bytes().unwrap_or_else(|error| panic!("snapshot bytes: {error}")))
        .unwrap_or_else(|error| panic!("restart snapshot: {error}"));
    assert_eq!(restarted_snapshot, *snapshot.state());
    assert!(source_storage.namespace_persistent_bytes(source.namespace().storage_namespace()
        .unwrap_or_else(|error| panic!("namespace: {error}")))
        .unwrap_or_else(|error| panic!("source occupancy: {error}")) > 0);

    let mut destroyed = advance_existing(source, &[
        (LeaseActivity::BeginSettlement, LeaseState::Active, LeaseState::Settling, 14),
        (LeaseActivity::Expire, LeaseState::Settling, LeaseState::Expired, 15),
    ]);
    let (destroy, destroy_evidence) = evidence(&destroyed, LeaseTransition {
        lease: destroyed.id(), tenant: destroyed.tenant(), activity: LeaseActivity::Destroy,
        from: LeaseState::Expired, to: LeaseState::Destroyed, activity_id: [0; 32],
        usage_observation_digest: [0; 32] }, 16);
    let mut reclaim_meter = meter();
    destroyed.destroy_with_evidence(&mut source_storage, &mut reclaim_meter, destroy, destroy_evidence)
        .unwrap_or_else(|error| panic!("destroy: {error}"));
    assert_eq!(destroyed.state(), LeaseState::Destroyed);
    assert_eq!(destroyed.snapshot_records().len(), 1);
    assert_eq!(destroyed.snapshot_records()[0].digest(), snapshot.digest());
    assert_eq!(destroyed.snapshot_records()[0].namespace(), destroyed.namespace());
    assert_eq!(source_storage.namespace_persistent_bytes(destroyed.namespace().storage_namespace()
        .unwrap_or_else(|error| panic!("namespace: {error}"))), Ok(0));
    let restarted_destroyed = Lease::decode_state(&destroyed.canonical_state_bytes()
        .unwrap_or_else(|error| panic!("destroyed state: {error}")))
        .unwrap_or_else(|error| panic!("restart destroyed: {error}"));
    let mut target = funded(2, 9);
    let mut target_storage = Storage::new();
    let (activate, activation_evidence) = activation(&target, 22);
    let mut restore_meter = meter();
    let target_authorization = AuthorizationContext::new(target.tenant(), CapabilitySet::empty());
    let restored = restore(&restarted_destroyed, &mut target, &mut target_storage, snapshot.digest(),
        restarted_snapshot, target_authorization, &mut restore_meter, &module,
        activate, activation_evidence)
        .unwrap_or_else(|error| panic!("restore: {error}"));
    assert_eq!(restored.state().linear_memory(), uninterrupted.linear_memory());
    assert_eq!(restored.state().globals(), uninterrupted.globals());
    assert_eq!(restored.state().continuation(), uninterrupted.continuation());
    assert_eq!(restored.state().source_lease(), target.id());
    assert_eq!(restored.state().source_namespace(), target.namespace());
    assert_eq!(target.state(), LeaseState::Active);
    assert_eq!(target.restored_from(), Some(snapshot.digest()));
    let restore_usage = restore_meter.finish().unwrap_or_else(|error| panic!("usage: {error}"));
    assert!(restore_usage.storage_write_bytes > snapshot.storage_bytes());

    let uninterrupted_before = uninterrupted_instance.meter().finish()
        .unwrap_or_else(|error| panic!("uninterrupted before: {error}"));
    let uninterrupted_output = uninterrupted_instance.call("continue", &[])
        .unwrap_or_else(|error| panic!("uninterrupted continuation: {error}"));
    let uninterrupted_after = uninterrupted_instance.meter().finish()
        .unwrap_or_else(|error| panic!("uninterrupted after: {error}"));
    let restored_output = restored.outputs();
    let restored_usage = restored.instance().meter().finish()
        .unwrap_or_else(|error| panic!("restored runtime usage: {error}"));
    assert_eq!(restored_output, uninterrupted_output);
    assert_eq!(restored_output, [WasmValue::I32(42)]);
    let uninterrupted_delta = (
        uninterrupted_after.cpu_fuel - uninterrupted_before.cpu_fuel,
        uninterrupted_after.memory_bytes - uninterrupted_before.memory_bytes,
        uninterrupted_after.storage_read_bytes - uninterrupted_before.storage_read_bytes,
        uninterrupted_after.storage_write_bytes - uninterrupted_before.storage_write_bytes,
        uninterrupted_after.output_values - uninterrupted_before.output_values,
        uninterrupted_after.output_bytes - uninterrupted_before.output_bytes,
    );
    assert_eq!(restored_usage.cpu_fuel, uninterrupted_delta.0);
}

#[test]
fn ownership_digest_and_target_bindings_are_refused_before_metering() {
    let mut source = active(3, 9);
    let module = validated_continuation();
    let mut source_instance = running_instance(&source, &module, Storage::new());
    source_instance.call("seed", &[]).unwrap_or_else(|error| panic!("seed: {error}"));
    let canonical_capture = Snapshot::capture(&source, &mut source_instance, "continue", &[])
        .unwrap_or_else(|error| panic!("capture: {error}"));
    let canonical = canonical_capture.state().clone();
    let (edge, proof) = snapshot_transition(&source,
        canonical_capture.digest().unwrap_or_else(|error| panic!("digest: {error}")), 13);
    let snapshot = Snapshot::commit(&mut source, &mut source_instance, canonical_capture, edge, proof)
        .unwrap_or_else(|error| panic!("snapshot: {error}"));
    let source_storage = source_instance.storage_snapshot().unwrap_or_else(|| panic!("storage"));
    let mut target = funded(4, 9);
    let mut target_storage = Storage::new();
    let intruder = PrincipalId::new([8; 32]).unwrap_or_else(|error| panic!("intruder: {error}"));
    let mut intruder_meter = meter();
    let (activate, activation_evidence) = activation(&target, 22);
    let intruder_authorization = AuthorizationContext::new(intruder, CapabilitySet::empty());
    assert!(matches!(restore(&source, &mut target, &mut target_storage, snapshot.digest(),
        snapshot.state().clone(), intruder_authorization, &mut intruder_meter, &module,
        activate, activation_evidence),
        Err(SnapshotRefusal::NotSnapshotOwner)));
    assert_eq!(intruder_meter.finish().map(|usage| usage.storage_write_bytes), Ok(0));

    let mut altered_instance = running_instance(&source, &module, source_storage.clone());
    let altered = Snapshot::capture(&source, &mut altered_instance, "continue", &[])
        .unwrap_or_else(|error| panic!("altered: {error}"));
    let mut mismatch_meter = meter();
    let (activate, activation_evidence) = activation(&target, 22);
    let target_authorization = AuthorizationContext::new(target.tenant(), CapabilitySet::empty());
    assert!(matches!(restore(&source, &mut target, &mut target_storage, snapshot.digest(), altered.state().clone(),
        target_authorization, &mut mismatch_meter, &module, activate, activation_evidence),
        Err(SnapshotRefusal::DigestMismatch)));
    assert_eq!(mismatch_meter.finish().map(|usage| usage.storage_write_bytes), Ok(0));

    let mut foreign = funded(5, 8);
    let mut foreign_storage = Storage::new();
    let mut foreign_meter = meter();
    let (activate, activation_evidence) = activation(&foreign, 22);
    let foreign_authorization = AuthorizationContext::new(foreign.tenant(), CapabilitySet::empty());
    assert!(matches!(restore(&source, &mut foreign, &mut foreign_storage, snapshot.digest(),
        snapshot.state().clone(), foreign_authorization, &mut foreign_meter, &module,
        activate, activation_evidence),
        Err(SnapshotRefusal::NotSnapshotOwner)));
}

#[test]
fn canonical_integer_discriminants_distinguish_runtime_value_widths() {
    let source = active(7, 9);
    let module = validated_continuation();
    let mut instance = running_instance(&source, &module, Storage::new());
    let i32_state = Snapshot::capture(&source, &mut instance, "continue", &[WasmValue::I32(1)])
        .unwrap_or_else(|error| panic!("i32 state: {error}"));
    let i64_state = Snapshot::capture(&source, &mut instance, "continue", &[WasmValue::I64(1)])
        .unwrap_or_else(|error| panic!("i64 state: {error}"));
    assert_ne!(i32_state.state().canonical_bytes(), i64_state.state().canonical_bytes());
    assert_ne!(i32_state.digest(), i64_state.digest());
}

#[test]
fn aggregate_snapshot_chunks_never_cross_the_lease_namespace_ceiling() {
    let mut lease = advance(candidate_namespace(8, 9, 1_024), &[
        (LeaseActivity::Fund, LeaseState::Requested, LeaseState::Funded, 11),
        (LeaseActivity::Activate, LeaseState::Funded, LeaseState::Active, 12),
    ]);
    let mut storage = Storage::new();
    let module = validated_continuation();
    let mut instance = running_instance(&lease, &module, storage);
    let mut refused = false;
    for index in 0u8..64 {
        let state = Snapshot::capture(&lease, &mut instance, "continue", &[WasmValue::I32(i32::from(index))])
            .unwrap_or_else(|error| panic!("state: {error}"));
        let (edge, proof) = snapshot_transition(&lease,
            state.digest().unwrap_or_else(|error| panic!("digest: {error}")), 13 + u64::from(index));
        match Snapshot::commit(&mut lease, &mut instance, state, edge, proof) {
            Ok(_) => {}
            Err(SnapshotRefusal::TargetBoundExceeded) => { refused = true; break; }
            Err(error) => panic!("unexpected snapshot refusal: {error}"),
        }
    }
    storage = instance.storage_snapshot().unwrap_or_else(|| panic!("storage"));
    assert!(refused);
    assert!(storage.namespace_persistent_bytes(lease.namespace().storage_namespace()
        .unwrap_or_else(|error| panic!("namespace: {error}")))
        .unwrap_or_else(|error| panic!("occupancy: {error}"))
        + storage.protocol_prefix_bytes(lease.namespace().snapshot_storage_namespace(), b"snapshot")
            .unwrap_or_else(|error| panic!("snapshot occupancy: {error}"))
        <= lease.limits().namespace_bytes);
}

#[test]
fn continuation_capture_fails_closed_when_a_mutable_global_is_hidden() {
    let module = WasmEngine::new(ValidationLimits::declared())
        .unwrap_or_else(|error| panic!("engine: {error}"))
        .validate(&continuation_module_with_global(false))
        .unwrap_or_else(|error| panic!("validation: {error}"));
    let mut instance = module.instantiate().unwrap_or_else(|error| panic!("instance: {error}"));
    assert!(matches!(instance.capture_continuation("continue", &[]),
        Err(layerx_programs_runtime::ExecutionFault::EngineFault { .. })));
}
