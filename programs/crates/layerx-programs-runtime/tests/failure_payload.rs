use layerx_programs_runtime::{
    ProgramFailure, ProgramId, RefusalClass, RefusalReason, MAX_REFUSAL_REASON_BYTES,
};

#[test]
fn frozen_v2_alias_contains_the_complete_host_table() {
    let table = layerx_programs_runtime::abi::response::CANDIDATE_HOST_FUNCTIONS;
    assert_eq!(table.len(), 19);
    assert_eq!(table[2].name, "refusal_write");
    assert_eq!(table[2].signature, "(i32,i32,i32)->i32");
    assert_eq!(table[3].name, "storage_read_scoped");
    assert_eq!(table[3].signature, "(i32,i32,i32,i32,i32)->i32");
    assert_eq!(table[4].name, "storage_write_scoped");
    assert_eq!(table[4].signature, "(i32,i32,i32,i32,i32)->i32");
    assert_eq!(table[5].name, "storage_delete_scoped");
    assert_eq!(table[5].signature, "(i32,i32,i32)->i32");
    assert_eq!(table[6].name, "storage_drop_scoped");
    assert_eq!(table[6].signature, "(i32)->i32");
    assert_eq!(table[7].name, "storage_scan_scoped");
    assert_eq!(
        table[7].signature,
        "(i32,i32,i32,i32,i32,i32,i32,i32,i32)->i32"
    );
    assert_eq!(table[10].name, "context_read");
    assert_eq!(table[11].name, "balance_read");
    assert_eq!(table[18].name, "bigint_modexp_256");
}

#[test]
fn refusal_reason_bounds_and_canonical_roundtrip_are_strict() {
    let empty = RefusalReason::new(&[]).unwrap_or_else(|error| panic!("empty: {error}"));
    assert!(empty.bytes().is_empty());
    let maximum_bytes = vec![0xa5; MAX_REFUSAL_REASON_BYTES];
    let maximum =
        RefusalReason::new(&maximum_bytes).unwrap_or_else(|error| panic!("maximum: {error}"));
    assert_eq!(maximum.bytes(), maximum_bytes);
    assert!(RefusalReason::new(&vec![0; MAX_REFUSAL_REASON_BYTES + 1]).is_err());
    let oversized_length = u32::try_from(MAX_REFUSAL_REASON_BYTES + 1)
        .unwrap_or_else(|error| panic!("test bound: {error}"));
    let mut fully_present_over = oversized_length.to_be_bytes().to_vec();
    fully_present_over.extend(vec![0; MAX_REFUSAL_REASON_BYTES + 1]);
    assert!(RefusalReason::canonical_decode(&fully_present_over).is_err());

    let encoded = maximum.canonical_encode();
    let decoded =
        RefusalReason::canonical_decode(&encoded).unwrap_or_else(|error| panic!("decode: {error}"));
    assert_eq!(decoded, maximum);

    for malformed in [encoded[..encoded.len() - 1].to_vec(), {
        let mut trailing = encoded.clone();
        trailing.push(0);
        trailing
    }] {
        assert!(RefusalReason::canonical_decode(&malformed).is_err());
    }
}

#[test]
fn program_failure_roundtrip_binds_host_program_identity() {
    let program = ProgramId::new([7; 32]).unwrap_or_else(|error| panic!("program: {error}"));
    let reason =
        RefusalReason::new(&[0, 0xff, 0x80]).unwrap_or_else(|error| panic!("reason: {error}"));
    let failure = ProgramFailure::new(program, RefusalClass::InvalidInput, reason)
        .unwrap_or_else(|error| panic!("failure: {error}"));
    let encoded = failure.canonical_encode();
    let decoded = ProgramFailure::canonical_decode(&encoded)
        .unwrap_or_else(|error| panic!("decode: {error}"));
    assert_eq!(decoded, failure);
    assert_eq!(decoded.program(), program);
    assert_eq!(decoded.reason().bytes(), [0, 0xff, 0x80]);

    let mut unknown = encoded;
    unknown[35] = 99;
    assert!(ProgramFailure::canonical_decode(&unknown).is_err());

    for class in [RefusalClass::RuntimeFault, RefusalClass::Legacy] {
        let nonempty = RefusalReason::new(&[1]).unwrap_or_else(|error| panic!("reason: {error}"));
        assert!(ProgramFailure::new(program, class, nonempty).is_err());
        let mut tampered = Vec::new();
        tampered.extend_from_slice(&program.bytes());
        tampered.extend_from_slice(&class.code().to_be_bytes());
        tampered.extend_from_slice(&1u32.to_be_bytes());
        tampered.push(1);
        assert!(ProgramFailure::canonical_decode(&tampered).is_err());
    }
}
