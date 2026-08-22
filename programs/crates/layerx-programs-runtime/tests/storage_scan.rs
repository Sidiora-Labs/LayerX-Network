#![allow(clippy::cast_possible_truncation, clippy::too_many_arguments)]

use layerx_programs_runtime::abi::response::CANDIDATE_ABI_MODULE;
use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    AbiError, AuthorizationContext, AuthorizedExecutionRequest, Capability, CapabilitySet,
    CompositionContext, Executor, PrincipalId, ProgramId, ReceiptOracle, ReceiptView, Storage,
    StorageNamespace, WasmEngine, WasmValue, CALL_ENTRY_EXPORT,
};

#[derive(Debug)]
struct NoReceipts;

impl ReceiptOracle for NoReceipts {
    fn verified_receipt(&self, _receipt_digest: [u8; 32]) -> Result<ReceiptView, AbiError> {
        Err(AbiError::ReceiptMismatch)
    }
}

fn program(byte: u8) -> ProgramId {
    ProgramId::new([byte; 32]).unwrap_or_else(|error| panic!("program: {error}"))
}

fn principal(byte: u8) -> PrincipalId {
    PrincipalId::new([byte; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn signed_leb(mut value: i32) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        let done = (value == 0 && byte & 0x40 == 0) || (value == -1 && byte & 0x40 != 0);
        bytes.push(if done { byte } else { byte | 0x80 });
        if done {
            return bytes;
        }
    }
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn data_section(segments: &[(i32, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(segments.len() as u64);
    for (offset, bytes) in segments {
        payload.extend_from_slice(&[0, 0x41]);
        payload.extend(signed_leb(*offset));
        payload.push(0x0b);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

fn memory_and_exports() -> (Vec<u8>, Vec<u8>) {
    memory_and_exports_at(2, 3)
}

fn memory_and_exports_at(reserve_index: u8, call_index: u8) -> (Vec<u8>, Vec<u8>) {
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(3);
    for (name, kind, index) in [
        ("layerx_reserve", 0u8, reserve_index),
        ("layerx_call", 0, call_index),
        ("memory", 2, 0),
    ] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    (memory, section(7, &exports))
}

fn write_then_scan_guest(output_capacity: i32, trap_after_scan: bool) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 4], &[TYPE_I32]),
        (&[TYPE_I32; 9], &[TYPE_I32]),
        (&[TYPE_I32; 3], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        ("layerx_v1", "storage_write", 0),
        (CANDIDATE_ABI_MODULE, "storage_scan_scoped", 1),
        (CANDIDATE_ABI_MODULE, "response_write", 2),
    ]);
    let (memory, exports) = memory_and_exports_at(3, 4);
    let mut entry = Vec::new();
    for value in [0, 1, 1, 1] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 0, 0x1a]);
    for value in [1, 0, 0, 32, 0, 1, 13, 128, output_capacity] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 1, 0x1a]);
    if trap_after_scan {
        entry.push(0x00);
    } else {
        for value in [0, 128, 13] {
            push_i32(&mut entry, value);
        }
        entry.extend_from_slice(&[0x10, 2, 0x1a, 0x41, 0]);
    }
    entry.push(0x0b);
    module(&[
        types,
        imports,
        function_section(&[3, 4]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, b"ab")]),
    ])
}

fn push_i32(body: &mut Vec<u8>, value: i32) {
    body.push(0x41);
    body.extend(signed_leb(value));
}

fn scan_guest(
    prefix: &[u8],
    cursor: &[u8],
    max_entries: i32,
    max_bytes: i32,
    output_pointer: i32,
    output_capacity: i32,
    response_pointer: i32,
    response_length: i32,
) -> Vec<u8> {
    scan_guest_selected(
        1,
        prefix,
        cursor,
        max_entries,
        max_bytes,
        output_pointer,
        output_capacity,
        response_pointer,
        response_length,
    )
}

fn scan_guest_selected(
    selector: i32,
    prefix: &[u8],
    cursor: &[u8],
    max_entries: i32,
    max_bytes: i32,
    output_pointer: i32,
    output_capacity: i32,
    response_pointer: i32,
    response_length: i32,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 9], &[TYPE_I32]),
        (&[TYPE_I32; 3], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_scan_scoped", 0),
        (CANDIDATE_ABI_MODULE, "response_write", 1),
    ]);
    let (memory, exports) = memory_and_exports();
    let mut entry = Vec::new();
    for value in [
        selector,
        0,
        i32::try_from(prefix.len()).unwrap_or(i32::MAX),
        32,
        i32::try_from(cursor.len()).unwrap_or(i32::MAX),
        max_entries,
        max_bytes,
        output_pointer,
        output_capacity,
    ] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 0, 0x1a]);
    for value in [0, response_pointer, response_length] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 1, 0x1a, 0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[func_body(&[], &[0x41, 0, 0x0b]), func_body(&[], &entry)]),
        data_section(&[(0, prefix), (32, cursor), (64, b"keep")]),
    ])
}

fn scan_status_guest(
    selector: i32,
    prefix: &[u8],
    cursor: &[u8],
    max_entries: i32,
    max_bytes: i32,
    output_pointer: i32,
    output_capacity: i32,
    sentinel_pointer: i32,
) -> Vec<u8> {
    let types = type_section(&[
        (&[TYPE_I32; 9], &[TYPE_I32]),
        (&[TYPE_I32; 3], &[TYPE_I32]),
        (&[TYPE_I32], &[TYPE_I32]),
        (&[TYPE_I32, TYPE_I32], &[TYPE_I32]),
    ]);
    let imports = import_section(&[
        (CANDIDATE_ABI_MODULE, "storage_scan_scoped", 0),
        (CANDIDATE_ABI_MODULE, "response_write", 1),
    ]);
    let (memory, exports) = memory_and_exports();
    let mut entry = Vec::new();
    for value in [
        selector,
        0,
        i32::try_from(prefix.len()).unwrap_or(i32::MAX),
        32,
        i32::try_from(cursor.len()).unwrap_or(i32::MAX),
        max_entries,
        max_bytes,
        output_pointer,
        output_capacity,
    ] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 0, 0x21, 0]);
    push_i32(&mut entry, 64);
    entry.extend_from_slice(&[0x20, 0, 0x36, 2, 0]);
    push_i32(&mut entry, 68);
    push_i32(&mut entry, sentinel_pointer);
    entry.extend_from_slice(&[0x28, 2, 0, 0x36, 2, 0]);
    for value in [0, 64, 8] {
        push_i32(&mut entry, value);
    }
    entry.extend_from_slice(&[0x10, 1, 0x1a, 0x41, 0, 0x0b]);
    module(&[
        types,
        imports,
        function_section(&[2, 3]),
        memory,
        exports,
        code_section(&[
            func_body(&[], &[0x41, 0, 0x0b]),
            func_body(&[(1, TYPE_I32)], &entry),
        ]),
        data_section(&[(0, prefix), (32, cursor), (64, b"keep"), (128, b"keep")]),
    ])
}

fn expected_cursor(
    owner: ProgramId,
    actor: PrincipalId,
    max_entries: u32,
    max_bytes: u32,
    after: &[u8],
) -> Vec<u8> {
    let mut cursor = vec![1, 65];
    cursor.extend_from_slice(&owner.bytes());
    cursor.push(0);
    cursor.extend_from_slice(&actor.bytes());
    cursor.extend_from_slice(&0u16.to_be_bytes());
    cursor.extend_from_slice(&max_entries.to_be_bytes());
    cursor.extend_from_slice(&max_bytes.to_be_bytes());
    cursor.extend_from_slice(&(after.len() as u16).to_be_bytes());
    cursor.extend_from_slice(after);
    cursor
}

fn expected_page(entries: &[(&[u8], &[u8])], cursor: Option<&[u8]>) -> Vec<u8> {
    let mut page = (entries.len() as u16).to_be_bytes().to_vec();
    for (key, value) in entries {
        page.extend_from_slice(&(key.len() as u16).to_be_bytes());
        page.extend_from_slice(key);
        page.extend_from_slice(&(value.len() as u32).to_be_bytes());
        page.extend_from_slice(value);
    }
    let cursor = cursor.unwrap_or_default();
    page.push(u8::from(!cursor.is_empty()));
    page.extend_from_slice(&(cursor.len() as u16).to_be_bytes());
    page.extend_from_slice(cursor);
    page
}

fn execute(
    wasm: &[u8],
    owner: ProgramId,
    actor: PrincipalId,
    capabilities: CapabilitySet,
    storage: &mut Storage,
    response_capacity: usize,
) -> layerx_programs_runtime::CandidateAuthorizedExecutionRecord {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(wasm)
        .unwrap_or_else(|error| panic!("candidate validation: {error}"));
    Executor::declared()
        .execute_authorized_candidate(
            storage,
            AuthorizedExecutionRequest {
                module: &module,
                program: owner,
                authorization: AuthorizationContext::new(actor, capabilities),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity,
            },
        )
        .unwrap_or_else(|error| panic!("candidate execution: {error}"))
}

fn seed(storage: &mut Storage, namespace: StorageNamespace, pairs: &[(&[u8], &[u8])]) {
    let mut transaction = storage.transaction(namespace);
    for (key, value) in pairs {
        transaction
            .write(key, value)
            .unwrap_or_else(|error| panic!("seed: {error}"));
    }
    let _ = transaction.commit();
}

fn principal_read() -> CapabilitySet {
    CapabilitySet::new([Capability::StorageRead]).unwrap_or_else(|error| panic!("grant: {error}"))
}

#[test]
fn candidate_scan_host_returns_empty_and_single_empty_prefix_pages() {
    let owner = program(1);
    let actor = principal(2);
    let mut empty = Storage::new();
    let empty_record = execute(
        &scan_guest(b"", b"", 1, 5, 128, 5, 128, 5),
        owner,
        actor,
        principal_read(),
        &mut empty,
        5,
    );
    assert_eq!(empty_record.execution().outputs(), [WasmValue::I32(0)]);
    assert_eq!(empty_record.execution().usage().storage_read_bytes, 5);
    assert_eq!(
        empty_record
            .response()
            .unwrap_or_else(|| panic!("empty response"))
            .bytes,
        vec![0, 0, 0, 0, 0]
    );

    let mut one = Storage::new();
    seed(
        &mut one,
        StorageNamespace::principal(owner, actor),
        &[(b"a", b"b")],
    );
    let one_record = execute(
        &scan_guest(b"", b"", 1, 13, 128, 13, 128, 13),
        owner,
        actor,
        principal_read(),
        &mut one,
        13,
    );
    assert_eq!(one_record.execution().usage().storage_read_bytes, 13);
    assert_eq!(
        one_record
            .response()
            .unwrap_or_else(|| panic!("single response"))
            .bytes,
        vec![0, 1, 0, 1, b'a', 0, 0, 0, 1, b'b', 0, 0, 0]
    );
}

#[test]
fn candidate_scan_paginates_across_activities_and_is_insertion_order_independent() {
    let owner = program(3);
    let actor = principal(4);
    let namespace = StorageNamespace::principal(owner, actor);
    let first_guest = scan_guest(b"", b"", 1, 93, 128, 93, 128, 93);
    let mut left = Storage::new();
    let mut right = Storage::new();
    seed(&mut left, namespace, &[(b"b", b"b"), (b"a", b"a")]);
    seed(&mut right, namespace, &[(b"a", b"a"), (b"b", b"b")]);
    let left_first = execute(&first_guest, owner, actor, principal_read(), &mut left, 93);
    let right_first = execute(&first_guest, owner, actor, principal_read(), &mut right, 93);
    assert_eq!(left_first.response(), right_first.response());
    assert_eq!(
        left_first.execution().usage(),
        right_first.execution().usage()
    );
    assert_eq!(
        left_first.canonical_evidence(),
        right_first.canonical_evidence()
    );
    let cursor = left_first
        .response()
        .unwrap_or_else(|| panic!("first response"))
        .bytes[13..]
        .to_vec();
    let second = execute(
        &scan_guest(b"", &cursor, 1, 93, 128, 13, 128, 13),
        owner,
        actor,
        principal_read(),
        &mut left,
        13,
    );
    assert_eq!(second.execution().usage().storage_read_bytes, 13);
    assert_eq!(
        second
            .response()
            .unwrap_or_else(|| panic!("second response"))
            .bytes,
        vec![0, 1, 0, 1, b'b', 0, 0, 0, 1, b'b', 0, 0, 0]
    );
}

#[test]
fn candidate_scan_enforces_complete_page_byte_ceiling_independently_of_entry_ceiling() {
    let owner = program(11);
    let actor = principal(12);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut exact_storage = Storage::new();
    let mut one_byte_lower_storage = Storage::new();
    seed(
        &mut exact_storage,
        namespace,
        &[(b"a", b"a"), (b"b", b"b"), (b"c", b"c")],
    );
    seed(
        &mut one_byte_lower_storage,
        namespace,
        &[(b"a", b"a"), (b"b", b"b"), (b"c", b"c")],
    );

    let exact_cursor = expected_cursor(owner, actor, 64, 101, b"b");
    let exact_page = expected_page(&[(b"a", b"a"), (b"b", b"b")], Some(&exact_cursor));
    assert_eq!(exact_page.len(), 101);
    let exact = execute(
        &scan_guest(b"", b"", 64, 101, 128, 101, 128, 101),
        owner,
        actor,
        principal_read(),
        &mut exact_storage,
        101,
    );
    assert_eq!(exact.execution().usage().storage_read_bytes, 101);
    assert_eq!(
        exact
            .response()
            .unwrap_or_else(|| panic!("exact response"))
            .bytes,
        exact_page
    );

    let lower_cursor = expected_cursor(owner, actor, 64, 100, b"a");
    let lower_page = expected_page(&[(b"a", b"a")], Some(&lower_cursor));
    assert_eq!(lower_page.len(), 93);
    let one_byte_lower = execute(
        &scan_guest(b"", b"", 64, 100, 128, 93, 128, 93),
        owner,
        actor,
        principal_read(),
        &mut one_byte_lower_storage,
        93,
    );
    assert_eq!(one_byte_lower.execution().usage().storage_read_bytes, 93);
    assert_eq!(
        one_byte_lower
            .response()
            .unwrap_or_else(|| panic!("one-byte-lower response"))
            .bytes,
        lower_page
    );
}

#[test]
fn candidate_scan_refusals_are_unmetered_and_leave_output_sentinel_unchanged() {
    let owner = program(5);
    let actor = principal(6);
    let mut storage = Storage::new();
    seed(
        &mut storage,
        StorageNamespace::principal(owner, actor),
        &[(b"a", b"b")],
    );
    for (guest, grants) in [
        (
            scan_guest(b"", b"", 1, 13, 128, 12, 64, 4),
            principal_read(),
        ),
        (
            scan_guest(b"", b"", 1, 13, 65_535, 13, 64, 4),
            principal_read(),
        ),
        (
            scan_guest(b"", b"", 1, 13, 128, 13, 64, 4),
            CapabilitySet::empty(),
        ),
        (
            scan_guest_selected(2, b"", b"", 1, 13, 128, 13, 64, 4),
            principal_read(),
        ),
    ] {
        let before = storage.clone();
        let record = execute(&guest, owner, actor, grants, &mut storage, 4);
        assert_eq!(record.execution().outputs(), [WasmValue::I32(0)]);
        assert_eq!(record.execution().usage().storage_read_bytes, 0);
        assert_eq!(
            record
                .response()
                .unwrap_or_else(|| panic!("sentinel response"))
                .bytes,
            b"keep"
        );
        assert_eq!(storage, before);
    }
}

#[test]
fn candidate_scan_status_fixtures_preserve_negative_status_and_scan_output_sentinel() {
    let owner = program(13);
    let actor = principal(14);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut storage = Storage::new();
    seed(&mut storage, namespace, &[(b"a", b"a"), (b"b", b"b")]);
    let issuing = execute(
        &scan_guest(b"", b"", 1, 93, 128, 93, 128, 93),
        owner,
        actor,
        principal_read(),
        &mut storage,
        93,
    );
    let foreign_cursor = issuing
        .response()
        .unwrap_or_else(|| panic!("issuing response"))
        .bytes[13..]
        .to_vec();
    let mut corrupt_cursor = foreign_cursor.clone();
    corrupt_cursor.push(0);
    let foreign_namespace_cursor = expected_cursor(program(15), actor, 1, 93, b"a");

    for (guest, grants, expected_status) in [
        (
            scan_status_guest(1, b"", b"", 1, 13, 128, 13, 128),
            CapabilitySet::empty(),
            -1,
        ),
        (
            scan_status_guest(2, b"", b"", 1, 13, 128, 13, 128),
            principal_read(),
            -1,
        ),
        (
            scan_status_guest(1, b"", b"", 1, 13, 128, 12, 128),
            principal_read(),
            -3,
        ),
        (
            scan_status_guest(1, b"", b"", 1, 13, 65_535, 13, 128),
            principal_read(),
            -3,
        ),
        (
            scan_status_guest(1, b"a", &foreign_cursor, 1, 93, 128, 13, 128),
            principal_read(),
            -2,
        ),
        (
            scan_status_guest(1, b"", &foreign_namespace_cursor, 1, 93, 128, 13, 128),
            principal_read(),
            -2,
        ),
        (
            scan_status_guest(1, b"", &corrupt_cursor, 1, 93, 128, 13, 128),
            principal_read(),
            -2,
        ),
    ] {
        let before = storage.clone();
        let record = execute(&guest, owner, actor, grants, &mut storage, 8);
        assert_eq!(record.execution().outputs(), [WasmValue::I32(0)]);
        assert_eq!(record.execution().usage().storage_read_bytes, 0);
        let response = record
            .response()
            .unwrap_or_else(|| panic!("status response"));
        assert_eq!(response.code, 0);
        let mut expected_response = expected_status.to_le_bytes().to_vec();
        expected_response.extend_from_slice(b"keep");
        assert_eq!(response.bytes, expected_response);
        assert_eq!(storage, before);
    }
}

#[test]
fn candidate_scan_rejects_cross_scope_prefix_and_limit_cursor_reuse() {
    let owner = program(7);
    let actor = principal(8);
    let namespace = StorageNamespace::principal(owner, actor);
    let mut storage = Storage::new();
    seed(&mut storage, namespace, &[(b"a", b"a"), (b"b", b"b")]);
    let first = execute(
        &scan_guest(b"", b"", 1, 93, 128, 93, 128, 93),
        owner,
        actor,
        principal_read(),
        &mut storage,
        93,
    );
    let cursor = first
        .response()
        .unwrap_or_else(|| panic!("first response"))
        .bytes[13..]
        .to_vec();
    for (guest, grants) in [
        (
            scan_guest(b"a", &cursor, 1, 93, 128, 13, 64, 4),
            principal_read(),
        ),
        (
            scan_guest(b"", &cursor, 2, 93, 128, 13, 64, 4),
            principal_read(),
        ),
        (
            scan_guest_selected(2, b"", &cursor, 1, 93, 128, 13, 64, 4),
            CapabilitySet::new([Capability::SharedStorageRead])
                .unwrap_or_else(|error| panic!("shared grant: {error}")),
        ),
    ] {
        let before = storage.clone();
        let record = execute(&guest, owner, actor, grants, &mut storage, 4);
        assert_eq!(record.execution().usage().storage_read_bytes, 0);
        assert_eq!(
            record
                .response()
                .unwrap_or_else(|| panic!("sentinel response"))
                .bytes,
            b"keep"
        );
        assert_eq!(storage, before);
    }
}

#[test]
fn candidate_scan_observes_same_activity_writes_and_a_later_failure_rolls_them_back() {
    let owner = program(9);
    let actor = principal(10);
    let grants = CapabilitySet::new([Capability::StorageRead, Capability::StorageWrite])
        .unwrap_or_else(|error| panic!("grants: {error}"));
    let mut storage = Storage::new();
    let success = execute(
        &write_then_scan_guest(13, false),
        owner,
        actor,
        grants.clone(),
        &mut storage,
        13,
    );
    assert_eq!(success.execution().usage().storage_read_bytes, 13);
    assert_eq!(
        success
            .response()
            .unwrap_or_else(|| panic!("same-activity response"))
            .bytes,
        vec![0, 1, 0, 1, b'a', 0, 0, 0, 1, b'b', 0, 0, 0]
    );
    assert_eq!(
        storage
            .transaction(StorageNamespace::principal(owner, actor))
            .read(b"a"),
        Ok(Some(b"b".to_vec()))
    );

    let mut refused = Storage::new();
    let before = refused.clone();
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_candidate_v2(&write_then_scan_guest(12, true))
        .unwrap_or_else(|error| panic!("candidate validation: {error}"));
    assert!(Executor::declared()
        .execute_authorized_candidate(
            &mut refused,
            AuthorizedExecutionRequest {
                module: &module,
                program: owner,
                authorization: AuthorizationContext::new(actor, grants),
                receipts: &NoReceipts,
                entrypoint: CALL_ENTRY_EXPORT,
                calldata: &[],
                composition: CompositionContext::isolated(),
                response_capacity: 0,
            },
        )
        .is_err());
    assert_eq!(refused, before);
}
