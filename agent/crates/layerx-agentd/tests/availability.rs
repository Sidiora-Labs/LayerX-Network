use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use layerx_agentd::read::{availability, AvailabilityAudit, AvailabilityRead, AvailabilityRequest};
use layerx_agentd::store::{ObjectKind, Store, TenantId};
use layerx_client::availability::{
    AvailabilitySelector, FetchContext, Provider, ProviderSet, RetrievalLimits,
};
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_proof::availability::{AvailabilityCheck, AvailabilityClass, Chunk, RootCommitments};
use layerx_proof::merkle::{build_leaf_hash_proof, root, Proof};
use layerx_wire::encode::Encoder;
use layerx_wire::hash::availability_chunk_digest;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-agentd-availability-{label}-{}-{sequence}.sock",
            std::process::id()
        )))
    }
}

impl Drop for SocketPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Clone)]
struct Fixture {
    chunks: Vec<Chunk>,
    proofs: Vec<Proof>,
    availability_root: [u8; 32],
    record_roots: RootCommitments,
}

fn sequence(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1024);
    assert_eq!(encoder.sequence_length(1, 1), Ok(()));
    assert_eq!(encoder.bytes(bytes, 1024), Ok(()));
    encoder.finish()
}

fn tagged(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut encoder = Encoder::new(1024);
    assert_eq!(encoder.u8(kind), Ok(()));
    assert_eq!(encoder.bytes(bytes, 1024), Ok(()));
    encoder.finish()
}

fn fixture() -> Fixture {
    let activities = sequence(b"activity");
    let mut receipts = tagged(1, b"receipt");
    receipts.extend_from_slice(&tagged(2, b"event"));
    let classes = [
        (AvailabilityClass::Activities, activities),
        (AvailabilityClass::Receipts, receipts),
        (AvailabilityClass::Oracle, sequence(b"oracle")),
        (AvailabilityClass::StateDiff, b"state-diff".to_vec()),
        (AvailabilityClass::Recovery, b"recovery".to_vec()),
    ];
    let mut chunks = Vec::new();
    for (index, (class, bytes)) in classes.into_iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(0);
        let claimed_hash = availability_chunk_digest(7, index, class as u8, 0, &bytes)
            .unwrap_or_else(|error| panic!("chunk digest: {error:?}"));
        chunks.push(Chunk {
            batch_number: 7,
            index,
            class,
            class_offset: 0,
            bytes,
            claimed_hash,
        });
    }
    let hashes: Vec<_> = chunks.iter().map(|chunk| chunk.claimed_hash).collect();
    let mut proofs = Vec::new();
    let mut availability_root = [0; 32];
    for index in 0..hashes.len() {
        let (proof, computed) = build_leaf_hash_proof(&hashes, index)
            .unwrap_or_else(|error| panic!("availability proof: {error:?}"));
        proofs.push(proof);
        availability_root = computed;
    }
    Fixture {
        chunks,
        proofs,
        availability_root,
        record_roots: RootCommitments {
            activity: root(&[b"activity"])
                .unwrap_or_else(|error| panic!("activity root: {error:?}")),
            receipt: root(&[b"receipt"]).unwrap_or_else(|error| panic!("receipt root: {error:?}")),
            event: root(&[b"event"]).unwrap_or_else(|error| panic!("event root: {error:?}")),
            oracle: root(&[b"oracle"]).unwrap_or_else(|error| panic!("oracle root: {error:?}")),
        },
    }
}

fn transport_limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 4,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn context(fixture: &Fixture, correlation_id: u64) -> FetchContext {
    FetchContext {
        interface_version: Version::V1_0,
        correlation_id,
        expected_batch_number: 7,
        data_availability_root: fixture.availability_root,
        record_roots: fixture.record_roots,
        limits: RetrievalLimits {
            maximum_bytes: 1024,
            maximum_chunks: 8,
            deadline: Duration::from_secs(2),
        },
    }
}

fn metadata(chunk: &Chunk, proof: &Proof) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&chunk.batch_number.to_be_bytes());
    bytes.extend_from_slice(&chunk.index.to_be_bytes());
    bytes.push(chunk.class as u8);
    bytes.extend_from_slice(&chunk.class_offset.to_be_bytes());
    bytes.extend_from_slice(&chunk.claimed_hash);
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.push(
        u8::try_from(proof.siblings().len())
            .unwrap_or_else(|error| panic!("sibling count: {error}")),
    );
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    bytes
}

fn request(stream: &mut UnixStream) -> u64 {
    let frame =
        read_frame(stream, 1024 * 1024).unwrap_or_else(|error| panic!("request frame: {error:?}"));
    let envelope =
        decode_envelope(&frame).unwrap_or_else(|error| panic!("request envelope: {error:?}"));
    assert_eq!(envelope.message_tag, 18);
    envelope.correlation_id
}

fn response(stream: &mut UnixStream, tag: u16, correlation: u64, bytes: &[u8], proof: &[u8]) {
    let envelope = encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id: correlation,
        canonical_payload: bytes,
        proof_material: proof,
    })
    .unwrap_or_else(|error| panic!("response envelope: {error:?}"));
    let _ = write_frame(stream, &envelope, 1024 * 1024);
}

fn provider(
    socket: &SocketPath,
    fixture: Fixture,
    chunks: usize,
    corrupt: Option<usize>,
) -> JoinHandle<()> {
    let listener =
        UnixListener::bind(&socket.0).unwrap_or_else(|error| panic!("listener: {error}"));
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .unwrap_or_else(|error| panic!("accept: {error}"));
        let correlation = request(&mut stream);
        for index in 0..chunks {
            let mut bytes = fixture.chunks[index].bytes.clone();
            if corrupt == Some(index) {
                bytes[0] ^= 1;
            }
            response(
                &mut stream,
                19,
                correlation,
                &bytes,
                &metadata(&fixture.chunks[index], &fixture.proofs[index]),
            );
        }
        response(&mut stream, 20, correlation, &[], &[]);
    })
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

#[test]
fn complete_five_class_retrieval_streams_and_emits_offline_replay_frames() {
    let fixture = fixture();
    let socket = SocketPath::new("complete");
    let server = provider(&socket, fixture.clone(), 5, None);
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&socket.0, &gate, transport_limits())
        .unwrap_or_else(|error| panic!("connect: {error:?}"));
    let mut providers = ProviderSet::new(vec![Provider {
        name: "provider-a".to_owned(),
        transport: &mut transport,
    }]);
    let root =
        std::env::temp_dir().join(format!("layerx-availability-store-{}", std::process::id()));
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut audit = AvailabilityAudit::default();
    let mut streamed = 0;
    let outcome = availability(
        &mut store,
        &tenant(),
        &mut audit,
        &mut providers,
        &AvailabilityRequest {
            selector: AvailabilitySelector::Batch(7),
            checkpoint_id: [0x77; 32],
            context: context(&fixture, 70),
        },
        |_| streamed += 1,
    )
    .unwrap_or_else(|error| panic!("availability: {error:?}"));
    let AvailabilityRead::Complete {
        report,
        replay_bytes,
        framing,
        ..
    } = outcome
    else {
        panic!("complete provider reported partial")
    };
    assert_eq!(streamed, 5);
    assert_eq!(report.classes.obtained.len(), 5);
    assert!(report.classes.missing.is_empty());
    assert_eq!(replay_bytes.get(..5), Some(&b"LXRP\x01"[..]));
    assert_eq!(
        framing.record_order,
        "flattened availability chunk index ascending"
    );
    assert!(audit.failures().is_empty());
    assert!(server.join().is_ok(), "provider failed");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn corruption_withholding_and_repeated_provider_failure_are_durable_evidence() {
    let fixture = fixture();
    let root = std::env::temp_dir().join(format!(
        "layerx-availability-failures-{}",
        std::process::id()
    ));
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error}"));
    let mut audit = AvailabilityAudit::default();
    let gate = ConnectionGate::new(1);

    for (attempt, (chunk_count, corrupt)) in [(2, Some(1)), (2, None)].into_iter().enumerate() {
        let socket = SocketPath::new("failure");
        let server = provider(&socket, fixture.clone(), chunk_count, corrupt);
        let mut transport = Uds::connect(&socket.0, &gate, transport_limits())
            .unwrap_or_else(|error| panic!("connect: {error:?}"));
        let mut providers = ProviderSet::new(vec![Provider {
            name: "provider-a".to_owned(),
            transport: &mut transport,
        }]);
        let outcome = availability(
            &mut store,
            &tenant(),
            &mut audit,
            &mut providers,
            &AvailabilityRequest {
                selector: AvailabilitySelector::Checkpoint([0x77; 32]),
                checkpoint_id: [0x77; 32],
                context: context(&fixture, 80 + u64::try_from(attempt).unwrap_or(0)),
            },
            |_| {},
        )
        .unwrap_or_else(|error| panic!("availability: {error:?}"));
        let AvailabilityRead::Partial { failures, .. } = outcome else {
            panic!("failed provider reported complete")
        };
        assert_eq!(failures.len(), 1);
        assert_eq!(
            failures[0].provider_failure_count,
            u64::try_from(attempt + 1).unwrap_or(0)
        );
        if attempt == 0 {
            assert_eq!(failures[0].check, AvailabilityCheck::ChunkHash);
            assert!(!failures[0].served_bytes.is_empty());
            assert_eq!(
                failures[0].mismatching_commitment,
                fixture.chunks[1].claimed_hash
            );
        } else {
            assert_eq!(failures[0].check, AvailabilityCheck::MissingClass);
            assert_eq!(failures[0].classes.obtained.len(), 2);
            assert_eq!(failures[0].classes.missing.len(), 3);
        }
        assert!(server.join().is_ok(), "provider failed");
    }
    assert_eq!(audit.provider_failure_count("provider-a"), 2);
    assert_eq!(audit.failures().len(), 2);
    assert_eq!(store.list_object_ids(&tenant(), ObjectKind::Audit).len(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn class_presence_precedes_decoding_and_complete_chunks_require_order() {
    for (case, order, expected) in [
        ("empty", vec![], Some(AvailabilityCheck::MissingClass)),
        ("missing", vec![0, 1], Some(AvailabilityCheck::MissingClass)),
        (
            "missing-reordered",
            vec![1, 0],
            Some(AvailabilityCheck::MissingClass),
        ),
        (
            "complete-reordered",
            vec![1, 0, 2, 3, 4],
            Some(AvailabilityCheck::ChunkOrder),
        ),
        ("complete", vec![0, 1, 2, 3, 4], None),
    ] {
        let mut fixture = fixture();
        let original = fixture.clone();
        fixture.chunks = order
            .iter()
            .map(|&index| original.chunks[index].clone())
            .collect();
        fixture.proofs = order
            .iter()
            .map(|&index| original.proofs[index].clone())
            .collect();
        let socket = SocketPath::new(case);
        let server = provider(&socket, fixture.clone(), order.len(), None);
        let gate = ConnectionGate::new(1);
        let mut transport = Uds::connect(&socket.0, &gate, transport_limits())
            .unwrap_or_else(|error| panic!("connect: {error:?}"));
        let mut providers = ProviderSet::new(vec![Provider {
            name: "provider-a".to_owned(),
            transport: &mut transport,
        }]);
        let outcome = layerx_client::availability::fetch(
            &mut providers,
            AvailabilitySelector::Batch(7),
            context(&fixture, 90),
            |_| {},
        )
        .unwrap_or_else(|error| panic!("fetch: {error:?}"));
        match (expected, outcome) {
            (None, layerx_client::availability::FetchOutcome::Complete(result)) => {
                assert!(result.report.classes.missing.is_empty());
                assert_eq!(result.records().activities, vec![b"activity".to_vec()]);
                assert_eq!(result.records().oracle_inputs, vec![b"oracle".to_vec()]);
            }
            (Some(expected), layerx_client::availability::FetchOutcome::Partial(reports)) => {
                assert_eq!(reports.len(), 1);
                let layerx_client::availability::ProviderFailure::Reassembly(failure) =
                    &reports[0].failure
                else {
                    panic!("expected reassembly failure: {reports:?}");
                };
                assert_eq!(failure.check, expected, "{case}");
                assert_eq!(failure.classes, reports[0].classes);
                assert_eq!(
                    failure.served_bytes,
                    fixture
                        .chunks
                        .iter()
                        .flat_map(|chunk| chunk.bytes.clone())
                        .collect::<Vec<_>>()
                );
            }
            (expected, outcome) => panic!("{case}: expected {expected:?}, got {outcome:?}"),
        }
        assert!(server.join().is_ok(), "provider failed");
    }
}

#[test]
fn withheld_recovery_tail_retains_evidence_without_emitting_complete_replay() {
    let mut fixture = fixture();
    let mut tail = fixture.chunks[4].clone();
    tail.index = 5;
    tail.class_offset =
        u64::try_from(tail.bytes.len()).unwrap_or_else(|error| panic!("offset: {error:?}"));
    tail.bytes = b"withheld recovery".to_vec();
    tail.claimed_hash =
        availability_chunk_digest(7, 5, tail.class as u8, tail.class_offset, &tail.bytes)
            .unwrap_or_else(|error| panic!("tail digest: {error:?}"));
    fixture.chunks.push(tail);
    let hashes: Vec<_> = fixture
        .chunks
        .iter()
        .map(|chunk| chunk.claimed_hash)
        .collect();
    fixture.proofs.clear();
    for index in 0..hashes.len() {
        let (proof, root) = build_leaf_hash_proof(&hashes, index)
            .unwrap_or_else(|error| panic!("proof: {error:?}"));
        fixture.proofs.push(proof);
        fixture.availability_root = root;
    }
    let socket = SocketPath::new("recovery-tail");
    let server = provider(&socket, fixture.clone(), 5, None);
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&socket.0, &gate, transport_limits())
        .unwrap_or_else(|error| panic!("transport: {error:?}"));
    let mut providers = ProviderSet::new(vec![Provider {
        name: "recovery-tail".to_owned(),
        transport: &mut transport,
    }]);
    let root =
        std::env::temp_dir().join(format!("layerx-availability-tail-{}", std::process::id()));
    let mut store = Store::open(&root).unwrap_or_else(|error| panic!("store: {error:?}"));
    let mut audit = AvailabilityAudit::default();
    let outcome = availability(
        &mut store,
        &tenant(),
        &mut audit,
        &mut providers,
        &AvailabilityRequest {
            selector: AvailabilitySelector::Batch(7),
            checkpoint_id: [0x77; 32],
            context: context(&fixture, 100),
        },
        |_| {},
    )
    .unwrap_or_else(|error| panic!("availability: {error:?}"));
    let AvailabilityRead::Partial { failures, .. } = outcome else {
        panic!("withheld recovery cannot produce complete replay frames");
    };
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].check, AvailabilityCheck::BundleCompleteness);
    assert!(failures[0].classes.missing.is_empty());
    assert_eq!(
        failures[0].mismatching_commitment,
        fixture.availability_root
    );
    assert_eq!(
        failures[0].served_bytes,
        fixture.chunks[..5]
            .iter()
            .flat_map(|chunk| chunk.bytes.iter().copied())
            .collect::<Vec<_>>()
    );
    assert_eq!(audit.provider_failure_count("recovery-tail"), 1);
    assert_eq!(store.list_object_ids(&tenant(), ObjectKind::Audit).len(), 1);
    assert!(server.join().is_ok(), "provider failed");
    drop(store);
    let recovered = Store::open(&root).unwrap_or_else(|error| panic!("recovered store: {error:?}"));
    assert_eq!(
        recovered
            .list_object_ids(&tenant(), ObjectKind::Audit)
            .len(),
        1
    );
    drop(recovered);
    let _ = fs::remove_dir_all(root);
}
