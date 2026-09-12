use std::net::IpAddr;
use std::time::Duration;

#[cfg(test)]
#[path = "../../../tests/support/wall_clock.rs"]
mod wall_clock;

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
    verify_program_evidence, AgentErrorClass, BoundProgramRequest, NativeProgramCallRequest,
    NativeProgramTransport, ProgramCallRequest, ProgramExecutionEvidence, ProgramLifecycle,
    ProgramOperationError, ProgramServiceError, ProgramSimulationEvidence, ProgramSource,
    ProgramSubmission, ProgramTransport, Retriability, VerifiedProgramDiscovery,
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
    pub fn new(
        key_id: impl Into<String>,
        secret: SecretBytes,
    ) -> Result<Self, ProgramOperationError> {
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

    pub(crate) fn authorization(&self) -> Result<Zeroizing<String>, ProgramOperationError> {
        self.secret.expose_to(|bytes| {
            let secret =
                std::str::from_utf8(bytes).map_err(|_| ProgramOperationError::Authentication)?;
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
    clock: fn() -> Result<u64, ProgramOperationError>,
    agent: ureq::Agent,
    endpoint: Url,
    credential: Option<LayerXKeyCredential>,
    trusted_sequencer_public_key: [u8; 32],
}

impl HttpProgramTransport {
    fn submit_lifecycle(
        &self,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
        key: [u8; 32],
        ordinal: u16,
        operation: &'static str,
        route: &str,
    ) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
        if request.ordinal() != ordinal || request.bound_idempotency_key() != key {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        self.endpoint(route)?;
        if let Some(credential) = &self.credential {
            let _ = credential.authorization()?;
        }
        let attempt = self
            .dispatch(
                operation,
                Method::Post,
                route,
                &json!({"signed_activity": hex(request.signed_activity())}),
                Some(key),
            )
            .and_then(|value| {
                Self::decode_lifecycle_submission(
                    &value,
                    request,
                    self.trusted_sequencer_public_key,
                )
            });
        resolve_lifecycle_submission(request, attempt)
    }

    fn decode_lifecycle_submission(
        value: &Value,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
        sequencer: [u8; 32],
    ) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
        use crate::program_lifecycle::ProgramLifecycleSubmission;
        let value = object(value)?;
        if value.get("state").and_then(Value::as_str) == Some("unknown") {
            if !exact_fields(
                value,
                &["state", "activity_id", "retry", "retry_after_seconds"],
            ) || fixed(value, "activity_id")? != request.bound_activity_id()
                || required_string(value, "retry")? != "after"
                || value.get("retry_after_seconds").and_then(Value::as_u64) != Some(2)
            {
                return Err(ProgramOperationError::IdentityMismatch);
            }
            return Ok(ProgramLifecycleSubmission::Unknown {
                activity_id: request.bound_activity_id(),
                idempotency_key: request.bound_idempotency_key(),
                retained_signed_activity: request.signed_activity().to_vec(),
            });
        }
        if !exact_fields(
            value,
            &[
                "state",
                "activity_id",
                "receipt",
                "terminal_payload",
                "call_graph",
            ],
        ) || fixed(value, "activity_id")? != request.bound_activity_id()
            || !required_string(value, "terminal_payload")?.is_empty()
            || !required_string(value, "call_graph")?.is_empty()
        {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        let receipt = bounded_hex(value, "receipt", MAX_SIGNED_ACTIVITY_BYTES, None)?;
        if receipt.is_empty() {
            return Err(ProgramOperationError::Decode);
        }
        let verified = crate::program_lifecycle::verify_lifecycle_receipt(
            &receipt,
            request.bound_activity_id(),
            sequencer,
        )?;
        let protocol = verified
            .protocol()
            .ok_or(ProgramOperationError::Verification)?;
        if required_string(value, "state")?
            != if protocol.result_code() == 0 {
                "executed"
            } else {
                "refused"
            }
        {
            return Err(ProgramOperationError::Verification);
        }
        Ok(ProgramLifecycleSubmission::Acknowledged {
            activity_id: request.bound_activity_id(),
            receipt,
            result_code: protocol.result_code(),
        })
    }
    fn submit_bound(
        &self,
        request: &impl BoundProgramRequest,
        wire: &Value,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        if idempotency_key != request.bound_idempotency_key() {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        if let Some(credential) = &self.credential {
            let _ = credential.authorization()?;
        }
        let encoded = serde_json::to_vec(wire).map_err(|_| ProgramOperationError::Decode)?;
        if encoded.is_empty() || encoded.len() > MAX_HTTP_REQUEST_BYTES {
            return Err(ProgramOperationError::Bounds);
        }
        let attempt = self
            .dispatch(
                "program.call",
                Method::Post,
                "/v1/programs/call",
                wire,
                Some(idempotency_key),
            )
            .and_then(|value| {
                decode_submission(
                    &value,
                    SubmissionExpectation {
                        program_id: Some(request.request_program()),
                        activity_id: Some(request.bound_activity_id()),
                        idempotency_key: Some(idempotency_key),
                        retained_signed_activity: Some(request.signed_activity()),
                        trusted_sequencer_public_key: self.trusted_sequencer_public_key,
                    },
                )
            });
        match attempt {
            Ok(submission) => Ok(submission),
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

    /// Connects the exact hosted/emulator Programs route set.
    ///
    /// # Errors
    ///
    /// Refuses credentials embedded in URLs, query/fragment components,
    /// unsupported schemes, and plaintext endpoints outside loopback.
    pub fn connect(
        endpoint: &str,
        credential: Option<LayerXKeyCredential>,
        trusted_sequencer_public_key: [u8; 32],
        clock: fn() -> Result<u64, ProgramOperationError>,
    ) -> Result<Self, ProgramOperationError> {
        let endpoint = validate_endpoint(endpoint)?;
        if trusted_sequencer_public_key.iter().all(|byte| *byte == 0) {
            return Err(ProgramOperationError::Authentication);
        }
        let config = ureq::Agent::config_builder()
            .tls_config(ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .root_certs(crate::tls::system_roots(endpoint.as_str())?)
                .build())
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        Ok(Self {
            clock,
            agent: config.into(),
            endpoint,
            credential,
            trusted_sequencer_public_key,
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
        operation: &'static str,
        method: Method,
        route: &str,
        body: &Value,
        idempotency_key: Option<[u8; 32]>,
    ) -> Result<Value, ProgramOperationError> {
        let endpoint = self.endpoint(route)?;
        let encoded = match method {
            Method::Post => bounded_hex(
                object(body)?,
                "signed_activity",
                MAX_SIGNED_ACTIVITY_BYTES,
                None,
            )?,
            Method::Get => serde_json::to_vec(body).map_err(|_| ProgramOperationError::Decode)?,
        };
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
                    .header("Content-Type", "application/octet-stream")
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
        decode_agent_response(response, operation)
    }
}

impl ProgramTransport for HttpProgramTransport {
    fn lifecycle_receipt(
        &self,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
    ) -> Result<layerx_wire::receipt::Receipt, ProgramOperationError> {
        let key = request.bound_idempotency_key();
        let value = self.dispatch("program.receipt", Method::Get, &format!("/v1/programs/receipts/by-idempotency/{}", hex(&key)),
            &json!({"idempotency_key":hex(&key),"expected_activity_id":hex(&request.bound_activity_id()),"requested_verification_level":"sequencer-signed"}), None)?;
        decode_lifecycle_recovery(
            &value,
            request.bound_activity_id(),
            self.trusted_sequencer_public_key,
        )
    }
    fn deploy(
        &self,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
        key: [u8; 32],
    ) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
        self.submit_lifecycle(request, key, 1, "program.deploy", "/v1/programs/deploy")
    }
    fn upgrade(
        &self,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
        key: [u8; 32],
    ) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
        self.submit_lifecycle(request, key, 2, "program.upgrade", "/v1/programs/upgrade")
    }
    fn wind_down(
        &self,
        request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
        key: [u8; 32],
    ) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
        self.submit_lifecycle(
            request,
            key,
            7,
            "program.wind-down",
            "/v1/programs/wind-down",
        )
    }
    fn discover(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramDiscovery, ProgramOperationError> {
        let program_id = hex(&program);
        let value = self.dispatch(
            "program.discover",
            Method::Get,
            &format!("/v1/programs/registry/{program_id}"),
            &json!({
                "program_id": program_id,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_discovery(&value, program, (self.clock)()?)
    }

    fn interface(
        &self,
        program: [u8; 32],
    ) -> Result<VerifiedProgramInterface, ProgramOperationError> {
        let program_id = hex(&program);
        let value = self.dispatch(
            "program.interface",
            Method::Get,
            &format!("/v1/programs/registry/{program_id}/interface"),
            &json!({
                "program_id": program_id,
                "requested_verification_level": REQUESTED_VERIFICATION,
            }),
            None,
        )?;
        decode_interface(&value, program, (self.clock)()?)
    }

    fn simulate(
        &self,
        request: &ProgramCallRequest,
    ) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
        let value = self.dispatch(
            "program.simulate",
            Method::Post,
            "/v1/programs/simulate",
            &wire_call(request),
            None,
        )?;
        decode_simulation(&value, request, self.trusted_sequencer_public_key)
    }

    fn submit(
        &self,
        request: &ProgramCallRequest,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        self.submit_bound(request, &wire_call(request), idempotency_key)
    }

    fn receipt(
        &self,
        idempotency_key: [u8; 32],
        expected_activity: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        let idempotency = hex(&idempotency_key);
        let activity = hex(&expected_activity);
        let value = self.dispatch(
            "program.receipt",
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
                trusted_sequencer_public_key: self.trusted_sequencer_public_key,
            },
        )
    }

    fn activity(&self, activity_id: [u8; 32]) -> Result<ProgramSubmission, ProgramOperationError> {
        let activity = hex(&activity_id);
        let value = self.dispatch(
            "program.activity",
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
                trusted_sequencer_public_key: self.trusted_sequencer_public_key,
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

#[derive(Clone, Copy)]
struct SubmissionExpectation<'a> {
    program_id: Option<[u8; 32]>,
    activity_id: Option<[u8; 32]>,
    idempotency_key: Option<[u8; 32]>,
    retained_signed_activity: Option<&'a [u8]>,
    trusted_sequencer_public_key: [u8; 32],
}

pub(crate) fn validate_endpoint(value: &str) -> Result<Url, ProgramOperationError> {
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
    operation: &str,
) -> Result<Value, ProgramOperationError> {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if content_type.split(';').next().map(str::trim) != Some("application/json") {
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
    decode_agent_document(status, document, operation)
}

fn decode_agent_document(
    status: u16,
    document: Value,
    operation: &str,
) -> Result<Value, ProgramOperationError> {
    let envelope = object(&document)?;
    if envelope.contains_key("class") {
        if !exact_fields(
            envelope,
            &[
                "class",
                "protocol_result_code",
                "retriability",
                "reason",
                "request_id",
            ],
        ) {
            return Err(ProgramOperationError::Decode);
        }
        return Err(ProgramOperationError::Service(decode_service_error(
            status, envelope,
        )?));
    }
    if matches!(
        operation,
        "program.deploy" | "program.upgrade" | "program.wind-down"
    ) || (operation == "program.receipt"
        && (envelope.contains_key("result") || envelope.contains_key("error")))
    {
        if envelope.contains_key("error") {
            if !(400..600).contains(&status) || !exact_fields(envelope, &["error"]) {
                return Err(ProgramOperationError::Decode);
            }
            return Err(decode_boundary_error(status, object(&envelope["error"])?)?);
        }
        if status == 202 && envelope.get("state").and_then(Value::as_str) == Some("unknown") {
            return Ok(document);
        }
        if !(200..300).contains(&status) || !exact_fields(envelope, &["result"]) {
            return Err(ProgramOperationError::Decode);
        }
        return envelope
            .get("result")
            .cloned()
            .ok_or(ProgramOperationError::Decode);
    }
    if !exact_fields(envelope, &["request_id", "value", "verification_status"]) {
        return Err(ProgramOperationError::Decode);
    }
    let value = envelope.get("value").ok_or(ProgramOperationError::Decode)?;
    if !(200..300).contains(&status) || !valid_request_id(required_string(envelope, "request_id")?)
    {
        return Err(ProgramOperationError::Decode);
    }
    if !accepted_program_verification(operation, value, envelope.get("verification_status")) {
        return Err(ProgramOperationError::Verification);
    }
    Ok(value.clone())
}

fn resolve_lifecycle_submission(
    request: &crate::program_lifecycle::NativeProgramLifecycleRequest,
    attempt: Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError>,
) -> Result<crate::program_lifecycle::ProgramLifecycleSubmission, ProgramOperationError> {
    match attempt {
        Ok(value) => Ok(value),
        Err(
            error @ (ProgramOperationError::Boundary {
                status: 400..=499, ..
            }
            | ProgramOperationError::Authentication
            | ProgramOperationError::InvalidEndpoint),
        ) => Err(error),
        Err(ProgramOperationError::Service(error))
            if error.retriability == Retriability::Terminal =>
        {
            Err(ProgramOperationError::Service(error))
        }
        Err(_) => Ok(
            crate::program_lifecycle::ProgramLifecycleSubmission::Unknown {
                activity_id: request.bound_activity_id(),
                idempotency_key: request.bound_idempotency_key(),
                retained_signed_activity: request.signed_activity().to_vec(),
            },
        ),
    }
}

fn decode_lifecycle_recovery(
    value: &Value,
    expected_activity: [u8; 32],
    sequencer: [u8; 32],
) -> Result<layerx_wire::receipt::Receipt, ProgramOperationError> {
    let value = object(value)?;
    if !exact_fields(value, &["activity_id", "receipt"])
        || fixed(value, "activity_id")? != expected_activity
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let receipt = bounded_hex(value, "receipt", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    crate::program_lifecycle::verify_lifecycle_receipt(&receipt, expected_activity, sequencer)
}

fn decode_boundary_error(
    status: u16,
    error: &Map<String, Value>,
) -> Result<ProgramOperationError, ProgramOperationError> {
    if !(400..600).contains(&status) {
        return Err(ProgramOperationError::Decode);
    }
    let code = required_string(error, "code")?;
    if code.is_empty()
        || code.len() > MAX_REASON_BYTES
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ProgramOperationError::Decode);
    }
    let retry_after_seconds = match required_string(error, "retry")? {
        "never" if exact_fields(error, &["code", "retry"]) => None,
        "after" if exact_fields(error, &["code", "retry", "retry_after_seconds"]) => Some(
            error
                .get("retry_after_seconds")
                .and_then(Value::as_u64)
                .filter(|seconds| *seconds > 0)
                .ok_or(ProgramOperationError::Decode)?,
        ),
        _ => return Err(ProgramOperationError::Decode),
    };
    Ok(ProgramOperationError::Boundary {
        status,
        code: code.to_owned(),
        retry_after_seconds,
    })
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
        || !reason.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.')
        })
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
    if value.get("protocol_result_code") != Some(&Value::Null) && protocol_result_code.is_none() {
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

fn accepted_program_verification(operation: &str, result: &Value, value: Option<&Value>) -> bool {
    let Some(status) = value.and_then(Value::as_object) else {
        return false;
    };
    if matches!(operation, "program.discover" | "program.interface") {
        return exact_unverified(status, "server_side_receipt_verification_only");
    }
    let result_state = result
        .as_object()
        .and_then(|object| object.get("state"))
        .and_then(Value::as_str);
    if matches!(
        operation,
        "program.call" | "program.receipt" | "program.activity"
    ) && matches!(result_state, Some("unknown" | "pending"))
    {
        return exact_unverified(status, "receipt_pending");
    }
    exact_fields(status, &["state", "level"])
        && status.get("state").and_then(Value::as_str) == Some("Achieved")
        && status.get("level").and_then(Value::as_str) == Some("SequencerSigned")
}

fn exact_unverified(value: &Map<String, Value>, reason: &str) -> bool {
    exact_fields(value, &["state", "requested", "achieved", "reason"])
        && value.get("state").and_then(Value::as_str) == Some("Unverified")
        && value.get("requested").and_then(Value::as_str) == Some("SequencerSigned")
        && value.get("achieved").and_then(Value::as_str) == Some("Unverified")
        && value.get("reason").and_then(Value::as_str) == Some(reason)
}

fn exact_fields(value: &Map<String, Value>, required: &[&str]) -> bool {
    value.len() == required.len() && required.iter().all(|field| value.contains_key(*field))
}

fn valid_request_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_REQUEST_ID_BYTES
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn decode_discovery(
    value: &Value,
    expected_program: [u8; 32],
    now: u64,
) -> Result<VerifiedProgramDiscovery, ProgramOperationError> {
    let value = object(value)?;
    if fixed(value, "program_id")? != expected_program
        || required_string(value, "verification")? != "registry-receipt-and-current-head-verified"
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
    if valid_through < observed_at || now > valid_through {
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
    now: u64,
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
    if valid_through < observed_at || now > valid_through {
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
    request: &impl BoundProgramRequest,
    trusted_sequencer_public_key: [u8; 32],
) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
    let value = object(value)?;
    if value.get("committed").and_then(Value::as_bool) != Some(false) {
        return Err(ProgramOperationError::Verification);
    }
    let decoded = decode_execution(
        value
            .get("execution")
            .ok_or(ProgramOperationError::Decode)?,
        Some(ExecutionState::Simulated),
        trusted_sequencer_public_key,
    )?;
    if decoded.activity_id != request.bound_activity_id()
        || decoded.program_id != request.request_program()
    {
        return Err(ProgramOperationError::IdentityMismatch);
    }
    let evidence = decode_simulation_evidence(
        value
            .get("simulation_evidence")
            .ok_or(ProgramOperationError::Decode)?,
    )?;
    let public_key = decoded.authority.sequencer_public_key();
    let expected_boundary: [u8; 32] =
        Sha256::digest([SIMULATION_BOUNDARY_DOMAIN, public_key.as_slice()].concat()).into();
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
    if matches!(
        object.get("state").and_then(Value::as_str),
        Some("unknown" | "pending")
    ) {
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
        if expected
            .activity_id
            .is_some_and(|value| value != activity_id)
            || expected
                .idempotency_key
                .is_some_and(|value| value != idempotency_key)
            || expected
                .retained_signed_activity
                .is_some_and(|expected| retained.as_deref() != Some(expected))
        {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        return Ok(ProgramSubmission::Unknown {
            activity_id,
            idempotency_key,
            retained_signed_activity: retained,
        });
    }
    let decoded = decode_execution(value, None, expected.trusted_sequencer_public_key)?;
    if !matches!(
        decoded.state,
        ExecutionState::Executed | ExecutionState::Refused
    ) || expected
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

fn decode_execution_authority(
    value: &Map<String, Value>,
) -> Result<layerx_proof::receipt::AuthorizedBatch, ProgramOperationError> {
    let authority_value = value
        .get("authority")
        .and_then(Value::as_object)
        .ok_or(ProgramOperationError::Decode)?;
    Ok(layerx_proof::receipt::AuthorizedBatch::new(
        fixed(authority_value, "batch_id")?,
        fixed(authority_value, "asset")?,
        fixed(authority_value, "previous_state_root")?,
        fixed(authority_value, "resulting_state_root")?,
        fixed(authority_value, "sequencer_public_key")?,
    ))
}

fn decode_execution(
    value: &Value,
    expected_state: Option<ExecutionState>,
    trusted_sequencer_public_key: [u8; 32],
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
    let module_version = bounded_u32(value, "module_version", 1, 4)?;
    let batch_id = fixed(value, "batch_id")?;
    let global_sequence = decimal_u64(value, "global_sequence")?;
    let result_code = exact_i32(value, "result_code")?;
    let state_root = fixed(value, "state_root")?;
    let receipt_digest = fixed(value, "receipt_digest")?;
    let receipt = bounded_hex(value, "receipt", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    let terminal_payload = bounded_hex(value, "terminal_payload", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    let call_graph = bounded_hex(value, "call_graph", MAX_SIGNED_ACTIVITY_BYTES, None)?;
    let authority = decode_execution_authority(value)?;
    if authority.sequencer_public_key() != trusted_sequencer_public_key
        || authority.batch_id() != batch_id
        || authority.resulting_state_root() != state_root
    {
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

fn fixed(value: &Map<String, Value>, field: &str) -> Result<[u8; 32], ProgramOperationError> {
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
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn decimal_u64(value: &Map<String, Value>, field: &str) -> Result<u64, ProgramOperationError> {
    canonical_decimal(value, field)?
        .parse()
        .map_err(|_| ProgramOperationError::Bounds)
}

fn decimal_u128(value: &Map<String, Value>, field: &str) -> Result<u128, ProgramOperationError> {
    canonical_decimal(value, field)?
        .parse()
        .map_err(|_| ProgramOperationError::Bounds)
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

fn exact_i32(value: &Map<String, Value>, field: &str) -> Result<i32, ProgramOperationError> {
    value
        .get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or(ProgramOperationError::Decode)
}

impl NativeProgramTransport for HttpProgramTransport {
    fn simulate_native(
        &self,
        request: &NativeProgramCallRequest,
    ) -> Result<VerifiedProgramSimulation, ProgramOperationError> {
        let value = self.dispatch(
            "program.simulate",
            Method::Post,
            "/v1/programs/simulate",
            &wire_native_call(request)?,
            None,
        )?;
        let verified = decode_simulation(&value, request, self.trusted_sequencer_public_key)?;
        require_native_execution(verified.execution())?;
        Ok(verified)
    }
    fn submit_native(
        &self,
        request: &NativeProgramCallRequest,
        idempotency_key: [u8; 32],
    ) -> Result<ProgramSubmission, ProgramOperationError> {
        let submission =
            self.submit_bound(request, &wire_native_call(request)?, idempotency_key)?;
        match &submission {
            ProgramSubmission::Executed(verified) | ProgramSubmission::Refused(verified) => {
                require_native_execution(verified)?;
            }
            ProgramSubmission::Unknown { .. } => {}
        }
        Ok(submission)
    }
}

fn wire_native_call(request: &NativeProgramCallRequest) -> Result<Value, ProgramOperationError> {
    let native = layerx_types::program_call::NativeProgramCall::decode(&request.payload)
        .map_err(|_| ProgramOperationError::Decode)?;
    Ok(json!({
        "payload_encoding": "native-v1", "program_id": hex(&request.program_id), "calldata": hex(native.calldata),
        "budget": { "fuel": native.resources.0[0].to_string(), "fee_limit": request.fee_limit.to_string() },
        "signed_activity": hex(request.signed_activity()),
        "native_call": { "guest_abi": native.guest_abi, "entrypoint": std::str::from_utf8(native.entrypoint).map_err(|_| ProgramOperationError::Decode)?,
            "capabilities_hex": hex(native.capabilities), "access_declaration_hex": hex(native.access_declaration),
            "response_capacity": native.response_capacity, "resources": native.resources.0.map(|value| value.to_string()) }
    }))
}

fn require_native_execution(
    verified: &layerx_proof::program::VerifiedProgramExecution,
) -> Result<(), ProgramOperationError> {
    let protocol = verified
        .receipt()
        .receipt()
        .protocol()
        .ok_or(ProgramOperationError::Verification)?;
    if protocol.protocol_version() != 3 {
        return Err(ProgramOperationError::Verification);
    }
    Ok(())
}

#[cfg(test)]
mod source_contract {
    use serde_json::json;

    use super::{accepted_program_verification, decode_service_error, exact_fields, object};

    #[test]
    fn corrupt_lifecycle_boundary_responses_retain_c_signed_request() -> Result<(), String> {
        use crate::program_lifecycle::{
            programs_module_registry, NativeProgramLifecycleRequest, ProgramLifecycleSubmission,
        };
        use layerx_types::program_lifecycle::NativeProgramDeploy;
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../platform/sdk/conformance/fixtures/native-program-deploy-v3.json"
        ))
        .map_err(|error| error.to_string())?;
        let fields = object(&fixture).map_err(|error| format!("{error:?}"))?;
        let payload = super::bounded_hex(fields, "payload_hex", 524_288, None)
            .map_err(|error| format!("{error:?}"))?;
        let signed = super::bounded_hex(fields, "signed_activity_hex", 1_048_576, None)
            .map_err(|error| format!("{error:?}"))?;
        let registry = programs_module_registry().map_err(|error| format!("{error:?}"))?;
        let request = NativeProgramLifecycleRequest::deploy(
            &registry,
            NativeProgramDeploy::decode(&payload).map_err(|error| format!("{error:?}"))?,
            &signed,
        )
        .map_err(|error| format!("{error:?}"))?;
        let receipt_fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v3.json"
        ))
        .map_err(|error| error.to_string())?;
        let mut receipt = super::bounded_hex(
            object(&receipt_fixture).map_err(|error| format!("{error:?}"))?,
            "canonical_receipt_hex",
            1_048_576,
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
        *receipt.last_mut().ok_or("missing receipt signature")? ^= 1;
        let expected = ProgramLifecycleSubmission::Unknown {
            activity_id: request.bound_activity_id(),
            idempotency_key: request.bound_idempotency_key(),
            retained_signed_activity: signed,
        };
        for document in [
            json!({"result":{"state":"executed", "activity_id":fixture["activity_id_hex"], "receipt":super::hex(&receipt), "terminal_payload":"", "call_graph":""}}),
            json!({"result":{"state":"refused", "activity_id":fixture["activity_id_hex"], "receipt":"00", "terminal_payload":"", "call_graph":""}}),
            json!({"result":{},"extra":true}),
            json!({"state":"unknown", "activity_id":"00".repeat(32), "retry":"after", "retry_after_seconds":2}),
        ] {
            let attempt =
                super::decode_agent_document(200, document, "program.deploy").and_then(|value| {
                    super::HttpProgramTransport::decode_lifecycle_submission(
                        &value, &request, [1; 32],
                    )
                });
            assert_eq!(
                super::resolve_lifecycle_submission(&request, attempt)
                    .map_err(|error| format!("{error:?}"))?,
                expected
            );
        }
        let malformed = serde_json::from_slice::<serde_json::Value>(b"{\"result\":")
            .map_err(|_| super::ProgramOperationError::Decode)
            .and_then(|value| {
                super::HttpProgramTransport::decode_lifecycle_submission(&value, &request, [1; 32])
            });
        assert_eq!(
            super::resolve_lifecycle_submission(&request, malformed)
                .map_err(|error| format!("{error:?}"))?,
            expected
        );
        let refusal = super::decode_agent_document(
            400,
            json!({"error":{"code":"invalid_program_payload","retry":"never"}}),
            "program.deploy",
        )
        .and_then(|value| {
            super::HttpProgramTransport::decode_lifecycle_submission(&value, &request, [1; 32])
        });
        assert!(matches!(
            super::resolve_lifecycle_submission(&request, refusal),
            Err(super::ProgramOperationError::Boundary { status: 400, .. })
        ));
        let transport =
            super::HttpProgramTransport::connect("http://127.0.0.1:1", None, [1; 32], || {
                super::wall_clock::wall_time()
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .and_then(|duration| u64::try_from(duration.as_millis()).ok())
                    .ok_or(super::ProgramOperationError::Verification)
            })
            .map_err(|error| format!("{error:?}"))?;
        assert!(matches!(
            transport.submit_lifecycle(
                &request,
                [0; 32],
                1,
                "program.deploy",
                "/v1/programs/deploy"
            ),
            Err(super::ProgramOperationError::IdentityMismatch)
        ));
        Ok(())
    }

    #[test]
    fn lifecycle_recovery_requires_exact_identity_and_state_receipt() {
        let activity = [0x41; 32];
        let response = json!({"activity_id":super::hex(&activity),"receipt":"00"});
        assert!(super::decode_lifecycle_recovery(&response, [0x42; 32], [1; 32]).is_err());
        assert!(super::decode_lifecycle_recovery(&response, activity, [1; 32]).is_err());
        let mut widened = response;
        widened["program_id"] = json!("11".repeat(32));
        assert!(super::decode_lifecycle_recovery(&widened, activity, [1; 32]).is_err());
    }

    #[test]
    fn lifecycle_boundary_errors_preserve_exact_refusals(
    ) -> Result<(), super::ProgramOperationError> {
        let refusal = json!({"code":"invalid_program_payload","retry":"never"});
        assert_eq!(
            super::decode_boundary_error(400, object(&refusal)?)?,
            super::ProgramOperationError::Boundary {
                status: 400,
                code: "invalid_program_payload".into(),
                retry_after_seconds: None
            }
        );
        let retry = json!({"code":"node_unavailable","retry":"after","retry_after_seconds":2});
        assert_eq!(
            super::decode_boundary_error(503, object(&retry)?)?,
            super::ProgramOperationError::Boundary {
                status: 503,
                code: "node_unavailable".into(),
                retry_after_seconds: Some(2)
            }
        );
        for malformed in [
            json!({"code":"invalid_program_payload","retry":"never","extra":0}),
            json!({"code":"node_unavailable","retry":"after","retry_after_seconds":true}),
        ] {
            assert!(super::decode_boundary_error(400, object(&malformed)?).is_err());
        }
        assert!(super::decode_boundary_error(200, object(&refusal)?).is_err());
        Ok(())
    }

    #[test]
    fn programs_verification_status_matrix_is_closed() {
        let server_attested = json!({
            "state":"Unverified",
            "requested":"SequencerSigned",
            "achieved":"Unverified",
            "reason":"server_side_receipt_verification_only",
        });
        let pending = json!({
            "state":"Unverified",
            "requested":"SequencerSigned",
            "achieved":"Unverified",
            "reason":"receipt_pending",
        });
        let achieved = json!({"state":"Achieved","level":"SequencerSigned"});
        assert!(accepted_program_verification(
            "program.discover",
            &json!({"program_id":"00"}),
            Some(&server_attested),
        ));
        assert!(accepted_program_verification(
            "program.call",
            &json!({"state":"unknown"}),
            Some(&pending),
        ));
        assert!(accepted_program_verification(
            "program.simulate",
            &json!({"committed":false}),
            Some(&achieved),
        ));
        assert!(!accepted_program_verification(
            "program.discover",
            &json!({"program_id":"00"}),
            Some(&achieved),
        ));
        assert!(!accepted_program_verification(
            "program.call",
            &json!({"state":"unknown"}),
            Some(&achieved),
        ));
        let mut widened = pending;
        widened["extra"] = json!(true);
        assert!(!accepted_program_verification(
            "program.receipt",
            &json!({"state":"pending"}),
            Some(&widened),
        ));
    }

    #[test]
    fn programs_error_envelope_fields_are_exact() {
        let exact = json!({
            "class":"CoreRejection",
            "protocol_result_code":-7,
            "retriability":"Terminal",
            "request_id":"request-1",
            "reason":"core_refused",
        });
        let exact = object(&exact).unwrap_or_else(|_| panic!("object"));
        assert!(exact_fields(
            exact,
            &[
                "class",
                "protocol_result_code",
                "retriability",
                "reason",
                "request_id"
            ],
        ));
        assert!(decode_service_error(400, exact).is_ok());
        let mut widened = exact.clone();
        widened.insert("extra".to_owned(), json!(true));
        assert!(!exact_fields(
            &widened,
            &[
                "class",
                "protocol_result_code",
                "retriability",
                "reason",
                "request_id"
            ],
        ));
    }
}
