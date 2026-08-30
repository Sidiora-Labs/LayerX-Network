//! Proof-gated point reads and gap-free history pagination.

use std::cmp::Ordering;

use layerx_proof::inclusion::{verify_activity, InclusionError, SequencerAuthorization};
use layerx_proof::merkle::{MerkleError, Proof, MAX_DEPTH};
use layerx_proof::state::{decode_account_value, AccountProofError};
use layerx_types::amount::Amount;
use layerx_types::verify::VerificationLevel;
use layerx_wire::receipt::decode_batch_header;

use crate::evidence::{decode_nested_evidence, EvidenceError, RootSelector};
use crate::head::Head;
use crate::lni::refusal::decode_core_refusal;
use crate::lni::schema::{decode_envelope, encode_envelope, Envelope, SchemaError, Version};
use crate::lni::transport::{FrameTransport, TransportError};

const ACCOUNT_READ_REQUEST_TAG: u16 = 7;
const ACCOUNT_READ_RESPONSE_TAG: u16 = 8;
const ERROR_RESPONSE_TAG: u16 = 25;
const HISTORY_RANGE_REQUEST_TAG: u16 = 9;
const HISTORY_ITEM_TAG: u16 = 10;
const HISTORY_END_TAG: u16 = 11;
const MAX_SELECTOR_KEY_BYTES: usize = 4096;

/// A caller-requested evidence floor. Returned values carry only the level
/// actually established by local verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Requested(VerificationLevel);

impl Requested {
    #[must_use]
    pub const fn new(level: VerificationLevel) -> Self {
        Self(level)
    }

    #[must_use]
    pub const fn level(self) -> VerificationLevel {
        self.0
    }
}

/// Trusted client context for state and history proof verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReadContext {
    pub interface_version: Version,
    pub correlation_id: u64,
    pub expected_protocol_version: u16,
    pub expected_network_id: u32,
    pub requested: Requested,
    pub head: Head,
    pub sequencer_authorization: SequencerAuthorization,
    pub handshake_sequencer_key: [u8; 32],
    pub root_selector: RootSelector,
}

/// Freshness coordinates inseparable from a returned read value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Freshness {
    pub global_sequence: u64,
    pub batch_number: u64,
    pub observed_head_sequence: u64,
    pub observed_checkpoint: [u8; 32],
}

/// Exact protocol bytes with a locally achieved level and freshness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadValue {
    canonical_bytes: Vec<u8>,
    proof_material: Vec<u8>,
    achieved: VerificationLevel,
    freshness: Freshness,
}

impl ReadValue {
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    /// Returns the exact proof bytes independently verified for this value.
    #[must_use]
    pub fn proof_material(&self) -> &[u8] {
        &self.proof_material
    }

    #[must_use]
    pub const fn achieved(&self) -> VerificationLevel {
        self.achieved
    }

    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }
}

/// A core-produced balance leaf bound to its identity, freshness and achieved
/// verification level.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Balance {
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub amount: Amount,
    value: ReadValue,
}

impl Balance {
    #[must_use]
    pub const fn achieved(&self) -> VerificationLevel {
        self.value.achieved()
    }

    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.value.freshness()
    }

    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        self.value.canonical_bytes()
    }

    #[must_use]
    pub fn proof_material(&self) -> &[u8] {
        self.value.proof_material()
    }
}

/// A stable resume point bound to the head/checkpoint observed for the page.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HistoryCursor {
    next_sequence: u64,
    end_sequence: u64,
    head_sequence: u64,
    checkpoint: [u8; 32],
}

impl HistoryCursor {
    /// Reconstructs a cursor from durable daemon coordinates.
    #[must_use]
    pub const fn from_coordinates(
        next_sequence: u64,
        end_sequence: u64,
        head_sequence: u64,
        checkpoint: [u8; 32],
    ) -> Self {
        Self {
            next_sequence,
            end_sequence,
            head_sequence,
            checkpoint,
        }
    }

    #[must_use]
    pub const fn next_sequence(self) -> u64 {
        self.next_sequence
    }
}

/// Exact core history record class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryKind {
    Activity,
    Receipt,
    Event,
}

/// One ordered core-produced history byte string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryItem {
    pub global_sequence: u64,
    pub kind: HistoryKind,
    canonical_bytes: Vec<u8>,
    achieved: VerificationLevel,
}

impl HistoryItem {
    #[must_use]
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    #[must_use]
    pub const fn achieved(&self) -> VerificationLevel {
        self.achieved
    }
}

/// One bounded gap-free history page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub cursor: Option<HistoryCursor>,
}

/// Point-read selector whose bytes are LNI metadata, never a second protocol
/// representation.
#[derive(Clone, Debug, Eq, PartialEq)]
enum StateSelector {
    Balance { account: [u8; 32], asset: [u8; 32] },
    Account { account: [u8; 32] },
}

/// A typed refusal that names evidence shortfall and sequence discontinuity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadError {
    Transport(TransportError),
    Envelope(SchemaError),
    UnexpectedResponse,
    CoreRefusal {
        class: u8,
        result: layerx_types::result::ResultCode,
    },
    MalformedValue,
    SelectorMismatch,
    Evidence(MerkleError),
    Inclusion(InclusionError),
    ProductionEvidence(EvidenceError),
    Account(AccountProofError),
    MissingEvidence {
        requested: VerificationLevel,
        achieved: VerificationLevel,
    },
    HistoryGap {
        expected: u64,
        actual: u64,
    },
    HistoryRepetition {
        previous: u64,
        actual: u64,
    },
    PageBound,
    UnavailableCapability,
    Disconnected,
}

impl From<TransportError> for ReadError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<SchemaError> for ReadError {
    fn from(value: SchemaError) -> Self {
        Self::Envelope(value)
    }
}

/// Retrieves a balance leaf and verifies all evidence needed by the requested
/// level before returning it.
///
/// # Errors
///
/// Refuses malformed values, selector substitution, invalid proofs and missing
/// evidence rather than downgrading.
pub fn balance(
    transport: &mut dyn FrameTransport,
    account: [u8; 32],
    asset: [u8; 32],
    context: ReadContext,
) -> Result<Balance, ReadError> {
    let value = point_read(
        transport,
        &StateSelector::Balance { account, asset },
        context,
    )?;
    let canonical =
        decode_account_value(account, value.canonical_bytes()).map_err(ReadError::Account)?;
    if !canonical.has_asset || canonical.asset_id != asset {
        return Err(ReadError::SelectorMismatch);
    }
    Ok(Balance {
        account,
        asset,
        amount: Amount::from_u128(canonical.balance),
        value,
    })
}

/// Retrieves exact account-state bytes under the same proof policy as balance.
///
/// # Errors
///
/// Returns the complete point-read refusal set.
pub fn account(
    transport: &mut dyn FrameTransport,
    account: [u8; 32],
    context: ReadContext,
) -> Result<ReadValue, ReadError> {
    point_read(transport, &StateSelector::Account { account }, context)
}

/// Retrieves exact module-owned state bytes without interpreting them in the
/// client.
///
/// # Errors
///
/// Refuses an overlong selector and the complete point-read refusal set.
pub fn module_state(
    transport: &mut dyn FrameTransport,
    module_id: u16,
    key: &[u8],
    context: ReadContext,
) -> Result<ReadValue, ReadError> {
    if key.len() > MAX_SELECTOR_KEY_BYTES {
        return Err(ReadError::PageBound);
    }
    let _ = (transport, module_id, context);
    Err(ReadError::UnavailableCapability)
}

fn point_read(
    transport: &mut dyn FrameTransport,
    selector: &StateSelector,
    context: ReadContext,
) -> Result<ReadValue, ReadError> {
    let selector_bytes = encode_state_selector(selector, context.root_selector, context.requested)?;
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: ACCOUNT_READ_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &selector_bytes,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let response_bytes = transport.receive()?;
    let response = decode_envelope(&response_bytes)?;
    if response.version.major == context.interface_version.major
        && response.message_tag == ERROR_RESPONSE_TAG
        && response.correlation_id == context.correlation_id
        && response.proof_material.is_empty()
    {
        let refusal =
            decode_core_refusal(response.canonical_payload).ok_or(ReadError::UnexpectedResponse)?;
        return Err(ReadError::CoreRefusal {
            class: refusal.class,
            result: refusal.result,
        });
    }
    if response.version.major != context.interface_version.major
        || response.message_tag != ACCOUNT_READ_RESPONSE_TAG
        || response.correlation_id != context.correlation_id
    {
        return Err(ReadError::UnexpectedResponse);
    }
    verify_state_value(
        response.canonical_payload,
        response.proof_material,
        selector,
        context,
    )
}

fn verify_state_value(
    canonical_bytes: &[u8],
    proof_material: &[u8],
    selector: &StateSelector,
    context: ReadContext,
) -> Result<ReadValue, ReadError> {
    if proof_material.is_empty() {
        require_level(context.requested, VerificationLevel::UNVERIFIED)?;
        return Ok(ReadValue {
            canonical_bytes: canonical_bytes.to_vec(),
            proof_material: Vec::new(),
            achieved: VerificationLevel::UNVERIFIED,
            freshness: Freshness {
                global_sequence: context.head.chain_sequence,
                batch_number: context.head.sealed_batch,
                observed_head_sequence: context.head.chain_sequence,
                observed_checkpoint: context.head.finalised_checkpoint,
            },
        });
    }
    let (expected_account, expected_asset) = match selector {
        StateSelector::Balance { account, asset } => (*account, Some(*asset)),
        StateSelector::Account { account } => (*account, None),
    };
    let decoded = decode_nested_evidence(
        proof_material,
        context.expected_protocol_version,
        context.expected_network_id,
    )
    .map_err(ReadError::ProductionEvidence)?;
    if decoded.selector != context.root_selector
        || decoded.proof.account_id != expected_account
        || decoded.signed_header.public_key != context.handshake_sequencer_key
        || decoded.signed_header.response_authorization() != context.sequencer_authorization
    {
        return Err(ReadError::SelectorMismatch);
    }
    let header = decode_batch_header(&decoded.signed_header.canonical_bytes)
        .map_err(|_| ReadError::MalformedValue)?;
    if context.expected_protocol_version == 0
        || context.expected_network_id == 0
        || header.protocol_version() != context.expected_protocol_version
        || header.network_id() != context.expected_network_id
    {
        return Err(ReadError::SelectorMismatch);
    }
    match context.root_selector {
        RootSelector::Latest if header.batch_number() != context.head.sealed_batch => {
            return Err(ReadError::SelectorMismatch);
        }
        RootSelector::Latest | RootSelector::Batch(_) | RootSelector::Checkpoint(_) => {}
    }
    let verified = layerx_proof::state::verify_nested_account(
        canonical_bytes,
        expected_account,
        expected_asset,
        &decoded.proof,
        &context.sequencer_authorization,
    )
    .map_err(ReadError::Account)?;
    let achieved = if let Some(checkpoint) = decoded.checkpoint {
        checkpoint.report().level()
    } else {
        VerificationLevel::STATE_PROVEN
    };
    require_level(context.requested, achieved)?;
    Ok(ReadValue {
        canonical_bytes: canonical_bytes.to_vec(),
        proof_material: proof_material.to_vec(),
        achieved,
        freshness: Freshness {
            global_sequence: verified.observed_sequence(),
            batch_number: verified.header().header().batch_number(),
            observed_head_sequence: context.head.chain_sequence,
            observed_checkpoint: context.head.finalised_checkpoint,
        },
    })
}

fn require_level(requested: Requested, achieved: VerificationLevel) -> Result<(), ReadError> {
    if achieved.compare(requested.level()) == Ordering::Less {
        Err(ReadError::MissingEvidence {
            requested: requested.level(),
            achieved,
        })
    } else {
        Ok(())
    }
}

fn encode_state_selector(
    selector: &StateSelector,
    root_selector: RootSelector,
    requested: Requested,
) -> Result<Vec<u8>, ReadError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    match selector {
        StateSelector::Balance { account, asset } => {
            bytes.push(1);
            bytes.extend_from_slice(account);
            bytes.extend_from_slice(asset);
        }
        StateSelector::Account { account } => {
            bytes.push(2);
            bytes.extend_from_slice(account);
        }
    }
    root_selector.encode(&mut bytes);
    bytes.push(requested.level().wire_rank());
    Ok(bytes)
}

/// Retrieves one strictly ordered bounded history page.
///
/// # Errors
///
/// Refuses zero/excessive page bounds, cursor mismatch, gaps, repetitions,
/// malformed stream records, and any requested evidence level not established
/// locally for every returned item.
pub fn history(
    transport: &mut dyn FrameTransport,
    start_sequence: u64,
    end_sequence: u64,
    page_bound: u16,
    cursor: Option<HistoryCursor>,
    context: ReadContext,
) -> Result<HistoryPage, ReadError> {
    if page_bound == 0 || end_sequence < start_sequence {
        return Err(ReadError::PageBound);
    }
    let expected_start = if let Some(cursor) = cursor {
        if cursor.end_sequence != end_sequence
            || cursor.head_sequence != context.head.chain_sequence
            || cursor.checkpoint != context.head.finalised_checkpoint
            || cursor.next_sequence < start_sequence
        {
            return Err(ReadError::UnexpectedResponse);
        }
        cursor.next_sequence
    } else {
        start_sequence
    };
    let mut selector = Vec::with_capacity(28);
    selector.extend_from_slice(&expected_start.to_be_bytes());
    selector.extend_from_slice(&end_sequence.to_be_bytes());
    selector.extend_from_slice(&page_bound.to_be_bytes());
    selector.push(context.requested.level().wire_rank());
    let request = encode_envelope(Envelope {
        version: context.interface_version,
        message_tag: HISTORY_RANGE_REQUEST_TAG,
        correlation_id: context.correlation_id,
        canonical_payload: &selector,
        proof_material: &[],
    })?;
    transport.send(&request)?;
    let mut items = Vec::new();
    let mut expected = expected_start;
    loop {
        let response_bytes = transport.receive()?;
        let response = decode_envelope(&response_bytes)?;
        if response.version.major != context.interface_version.major
            || response.correlation_id != context.correlation_id
        {
            return Err(ReadError::UnexpectedResponse);
        }
        match response.message_tag {
            HISTORY_ITEM_TAG => {
                if items.len() >= usize::from(page_bound) {
                    return Err(ReadError::PageBound);
                }
                let (kind, sequence, evidence_bytes) = history_metadata(response.proof_material)?;
                if sequence < expected {
                    return Err(ReadError::HistoryRepetition {
                        previous: expected.saturating_sub(1),
                        actual: sequence,
                    });
                }
                if sequence > expected {
                    return Err(ReadError::HistoryGap {
                        expected,
                        actual: sequence,
                    });
                }
                let achieved =
                    verify_history_item(kind, response.canonical_payload, evidence_bytes, context)?;
                items.push(HistoryItem {
                    global_sequence: sequence,
                    kind,
                    canonical_bytes: response.canonical_payload.to_vec(),
                    achieved,
                });
                expected = expected.checked_add(1).ok_or(ReadError::PageBound)?;
            }
            HISTORY_END_TAG => {
                let next = decode_history_end(response.canonical_payload)?;
                if next != expected {
                    return Err(if next < expected {
                        ReadError::HistoryRepetition {
                            previous: expected.saturating_sub(1),
                            actual: next,
                        }
                    } else {
                        ReadError::HistoryGap {
                            expected,
                            actual: next,
                        }
                    });
                }
                let cursor = (next <= end_sequence).then_some(HistoryCursor {
                    next_sequence: next,
                    end_sequence,
                    head_sequence: context.head.chain_sequence,
                    checkpoint: context.head.finalised_checkpoint,
                });
                return Ok(HistoryPage { items, cursor });
            }
            _ => return Err(ReadError::UnexpectedResponse),
        }
    }
}

fn history_metadata(bytes: &[u8]) -> Result<(HistoryKind, u64, &[u8]), ReadError> {
    let (&kind, rest) = bytes.split_first().ok_or(ReadError::MalformedValue)?;
    let sequence_bytes: [u8; 8] = rest
        .get(..8)
        .ok_or(ReadError::MalformedValue)?
        .try_into()
        .map_err(|_| ReadError::MalformedValue)?;
    let kind = match kind {
        1 => HistoryKind::Activity,
        2 => HistoryKind::Receipt,
        3 => HistoryKind::Event,
        _ => return Err(ReadError::MalformedValue),
    };
    let evidence = rest.get(8..).ok_or(ReadError::MalformedValue)?;
    Ok((kind, u64::from_be_bytes(sequence_bytes), evidence))
}

fn verify_history_item(
    kind: HistoryKind,
    bytes: &[u8],
    proof_material: &[u8],
    context: ReadContext,
) -> Result<VerificationLevel, ReadError> {
    if proof_material.is_empty() {
        require_level(context.requested, VerificationLevel::UNVERIFIED)?;
        return Ok(VerificationLevel::UNVERIFIED);
    }
    if kind != HistoryKind::Activity {
        return Err(ReadError::MissingEvidence {
            requested: context.requested.level(),
            achieved: VerificationLevel::UNVERIFIED,
        });
    }
    let bundle = ProofBundle::decode(proof_material)?;
    let evidence = verify_activity(
        bytes,
        &bundle.proof,
        &bundle.header,
        &bundle.header_signature,
        &context.sequencer_authorization,
    )
    .map_err(ReadError::Inclusion)?;
    let achieved = evidence.level();
    require_level(context.requested, achieved)?;
    Ok(achieved)
}

fn decode_history_end(bytes: &[u8]) -> Result<u64, ReadError> {
    let encoded: [u8; 8] = bytes.try_into().map_err(|_| ReadError::MalformedValue)?;
    Ok(u64::from_be_bytes(encoded))
}

struct ProofBundle {
    proof: Proof,
    header: Vec<u8>,
    header_signature: [u8; 64],
}

impl ProofBundle {
    fn decode(bytes: &[u8]) -> Result<Self, ReadError> {
        let mut reader = Reader::new(bytes);
        let _asserted_level = reader.u8()?;
        let _root: [u8; 32] = reader.array()?;
        let leaf_index = reader.u32()?;
        let leaf_count = reader.u32()?;
        let sibling_count = usize::from(reader.u8()?);
        if sibling_count > MAX_DEPTH {
            return Err(ReadError::MalformedValue);
        }
        let mut siblings = Vec::with_capacity(sibling_count);
        for _ in 0..sibling_count {
            siblings.push(reader.array()?);
        }
        let header_length =
            usize::try_from(reader.u32()?).map_err(|_| ReadError::MalformedValue)?;
        let header = reader.bytes(header_length)?.to_vec();
        let header_signature = reader.array()?;
        reader.finish()?;
        let proof = Proof::new(leaf_index, leaf_count, siblings).map_err(ReadError::Evidence)?;
        Ok(Self {
            proof,
            header,
            header_signature,
        })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], ReadError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(ReadError::MalformedValue)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ReadError::MalformedValue)?;
        self.offset = end;
        Ok(value)
    }

    fn array<const LENGTH: usize>(&mut self) -> Result<[u8; LENGTH], ReadError> {
        self.bytes(LENGTH)?
            .try_into()
            .map_err(|_| ReadError::MalformedValue)
    }

    fn u8(&mut self) -> Result<u8, ReadError> {
        self.array().map(u8::from_be_bytes)
    }

    fn u32(&mut self) -> Result<u32, ReadError> {
        self.array().map(u32::from_be_bytes)
    }

    fn finish(self) -> Result<(), ReadError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ReadError::MalformedValue)
        }
    }
}
