use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use layerx_client::availability::{
    fetch, fetch_sealed_candidate, AvailabilitySelector, FetchContext, FetchOutcome, Provider,
    ProviderSet, RetrievalLimits,
};
use layerx_client::batch::lookup;
use layerx_client::evidence::{register_finality_evidence, FinalityEvidenceCandidate};
use layerx_client::lni::handshake::{perform, HandshakeConfig};
use layerx_client::lni::refusal::decode_core_refusal;
use layerx_client::lni::schema::{decode_envelope, encode_envelope, Envelope, Version};
use layerx_client::lni::transport::{ConnectionGate, FrameTransport, Limits, Uds};
use layerx_proof::availability::RootCommitments;

fn refusal(transport: &mut Uds, payload: &[u8], expected: i32, correlation: u64) {
    let encoded = encode_envelope(Envelope {
        version: Version::V1_4,
        message_tag: 18,
        correlation_id: correlation,
        canonical_payload: payload,
        proof_material: &[],
    })
    .unwrap_or_else(|error| panic!("availability request encoding: {error:?}"));
    transport
        .send(&encoded)
        .unwrap_or_else(|error| panic!("availability request send: {error:?}"));
    let bytes = transport
        .receive()
        .unwrap_or_else(|error| panic!("availability refusal receive: {error:?}"));
    let response = decode_envelope(&bytes)
        .unwrap_or_else(|error| panic!("availability refusal envelope: {error:?}"));
    assert_eq!(response.message_tag, 25);
    assert_eq!(response.correlation_id, correlation);
    assert!(response.proof_material.is_empty());
    let result = decode_core_refusal(response.canonical_payload)
        .unwrap_or_else(|| panic!("malformed core refusal"));
    assert_eq!(result.class, 3);
    assert_eq!(result.result.raw(), expected);
}

fn candidate_fetch(transport: &mut Uds, signed: &layerx_client::batch::SignedBatchHeader) {
    let context = FetchContext {
        interface_version: Version::V1_4,
        correlation_id: 30,
        expected_batch_number: signed.header.batch_number(),
        data_availability_root: signed.header.data_availability_root(),
        record_roots: RootCommitments {
            activity: signed.header.activity_merkle_root(),
            receipt: signed.header.receipt_merkle_root(),
            event: signed.header.event_merkle_root(),
            oracle: signed.header.oracle_root(),
        },
        limits: RetrievalLimits {
            maximum_bytes: 16 * 1024 * 1024,
            maximum_chunks: 1024,
            deadline: Duration::from_secs(10),
        },
    };
    let mut providers = ProviderSet::new(vec![Provider {
        name: "real-layerxd-candidate".to_owned(),
        transport,
    }]);
    let outcome = fetch_sealed_candidate(&mut providers, context, |_| {})
        .unwrap_or_else(|error| panic!("candidate fetch: {error:?}"));
    match outcome {
        FetchOutcome::Complete(result) => {
            assert!(result.chunks.len() >= 5);
            assert_eq!(result.batch_number(), context.expected_batch_number);
            assert_eq!(
                result.data_availability_root(),
                context.data_availability_root
            );
            assert!(!result.records().activities.is_empty());
            assert!(!result.records().receipts.is_empty());
        }
        FetchOutcome::Partial(reports) => panic!("candidate incomplete: {reports:?}"),
    }
}

fn finalized_fetch(
    transport: &mut Uds,
    first: &layerx_client::batch::SignedBatchHeader,
    work: &Path,
) {
    let checkpoint = fs::read(work.join("availability-output/checkpoint.bin"))
        .unwrap_or_else(|error| panic!("checkpoint bytes: {error}"));
    let proof = fs::read(work.join("availability-output/finality.bin"))
        .unwrap_or_else(|error| panic!("finality bytes: {error}"));
    let candidate = FinalityEvidenceCandidate::from_exact_bytes(checkpoint, proof, 3, 77)
        .unwrap_or_else(|error| panic!("real settlement candidate: {error:?}"));
    let registered = register_finality_evidence(transport, &candidate, Version::V1_4, 10)
        .unwrap_or_else(|error| panic!("daemon finality registration: {error:?}"));
    assert_eq!(registered.batch_number, 1);
    let text = fs::read_to_string(work.join("availability-activity-id"))
        .unwrap_or_else(|error| panic!("activity identifier: {error}"));
    let text = text.trim();
    assert_eq!(text.len(), 64);
    let mut activity = [0_u8; 32];
    for (index, byte) in activity.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
            .unwrap_or_else(|error| panic!("activity hex: {error}"));
    }
    for selector in [
        AvailabilitySelector::Batch(1),
        AvailabilitySelector::Activity(activity),
        AvailabilitySelector::SequenceRange {
            first: first.header.first_sequence(),
            last: first.header.first_sequence(),
        },
        AvailabilitySelector::Checkpoint(registered.checkpoint_id),
    ] {
        let context = FetchContext {
            interface_version: Version::V1_4,
            correlation_id: 20,
            expected_batch_number: 1,
            data_availability_root: first.header.data_availability_root(),
            record_roots: RootCommitments {
                activity: first.header.activity_merkle_root(),
                receipt: first.header.receipt_merkle_root(),
                event: first.header.event_merkle_root(),
                oracle: first.header.oracle_root(),
            },
            limits: RetrievalLimits {
                maximum_bytes: 16 * 1024 * 1024,
                maximum_chunks: 1024,
                deadline: Duration::from_secs(10),
            },
        };
        let mut chunks = 0;
        let mut providers = ProviderSet::new(vec![Provider {
            name: "real-layerxd".to_owned(),
            transport,
        }]);
        let outcome = fetch(&mut providers, selector, context, |progress| {
            assert_eq!(
                progress.chunk.data_availability_root(),
                context.data_availability_root
            );
            assert_eq!(progress.chunk.chunk().batch_number, 1);
            chunks += 1;
        })
        .unwrap_or_else(|error| panic!("native availability fetch: {error:?}"));
        match outcome {
            FetchOutcome::Complete(result) => {
                assert_eq!(result.chunks.len(), chunks);
                assert!(chunks >= 5);
            }
            FetchOutcome::Partial(reports) => {
                panic!("native availability incomplete: {reports:?}")
            }
        }
    }
}

fn probe(socket: &Path, stage: &str) {
    let corrupt = stage == "corrupt";
    let gate = ConnectionGate::new(1);
    let mut transport = Uds::connect(
        socket,
        &gate,
        Limits {
            maximum_frame_bytes: 1_212_416,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 1_212_416,
            deadline: Duration::from_secs(10),
        },
    )
    .unwrap_or_else(|error| panic!("real daemon connection: {error:?}"));
    let handshake = perform(
        &mut transport,
        &HandshakeConfig {
            built_interface_version: Version::V1_4,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        None,
    )
    .unwrap_or_else(|error| panic!("real daemon handshake: {error:?}"));
    let node = handshake.node();
    assert_eq!(node.latest_sealed_batch, 9);
    assert_eq!(
        node.advertised_capabilities
            .iter()
            .any(|capability| capability == "availability_fetch"),
        !corrupt,
    );
    let first = lookup(
        &mut transport,
        Version::V1_4,
        1,
        1,
        node.authorised_sequencer_key,
    )
    .unwrap_or_else(|error| panic!("first signed header: {error:?}"));
    let last = lookup(
        &mut transport,
        Version::V1_4,
        9,
        2,
        node.authorised_sequencer_key,
    )
    .unwrap_or_else(|error| panic!("last signed header: {error:?}"));
    assert_ne!(first.header.data_availability_root(), [0; 32]);
    let work = PathBuf::from(
        std::env::var_os("LAYERX_TEST_AVAILABILITY_WORK")
            .unwrap_or_else(|| panic!("availability work directory missing")),
    );
    if stage == "retained" {
        assert_eq!(node.latest_finalised_checkpoint, [0; 32]);
        assert!(!work.join("availability-output/checkpoint.bin").exists());
        assert!(!work
            .join("availability-output/available-header.bin")
            .exists());
        candidate_fetch(&mut transport, &first);
        assert!(!work.join("availability-output/checkpoint.bin").exists());
        let pending = work.join("availability-output/header.pending");
        fs::write(&pending, first.canonical_bytes())
            .unwrap_or_else(|error| panic!("write signed header: {error}"));
        fs::rename(
            &pending,
            work.join("availability-output/available-header.bin"),
        )
        .unwrap_or_else(|error| panic!("publish signed header: {error}"));
    }
    if stage == "finalized" {
        finalized_fetch(&mut transport, &first, &work);
        candidate_fetch(&mut transport, &last);
        let mut unfinalized = vec![2];
        unfinalized.extend_from_slice(&9_u64.to_be_bytes());
        refusal(&mut transport, &unfinalized, -804, 31);
        return;
    }
    let mut batch = vec![2];
    batch.extend_from_slice(&1_u64.to_be_bytes());
    refusal(&mut transport, &batch, -804, 3);
    if corrupt {
        batch[0] = 5;
        refusal(&mut transport, &batch, -804, 32);
    }
    if !corrupt {
        for number in [0, 10, u64::MAX] {
            let mut candidate = vec![5];
            candidate.extend_from_slice(&number.to_be_bytes());
            refusal(&mut transport, &candidate, -106, 33);
        }
        let mut unknown = vec![2];
        unknown.extend_from_slice(&u64::MAX.to_be_bytes());
        refusal(&mut transport, &unknown, -106, 4);
        let mut range = vec![3];
        range.extend_from_slice(&first.header.first_sequence().to_be_bytes());
        range.extend_from_slice(&last.header.last_sequence().to_be_bytes());
        refusal(&mut transport, &range, -5, 5);
    }
}

#[test]
fn real_daemon_availability_refusals() {
    if let Some(socket) = std::env::var_os("LAYERX_TEST_AVAILABILITY_SOCKET") {
        let stage = std::env::var("LAYERX_TEST_AVAILABILITY_STAGE")
            .unwrap_or_else(|error| panic!("availability stage: {error}"));
        assert!(stage == "retained" || stage == "finalized" || stage == "corrupt");
        probe(Path::new(&socket), &stage);
        return;
    }
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let repository = repository
        .canonicalize()
        .unwrap_or_else(|error| panic!("repository path: {error}"));
    let binaries = std::env::var_os("LAYERX_TEST_NATIVE_BIN_DIR")
        .map_or_else(|| repository.join("build/bin"), PathBuf::from);
    assert!(binaries.join("layerxd").is_file(), "layerxd must be built");
    assert!(binaries.join("layerx-genesis-build").is_file());
    assert!(repository
        .join("build/tests/lxp_test_program_admission")
        .is_file());
    assert!(repository
        .join("build/tests/lxp_test_daemon_finality_authority")
        .is_file());
    let executable = std::env::current_exe()
        .unwrap_or_else(|error| panic!("Rust integration executable: {error}"));
    let status = Command::new("bash")
        .arg(repository.join("tests/daemon/program-admission.sh"))
        .arg("build")
        .arg("--availability-batches")
        .arg(executable)
        .env("LAYERX_TEST_NATIVE_BIN_DIR", binaries)
        .env("CARGO_BUILD_JOBS", "16")
        .env("MAKEFLAGS", "-j16")
        .current_dir(&repository)
        .status()
        .unwrap_or_else(|error| panic!("real daemon harness: {error}"));
    assert!(status.success(), "real daemon harness exit: {status}");
}
