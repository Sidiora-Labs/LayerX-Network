//! Agent read route backed by the authenticated layerxd Programs service.

use std::time::Duration;

use layerx_programs::{
    hex, AccountStateHead, ProgramId, ProtocolDeploymentVerifier, ProtocolHeadMaintenanceProof,
    ProtocolHeadProof, Registry,
};
use layerx_programs_protocol_adapter::{ProtocolAdapterError, ProtocolProgramStateRead};
use layerx_proof::merkle::{decode_proof, Proof};
use layerx_wire::hash::receipt_digest;
use layerx_wire::receipt::{decode as decode_receipt, encode_unsigned};
use serde_json::Value;

use super::program_balances_impl::{program_balances, ProgramBalanceRead};

#[derive(Clone, Debug, Eq, PartialEq)]
struct BatchEvidence {
    header: Vec<u8>,
    signature: [u8; 64],
    receipt_proof: Proof,
    batch_identity: Value,
    maintenance: Option<ProtocolHeadMaintenanceProof>,
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
    verifier: ProtocolDeploymentVerifier,
    registry: Registry,
    staleness_limit: u64,
}

/// Explicit independent authority identity and outbound trust for both read endpoints.
pub struct ProgramAuthority<'a> {
    pub endpoint: &'a str,
    pub authorization: String,
    pub replica_id: [u8; 32],
    pub ca_der: &'a [u8],
}

impl LayerxdProgramBalanceReader {
    /// Connects the running agent route to the production node pair.
    ///
    /// # Errors
    ///
    /// Refuses empty authorizations, a zero replica id, identical or insecure
    /// endpoints as a non-canonical view.
    pub fn connect(
        endpoint: &str,
        authorization: String,
        authority: ProgramAuthority<'_>,
        verifier: ProtocolDeploymentVerifier,
        registry: Registry,
    ) -> Result<Self, ProtocolAdapterError> {
        let ProgramAuthority {
            endpoint: authority_endpoint,
            authorization: authority_authorization,
            replica_id: authority_replica_id,
            ca_der,
        } = authority;
        let endpoint = endpoint.trim_end_matches('/');
        let authority_endpoint = authority_endpoint.trim_end_matches('/');
        if authorization.is_empty()
            || authority_authorization.is_empty()
            || authority_replica_id == [0; 32]
            || endpoint == authority_endpoint
            || !secure_endpoint(endpoint)
            || !secure_endpoint(authority_endpoint)
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let tls = crate::outbound_tls::private_ca(ca_der)
            .ok_or(ProtocolAdapterError::NonCanonicalView)?;
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
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
            staleness_limit: verifier.staleness_limit_ms(),
            verifier,
            registry,
        })
    }

    /// Reads and locally re-verifies one complete current protocol state.
    ///
    /// # Errors
    ///
    /// Refuses a zero clock as a non-canonical view and surfaces transport,
    /// decode, and verification refusals from the node pair unchanged.
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
        let head = self.verify_head(&head_document, now)?;
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
    ///
    /// # Errors
    ///
    /// Propagates every protocol-state refusal and the freshness-window
    /// projection failure unchanged.
    pub fn read(
        &mut self,
        program: ProgramId,
        now: u64,
    ) -> Result<ProgramBalanceRead, ProtocolAdapterError> {
        let state = self.read_protocol_state(program, now)?;
        program_balances(&state.into_balances(), self.staleness_limit)
    }

    #[must_use]
    pub const fn staleness_limit(&self) -> u64 {
        self.staleness_limit
    }

    fn verify_head(
        &self,
        value: &Value,
        now_ms: u64,
    ) -> Result<AccountStateHead, ProtocolAdapterError> {
        if value["current"].as_bool() != Some(true) {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let receipt_bytes = hex::decode(field(value, "receipt_hex")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let node = batch_evidence(&value["batch_evidence"])?;
        let (batch_id, receipt_digest) = head_identity(&receipt_bytes, &node)?;
        let path = format!(
            "/v1/batches/{}/receipt-authority?receipt_digest={}",
            hex::encode(&batch_id),
            hex::encode(&receipt_digest)
        );
        let authority_document = self.get(
            &self.authority_endpoint,
            &self.authority_authorization,
            &path,
        )?;
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
        let (verified, sequencer_key) =
            self.verify_head_claims(&receipt_bytes, &independent, now_ms)?;
        if hex::decode_digest(field(&authority_document, "sequencer_public_key")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
            != sequencer_key
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let state_root = hex::decode_digest(field(value, "state_root")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        if hex::decode_digest(field(value, "receipt_digest")?)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?
            != verified.receipt_digest
            || receipt_digest != verified.receipt_digest
            || state_root != verified.state_root
            || value["observed_sequence"].as_u64() != Some(verified.freshness.observed_sequence)
            || value["observed_at"].as_u64() != Some(verified.freshness.observed_at)
        {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        Ok(verified)
    }

    fn verify_head_claims(
        &self,
        receipt: &[u8],
        evidence: &BatchEvidence,
        now_ms: u64,
    ) -> Result<(AccountStateHead, [u8; 32]), ProtocolAdapterError> {
        if receipt.starts_with(b"LXP/programs/occupancy-receipt/v2\0") {
            return self
                .verifier
                .verify_current_maintenance_head(
                    receipt,
                    &evidence.receipt_proof,
                    &evidence.header,
                    &evidence.signature,
                    now_ms,
                )
                .map_err(|_| ProtocolAdapterError::NonCanonicalView);
        }
        let verified = self
            .verifier
            .verify_current_protocol_head_proof(
                &ProtocolHeadProof {
                    receipt,
                    receipt_proof: &evidence.receipt_proof,
                    header: &evidence.header,
                    header_signature: &evidence.signature,
                    maintenance: evidence.maintenance.as_ref(),
                },
                now_ms,
            )
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        Ok((
            AccountStateHead {
                receipt_digest: verified.receipt_digest(),
                state_root: verified.state_root(),
                freshness: verified.freshness(),
            },
            verified.sequencer_public_key(),
        ))
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

    /// Serves one verified balance read through the connected reader.
    ///
    /// # Errors
    ///
    /// Propagates every reader refusal unchanged.
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
        batch_identity: value["batch_identity"].clone(),
        maintenance: maintenance_proof(&value["batch_identity"])?,
    })
}

fn head_identity(
    receipt: &[u8],
    evidence: &BatchEvidence,
) -> Result<([u8; 32], [u8; 32]), ProtocolAdapterError> {
    if receipt.starts_with(b"LXP/programs/occupancy-receipt/v2\0") {
        let maintenance = evidence
            .maintenance
            .as_ref()
            .ok_or(ProtocolAdapterError::NonCanonicalView)?;
        if maintenance.receipt != receipt || maintenance.receipt_proof != evidence.receipt_proof {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        let header = layerx_wire::receipt::decode_batch_header(&evidence.header)
            .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        let last_activity = header
            .last_sequence()
            .checked_sub(1)
            .ok_or(ProtocolAdapterError::NonCanonicalView)?;
        let batch = layerx_wire::hash::program_execution_batch_id(
            header.previous_state_root(),
            header.activity_merkle_root(),
            header.first_sequence(),
            last_activity,
            header.batch_number(),
        )
        .map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
        return Ok((batch, digest(receipt)));
    }
    let decoded = decode_receipt(receipt).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    let protocol = decoded
        .protocol()
        .ok_or(ProtocolAdapterError::NonCanonicalView)?;
    let unsigned = encode_unsigned(&decoded).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    Ok((
        protocol.batch_id(),
        receipt_digest(&unsigned).map_err(|_| ProtocolAdapterError::NonCanonicalView)?,
    ))
}

fn maintenance_proof(
    value: &Value,
) -> Result<Option<ProtocolHeadMaintenanceProof>, ProtocolAdapterError> {
    if value.is_null() {
        return Ok(None);
    }
    if field(value, "kind")? != "occupancy_maintenance_v2" {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let items = value["activity_receipts_hex"]
        .as_array()
        .ok_or(ProtocolAdapterError::NonCanonicalView)?;
    if items.is_empty() || items.len() > 64 {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let mut remaining = 16 * 1024 * 1024;
    let mut decode = |encoded: &str| {
        if encoded.len() % 2 != 0 || encoded.len() / 2 > remaining {
            return Err(ProtocolAdapterError::NonCanonicalView);
        }
        remaining -= encoded.len() / 2;
        hex::decode(encoded).map_err(|_| ProtocolAdapterError::NonCanonicalView)
    };
    let receipt = decode(field(value, "receipt_hex")?)?;
    let mut activity_receipts = Vec::with_capacity(items.len());
    for item in items {
        activity_receipts.push(decode(
            item.as_str()
                .ok_or(ProtocolAdapterError::NonCanonicalView)?,
        )?);
    }
    let encoded = field(value, "receipt_proof_hex")?;
    if encoded.len() > 2 * 1_034 {
        return Err(ProtocolAdapterError::NonCanonicalView);
    }
    let bytes = hex::decode(encoded).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    let receipt_proof = decode_proof(&bytes).map_err(|_| ProtocolAdapterError::NonCanonicalView)?;
    Ok(Some(ProtocolHeadMaintenanceProof {
        receipt,
        receipt_proof,
        activity_receipts,
    }))
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
