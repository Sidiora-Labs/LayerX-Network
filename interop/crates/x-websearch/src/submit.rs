use std::io;
use std::path::{Path, PathBuf};

use k256::ecdsa::SigningKey;
use serde_json::{json, Value};

use crate::attest::{sign_digest, signer_address, AttestorSet, Ready, SIGNATURE_LENGTH};
use crate::watch::{hex0x, keccak, parse_quantity, unhex0x, EvmError, EvmRpc, XWEB_PRECOMPILE};

/// The precompile method every fulfilment calls.
pub const FULFIL_SIGNATURE: &str = "fulfil(uint64,bytes,bytes32,uint32,bytes[])";

/// The request views the submitter reads.
pub const GET_REQUEST_SIGNATURE: &str = "getRequest(uint64)";
pub const GET_ATTESTORS_SIGNATURE: &str = "getAttestors()";

/// The EIP-1559 transaction type byte.
pub const DYNAMIC_FEE_TYPE: u8 = 2;

/// Request states the precompile records.
pub const STATUS_PENDING: u8 = 0;
pub const STATUS_FULFILLED: u8 = 1;
pub const STATUS_REFUNDED: u8 = 2;

/// The gas every transaction pays before its calldata.
const TRANSACTION_BASE_GAS: u64 = 21_000;
/// The precompile's own base charge for a transaction.
const PRECOMPILE_BASE_GAS: u64 = 30_000;
/// The calldata charge per byte, counted at the non-zero rate.
const CALLDATA_BYTE_GAS: u64 = 16;
/// The precompile's charge per attestor signature.
const SIGNATURE_GAS: u64 = 8_000;
/// What the precompile keeps beyond the callback bound to record the result.
const CALLBACK_RECORD_GAS: u64 = 10_000;

const JOURNAL_SUFFIX: &str = "json";

/// The four-byte selector of a method signature.
#[must_use]
pub fn selector(signature: &str) -> [u8; 4] {
    let hash = keccak(signature.as_bytes());
    [hash[0], hash[1], hash[2], hash[3]]
}

fn word(value: u64) -> [u8; 32] {
    let mut out = [0; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

fn usize_word(value: usize) -> [u8; 32] {
    word(u64::try_from(value).unwrap_or(u64::MAX))
}

fn dynamic(bytes: &[u8]) -> Vec<u8> {
    let mut out = usize_word(bytes.len()).to_vec();
    out.extend_from_slice(bytes);
    out.resize(32 + bytes.len().div_ceil(32) * 32, 0);
    out
}

/// The ABI calldata of `fulfil(requestId, response, contentDigest,
/// fullLength, signatures)`.
#[must_use]
pub fn fulfil_calldata(ready: &Ready) -> Vec<u8> {
    let response = dynamic(&ready.response);
    let mut signatures = usize_word(ready.signatures.len()).to_vec();
    let element = 32 + SIGNATURE_LENGTH.div_ceil(32) * 32;
    for index in 0..ready.signatures.len() {
        signatures.extend(usize_word(ready.signatures.len() * 32 + index * element));
    }
    for signature in &ready.signatures {
        signatures.extend(dynamic(signature));
    }
    let mut out = selector(FULFIL_SIGNATURE).to_vec();
    out.extend(word(ready.request_id));
    out.extend(usize_word(5 * 32));
    out.extend(ready.content_digest);
    out.extend(word(u64::from(ready.full_length)));
    out.extend(usize_word(5 * 32 + response.len()));
    out.extend(response);
    out.extend(signatures);
    out
}

/// The gas limit of a fulfil transaction: the intrinsic cost, the
/// precompile's base, calldata and per-signature charges, the callback bound
/// and what the precompile keeps to record the result.
#[must_use]
pub fn fulfil_gas_limit(calldata: &[u8], signatures: usize, callback_gas: u64) -> u64 {
    let length = u64::try_from(calldata.len()).unwrap_or(u64::MAX);
    let count = u64::try_from(signatures).unwrap_or(u64::MAX);
    TRANSACTION_BASE_GAS
        .saturating_add(CALLDATA_BYTE_GAS.saturating_mul(length))
        .saturating_add(PRECOMPILE_BASE_GAS)
        .saturating_add(CALLDATA_BYTE_GAS.saturating_mul(length.saturating_sub(4)))
        .saturating_add(SIGNATURE_GAS.saturating_mul(count))
        .saturating_add(callback_gas)
        .saturating_add(CALLBACK_RECORD_GAS)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fees {
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

/// One EIP-1559 transaction with no value and an empty access list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transaction {
    pub chain_id: u64,
    pub nonce: u64,
    pub fees: Fees,
    pub gas_limit: u64,
    pub to: [u8; 20],
    pub data: Vec<u8>,
}

/// A signed transaction: the raw type-2 envelope and its hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedTransaction {
    pub raw: Vec<u8>,
    pub hash: [u8; 32],
}

fn rlp_length(length: usize, short: u8, out: &mut Vec<u8>) -> Result<(), SubmitError> {
    if length < 56 {
        out.push(short + u8::try_from(length).map_err(|_| SubmitError::Encode)?);
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        out.push(short + 55 + u8::try_from(bytes.len() - skip).map_err(|_| SubmitError::Encode)?);
        out.extend_from_slice(&bytes[skip..]);
    }
    Ok(())
}

fn rlp_bytes(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), SubmitError> {
    if bytes.len() == 1 && bytes[0] < 128 {
        out.push(bytes[0]);
    } else {
        rlp_length(bytes.len(), 128, out)?;
        out.extend_from_slice(bytes);
    }
    Ok(())
}

fn integer(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), SubmitError> {
    let skip = bytes.iter().take_while(|byte| **byte == 0).count();
    rlp_bytes(&bytes[skip..], out)
}

fn list(payload: &[u8]) -> Result<Vec<u8>, SubmitError> {
    let mut out = Vec::new();
    rlp_length(payload.len(), 192, &mut out)?;
    out.extend_from_slice(payload);
    Ok(out)
}

impl Transaction {
    fn fields(&self) -> Result<Vec<u8>, SubmitError> {
        let mut payload = Vec::new();
        integer(&self.chain_id.to_be_bytes(), &mut payload)?;
        integer(&self.nonce.to_be_bytes(), &mut payload)?;
        integer(
            &self.fees.max_priority_fee_per_gas.to_be_bytes(),
            &mut payload,
        )?;
        integer(&self.fees.max_fee_per_gas.to_be_bytes(), &mut payload)?;
        integer(&self.gas_limit.to_be_bytes(), &mut payload)?;
        rlp_bytes(&self.to, &mut payload)?;
        integer(&[], &mut payload)?;
        rlp_bytes(&self.data, &mut payload)?;
        payload.push(192);
        Ok(payload)
    }

    /// Signs the transaction with `key`: the type byte and the RLP list of
    /// the fields, then the parity, `r` and `s`.
    ///
    /// # Errors
    /// Refuses a zero chain id or gas limit, a priority fee above the fee
    /// cap and a signature that does not recover the key.
    pub fn sign(&self, key: &SigningKey) -> Result<SignedTransaction, SubmitError> {
        if self.chain_id == 0
            || self.gas_limit == 0
            || self.fees.max_fee_per_gas == 0
            || self.fees.max_priority_fee_per_gas > self.fees.max_fee_per_gas
        {
            return Err(SubmitError::Encode);
        }
        let mut payload = self.fields()?;
        let unsigned = [vec![DYNAMIC_FEE_TYPE], list(&payload)?].concat();
        let signature = sign_digest(key, &keccak(&unsigned)).map_err(|_| SubmitError::Sign)?;
        integer(&[signature[64] - 27], &mut payload)?;
        integer(&signature[..32], &mut payload)?;
        integer(&signature[32..64], &mut payload)?;
        let raw = [vec![DYNAMIC_FEE_TYPE], list(&payload)?].concat();
        Ok(SignedTransaction {
            hash: keccak(&raw),
            raw,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitError {
    /// The EVM endpoint failed or answered out of shape.
    Evm(EvmError),
    /// The transaction could not be encoded.
    Encode,
    /// The transaction could not be signed.
    Sign,
    /// The journal could not be read or written.
    Journal,
    /// The precompile holds a request state this sidecar does not know.
    UnknownStatus(u8),
    /// The node answered the broadcast with another transaction hash.
    HashMismatch,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evm(error) => write!(f, "fulfil refused: {error}"),
            Self::Encode => f.write_str("fulfil refused: transaction not encodable"),
            Self::Sign => f.write_str("fulfil refused: transaction not signable"),
            Self::Journal => f.write_str("fulfil refused: journal unreadable or unwritable"),
            Self::UnknownStatus(status) => write!(f, "fulfil refused: request status {status}"),
            Self::HashMismatch => f.write_str("fulfil refused: node answered another hash"),
        }
    }
}

impl std::error::Error for SubmitError {}

impl From<EvmError> for SubmitError {
    fn from(error: EvmError) -> Self {
        Self::Evm(error)
    }
}

fn read_word(bytes: &[u8], index: usize) -> Option<&[u8]> {
    bytes.get(index.checked_mul(32)?..index.checked_add(1)?.checked_mul(32)?)
}

fn small(word: &[u8]) -> Option<u64> {
    let word: &[u8; 32] = word.try_into().ok()?;
    if word[..24].iter().any(|byte| *byte != 0) {
        return None;
    }
    let mut low = [0; 8];
    low.copy_from_slice(&word[24..]);
    Some(u64::from_be_bytes(low))
}

fn offset(bytes: &[u8], at: usize) -> Option<usize> {
    let value = usize::try_from(small(bytes.get(at..at.checked_add(32)?)?)?).ok()?;
    value.is_multiple_of(32).then_some(value)
}

/// The state of a request as `getRequest` reports it.
///
/// # Errors
/// Returns the endpoint's error and an answer that is not the request tuple.
pub fn request_status(rpc: &EvmRpc, request_id: u64) -> Result<u8, EvmError> {
    let mut data = selector(GET_REQUEST_SIGNATURE).to_vec();
    data.extend(word(request_id));
    let answer = rpc.eth_call(XWEB_PRECOMPILE, &data)?;
    if answer.len() != 9 * 32 || read_word(&answer, 0).and_then(small) != Some(request_id) {
        return Err(EvmError::Malformed);
    }
    read_word(&answer, 8)
        .and_then(small)
        .and_then(|status| u8::try_from(status).ok())
        .ok_or(EvmError::Malformed)
}

/// The registered attestor signers and the fulfil threshold as
/// `getAttestors` reports them.
///
/// # Errors
/// Returns the endpoint's error and an answer that does not decode.
pub fn attestor_set(rpc: &EvmRpc) -> Result<AttestorSet, EvmError> {
    let answer = rpc.eth_call(XWEB_PRECOMPILE, &selector(GET_ATTESTORS_SIGNATURE))?;
    decode_attestors(&answer).ok_or(EvmError::Malformed)
}

fn decode_attestors(answer: &[u8]) -> Option<AttestorSet> {
    let threshold = u32::try_from(small(read_word(answer, 1)?)?).ok()?;
    let array = offset(answer, 0)?;
    let count = usize::try_from(small(answer.get(array..array.checked_add(32)?)?)?).ok()?;
    let elements = array.checked_add(32)?;
    if count > answer.len() / 32 {
        return None;
    }
    let mut signers = Vec::with_capacity(count);
    for index in 0..count {
        let tuple = elements.checked_add(offset(answer, elements.checked_add(index * 32)?)?)?;
        let signer_word = answer.get(tuple..tuple.checked_add(32)?)?;
        if signer_word[..12].iter().any(|byte| *byte != 0) {
            return None;
        }
        let payout = tuple.checked_add(offset(answer, tuple.checked_add(32)?)?)?;
        let length = usize::try_from(small(answer.get(payout..payout.checked_add(32)?)?)?).ok()?;
        answer.get(payout.checked_add(32)?..payout.checked_add(32)?.checked_add(length)?)?;
        let mut signer = [0; 20];
        signer.copy_from_slice(&signer_word[12..]);
        signers.push(signer);
    }
    Some(AttestorSet { signers, threshold })
}

/// Where a request's fulfilment stands in the journal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalState {
    /// The signed transaction is journalled and may not be mined yet.
    Signed,
    /// This sidecar's fulfil transaction succeeded.
    Fulfilled,
    /// Another submitter fulfilled the request first.
    AlreadyFulfilled,
    /// The request was refunded before a fulfilment.
    Refunded,
}

impl JournalState {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Signed => "signed",
            Self::Fulfilled => "fulfilled",
            Self::AlreadyFulfilled => "already_fulfilled",
            Self::Refunded => "refunded",
        }
    }

    fn parse(code: &str) -> Option<Self> {
        [
            Self::Signed,
            Self::Fulfilled,
            Self::AlreadyFulfilled,
            Self::Refunded,
        ]
        .into_iter()
        .find(|state| state.code() == code)
    }

    /// Whether the request needs nothing more from this sidecar.
    #[must_use]
    pub const fn completed(self) -> bool {
        !matches!(self, Self::Signed)
    }
}

/// One journal entry: the request, its state and, once signed, the nonce,
/// the raw transaction and its hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JournalEntry {
    pub request_id: u64,
    pub state: JournalState,
    pub transaction: Option<(u64, SignedTransaction)>,
}

impl JournalEntry {
    fn to_json(&self) -> Value {
        let mut value = json!({
            "request_id": self.request_id,
            "state": self.state.code(),
        });
        if let (Some(object), Some((nonce, signed))) = (value.as_object_mut(), &self.transaction) {
            object.insert("nonce".to_owned(), json!(nonce));
            object.insert("raw".to_owned(), json!(hex0x(&signed.raw)));
            object.insert("hash".to_owned(), json!(hex0x(&signed.hash)));
        }
        value
    }

    fn from_json(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let known = ["request_id", "state", "nonce", "raw", "hash"];
        if object.keys().any(|key| !known.contains(&key.as_str())) {
            return None;
        }
        let state = JournalState::parse(object.get("state")?.as_str()?)?;
        let transaction = match (object.get("nonce"), object.get("raw"), object.get("hash")) {
            (Some(nonce), Some(raw), Some(hash)) => {
                let raw = unhex0x(raw.as_str()?)?;
                let hash: [u8; 32] = unhex0x(hash.as_str()?)?.try_into().ok()?;
                if keccak(&raw) != hash {
                    return None;
                }
                Some((nonce.as_u64()?, SignedTransaction { raw, hash }))
            }
            (None, None, None) => None,
            _ => return None,
        };
        if state == JournalState::Signed && transaction.is_none() {
            return None;
        }
        Some(Self {
            request_id: object.get("request_id")?.as_u64()?,
            state,
            transaction,
        })
    }
}

/// The submitter's journal: one file per request, written and synced before
/// any broadcast, so a restart rebroadcasts the same signed bytes.
pub struct Journal {
    directory: PathBuf,
}

impl Journal {
    /// # Errors
    /// Returns the error creating the directory.
    pub fn open(directory: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
        })
    }

    /// The file holding a request's entry.
    #[must_use]
    pub fn path(&self, request_id: u64) -> PathBuf {
        self.directory
            .join(format!("{request_id}.{JOURNAL_SUFFIX}"))
    }

    /// A request's entry.
    ///
    /// # Errors
    /// Refuses an unreadable file and one that is not a journal entry for
    /// the request.
    pub fn load(&self, request_id: u64) -> Result<Option<JournalEntry>, SubmitError> {
        let text = match std::fs::read(self.path(request_id)) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(SubmitError::Journal),
        };
        serde_json::from_slice(&text)
            .ok()
            .as_ref()
            .and_then(JournalEntry::from_json)
            .filter(|entry| entry.request_id == request_id)
            .map(Some)
            .ok_or(SubmitError::Journal)
    }

    /// Writes an entry to a temporary file, syncs it, renames it into place
    /// and syncs the directory.
    ///
    /// # Errors
    /// Returns a write, sync or rename that failed.
    pub fn store(&self, entry: &JournalEntry) -> Result<(), SubmitError> {
        let path = self.path(entry.request_id);
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, entry.to_json().to_string())
            .and_then(|()| std::fs::File::open(&temporary)?.sync_all())
            .and_then(|()| std::fs::rename(&temporary, &path))
            .and_then(|()| std::fs::File::open(&self.directory)?.sync_all())
            .map_err(|_| SubmitError::Journal)
    }

    fn remove(&self, request_id: u64) -> Result<(), SubmitError> {
        std::fs::remove_file(self.path(request_id)).map_err(|_| SubmitError::Journal)
    }

    /// Every request with a journalled entry, ascending.
    ///
    /// # Errors
    /// Returns an unreadable directory.
    pub fn requests(&self) -> Result<Vec<u64>, SubmitError> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.directory).map_err(|_| SubmitError::Journal)? {
            let name = entry.map_err(|_| SubmitError::Journal)?.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .and_then(|id| id.parse().ok())
            else {
                continue;
            };
            ids.push(id);
        }
        ids.sort_unstable();
        Ok(ids)
    }
}

/// What one submit or confirm step did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The signed fulfil transaction was broadcast and is not mined yet.
    Sent { hash: [u8; 32] },
    /// The fulfil transaction succeeded.
    Fulfilled,
    /// Another submitter fulfilled the request first: recorded as completed.
    AlreadyFulfilled,
    /// The request was refunded: nothing is submitted.
    Refunded,
    /// The fulfil transaction reverted while the request is still pending;
    /// the journal entry is dropped so the next round signs afresh.
    Reverted,
}

impl Outcome {
    /// Whether the request needs nothing more from this sidecar.
    #[must_use]
    pub const fn completed(self) -> bool {
        !matches!(self, Self::Sent { .. } | Self::Reverted)
    }
}

/// Each rebroadcast request with the result of its broadcast.
pub type Resumed = Vec<(u64, Result<Outcome, SubmitError>)>;

/// Posts fulfil through the precompile with the submitter key.
pub struct Submitter {
    rpc: EvmRpc,
    key: SigningKey,
    address: [u8; 20],
    chain_id: u64,
    journal: Journal,
}

impl Submitter {
    /// # Errors
    /// Returns the error opening the journal directory.
    pub fn open(
        rpc: EvmRpc,
        key: SigningKey,
        chain_id: u64,
        journal_dir: &Path,
    ) -> io::Result<Self> {
        Ok(Self {
            rpc,
            address: signer_address(&key),
            key,
            chain_id,
            journal: Journal::open(journal_dir)?,
        })
    }

    /// The address the submitter pays gas from.
    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }

    fn settle(&self, request_id: u64, state: JournalState) -> Result<Outcome, SubmitError> {
        self.journal.store(&JournalEntry {
            request_id,
            state,
            transaction: None,
        })?;
        Ok(match state {
            JournalState::Refunded => Outcome::Refunded,
            _ => Outcome::AlreadyFulfilled,
        })
    }

    /// Records a request the precompile no longer holds as pending.
    fn closed(&self, request_id: u64, status: u8) -> Result<Option<Outcome>, SubmitError> {
        match status {
            STATUS_PENDING => Ok(None),
            STATUS_FULFILLED => self
                .settle(request_id, JournalState::AlreadyFulfilled)
                .map(Some),
            STATUS_REFUNDED => self.settle(request_id, JournalState::Refunded).map(Some),
            other => Err(SubmitError::UnknownStatus(other)),
        }
    }

    fn broadcast(
        &self,
        request_id: u64,
        signed: &SignedTransaction,
    ) -> Result<Outcome, SubmitError> {
        match self
            .rpc
            .call("eth_sendRawTransaction", &json!([hex0x(&signed.raw)]))
        {
            Ok(answer) => {
                let hash = answer.as_str().and_then(unhex0x);
                if hash.as_deref() != Some(signed.hash.as_slice()) {
                    return Err(SubmitError::HashMismatch);
                }
                Ok(Outcome::Sent { hash: signed.hash })
            }
            Err(EvmError::Rejected { code }) => {
                let status = request_status(&self.rpc, request_id)?;
                match self.closed(request_id, status)? {
                    Some(outcome) => Ok(outcome),
                    None => Err(SubmitError::Evm(EvmError::Rejected { code })),
                }
            }
            Err(error) => Err(SubmitError::Evm(error)),
        }
    }

    /// Rebroadcasts every journalled transaction that is signed and not yet
    /// settled, byte for byte as it was signed.
    ///
    /// # Errors
    /// Returns an unreadable journal. Each request's own result is returned
    /// beside its id.
    pub fn resume(&self) -> Result<Resumed, SubmitError> {
        let mut results = Vec::new();
        for request_id in self.journal.requests()? {
            let Some(entry) = self.journal.load(request_id)? else {
                continue;
            };
            if let (JournalState::Signed, Some((_, signed))) = (entry.state, &entry.transaction) {
                results.push((request_id, self.broadcast(request_id, signed)));
            }
        }
        Ok(results)
    }

    fn fees(&self) -> Result<Fees, SubmitError> {
        let tip = self.rpc.quantity("eth_maxPriorityFeePerGas", &json!([]))?;
        let block = self
            .rpc
            .call("eth_getBlockByNumber", &json!(["latest", false]))?;
        let base = block
            .get("baseFeePerGas")
            .and_then(Value::as_str)
            .and_then(parse_quantity)
            .ok_or(EvmError::Malformed)?;
        let max_fee = base
            .checked_mul(2)
            .and_then(|doubled| doubled.checked_add(tip))
            .ok_or(EvmError::Malformed)?;
        Ok(Fees {
            max_fee_per_gas: max_fee,
            max_priority_fee_per_gas: tip,
        })
    }

    /// Posts fulfil for a request whose signatures reached the threshold.
    /// A journalled request is rebroadcast or left settled; a request the
    /// precompile already fulfilled is recorded as completed; otherwise the
    /// transaction is signed, journalled and synced, and only then
    /// broadcast.
    ///
    /// # Errors
    /// Refuses signatures out of ascending signer order and returns the
    /// endpoint, signing and journal failures.
    pub fn submit(&self, ready: &Ready) -> Result<Outcome, SubmitError> {
        if ready.signers.len() != ready.signatures.len()
            || ready.signers.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(SubmitError::Encode);
        }
        let request_id = ready.request_id;
        if let Some(entry) = self.journal.load(request_id)? {
            return match (entry.state, &entry.transaction) {
                (JournalState::Signed, Some((_, signed))) => self.broadcast(request_id, signed),
                (JournalState::Fulfilled, _) => Ok(Outcome::Fulfilled),
                (JournalState::Refunded, _) => Ok(Outcome::Refunded),
                _ => Ok(Outcome::AlreadyFulfilled),
            };
        }
        let status = request_status(&self.rpc, request_id)?;
        if let Some(outcome) = self.closed(request_id, status)? {
            return Ok(outcome);
        }
        let nonce = u64::try_from(self.rpc.quantity(
            "eth_getTransactionCount",
            &json!([hex0x(&self.address), "pending"]),
        )?)
        .map_err(|_| EvmError::Malformed)?;
        let fees = self.fees()?;
        let data = fulfil_calldata(ready);
        let transaction = Transaction {
            chain_id: self.chain_id,
            nonce,
            fees,
            gas_limit: fulfil_gas_limit(&data, ready.signatures.len(), ready.callback_gas),
            to: XWEB_PRECOMPILE,
            data,
        };
        let signed = transaction.sign(&self.key)?;
        self.journal.store(&JournalEntry {
            request_id,
            state: JournalState::Signed,
            transaction: Some((nonce, signed.clone())),
        })?;
        self.broadcast(request_id, &signed)
    }

    /// Reads the receipt of a journalled transaction. `None` means it is not
    /// mined yet or nothing is journalled.
    ///
    /// # Errors
    /// Returns the endpoint and journal failures.
    pub fn confirm(&self, request_id: u64) -> Result<Option<Outcome>, SubmitError> {
        let Some(entry) = self.journal.load(request_id)? else {
            return Ok(None);
        };
        let (JournalState::Signed, Some((nonce, signed))) = (entry.state, entry.transaction) else {
            return Ok(Some(match entry.state {
                JournalState::Fulfilled => Outcome::Fulfilled,
                JournalState::Refunded => Outcome::Refunded,
                _ => Outcome::AlreadyFulfilled,
            }));
        };
        let receipt = self
            .rpc
            .call("eth_getTransactionReceipt", &json!([hex0x(&signed.hash)]))?;
        if receipt.is_null() {
            return Ok(None);
        }
        match receipt.get("status").and_then(Value::as_str) {
            Some("0x1") => {
                self.journal.store(&JournalEntry {
                    request_id,
                    state: JournalState::Fulfilled,
                    transaction: Some((nonce, signed)),
                })?;
                Ok(Some(Outcome::Fulfilled))
            }
            Some("0x0") => {
                let status = request_status(&self.rpc, request_id)?;
                if let Some(outcome) = self.closed(request_id, status)? {
                    return Ok(Some(outcome));
                }
                self.journal.remove(request_id)?;
                Ok(Some(Outcome::Reverted))
            }
            _ => Err(SubmitError::Evm(EvmError::Malformed)),
        }
    }
}
