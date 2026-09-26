use std::path::PathBuf;
use std::sync::Arc;

use layerx_programs_runtime::test_support::{
    code_section, func_body, function_section, import_section, module, type_section, unsigned_leb,
    TYPE_I32,
};
use layerx_programs_runtime::{
    abi_manifest, Abi, AbiError, AuthorizationContext, Capability, CapabilitySet,
    CommittedWebAnswers, FeeSchedule, Meter, PrincipalId, ProgramId, ReceiptOracle, ReceiptView,
    ResourceBudget, Storage, StorageNamespace, ValidationRefusal, WasmEngine, WasmValue, WebAnswer,
    ABI_MANIFEST, ABI_MODULE, ABI_V1_MANIFEST, ABI_V1_VERSION, ABI_V2_MANIFEST, ABI_V2_VERSION,
    ABI_V3_MANIFEST, ABI_V3_VERSION, ABI_V4_HOST_FUNCTIONS, ABI_V4_MANIFEST, ABI_V4_MODULE,
    ABI_V4_VERSION, ABI_VERSION, WEB_ANSWER_HEADER_BYTES,
};
use sha2::{Digest, Sha256};

const STATUS_INVALID: i32 = -2;
const STATUS_BOUNDS: i32 = -3;
const STATUS_ABSENT: i32 = -7;
const OWNED_REQUEST_POINTER: i32 = 16;
const ABSENT_REQUEST_POINTER: i32 = 24;
const FOREIGN_REQUEST_POINTER: i32 = 32;
const OUTPUT_POINTER: i32 = 256;
const OUTPUT_CAPACITY: i32 = 4_136;
const OWNED_REQUEST: u64 = 0x0102_0304_0506_0708;
const ABSENT_REQUEST: u64 = 0x0102_0304_0506_0709;
const FOREIGN_REQUEST: u64 = 0x0102_0304_0506_070a;
const RESPONSE: &[u8] = b"Paxeer X Network";
const FULL_LENGTH: u32 = 5_000;

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

fn principal() -> PrincipalId {
    PrincipalId::new([0x33; 32]).unwrap_or_else(|error| panic!("principal: {error}"))
}

fn section(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![id];
    bytes.extend(unsigned_leb(payload.len() as u64));
    bytes.extend_from_slice(payload);
    bytes
}

fn data_segments(segments: &[(u8, &[u8])]) -> Vec<u8> {
    let mut payload = unsigned_leb(segments.len() as u64);
    for (offset, bytes) in segments {
        payload.extend_from_slice(&[0, 0x41, *offset, 0x0b]);
        payload.extend(unsigned_leb(bytes.len() as u64));
        payload.extend_from_slice(bytes);
    }
    section(11, &payload)
}

/// Guest exporting `read(request_pointer, request_length, output_pointer,
/// output_capacity)`: it calls `web_read` and, on success, stores the exact
/// record the host wrote under the principal key `answer`.
fn web_guest() -> Vec<u8> {
    let types = type_section(&[(&[TYPE_I32; 4], &[TYPE_I32])]);
    let imports = import_section(&[
        (ABI_V4_MODULE, "web_read", 0),
        (ABI_MODULE, "storage_write", 0),
    ]);
    let memory = section(5, &[1, 1, 1, 1]);
    let mut exports = unsigned_leb(2);
    for (name, kind, index) in [("read", 0u8, 2u8), ("memory", 2, 0)] {
        exports.extend(unsigned_leb(name.len() as u64));
        exports.extend_from_slice(name.as_bytes());
        exports.extend_from_slice(&[kind, index]);
    }
    let body = [
        0x20, 0, 0x20, 1, 0x20, 2, 0x20, 3, 0x10, 0, 0x22, 4, 0x41, 0, 0x4a, 0x04, 0x40, 0x41, 0,
        0x41, 6, 0x20, 2, 0x20, 4, 0x10, 1, 0x1a, 0x0b, 0x20, 4, 0x0b,
    ];
    module(&[
        types,
        imports,
        function_section(&[0]),
        memory,
        section(7, &exports),
        code_section(&[func_body(&[(1, TYPE_I32)], &body)]),
        data_segments(&[
            (0, b"answer"),
            (16, &OWNED_REQUEST.to_le_bytes()),
            (24, &ABSENT_REQUEST.to_le_bytes()),
            (32, &FOREIGN_REQUEST.to_le_bytes()),
        ]),
    ])
}

fn answer() -> WebAnswer {
    WebAnswer {
        content_digest: [0x5a; 32],
        full_length: FULL_LENGTH,
        response: RESPONSE.to_vec(),
    }
}

fn committed() -> CommittedWebAnswers {
    let mut answers = CommittedWebAnswers::new();
    answers
        .commit(program(0xa1), OWNED_REQUEST, answer())
        .unwrap_or_else(|error| panic!("owned answer: {error}"));
    answers
        .commit(
            program(0xb2),
            FOREIGN_REQUEST,
            WebAnswer {
                content_digest: [0x6b; 32],
                full_length: 4,
                response: b"else".to_vec(),
            },
        )
        .unwrap_or_else(|error| panic!("foreign answer: {error}"));
    answers
}

fn expected_record() -> Vec<u8> {
    let mut record = vec![0x5a; 32];
    record.extend_from_slice(&FULL_LENGTH.to_le_bytes());
    record.extend_from_slice(&u32::try_from(RESPONSE.len()).unwrap_or(0).to_le_bytes());
    record.extend_from_slice(RESPONSE);
    record
}

struct Outcome {
    status: i32,
    stored: Option<Vec<u8>>,
}

fn run(owner: ProgramId, web: Option<CommittedWebAnswers>, args: [i32; 4]) -> Outcome {
    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let module = engine
        .validate_v4(&web_guest())
        .unwrap_or_else(|error| panic!("v4 guest validation: {error}"));
    let abi = Abi::new(
        ABI_V4_VERSION,
        owner,
        AuthorizationContext::new(
            principal(),
            CapabilitySet::new([Capability::StorageWrite])
                .unwrap_or_else(|error| panic!("grants: {error}")),
        ),
        Storage::new(),
        &NoReceipts,
    )
    .unwrap_or_else(|error| panic!("abi: {error}"));
    let abi = match web {
        Some(answers) => abi.with_committed_web(Arc::new(answers)),
        None => abi,
    };
    let mut instance = module
        .instantiate_sandbox(
            Meter::new(ResourceBudget::declared(), FeeSchedule::declared()),
            abi,
        )
        .unwrap_or_else(|error| panic!("instantiate: {error}"));
    let values = instance
        .call("read", &args.map(WasmValue::I32))
        .unwrap_or_else(|error| panic!("read call: {error}"));
    let status = match values.as_slice() {
        [WasmValue::I32(status)] => *status,
        other => panic!("unexpected results {other:?}"),
    };
    let mut storage = instance
        .storage_snapshot()
        .unwrap_or_else(|| panic!("sandbox storage"));
    let stored = storage
        .transaction(StorageNamespace::principal(owner, principal()))
        .read(b"answer")
        .unwrap_or_else(|error| panic!("storage read: {error}"));
    Outcome { status, stored }
}

#[test]
fn present_read_returns_digest_full_length_and_response() {
    let outcome = run(
        program(0xa1),
        Some(committed()),
        [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    let record = expected_record();
    assert_eq!(
        outcome.status,
        i32::try_from(WEB_ANSWER_HEADER_BYTES + RESPONSE.len()).unwrap_or(0)
    );
    assert_eq!(outcome.stored, Some(record.clone()));
    assert_eq!(answer().canonical_bytes(), Ok(record.clone()));

    let exact = run(
        program(0xa1),
        Some(committed()),
        [
            OWNED_REQUEST_POINTER,
            8,
            OUTPUT_POINTER,
            i32::try_from(record.len()).unwrap_or(0),
        ],
    );
    assert_eq!(exact.stored, Some(record));
}

#[test]
fn absent_read_returns_the_absent_code() {
    let outcome = run(
        program(0xa1),
        Some(committed()),
        [ABSENT_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    assert_eq!(outcome.status, STATUS_ABSENT);
    assert_eq!(outcome.stored, None);

    let unattached = run(
        program(0xa1),
        None,
        [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    assert_eq!(unattached.status, STATUS_ABSENT);
    assert_eq!(unattached.stored, None);
}

#[test]
fn another_programs_request_is_never_visible() {
    let outcome = run(
        program(0xa1),
        Some(committed()),
        [FOREIGN_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    assert_eq!(outcome.status, STATUS_ABSENT);
    assert_eq!(outcome.stored, None);

    let stranger = run(
        program(0xb2),
        Some(committed()),
        [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    assert_eq!(stranger.status, STATUS_ABSENT);
    assert_eq!(stranger.stored, None);

    let owner = run(
        program(0xb2),
        Some(committed()),
        [FOREIGN_REQUEST_POINTER, 8, OUTPUT_POINTER, OUTPUT_CAPACITY],
    );
    let mut record = vec![0x6b; 32];
    record.extend_from_slice(&4u32.to_le_bytes());
    record.extend_from_slice(&4u32.to_le_bytes());
    record.extend_from_slice(b"else");
    assert_eq!(owner.status, 44);
    assert_eq!(owner.stored, Some(record));
}

#[test]
fn every_bad_buffer_is_refused_without_a_write() {
    let record_length = i32::try_from(expected_record().len()).unwrap_or(0);
    let cases = [
        (
            [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, 39],
            STATUS_BOUNDS,
        ),
        (
            [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, record_length - 1],
            STATUS_BOUNDS,
        ),
        (
            [OWNED_REQUEST_POINTER, 8, OUTPUT_POINTER, -1],
            STATUS_INVALID,
        ),
        (
            [OWNED_REQUEST_POINTER, 8, 65_530, OUTPUT_CAPACITY],
            STATUS_BOUNDS,
        ),
        (
            [OWNED_REQUEST_POINTER, 8, -1, OUTPUT_CAPACITY],
            STATUS_INVALID,
        ),
        (
            [OWNED_REQUEST_POINTER, 7, OUTPUT_POINTER, OUTPUT_CAPACITY],
            STATUS_INVALID,
        ),
        (
            [OWNED_REQUEST_POINTER, 9, OUTPUT_POINTER, OUTPUT_CAPACITY],
            STATUS_INVALID,
        ),
        ([65_532, 8, OUTPUT_POINTER, OUTPUT_CAPACITY], STATUS_BOUNDS),
        ([-1, 8, OUTPUT_POINTER, OUTPUT_CAPACITY], STATUS_INVALID),
    ];
    for (args, expected) in cases {
        let outcome = run(program(0xa1), Some(committed()), args);
        assert_eq!(outcome.status, expected, "arguments {args:?}");
        assert_eq!(outcome.stored, None, "arguments {args:?}");
    }
}

#[test]
fn committed_answers_refuse_oversized_inconsistent_and_repeated_records() {
    let mut answers = committed();
    assert_eq!(
        answers.commit(program(0xa1), OWNED_REQUEST, answer()),
        Err(AbiError::InvalidEncoding)
    );
    assert_eq!(
        answers.commit(
            program(0xa1),
            ABSENT_REQUEST,
            WebAnswer {
                content_digest: [1; 32],
                full_length: 2,
                response: b"abc".to_vec(),
            },
        ),
        Err(AbiError::InvalidEncoding)
    );
    assert_eq!(
        answers.commit(
            program(0xa1),
            ABSENT_REQUEST,
            WebAnswer {
                content_digest: [1; 32],
                full_length: u32::MAX,
                response: vec![0; 4_097],
            },
        ),
        Err(AbiError::InvalidEncoding)
    );
}

fn vector(version: u16) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../tests/vectors/abi-v{version}.hex"));
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn frozen_checksums() -> Vec<(u16, String)> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../abi-frozen.sha256");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let (version, digest) = line
                .split_once(' ')
                .unwrap_or_else(|| panic!("checksum line {line}"));
            (
                version
                    .parse()
                    .unwrap_or_else(|error| panic!("checksum version {version}: {error}")),
                digest.to_string(),
            )
        })
        .collect()
}

fn encoded(version: u16, manifest: &str) -> String {
    let mut bytes = version.to_be_bytes().to_vec();
    bytes.extend_from_slice(manifest.as_bytes());
    let mut hex = String::new();
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.push('\n');
    hex
}

#[test]
fn frozen_manifests_are_unchanged_and_v4_only_appends_web_read() {
    let checksums = frozen_checksums();
    for (version, manifest) in [
        (ABI_V1_VERSION, ABI_V1_MANIFEST),
        (ABI_V2_VERSION, ABI_V2_MANIFEST),
        (ABI_V3_VERSION, ABI_V3_MANIFEST),
        (ABI_V4_VERSION, ABI_V4_MANIFEST),
    ] {
        let vector = vector(version);
        assert_eq!(vector, encoded(version, manifest), "ABI v{version} vector");
        let digest = Sha256::digest(vector.as_bytes());
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(
            checksums.contains(&(version, hex)),
            "ABI v{version} checksum is not frozen"
        );
        assert_eq!(abi_manifest(version), Some(manifest));
    }
    assert_eq!((ABI_V1_VERSION, ABI_V2_VERSION, ABI_V3_VERSION), (1, 2, 3));
    assert_eq!((ABI_VERSION, ABI_V4_VERSION), (4, 4));
    assert_eq!(ABI_MANIFEST, ABI_V4_MANIFEST);
    assert_eq!(
        ABI_V4_MANIFEST.strip_prefix(ABI_V3_MANIFEST),
        Some("layerx_v4\0web_read(i32,i32,i32,i32)->i32\0")
    );
    assert!(!ABI_V3_MANIFEST.contains("layerx_v4"));
    assert_eq!(ABI_V4_HOST_FUNCTIONS.len(), 1);
    assert_eq!(abi_manifest(5), None);

    let engine = WasmEngine::declared().unwrap_or_else(|error| panic!("engine: {error}"));
    let guest = web_guest();
    for refusal in [
        engine.validate(&guest).err(),
        engine.validate_v2(&guest).err(),
        engine.validate_v3(&guest).err(),
    ] {
        assert!(matches!(
            refusal,
            Some(ValidationRefusal::ForbiddenImport { .. })
        ));
    }
    assert!(engine.validate_versioned(4, &guest).is_ok());
}
