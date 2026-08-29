use std::net::IpAddr;
use std::time::Duration;

use layerx_crypto::ed25519;
use layerx_types::intent::{
    CapabilityRequest, ProgramCallFailure, ProgramCallOutcome, ProgramLegacyValue,
};
use serde_json::{json, Map, Value};
use sha2::{Digest as _, Sha256};
use url::{Host, Url};
use zeroize::Zeroizing;

use crate::production::SecretBytes;

use super::{
    verify_program_evidence, AgentErrorClass, ProgramCallRequest, ProgramExecutionEvidence,
    ProgramLifecycle, ProgramOperationError, ProgramServiceError, ProgramSimulationEvidence,
    ProgramSource, ProgramSubmission, ProgramTransport, Retriability, VerifiedProgramDiscovery,
    VerifiedProgramInterface, VerifiedProgramSimulation, MAX_SIGNED_ACTIVITY_BYTES,
};

const MAX_HTTP_REQUEST_BYTES: usize = 4 * 1_048_576 + 4096;
const MAX_HTTP_RESPONSE_BYTES: u64 = 9 * 1_048_576;
const MAX_INTERFACE_BYTES: usize = 952;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_REASON_BYTES: usize = 128;
const REQUESTED_VERIFICATION: &str = "sequencer-signed";
const EXECUTION_VERIFICATION: &str = "receipt-terminal-and-call-graph-verified";
const SIMULATION_EVIDENCE_DOMAIN: &[u8] = b"LayerX/agent/program-simulation-evidence/v1\0";
const SIMULATION_BOUNDARY_DOMAIN: &[u8] = b"LayerX/emulator/simulation-boundary/v1\0";

pub struct LayerXKeyCredential {
    key_id: String,
    secret: SecretBytes,
}

impl LayerXKeyCredential {
    /// Creates a redacted hosted-gateway credential.
    ///
    /// # Errors
    ///
    /// Refuses a key identifier outside the exact gateway identifier grammar.
    pub fn new(key_id: impl Into<String>, secret: SecretBytes) -> Result<Self, ProgramOperationError> {
        let key_id = key_id.into();
        if key_id.is_empty()
            || key_id.len() > 64
            || !key_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ProgramOperationError::Authentication);
        }
        Ok(Self { key_id, secret })
    }

    fn authorization(&self) -> Result<Zeroizing<String>, ProgramOperationError> {
        self.secret.expose_to(|bytes| {
            let secret = std::str::from_utf8(bytes)
                .map_err(|_| ProgramOperationError::Authentication)?;
            let suffix = secret
                .strip_prefix("lxp_live_")
                .ok_or(ProgramOperationError::Authentication)?;
            if suffix.len() != 64 || !suffix.bytes().all(canonical_hex_byte) {
                return Err(ProgramOperationError::Authentication);
            }
            Ok(Zeroizing::new(format!(
                "LayerX-Key {}:{secret}",
                self.key_id
            )))
        })
    }
}

impl std::fmt::Debug for LayerXKeyCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("LayerXKeyCredential([REDACTED])")
    }
}

pub struct HttpProgramTransport {
    agent: ureq::Agent,
    endpoint: Url,
    credential: Option<LayerXKeyCredential>,
}

impl HttpProgramTransport {
    /// Connects the exact hosted/emulator Programs route set.
    ///
    /// # Errors
    ///
    /// Refuses credentials embedded in URLs, query/fragment components,
    /// unsupported schemes, and plaintext endpoints outside loopback.
    pub fn connect(
        endpoint: &str,
        credential: Option<LayerXKeyCredential>,
    ) -> Result<Self, ProgramOperationError> {
        let endpoint = validate_endpoint(endpoint)?;
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        Ok(Self {
            agent: config.into(),
            endpoint,
            credential,
        })
    }

    fn endpoint(&self, route: &str) -> Result<Url, ProgramOperationError> {
        let mut endpoint = self.endpoint.clone();
        let base = endpoint.path().trim_end_matches('/');
        endpoint.set_path(&format!("{base}{route}"));
        endpoint.set_query(None);
        endpoint.set_fragment(None);
        validate_endpoint(endpoint.as_str())
    }

    fn dispatch(
        &self,
        method: Method,
        route: &str,
        body: &Value,
        idempotency_key: Option<[u8; 32]>,
    ) -> Result<Value, ProgramOperationError> {
        let endpoint = self.endpoint(route)?;
        let encoded = serde_json::to_vec(body).map_err(|_| ProgramOperationError::Decode)?;
        if encoded.is_empty() || encoded.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(ProgramOperationError::Bounds);
        }
        let authorization = self
            .credential
            .as_ref()
            .map(LayerXKeyCredential::authorization)
            .transpose()?;
        let response = match method {
            Method::Get => {
                if idempotency_key.is_some() {
                    return Err(ProgramOperationError::IdentityMismatch);
                }
                let mut request = self
                    .agent
                    .get(endpoint.as_str())
                    .header("Accept", "application/json")
                    .header("Content-Type", "application/json")
                    .header(
                        "User-Agent",
                        concat!("layerx-rust/", env!("CARGO_PKG_VERSION")),
                    );
                if let Some(value) = authorization.as_deref() {
                    request = request.header("Authorization", value);
                }
                request.force_send_body().send(encoded.as_slice())
            }
            Method::Post => {
                let mut request = self
                    .agent
                    .post(endpoint.as_str())
                    .header("Accept", "application/json")
                    .header("Content-Type", "application/json")
                    .header(
                        "User-Agent",
                        concat!("layerx-rust/", env!("CARGO_PKG_VERSION")),
                    );
                if let Some(value) = authorization.as_deref() {
                    request = request.header("Authorization", value);
                }
                if let Some(key) = idempotency_key {
                    request = request.header("Idempotency-Key", hex(&key));
                }
                request.send(encoded.as_slice())
            }
        }
        .map_err(|_| ProgramOperationError::Transport)?;
        decode_agent_response(response)
    }
}

impl ProgramTransport for HttpProgramTransport {
    fn discover(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramDiscovery, ProgramOperationError> {
        let program_id = hex(&program);
        let value = self.dispatch(
            Method::Get,
            &format!("/v1/programs/registry/{program_id}"),
            &json!({
                "program_id": program_id,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_discovery(&value, program)
    }

    fn interface(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramInterface, ProgramOperationError> {
        let program_id = hex(&program);
        let value = self.dispatch(
            Method::Get,
            &format!("/v1/programs/registry/{program_id}/interface"),
            &json!({
                "program_id": program_id,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_interface(&value, program)
    }

    fn simulate(
        &self,
        request: &ProgramCallRequest,
    ) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
        let value = self.dispatch(
            Method::Post,
            "/v1/programs/simulate",
            &wire_call(request),
            None,
        )?;
        decode_simulation(&value, request)
    }

    fn submit(
        &self,
        request: &ProgramCallRequest,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        if idempotency_key != request.bound_idempotency_key() {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        let attempt = self
            .dispatch(
                Method::Post,
                "/v1/programs/call",
                &wire_call(request),
                Some(idempotency_key),
            )
            .and_then(|value| {
                decode_submission(
                    &value,
                    SubmissionExpectation {
                        program_id: Some(request.call().callee().bytes()),
                        activity_id: Some(request.bound_activity_id()),
                        idempotency_key: Some(idempotency_key),
                        retained_signed_activity: Some(request.signed_activity()),
                    },
                )
        });
        match attempt {
            Ok(submission) => Ok(submission),
            Err(ProgramOperationError::Authentication) => {
                Err(ProgramOperationError::Authentication)
            }
            Err(ProgramOperationError::Bounds) => Err(ProgramOperationError::Bounds),
            Err(ProgramOperationError::Service(error))
                if error.retriability == Retriability::Terminal =>
            {
                Err(ProgramOperationError::Service(error))
            }
            Err(_) => Ok(ProgramSubmission::Unknown {
                activity_id: request.bound_activity_id(),
                idempotency_key,
                retained_signed_activity: Some(request.signed_activity().to_vec()),
            }),
        }
    }

    fn receipt(
        &self,
        idempotency_key: [u8; 32],
        expected_activity: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        let idempotency = hex(&idempotency_key);
        let activity = hex(&expected_activity);
        let value = self.dispatch(
            Method::Get,
            &format!("/v1/programs/receipts/by-idempotency/{idempotency}"),
            &json!({
                "idempotency_key": idempotency,
                "expected_activity_id": activity,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_submission(
            &value,
            SubmissionExpectation {
                program_id: None,
                activity_id: Some(expected_activity),
                idempotency_key: Some(idempotency_key),
                retained_signed_activity: None,
            },
        )
    }

    fn activity(
        &self,
        activity_id: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        let activity = hex(&activity_id);
        let value = self.dispatch(
            Method::Get,
            &format!("/v1/programs/activities/{activity}"),
            &json!({
                "activity_id": activity,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_submission(
            &value,
            SubmissionExpectation {
                program_id: None,
                activity_id: Some(activity_id),
                idempotency_key: None,
                retained_signed_activity: None,
            },
        )
    }
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecutionState {
    Executed,
    Refused,
    Simulated,
}

struct DecodedExecution {
    state: ExecutionState,
    activity_id: [u8; 32],
    program_id: [u8; 32],
    idempotency_key: Option<[u8; 32]>,
    authority: layerx_proof::receipt::AuthorizedBatch,
    verified: layerx_proof::program::VerifiedProgramExecution,
}

struct SubmissionExpectation<'a> {
    program_id: Option<[u8; 32]>,
    activity_id: Option<[u8; 32]>,
    idempotency_key: Option<[u8; 32]>,
    retained_signed_activity: Option<&'a [u8]>,
}

fn validate_endpoint(value: &str) -> Result<Url, ProgramOperationError> {
    let endpoint = Url::parse(value).map_err(|_| ProgramOperationError::InvalidEndpoint)?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || (endpoint.scheme() == "http" && !loopback(&endpoint))
    {
        return Err(ProgramOperationError::InvalidEndpoint);
    }
    Ok(endpoint)
}

fn loopback(endpoint: &Url) -> bool {
    match endpoint.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

fn decode_agent_response(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<Value, ProgramOperationError> {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if content_type
        .split(';')
        .next()
        .map(str::trim)
        != Some("application/json")
    {
        return Err(ProgramOperationError::Decode);
    }
    let encoded = response
        .body_mut()
        .with_config()
        .limit(MAX_HTTP_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|_| ProgramOperationError::Decode)?;
    let document: Value =
        serde_json::from_slice(&encoded).map_err(|_| ProgramOperationError::Decode)?;
    let envelope = object(&document)?;
    if envelope.contains_key("class") {
        return Err(ProgramOperationError::Service(decode_service_error(
            status, envelope,
        )?));
    }
    if !(200..300).contains(&status)
        || !valid_request_id(required_string(envelope, "request_id")?)
        || !achieved_sequencer(envelope.get("verification_status"))
    {
        return Err(ProgramOperationError::Decode);
    }
    envelope
        .get("value")
        .cloned()
        .ok_or(ProgramOperationError::Decode)
}

fn decode_service_error(
    status: u16,
    value: &Map<String, Value>,
) -> Result<ProgramServiceError, ProgramOperationError> {
    if (200..300).contains(&status) {
        return Err(ProgramOperationError::Decode);
    }
    let request_id = required_string(value, "request_id")?;
    let reason = required_string(value, "reason")?;
    if !valid_request_id(request_id)
        || reason.is_empty()
        || reason.len() > MAX_REASON_BYTES
        || !reason
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.'))
    {
        return Err(ProgramOperationError::Decode);
    }
    let class = match required_string(value, "class")? {
        "TransportFailure" => AgentErrorClass::TransportFailure,
        "Deadline" => AgentErrorClass::Deadline,
        "ProtocolIncompatibility" => AgentErrorClass::ProtocolIncompatibility,
        "UnavailableCapability" => AgentErrorClass::UnavailableCapability,
        "CoreRejection" => AgentErrorClass::CoreRejection,
        "VerificationFailure" => AgentErrorClass::VerificationFailure,
        "PolicyRefusal" => AgentErrorClass::PolicyRefusal,
        "CapabilityRefusal" => AgentErrorClass::CapabilityRefusal,
        "BudgetRefusal" => AgentErrorClass::BudgetRefusal,
        "RateLimit" => AgentErrorClass::RateLimit,
        "IdempotencyConflict" => AgentErrorClass::IdempotencyConflict,
        "InternalFault" => AgentErrorClass::InternalFault,
        _ => return Err(ProgramOperationError::Decode),
    };
    let retriability = match required_string(value, "retriability")? {
        "Terminal" => Retriability::Terminal,
        "Retriable" => Retriability::Retriable,
        _ => return Err(ProgramOperationError::Decode),
    };
    let protocol_result_code = match value.get("protocol_result_code") {
        Some(Value::Null) => None,
        Some(Value::Number(number)) => number
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .map(layerx_types::result::ResultCode::from_raw),
        _ => return Err(ProgramOperationError::Decode),
    };
    if value.get("protocol_result_code") != Some(&Value::Null)
        && protocol_result_code.is_none()
    {
        return Err(ProgramOperationError::Decode);
    }
    Ok(ProgramServiceError {
        class,
        retriability,
        request_id: request_id.to_owned(),
        reason: reason.to_owned(),
        protocol_result_code,
    })
}

fn achieved_sequencer(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_object)
        .is_some_and(|status| {
            status.get("state").and_then(Value::as_str) == Some("Achieved")
                && status.get("level").and_then(Value::as_str) == Some("SequencerSigned")
        })
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REQUEST_ID_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn decode_discovery(
    value: &Value,
    expected_program: [u8; 32],
) -> Result<VerifiedProgramDiscovery, ProgramOperationError> {
    let value = object(value)?;
    if fixed(value, "program_id")? != expected_program
        || required_string(value, "verification")?
            != "registry-receipt-and-current-head-verified"
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let lifecycle = match required_string(value, "lifecycle")? {
        "active" => ProgramLifecycle::Active,
        "deprecated" => ProgramLifecycle::Deprecated,
        "tombstoned" => ProgramLifecycle::Tombstoned,
        _ => return Err(ProgramOperationError::Decode),
    };
    let version = bounded_u32(value, "version", 1, u32::MAX)?;
    let abi_version = bounded_u16(value, "abi_version", 1, 2)?;
    let observed_sequence = decimal_u64(value, "observed_sequence")?;
    let observed_at = decimal_u64(value, "observed_at")?;
    let valid_through = decimal_u64(value, "valid_through")?;
    if valid_through < observed_at {
        return Err(ProgramOperationError::Verification);
    }
    Ok(VerifiedProgramDiscovery {
        program_id: expected_program,
        lifecycle,
        version,
        code_hash: fixed(value, "code_hash")?,
        abi_version,
        receipt_digest: fixed(value, "receipt_digest")?,
        state_root: fixed(value, "state_root")?,
        observed_sequence,
        observed_at,
        valid_through,
    })
}

fn decode_interface(
    value: &Value,
    expected_program: [u8; 32],
) -> Result<VerifiedProgramInterface, ProgramOperationError> {
    let value = object(value)?;
    if fixed(value, "program_id")? != expected_program
        || required_string(value, "verification")?
            != "deployment-interface-and-current-head-verified"
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let interface = bounded_hex(value, "interface", MAX_INTERFACE_BYTES, None)?;
    if interface.is_empty() {
        return Err(ProgramOperationError::Bounds);
    }
    let interface_digest = fixed(value, "interface_digest")?;
    if <[u8; 32]>::from(Sha256::digest(&interface)) != interface_digest {
        return Err(ProgramOperationError::Verification);
    }
    let observed_at = decimal_u64(value, "observed_at")?;
    let valid_through = decimal_u64(value, "valid_through")?;
    if valid_through < observed_at {
        return Err(ProgramOperationError::Verification);
    }
    Ok(VerifiedProgramInterface {
        program_id: expected_program,
        version: bounded_u32(value, "version", 1, u32::MAX)?,
        code_hash: fixed(value, "code_hash")?,
        abi_version: bounded_u16(value, "abi_version", 1, 2)?,
        interface,
        interface_digest,
        receipt_digest: fixed(value, "receipt_digest")?,
        state_root: fixed(value, "state_root")?,
        observed_sequence: decimal_u64(value, "observed_sequence")?,
        observed_at,
        valid_through,
        source: decode_source(value.get("source"))?,
    })
}

fn decode_source(value: Option<&Value>) -> Result<ProgramSource, ProgramOperationError> {
    let value = value
        .and_then(Value::as_object)
        .ok_or(ProgramOperationError::Decode)?;
    match required_string(value, "status")? {
        "unpublished" => Ok(ProgramSource::Unpublished),
        "verified" => Ok(ProgramSource::Verified {
            source_digest: fixed(value, "source_digest")?,
            environment_digest: fixed(value, "environment_digest")?,
        }),
        "mismatch" => Ok(ProgramSource::Mismatch {
            expected_code_hash: fixed(value, "expected_code_hash")?,
            reproduced_artifact_digest: fixed(value, "reproduced_artifact_digest")?,
        }),
        _ => Err(ProgramOperationError::Decode),
    }
}

fn decode_simulation(
    value: &Value,
    request: &ProgramCallRequest,
) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
    let value = object(value)?;
    if value.get("committed").and_then(Value::as_bool) != Some(false) {
        return Err(ProgramOperationError::Verification);
    }
    let decoded = decode_execution(
        value.get("execution").ok_or(ProgramOperationError::Decode)?,
        Some(ExecutionState::Simulated),
    )?;
    if decoded.activity_id != request.bound_activity_id()
        || decoded.program_id != request.call().callee().bytes()
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let evidence = decode_simulation_evidence(
        value
            .get("simulation_evidence")
            .ok_or(ProgramOperationError::Decode)?,
    )?;
    let public_key = decoded.authority.sequencer_public_key();
    let expected_boundary: [u8; 32] = Sha256::digest(
        [SIMULATION_BOUNDARY_DOMAIN, public_key.as_slice()].concat(),
    )
    .into();
    let protocol = decoded
        .verified
        .receipt()
        .receipt()
        .protocol()
        .ok_or(ProgramOperationError::Verification)?;
    if evidence.boundary_id != expected_boundary
        || evidence.public_key != public_key
        || evidence.activity_id != decoded.activity_id
        || evidence.previous_state_root != decoded.authority.previous_state_root()
        || evidence.hypothetical_state_root != decoded.authority.resulting_state_root()
        || evidence.observed_sequence.checked_add(1) != Some(protocol.global_sequence())
    {
        return Err(ProgramOperationError::Verification);
    }
    let mut signed = Vec::with_capacity(SIMULATION_EVIDENCE_DOMAIN.len() + 137);
    signed.extend_from_slice(SIMULATION_EVIDENCE_DOMAIN);
    signed.extend_from_slice(&evidence.boundary_id);
    signed.extend_from_slice(&evidence.activity_id);
    signed.extend_from_slice(&evidence.previous_state_root);
    signed.extend_from_slice(&evidence.hypothetical_state_root);
    signed.extend_from_slice(&evidence.observed_sequence.to_be_bytes());
    signed.extend_from_slice(&evidence.observed_at.to_be_bytes());
    signed.push(0);
    let digest: [u8; 32] = Sha256::digest(signed).into();
    ed25519::verify_digest(&public_key, &evidence.signature, &digest)
        .map_err(|_| ProgramOperationError::Verification)?;
    Ok(VerifiedProgramSimulation {
        execution: decoded.verified,
        evidence,
    })
}

fn decode_simulation_evidence(
    value: &Value,
) -> Result<ProgramSimulationEvidence, ProgramOperationError> {
    let value = object(value)?;
    if value.get("committed").and_then(Value::as_bool) != Some(false) {
        return Err(ProgramOperationError::Verification);
    }
    Ok(ProgramSimulationEvidence {
        boundary_id: fixed(value, "boundary_id")?,
        activity_id: fixed(value, "activity_id")?,
        previous_state_root: fixed(value, "previous_state_root")?,
        hypothetical_state_root: fixed(value, "hypothetical_state_root")?,
        observed_sequence: decimal_u64(value, "observed_sequence")?,
        observed_at: decimal_u64(value, "observed_at")?,
        public_key: fixed(value, "public_key")?,
        signature: fixed_n(value, "signature")?,
    })
}

fn decode_submission(
    value: &Value,
    expected: SubmissionExpectation<'_>,
) -> Result<ProgramSubmission, ProgramOperationError> {
    let object = object(value)?;
    if object.get("state").and_then(Value::as_str) == Some("unknown") {
        let activity_id = fixed(object, "activity_id")?;
        let idempotency_key = fixed(object, "idempotency_key")?;
        let retained = object
            .get("retained_signed_activity")
            .map(|_| {
                bounded_hex(
                    object,
                    "retained_signed_activity",
                    MAX_SIGNED_ACTIVITY_BYTES,
                    None,
                )
            })
            .transpose()?;
        if expected.activity_id.is_some_and(|value| value != activity_id)
            || expected
                .idempotency_key
                .is_some_and(|value| value != idempotency_key)
            || expected.retained_signed_activity.is_some_and(|expected| {
                retained.as_deref() != Some(expected)
            })
        {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        return Ok(ProgramSubmission::Unknown {
            activity_id,
            idempotency_key,
            retained_signed_activity: retained,
        });
    }
    let decoded = decode_execution(value, None)?;
    if !matches!(decoded.state, ExecutionState::Executed | ExecutionState::Refused)
        || expected
            .program_id
            .is_some_and(|value| value != decoded.program_id)
        || expected
            .activity_id
            .is_some_and(|value| value != decoded.activity_id)
        || expected
            .idempotency_key
            .is_some_and(|value| Some(value) != decoded.idempotency_key)
        || decoded.idempotency_key.is_none()
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    match decoded.state {
        ExecutionState::Executed => Ok(ProgramSubmission::Executed(decoded.verified)),
        ExecutionState::Refused => Ok(ProgramSubmission::Refused(decoded.verified)),
        ExecutionState::Simulated => Err(ProgramOperationError::Decode),
    }
}

fn decode_execution(
    value: &Value,
    expected_state: Option<ExecutionState>,
) -> Result<DecodedExecution, ProgramOperationError> {
    let value = object(value)?;
    let state = match required_string(value, "state")? {
        "executed" => ExecutionState::Executed,
        "refused" => ExecutionState::Refused,
        "simulated" => ExecutionState::Simulated,
        _ => return Err(ProgramOperationError::Decode),
    };
    if expected_state.is_some_and(|expected| expected != state)
        || required_string(value, "verification")? != EXECUTION_VERIFICATION
    {
        return Err(ProgramOperationError::Verification);
    }
    let activity_id = fixed(value, "activity_id")?;
    let program_id = fixed(value, "program_id")?;
    let guest_abi_version = bounded_u16(value, "guest_abi_version", 1, 2)?;
    let module_version = bounded_u32(value, "module_version", 1, 3)?;
    let batch_id = fixed(value, "batch_id")?;
    let global_sequence = decimal_u64(value, "global_sequence")?;
    let result_code = exact_i32(value, "result_code")?;
    let state_root = fixed(value, "state_root")?;
    let receipt_digest = fixed(value, "receipt_digest")?;
    let receipt = bounded_hex(value, "receipt", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    let terminal_payload = bounded_hex(
        value,
        "terminal_payload",
        MAX_SIGNED_ACTIVITY_BYTES,
        None,
    )?;
    let call_graph = bounded_hex(value, "call_graph", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    let authority_value = value
        .get("authority")
        .and_then(Value::as_object)
        .ok_or(ProgramOperationError::Decode)?;
    let authority = layerx_proof::receipt::AuthorizedBatch::new(
        fixed(authority_value, "batch_id")?,
        fixed(authority_value, "asset")?,
        fixed(authority_value, "previous_state_root")?,
        fixed(authority_value, "resulting_state_root")?,
        fixed(authority_value, "sequencer_public_key")?,
    );
    if authority.batch_id() != batch_id || authority.resulting_state_root() != state_root {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let usage = value
        .get("usage")
        .and_then(Value::as_object)
        .ok_or(ProgramOperationError::Decode)?;
    let cpu_fuel = decimal_u64(usage, "cpu_fuel")?;
    let memory_bytes = decimal_u64(usage, "memory_bytes")?;
    let storage_read_bytes = decimal_u64(usage, "storage_read_bytes")?;
    let storage_write_bytes = decimal_u64(usage, "storage_write_bytes")?;
    let output_values = bounded_u32(usage, "output_values", 0, u32::MAX)?;
    let output_bytes = decimal_u64(usage, "output_bytes")?;
    let fee_units = decimal_u128(usage, "fee_units")?;
    let outcome = value.get("outcome").ok_or(ProgramOperationError::Decode)?;
    let evidence = ProgramExecutionEvidence {
        receipt,
        terminal_payload,
        call_graph,
        authority,
        activity_id,
        program_id,
        guest_abi_version,
    };
    let verified = verify_program_evidence(&evidence)?;
    let protocol = verified
        .receipt()
        .receipt()
        .protocol()
        .ok_or(ProgramOperationError::Verification)?;
    let verified_digest = verified
        .receipt()
        .evidence()
        .receipt_digest()
        .ok_or(ProgramOperationError::Verification)?;
    if protocol.module_version() != module_version
        || protocol.batch_id() != batch_id
        || protocol.global_sequence() != global_sequence
        || protocol.result_code() != result_code
        || protocol.resulting_state_root() != state_root
        || verified_digest != receipt_digest
        || verified.result_code() != result_code
        || verified.cpu_fuel() != cpu_fuel
        || verified.memory_bytes() != memory_bytes
        || verified.storage_read_bytes() != storage_read_bytes
        || verified.storage_write_bytes() != storage_write_bytes
        || verified.output_values() != output_values
        || verified.output_bytes() != output_bytes
        || verified.fee_units() != fee_units
        || expected_outcome(verified.outcome()) != *outcome
        || (state == ExecutionState::Refused && verified.outcome().is_completed())
        || (state == ExecutionState::Executed && !verified.outcome().is_completed())
    {
        return Err(ProgramOperationError::Verification);
    }
    let idempotency_key = value
        .get("idempotency_key")
        .map(|_| fixed(value, "idempotency_key"))
        .transpose()?;
    Ok(DecodedExecution {
        state,
        activity_id,
        program_id,
        idempotency_key,
        authority,
        verified,
    })
}

fn expected_outcome(outcome: &ProgramCallOutcome) -> Value {
    match outcome {
        ProgramCallOutcome::Completed(response) => json!({
            "kind": "completed",
            "code": response.code(),
            "response": hex(response.body()),
        }),
        ProgramCallOutcome::LegacyCompleted(response) => json!({
            "kind": "legacy_completed",
            "code": response.code(),
            "values": response.values().iter().map(|value| match value {
                ProgramLegacyValue::I32(value) => json!({"type":"i32","value":value}),
                ProgramLegacyValue::I64(value) => json!({"type":"i64","value":value.to_string()}),
            }).collect::<Vec<_>>(),
        }),
        ProgramCallOutcome::Refused(failure) => json!({
            "kind": "refused",
            "failure": expected_failure(*failure),
        }),
    }
}

fn expected_failure(failure: ProgramCallFailure) -> Value {
    match failure {
        ProgramCallFailure::UnknownProgram => json!({"kind":"unknown_program"}),
        ProgramCallFailure::Reentrancy => json!({"kind":"reentrancy"}),
        ProgramCallFailure::DepthExceeded { limit, attempted } => {
            json!({"kind":"depth_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::FanoutExceeded { limit, attempted } => {
            json!({"kind":"fanout_exceeded","limit":limit,"attempted":attempted})
        }
        ProgramCallFailure::GuestRefused { code } => {
            json!({"kind":"guest_refused","code":code})
        }
        ProgramCallFailure::Authority => json!({"kind":"authority"}),
        ProgramCallFailure::Resource => json!({"kind":"resource"}),
        ProgramCallFailure::Response => json!({"kind":"response"}),
        ProgramCallFailure::Fault => json!({"kind":"fault"}),
    }
}

fn wire_call(request: &ProgramCallRequest) -> Value {
    let call = request.call();
    json!({
        "program_id": hex(&call.callee().bytes()),
        "calldata": hex(call.calldata().as_bytes()),
        "budget": {
            "fuel": call.budget().fuel().to_string(),
            "fee_limit": call.budget().fee_limit().value().to_string(),
        },
        "capabilities": call.capabilities().as_slice().iter().map(|capability| match capability {
            CapabilityRequest::StorageRead => "storage_read",
            CapabilityRequest::StorageWrite => "storage_write",
            CapabilityRequest::Transfer => "transfer",
            CapabilityRequest::EmitEvent => "emit_event",
            CapabilityRequest::Compose => "compose",
        }).collect::<Vec<_>>(),
        "signed_activity": hex(request.signed_activity()),
    })
}

fn object(value: &Value) -> Result<&Map<String, Value>, ProgramOperationError> {
    value.as_object().ok_or(ProgramOperationError::Decode)
}

fn required_string<'a>(
    value: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, ProgramOperationError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(ProgramOperationError::Decode)
}

fn fixed(
    value: &Map<String, Value>,
    field: &str,
) -> Result<[u8; 32], ProgramOperationError> {
    fixed_n(value, field)
}

fn fixed_n<const N: usize>(
    value: &Map<String, Value>,
    field: &str,
) -> Result<[u8; N], ProgramOperationError> {
    bounded_hex(value, field, N, Some(N))?
        .try_into()
        .map_err(|_| ProgramOperationError::Decode)
}

fn bounded_hex(
    value: &Map<String, Value>,
    field: &str,
    maximum: usize,
    exact: Option<usize>,
) -> Result<Vec<u8>, ProgramOperationError> {
    let text = required_string(value, field)?;
    if text.len() % 2 != 0
        || text.len() > maximum.saturating_mul(2)
        || exact.is_some_and(|length| text.len() != length.saturating_mul(2))
        || !text.bytes().all(canonical_hex_byte)
    {
        return Err(ProgramOperationError::Bounds);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or(ProgramOperationError::Decode)?;
            let low = hex_nibble(pair[1]).ok_or(ProgramOperationError::Decode)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn canonical_hex_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn decimal_u64(
    value: &Map<String, Value>,
    field: &str,
) -> Result<u64, ProgramOperationError> {
    canonical_decimal(value, field)?.parse().map_err(|_| ProgramOperationError::Bounds)
}

fn decimal_u128(
    value: &Map<String, Value>,
    field: &str,
) -> Result<u128, ProgramOperationError> {
    canonical_decimal(value, field)?.parse().map_err(|_| ProgramOperationError::Bounds)
}

fn canonical_decimal<'a>(
    value: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, ProgramOperationError> {
    let text = required_string(value, field)?;
    if text.is_empty()
        || !text.bytes().all(|byte| byte.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(ProgramOperationError::Decode);
    }
    Ok(text)
}

fn bounded_u32(
    value: &Map<String, Value>,
    field: &str,
    minimum: u32,
    maximum: u32,
) -> Result<u32, ProgramOperationError> {
    let parsed = value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or(ProgramOperationError::Decode)?;
    if parsed < minimum || parsed > maximum {
        return Err(ProgramOperationError::Bounds);
    }
    Ok(parsed)
}

fn bounded_u16(
    value: &Map<String, Value>,
    field: &str,
    minimum: u16,
    maximum: u16,
) -> Result<u16, ProgramOperationError> {
    let parsed = value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .ok_or(ProgramOperationError::Decode)?;
    if parsed < minimum || parsed > maximum {
        return Err(ProgramOperationError::Bounds);
    }
    Ok(parsed)
}

fn exact_i32(
    value: &Map<String, Value>,
    field: &str,
) -> Result<i32, ProgramOperationError> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(ProgramOperationError::Decode)
}
