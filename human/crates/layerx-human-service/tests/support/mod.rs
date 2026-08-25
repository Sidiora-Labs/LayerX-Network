use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_agentd::protocol_evidence::{RawReceiptEvidence, RawStateEvidence};
use layerx_human_service::store::{
    AgentTenantId, PrincipalId, PrincipalStore, RetentionPeriod, RetentionPolicy, RowKey,
    TenancyDigest, TenancyMap,
};
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::build_proof;
use layerx_proof::receipt::AuthorizedBatch;
use sha2::{Digest as _, Sha256};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

pub fn directory(label: &str) -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "layerx-human-store-{label}-{}-{sequence}",
        std::process::id()
    ))
}

pub fn principal(name: &str) -> PrincipalId {
    PrincipalId::new(name).unwrap_or_else(|error| panic!("principal: {error}"))
}

pub fn row_key(name: &str) -> RowKey {
    RowKey::new(name).unwrap_or_else(|error| panic!("row key: {error}"))
}

pub fn tenancy(pairs: &[(&str, &str)]) -> TenancyMap {
    let entries = pairs.iter().map(|(name, tenant)| {
        (
            principal(name),
            AgentTenantId::new(*tenant).unwrap_or_else(|error| panic!("tenant: {error}")),
        )
    });
    TenancyMap::new(entries).unwrap_or_else(|error| panic!("tenancy map: {error}"))
}

pub fn retention_uniform(units: u64) -> RetentionPolicy {
    RetentionPolicy {
        journeys: RetentionPeriod::new(units),
        notifications: RetentionPeriod::new(units),
        audit: RetentionPeriod::new(units),
        telemetry: RetentionPeriod::new(units),
        cache: RetentionPeriod::new(units),
    }
}

pub fn install_and_open(
    root: &Path,
    map: &TenancyMap,
    retention: RetentionPolicy,
) -> (PrincipalStore, TenancyDigest) {
    let digest = map
        .install(root)
        .unwrap_or_else(|error| panic!("install tenancy: {error}"));
    let store = PrincipalStore::open(root, retention, digest)
        .unwrap_or_else(|error| panic!("open store: {error}"));
    (store, digest)
}

pub fn raw_receipt_evidence(
    canonical_receipt: Vec<u8>,
    authorised_batch: AuthorizedBatch,
    global_sequence: u64,
    signer: &SigningKey,
) -> RawReceiptEvidence {
    assert_eq!(
        authorised_batch.sequencer_public_key(),
        signer.verifying_key().to_bytes()
    );
    let leaves = [canonical_receipt.as_slice()];
    let (proof, receipt_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("receipt proof: {error:?}"));
    let sequencer_id = signer.verifying_key().to_bytes();
    let header = canonical_header(
        authorised_batch.previous_state_root(),
        authorised_batch.resulting_state_root(),
        [0x32; 32],
        receipt_root,
        global_sequence,
        sequencer_id,
    );
    let digest = batch_header_digest(&header);
    RawReceiptEvidence::new(
        canonical_receipt,
        proof,
        header,
        signer.sign(&digest).to_bytes(),
        SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7),
    )
}

pub fn raw_budget_state(
    consumed: u128,
    remaining: u128,
    window_start: u64,
    window_end: u64,
    observed_head: u64,
) -> RawStateEvidence {
    let mut state = Vec::with_capacity(52);
    state.extend_from_slice(b"LXBS");
    state.extend_from_slice(&consumed.to_be_bytes());
    state.extend_from_slice(&remaining.to_be_bytes());
    state.extend_from_slice(&window_start.to_be_bytes());
    state.extend_from_slice(&window_end.to_be_bytes());
    let leaves = [state.as_slice()];
    let (proof, state_root) = build_proof(&leaves, 0)
        .unwrap_or_else(|error| panic!("state proof: {error:?}"));
    let signer = SigningKey::from_bytes(&[0x4a; 32]);
    let sequencer_id = signer.verifying_key().to_bytes();
    let header = canonical_header(
        [0x31; 32],
        state_root,
        [0x32; 32],
        [0x33; 32],
        observed_head,
        sequencer_id,
    );
    let digest = batch_header_digest(&header);
    RawStateEvidence::new(
        state,
        proof,
        state_root,
        header,
        signer.sign(&digest).to_bytes(),
        SequencerAuthorization::new(sequencer_id, sequencer_id, 7, 7),
    )
}

fn canonical_header(
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    receipt_root: [u8; 32],
    last_sequence: u64,
    sequencer_id: [u8; 32],
) -> Vec<u8> {
    let mut encoded = Vec::with_capacity(354);
    encoded.extend_from_slice(&1_u16.to_be_bytes());
    encoded.extend_from_slice(&0x1701_u16.to_be_bytes());
    encoded.push(15);
    let fields: [(u8, Vec<u8>); 15] = [
        (1, 1_u16.to_be_bytes().to_vec()),
        (2, 42_u32.to_be_bytes().to_vec()),
        (3, 2_u64.to_be_bytes().to_vec()),
        (4, 7_u64.to_be_bytes().to_vec()),
        (5, 1_u64.to_be_bytes().to_vec()),
        (6, last_sequence.to_be_bytes().to_vec()),
        (7, previous_state_root.to_vec()),
        (8, resulting_state_root.to_vec()),
        (9, activity_root.to_vec()),
        (10, receipt_root.to_vec()),
        (11, [0x34; 32].to_vec()),
        (12, [0x35; 32].to_vec()),
        (13, [0x36; 32].to_vec()),
        (14, 1_000_u64.to_be_bytes().to_vec()),
        (15, sequencer_id.to_vec()),
    ];
    for (field, value) in fields {
        encoded.push(field);
        match field {
            1..=6 | 14 => encoded.extend_from_slice(&value),
            _ => {
                encoded.extend_from_slice(&32_u32.to_be_bytes());
                encoded.extend_from_slice(&value);
            }
        }
    }
    encoded
}

fn batch_header_digest(header: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"LXP/v1/batch-header\0");
    digest.update(header);
    digest.finalize().into()
}

#[allow(dead_code)]
pub fn version_file_bytes(version: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(b"LXHV");
    bytes.extend_from_slice(&version.to_be_bytes());
    bytes
}
