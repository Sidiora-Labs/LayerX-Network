//! Program web requests: a kernel program emits a request record under the
//! `PAXEERX_WEB_REQUEST_V1` topic in the same call that pays the web fee
//! account. `KernelWatcher` follows those records through the gateway,
//! `KernelAttestor` fetches or searches each payload independently and signs
//! the origin-2 digest, `ProgramExchange` trades those signatures with the
//! peer attestors under the program id and request id together, and
//! `KernelRelay` posts the observation activity with `lx_sendActivity` once
//! the registered threshold agrees.

use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::net::{SocketAddr, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use ed25519_dalek::{Signer as _, SigningKey as SubmitterKey};
use k256::ecdsa::SigningKey;
use layerx_wire::encode::Encoder;
use layerx_wire::hash::Domain;
use layerx_wire::limits::{MAX_MESSAGE_BYTES, PROTOCOL_VERSION};
use layerx_wire::WireError;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use crate::attest::{
    network_word, recover_signer, sign_digest, signer_address, stored_response, Answer,
    AttestError, Attestation, AttestorSet, Discard, Level, Ready, MAX_RECORD_BYTES,
    MAX_RESPONSE_BYTES, ORIGIN_PROGRAM, SIGNATURE_LENGTH,
};
use crate::content::{ContentStore, PEER_HEADER};
use crate::fetch::{Fetcher, HttpClient, Url};
use crate::index::WebIndex;
use crate::payment::{hex, unhex, GatewayRpc, RpcAnswer, COMMITMENT};
use crate::search;
use crate::server::{Response, RouteError, RouteTable};
use crate::watch::{hex0x, keccak, unhex0x};

pub use crate::server::PROGRAM_ATTESTATION_PATH;

/// The topic every program web request record is emitted under.
pub const REQUEST_TOPIC: &[u8] = b"PAXEERX_WEB_REQUEST_V1";

/// The topics whose records the watcher decodes as program web requests.
pub const REQUEST_TOPICS: [&[u8]; 1] = [REQUEST_TOPIC];

/// Whether `topic` is one whose records the watcher decodes.
#[must_use]
pub fn request_topic(topic: &[u8]) -> bool {
    REQUEST_TOPICS.contains(&topic)
}

/// The gateway method that lists committed program events by topic.
pub const EVENTS_METHOD: &str = "lx_getProgramEvents";

/// The most events one poll asks the gateway for.
pub const EVENTS_PER_POLL: u64 = 256;

/// The activity type of a web observation: module 11, ordinal 1.
pub const OBSERVATION_ACTIVITY: u32 = 0x000B_0001;

/// Request id, kind and payload length ahead of the payload.
pub const RECORD_HEADER_BYTES: usize = 13;

/// The observation header ahead of the stored response.
pub const OBSERVATION_HEADER_BYTES: usize = 146;

/// The most attestor signatures one observation carries.
pub const MAX_OBSERVATION_SIGNATURES: usize = 32;

/// How long a posted observation activity stays valid.
pub const ACTIVITY_VALIDITY_MS: u64 = 60_000;

const ACTIVITY_STRUCTURE: u16 = 0x1001;
const ACTIVITY_FIELDS: u8 = 12;
const UNSIGNED_ACTIVITY_FIELDS: u8 = 11;
const MAX_DID_BYTES: usize = 255;
const MAX_ACTIVITY_PAYLOAD_BYTES: usize = 524_288;
const MAX_ACTIVITY_SIGNATURE_BYTES: usize = 128;
const CURSOR_FILE: &str = "kernel-cursor";
const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const PEER_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const DISCARD_LOG: &str = "program-discarded.jsonl";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KernelError {
    /// The gateway endpoint is not an http or https URL with a host.
    Endpoint,
    /// No well-formed answer arrived.
    Unavailable,
    /// The gateway answered with a JSON-RPC error.
    Rejected { code: i64 },
    /// The answer does not have the shape the method defines.
    Malformed,
    /// A request record is not a canonical program web request.
    Record,
    /// The watcher's cursor could not be read or written.
    Cursor,
    /// The observation or its activity could not be encoded.
    Encode,
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Endpoint => f.write_str("gateway endpoint refused"),
            Self::Unavailable => f.write_str("gateway unavailable"),
            Self::Rejected { code } => write!(f, "gateway rejected the call with {code}"),
            Self::Malformed => f.write_str("gateway answer malformed"),
            Self::Record => f.write_str("program web request record malformed"),
            Self::Cursor => f.write_str("kernel watch cursor unreadable or unwritable"),
            Self::Encode => f.write_str("observation activity could not be encoded"),
        }
    }
}

impl std::error::Error for KernelError {}

impl From<WireError> for KernelError {
    fn from(_: WireError) -> Self {
        Self::Encode
    }
}

/// One committed program web request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramRequest {
    pub program_id: [u8; 32],
    pub request_id: u64,
    pub kind: u8,
    pub payload: Vec<u8>,
    pub sequence: u64,
}

/// Decodes the record a program emits: the request id big-endian, the kind,
/// the payload length big-endian and the payload.
///
/// # Errors
/// Refuses an unknown kind, an empty payload and a length that does not
/// match the record.
pub fn decode_request_record(
    program_id: [u8; 32],
    sequence: u64,
    data: &[u8],
) -> Result<ProgramRequest, KernelError> {
    if data.len() < RECORD_HEADER_BYTES {
        return Err(KernelError::Record);
    }
    let (header, payload) = data.split_at(RECORD_HEADER_BYTES);
    let mut id = [0; 8];
    id.copy_from_slice(&header[..8]);
    let mut length = [0; 4];
    length.copy_from_slice(&header[9..13]);
    let length = usize::try_from(u32::from_be_bytes(length)).map_err(|_| KernelError::Record)?;
    let kind = header[8];
    if !matches!(kind, 1 | 2) || length == 0 || payload.len() != length {
        return Err(KernelError::Record);
    }
    Ok(ProgramRequest {
        program_id,
        request_id: u64::from_be_bytes(id),
        kind,
        payload: payload.to_vec(),
        sequence,
    })
}

impl ProgramRequest {
    /// The origin-2 attestation of this request's answer.
    #[must_use]
    pub fn attestation(
        &self,
        network_id: u32,
        content_digest: [u8; 32],
        response: &[u8],
        full_length: u32,
    ) -> Attestation {
        Attestation {
            origin: ORIGIN_PROGRAM,
            network_id: network_word(u64::from(network_id)),
            requester: self.program_id,
            request_id: self.request_id,
            kind: self.kind,
            payload_hash: keccak(&self.payload),
            content_digest,
            response_hash: keccak(response),
            full_length,
        }
    }
}

fn call(rpc: &GatewayRpc, method: &str, params: &Value) -> Result<Value, KernelError> {
    match rpc.call(method, params) {
        Some(RpcAnswer::Result(value)) => Ok(value),
        Some(RpcAnswer::Error { code, .. }) => Err(KernelError::Rejected { code }),
        None => Err(KernelError::Unavailable),
    }
}

fn fixed<const N: usize>(value: Option<&Value>) -> Option<[u8; N]> {
    unhex(value?.as_str()?)?.try_into().ok()
}

fn decode_event(
    event: &Value,
    topic: &[u8],
    from: u64,
    next: u64,
) -> Result<ProgramRequest, KernelError> {
    let object = event.as_object().ok_or(KernelError::Malformed)?;
    let sequence = object
        .get("sequence")
        .and_then(Value::as_u64)
        .filter(|sequence| (from..next).contains(sequence))
        .ok_or(KernelError::Malformed)?;
    let program_id = fixed::<32>(object.get("program_id")).ok_or(KernelError::Malformed)?;
    let emitted = object
        .get("topic")
        .and_then(Value::as_str)
        .and_then(unhex)
        .ok_or(KernelError::Malformed)?;
    if emitted != topic {
        return Err(KernelError::Malformed);
    }
    let data = object
        .get("data")
        .and_then(Value::as_str)
        .and_then(unhex)
        .ok_or(KernelError::Malformed)?;
    decode_request_record(program_id, sequence, &data)
}

/// Follows committed program web request records through the gateway. The
/// next global sequence to read is kept in a cursor file under the state
/// directory, so a restart resumes where the last poll stopped.
pub struct KernelWatcher {
    rpc: GatewayRpc,
    cursor_path: PathBuf,
    next_sequence: u64,
    topics: Vec<Vec<u8>>,
}

impl KernelWatcher {
    /// Opens the watcher over [`REQUEST_TOPIC`]. A cursor file already under
    /// `state_dir` wins over `start`.
    ///
    /// # Errors
    /// Refuses an endpoint that is not an http or https URL, and returns the
    /// error creating the directory and a cursor file that does not hold one
    /// sequence number.
    pub fn open(endpoint: &str, state_dir: &Path, start: u64) -> io::Result<Self> {
        let rpc = GatewayRpc::new(endpoint)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid gateway endpoint"))?;
        std::fs::create_dir_all(state_dir)?;
        let cursor_path = state_dir.join(CURSOR_FILE);
        let stored = match std::fs::read_to_string(&cursor_path) {
            Ok(text) => Some(text.trim().parse::<u64>().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "kernel watch cursor malformed")
            })?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        Ok(Self {
            rpc,
            cursor_path,
            next_sequence: stored.unwrap_or(start),
            topics: vec![REQUEST_TOPIC.to_vec()],
        })
    }

    /// Watches `topics` instead of [`REQUEST_TOPIC`] alone.
    ///
    /// # Errors
    /// Refuses an empty list, a repeated topic and a topic whose records are
    /// not program web requests.
    pub fn with_topics(mut self, topics: &[&[u8]]) -> io::Result<Self> {
        let refused = topics.is_empty()
            || topics.iter().any(|topic| !request_topic(topic))
            || topics
                .iter()
                .enumerate()
                .any(|(index, topic)| topics[..index].contains(topic));
        if refused {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "kernel watch topics refused",
            ));
        }
        self.topics = topics.iter().map(|topic| topic.to_vec()).collect();
        Ok(self)
    }

    /// The topics the watcher reads, in the order it asks for them.
    #[must_use]
    pub fn topics(&self) -> &[Vec<u8>] {
        &self.topics
    }

    /// The next global sequence the watcher reads.
    #[must_use]
    pub const fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    fn store_cursor(&self, next: u64) -> Result<(), KernelError> {
        let temporary = self.cursor_path.with_extension("tmp");
        std::fs::write(&temporary, next.to_string())
            .and_then(|()| std::fs::File::open(&temporary)?.sync_all())
            .and_then(|()| std::fs::rename(&temporary, &self.cursor_path))
            .map_err(|_| KernelError::Cursor)
    }

    fn poll_topic(&self, topic: &[u8], from: u64) -> Result<(u64, Vec<Value>), KernelError> {
        let answer = call(
            &self.rpc,
            EVENTS_METHOD,
            &json!([{
                "topic": hex(topic),
                "from_sequence": from,
                "limit": EVENTS_PER_POLL,
            }]),
        )?;
        let next = answer
            .get("next_sequence")
            .and_then(Value::as_u64)
            .filter(|next| *next >= from)
            .ok_or(KernelError::Malformed)?;
        let events = answer
            .get("events")
            .and_then(Value::as_array)
            .filter(|events| u64::try_from(events.len()).is_ok_and(|n| n <= EVENTS_PER_POLL))
            .ok_or(KernelError::Malformed)?;
        Ok((next, events.clone()))
    }

    /// Reads the request records committed at or after the cursor under
    /// every watched topic, at most [`EVENTS_PER_POLL`] per topic, in
    /// sequence order. The cursor moves to the lowest next sequence the
    /// topics report, so a record past it is read again by the next poll and
    /// never skipped, and only after every record decoded.
    ///
    /// # Errors
    /// Returns the gateway's error, a malformed answer or record and a
    /// cursor that could not be written.
    pub fn poll(&mut self) -> Result<Vec<ProgramRequest>, KernelError> {
        let from = self.next_sequence;
        let mut answers = Vec::with_capacity(self.topics.len());
        for topic in &self.topics {
            let (next, events) = self.poll_topic(topic, from)?;
            answers.push((topic, next, events));
        }
        let next = answers
            .iter()
            .map(|(_, next, _)| *next)
            .min()
            .ok_or(KernelError::Malformed)?;
        let mut requests = Vec::new();
        for (topic, reported, events) in &answers {
            let mut decoded = events
                .iter()
                .map(|event| decode_event(event, topic, from, *reported))
                .collect::<Result<Vec<_>, _>>()?;
            if decoded
                .windows(2)
                .any(|pair| pair[0].sequence >= pair[1].sequence)
            {
                return Err(KernelError::Malformed);
            }
            decoded.retain(|request| request.sequence < next);
            requests.extend(decoded);
        }
        requests.sort_by_key(|request| request.sequence);
        if requests
            .windows(2)
            .any(|pair| pair[0].sequence == pair[1].sequence)
        {
            return Err(KernelError::Malformed);
        }
        if next != from {
            self.store_cursor(next)?;
            self.next_sequence = next;
        }
        Ok(requests)
    }
}

/// Fetches or searches a program request's payload independently and signs
/// the origin-2 digest with the web-attestor key.
pub struct KernelAttestor {
    key: SigningKey,
    signer: [u8; 20],
    network_id: u32,
    fetcher: Arc<Fetcher>,
    index: Arc<WebIndex>,
    store: Arc<ContentStore>,
}

impl KernelAttestor {
    #[must_use]
    pub fn new(
        key: SigningKey,
        network_id: u32,
        fetcher: Arc<Fetcher>,
        index: Arc<WebIndex>,
        store: Arc<ContentStore>,
    ) -> Self {
        Self {
            signer: signer_address(&key),
            key,
            network_id,
            fetcher,
            index,
            store,
        }
    }

    /// The address this attestor signs as.
    #[must_use]
    pub const fn signer(&self) -> [u8; 20] {
        self.signer
    }

    fn content(&self, request: &ProgramRequest) -> Result<([u8; 32], String), AttestError> {
        let payload = std::str::from_utf8(&request.payload).map_err(|_| AttestError::Payload)?;
        let (canonical, text) = match request.kind {
            1 => {
                let page = self.fetcher.fetch(payload).map_err(AttestError::Fetch)?;
                (page.canonical, page.text)
            }
            2 => {
                let results: Vec<search::SearchResult> = search::search(&self.index, payload)
                    .map_err(|_| AttestError::Search)?
                    .into_iter()
                    .map(|scored| scored.result)
                    .collect();
                let canonical = search::search_canonical_bytes(payload, &results)
                    .map_err(|_| AttestError::Search)?;
                let text = search::search_text(&results).map_err(|_| AttestError::Search)?;
                (canonical, text)
            }
            kind => return Err(AttestError::UnknownKind(kind)),
        };
        let digest = self.store.put(&canonical).map_err(|_| AttestError::Store)?;
        Ok((digest, text))
    }

    /// Answers one program request: the content, the stored response and the
    /// signature over the origin-2 digest. A program request carries no
    /// callback gas and no timeout height.
    ///
    /// # Errors
    /// Returns why the request could not be answered.
    pub fn attest(&self, request: &ProgramRequest) -> Result<Answer, AttestError> {
        let (content_digest, text) = self.content(request)?;
        let (response, full_length) = stored_response(&text)?;
        let attestation =
            request.attestation(self.network_id, content_digest, &response, full_length);
        let digest = attestation.digest();
        let signature = sign_digest(&self.key, &digest).map_err(AttestError::Sign)?;
        Ok(Answer {
            attestation,
            level: Level::Majority,
            response,
            callback_gas: 0,
            timeout_height: u64::MAX,
            digest,
            signer: self.signer,
            signature,
        })
    }
}

/// The observation payload the kernel's web intake admits: the 146-byte
/// header, the stored response, the signature count and the signatures in
/// ascending signer order.
///
/// # Errors
/// Refuses a response longer than a fulfilment stores, a full length shorter
/// than the response and a signature count outside one to
/// [`MAX_OBSERVATION_SIGNATURES`].
pub fn observation_bytes(
    network_id: u32,
    request: &ProgramRequest,
    ready: &Ready,
) -> Result<Vec<u8>, KernelError> {
    let response_length = u32::try_from(ready.response.len()).map_err(|_| KernelError::Encode)?;
    let count = ready.signatures.len();
    if network_id == 0
        || ready.request_id != request.request_id
        || ready.response.len() > MAX_RESPONSE_BYTES
        || ready.full_length < response_length
        || count == 0
        || count > MAX_OBSERVATION_SIGNATURES
    {
        return Err(KernelError::Encode);
    }
    let mut out = Vec::with_capacity(
        OBSERVATION_HEADER_BYTES + ready.response.len() + 1 + count * SIGNATURE_LENGTH,
    );
    out.push(ORIGIN_PROGRAM);
    out.extend_from_slice(&[0; 28]);
    out.extend_from_slice(&network_id.to_be_bytes());
    out.extend_from_slice(&request.program_id);
    out.extend_from_slice(&request.request_id.to_be_bytes());
    out.push(request.kind);
    out.extend_from_slice(&keccak(&request.payload));
    out.extend_from_slice(&ready.content_digest);
    out.extend_from_slice(&ready.full_length.to_be_bytes());
    out.extend_from_slice(&response_length.to_be_bytes());
    out.extend_from_slice(&ready.response);
    out.push(u8::try_from(count).map_err(|_| KernelError::Encode)?);
    for signature in &ready.signatures {
        out.extend_from_slice(signature);
    }
    Ok(out)
}

fn domain_hash(domain: Domain, bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(domain.tag());
    hash.update(bytes);
    hash.finalize().into()
}

/// The envelope fields of one observation activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityOptions<'a> {
    pub network_id: u32,
    pub actor_did: &'a str,
    pub account_sequence: u64,
    pub fee_limit: u128,
    pub not_before: u64,
    pub not_after: u64,
}

fn encode_envelope(
    observation: &[u8],
    authority: &[u8; 32],
    options: &ActivityOptions<'_>,
    signature: Option<&[u8; 64]>,
) -> Result<Vec<u8>, KernelError> {
    let mut encoder = Encoder::new(MAX_MESSAGE_BYTES);
    encoder.structure_header_version(ACTIVITY_STRUCTURE, PROTOCOL_VERSION)?;
    encoder.u8(if signature.is_some() {
        ACTIVITY_FIELDS
    } else {
        UNSIGNED_ACTIVITY_FIELDS
    })?;
    encoder.tag(1, ACTIVITY_FIELDS)?;
    encoder.u16(PROTOCOL_VERSION)?;
    encoder.tag(2, ACTIVITY_FIELDS)?;
    encoder.u32(options.network_id)?;
    encoder.tag(3, ACTIVITY_FIELDS)?;
    encoder.u32(OBSERVATION_ACTIVITY)?;
    encoder.tag(4, ACTIVITY_FIELDS)?;
    encoder.bytes(options.actor_did.as_bytes(), MAX_DID_BYTES)?;
    encoder.tag(5, ACTIVITY_FIELDS)?;
    encoder.bytes(authority, MAX_ACTIVITY_PAYLOAD_BYTES)?;
    encoder.tag(6, ACTIVITY_FIELDS)?;
    encoder.u64(options.account_sequence)?;
    encoder.tag(7, ACTIVITY_FIELDS)?;
    encoder.u64(options.not_before)?;
    encoder.u64(options.not_after)?;
    encoder.tag(8, ACTIVITY_FIELDS)?;
    encoder.bytes(&domain_hash(Domain::ContextHash, observation), 32)?;
    encoder.tag(9, ACTIVITY_FIELDS)?;
    encoder.u128(options.fee_limit)?;
    encoder.tag(10, ACTIVITY_FIELDS)?;
    encoder.bytes(&domain_hash(Domain::PayloadHash, observation), 32)?;
    encoder.tag(11, ACTIVITY_FIELDS)?;
    encoder.bytes(observation, MAX_ACTIVITY_PAYLOAD_BYTES)?;
    if let Some(signature) = signature {
        encoder.tag(12, ACTIVITY_FIELDS)?;
        encoder.bytes(signature, MAX_ACTIVITY_SIGNATURE_BYTES)?;
    }
    Ok(encoder.finish())
}

/// The signed observation activity: the twelve-field envelope of type
/// [`OBSERVATION_ACTIVITY`], its idempotency key the context hash of the
/// observation and its signature the submitter's Ed25519 signature over the
/// signing preimage of the eleven-field unsigned form.
///
/// # Errors
/// Refuses an empty or oversized DID, a zero network or fee limit and a
/// timestamp bound that ends before it starts.
pub fn encode_activity(
    observation: &[u8],
    submitter: &SubmitterKey,
    options: &ActivityOptions<'_>,
) -> Result<Vec<u8>, KernelError> {
    if options.actor_did.is_empty()
        || options.actor_did.len() > MAX_DID_BYTES
        || options.network_id == 0
        || options.fee_limit == 0
        || options.not_before > options.not_after
    {
        return Err(KernelError::Encode);
    }
    let authority = submitter.verifying_key().to_bytes();
    let unsigned = encode_envelope(observation, &authority, options, None)?;
    let preimage = domain_hash(Domain::SignaturePreimage, &unsigned);
    let signature = submitter.sign(&preimage).to_bytes();
    encode_envelope(observation, &authority, options, Some(&signature))
}

/// Posts observation activities through the gateway as the submitter DID.
pub struct ObservationSubmitter {
    rpc: GatewayRpc,
    key: SubmitterKey,
    did: String,
    network_id: u32,
    fee_limit: u128,
}

impl ObservationSubmitter {
    /// # Errors
    /// Refuses an endpoint that is not an http or https URL with a host.
    pub fn new(
        endpoint: &str,
        key: SubmitterKey,
        did: String,
        network_id: u32,
        fee_limit: u128,
    ) -> Result<Self, KernelError> {
        Ok(Self {
            rpc: GatewayRpc::new(endpoint).map_err(|_| KernelError::Endpoint)?,
            key,
            did,
            network_id,
            fee_limit,
        })
    }

    fn next_sequence(&self) -> Result<u64, KernelError> {
        let answer = call(&self.rpc, "lx_getSequence", &json!([self.did, "identity"]))?;
        let text = answer
            .get("next_sequence")
            .and_then(Value::as_str)
            .ok_or(KernelError::Malformed)?;
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(KernelError::Malformed);
        }
        text.parse().map_err(|_| KernelError::Malformed)
    }

    /// Signs the observation at the submitter's next identity sequence,
    /// valid from one second before `now_ms` for [`ACTIVITY_VALIDITY_MS`],
    /// and posts it with `lx_sendActivity`. Returns the gateway's result.
    ///
    /// # Errors
    /// Returns the gateway's error, a malformed sequence answer and an
    /// activity that could not be encoded.
    pub fn submit(&self, observation: &[u8], now_ms: u64) -> Result<Value, KernelError> {
        let activity = encode_activity(
            observation,
            &self.key,
            &ActivityOptions {
                network_id: self.network_id,
                actor_did: &self.did,
                account_sequence: self.next_sequence()?,
                fee_limit: self.fee_limit,
                not_before: now_ms.saturating_sub(1_000),
                not_after: now_ms.saturating_add(ACTIVITY_VALIDITY_MS),
            },
        )?;
        call(
            &self.rpc,
            "lx_sendActivity",
            &json!([hex(&activity), COMMITMENT]),
        )
    }
}

/// The key every program answer is held under: the program id and the
/// request id together, so two programs' request ids never share a slot.
pub type ProgramKey = ([u8; 32], u64);

/// One peer signature for a program request that was discarded, as recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramDiscarded {
    pub peer: String,
    pub program_id: [u8; 32],
    pub request_id: u64,
    pub reason: Discard,
    pub claimed_digest: Option<[u8; 32]>,
    pub claimed_signer: Option<[u8; 20]>,
}

impl ProgramDiscarded {
    fn line(&self) -> String {
        json!({
            "peer": self.peer,
            "program_id": hex0x(&self.program_id),
            "request_id": self.request_id,
            "reason": self.reason.code(),
            "claimed_digest": self.claimed_digest.map(|digest| hex0x(&digest)),
            "claimed_signer": self.claimed_signer.map(|signer| hex0x(&signer)),
        })
        .to_string()
    }
}

/// The record the program signature-exchange route serves for an answer:
/// the fields of [`Answer::record`] with the program id beside the request
/// id.
#[must_use]
pub fn program_record(answer: &Answer) -> Value {
    let mut record = answer.record();
    if let Some(object) = record.as_object_mut() {
        object.insert(
            "program_id".to_owned(),
            Value::String(hex0x(&answer.attestation.requester)),
        );
    }
    record
}

struct ProgramRecord {
    program_id: [u8; 32],
    request_id: u64,
    digest: [u8; 32],
    signer: [u8; 20],
    signature: [u8; SIGNATURE_LENGTH],
}

fn fixed0x<const N: usize>(value: Option<&Value>) -> Option<[u8; N]> {
    unhex0x(value?.as_str()?)?.try_into().ok()
}

fn parse_program_record(record: &Value) -> Option<ProgramRecord> {
    let object = record.as_object()?;
    let known = [
        "program_id",
        "request_id",
        "digest",
        "content_digest",
        "response_hash",
        "full_length",
        "signer",
        "signature",
    ];
    if object.keys().any(|key| !known.contains(&key.as_str())) {
        return None;
    }
    fixed0x::<32>(object.get("content_digest"))?;
    fixed0x::<32>(object.get("response_hash"))?;
    u32::try_from(object.get("full_length")?.as_u64()?).ok()?;
    Some(ProgramRecord {
        program_id: fixed0x(object.get("program_id"))?,
        request_id: object.get("request_id")?.as_u64()?,
        digest: fixed0x(object.get("digest"))?,
        signer: fixed0x(object.get("signer"))?,
        signature: fixed0x(object.get("signature"))?,
    })
}

struct Held {
    answer: Answer,
    signatures: BTreeMap<[u8; 20], [u8; SIGNATURE_LENGTH]>,
}

/// Exchanges attestor signatures for program requests with the configured
/// peer sidecars, holding every answer under its [`ProgramKey`].
///
/// Each sidecar serves its own signed record for a program request at
/// `GET /program-attestations/<program id>/<request id>`. A record is taken
/// only when it names the same program and request, its signature recovers
/// over this sidecar's own digest to the signer it claims, and that signer is
/// a registered attestor. Every refusal is recorded, never accepted.
pub struct ProgramExchange {
    peers: Vec<(String, Url)>,
    client: HttpClient,
    answers: Mutex<BTreeMap<ProgramKey, Held>>,
    discarded: Mutex<Vec<ProgramDiscarded>>,
    log_path: PathBuf,
}

impl ProgramExchange {
    /// Opens the exchange with its discard log under `state_dir`.
    ///
    /// # Errors
    /// Refuses a peer that is not an http or https URL with no query and
    /// returns the error creating the directory.
    pub fn open(state_dir: &Path, peers: &[String]) -> io::Result<Self> {
        let peers = peers
            .iter()
            .map(|peer| {
                Url::parse(peer)
                    .ok()
                    .filter(|url| !url.target.contains('?'))
                    .map(|url| (peer.clone(), url))
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid peer url"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        std::fs::create_dir_all(state_dir)?;
        let client = HttpClient::new(PEER_CONNECT_TIMEOUT)
            .map_err(|error| io::Error::other(error.code()))?;
        Ok(Self {
            peers,
            client,
            answers: Mutex::new(BTreeMap::new()),
            discarded: Mutex::new(Vec::new()),
            log_path: state_dir.join(DISCARD_LOG),
        })
    }

    /// The file every discarded signature is appended to.
    #[must_use]
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    fn answers(&self) -> std::sync::MutexGuard<'_, BTreeMap<ProgramKey, Held>> {
        self.answers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps this sidecar's own answer and its own signature under the
    /// answer's program and request. An answer already held there is kept
    /// as it is.
    pub fn record(&self, answer: Answer) {
        self.answers()
            .entry((answer.attestation.requester, answer.request_id()))
            .or_insert_with(|| Held {
                signatures: BTreeMap::from([(answer.signer, answer.signature)]),
                answer,
            });
    }

    /// This sidecar's answer to a program's request.
    #[must_use]
    pub fn answer(&self, key: ProgramKey) -> Option<Answer> {
        self.answers().get(&key).map(|held| held.answer.clone())
    }

    /// The program requests this sidecar holds an answer for, ascending.
    #[must_use]
    pub fn pending(&self) -> Vec<ProgramKey> {
        self.answers().keys().copied().collect()
    }

    /// Drops the answer and signatures for a program's request.
    pub fn forget(&self, key: ProgramKey) {
        self.answers().remove(&key);
    }

    /// Every signature discarded so far, in the order it was discarded.
    #[must_use]
    pub fn discarded(&self) -> Vec<ProgramDiscarded> {
        self.discarded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn discard(&self, entry: ProgramDiscarded) {
        let line = entry.line();
        eprintln!("x-websearch discarded a peer program signature: {line}");
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .and_then(|mut file| {
                file.write_all(line.as_bytes())?;
                file.write_all(b"\n")?;
                file.sync_all()
            });
        if let Err(error) = appended {
            eprintln!("x-websearch could not append to the discard log: {error}");
        }
        self.discarded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
    }

    /// The `GET /program-attestations/<program id>/<request id>` resource:
    /// this sidecar's own signed record, or 404.
    #[must_use]
    pub fn handle(&self, key: ProgramKey) -> Response {
        match self.answer(key) {
            Some(answer) => Response::json(200, program_record(&answer).to_string().into_bytes()),
            None => Response::error(404, "attestation_not_found"),
        }
    }

    /// Checks one peer record for a program request against this sidecar's
    /// own answer and the registered set, and keeps its signature. Every
    /// refusal is recorded.
    ///
    /// # Errors
    /// Returns why the signature was discarded. A request this sidecar holds
    /// no answer for is not an error and takes nothing.
    pub fn accept(
        &self,
        peer: &str,
        key: ProgramKey,
        record: &Value,
        set: &AttestorSet,
    ) -> Result<Option<[u8; 20]>, Discard> {
        let Some(local) = self.answer(key) else {
            return Ok(None);
        };
        let parsed = parse_program_record(record);
        let verdict = match &parsed {
            None => Err(Discard::Malformed),
            Some(record) if (record.program_id, record.request_id) != key => {
                Err(Discard::WrongRequest)
            }
            Some(record) if record.digest != local.digest => Err(Discard::DifferentDigest),
            Some(record) => match recover_signer(&local.digest, &record.signature) {
                Ok(signer) if signer != record.signer => Err(Discard::BadSignature),
                Err(_) => Err(Discard::BadSignature),
                Ok(signer) if !set.contains(&signer) => Err(Discard::UnknownSigner),
                Ok(signer) => Ok(signer),
            },
        };
        match verdict {
            Ok(signer) => {
                if let (Some(held), Some(record)) = (self.answers().get_mut(&key), parsed) {
                    held.signatures.insert(signer, record.signature);
                }
                Ok(Some(signer))
            }
            Err(reason) => {
                self.discard(ProgramDiscarded {
                    peer: peer.to_owned(),
                    program_id: key.0,
                    request_id: key.1,
                    reason,
                    claimed_digest: parsed.as_ref().map(|record| record.digest),
                    claimed_signer: parsed.as_ref().map(|record| record.signer),
                });
                Err(reason)
            }
        }
    }

    fn ask(&self, peer: &Url, key: ProgramKey) -> Option<Value> {
        let url = Url {
            target: format!(
                "{}{PROGRAM_ATTESTATION_PATH}{}/{}",
                peer.target.trim_end_matches('/'),
                hex(&key.0),
                key.1
            ),
            ..peer.clone()
        };
        let address: SocketAddr = (url.bare_host(), url.port).to_socket_addrs().ok()?.next()?;
        let response = self
            .client
            .get(
                &url,
                address,
                Instant::now() + PEER_TOTAL_TIMEOUT,
                MAX_RECORD_BYTES,
                &[("Accept", "application/json"), (PEER_HEADER, "1")],
            )
            .ok()?;
        (response.status == 200)
            .then(|| serde_json::from_slice(&response.body).ok())
            .flatten()
    }

    /// Asks every peer for its record of a program request and keeps each
    /// signature that checks out. An unreachable peer or one with no record
    /// is skipped. Returns the number of signatures held for the request.
    #[must_use]
    pub fn collect(&self, key: ProgramKey, set: &AttestorSet) -> usize {
        if self.answer(key).is_none() {
            return 0;
        }
        for (name, peer) in &self.peers {
            let Some(record) = self.ask(peer, key) else {
                continue;
            };
            let _ = self.accept(name, key, &record, set);
        }
        self.answers()
            .get(&key)
            .map_or(0, |held| held.signatures.len())
    }

    /// The signatures for the observation once at least the threshold of
    /// registered signers agree with this sidecar's answer, ascending by
    /// signer.
    #[must_use]
    pub fn ready(&self, key: ProgramKey, set: &AttestorSet) -> Option<Ready> {
        let answers = self.answers();
        let held = answers.get(&key)?;
        let answer = &held.answer;
        let registered: Vec<([u8; 20], [u8; SIGNATURE_LENGTH])> = held
            .signatures
            .iter()
            .filter(|(signer, _)| set.contains(signer))
            .map(|(signer, signature)| (*signer, *signature))
            .collect();
        let enough = set.threshold != 0
            && u32::try_from(registered.len()).is_ok_and(|count| count >= set.threshold);
        if !enough {
            return None;
        }
        Some(Ready {
            request_id: key.1,
            response: answer.response.clone(),
            content_digest: answer.attestation.content_digest,
            full_length: answer.attestation.full_length,
            callback_gas: answer.callback_gas,
            digest: answer.digest,
            signers: registered.iter().map(|(signer, _)| *signer).collect(),
            signatures: registered.iter().map(|(_, signature)| *signature).collect(),
        })
    }
}

/// Registers the program signature-exchange route
/// `GET /program-attestations/<program id>/<request id>`.
///
/// # Errors
/// Refuses a table that already has a program exchange handler.
pub fn register(
    routes: &mut RouteTable,
    exchange: &Arc<ProgramExchange>,
) -> Result<(), RouteError> {
    let exchange = Arc::clone(exchange);
    routes.set_program_attestations(move |program_id, request_id| {
        exchange.handle((program_id, request_id))
    })
}

/// What one relay step did for one request.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The observation was posted; the gateway's result.
    Posted {
        program_id: [u8; 32],
        request_id: u64,
        result: Value,
    },
    /// The request could not be answered and was dropped.
    Refused {
        program_id: [u8; 32],
        request_id: u64,
        reason: AttestError,
    },
}

/// Ties the watcher, the attestor, the program signature exchange and the
/// submitter together. Requests stay queued until their observation posts.
pub struct KernelRelay {
    pub watcher: KernelWatcher,
    pub attestor: KernelAttestor,
    pub exchange: Arc<ProgramExchange>,
    pub set: AttestorSet,
    pub submitter: ObservationSubmitter,
    network_id: u32,
    queue: Vec<ProgramRequest>,
}

impl KernelRelay {
    #[must_use]
    pub fn new(
        watcher: KernelWatcher,
        attestor: KernelAttestor,
        exchange: Arc<ProgramExchange>,
        set: AttestorSet,
        submitter: ObservationSubmitter,
    ) -> Self {
        Self {
            network_id: attestor.network_id,
            watcher,
            attestor,
            exchange,
            set,
            submitter,
            queue: Vec::new(),
        }
    }

    /// The requests waiting for their observation to post, in sequence
    /// order.
    #[must_use]
    pub fn queued(&self) -> &[ProgramRequest] {
        &self.queue
    }

    /// Polls the watcher once, answers every queued request this sidecar
    /// has not answered yet, collects the peers' signatures and posts each
    /// observation whose signatures reach the threshold. Every request is
    /// held under its program id and request id together.
    ///
    /// # Errors
    /// Returns the watcher's error. A refused post keeps the request queued.
    pub fn step(&mut self, now_ms: u64) -> Result<Vec<Step>, KernelError> {
        let polled = self.watcher.poll()?;
        self.queue.extend(polled);
        let mut steps = Vec::new();
        let mut kept = Vec::new();
        for request in std::mem::take(&mut self.queue) {
            let key = (request.program_id, request.request_id);
            if self.exchange.answer(key).is_none() {
                match self.attestor.attest(&request) {
                    Ok(answer) => self.exchange.record(answer),
                    Err(reason) => {
                        steps.push(Step::Refused {
                            program_id: request.program_id,
                            request_id: request.request_id,
                            reason,
                        });
                        continue;
                    }
                }
            }
            let _ = self.exchange.collect(key, &self.set);
            let Some(ready) = self.exchange.ready(key, &self.set) else {
                kept.push(request);
                continue;
            };
            let posted = observation_bytes(self.network_id, &request, &ready)
                .and_then(|observation| self.submitter.submit(&observation, now_ms));
            match posted {
                Ok(result) => {
                    self.exchange.forget(key);
                    steps.push(Step::Posted {
                        program_id: request.program_id,
                        request_id: request.request_id,
                        result,
                    });
                }
                Err(_) => kept.push(request),
            }
        }
        self.queue = kept;
        Ok(steps)
    }
}
