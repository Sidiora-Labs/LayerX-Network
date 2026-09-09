use std::cell::Cell;
use std::time::{Duration, Instant};

use layerx_programs::{
    hex, AccountStateHead, DeploymentProof, ProgramId, ProtocolDeploymentVerifier, ReadFreshness,
    VerifiedDeploymentEvidence,
};
use layerx_proof::merkle::Proof;
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode as decode_receipt, decode_merkle_proof, encode_unsigned};
use serde_json::Value;

const ACCOUNT_ACTIVITY: u32 = 0x0009_0006;
const WIND_DOWN_ACTIVITY: u32 = 0x0009_0007;
const MAX_CHANGE_RECORDS: usize = 4_096;
const PROJECTION_STALE: i64 = -903;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HeadAnswer {
    Head,
    Pending,
    Refused(u16),
}

fn classify_head_answer(status: u16, body: &Value) -> HeadAnswer {
    if (200..300).contains(&status) {
        return HeadAnswer::Head;
    }
    if status == 503
        && body.as_object().is_some_and(|fields| fields.len() == 1)
        && body["error"].as_i64() == Some(PROJECTION_STALE)
    {
        return HeadAnswer::Pending;
    }
    HeadAnswer::Refused(status)
}

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
    deployment_verifier: ProtocolDeploymentVerifier,
    request_deadline: Cell<Option<Instant>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BatchEvidence {
    header: Vec<u8>,
    signature: [u8; 64],
    receipt_proof: Proof,
}

impl NodeProgramStateSource {
    ///
    /// # Errors
    /// Refuses missing authority configuration and endpoints that are not distinct HTTPS or loopback URLs.
    pub fn connect(
        endpoint: &str,
        authorization: String,
        outbound_ca_der: &[u8],
        authority_endpoint: &str,
        authority_authorization: String,
        authority_replica_id: [u8; 32],
        deployment_verifier: ProtocolDeploymentVerifier,
    ) -> Result<Self, String> {
        let endpoint = endpoint.trim_end_matches('/');
        let authority_endpoint = authority_endpoint.trim_end_matches('/');
        if authorization.is_empty()
            || authority_authorization.is_empty()
            || authority_replica_id == [0; 32]
        {
            return Err("node authorities and a configured verifier are required".to_owned());
        }
        if !(endpoint.starts_with("https://") || loopback_http(endpoint))
            || !(authority_endpoint.starts_with("https://") || loopback_http(authority_endpoint))
            || endpoint == authority_endpoint
        {
            return Err(
                "node state and independent receipt authority must be distinct HTTPS or loopback endpoints".to_owned(),
            );
        }
        if outbound_ca_der.is_empty() {
            return Err("an outbound CA certificate is required".to_owned());
        }
        let root = ureq::tls::Certificate::from_der(outbound_ca_der).to_owned();
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::new_with_certs(&[root]))
            .build();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .tls_config(tls)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint: endpoint.to_owned(),
            authorization,
            authority_endpoint: authority_endpoint.to_owned(),
            authority_authorization,
            authority_replica_id,
            deployment_verifier,
            request_deadline: Cell::new(None),
        })
    }

    /// # Errors
    /// Refuses native admission, unavailable proof material and mismatched activity bytes.
    pub fn deploy(&self, canonical: &[u8], deadline: Instant) -> Result<DeploymentProof, String> {
        self.set_request_deadline(deadline);
        let registration = layerx_types::payload::ModuleRegistration::new(
            layerx_types::payload::ModuleId::Programs,
            &[1, 2]
                .map(|ordinal| {
                    layerx_types::payload::ActivityType::new(
                        layerx_types::payload::ModuleId::Programs,
                        ordinal,
                    )
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        let registry = layerx_types::payload::ModuleRegistry::new(&[registration])
            .map_err(|error| format!("{error:?}"))?;
        let activity = layerx_wire::activity::decode_signed(canonical, &registry)
            .map_err(|error| format!("deployment activity: {error:?}"))?;
        let id = layerx_wire::hash::activity_id(&activity).map_err(|error| format!("{error:?}"))?;
        let operation = if activity.activity_type().ordinal() == 1 {
            "deploy"
        } else {
            "upgrade"
        };
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| "deployment deadline expired".to_owned())?;
        let mut response = self
            .agent
            .post(format!(
                "{}/internal/v1/programs/{operation}",
                self.endpoint
            ))
            .header("Authorization", &format!("Bearer {}", self.authorization))
            .header("Content-Type", "application/octet-stream")
            .config()
            .timeout_global(Some(remaining))
            .build()
            .send(canonical)
            .map_err(|error| format!("deployment submission: {error}"))?;
        if response.status().as_u16() != 202 {
            return Err(format!(
                "deployment admission returned HTTP {}",
                response.status()
            ));
        }
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        let acknowledgement: Value =
            serde_json::from_str(&body).map_err(|error| error.to_string())?;
        if hex::decode_digest(field(&acknowledgement, "activity_id")?)
            .map_err(|error| error.to_string())?
            != id
        {
            return Err("deployment acknowledgement names a different activity".to_owned());
        }
        let path = format!("/internal/v1/deployment-proof/{}", hex::encode(&id));
        loop {
            let (status, document) = self.fetch_from(&self.endpoint, &self.authorization, &path)?;
            if status == 200 {
                let bytes = hex::decode(field(&document, "proof_hex")?)
                    .map_err(|error| error.to_string())?;
                let proof = DeploymentProof::decode(&bytes).map_err(|error| error.to_string())?;
                if proof.activity != canonical {
                    return Err("deployment proof names different activity bytes".to_owned());
                }
                return Ok(proof);
            }
            if status != 503 || document["native_result"].as_i64() != Some(-106) {
                return Err(format!("deployment evidence refused with HTTP {status}"));
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| "deployment outcome unavailable at deadline".to_owned())?;
            std::thread::sleep(remaining.min(Duration::from_millis(20)));
        }
    }

    pub fn set_request_deadline(&self, deadline: Instant) {
        self.request_deadline.set(Some(deadline));
    }

    #[must_use]
    pub fn request_deadline_expired(&self) -> bool {
        self.request_deadline
            .get()
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    ///
    /// # Errors
    /// Returns the protocol verifier refusal for invalid or stale deployment evidence.
    pub fn verify_deployment(
        &self,
        proof: &DeploymentProof,
        now_ms: u64,
    ) -> Result<VerifiedDeploymentEvidence, String> {
        self.deployment_verifier
            .verify_deployment(proof, now_ms)
            .map_err(|error| format!("protocol deployment evidence refused: {error}"))
    }

    ///
    /// # Errors
    /// Returns the protocol verifier refusal for invalid historical deployment evidence.
    pub fn verify_stored_deployment(
        &self,
        proof: &DeploymentProof,
    ) -> Result<VerifiedDeploymentEvidence, String> {
        self.deployment_verifier
            .verify_historical_deployment(proof)
            .map_err(|error| format!("stored protocol deployment evidence refused: {error}"))
    }

    ///
    /// # Errors
    /// Refuses unavailable node or replica responses and invalid, unverified or stale head evidence.
    pub fn current_head(&self, now_ms: u64) -> Result<AccountStateHead, String> {
        self.parse_head(&self.get("/v1/protocol/account-state/head")?, Some(now_ms))
    }

    /// Reads the current head, distinguishing a network that has not yet
    /// sequenced its first receipt from every other refusal.
    ///
    /// # Errors
    /// Refuses unavailable node responses other than the stale-projection
    /// answer and invalid, unverified or stale head evidence.
    pub fn current_head_or_pending(&self, now_ms: u64) -> Result<Option<AccountStateHead>, String> {
        let path = "/v1/protocol/account-state/head";
        let (status, body) = self.fetch_from(&self.endpoint, &self.authorization, path)?;
        match classify_head_answer(status, &body) {
            HeadAnswer::Head => self.parse_head(&body, Some(now_ms)).map(Some),
            HeadAnswer::Pending => Ok(None),
            HeadAnswer::Refused(status) => {
                Err(format!("node authority GET {path} returned HTTP {status}"))
            }
        }
    }

    /// # Errors
    /// Refuses deployment evidence that differs from the independent receipt authority.
    pub fn verify_deployment_authority(&self, proof: &DeploymentProof) -> Result<(), String> {
        let decoded = decode_receipt(&proof.state.receipt)
            .map_err(|_| "deployment receipt decoding failed".to_owned())?;
        let protocol = decoded
            .protocol()
            .ok_or_else(|| "deployment receipt shape".to_owned())?;
        let digest = proof
            .claimed_receipt_digest()
            .map_err(|error| error.to_string())?;
        let path = format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex::encode(&protocol.batch_id()),
            hex::encode(&digest)
        );
        let document = self.get_authority(&path)?;
        let independent = parse_batch_evidence(&document["batch_evidence"])?;
        if independent.header != proof.state.header
            || independent.signature != proof.state.header_signature
            || independent.receipt_proof != proof.state.receipt_proof
            || hex::decode_digest(field(&document, "authority_replica_id")?)
                .map_err(|error| error.to_string())?
                != self.authority_replica_id
        {
            return Err("deployment evidence disagrees with independent authority".to_owned());
        }
        let key = hex::decode_digest(field(&document, "sequencer_public_key")?)
            .map_err(|error| error.to_string())?;
        layerx_proof::receipt::verify_sequencer_signature(&proof.state.receipt, key)
            .map_err(|error| format!("independent authority sequencer key refused: {error:?}"))?;
        Ok(())
    }

    ///
    /// # Errors
    /// Refuses unavailable or invalid evidence and a receipt digest different from the requested digest.
    pub fn receipt_head(&self, digest: [u8; 32]) -> Result<AccountStateHead, String> {
        let path = format!("/v1/receipts/{}/account-state", hex::encode(&digest));
        let head = self.parse_head(&self.get(&path)?, None)?;
        if head.receipt_digest != digest {
            return Err("node receipt lookup returned a different receipt digest".to_owned());
        }
        Ok(head)
    }

    ///
    /// # Errors
    /// Refuses unavailable or malformed records, digest mismatches and records not anchored at the current head.
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

    ///
    /// # Errors
    /// Refuses unavailable or malformed feeds, invalid cursors, record bounds and inconsistent receipt evidence.
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
        let (status, body) = self.fetch_from(endpoint, authorization, path)?;
        if !(200..300).contains(&status) {
            return Err(format!("node authority GET {path} returned HTTP {status}"));
        }
        Ok(body)
    }

    fn fetch_from(
        &self,
        endpoint: &str,
        authorization: &str,
        path: &str,
    ) -> Result<(u16, Value), String> {
        let remaining = self
            .request_deadline
            .get()
            .map_or(Some(Duration::from_secs(30)), |deadline| {
                deadline.checked_duration_since(Instant::now())
            })
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| {
                "registry request deadline expired before node authority access".to_owned()
            })?;
        let url = format!("{endpoint}{path}");
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {authorization}"))
            .config()
            .timeout_global(Some(remaining))
            .build()
            .call()
            .map_err(|error| format!("node authority GET {path} failed: {error}"))?;
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| format!("node authority GET {path} was unreadable: {error}"))?;
        if !status.is_success() {
            return Ok((
                status.as_u16(),
                serde_json::from_str(&body).unwrap_or(Value::Null),
            ));
        }
        serde_json::from_str(&body)
            .map(|value| (status.as_u16(), value))
            .map_err(|error| format!("node authority GET {path} returned invalid JSON: {error}"))
    }

    fn parse_head(&self, value: &Value, now_ms: Option<u64>) -> Result<AccountStateHead, String> {
        let current = value["current"]
            .as_bool()
            .ok_or_else(|| "node account-state response omitted current".to_owned())?;
        if now_ms.is_some() && !current {
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
        if hex::decode_digest(field(&independent_document, "authority_replica_id")?)
            .map_err(|error| format!("independent authority id is invalid: {error}"))?
            != self.authority_replica_id
        {
            return Err("independent authority declared a different replica id".to_owned());
        }
        let verified = match now_ms {
            Some(now_ms) => self.deployment_verifier.verify_current_protocol_head(
                &receipt_bytes,
                &independent_evidence.receipt_proof,
                &independent_evidence.header,
                &independent_evidence.signature,
                now_ms,
            ),
            None => self.deployment_verifier.verify_historical_protocol_head(
                &receipt_bytes,
                &independent_evidence.receipt_proof,
                &independent_evidence.header,
                &independent_evidence.signature,
            ),
        }
        .map_err(|error| format!("program-state receipt verification failed: {error}"))?;
        if hex::decode_digest(field(&independent_document, "sequencer_public_key")?)
            .map_err(|error| format!("independent sequencer key is invalid: {error}"))?
            != verified.sequencer_public_key()
        {
            return Err("independent authority declared a different sequencer key".to_owned());
        }
        let declared_digest = hex::decode_digest(field(value, "receipt_digest")?)
            .map_err(|error| format!("node receipt digest is invalid: {error}"))?;
        let declared_root = hex::decode_digest(field(value, "state_root")?)
            .map_err(|error| format!("node state root is invalid: {error}"))?;
        if declared_digest != verified.receipt_digest()
            || declared_root != verified.state_root()
            || value["observed_sequence"].as_u64() != Some(verified.freshness().observed_sequence)
            || value["observed_at"].as_u64() != Some(verified.freshness().observed_at)
        {
            return Err("node account-state claims disagree with the verified receipt".to_owned());
        }
        Ok(AccountStateHead {
            receipt_digest: verified.receipt_digest(),
            state_root: verified.state_root(),
            freshness: ReadFreshness {
                observed_sequence: verified.freshness().observed_sequence,
                observed_at: verified.freshness().observed_at,
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
    let canonical = decode_merkle_proof(&proof_bytes)
        .map_err(|error| format!("receipt proof is non-canonical: {error:?}"))?;
    let receipt_proof = Proof::new(
        canonical.leaf_index(),
        canonical.leaf_count(),
        canonical.siblings().to_vec(),
    )
    .map_err(|error| format!("receipt proof structure is invalid: {error:?}"))?;
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

#[cfg(test)]
mod tests {
    use super::{classify_head_answer, HeadAnswer};
    use serde_json::json;

    #[test]
    fn only_the_stale_projection_answer_is_a_pending_head() {
        assert_eq!(
            classify_head_answer(503, &json!({"error": -903})),
            HeadAnswer::Pending
        );
        assert_eq!(
            classify_head_answer(200, &json!({"current": true})),
            HeadAnswer::Head
        );
        for (status, body) in [
            (503, json!({"error": -903, "detail": "x"})),
            (503, json!({"error": -902})),
            (503, json!({"error": "node_unavailable"})),
            (503, json!(null)),
            (500, json!({"error": -903})),
            (404, json!({"error": -903})),
        ] {
            assert_eq!(
                classify_head_answer(status, &body),
                HeadAnswer::Refused(status)
            );
        }
    }
}
