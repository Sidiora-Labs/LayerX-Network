use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use ed25519_dalek::SigningKey;
use layerx_agentd::read::{NativeReadError, NativeReadRoute};
use layerx_client::availability::{
    AvailabilitySelector, FetchContext, FetchOutcome, RetrievalLimits,
};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::evidence::FinalityEvidenceCandidate;
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_client::Client;
use layerx_programs::hex;
use layerx_proof::availability::RootCommitments;
use layerx_types::ids::Did;

fn connect(socket: &Path) -> Client {
    Client::connect(ClientConfig {
        endpoint: socket.to_path_buf(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: 77,
        },
        limits: Limits {
            maximum_frame_bytes: 1_212_416,
            maximum_connections: 1,
            maximum_streams: 1,
            maximum_queued_bytes: 1_212_416,
            deadline: Duration::from_secs(10),
        },
        reconnect: ReconnectPolicy {
            maximum_attempts: 1,
            base_delay: Duration::ZERO,
            maximum_delay: Duration::ZERO,
            jitter_percent: 0,
        },
    })
    .unwrap_or_else(|error| panic!("real daemon connection: {error:?}"))
}

fn prepare_stage(client: &mut Client, stage: &str, work: &Path) {
    if stage == "retained" {
        let signed = client
            .batch_header(1, 1)
            .unwrap_or_else(|error| panic!("signed header: {error:?}"));
        let header = &signed.header;
        let outcome = client
            .fetch_availability(
                AvailabilitySelector::SealedCandidate(1),
                FetchContext {
                    interface_version: Version::V1_5,
                    correlation_id: 2,
                    expected_batch_number: 1,
                    data_availability_root: header.data_availability_root(),
                    record_roots: RootCommitments {
                        activity: header.activity_merkle_root(),
                        receipt: header.receipt_merkle_root(),
                        event: header.event_merkle_root(),
                        oracle: header.oracle_root(),
                    },
                    limits: RetrievalLimits {
                        maximum_bytes: 96 * 1024,
                        maximum_chunks: 256,
                        deadline: Duration::from_secs(10),
                    },
                },
                |_| {},
            )
            .unwrap_or_else(|error| panic!("candidate availability: {error:?}"));
        assert!(
            matches!(outcome, FetchOutcome::Complete(_)),
            "candidate incomplete: {outcome:?}"
        );
        assert!(!work.join("availability-output/checkpoint.bin").exists());
        let pending = work.join("availability-output/header.pending");
        fs::write(&pending, signed.canonical_bytes())
            .unwrap_or_else(|error| panic!("signed header: {error}"));
        fs::rename(
            pending,
            work.join("availability-output/available-header.bin"),
        )
        .unwrap_or_else(|error| panic!("header publication: {error}"));
    } else if stage == "finalized" {
        let checkpoint = fs::read(work.join("availability-output/checkpoint.bin"))
            .unwrap_or_else(|error| panic!("checkpoint: {error}"));
        let finality = fs::read(work.join("availability-output/finality.bin"))
            .unwrap_or_else(|error| panic!("finality: {error}"));
        let candidate = FinalityEvidenceCandidate::from_exact_bytes(checkpoint, finality, 3, 77)
            .unwrap_or_else(|error| panic!("settlement verification: {error:?}"));
        assert_eq!(
            client
                .register_finality_evidence(&candidate, 3)
                .unwrap_or_else(|error| panic!("finality registration: {error:?}"))
                .batch_number,
            1
        );
    }
}

fn probe(socket: &Path, stage: &str, work: &Path) {
    let mut client = connect(socket);
    prepare_stage(&mut client, stage, work);
    let key = SigningKey::from_bytes(&[0x11; 32]);
    let did =
        Did::new(format!("did:layerx:{}", hex::encode(key.verifying_key().as_bytes())).as_bytes())
            .unwrap_or_else(|error| panic!("actor DID: {error:?}"));
    let mut route = NativeReadRoute::new(
        client,
        did,
        "test-native-read-cursor-key-32-bytes".to_owned(),
    )
    .unwrap_or_else(|error| panic!("native route: {error:?}"));
    if stage != "finalized" {
        assert!(route.read("/v1/reads/checkpoint/1").is_err());
        let availability = route.read("/v1/reads/availability/1");
        assert!(availability
            .as_ref()
            .map_or(true, |value| value["complete"] != true));
        return;
    }
    finalized_reads(&mut route, work);
}

fn finalized_reads(route: &mut NativeReadRoute, work: &Path) {
    let activity = fs::read_to_string(work.join("availability-activity-id"))
        .unwrap_or_else(|error| panic!("activity: {error}"));
    let activity = activity.trim();
    let receipt = route
        .read(&format!("/v1/reads/receipt/{activity}"))
        .unwrap_or_else(|error| panic!("receipt read: {error:?}"));
    assert_eq!(receipt["activity_id"], activity);
    assert_eq!(receipt["verification_level"], 2);
    let proof = route
        .read(&format!("/v1/reads/proof/{activity}"))
        .unwrap_or_else(|error| panic!("activity proof: {error:?}"));
    assert_eq!(proof["activity_id"], activity);
    assert_eq!(proof["verification_level"], 2);
    let checkpoint = route
        .read("/v1/reads/checkpoint/1")
        .unwrap_or_else(|error| panic!("checkpoint read: {error:?}"));
    assert_eq!(checkpoint["availability_obtained"], true);
    assert!(checkpoint["verification_level"]
        .as_u64()
        .is_some_and(|level| level >= 4));
    let availability = route
        .read("/v1/reads/availability/1")
        .unwrap_or_else(|error| panic!("availability read: {error:?}"));
    assert_eq!(availability["complete"], true);
    assert!(availability["chunks"]
        .as_array()
        .is_some_and(|chunks| chunks.len() >= 5));
    assert_eq!(availability["header_hex"], checkpoint["header_hex"]);
    let key = SigningKey::from_bytes(&[0x11; 32]);
    let account_name = layerx_types::account::AccountId::parse(&format!(
        "agent:did:layerx:{}:main",
        hex::encode(key.verifying_key().as_bytes())
    ))
    .unwrap_or_else(|error| panic!("actual actor account: {error:?}"));
    let account = hex::encode(
        &layerx_wire::hash::account_id_for_protocol(&account_name, 3)
            .unwrap_or_else(|error| panic!("actor account identifier: {error:?}")),
    );
    history_reads(route, &account);
}

fn history_reads(route: &mut NativeReadRoute, account: &str) {
    let mut page = route
        .read(&format!("/v1/reads/history/{account}?limit=2"))
        .unwrap_or_else(|error| panic!("history first page: {error:?}"));
    let cursor = page["cursor"]
        .as_str()
        .unwrap_or_else(|| panic!("history cursor"))
        .to_owned();
    assert!(matches!(
        route.read(&format!(
            "/v1/reads/history/{}?limit=2&cursor={cursor}",
            hex::encode(&[0x99; 32])
        )),
        Err(NativeReadError::CursorMismatch)
    ));
    let mut tampered =
        hex::decode_digest(&cursor).unwrap_or_else(|error| panic!("cursor hex: {error:?}"));
    tampered[0] ^= 1;
    assert!(matches!(
        route.read(&format!(
            "/v1/reads/history/{account}?limit=2&cursor={}",
            hex::encode(&tampered)
        )),
        Err(NativeReadError::CursorMismatch)
    ));
    let mut scanned = 0_u64;
    let mut previous = 0_u64;
    let mut count = 0;
    for _ in 0..32 {
        scanned += page["scanned_items"]
            .as_u64()
            .unwrap_or_else(|| panic!("scanned count"));
        for item in page["items"]
            .as_array()
            .unwrap_or_else(|| panic!("history items"))
        {
            let sequence: u64 = item["global_sequence"]
                .as_str()
                .unwrap_or_else(|| panic!("item sequence"))
                .parse()
                .unwrap_or_else(|error| panic!("sequence: {error}"));
            assert!(sequence > previous);
            previous = sequence;
            count += 1;
        }
        let Some(cursor) = page["cursor"].as_str() else {
            break;
        };
        page = route
            .read(&format!(
                "/v1/reads/history/{account}?limit=2&cursor={cursor}"
            ))
            .unwrap_or_else(|error| panic!("history continuation: {error:?}"));
    }
    assert_eq!(page["complete"], true);
    assert_eq!(
        scanned,
        page["freshness"]["snapshot_end_sequence"]
            .as_u64()
            .unwrap_or_else(|| panic!("snapshot end"))
    );
    assert!(
        count >= 9,
        "all nine actual activities must be discoverable"
    );
    assert!(route
        .read(&format!("/v1/reads/receipt/{}", hex::encode(&[0xff; 32])))
        .is_err());
    assert!(route.read("/v1/reads/history/bad?limit=0").is_err());
}

#[test]
fn real_daemon_availability_refusals() {
    if let Some(socket) = std::env::var_os("LAYERX_TEST_AVAILABILITY_SOCKET") {
        let stage = std::env::var("LAYERX_TEST_AVAILABILITY_STAGE")
            .unwrap_or_else(|error| panic!("stage: {error}"));
        assert!(matches!(
            stage.as_str(),
            "retained" | "finalized" | "corrupt"
        ));
        let work = PathBuf::from(
            std::env::var_os("LAYERX_TEST_AVAILABILITY_WORK")
                .unwrap_or_else(|| panic!("work directory")),
        );
        probe(Path::new(&socket), &stage, &work);
        return;
    }
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let binaries = std::env::var_os("LAYERX_TEST_NATIVE_BIN_DIR")
        .map_or_else(|| repository.join("build/bin"), PathBuf::from);
    assert!(binaries.join("layerxd").is_file());
    assert!(binaries.join("layerx-genesis-build").is_file());
    let status = Command::new("bash")
        .arg(repository.join("tests/daemon/program-admission.sh"))
        .args(["build", "--availability-batches"])
        .arg(std::env::current_exe().unwrap_or_else(|error| panic!("test executable: {error}")))
        .env("LAYERX_TEST_NATIVE_BIN_DIR", binaries)
        .env("CARGO_BUILD_JOBS", "6")
        .env("MAKEFLAGS", "-j6")
        .current_dir(&repository)
        .status()
        .unwrap_or_else(|error| panic!("daemon harness: {error}"));
    assert!(status.success(), "real native reads harness: {status}");
}
