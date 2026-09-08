use std::fs;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use layerx_client::availability::{
    fetch, AvailabilitySelector, FetchContext, FetchError, FetchOutcome, Provider, ProviderFailure,
    ProviderSet, RetrievalLimits,
};
use layerx_client::lni::framing::{read_frame, write_frame};
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, Limits, Uds};
use layerx_proof::availability::{AvailabilityCheck, AvailabilityClass, Chunk, RootCommitments};
use layerx_proof::merkle::{build_leaf_hash_proof, root, Proof};
use layerx_wire::hash::availability_chunk_digest;

static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

struct SocketPath(PathBuf);

impl SocketPath {
    fn new(label: &str) -> Self {
        let sequence = NEXT_SOCKET.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "layerx-availability-{label}-{}-{sequence}.sock",
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

fn framed_record(bytes: &[u8]) -> Vec<u8> {
    let length = match u32::try_from(bytes.len()) {
        Ok(length) => length,
        Err(error) => panic!("record length failed: {error}"),
    };
    let mut encoded = length.to_be_bytes().to_vec();
    encoded.extend_from_slice(bytes);
    encoded
}

fn tagged_record(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut encoded = vec![kind];
    encoded.extend_from_slice(&framed_record(bytes));
    encoded
}

fn fixture() -> Fixture {
    let mut activities = 1_u32.to_be_bytes().to_vec();
    activities.extend_from_slice(&framed_record(b"activity"));
    let mut receipts = tagged_record(1, b"receipt");
    receipts.extend_from_slice(&tagged_record(2, b"event"));
    let mut oracle = 1_u32.to_be_bytes().to_vec();
    oracle.extend_from_slice(&framed_record(b"oracle"));
    let class_bytes = [
        (AvailabilityClass::Activities, activities),
        (AvailabilityClass::Receipts, receipts),
        (AvailabilityClass::Oracle, oracle),
        (AvailabilityClass::StateDiff, b"state-diff".to_vec()),
        (AvailabilityClass::Recovery, b"recovery".to_vec()),
    ];
    let mut chunks = Vec::new();
    for (index, (class, bytes)) in class_bytes.into_iter().enumerate() {
        let index = u32::try_from(index).unwrap_or_default();
        let claimed_hash = match availability_chunk_digest(7, index, class as u8, 0, &bytes) {
            Ok(hash) => hash,
            Err(error) => panic!("chunk digest failed: {error:?}"),
        };
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
        let (proof, computed) = match build_leaf_hash_proof(&hashes, index) {
            Ok(result) => result,
            Err(error) => panic!("availability proof failed: {error:?}"),
        };
        proofs.push(proof);
        availability_root = computed;
    }
    let record_roots = RootCommitments {
        activity: root(&[b"activity"])
            .unwrap_or_else(|error| panic!("activity root failed: {error:?}")),
        receipt: root(&[b"receipt"])
            .unwrap_or_else(|error| panic!("receipt root failed: {error:?}")),
        event: root(&[b"event"]).unwrap_or_else(|error| panic!("event root failed: {error:?}")),
        oracle: root(&[b"oracle"]).unwrap_or_else(|error| panic!("oracle root failed: {error:?}")),
    };
    Fixture {
        chunks,
        proofs,
        availability_root,
        record_roots,
    }
}

fn limits() -> Limits {
    Limits {
        maximum_frame_bytes: 1024 * 1024,
        maximum_connections: 1,
        maximum_streams: 4,
        maximum_queued_bytes: 2 * 1024 * 1024,
        deadline: Duration::from_secs(2),
    }
}

fn context(fixture: &Fixture, maximum_bytes: usize, maximum_chunks: usize) -> FetchContext {
    FetchContext {
        interface_version: Version::V1_0,
        correlation_id: 70,
        expected_batch_number: 7,
        data_availability_root: fixture.availability_root,
        record_roots: fixture.record_roots,
        limits: RetrievalLimits {
            maximum_bytes,
            maximum_chunks,
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
    let count = match u8::try_from(proof.siblings().len()) {
        Ok(count) => count,
        Err(error) => panic!("sibling count failed: {error}"),
    };
    bytes.push(count);
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    bytes
}

fn request(stream: &mut UnixStream) -> (u64, u8) {
    let frame = match read_frame(stream, 1024 * 1024) {
        Ok(frame) => frame,
        Err(error) => panic!("request frame failed: {error:?}"),
    };
    let envelope = match decode_envelope(&frame) {
        Ok(envelope) => envelope,
        Err(error) => panic!("request envelope failed: {error:?}"),
    };
    assert_eq!(envelope.message_tag, 18);
    let selector = envelope
        .canonical_payload
        .first()
        .copied()
        .unwrap_or_default();
    (envelope.correlation_id, selector)
}

fn response(
    stream: &mut UnixStream,
    tag: u16,
    correlation_id: u64,
    payload: &[u8],
    proof: &[u8],
) -> bool {
    let encoded = match encode_envelope(Envelope {
        version: Version::V1_0,
        message_tag: tag,
        correlation_id,
        canonical_payload: payload,
        proof_material: proof,
    }) {
        Ok(encoded) => encoded,
        Err(error) => panic!("response encoding failed: {error:?}"),
    };
    write_frame(stream, &encoded, 1024 * 1024).is_ok()
}

fn spawn_provider(
    socket: &SocketPath,
    fixture: Fixture,
    chunks_to_send: usize,
    corrupt_index: Option<usize>,
    send_end: bool,
) -> JoinHandle<u8> {
    let listener = match UnixListener::bind(&socket.0) {
        Ok(listener) => listener,
        Err(error) => panic!("provider bind failed: {error}"),
    };
    thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("provider accept failed: {error}"),
        };
        let (correlation, selector) = request(&mut stream);
        for index in 0..chunks_to_send.min(fixture.chunks.len()) {
            let mut bytes = fixture.chunks[index].bytes.clone();
            if corrupt_index == Some(index) {
                if let Some(first) = bytes.first_mut() {
                    *first ^= 1;
                }
            }
            if !response(
                &mut stream,
                19,
                correlation,
                &bytes,
                &metadata(&fixture.chunks[index], &fixture.proofs[index]),
            ) {
                break;
            }
        }
        if send_end {
            let _ = response(&mut stream, 20, correlation, &[], &[]);
        }
        selector
    })
}

#[test]
fn corrupt_provider_is_isolated_and_complete_provider_wins() {
    let fixture = fixture();
    let corrupt_socket = SocketPath::new("corrupt");
    let good_socket = SocketPath::new("good");
    let corrupt_server = spawn_provider(&corrupt_socket, fixture.clone(), 2, Some(1), false);
    let good_server = spawn_provider(&good_socket, fixture.clone(), 5, None, true);
    let corrupt_gate = ConnectionGate::new(1);
    let good_gate = ConnectionGate::new(1);
    let mut corrupt = Uds::connect(&corrupt_socket.0, &corrupt_gate, limits())
        .unwrap_or_else(|error| panic!("corrupt provider connection failed: {error:?}"));
    let mut good = Uds::connect(&good_socket.0, &good_gate, limits())
        .unwrap_or_else(|error| panic!("good provider connection failed: {error:?}"));
    let mut providers = ProviderSet::new(vec![
        Provider {
            name: "corrupt".to_owned(),
            transport: &mut corrupt,
        },
        Provider {
            name: "good".to_owned(),
            transport: &mut good,
        },
    ]);
    let mut progress = Vec::new();
    let outcome = fetch(
        &mut providers,
        AvailabilitySelector::Batch(7),
        context(&fixture, 4096, 8),
        |update| progress.push((update.provider.to_owned(), update.chunk.chunk().index)),
    )
    .unwrap_or_else(|error| panic!("provider fetch failed: {error:?}"));
    let FetchOutcome::Complete(result) = outcome else {
        panic!("good provider did not complete retrieval");
    };
    assert_eq!(result.provider, "good");
    assert_eq!(result.chunks.len(), 5);
    assert_eq!(result.report.classes.obtained.len(), 5);
    assert_eq!(result.batch_number(), 7);
    assert_eq!(result.data_availability_root(), fixture.availability_root);
    assert_eq!(result.record_roots(), fixture.record_roots);
    assert_eq!(result.records().activities, vec![b"activity".to_vec()]);
    assert_eq!(result.records().receipts, vec![b"receipt".to_vec()]);
    assert_eq!(result.records().events, vec![b"event".to_vec()]);
    assert_eq!(result.records().oracle_inputs, vec![b"oracle".to_vec()]);
    assert_eq!(progress[0], ("corrupt".to_owned(), 0));
    assert_eq!(
        progress.iter().filter(|(name, _)| name == "good").count(),
        5
    );
    assert!(corrupt_server.join().is_ok(), "corrupt provider panicked");
    assert!(good_server.join().is_ok(), "good provider panicked");
}

#[test]
fn withholding_and_partial_class_sets_are_reported_honestly() {
    let fixture = fixture();
    let withholding_socket = SocketPath::new("withhold");
    let partial_socket = SocketPath::new("partial");
    let withholding_server = spawn_provider(&withholding_socket, fixture.clone(), 2, None, false);
    let partial_server = spawn_provider(&partial_socket, fixture.clone(), 4, None, true);
    let withholding_gate = ConnectionGate::new(1);
    let partial_gate = ConnectionGate::new(1);
    let mut withholding = Uds::connect(&withholding_socket.0, &withholding_gate, limits())
        .unwrap_or_else(|error| panic!("withholding connection failed: {error:?}"));
    let mut partial = Uds::connect(&partial_socket.0, &partial_gate, limits())
        .unwrap_or_else(|error| panic!("partial connection failed: {error:?}"));
    let mut providers = ProviderSet::new(vec![
        Provider {
            name: "withholding".to_owned(),
            transport: &mut withholding,
        },
        Provider {
            name: "partial".to_owned(),
            transport: &mut partial,
        },
    ]);
    let outcome = fetch(
        &mut providers,
        AvailabilitySelector::Checkpoint([1; 32]),
        context(&fixture, 4096, 8),
        |_| {},
    )
    .unwrap_or_else(|error| panic!("partial fetch failed: {error:?}"));
    let FetchOutcome::Partial(reports) = outcome else {
        panic!("partial providers produced complete retrieval");
    };
    assert_eq!(reports.len(), 2);
    assert!(matches!(reports[0].failure, ProviderFailure::Transport(_)));
    assert!(matches!(
        reports[1].failure,
        ProviderFailure::Reassembly(ref failure)
            if failure.check == AvailabilityCheck::MissingClass
    ));
    assert_eq!(
        reports[1].classes.missing,
        vec![AvailabilityClass::Recovery]
    );
    assert!(
        withholding_server.join().is_ok(),
        "withholding provider panicked"
    );
    assert!(partial_server.join().is_ok(), "partial provider panicked");
}

#[test]
fn retrieval_exceeding_size_bound_is_partial_at_the_provider() {
    let fixture = fixture();
    let socket = SocketPath::new("bound");
    let server = spawn_provider(&socket, fixture.clone(), 1, None, false);
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(&socket.0, &gate, limits())
        .unwrap_or_else(|error| panic!("bound provider connection failed: {error:?}"));
    let mut providers = ProviderSet::new(vec![Provider {
        name: "bounded".to_owned(),
        transport: &mut transport,
    }]);
    let outcome = fetch(
        &mut providers,
        AvailabilitySelector::Activity([2; 32]),
        context(&fixture, 1, 8),
        |_| {},
    )
    .unwrap_or_else(|error| panic!("bounded fetch failed: {error:?}"));
    let FetchOutcome::Partial(reports) = outcome else {
        panic!("over-bound retrieval completed");
    };
    assert!(matches!(
        reports[0].failure,
        ProviderFailure::SizeBound { maximum: 1, .. }
    ));
    assert_eq!(reports[0].verified_chunks, 0);
    assert!(server.join().is_ok(), "bound provider panicked");
}

#[test]
fn all_selector_forms_are_encoded_distinctly_and_limits_are_mandatory() {
    let fixture = fixture();
    let selectors = [
        AvailabilitySelector::Checkpoint([1; 32]),
        AvailabilitySelector::Batch(7),
        AvailabilitySelector::SequenceRange { first: 8, last: 9 },
        AvailabilitySelector::Activity([2; 32]),
    ];
    for (index, selector) in selectors.into_iter().enumerate() {
        let socket = SocketPath::new("selector");
        let server = spawn_provider(&socket, fixture.clone(), 0, None, true);
        let gate = ConnectionGate::new(1);
        let mut transport = Uds::connect(&socket.0, &gate, limits())
            .unwrap_or_else(|error| panic!("selector connection failed: {error:?}"));
        let mut providers = ProviderSet::new(vec![Provider {
            name: "selector".to_owned(),
            transport: &mut transport,
        }]);
        let _ = fetch(&mut providers, selector, context(&fixture, 4096, 8), |_| {})
            .unwrap_or_else(|error| panic!("selector fetch failed: {error:?}"));
        let observed = server
            .join()
            .unwrap_or_else(|_| panic!("selector provider panicked"));
        assert_eq!(observed, u8::try_from(index + 1).unwrap_or_default());
    }
    let mut providers = ProviderSet::new(Vec::new());
    assert_eq!(
        fetch(
            &mut providers,
            AvailabilitySelector::Batch(7),
            context(&fixture, 4096, 8),
            |_| {},
        ),
        Err(FetchError::NoProviders)
    );
}
