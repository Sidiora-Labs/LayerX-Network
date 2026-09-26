use std::collections::BTreeMap;
use std::io::{self, Write as _};
use std::net::{SocketAddr, ToSocketAddrs as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use serde_json::{json, Value};

use crate::content::{ContentStore, PEER_HEADER};
use crate::fetch::{Fetcher, HttpClient, Url};
use crate::index::WebIndex;
use crate::search;
use crate::server::{Response, RouteError, RouteTable};
use crate::watch::{hex0x, keccak, unhex0x, WebRequest};

/// The ASCII domain every attestation preimage starts with.
pub const DOMAIN: &[u8] = b"PAXEERX_WEB_V1";

/// The length of the attestation preimage.
pub const PREIMAGE_LENGTH: usize = 14 + 1 + 32 + 32 + 8 + 1 + 32 + 32 + 32 + 4;

/// A request made by a contract through the xweb precompile.
pub const ORIGIN_EVM: u8 = 1;

/// A request made by a kernel program.
pub const ORIGIN_PROGRAM: u8 = 2;

/// The most response bytes a fulfilment stores.
pub const MAX_RESPONSE_BYTES: usize = 4_096;

/// A secp256k1 signature `r || s || v` with `v` in 27 or 28.
pub const SIGNATURE_LENGTH: usize = 65;

pub use crate::server::ATTESTATION_PATH;

/// The largest attestation record the exchange reads from a peer.
pub const MAX_RECORD_BYTES: usize = 4_096;

const PEER_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const PEER_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const DISCARD_LOG: &str = "discarded.jsonl";

/// What attestors sign for one request's answer, laid out byte for byte as
/// `modules/xweb/ATTESTATION.md` specifies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Attestation {
    pub origin: u8,
    pub network_id: [u8; 32],
    pub requester: [u8; 32],
    pub request_id: u64,
    pub kind: u8,
    pub payload_hash: [u8; 32],
    pub content_digest: [u8; 32],
    pub response_hash: [u8; 32],
    pub full_length: u32,
}

impl Attestation {
    /// The origin-1 attestation of a contract's request.
    #[must_use]
    pub fn evm(
        chain_id: u64,
        request: &WebRequest,
        content_digest: [u8; 32],
        response: &[u8],
        full_length: u32,
    ) -> Self {
        Self {
            origin: ORIGIN_EVM,
            network_id: network_word(chain_id),
            requester: evm_requester(request.requester),
            request_id: request.request_id,
            kind: request.kind,
            payload_hash: keccak(&request.payload),
            content_digest,
            response_hash: keccak(response),
            full_length,
        }
    }

    /// The 188-byte preimage, every integer big-endian.
    #[must_use]
    pub fn preimage(&self) -> [u8; PREIMAGE_LENGTH] {
        let mut out = [0; PREIMAGE_LENGTH];
        let fields: [&[u8]; 10] = [
            DOMAIN,
            &[self.origin],
            &self.network_id,
            &self.requester,
            &self.request_id.to_be_bytes(),
            &[self.kind],
            &self.payload_hash,
            &self.content_digest,
            &self.response_hash,
            &self.full_length.to_be_bytes(),
        ];
        let mut offset = 0;
        for field in fields {
            out[offset..offset + field.len()].copy_from_slice(field);
            offset += field.len();
        }
        out
    }

    /// keccak256 of the preimage: what attestors sign.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        keccak(&self.preimage())
    }
}

/// An EVM address left-padded with zeros to 32 bytes.
#[must_use]
pub fn evm_requester(address: [u8; 20]) -> [u8; 32] {
    let mut out = [0; 32];
    out[12..].copy_from_slice(&address);
    out
}

/// A network id as a big-endian uint256.
#[must_use]
pub fn network_word(id: u64) -> [u8; 32] {
    let mut out = [0; 32];
    out[24..].copy_from_slice(&id.to_be_bytes());
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureError {
    /// `v` is not 27 or 28.
    Recovery,
    /// `s` is above half the group order.
    HighS,
    /// `r` and `s` are not a signature or recover no key.
    Invalid,
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Recovery => "signature v is not 27 or 28",
            Self::HighS => "signature s is above half the order",
            Self::Invalid => "signature does not recover a key",
        })
    }
}

impl std::error::Error for SignatureError {}

/// The 20-byte address of a public key.
#[must_use]
pub fn address_of(key: &VerifyingKey) -> [u8; 20] {
    let hash = keccak(&key.to_encoded_point(false).as_bytes()[1..]);
    let mut address = [0; 20];
    address.copy_from_slice(&hash[12..]);
    address
}

/// The 20-byte address a signing key signs as.
#[must_use]
pub fn signer_address(key: &SigningKey) -> [u8; 20] {
    address_of(key.verifying_key())
}

/// Recovers the signer of a 65-byte signature over a raw digest, refusing a
/// `v` outside 27 and 28 and an `s` above half the order.
///
/// # Errors
/// Returns why the signature is refused.
pub fn recover_signer(
    digest: &[u8; 32],
    signature: &[u8; SIGNATURE_LENGTH],
) -> Result<[u8; 20], SignatureError> {
    let parity = signature[64]
        .checked_sub(27)
        .filter(|parity| *parity <= 1)
        .ok_or(SignatureError::Recovery)?;
    let parsed = Signature::from_slice(&signature[..64]).map_err(|_| SignatureError::Invalid)?;
    if parsed.normalize_s().is_some() {
        return Err(SignatureError::HighS);
    }
    let recovery = RecoveryId::from_byte(parity).ok_or(SignatureError::Invalid)?;
    let key = VerifyingKey::recover_from_prehash(digest, &parsed, recovery)
        .map_err(|_| SignatureError::Invalid)?;
    Ok(address_of(&key))
}

/// Signs a raw digest with no prefix: `r || s || v`, low `s`, `v` in 27 or
/// 28, checked by recovering the signer.
///
/// # Errors
/// Returns a signature that cannot be produced or does not recover the key.
pub fn sign_digest(
    key: &SigningKey,
    digest: &[u8; 32],
) -> Result<[u8; SIGNATURE_LENGTH], SignatureError> {
    let (signature, recovery) = key
        .sign_prehash_recoverable(digest)
        .map_err(|_| SignatureError::Invalid)?;
    let (signature, recovery) = match signature.normalize_s() {
        Some(normalized) => (
            normalized,
            RecoveryId::new(!recovery.is_y_odd(), recovery.is_x_reduced()),
        ),
        None => (signature, recovery),
    };
    let mut out = [0; SIGNATURE_LENGTH];
    out[..64].copy_from_slice(&signature.to_bytes());
    out[64] = 27 + u8::from(recovery.is_y_odd());
    if recover_signer(digest, &out)? != signer_address(key) {
        return Err(SignatureError::Invalid);
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttestError {
    /// The request kind is neither fetch nor search.
    UnknownKind(u8),
    /// The payload is not UTF-8 text.
    Payload,
    /// The fetch was refused.
    Fetch(crate::fetch::FetchError),
    /// The search failed or its canonical bytes could not be built.
    Search,
    /// The content store refused the canonical bytes.
    Store,
    /// The full text is longer than a uint32 length can carry.
    TooLong,
    /// The digest could not be signed.
    Sign(SignatureError),
}

impl std::fmt::Display for AttestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(f, "attestation refused: unknown kind {kind}"),
            Self::Payload => f.write_str("attestation refused: payload is not UTF-8"),
            Self::Fetch(error) => write!(f, "attestation refused: fetch {error}"),
            Self::Search => f.write_str("attestation refused: search failed"),
            Self::Store => f.write_str("attestation refused: content store"),
            Self::TooLong => f.write_str("attestation refused: text longer than uint32"),
            Self::Sign(error) => write!(f, "attestation refused: {error}"),
        }
    }
}

impl std::error::Error for AttestError {}

/// The stored response and the full length for a text: at most
/// [`MAX_RESPONSE_BYTES`] leading bytes, cut back to a character boundary,
/// and the byte length of the whole text.
///
/// # Errors
/// Refuses a text longer than a uint32 length can carry.
pub fn stored_response(text: &str) -> Result<(Vec<u8>, u32), AttestError> {
    let full_length = u32::try_from(text.len()).map_err(|_| AttestError::TooLong)?;
    let mut end = text.len().min(MAX_RESPONSE_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok((text.as_bytes()[..end].to_vec(), full_length))
}

/// This sidecar's signed answer to one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Answer {
    pub attestation: Attestation,
    pub response: Vec<u8>,
    pub callback_gas: u64,
    pub timeout_height: u64,
    pub digest: [u8; 32],
    pub signer: [u8; 20],
    pub signature: [u8; SIGNATURE_LENGTH],
}

impl Answer {
    #[must_use]
    pub const fn request_id(&self) -> u64 {
        self.attestation.request_id
    }

    /// The record the signature-exchange route serves for this answer.
    #[must_use]
    pub fn record(&self) -> Value {
        json!({
            "request_id": self.attestation.request_id,
            "digest": hex0x(&self.digest),
            "content_digest": hex0x(&self.attestation.content_digest),
            "response_hash": hex0x(&self.attestation.response_hash),
            "full_length": self.attestation.full_length,
            "signer": hex0x(&self.signer),
            "signature": hex0x(&self.signature),
        })
    }
}

/// Fetches or searches a request's payload independently and signs the
/// origin-1 digest with the web-attestor key.
pub struct Attestor {
    key: SigningKey,
    signer: [u8; 20],
    chain_id: u64,
    fetcher: Arc<Fetcher>,
    index: Arc<WebIndex>,
    store: Arc<ContentStore>,
}

impl Attestor {
    #[must_use]
    pub fn new(
        key: SigningKey,
        chain_id: u64,
        fetcher: Arc<Fetcher>,
        index: Arc<WebIndex>,
        store: Arc<ContentStore>,
    ) -> Self {
        Self {
            signer: signer_address(&key),
            key,
            chain_id,
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

    /// The canonical bytes, their digest and the text for a request's
    /// payload, the canonical bytes written to the content store.
    fn content(&self, request: &WebRequest) -> Result<([u8; 32], String), AttestError> {
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

    /// Answers one request: the content, the stored response and the
    /// signature over the origin-1 digest.
    ///
    /// # Errors
    /// Returns why the request could not be answered.
    pub fn attest(&self, request: &WebRequest) -> Result<Answer, AttestError> {
        let (content_digest, text) = self.content(request)?;
        let (response, full_length) = stored_response(&text)?;
        let attestation = Attestation::evm(
            self.chain_id,
            request,
            content_digest,
            &response,
            full_length,
        );
        let digest = attestation.digest();
        let signature = sign_digest(&self.key, &digest).map_err(AttestError::Sign)?;
        Ok(Answer {
            attestation,
            response,
            callback_gas: request.callback_gas,
            timeout_height: request.timeout_height,
            digest,
            signer: self.signer,
            signature,
        })
    }
}

/// The registered attestor signers and the threshold fulfil requires.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AttestorSet {
    pub signers: Vec<[u8; 20]>,
    pub threshold: u32,
}

impl AttestorSet {
    #[must_use]
    pub fn contains(&self, signer: &[u8; 20]) -> bool {
        self.signers.contains(signer)
    }
}

/// Why a peer's signature was not taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Discard {
    /// The record is not a well-formed attestation record.
    Malformed,
    /// The record answers another request.
    WrongRequest,
    /// The peer signed a different digest from this sidecar's.
    DifferentDigest,
    /// The signature does not recover the claimed signer over the digest.
    BadSignature,
    /// The signer is not a registered attestor.
    UnknownSigner,
}

impl Discard {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Malformed => "malformed_record",
            Self::WrongRequest => "wrong_request",
            Self::DifferentDigest => "different_digest",
            Self::BadSignature => "bad_signature",
            Self::UnknownSigner => "unknown_signer",
        }
    }
}

/// One peer signature that was discarded, as recorded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discarded {
    pub peer: String,
    pub request_id: u64,
    pub reason: Discard,
    pub claimed_digest: Option<[u8; 32]>,
    pub claimed_signer: Option<[u8; 20]>,
}

impl Discarded {
    fn line(&self) -> String {
        json!({
            "peer": self.peer,
            "request_id": self.request_id,
            "reason": self.reason.code(),
            "claimed_digest": self.claimed_digest.map(|digest| hex0x(&digest)),
            "claimed_signer": self.claimed_signer.map(|signer| hex0x(&signer)),
        })
        .to_string()
    }
}

/// Signatures ready for fulfil: at least the threshold, every signer
/// registered, in strictly ascending signer order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ready {
    pub request_id: u64,
    pub response: Vec<u8>,
    pub content_digest: [u8; 32],
    pub full_length: u32,
    pub callback_gas: u64,
    pub digest: [u8; 32],
    pub signers: Vec<[u8; 20]>,
    pub signatures: Vec<[u8; SIGNATURE_LENGTH]>,
}

struct Collected {
    answer: Answer,
    signatures: BTreeMap<[u8; 20], [u8; SIGNATURE_LENGTH]>,
}

/// A parsed attestation record.
struct Record {
    request_id: u64,
    digest: [u8; 32],
    signer: [u8; 20],
    signature: [u8; SIGNATURE_LENGTH],
}

fn fixed<const N: usize>(value: Option<&Value>) -> Option<[u8; N]> {
    unhex0x(value?.as_str()?)?.try_into().ok()
}

fn parse_record(record: &Value) -> Option<Record> {
    let object = record.as_object()?;
    let known = [
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
    fixed::<32>(object.get("content_digest"))?;
    fixed::<32>(object.get("response_hash"))?;
    u32::try_from(object.get("full_length")?.as_u64()?).ok()?;
    Some(Record {
        request_id: object.get("request_id")?.as_u64()?,
        digest: fixed(object.get("digest"))?,
        signer: fixed(object.get("signer"))?,
        signature: fixed(object.get("signature"))?,
    })
}

/// Exchanges attestor signatures with the configured peer sidecars.
///
/// Each sidecar serves its own signed record for a request at
/// `GET /attestations/<request id>`. The route is authenticated by the
/// records themselves: a record is taken only when its signature recovers,
/// over this sidecar's own digest for the request, to the signer it claims
/// and that signer is a registered attestor. A signature over a different
/// digest is discarded and recorded, never accepted.
pub struct SignatureExchange {
    peers: Vec<(String, Url)>,
    client: HttpClient,
    answers: Mutex<BTreeMap<u64, Collected>>,
    discarded: Mutex<Vec<Discarded>>,
    log_path: PathBuf,
}

impl SignatureExchange {
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

    fn answers(&self) -> std::sync::MutexGuard<'_, BTreeMap<u64, Collected>> {
        self.answers.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps this sidecar's own answer and its own signature. An answer
    /// already held for the request is kept as it is.
    pub fn record(&self, answer: Answer) {
        self.answers()
            .entry(answer.request_id())
            .or_insert_with(|| Collected {
                signatures: BTreeMap::from([(answer.signer, answer.signature)]),
                answer,
            });
    }

    /// This sidecar's answer to a request.
    #[must_use]
    pub fn answer(&self, request_id: u64) -> Option<Answer> {
        self.answers()
            .get(&request_id)
            .map(|collected| collected.answer.clone())
    }

    /// The requests this sidecar holds an answer for, ascending.
    #[must_use]
    pub fn pending(&self) -> Vec<u64> {
        self.answers().keys().copied().collect()
    }

    /// Drops the answer and signatures for a request.
    pub fn forget(&self, request_id: u64) {
        self.answers().remove(&request_id);
    }

    /// Drops every answer whose request timed out before `height`.
    pub fn expire(&self, height: u64) {
        self.answers()
            .retain(|_, collected| collected.answer.timeout_height >= height);
    }

    /// Every signature discarded so far, in the order it was discarded.
    #[must_use]
    pub fn discarded(&self) -> Vec<Discarded> {
        self.discarded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn discard(&self, entry: Discarded) {
        let line = entry.line();
        eprintln!("x-websearch discarded a peer signature: {line}");
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

    /// The `GET /attestations/<request id>` resource: this sidecar's own
    /// signed record, or 404.
    #[must_use]
    pub fn handle(&self, request_id: u64) -> Response {
        match self.answer(request_id) {
            Some(answer) => Response::json(200, answer.record().to_string().into_bytes()),
            None => Response::error(404, "attestation_not_found"),
        }
    }

    /// Checks one peer record for a request against this sidecar's own
    /// answer and the registered set, and keeps its signature. Every
    /// refusal is recorded.
    ///
    /// # Errors
    /// Returns why the signature was discarded. A request this sidecar holds
    /// no answer for is not an error and takes nothing.
    pub fn accept(
        &self,
        peer: &str,
        request_id: u64,
        record: &Value,
        set: &AttestorSet,
    ) -> Result<Option<[u8; 20]>, Discard> {
        let Some(local) = self.answer(request_id) else {
            return Ok(None);
        };
        let parsed = parse_record(record);
        let verdict = match &parsed {
            None => Err(Discard::Malformed),
            Some(record) if record.request_id != request_id => Err(Discard::WrongRequest),
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
                if let (Some(collected), Some(record)) =
                    (self.answers().get_mut(&request_id), parsed)
                {
                    collected.signatures.insert(signer, record.signature);
                }
                Ok(Some(signer))
            }
            Err(reason) => {
                self.discard(Discarded {
                    peer: peer.to_owned(),
                    request_id,
                    reason,
                    claimed_digest: parsed.as_ref().map(|record| record.digest),
                    claimed_signer: parsed.as_ref().map(|record| record.signer),
                });
                Err(reason)
            }
        }
    }

    fn ask(&self, peer: &Url, request_id: u64) -> Option<Value> {
        let url = Url {
            target: format!(
                "{}{ATTESTATION_PATH}{request_id}",
                peer.target.trim_end_matches('/')
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

    /// Asks every peer for its record of a request and keeps each signature
    /// that checks out. An unreachable peer or one with no record is
    /// skipped. Returns the number of signatures held for the request.
    #[must_use]
    pub fn collect(&self, request_id: u64, set: &AttestorSet) -> usize {
        if self.answer(request_id).is_none() {
            return 0;
        }
        for (name, peer) in &self.peers {
            let Some(record) = self.ask(peer, request_id) else {
                continue;
            };
            let _ = self.accept(name, request_id, &record, set);
        }
        self.answers()
            .get(&request_id)
            .map_or(0, |collected| collected.signatures.len())
    }

    /// The signatures for fulfil once at least the threshold of registered
    /// signers agree with this sidecar's answer, ascending by signer.
    #[must_use]
    pub fn ready(&self, request_id: u64, set: &AttestorSet) -> Option<Ready> {
        let answers = self.answers();
        let collected = answers.get(&request_id)?;
        let registered: Vec<([u8; 20], [u8; SIGNATURE_LENGTH])> = collected
            .signatures
            .iter()
            .filter(|(signer, _)| set.contains(signer))
            .map(|(signer, signature)| (*signer, *signature))
            .collect();
        let enough = u32::try_from(registered.len()).is_ok_and(|count| count >= set.threshold);
        if set.threshold == 0 || !enough {
            return None;
        }
        let answer = &collected.answer;
        Some(Ready {
            request_id,
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

/// Registers the signature-exchange route `GET /attestations/<request id>`.
///
/// # Errors
/// Refuses a table that already has an exchange handler.
pub fn register(
    routes: &mut RouteTable,
    exchange: &Arc<SignatureExchange>,
) -> Result<(), RouteError> {
    let exchange = Arc::clone(exchange);
    routes.set_attestations(move |request_id| exchange.handle(request_id))
}
