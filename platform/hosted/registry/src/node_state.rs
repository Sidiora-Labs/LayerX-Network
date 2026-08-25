use std::time::Duration;

use layerx_programs::{
    hex, AccountStateHead, DeploymentProof, ProgramId, ProtocolDeploymentVerifier,
    ReadFreshness, VerifiedDeploymentEvidence,
};
use layerx_proof::inclusion::{verify_receipt as verify_receipt_inclusion, SequencerAuthorization};
use layerx_proof::merkle::{decode_proof, Proof};
use layerx_proof::receipt::{verify_program_state, AuthorizedBatch};
use layerx_wire::hash::{execution_batch_id, receipt_digest};
use layerx_wire::receipt::{decode as decode_receipt, encode_unsigned};
use serde_json::Value;

const ACCOUNT_ACTIVITY: u32 = 0x0009_0006;
const WIND_DOWN_ACTIVITY: u32 = 0x0009_0007;
const MAX_CHANGE_RECORDS: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProgramStateCursor {
    pub sequence: u64,
    pub ordinal: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramStateNotice {
    pub cursor: ProgramStateCursor,
    pub program: ProgramId,
    pub activity_type: u32,
    pub event_type: u16,
    pub receipt_digest: [u8; 32],
}

pub struct ProgramStateRecord {
    pub program: ProgramId,
    pub bytes: Vec<u8>,
    pub receipt: AccountStateHead,
}

pub struct NodeProgramStateSource {
    agent: ureq::Agent,
    endpoint: String,
    authorization: String,
    authority_endpoint: String,
    authority_authorization: String,
    authority_replica_id: [u8; 32],
    expected_protocol_version: u16,
    expected_network_id: u32,
    expected_epoch: u64,
    revoked_from_batch: Option<u64>,
    sequencer_authorization: SequencerAuthorization,
    sequencer_public_key: [u8; 32],
    deployment_verifier: ProtocolDeploymentVerifier,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BatchEvidence {
    header: Vec<u8>,
    signature: [u8; 64],
    receipt_proof: Proof,
}

impl NodeProgramStateSource {
    pub fn connect(
        endpoint: &str,
        authorization: String,
        authority_endpoint: &str,
        authority_authorization: String,
        authority_replica_id: [u8; 32],
        expected_protocol_version: u16,
        expected_network_id: u32,
        expected_epoch: u64,
        sequencer_id: [u8; 32],
        sequencer_public_key: [u8; 32],
        sequencer_first_batch: u64,
        sequencer_last_batch: u64,
        revoked_from_batch: Option<u64>,
        staleness_limit_ms: u64,
    ) -> Result<Self, String> {
        let endpoint = endpoint.trim_end_matches('/');
        let authority_endpoint = authority_endpoint.trim_end_matches('/');
        if authorization.is_empty()
            || authority_authorization.is_empty()
            || authority_replica_id == [0; 32]
            || !matches!(expected_protocol_version, 1 | 2)
            || expected_network_id == 0
            || expected_epoch == 0
            || sequencer_id == [0; 32]
            || sequencer_public_key == [0; 32]
            || sequencer_first_batch == 0
            || sequencer_last_batch < sequencer_first_batch
            || revoked_from_batch
                .is_some_and(|batch| batch == 0 || batch <= sequencer_first_batch)
            || staleness_limit_ms == 0
        {
            return Err(
                "node authorities, protocol domain, key window and millisecond freshness are required"
                    .to_owned(),
            );
        }
        if !(endpoint.starts_with("https://") || loopback_http(endpoint))
            || !(authority_endpoint.starts_with("https://") || loopback_http(authority_endpoint))
            || endpoint == authority_endpoint
        {
            return Err(
                "node state and independent receipt authority must be distinct HTTPS or loopback endpoints".to_owned(),
            );
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build();
        let deployment_verifier = ProtocolDeploymentVerifier::new(
            expected_protocol_version,
            expected_network_id,
            expected_epoch,
            sequencer_id,
            sequencer_public_key,
            sequencer_first_batch,
            sequencer_last_batch,
            revoked_from_batch,
            staleness_limit_ms,
        )
        .map_err(|error| format!("deployment verifier is not configured: {error}"))?;
        Ok(Self {
            agent: config.into(),
            endpoint: endpoint.to_owned(),
            authorization,
            authority_endpoint: authority_endpoint.to_owned(),
            authority_authorization,
            authority_replica_id,
            expected_protocol_version,
            expected_network_id,
            expected_epoch,
            revoked_from_batch,
            sequencer_authorization: SequencerAuthorization::new(
                sequencer_id,
                sequencer_public_key,
                sequencer_first_batch,
                sequencer_last_batch,
            ),
            sequencer_public_key,
            deployment_verifier,
        })
    }

    pub fn verify_deployment(
        &self,
        proof: &DeploymentProof,
        now_ms: u64,
    ) -> Result<VerifiedDeploymentEvidence, String> {
        self.deployment_verifier
            .verify_deployment(proof, now_ms)
            .map_err(|error| format!("protocol deployment evidence refused: {error}"))
    }

    pub fn verify_stored_deployment(
        &self,
        proof: &DeploymentProof,
    ) -> Result<VerifiedDeploymentEvidence, String> {
        self.deployment_verifier
            .verify_historical_deployment(proof)
            .map_err(|error| format!("stored protocol deployment evidence refused: {error}"))
    }

    pub fn current_head(&self) -> Result<AccountStateHead, String> {
        self.parse_head(&self.get("/v1/protocol/account-state/head")?, true)
    }

    pub fn receipt_head(&self, digest: [u8; 32]) -> Result<AccountStateHead, String> {
        let path = format!("/v1/receipts/{}/account-state", hex::encode(&digest));
        let head = self.parse_head(&self.get(&path)?, false)?;
        if head.receipt_digest != digest {
            return Err("node receipt lookup returned a different receipt digest".to_owned());
        }
        Ok(head)
    }

    pub fn program_state(
        &self,
        program: ProgramId,
        current_head: AccountStateHead,
    ) -> Result<ProgramStateRecord, String> {
        let path = format!(
            "/v1/programs/{}/account-state?at={}",
            hex::encode(&program.bytes()),
            current_head.freshness.observed_sequence
        );
        let document = self.get(&path)?;
        let encoded = field(&document, "record_hex")?;
        let bytes = hex::decode(encoded)
            .map_err(|error| format!("node program-state record is not hexadecimal: {error}"))?;
        if bytes.is_empty() {
            return Err("node program-state record is empty".to_owned());
        }
        let record_digest = digest(&bytes);
        if hex::decode_digest(field(&document, "record_digest")?)
            .map_err(|error| format!("node program-state digest is invalid: {error}"))?
            != record_digest
        {
            return Err("node program-state record does not match its content digest".to_owned());
        }
        let receipt_digest = hex::decode_digest(field(&document, "receipt_digest")?)
            .map_err(|error| format!("node program-state receipt digest is invalid: {error}"))?;
        let receipt = self.receipt_head(receipt_digest)?;
        if receipt != current_head {
            return Err("node program-state record is not anchored at the current head".to_owned());
        }
        Ok(ProgramStateRecord {
            program,
            bytes,
            receipt,
        })
    }

    pub fn changes(
        &self,
        after: ProgramStateCursor,
    ) -> Result<(Vec<ProgramStateNotice>, ProgramStateCursor, u64, bool), String> {
        let path = format!(
            "/v1/programs/account-state/changes?after_sequence={}",
            after.sequence
        );
        if after.ordinal != 0 {
            return Err("durable scan cursors cannot carry an event ordinal".to_owned());
        }
        let document = self.get(&path)?;
        let complete = parse_cursor(&document["complete_through"])?;
        let caught_up = document["caught_up"]
            .as_bool()
            .ok_or_else(|| "node program-state change feed omitted caught_up".to_owned())?;
        let scanned_through_sequence =
            document["scanned_through_sequence"]
                .as_u64()
                .ok_or_else(|| {
                    "node program-state change feed omitted scanned_through_sequence".to_owned()
                })?;
        if scanned_through_sequence < complete.sequence {
            return Err("node change feed cursor is ahead of its canonical scan".to_owned());
        }
        let records = document["records"]
            .as_array()
            .ok_or_else(|| "node program-state change feed omitted records".to_owned())?;
        if records.len() > MAX_CHANGE_RECORDS {
            return Err("node program-state change feed exceeds its record bound".to_owned());
        }
        let mut notices = Vec::with_capacity(records.len());
        let mut prior_notice: Option<ProgramStateCursor> = None;
        for record in records {
            let cursor = parse_cursor(record)?;
            let program = ProgramId::new(
                hex::decode_digest(field(record, "program_id")?)
                    .map_err(|error| format!("change program id is invalid: {error}"))?,
            )
            .map_err(|error| format!("change program id is reserved: {error}"))?;
            let activity_type = u32::try_from(
                record["activity_type"]
                    .as_u64()
                    .ok_or_else(|| "change omitted activity_type".to_owned())?,
            )
            .map_err(|_| "change activity_type is out of range".to_owned())?;
            let event_type = u16::try_from(
                record["event_type"]
                    .as_u64()
                    .ok_or_else(|| "change omitted event_type".to_owned())?,
            )
            .map_err(|_| "change event_type is out of range".to_owned())?;
            let receipt_digest = hex::decode_digest(field(record, "receipt_digest")?)
                .map_err(|error| format!("change receipt digest is invalid: {error}"))?;
            let receipt = self.receipt_head(receipt_digest)?;
            let ordered = prior_notice.is_none_or(|prior| {
                if cursor.sequence == prior.sequence {
                    cursor.ordinal == prior.ordinal.saturating_add(1)
                } else {
                    cursor.sequence > prior.sequence && cursor.ordinal == 0
                }
            });
            if !ordered
                || cursor.sequence <= after.sequence
                || cursor.sequence > complete.sequence
                || receipt.freshness.observed_sequence != cursor.sequence
                || !matches!(activity_type, ACCOUNT_ACTIVITY | WIND_DOWN_ACTIVITY)
                || !(8..=12).contains(&event_type)
            {
                return Err("node program-state change feed is non-canonical".to_owned());
            }
            prior_notice = Some(cursor);
            notices.push(ProgramStateNotice {
                cursor,
                program,
                activity_type,
                event_type,
                receipt_digest,
            });
        }
        if complete.ordinal != 0 || complete.sequence < after.sequence {
            return Err("node program-state scan cursor is non-canonical".to_owned());
        }
        if caught_up != (complete.sequence == scanned_through_sequence) {
            return Err("node program-state caught_up disagrees with its scan head".to_owned());
        }
        Ok((notices, complete, scanned_through_sequence, caught_up))
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        self.get_from(&self.endpoint, &self.authorization, path)
    }

    fn get_authority(&self, path: &str) -> Result<Value, String> {
        self.get_from(
            &self.authority_endpoint,
            &self.authority_authorization,
            path,
        )
    }

    fn get_from(&self, endpoint: &str, authorization: &str, path: &str) -> Result<Value, String> {
        let url = format!("{endpoint}{path}");
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {authorization}"))
            .call()
            .map_err(|error| format!("node authority GET {path} failed: {error}"))?;
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("node authority GET {path} was unreadable: {error}"))?;
        if !status.is_success() {
            return Err(format!(
                "node authority GET {path} returned HTTP {}",
                status.as_u16()
            ));
        }
        serde_json::from_str(&body)
            .map_err(|error| format!("node authority GET {path} returned invalid JSON: {error}"))
    }

    fn parse_head(&self, value: &Value, require_current: bool) -> Result<AccountStateHead, String> {
        let current = value["current"]
            .as_bool()
            .ok_or_else(|| "node account-state response omitted current".to_owned())?;
        if require_current && !current {
            return Err("node account-state response is not the current head".to_owned());
        }
        let receipt_bytes = hex::decode(field(value, "receipt_hex")?)
            .map_err(|error| format!("node receipt bytes are invalid: {error}"))?;
        let decoded = decode_receipt(&receipt_bytes)
            .map_err(|_| "node receipt is not canonically decodable".to_owned())?;
        let protocol = decoded
            .protocol()
            .ok_or_else(|| "node returned a non-protocol receipt".to_owned())?;
        let batch_path = format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex::encode(&protocol.batch_id()),
            hex::encode(
                &receipt_digest(
                    &encode_unsigned(&decoded)
                        .map_err(|_| { "node receipt could not be encoded unsigned".to_owned() })?
                )
                .map_err(|_| "node receipt digest could not be computed".to_owned())?
            )
        );
        let node_evidence = parse_batch_evidence(&value["batch_evidence"])?;
        let independent_document = self.get_authority(&batch_path)?;
        let independent_evidence = parse_batch_evidence(&independent_document["batch_evidence"])?;
        if node_evidence != independent_evidence {
            return Err("node batch authority disagrees with the independent authority".to_owned());
        }
        if hex::decode_digest(field(&independent_document, "sequencer_public_key")?)
            .map_err(|error| format!("independent sequencer key is invalid: {error}"))?
            != self.sequencer_public_key
        {
            return Err("independent authority declared a different sequencer key".to_owned());
        }
        if hex::decode_digest(field(&independent_document, "authority_replica_id")?)
            .map_err(|error| format!("independent authority id is invalid: {error}"))?
            != self.authority_replica_id
        {
            return Err("independent authority declared a different replica id".to_owned());
        }
        let inclusion = verify_receipt_inclusion(
            &receipt_bytes,
            &independent_evidence.receipt_proof,
            &independent_evidence.header,
            &independent_evidence.signature,
            &self.sequencer_authorization,
        )
        .map_err(|error| format!("program-state receipt inclusion failed: {error:?}"))?;
        let header = inclusion.header().header();
        if header.protocol_version() != self.expected_protocol_version
            || header.network_id() != self.expected_network_id
            || header.epoch() != self.expected_epoch
        {
            return Err("program-state receipt belongs to another protocol domain".to_owned());
        }
        if self
            .revoked_from_batch
            .is_some_and(|revoked| header.batch_number() >= revoked)
        {
            return Err("program-state receipt uses a revoked sequencer key".to_owned());
        }
        if protocol.global_sequence() < header.first_sequence()
            || protocol.global_sequence() > header.last_sequence()
        {
            return Err("program-state receipt sequence is outside its signed batch".to_owned());
        }
        let expected_batch_id = execution_batch_id(
            header.previous_state_root(),
            protocol.activity_id(),
            protocol.global_sequence(),
            header.batch_number(),
        )
        .map_err(|_| "program-state batch identifier could not be derived".to_owned())?;
        if protocol.batch_id() != expected_batch_id {
            return Err("program-state receipt carries the wrong batch identifier".to_owned());
        }
        let authorized = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            self.sequencer_public_key,
        );
        let verified = verify_program_state(&receipt_bytes, &authorized).map_err(|error| {
            format!(
                "program-state receipt verification failed: {:?}",
                error.check
            )
        })?;
        let verified_protocol = verified
            .receipt()
            .protocol()
            .ok_or_else(|| "verified state receipt lost its protocol body".to_owned())?;
        if verified_protocol.protocol_version() != header.protocol_version()
            || verified_protocol.timestamp() != header.timestamp_ms()
            || verified_protocol.activity_root() != header.activity_merkle_root()
        {
            return Err(
                "program-state receipt disagrees with its signed batch header".to_owned(),
            );
        }
        let unsigned = encode_unsigned(verified.receipt())
            .map_err(|_| "verified state receipt could not be encoded unsigned".to_owned())?;
        let verified_digest = receipt_digest(&unsigned)
            .map_err(|_| "verified state receipt digest could not be computed".to_owned())?;
        let declared_digest = hex::decode_digest(field(value, "receipt_digest")?)
            .map_err(|error| format!("node receipt digest is invalid: {error}"))?;
        let declared_root = hex::decode_digest(field(value, "state_root")?)
            .map_err(|error| format!("node state root is invalid: {error}"))?;
        if declared_digest != verified_digest
            || declared_root != verified_protocol.resulting_state_root()
            || value["observed_sequence"].as_u64() != Some(verified_protocol.global_sequence())
            || value["observed_at"].as_u64() != Some(verified_protocol.timestamp())
        {
            return Err("node account-state claims disagree with the verified receipt".to_owned());
        }
        Ok(AccountStateHead {
            receipt_digest: verified_digest,
            state_root: verified_protocol.resulting_state_root(),
            freshness: ReadFreshness {
                observed_sequence: verified_protocol.global_sequence(),
                observed_at: verified_protocol.timestamp(),
            },
        })
    }
}

fn parse_batch_evidence(value: &Value) -> Result<BatchEvidence, String> {
    let header = hex::decode(field(value, "header_hex")?)
        .map_err(|error| format!("batch authority header is invalid: {error}"))?;
    let signature = hex::decode(field(value, "header_signature")?)
        .map_err(|error| format!("batch authority signature is invalid: {error}"))?
        .try_into()
        .map_err(|_| "batch authority signature must be sixty-four bytes".to_owned())?;
    let proof_bytes = hex::decode(field(value, "receipt_proof_hex")?)
        .map_err(|error| format!("receipt proof is invalid: {error}"))?;
    let receipt_proof = decode_proof(&proof_bytes)
        .map_err(|error| format!("receipt proof is non-canonical: {error:?}"))?;
    Ok(BatchEvidence {
        header,
        signature,
        receipt_proof,
    })
}

fn parse_cursor(value: &Value) -> Result<ProgramStateCursor, String> {
    let sequence = value["sequence"]
        .as_u64()
        .ok_or_else(|| "program-state cursor omitted sequence".to_owned())?;
    let ordinal = u32::try_from(
        value["ordinal"]
            .as_u64()
            .ok_or_else(|| "program-state cursor omitted ordinal".to_owned())?,
    )
    .map_err(|_| "program-state cursor ordinal is out of range".to_owned())?;
    Ok(ProgramStateCursor { sequence, ordinal })
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .ok_or_else(|| format!("node response omitted {name}"))
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

fn loopback_http(endpoint: &str) -> bool {
    endpoint
        .strip_prefix("http://")
        .and_then(|authority| authority.split('/').next())
        .is_some_and(|host| {
            host == "localhost"
                || host.starts_with("localhost:")
                || host == "127.0.0.1"
                || host.starts_with("127.0.0.1:")
                || host == "[::1]"
                || host.starts_with("[::1]:")
        })
}
