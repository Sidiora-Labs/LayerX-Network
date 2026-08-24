//! Agent read route backed by the authenticated layerxd Programs service.

use std::time::Duration;

use layerx_programs::{hex, AccountStateHead, ProgramId, ReadFreshness, Registry};
use layerx_programs_protocol_adapter::{ProtocolAdapterError, ProtocolProgramStateRead};
use layerx_proof::inclusion::{verify_receipt as verify_receipt_inclusion, SequencerAuthorization};
use layerx_proof::merkle::{decode_proof, Proof};
use layerx_proof::receipt::{verify_program_state, AuthorizedBatch};
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode as decode_receipt, encode_unsigned};
use serde_json::Value;

use super::program_balances_impl::{program_balances, ProgramBalanceRead};

#[derive(Clone, Debug, Eq, PartialEq)]
struct BatchEvidence {
    header: Vec<u8>,
    signature: [u8; 64],
    receipt_proof: Proof,
}

/// Production agent reader connected to layerxd and an independent layerxd
/// receipt-authority replica. Neither endpoint is allowed to be agent-local.
pub struct LayerxdProgramBalanceReader {
    agent: ureq::Agent,
    endpoint: String,
    authorization: String,
    authority_endpoint: String,
    authority_authorization: String,
    authority_replica_id: [u8; 32],
    sequencer_authorization: SequencerAuthorization,
    sequencer_public_key: [u8; 32],
    registry: Registry,
    staleness_limit: u64,
}

impl LayerxdProgramBalanceReader {
    /// Connects the running agent route to the production node pair.
    pub fn connect(
        endpoint: &str,
        authorization: String,
        authority_endpoint: &str,
        authority_authorization: String,
        authority_replica_id: [u8; 32],
        sequencer_id: [u8; 32],
        sequencer_public_key: [u8; 32],
        first_batch: u64,
        last_batch: u64,
        registry: Registry,
        staleness_limit: u64,
    ) -> Result<Self, ProtocolAdapterError> {
        let endpoint = endpoint.trim_end_matches('/');
        let authority_endpoint = authority_endpoint.trim_end_matches('/');
        if authorization.is_empty()
            || authority_authorization.is_empty()
            || authority_replica_id == [0; 32]
            || sequencer_id == [0; 32]
            || sequencer_public_key == [0; 32]
            || first_batch == 0
            || last_batch < first_batch
            || staleness_limit == 0
            || endpoint == authority_endpoint
            || !secure_endpoint(endpoint)
            || !secure_endpoint(authority_endpoint)
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint: endpoint.to_owned(),
            authorization,
            authority_endpoint: authority_endpoint.to_owned(),
            authority_authorization,
            authority_replica_id,
            sequencer_authorization: SequencerAuthorization::new(
                sequencer_id,
                sequencer_public_key,
                first_batch,
                last_batch,
            ),
            sequencer_public_key,
            registry,
            staleness_limit,
        })
    }

    /// Reads and locally re-verifies one complete current protocol state.
    pub fn read_protocol_state(
        &mut self,
        program: ProgramId,
        now: u64,
    ) -> Result<ProtocolProgramStateRead, ProtocolAdapterError> {
        if now == 0 {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let head_document = self.get(
            &self.endpoint,
            &self.authorization,
            "/v1/protocol/account-state/head",
        )?;
        let head = self.verify_head(&head_document, true)?;
        let path = format!(
            "/v1/programs/{}/account-state?at={}",
            hex::encode(&program.bytes()),
            head.freshness.observed_sequence
        );
        let document = self.get(&self.endpoint, &self.authorization, &path)?;
        let bytes = hex::decode(field(&document, "record_hex")?)
            .map_err(|_| ProtocolAdapterError::CorruptRecord)?;
        let declared = hex::decode_digest(field(&document, "record_digest")?)
            .map_err(|_| ProtocolAdapterError::CorruptRecord)?;
        if digest(&bytes) != declared
            || hex::decode_digest(field(&document, "receipt_digest")?)
                .map_err(|_| ProtocolAdapterError::CorruptRecord)?
                != head.receipt_digest
        {
            return Err(ProtocolAdapterError::CorruptRecord);
        }
        ProtocolProgramStateRead::restore_verified(
            &bytes,
            &mut self.registry,
            head,
            head,
            now,
            self.staleness_limit,
        )
    }

    /// Serves the agent's balance model only from the verified protocol read.
    pub fn read(
        &mut self,
        program: ProgramId,
        now: u64,
    ) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
        let state = self.read_protocol_state(program, now)?;
        program_balances(state.into_balances(), self.staleness_limit)
    }

    #[must_use]
    pub const fn staleness_limit(&self) -> u64 {
        self.staleness_limit
    }

    fn verify_head(
        &self,
        value: &Value,
        require_current: bool,
    ) -> Result<AccountStateHead, ProtocolAdapterError> {
        if require_current && value["current"].as_bool() != Some(true) {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let receipt_bytes = hex::decode(field(value, "receipt_hex")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let decoded =
            decode_receipt(&receipt_bytes).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let protocol = decoded
            .protocol()
            .ok_or(ProtocolAdapterError::NonCanonicalView)?;
        let unsigned =
            encode_unsigned(&decoded).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let digest =
            receipt_digest(&unsigned).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let node = batch_evidence(&value["batch_evidence"])?;
        let path = format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex::encode(&protocol.batch_id()),
            hex::encode(&digest)
        );
        let authority_document = self.get(
            &self.authority_endpoint,
            &self.authority_authorization,
            &path,
        )?;
        if hex::decode_digest(field(&authority_document, "sequencer_public_key")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
            != self.sequencer_public_key
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        if hex::decode_digest(field(&authority_document, "authority_replica_id")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
            != self.authority_replica_id
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let independent = batch_evidence(&authority_document["batch_evidence"])?;
        if node != independent {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let inclusion = verify_receipt_inclusion(
            &receipt_bytes,
            &independent.receipt_proof,
            &independent.header,
            &independent.signature,
            &self.sequencer_authorization,
        )
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let header = inclusion.header().header();
        if protocol.global_sequence() < header.first_sequence()
            || protocol.global_sequence() > header.last_sequence()
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let authorized = AuthorizedBatch::new(
            protocol.batch_id(),
            protocol.asset(),
            header.previous_state_root(),
            header.resulting_state_root(),
            self.sequencer_public_key,
        );
        let verified = verify_program_state(&receipt_bytes, &authorized)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let verified_protocol = verified
            .receipt()
            .protocol()
            .ok_or(ProtocolAdapterError::NonCanonicalView)?;
        let state_root = hex::decode_digest(field(value, "state_root")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        if hex::decode_digest(field(value, "receipt_digest")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
            != digest
            || state_root != verified_protocol.resulting_state_root()
            || value["observed_sequence"].as_u64() != Some(verified_protocol.global_sequence())
            || value["observed_at"].as_u64() != Some(verified_protocol.timestamp())
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        Ok(AccountStateHead {
            receipt_digest: digest,
            state_root,
            freshness: ReadFreshness {
                observed_sequence: verified_protocol.global_sequence(),
                observed_at: verified_protocol.timestamp(),
            },
        })
    }

    fn get(
        &self,
        endpoint: &str,
        authorization: &str,
        path: &str,
    ) -> Result<Value, ProtocolAdapterError> {
        let url = format!("{endpoint}{path}");
        let mut response = self
            .agent
            .get(&url)
            .header("Authorization", &format!("Bearer {authorization}"))
            .call()
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        if !response.status().is_success() {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        serde_json::from_str(&body).map_err(|_| ProtocolAdapterError::NonCanonicalView)
    }
}

/// Agent service route that always refreshes from layerxd at request time.
pub struct ProgramBalanceReadRoute {
    reader: LayerxdProgramBalanceReader,
}

impl ProgramBalanceReadRoute {
    #[must_use]
    pub const fn new(reader: LayerxdProgramBalanceReader) -> Self {
        Self { reader }
    }

    pub fn read(
        &mut self,
        program: ProgramId,
        now: u64,
    ) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
        self.reader.read(program, now)
    }
}

fn batch_evidence(value: &Value) -> Result<BatchEvidence, ProtocolAdapterError> {
    let header = hex::decode(field(value, "header_hex")?)
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    let signature = hex::decode(field(value, "header_signature")?)
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
        .try_into()
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    let proof = hex::decode(field(value, "receipt_proof_hex")?)
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    Ok(BatchEvidence {
        header,
        signature,
        receipt_proof: decode_proof(&proof).map_err(|_| ProtocolAdapterError::NonCanonicalView)?,
    })
}

fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, ProtocolAdapterError> {
    value[name]
        .as_str()
        .ok_or(ProtocolAdapterError::NonCanonicalView)
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).into()
}

fn secure_endpoint(endpoint: &str) -> bool {
    endpoint.starts_with("https://")
        || endpoint
            .strip_prefix("http://")
            .and_then(|value| value.split('/').next())
            .is_some_and(|host| {
                host == "localhost"
                    || host.starts_with("localhost:")
                    || host == "127.0.0.1"
                    || host.starts_with("127.0.0.1:")
                    || host == "[::1]"
                    || host.starts_with("[::1]:")
            })
}
