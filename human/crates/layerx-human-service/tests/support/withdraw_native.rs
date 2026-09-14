use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use layerx_agentd::protocol_evidence::{RawReceiptEvidence, VerifiedReceiptEvidence};
use layerx_client::client::{ClientConfig, ReconnectPolicy};
use layerx_client::evidence::{ProofBundleSelector, VerifiedProofBundle};
use layerx_client::lni::handshake::HandshakeConfig;
use layerx_client::lni::schema::Version;
use layerx_client::lni::transport::Limits;
use layerx_client::Client;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::receipt::{AuthorizedBatch, MaintainedOutcomeEvidence};
use layerx_types::ids::Did;
use layerx_types::payload::ModuleRegistry;
use layerx_types::verify::VerificationLevel;

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("real withdrawal node: {error:?}"))
}

pub struct NativeFixture {
    child: Child,
    root: PathBuf,
    pub endpoint: PathBuf,
    pub timestamp: u64,
    pub account_sequence: u64,
}

impl NativeFixture {
    pub fn new() -> Self {
        let root = super::directory("hwd");
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/withdraw-daemon.py");
        let mut child = checked(
            Command::new(
                std::env::var("LAYERX_TEST_PYTHON").unwrap_or_else(|_| "python3".to_owned()),
            )
            .arg(script)
            .arg(&root)
            .stdin(Stdio::piped())
            .spawn(),
        );
        let deadline = Instant::now() + Duration::from_secs(90);
        while !root.join("ready.json").exists() {
            if let Some(status) = checked(child.try_wait()) {
                panic!(
                    "native fixture exited {status}; evidence {}",
                    root.display()
                );
            }
            assert!(
                Instant::now() < deadline,
                "native fixture readiness; evidence {}",
                root.display()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let ready: serde_json::Value = checked(serde_json::from_slice(&checked(std::fs::read(
            root.join("ready.json"),
        ))));
        let endpoint = PathBuf::from(
            ready["socket"]
                .as_str()
                .unwrap_or_else(|| panic!("socket absent")),
        );
        let mut node = connect(&endpoint);
        let actor = checked(Did::new(super::owner_did().as_bytes()));
        let state = checked(node.preparation_state(&actor, 1));
        assert_eq!(state.account_sequence, 0);
        let credit = checked(std::fs::read(
            ready["credit"]
                .as_str()
                .unwrap_or_else(|| panic!("credit absent")),
        ));
        let submission = checked(node.submit_signed(
            &state.module_registry,
            super::owner_public(),
            2,
            1,
            &credit,
        ));
        let layerx_client::submit::Submission::Acknowledged(ack) = submission else {
            panic!("actual custody credit was not acknowledged: {submission:?}");
        };
        let (_, evidence) = receipt(&mut node, &state.module_registry, ack.activity_id());
        assert_eq!(evidence.result_code(), 0);
        let state = checked(node.preparation_state(&actor, 3));
        Self {
            child,
            root,
            endpoint,
            timestamp: state.protocol_timestamp,
            account_sequence: state.account_sequence,
        }
    }
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.wait();
        if std::thread::panicking() {
            eprintln!("native withdrawal evidence: {}", self.root.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

pub fn connect(endpoint: &Path) -> Client {
    checked(Client::connect(ClientConfig {
        endpoint: endpoint.to_path_buf(),
        handshake: HandshakeConfig {
            built_interface_version: Version::V1_5,
            expected_protocol_version: 3,
            expected_network_id: super::NETWORK_ID,
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
    }))
}

pub fn receipt(
    node: &mut Client,
    registry: &ModuleRegistry,
    activity_id: [u8; 32],
) -> (super::ReceiptMaterial, VerifiedReceiptEvidence) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut correlation = 100;
    let bundle = loop {
        correlation += 1;
        match node.proof_bundle(
            ProofBundleSelector::Receipt(activity_id),
            correlation,
            registry,
        ) {
            Ok(bundle) => break bundle,
            Err(layerx_client::evidence::EvidenceError::Unavailable)
                if Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(20))
            }
            Err(error) => panic!("actual withdrawal receipt proof: {error:?}"),
        }
    };
    let VerifiedProofBundle::Receipt {
        canonical_bytes,
        proof,
        signed_header,
        ..
    } = bundle
    else {
        panic!("receipt proof kind")
    };
    let header = checked(layerx_intents::canonical::decode_batch_header(
        &signed_header.canonical_bytes,
    ));
    let decoded = checked(layerx_intents::canonical::decode_receipt(&canonical_bytes));
    let protocol = decoded
        .protocol()
        .unwrap_or_else(|| panic!("protocol receipt"));
    let authority = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        protocol.previous_state_root(),
        protocol.resulting_state_root(),
        signed_header.public_key,
    );
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        signed_header.public_key,
        header.batch_number(),
        header.batch_number(),
    );
    assert!(header.last_sequence() > header.first_sequence());
    assert!(header.last_sequence() - header.first_sequence() <= 64);
    let history = checked(node.history(
        header.first_sequence(),
        header.last_sequence(),
        65,
        None,
        VerificationLevel::BATCH_INCLUDED,
        correlation + 1,
        authorization,
    ));
    assert!(history.cursor.is_none());
    assert_eq!(
        history.items.len() as u64,
        header.last_sequence() - header.first_sequence() + 1
    );
    let maintenance = history
        .items
        .last()
        .unwrap_or_else(|| panic!("maintenance receipt missing"));
    let maintenance_proof = checked(layerx_client::read::HistoryProof::decode(
        maintenance.proof_material(),
    ));
    assert_eq!(maintenance_proof.header, signed_header.canonical_bytes);
    assert_eq!(maintenance_proof.header_signature, signed_header.signature);
    let receipts = history.items[..history.items.len() - 1]
        .iter()
        .map(|item| item.canonical_bytes().to_vec())
        .collect::<Vec<_>>();
    let raw = RawReceiptEvidence::new(
        canonical_bytes.clone(),
        proof,
        signed_header.canonical_bytes,
        signed_header.signature,
    );
    let signature = raw.header_signature();
    let evidence = MaintainedOutcomeEvidence {
        header: raw.canonical_header(),
        header_signature: &signature,
        activity_proof: raw.proof(),
        maintenance: maintenance.canonical_bytes(),
        maintenance_proof: &maintenance_proof.proof,
        authorization: &authorization,
    };
    let terminal = checked(VerifiedReceiptEvidence::verify_authorized_maintained(
        &raw,
        &authority,
        &evidence,
        &receipts,
        3,
        super::NETWORK_ID,
    ));
    let material = super::ReceiptMaterial {
        canonical_bytes,
        authorised_batch: authority,
        verification_level: terminal.level(),
    };
    (material, terminal)
}
