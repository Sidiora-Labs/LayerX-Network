//! Verified name resolution through the naming reference program.
//!
//! The index signs one noncommitting `resolve` entry-point read with its own
//! chain-registered read principal, and accepts the answer only when the
//! returned receipt, terminal payload, call graph and simulation evidence all
//! verify against the trusted sequencer key and bind to that exact request.

use std::io::{Read as _, Write as _};
use std::net::{TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use layerx_client::lni::simulate::{
    simulation_boundary_id, simulation_evidence_digest, SimulationEvidence,
};
use layerx_program_sdk::error::STATUS_INVALID;
use layerx_program_sdk::naming::{
    Name, Record, Request, RECORD_BYTES, REFERENCE_ASSET, REFERENCE_OCCUPANCY_CEILING,
    REFERENCE_OCCUPANCY_SEED,
};
use layerx_programs::{hex, InterfaceCapability, ProgramInterface, ValueSchema, ValueType};
use layerx_proof::program::{verify_program_execution, ProgramExecutionExpectation};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_sdk::native_capabilities::{
    derive_native_program_account, NativeCapability, NativeCapabilitySet,
};
use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::{ProgramCallFailure, ProgramCallOutcome, ProgramId};
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload};
use layerx_types::program_call::{NativeProgramCall, Resources};
use layerx_wire::activity::{decode_signed, encode_signed_envelope, encode_unsigned_envelope};
use layerx_wire::hash::{activity_id, payload_hash, Domain};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// The canonical absent access declaration: the node derives the access set
/// from the granted capabilities.
const ABSENT_ACCESS_DECLARATION: &[u8] = b"LayerX/programs/access-declaration/v1\0\0";
/// Declared resource ceilings in native wire order; the node's own defaults.
const READ_RESOURCES: [u64; 7] = [
    1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096,
];
const RESPONSE_CAPACITY: u32 = 128;
const RESPONSE_HEADER: [u8; 2] = [1, 0x20];
const NOT_BEFORE_SKEW_MS: u64 = 30_000;
const VALIDITY_MS: u64 = 120_000;
const IDEMPOTENCY_DOMAIN: &[u8] = b"layerx-explorer-index/name-read/v1\0";
const REFERENCE_ABI_VERSION: u16 = 2;
const REFERENCE_CALLDATA_BYTES: u32 = 100;
const REFERENCE_RESPONSE_BYTES: u32 = 64;
const ANSWER_LIMIT: u64 = 1024 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(10);

static NEXT_READ: AtomicU64 = AtomicU64::new(0);

/// A name that no registration can hold concurrently with the probe's meaning:
/// the readiness probe only needs verified evidence for a signed read, so any
/// verified answer for this label proves the read principal is registered.
pub const READINESS_PROBE_NAME: &str = "layerx-explorer-index-readiness-probe";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    /// The read key is not a 32-byte ed25519 seed in hexadecimal.
    InvalidKey,
    /// The read endpoint is not `https://<host>:<port>`.
    InvalidEndpoint,
    /// The name is outside the naming program's own label grammar.
    InvalidName,
    /// The signed read could not be constructed from canonical parts.
    Construction,
    /// The answer is not the documented program-read document.
    MalformedAnswer,
    /// The answer does not bind to the request the index signed.
    Unbound,
    /// The receipt, terminal payload or call graph failed verification.
    UnverifiedExecution,
    /// The simulation evidence is not signed by the trusted sequencer key.
    UnverifiedEvidence,
    /// The verified response is not a naming record.
    UnexpectedResponse,
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidKey => "read key is not a hexadecimal ed25519 seed",
            Self::InvalidEndpoint => "read endpoint is not https://<host>:<port>",
            Self::InvalidName => "name is outside the naming grammar",
            Self::Construction => "signed read could not be constructed",
            Self::MalformedAnswer => "read answer is malformed",
            Self::Unbound => "read answer is bound to another request",
            Self::UnverifiedExecution => "read execution proof is unverified",
            Self::UnverifiedEvidence => "read evidence is unverified",
            Self::UnexpectedResponse => "read response is not a naming record",
        })
    }
}

impl std::error::Error for ReadError {}

/// The index's own chain-registered ed25519 read identity.
pub struct ReadPrincipal {
    signing_key: SigningKey,
    did: String,
}

impl ReadPrincipal {
    /// # Errors
    /// Refuses anything other than sixty-four hexadecimal characters.
    pub fn from_seed_hex(text: &str) -> Result<Self, ReadError> {
        let seed = hex::decode_digest(text.trim()).map_err(|_| ReadError::InvalidKey)?;
        let signing_key = SigningKey::from_bytes(&seed);
        let did = format!(
            "did:layerx:{}",
            hex::encode(&signing_key.verifying_key().to_bytes())
        );
        Ok(Self { signing_key, did })
    }

    #[must_use]
    pub fn did(&self) -> &str {
        &self.did
    }

    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }
}

/// The network scope every signed read is bound to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadScope {
    pub network_id: u32,
    pub protocol_version: u16,
    pub fee_limit: u128,
}

/// One signed `resolve` read and the identifiers its answer must bind to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolveRead {
    pub signed_activity: Vec<u8>,
    pub activity_id: [u8; 32],
    pub payload_hash: [u8; 32],
    pub program: [u8; 32],
    pub guest_abi: u16,
    pub name: String,
}

/// The verified meaning of one resolve answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolveOutcome {
    Resolved {
        did: [u8; 32],
        expiry: u64,
    },
    /// The naming program refused the name as absent or expired.
    NotFound,
    /// The node authenticated a refusal other than the naming program's own.
    Refused {
        result_code: i32,
    },
}

fn program_call_registry() -> Result<(ActivityType, ModuleRegistry), ReadError> {
    let activity_type =
        ActivityType::new(ModuleId::Programs, 3).map_err(|_| ReadError::Construction)?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &[activity_type])
        .map_err(|_| ReadError::Construction)?;
    let registry = ModuleRegistry::new(&[registration]).map_err(|_| ReadError::Construction)?;
    Ok((activity_type, registry))
}

fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain.tag());
    digest.update(bytes);
    digest.finalize().into()
}

/// Validates a name with the naming program's own grammar.
///
/// # Errors
/// Refuses a label outside three to sixty-three bytes of `[a-z0-9-]` or with
/// a leading or trailing hyphen.
pub fn validate_name(name: &str) -> Result<(), ReadError> {
    Name::new(name.as_bytes())
        .map(|_| ())
        .map_err(|_| ReadError::InvalidName)
}

/// Returns the exact `resolve` calldata the naming program decodes.
///
/// # Errors
/// Refuses a name outside the naming program's grammar.
pub fn resolve_calldata(name: &str) -> Result<Vec<u8>, ReadError> {
    let label = Name::new(name.as_bytes()).map_err(|_| ReadError::InvalidName)?;
    Request::Resolve { name: label }
        .encode()
        .map(|bytes| bytes.as_slice().to_vec())
        .map_err(|_| ReadError::InvalidName)
}

/// Builds and signs one noncommitting `resolve` read at the principal's next
/// account sequence: the node admits a read only at that exact sequence.
///
/// # Errors
/// Refuses an invalid name and any part the canonical encoders refuse.
pub fn build_resolve_read(
    principal: &ReadPrincipal,
    scope: ReadScope,
    program: [u8; 32],
    guest_abi: u16,
    name: &str,
    account_sequence: u64,
    now_ms: u64,
) -> Result<ResolveRead, ReadError> {
    let label = Name::new(name.as_bytes()).map_err(|_| ReadError::InvalidName)?;
    let request = Request::Resolve { name: label };
    let calldata = request.encode().map_err(|_| ReadError::InvalidName)?;
    let capabilities = NativeCapabilitySet::new(vec![
        NativeCapability::StorageRead,
        NativeCapability::SharedStorageRead,
    ])
    .and_then(|set| set.encode())
    .map_err(|_| ReadError::Construction)?;
    let payload_bytes = NativeProgramCall {
        program_id: ProgramId::new(program),
        guest_abi,
        entrypoint: request.method().as_bytes(),
        calldata: calldata.as_slice(),
        capabilities: &capabilities,
        access_declaration: ABSENT_ACCESS_DECLARATION,
        response_capacity: RESPONSE_CAPACITY,
        resources: Resources(READ_RESOURCES),
    }
    .encode()
    .map_err(|_| ReadError::Construction)?;
    let (activity_type, registry) = program_call_registry()?;
    let payload = Payload::new(&registry, activity_type, &payload_bytes)
        .map_err(|_| ReadError::Construction)?;
    let declared_payload_hash = domain_hash(Domain::PayloadHash, payload.as_bytes());
    let public_key = principal.public_key();
    let mut idempotency = Sha256::new();
    idempotency.update(IDEMPOTENCY_DOMAIN);
    idempotency.update(public_key);
    idempotency.update(now_ms.to_be_bytes());
    idempotency.update(NEXT_READ.fetch_add(1, Ordering::Relaxed).to_be_bytes());
    idempotency.update(&payload_bytes);
    let idempotency: [u8; 32] = idempotency.finalize().into();
    let actor = Did::new(principal.did.as_bytes()).map_err(|_| ReadError::Construction)?;
    let authority = Authority::owner(&public_key).map_err(|_| ReadError::Construction)?;
    let bound = TimestampBound::new(
        now_ms.saturating_sub(NOT_BEFORE_SKEW_MS),
        now_ms.saturating_add(VALIDITY_MS),
    )
    .map_err(|_| ReadError::Construction)?;
    let mut builder = EnvelopeBuilder::new();
    builder
        .protocol_version(scope.protocol_version)
        .and_then(|value| value.network_id(scope.network_id))
        .and_then(|value| value.activity_type(activity_type))
        .and_then(|value| value.actor_did(actor))
        .and_then(|value| value.authority(authority))
        .and_then(|value| value.account_sequence(account_sequence))
        .and_then(|value| value.timestamp_bound(bound))
        .and_then(|value| value.idempotency_key(IdempotencyKey::new(idempotency)))
        .and_then(|value| value.fee_limit(Amount::from_u128(scope.fee_limit)))
        .and_then(|value| value.payload_hash(declared_payload_hash))
        .and_then(|value| value.payload(payload))
        .map(|_| ())
        .map_err(|_| ReadError::Construction)?;
    let unsigned = builder.build().map_err(|_| ReadError::Construction)?;
    let unsigned_bytes =
        encode_unsigned_envelope(&unsigned).map_err(|_| ReadError::Construction)?;
    let signature = principal
        .signing_key
        .sign(&domain_hash(Domain::SignaturePreimage, &unsigned_bytes))
        .to_bytes();
    let signed =
        unsigned.attach_signature(Signature::new(&signature).map_err(|_| ReadError::Construction)?);
    let signed_activity = encode_signed_envelope(&signed).map_err(|_| ReadError::Construction)?;
    let activity =
        decode_signed(&signed_activity, &registry).map_err(|_| ReadError::Construction)?;
    Ok(ResolveRead {
        activity_id: activity_id(&activity).map_err(|_| ReadError::Construction)?,
        payload_hash: payload_hash(&activity).map_err(|_| ReadError::Construction)?,
        signed_activity,
        program,
        guest_abi,
        name: name.to_owned(),
    })
}

fn text<'a>(value: &'a Value, name: &str) -> Result<&'a str, ReadError> {
    value[name].as_str().ok_or(ReadError::MalformedAnswer)
}

fn bytes(value: &Value, name: &str) -> Result<Vec<u8>, ReadError> {
    hex::decode(text(value, name)?).map_err(|_| ReadError::MalformedAnswer)
}

fn digest(value: &Value, name: &str) -> Result<[u8; 32], ReadError> {
    hex::decode_digest(text(value, name)?).map_err(|_| ReadError::MalformedAnswer)
}

fn decimal(value: &Value, name: &str) -> Result<u64, ReadError> {
    text(value, name)?
        .parse()
        .map_err(|_| ReadError::MalformedAnswer)
}

/// Verifies the core boundary's program-read document for `read` and decodes
/// the naming record. Every check fails closed.
///
/// # Errors
/// Refuses a malformed document, an answer for another request, and any
/// receipt, terminal payload, call graph or evidence that does not verify
/// against `sequencer_public_key`.
pub fn verify_resolve_answer(
    read: &ResolveRead,
    sequencer_public_key: [u8; 32],
    answer: &Value,
) -> Result<ResolveOutcome, ReadError> {
    let result = &answer["result"];
    if result["committed"] != Value::Bool(false) || result["read_only"] != Value::Bool(true) {
        return Err(ReadError::MalformedAnswer);
    }
    let execution = &result["execution"];
    if digest(execution, "activity_id")? != read.activity_id
        || digest(execution, "program_id")? != read.program
        || text(execution, "receipt_kind")? != "hypothetical"
    {
        return Err(ReadError::Unbound);
    }
    let receipt_bytes = bytes(execution, "receipt")?;
    let terminal = bytes(execution, "terminal_payload")?;
    let call_graph = bytes(execution, "call_graph")?;
    let receipt = verify_sequencer_signature(&receipt_bytes, sequencer_public_key)
        .map_err(|_| ReadError::UnverifiedExecution)?;
    let protocol = receipt.protocol().ok_or(ReadError::UnverifiedExecution)?;
    if protocol.activity_id() != read.activity_id {
        return Err(ReadError::Unbound);
    }
    let previous_state_root = protocol.previous_state_root();
    let hypothetical_state_root = protocol.resulting_state_root();
    let verified = verify_program_execution(
        &receipt_bytes,
        &terminal,
        &call_graph,
        ProgramExecutionExpectation {
            sequencer_public_key,
            previous_state_root,
            activity_id: read.activity_id,
            payload_hash: read.payload_hash,
            program_id: read.program,
            guest_abi_version: read.guest_abi,
        },
    )
    .map_err(|_| ReadError::UnverifiedExecution)?;
    let evidence = &result["simulation_evidence"];
    if digest(evidence, "boundary_id")? != simulation_boundary_id(&sequencer_public_key)
        || digest(evidence, "activity_id")? != read.activity_id
        || digest(evidence, "previous_state_root")? != previous_state_root
        || digest(evidence, "hypothetical_state_root")? != hypothetical_state_root
        || digest(evidence, "public_key")? != sequencer_public_key
        || evidence["committed"] != Value::Bool(false)
    {
        return Err(ReadError::UnverifiedEvidence);
    }
    let signature: [u8; 64] = bytes(evidence, "signature")?
        .try_into()
        .map_err(|_| ReadError::MalformedAnswer)?;
    let evidence_digest = simulation_evidence_digest(&SimulationEvidence {
        boundary_id: simulation_boundary_id(&sequencer_public_key),
        activity_id: read.activity_id,
        previous_state_root,
        hypothetical_state_root,
        observed_sequence: decimal(evidence, "observed_sequence")?,
        observed_at: decimal(evidence, "observed_at")?,
        public_key: sequencer_public_key,
        signature,
    });
    VerifyingKey::from_bytes(&sequencer_public_key)
        .and_then(|key| {
            key.verify_strict(
                &evidence_digest,
                &ed25519_dalek::Signature::from_bytes(&signature),
            )
        })
        .map_err(|_| ReadError::UnverifiedEvidence)?;
    let snapshot = &result["snapshot"];
    if digest(snapshot, "state_root")? != previous_state_root
        || text(snapshot, "verification")? != "sequencer_signed_snapshot"
    {
        return Err(ReadError::UnverifiedEvidence);
    }
    match verified.outcome() {
        ProgramCallOutcome::Completed(response) => {
            if verified.result_code() != 0 || response.code() != 0 {
                return Err(ReadError::UnexpectedResponse);
            }
            decode_record(response.body())
        }
        ProgramCallOutcome::LegacyCompleted(_) => Err(ReadError::UnexpectedResponse),
        ProgramCallOutcome::Refused(ProgramCallFailure::GuestRefused { code })
            if *code == STATUS_INVALID =>
        {
            Ok(ResolveOutcome::NotFound)
        }
        ProgramCallOutcome::Refused(_) => Ok(ResolveOutcome::Refused {
            result_code: verified.result_code(),
        }),
    }
}

/// Decodes the naming program's `[1, 0x20] || u32be(40) || did || expiry`.
///
/// # Errors
/// Refuses any other framing, length, reserved identifier or zero expiry.
pub fn decode_record(body: &[u8]) -> Result<ResolveOutcome, ReadError> {
    let header = body.get(..2).ok_or(ReadError::UnexpectedResponse)?;
    let length: [u8; 4] = body
        .get(2..6)
        .and_then(|value| value.try_into().ok())
        .ok_or(ReadError::UnexpectedResponse)?;
    let payload = body.get(6..).ok_or(ReadError::UnexpectedResponse)?;
    if header != RESPONSE_HEADER
        || usize::try_from(u32::from_be_bytes(length)).ok() != Some(RECORD_BYTES)
        || payload.len() != RECORD_BYTES
    {
        return Err(ReadError::UnexpectedResponse);
    }
    let record = Record::decode(payload).map_err(|_| ReadError::UnexpectedResponse)?;
    Ok(ResolveOutcome::Resolved {
        did: record.did.bytes(),
        expiry: record.expiry,
    })
}

/// Renders the exact document the web explorer decodes.
#[must_use]
pub fn resolved_json(name: &str, did: [u8; 32], expiry: u64) -> String {
    serde_json::json!({
        "name": name,
        "did": hex::encode(&did),
        "expiry": expiry.to_string(),
    })
    .to_string()
}

/// Reports whether a verified deployment interface is exactly the naming
/// reference interface for `program`.
#[must_use]
pub fn is_naming_reference_interface(program: [u8; 32], interface: &ProgramInterface) -> bool {
    let Ok(occupancy_account) = derive_native_program_account(program, REFERENCE_OCCUPANCY_SEED)
    else {
        return false;
    };
    let read = vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::SharedStorageRead,
    ];
    let write = vec![
        InterfaceCapability::StorageRead,
        InterfaceCapability::StorageWrite,
        InterfaceCapability::SharedStorageRead,
        InterfaceCapability::SharedStorageWrite,
    ];
    let mut occupancy = write.clone();
    occupancy.push(InterfaceCapability::Transfer402 {
        asset: REFERENCE_ASSET,
        to: occupancy_account,
        maximum_amount: REFERENCE_OCCUPANCY_CEILING,
    });
    let expected = [
        ("register", 1_u8, &occupancy),
        ("transfer", 2, &write),
        ("renew", 3, &occupancy),
        ("resolve", 4, &read),
        ("reverse_resolve", 5, &read),
    ];
    let calldata = ValueSchema::layerx(ValueType::Bytes {
        max_len: REFERENCE_CALLDATA_BYTES,
    });
    let response = ValueSchema::layerx(ValueType::Bytes {
        max_len: REFERENCE_RESPONSE_BYTES,
    });
    interface.abi_version() == REFERENCE_ABI_VERSION
        && interface.entries().len() == expected.len()
        && expected.iter().all(|(name, ordinal, capabilities)| {
            interface.entries().iter().any(|entry| {
                entry.name == *name
                    && entry.discriminator == [b'L', b'X', b'N', *ordinal]
                    && entry.calldata == calldata
                    && entry.response == response
                    && entry.capabilities.len() == capabilities.len()
                    && capabilities
                        .iter()
                        .all(|capability| entry.capabilities.contains(capability))
                    && entry.event_topics.is_empty()
                    && entry.failures.is_empty()
            })
        })
}

/// The core boundary route that accepts signed noncommitting program reads.
pub struct ReadEndpoint {
    host: String,
    port: u16,
    ca_der: Vec<u8>,
}

/// One HTTP answer from the read endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadAnswer {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Why one resolve produced no verified outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveFailure {
    /// The endpoint could not be reached or answered outside HTTP.
    Transport(String),
    /// The boundary or node refused the read before producing evidence.
    Refused { status: u16, code: String },
    /// The answer failed verification.
    Unverified(ReadError),
}

impl ReadEndpoint {
    /// # Errors
    /// Refuses anything other than `https://<host>:<port>` with a nonzero port.
    pub fn parse(endpoint: &str, ca_der: Vec<u8>) -> Result<Self, ReadError> {
        let (host, port) = endpoint
            .strip_prefix("https://")
            .and_then(|authority| authority.rsplit_once(':'))
            .ok_or(ReadError::InvalidEndpoint)?;
        let port: u16 = port.parse().map_err(|_| ReadError::InvalidEndpoint)?;
        if port == 0
            || host.is_empty()
            || !host
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
        {
            return Err(ReadError::InvalidEndpoint);
        }
        Ok(Self {
            host: host.to_owned(),
            port,
            ca_der,
        })
    }

    /// Posts one signed activity to `POST /v1/programs/read`.
    ///
    /// # Errors
    /// Reports connection, TLS and HTTP framing failures.
    pub fn program_read(&self, signed_activity: &[u8]) -> Result<ReadAnswer, String> {
        let head = format!(
            "POST /v1/programs/read HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.host,
            self.port,
            signed_activity.len()
        );
        self.exchange(&head, signed_activity)
    }

    /// Reads the principal's sequence document from
    /// `GET /v1/dids/<did>/sequence`.
    ///
    /// # Errors
    /// Reports connection, TLS and HTTP framing failures.
    pub fn account_sequence(&self, principal: &ReadPrincipal) -> Result<ReadAnswer, String> {
        let head = format!(
            "GET /v1/dids/{}/sequence HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
            principal.did(),
            self.host,
            self.port
        );
        self.exchange(&head, &[])
    }

    fn exchange(&self, head: &str, body: &[u8]) -> Result<ReadAnswer, String> {
        let certificate = native_tls::Certificate::from_der(&self.ca_der)
            .map_err(|error| format!("read endpoint trust root is invalid: {error}"))?;
        let connector = native_tls::TlsConnector::builder()
            .disable_built_in_roots(true)
            .add_root_certificate(certificate)
            .build()
            .map_err(|error| format!("read endpoint TLS setup failed: {error}"))?;
        let mut last = "read endpoint has no address".to_owned();
        let mut connected = None;
        for address in (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| format!("read endpoint resolution failed: {error}"))?
        {
            match TcpStream::connect_timeout(&address, IO_TIMEOUT) {
                Ok(stream) => {
                    connected = Some(stream);
                    break;
                }
                Err(error) => last = format!("read endpoint connection failed: {error}"),
            }
        }
        let stream = connected.ok_or(last)?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
            .map_err(|error| format!("read endpoint timeout setup failed: {error}"))?;
        let mut stream = connector
            .connect(&self.host, stream)
            .map_err(|error| format!("read endpoint TLS handshake failed: {error}"))?;
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body))
            .and_then(|()| stream.flush())
            .map_err(|error| format!("read endpoint request failed: {error}"))?;
        let mut answer = Vec::new();
        let mut limited = (&mut stream).take(ANSWER_LIMIT + 1);
        loop {
            let mut chunk = [0_u8; 8192];
            match limited.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => answer.extend_from_slice(&chunk[..count]),
                Err(error) => {
                    if complete_answer(&answer) {
                        break;
                    }
                    return Err(format!("read endpoint answer failed: {error}"));
                }
            }
            if complete_answer(&answer) {
                break;
            }
        }
        parse_answer(&answer)
    }
}

fn split_answer(answer: &[u8]) -> Option<(&str, &[u8])> {
    let end = answer.windows(4).position(|value| value == b"\r\n\r\n")?;
    Some((
        std::str::from_utf8(answer.get(..end)?).ok()?,
        answer.get(end + 4..)?,
    ))
}

fn declared_length(head: &str) -> Option<usize> {
    head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

fn complete_answer(answer: &[u8]) -> bool {
    split_answer(answer).is_some_and(|(head, body)| {
        declared_length(head).is_some_and(|length| body.len() >= length)
    })
}

/// Parses one bounded `Content-Length` framed HTTP/1.1 answer.
///
/// # Errors
/// Refuses an oversized, truncated or unframed answer.
pub fn parse_answer(answer: &[u8]) -> Result<ReadAnswer, String> {
    if u64::try_from(answer.len()).map_or(true, |length| length > ANSWER_LIMIT) {
        return Err("read endpoint answer exceeds its size limit".to_owned());
    }
    let (head, body) =
        split_answer(answer).ok_or_else(|| "read endpoint answer has no headers".to_owned())?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("HTTP/1.1 "))
        .and_then(|line| line.get(..3))
        .and_then(|code| code.parse::<u16>().ok())
        .filter(|code| (100..600).contains(code))
        .ok_or_else(|| "read endpoint answer has no status".to_owned())?;
    let length = declared_length(head)
        .ok_or_else(|| "read endpoint answer declares no length".to_owned())?;
    if body.len() != length {
        return Err("read endpoint answer disagrees with its declared length".to_owned());
    }
    Ok(ReadAnswer {
        status,
        body: body.to_vec(),
    })
}

/// Interprets one endpoint answer for `read`, failing closed.
///
/// # Errors
/// Reports a typed boundary refusal or the exact verification failure.
pub fn interpret_answer(
    read: &ResolveRead,
    sequencer_public_key: [u8; 32],
    answer: &ReadAnswer,
) -> Result<ResolveOutcome, ResolveFailure> {
    let document: Value = serde_json::from_slice(&answer.body)
        .map_err(|_| ResolveFailure::Unverified(ReadError::MalformedAnswer))?;
    if answer.status != 200 {
        return Err(ResolveFailure::Refused {
            status: answer.status,
            code: document["error"]["code"]
                .as_str()
                .unwrap_or("unspecified")
                .to_owned(),
        });
    }
    if document["ok"] != Value::Bool(true) {
        return Err(ResolveFailure::Unverified(ReadError::MalformedAnswer));
    }
    verify_resolve_answer(read, sequencer_public_key, &document).map_err(ResolveFailure::Unverified)
}

/// Interprets the core boundary's sequence document for `principal`, failing
/// closed. The value only selects the sequence the read is signed at: a wrong
/// value makes the node refuse the read, it can never make an answer verify.
///
/// # Errors
/// Reports a typed boundary refusal, a document for another identity, or a
/// sequence that is not a canonical decimal.
pub fn interpret_sequence(
    principal: &ReadPrincipal,
    answer: &ReadAnswer,
) -> Result<u64, ResolveFailure> {
    let document: Value = serde_json::from_slice(&answer.body)
        .map_err(|_| ResolveFailure::Unverified(ReadError::MalformedAnswer))?;
    if answer.status != 200 {
        return Err(ResolveFailure::Refused {
            status: answer.status,
            code: document["error"]["code"]
                .as_str()
                .unwrap_or("unspecified")
                .to_owned(),
        });
    }
    if document["ok"] != Value::Bool(true) {
        return Err(ResolveFailure::Unverified(ReadError::MalformedAnswer));
    }
    let result = &document["result"];
    if result["did"].as_str() != Some(principal.did()) {
        return Err(ResolveFailure::Unverified(ReadError::Unbound));
    }
    let sequence = text(result, "next_sequence").map_err(ResolveFailure::Unverified)?;
    sequence
        .parse::<u64>()
        .ok()
        .filter(|value| value.to_string() == sequence)
        .ok_or(ResolveFailure::Unverified(ReadError::MalformedAnswer))
}

/// Reads the principal's next sequence, then signs, posts and verifies one
/// resolve at it.
///
/// # Errors
/// Reports the first construction, transport, refusal or verification failure.
pub fn resolve(
    endpoint: &ReadEndpoint,
    principal: &ReadPrincipal,
    scope: ReadScope,
    sequencer_public_key: [u8; 32],
    target: (/* program */ [u8; 32], /* guest ABI */ u16),
    name: &str,
    now_ms: u64,
) -> Result<ResolveOutcome, ResolveFailure> {
    let sequence = endpoint
        .account_sequence(principal)
        .map_err(ResolveFailure::Transport)
        .and_then(|answer| interpret_sequence(principal, &answer))?;
    let read = build_resolve_read(principal, scope, target.0, target.1, name, sequence, now_ms)
        .map_err(ResolveFailure::Unverified)?;
    let answer = endpoint
        .program_read(&read.signed_activity)
        .map_err(ResolveFailure::Transport)?;
    interpret_answer(&read, sequencer_public_key, &answer)
}
