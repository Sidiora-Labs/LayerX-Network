use std::time::{Duration, Instant};

use layerx_client::availability::{
    AvailabilitySelector, FetchContext, FetchOutcome, RetrievalLimits,
};
use layerx_client::evidence::{CheckpointSelector, ProofBundleSelector, VerifiedProofBundle};
use layerx_client::read::{HistoryKind, HistoryPage};
use layerx_client::Client;
use layerx_programs::hex;
use layerx_proof::availability::RootCommitments;
use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::merkle::Proof;
use layerx_types::account::AccountId;
use layerx_types::activity::Envelope;
use layerx_types::ids::Did;
use layerx_types::payload::ModuleRegistry;
use layerx_types::verify::VerificationLevel;
use layerx_wire::activity::decode_signed;
use layerx_wire::hash;
use layerx_wire::receipt::decode_batch_header;
use layerx_wire::receipt::ProtocolReceipt;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

const MAX_RESULT_BYTES: usize = 240 * 1024;
const MAX_AVAILABILITY_BYTES: usize = 96 * 1024;

#[derive(Debug)]
pub enum NativeReadError {
    InvalidRequest,
    CursorMismatch,
    ResultTooLarge,
    Unavailable,
    Verification,
}

pub struct NativeReadRoute {
    client: Client,
    actor: Did,
    cursor_key: Zeroizing<String>,
    correlation: u64,
    deadline: Instant,
}

impl NativeReadRoute {
    /// # Errors
    /// Rejects an empty actor or a cursor authentication key below the bearer bound.
    pub fn new(client: Client, actor: Did, cursor_key: String) -> Result<Self, NativeReadError> {
        if actor.as_bytes().is_empty() || cursor_key.len() < 32 {
            return Err(NativeReadError::InvalidRequest);
        }
        Ok(Self {
            client,
            actor,
            cursor_key: Zeroizing::new(cursor_key),
            correlation: 10_000,
            deadline: Instant::now(),
        })
    }

    fn next_id(&mut self) -> Result<u64, NativeReadError> {
        if Instant::now() >= self.deadline {
            return Err(NativeReadError::Unavailable);
        }
        self.correlation = self
            .correlation
            .checked_add(1)
            .ok_or(NativeReadError::Unavailable)?;
        Ok(self.correlation)
    }

    /// # Errors
    /// Refuses malformed selectors, unavailable node evidence and all verification failures.
    pub fn read(&mut self, path: &str) -> Result<Value, NativeReadError> {
        let path = path
            .strip_prefix("/v1/reads/")
            .ok_or(NativeReadError::InvalidRequest)?;
        let (kind, selector) = path
            .split_once('/')
            .ok_or(NativeReadError::InvalidRequest)?;
        self.deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or(NativeReadError::Unavailable)?;
        self.client
            .reconnect()
            .map_err(|_| NativeReadError::Unavailable)?;
        let correlation = self.next_id()?;
        let registry = self
            .client
            .preparation_state(&self.actor, correlation)
            .map_err(|_| NativeReadError::Unavailable)?
            .module_registry;
        let value = match kind {
            "receipt" => self.proof(digest(selector)?, true, &registry)?,
            "proof" => self.proof(digest(selector)?, false, &registry)?,
            "checkpoint" => self.checkpoint(number(selector)?)?,
            "availability" => self.availability(number(selector)?)?,
            "history" => self.history(selector, &registry)?,
            _ => return Err(NativeReadError::InvalidRequest),
        };
        if serde_json::to_vec(&value)
            .map_err(|_| NativeReadError::Verification)?
            .len()
            > MAX_RESULT_BYTES
        {
            return Err(NativeReadError::ResultTooLarge);
        }
        Ok(value)
    }

    fn proof(
        &mut self,
        activity: [u8; 32],
        receipt: bool,
        registry: &ModuleRegistry,
    ) -> Result<Value, NativeReadError> {
        let selector = if receipt {
            ProofBundleSelector::Receipt(activity)
        } else {
            ProofBundleSelector::Activity(activity)
        };
        let correlation = self.next_id()?;
        let verified = self
            .client
            .proof_bundle(selector, correlation, registry)
            .map_err(|_| NativeReadError::Verification)?;
        proof_json(&verified, self.client.head().chain_sequence)
    }

    fn checkpoint(&mut self, batch: u64) -> Result<Value, NativeReadError> {
        let correlation = self.next_id()?;
        let checkpoint = self
            .client
            .checkpoint_evidence(CheckpointSelector::Batch(batch), correlation)
            .map_err(|_| NativeReadError::Verification)?;
        let availability = self.availability(batch)?;
        if availability["complete"].as_bool() != Some(true) {
            return Err(NativeReadError::Unavailable);
        }
        let header = decode_batch_header(checkpoint.canonical_header())
            .map_err(|_| NativeReadError::Verification)?;
        if availability["header_hex"].as_str()
            != Some(hex::encode(checkpoint.canonical_header()).as_str())
        {
            return Err(NativeReadError::Verification);
        }
        Ok(json!({
            "batch_number": batch.to_string(),
            "checkpoint_hex": hex::encode(checkpoint.checkpoint_bytes()),
            "context_hex": hex::encode(checkpoint.context_bytes()),
            "header_hex": hex::encode(checkpoint.canonical_header()),
            "verification_level": checkpoint.report().level().wire_rank(),
            "availability_obtained": true,
            "complete": true,
            "freshness": {"observed_sequence": self.client.head().chain_sequence,
                "batch_number": batch, "observed_at": header.timestamp_ms()}
        }))
    }

    fn availability(&mut self, batch: u64) -> Result<Value, NativeReadError> {
        let correlation = self.next_id()?;
        let signed = self
            .client
            .batch_header(batch, correlation)
            .map_err(|_| NativeReadError::Verification)?;
        let header = &signed.header;
        let correlation = self.next_id()?;
        let context = FetchContext {
            interface_version: self.client.handshake().node().interface_version,
            correlation_id: correlation,
            expected_batch_number: batch,
            data_availability_root: header.data_availability_root(),
            record_roots: RootCommitments {
                activity: header.activity_merkle_root(),
                receipt: header.receipt_merkle_root(),
                event: header.event_merkle_root(),
                oracle: header.oracle_root(),
            },
            limits: RetrievalLimits {
                maximum_bytes: MAX_AVAILABILITY_BYTES,
                maximum_chunks: 256,
                deadline: Duration::from_secs(10),
            },
        };
        let mut chunks = Vec::new();
        let outcome = self
            .client
            .fetch_availability(AvailabilitySelector::Batch(batch), context, |progress| {
                let chunk = progress.chunk.chunk();
                chunks.push(json!({"provider": progress.provider, "index": chunk.index,
                "class": chunk.class as u8, "offset": chunk.class_offset.to_string(),
                "bytes_hex": hex::encode(&chunk.bytes), "digest": hex::encode(&chunk.claimed_hash),
                "verified": true}));
            })
            .map_err(|_| NativeReadError::Unavailable)?;
        let (complete, failures) = match outcome {
            FetchOutcome::Complete(_) => (true, Vec::new()),
            FetchOutcome::Partial(reports) => (false, reports.into_iter().map(|report| json!({
                "provider": report.provider, "verified_chunks": report.verified_chunks,
                "verified_bytes": report.verified_bytes, "failure": format!("{:?}", report.failure),
                "missing_classes": report.classes.missing.iter().map(|class| *class as u8).collect::<Vec<_>>()
            })).collect()),
        };
        Ok(
            json!({"batch_number": batch.to_string(), "header_hex": hex::encode(signed.canonical_bytes()),
            "header_signature": hex::encode(&signed.signature), "sequencer_public_key": hex::encode(&signed.sequencer_public_key),
            "availability_root": hex::encode(&header.data_availability_root()), "complete": complete,
            "chunks": chunks, "provider_failures": failures,
            "verification_level": if complete { VerificationLevel::BATCH_INCLUDED.wire_rank() } else { VerificationLevel::UNVERIFIED.wire_rank() },
            "freshness": {"observed_sequence": self.client.head().chain_sequence, "batch_number": batch,
                "observed_at": header.timestamp_ms()}}),
        )
    }

    fn history(
        &mut self,
        selector: &str,
        registry: &ModuleRegistry,
    ) -> Result<Value, NativeReadError> {
        let (account, query) = selector
            .split_once('?')
            .ok_or(NativeReadError::InvalidRequest)?;
        let account = digest(account)?;
        let (limit, cursor) = history_query(query)?;
        let head = self.client.head();
        let (start, end) = match cursor {
            Some(cursor) => self.decode_cursor(account, cursor)?,
            None => (1, head.chain_sequence),
        };
        if end > head.chain_sequence || start == 0 || start > end.saturating_add(1) {
            return Err(NativeReadError::CursorMismatch);
        }
        if start > end {
            return Ok(
                json!({"account": hex::encode(&account), "items": [], "complete": true,
                "cursor": null, "scanned_items": 0, "verification_level": VerificationLevel::BATCH_INCLUDED.wire_rank(),
                "freshness": {"observed_sequence": head.chain_sequence}}),
            );
        }
        let correlation = self.next_id()?;
        let signed = self
            .client
            .batch_header(head.sealed_batch, correlation)
            .map_err(|_| NativeReadError::Verification)?;
        let authorization = SequencerAuthorization::new(
            signed.sequencer_id,
            signed.sequencer_public_key,
            0,
            head.sealed_batch,
        );
        let correlation = self.next_id()?;
        let page = self
            .client
            .history(
                start,
                end,
                limit,
                None,
                VerificationLevel::BATCH_INCLUDED,
                correlation,
                authorization,
            )
            .map_err(|_| NativeReadError::Verification)?;
        self.history_json(account, end, page, registry)
    }

    fn history_json(
        &mut self,
        account: [u8; 32],
        end: u64,
        page: HistoryPage,
        registry: &ModuleRegistry,
    ) -> Result<Value, NativeReadError> {
        let mut items = Vec::new();
        let mut size = 0_usize;
        let mut next = page
            .cursor
            .map(layerx_client::read::HistoryCursor::next_sequence);
        let mut scanned = 0_usize;
        for item in page.items {
            let selected = match item.kind {
                HistoryKind::Activity => {
                    let activity = decode_signed(item.canonical_bytes(), registry)
                        .map_err(|_| NativeReadError::Verification)?;
                    let id =
                        hash::activity_id(&activity).map_err(|_| NativeReadError::Verification)?;
                    let correlation = self.next_id()?;
                    let receipt = self
                        .client
                        .proof_bundle(ProofBundleSelector::Receipt(id), correlation, registry)
                        .map_err(|_| NativeReadError::Verification)?;
                    let decoded = layerx_wire::receipt::decode(receipt.canonical_bytes())
                        .map_err(|_| NativeReadError::Verification)?;
                    let protocol = decoded.protocol().ok_or(NativeReadError::Verification)?;
                    if protocol.global_sequence() != item.global_sequence {
                        return Err(NativeReadError::Verification);
                    }
                    let receipt_value = proof_json(&receipt, self.client.head().chain_sequence)?;
                    receipt_mentions_account(protocol, &activity, account)?.then(|| json!({
                        "global_sequence": item.global_sequence.to_string(), "kind": "activity",
                        "activity_id": hex::encode(&id), "canonical_hex": hex::encode(item.canonical_bytes()),
                        "receipt": receipt_value,
                        "verification_level": item.achieved().wire_rank()}))
                }
                HistoryKind::Receipt => {
                    maintenance_json(item.canonical_bytes(), account, item.global_sequence)?
                }
                HistoryKind::Event => return Err(NativeReadError::Verification),
            };
            if let Some(value) = selected {
                let bytes = serde_json::to_vec(&value)
                    .map_err(|_| NativeReadError::Verification)?
                    .len();
                if size
                    .checked_add(bytes)
                    .ok_or(NativeReadError::ResultTooLarge)?
                    > MAX_RESULT_BYTES - 2048
                {
                    if items.is_empty() {
                        return Err(NativeReadError::ResultTooLarge);
                    }
                    next = Some(item.global_sequence);
                    break;
                }
                size += bytes;
                items.push(value);
            }
            scanned += 1;
        }
        let cursor = next.map(|next| hex::encode(&self.encode_cursor(account, next, end)));
        Ok(
            json!({"account": hex::encode(&account), "items": items, "complete": cursor.is_none(),
            "cursor": cursor, "scanned_items": scanned, "verification_level": VerificationLevel::BATCH_INCLUDED.wire_rank(),
            "freshness": {"observed_sequence": self.client.head().chain_sequence, "snapshot_end_sequence": end}}),
        )
    }

    fn encode_cursor(&self, account: [u8; 32], next: u64, end: u64) -> [u8; 32] {
        let mut cursor = [0_u8; 32];
        cursor[..8].copy_from_slice(&next.to_be_bytes());
        cursor[8..16].copy_from_slice(&end.to_be_bytes());
        let mut digest = Sha256::new();
        digest.update(b"LX:MCP:HISTORY:CURSOR:v1");
        digest.update(self.cursor_key.as_bytes());
        digest.update(account);
        digest.update(&cursor[..16]);
        cursor[16..].copy_from_slice(&digest.finalize()[..16]);
        cursor
    }

    fn decode_cursor(&self, account: [u8; 32], text: &str) -> Result<(u64, u64), NativeReadError> {
        let cursor = digest(text)?;
        let next = u64::from_be_bytes(
            cursor[..8]
                .try_into()
                .map_err(|_| NativeReadError::CursorMismatch)?,
        );
        let end = u64::from_be_bytes(
            cursor[8..16]
                .try_into()
                .map_err(|_| NativeReadError::CursorMismatch)?,
        );
        let expected = self.encode_cursor(account, next, end);
        if !layerx_crypto::ct::eq(&cursor, &expected) {
            return Err(NativeReadError::CursorMismatch);
        }
        Ok((next, end))
    }
}

fn digest(value: &str) -> Result<[u8; 32], NativeReadError> {
    hex::decode_digest(value).map_err(|_| NativeReadError::InvalidRequest)
}

fn number(value: &str) -> Result<u64, NativeReadError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NativeReadError::InvalidRequest);
    }
    value
        .parse()
        .ok()
        .filter(|value| *value != 0)
        .ok_or(NativeReadError::InvalidRequest)
}

fn history_query(query: &str) -> Result<(u16, Option<&str>), NativeReadError> {
    let mut limit = None;
    let mut cursor = None;
    for field in query.split('&') {
        let (key, value) = field
            .split_once('=')
            .ok_or(NativeReadError::InvalidRequest)?;
        match key {
            "limit" if limit.is_none() => {
                limit = Some(
                    u16::try_from(number(value)?)
                        .ok()
                        .filter(|value| *value <= 256)
                        .ok_or(NativeReadError::InvalidRequest)?,
                );
            }
            "cursor" if cursor.is_none() => {
                digest(value)?;
                cursor = Some(value);
            }
            _ => return Err(NativeReadError::InvalidRequest),
        }
    }
    Ok((limit.ok_or(NativeReadError::InvalidRequest)?, cursor))
}

fn proof_value(proof: &Proof) -> Value {
    json!({"leaf_index": proof.leaf_index(), "leaf_count": proof.leaf_count(),
        "siblings": proof.siblings().iter().map(|bytes| hex::encode(bytes)).collect::<Vec<_>>()})
}

fn proof_json(bundle: &VerifiedProofBundle, observed: u64) -> Result<Value, NativeReadError> {
    let (kind, activity, proof) = match bundle {
        VerifiedProofBundle::Activity {
            activity_id, proof, ..
        } => ("activity", activity_id, proof),
        VerifiedProofBundle::Receipt {
            activity_id, proof, ..
        } => ("receipt", activity_id, proof),
        _ => return Err(NativeReadError::Verification),
    };
    let signed = bundle.signed_header();
    let header =
        decode_batch_header(&signed.canonical_bytes).map_err(|_| NativeReadError::Verification)?;
    Ok(
        json!({"kind": kind, "activity_id": hex::encode(activity), "canonical_hex": hex::encode(bundle.canonical_bytes()),
        "proof": proof_value(proof), "header_hex": hex::encode(&signed.canonical_bytes),
        "header_signature": hex::encode(&signed.signature), "sequencer_public_key": hex::encode(&signed.public_key),
        "verification_level": VerificationLevel::BATCH_INCLUDED.wire_rank(), "complete": true,
        "freshness": {"observed_sequence": observed, "batch_number": header.batch_number(), "observed_at": header.timestamp_ms()}}),
    )
}

fn actor_main_account(actor: &Did, protocol: u16) -> Result<[u8; 32], NativeReadError> {
    let did = std::str::from_utf8(actor.as_bytes()).map_err(|_| NativeReadError::Verification)?;
    let name = AccountId::parse(&format!("agent:{did}:main"))
        .map_err(|_| NativeReadError::Verification)?;
    hash::account_id_for_protocol(&name, protocol).map_err(|_| NativeReadError::Verification)
}

fn receipt_mentions_account(
    receipt: &ProtocolReceipt,
    activity: &Envelope,
    account: [u8; 32],
) -> Result<bool, NativeReadError> {
    if account == [0; 32] {
        return Err(NativeReadError::InvalidRequest);
    }
    let directly_named = receipt.from() == account
        || receipt.to() == account
        || actor_main_account(activity.actor_did(), activity.protocol_version())? == account;
    if receipt.module_id() != 8
        || activity.activity_type().ordinal() != 1
        || receipt.result_code() != 0
    {
        return Ok(directly_named);
    }
    let payload = activity.payload().as_bytes();
    if payload.len() != 427 || !matches!(&payload[..5], b"LXDC1" | b"LXDC2") {
        return Err(NativeReadError::Verification);
    }
    let payload_hash = Sha256::digest(payload);
    let expected = [
        &payload[43..139],
        &payload[191..207],
        &payload[5..37],
        &payload_hash[..],
    ]
    .concat();
    let mut deposits = receipt
        .effects()
        .iter()
        .filter(|effect| effect.module_id() == 8 && effect.event_type() == 1 && !effect.monetary());
    let deposit = deposits.next().ok_or(NativeReadError::Verification)?;
    if deposits.next().is_some() || deposit.body().len() != 208 || deposit.body()[..176] != expected
    {
        return Err(NativeReadError::Verification);
    }
    Ok(directly_named || deposit.body()[64..96] == account)
}

fn maintenance_json(
    bytes: &[u8],
    account: [u8; 32],
    sequence: u64,
) -> Result<Option<Value>, NativeReadError> {
    let record = layerx_wire::maintenance::decode_occupancy_maintenance(bytes)
        .map_err(|_| NativeReadError::Verification)?;
    if record.global_sequence != sequence {
        return Err(NativeReadError::Verification);
    }
    Ok(record.payers.iter().any(|payer| payer.principal == account).then(|| json!({
        "global_sequence": sequence.to_string(), "kind": "maintenance", "canonical_hex": hex::encode(bytes),
        "verification_level": VerificationLevel::BATCH_INCLUDED.wire_rank()})))
}
