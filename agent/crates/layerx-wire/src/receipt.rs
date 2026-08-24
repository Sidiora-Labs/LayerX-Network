//! Canonical receipt, batch-header, checkpoint, and Merkle-proof codecs.

use layerx_types::result::KnownResult;

use crate::decode::Decoder;
use crate::encode::Encoder;
use crate::limits::MAX_MESSAGE_BYTES;
use crate::WireError;

const RECEIPT_TAG: u16 = 0x5201;
const BATCH_TAG: u16 = 0x1701;
const BATCH_FIELDS: u8 = 15;
const PROOF_TAG: u16 = 0x4d50;
const MAX_EFFECTS: usize = 512;
const MAX_EFFECT_BODY: usize = 256;
const MAX_PROOF_DEPTH: usize = 32;
const BATCH_ENCODED_BYTES: usize = 354;
const REPLAY_RECEIPT_BYTES: usize = 106;

/// One canonical receipt effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Effect {
    module_id: u16,
    ordinal: u16,
    event_type: u16,
    kind: u8,
    monetary: bool,
    transfer_set_root: [u8; 32],
    body: Vec<u8>,
}

/// Full canonical core receipt, including the 402LXP evidence fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolReceipt {
    protocol_version: u16,
    activity_id: [u8; 32],
    global_sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    activity_root: [u8; 32],
    result_code: i32,
    effects: Vec<Effect>,
    fee_charged: u128,
    batch_id: [u8; 32],
    module_id: u16,
    module_version: u32,
    parameter_version: u32,
    operation: u8,
    asset: [u8; 32],
    amount: u128,
    from: [u8; 32],
    from_balance_before: u128,
    from_balance_after: u128,
    from_sequence: u64,
    to: [u8; 32],
    to_balance_before: u128,
    to_balance_after: u128,
    transfer_set_root: [u8; 32],
    authorization_hash: [u8; 32],
    context_hash: [u8; 32],
    timestamp: u64,
    sequencer_signature: Option<[u8; 64]>,
}

impl ProtocolReceipt {
    /// Returns the receipt protocol version.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the activity identifier asserted by core.
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    /// Returns the global sequence assigned by core.
    #[must_use]
    pub const fn global_sequence(&self) -> u64 {
        self.global_sequence
    }

    /// Returns the state root before execution.
    #[must_use]
    pub const fn previous_state_root(&self) -> [u8; 32] {
        self.previous_state_root
    }

    /// Returns the state root after execution.
    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }

    /// Returns the exact protocol result code.
    #[must_use]
    pub const fn result_code(&self) -> i32 {
        self.result_code
    }

    /// Returns the batch identifier carrying this receipt.
    #[must_use]
    pub const fn batch_id(&self) -> [u8; 32] {
        self.batch_id
    }

    /// Returns the protocol module that executed the operation.
    #[must_use]
    pub const fn module_id(&self) -> u16 {
        self.module_id
    }

    /// Returns the executing module's exact semantic version.
    #[must_use]
    pub const fn module_version(&self) -> u32 {
        self.module_version
    }

    /// Returns the parameter-set version used for execution.
    #[must_use]
    pub const fn parameter_version(&self) -> u32 {
        self.parameter_version
    }

    /// Returns the ledger operation tag.
    #[must_use]
    pub const fn operation(&self) -> u8 {
        self.operation
    }

    /// Returns the one asset shared by both receipt legs.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    /// Returns the exact transfer amount.
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    /// Returns the exact fee charged by protocol execution.
    #[must_use]
    pub const fn fee_charged(&self) -> u128 {
        self.fee_charged
    }

    /// Returns the debit account.
    #[must_use]
    pub const fn from(&self) -> [u8; 32] {
        self.from
    }

    /// Returns the debit balance before execution.
    #[must_use]
    pub const fn debit_balance_before(&self) -> u128 {
        self.from_balance_before
    }

    /// Returns the debit balance after execution.
    #[must_use]
    pub const fn debit_balance_after(&self) -> u128 {
        self.from_balance_after
    }

    /// Returns the credit account.
    #[must_use]
    pub const fn to(&self) -> [u8; 32] {
        self.to
    }

    /// Returns the credit balance before execution.
    #[must_use]
    pub const fn credit_balance_before(&self) -> u128 {
        self.to_balance_before
    }

    /// Returns the credit balance after execution.
    #[must_use]
    pub const fn credit_balance_after(&self) -> u128 {
        self.to_balance_after
    }

    /// Returns the authority commitment consumed by core.
    #[must_use]
    pub const fn authorization_hash(&self) -> [u8; 32] {
        self.authorization_hash
    }

    /// Returns the application context commitment consumed by core.
    #[must_use]
    pub const fn context_hash(&self) -> [u8; 32] {
        self.context_hash
    }

    /// Returns the sequencer signature when one was encoded.
    #[must_use]
    pub const fn sequencer_signature(&self) -> Option<[u8; 64]> {
        self.sequencer_signature
    }
}

/// Compact activity receipt carried by the published replay corpus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayReceipt {
    protocol_version: u16,
    global_sequence: u64,
    activity_id: [u8; 32],
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
}

/// A receipt format accepted from one of the published protocol corpora.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Receipt {
    /// Full signed or unsigned core receipt.
    Protocol(Box<ProtocolReceipt>),
    /// Replay-corpus activity receipt.
    Replay(ReplayReceipt),
}

impl Receipt {
    /// Borrows the full protocol receipt, rejecting compact replay records.
    #[must_use]
    pub fn protocol(&self) -> Option<&ProtocolReceipt> {
        match self {
            Self::Protocol(receipt) => Some(receipt),
            Self::Replay(_) => None,
        }
    }
}

fn fixed_array<const N: usize>(decoder: &mut Decoder<'_>) -> Result<[u8; N], WireError> {
    let offset = decoder.offset();
    decoder
        .fixed(N)?
        .try_into()
        .map_err(|_| WireError::known(KnownResult::NonCanonical, offset))
}

fn bounded_array<const N: usize>(decoder: &mut Decoder<'_>) -> Result<[u8; N], WireError> {
    let offset = decoder.offset();
    decoder
        .bytes(N)?
        .try_into()
        .map_err(|_| WireError::known(KnownResult::NonCanonical, offset))
}

fn decode_effect(decoder: &mut Decoder<'_>) -> Result<Effect, WireError> {
    let module_id = decoder.u16()?;
    let ordinal = decoder.u16()?;
    let event_type = decoder.u16()?;
    let kind = decoder.tag(3)?;
    if kind == 0 {
        return Err(WireError::known(KnownResult::InvalidTag, decoder.offset()));
    }
    let monetary = match decoder.u8()? {
        0 => false,
        1 => true,
        _ => {
            return Err(WireError::known(
                KnownResult::NonCanonical,
                decoder.offset(),
            ))
        }
    };
    if monetary && kind != 2 {
        return Err(WireError::known(
            KnownResult::FatalInvariant,
            decoder.offset(),
        ));
    }
    let transfer_set_root = bounded_array(decoder)?;
    let body = decoder.bytes_owned(MAX_EFFECT_BODY)?;
    Ok(Effect {
        module_id,
        ordinal,
        event_type,
        kind,
        monetary,
        transfer_set_root,
        body,
    })
}

fn decode_protocol(bytes: &[u8]) -> Result<Receipt, WireError> {
    let mut decoder = Decoder::new(bytes, MAX_MESSAGE_BYTES);
    decoder.structure_header(RECEIPT_TAG)?;
    let protocol_version = decoder.u16()?;
    if protocol_version != 1 {
        return Err(WireError::known(
            KnownResult::VersionUnsupported,
            decoder.offset(),
        ));
    }
    let activity_id = bounded_array(&mut decoder)?;
    let global_sequence = decoder.u64()?;
    let previous_state_root = bounded_array(&mut decoder)?;
    let resulting_state_root = bounded_array(&mut decoder)?;
    let activity_root = bounded_array(&mut decoder)?;
    let result_code = decoder.i32()?;
    let effect_count = decoder.sequence_length(MAX_EFFECTS)?;
    let mut effects = Vec::with_capacity(effect_count);
    for _ in 0..effect_count {
        effects.push(decode_effect(&mut decoder)?);
    }
    let fee_charged = decoder.u128()?;
    let batch_id = bounded_array(&mut decoder)?;
    let module_id = decoder.u16()?;
    let module_version = decoder.u32()?;
    let parameter_version = decoder.u32()?;
    let operation = decoder.u8()?;
    let asset = bounded_array(&mut decoder)?;
    let amount = decoder.u128()?;
    let from = bounded_array(&mut decoder)?;
    let from_balance_before = decoder.u128()?;
    let from_balance_after = decoder.u128()?;
    let from_sequence = decoder.u64()?;
    let to = bounded_array(&mut decoder)?;
    let to_balance_before = decoder.u128()?;
    let to_balance_after = decoder.u128()?;
    let transfer_set_root = bounded_array(&mut decoder)?;
    let authorization_hash = bounded_array(&mut decoder)?;
    let context_hash = bounded_array(&mut decoder)?;
    let timestamp = decoder.u64()?;
    let sequencer_signature = match decoder.u8()? {
        0 => None,
        1 => Some(bounded_array(&mut decoder)?),
        _ => {
            return Err(WireError::known(
                KnownResult::NonCanonical,
                decoder.offset(),
            ))
        }
    };
    decoder.finish()?;
    Ok(Receipt::Protocol(Box::new(ProtocolReceipt {
        protocol_version,
        activity_id,
        global_sequence,
        previous_state_root,
        resulting_state_root,
        activity_root,
        result_code,
        effects,
        fee_charged,
        batch_id,
        module_id,
        module_version,
        parameter_version,
        operation,
        asset,
        amount,
        from,
        from_balance_before,
        from_balance_after,
        from_sequence,
        to,
        to_balance_before,
        to_balance_after,
        transfer_set_root,
        authorization_hash,
        context_hash,
        timestamp,
        sequencer_signature,
    })))
}

fn decode_replay(bytes: &[u8]) -> Result<Receipt, WireError> {
    if bytes.len() != REPLAY_RECEIPT_BYTES {
        return Err(WireError::known(KnownResult::NonCanonical, 0));
    }
    let mut decoder = Decoder::new(bytes, 0);
    let protocol_version = decoder.u16()?;
    if protocol_version != 1 {
        return Err(WireError::known(KnownResult::VersionUnsupported, 0));
    }
    let global_sequence = decoder.u64()?;
    let activity_id = fixed_array(&mut decoder)?;
    let previous_state_root = fixed_array(&mut decoder)?;
    let resulting_state_root = fixed_array(&mut decoder)?;
    decoder.finish()?;
    Ok(Receipt::Replay(ReplayReceipt {
        protocol_version,
        global_sequence,
        activity_id,
        previous_state_root,
        resulting_state_root,
    }))
}

/// Decodes a full core receipt or the published replay-corpus receipt shape.
///
/// # Errors
///
/// Returns a typed canonical rejection with no panic path.
pub fn decode(bytes: &[u8]) -> Result<Receipt, WireError> {
    if bytes.len() >= 4 && bytes[..4] == [0, 1, 0x52, 1] {
        decode_protocol(bytes)
    } else {
        decode_replay(bytes)
    }
}

fn encode_effect(encoder: &mut Encoder, effect: &Effect) -> Result<(), WireError> {
    encoder.u16(effect.module_id)?;
    encoder.u16(effect.ordinal)?;
    encoder.u16(effect.event_type)?;
    encoder.tag(effect.kind, 3)?;
    encoder.u8(u8::from(effect.monetary))?;
    encoder.bytes(&effect.transfer_set_root, 32)?;
    encoder.bytes(&effect.body, MAX_EFFECT_BODY)
}

fn encode_protocol(receipt: &ProtocolReceipt) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder::new(MAX_MESSAGE_BYTES);
    encoder.structure_header(RECEIPT_TAG)?;
    encoder.u16(receipt.protocol_version)?;
    encoder.bytes(&receipt.activity_id, 32)?;
    encoder.u64(receipt.global_sequence)?;
    encoder.bytes(&receipt.previous_state_root, 32)?;
    encoder.bytes(&receipt.resulting_state_root, 32)?;
    encoder.bytes(&receipt.activity_root, 32)?;
    encoder.i32(receipt.result_code)?;
    encoder.sequence_length(receipt.effects.len(), MAX_EFFECTS)?;
    for effect in &receipt.effects {
        encode_effect(&mut encoder, effect)?;
    }
    encoder.u128(receipt.fee_charged)?;
    encoder.bytes(&receipt.batch_id, 32)?;
    encoder.u16(receipt.module_id)?;
    encoder.u32(receipt.module_version)?;
    encoder.u32(receipt.parameter_version)?;
    encoder.u8(receipt.operation)?;
    encoder.bytes(&receipt.asset, 32)?;
    encoder.u128(receipt.amount)?;
    encoder.bytes(&receipt.from, 32)?;
    encoder.u128(receipt.from_balance_before)?;
    encoder.u128(receipt.from_balance_after)?;
    encoder.u64(receipt.from_sequence)?;
    encoder.bytes(&receipt.to, 32)?;
    encoder.u128(receipt.to_balance_before)?;
    encoder.u128(receipt.to_balance_after)?;
    encoder.bytes(&receipt.transfer_set_root, 32)?;
    encoder.bytes(&receipt.authorization_hash, 32)?;
    encoder.bytes(&receipt.context_hash, 32)?;
    encoder.u64(receipt.timestamp)?;
    encoder.u8(u8::from(receipt.sequencer_signature.is_some()))?;
    if let Some(signature) = receipt.sequencer_signature {
        encoder.bytes(&signature, 64)?;
    }
    Ok(encoder.finish())
}

/// Re-encodes a successfully decoded receipt byte-exactly.
///
/// # Errors
///
/// Returns a typed bound error if a decoded collection exceeds the protocol
/// limit.
pub fn encode(receipt: &Receipt) -> Result<Vec<u8>, WireError> {
    match receipt {
        Receipt::Protocol(receipt) => encode_protocol(receipt),
        Receipt::Replay(receipt) => {
            let mut encoder = Encoder::new(REPLAY_RECEIPT_BYTES);
            encoder.u16(receipt.protocol_version)?;
            encoder.u64(receipt.global_sequence)?;
            encoder.fixed(&receipt.activity_id)?;
            encoder.fixed(&receipt.previous_state_root)?;
            encoder.fixed(&receipt.resulting_state_root)?;
            Ok(encoder.finish())
        }
    }
}

/// Re-encodes the exact receipt signing preimage with the signature absent.
///
/// # Errors
///
/// Rejects compact replay records because they are not signed receipt shapes,
/// and returns canonical bound failures from the encoder.
pub fn encode_unsigned(receipt: &Receipt) -> Result<Vec<u8>, WireError> {
    match receipt {
        Receipt::Protocol(receipt) => {
            let mut unsigned = receipt.as_ref().clone();
            unsigned.sequencer_signature = None;
            encode_protocol(&unsigned)
        }
        Receipt::Replay(_) => Err(WireError::known(KnownResult::NonCanonical, 0)),
    }
}

/// Exact canonical batch header with fifteen commitments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchHeader {
    protocol_version: u16,
    network_id: u32,
    epoch: u64,
    batch_number: u64,
    first_sequence: u64,
    last_sequence: u64,
    previous_state_root: [u8; 32],
    resulting_state_root: [u8; 32],
    activity_merkle_root: [u8; 32],
    receipt_merkle_root: [u8; 32],
    event_merkle_root: [u8; 32],
    data_availability_root: [u8; 32],
    oracle_root: [u8; 32],
    timestamp_ms: u64,
    sequencer_id: [u8; 32],
}

impl BatchHeader {
    /// Returns the protocol version committed by the batch.
    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    /// Returns the node network identifier.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Returns the monotonically assigned batch number.
    #[must_use]
    pub const fn batch_number(&self) -> u64 {
        self.batch_number
    }

    /// Returns the first global sequence committed by the batch.
    #[must_use]
    pub const fn first_sequence(&self) -> u64 {
        self.first_sequence
    }

    /// Returns the last global sequence committed by the batch.
    #[must_use]
    pub const fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    /// Returns the previous state-root commitment.
    #[must_use]
    pub const fn previous_state_root(&self) -> [u8; 32] {
        self.previous_state_root
    }

    /// Returns the resulting state-root commitment.
    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }

    /// Returns the activity Merkle-root commitment.
    #[must_use]
    pub const fn activity_merkle_root(&self) -> [u8; 32] {
        self.activity_merkle_root
    }

    /// Returns the receipt Merkle-root commitment.
    #[must_use]
    pub const fn receipt_merkle_root(&self) -> [u8; 32] {
        self.receipt_merkle_root
    }

    /// Returns the event Merkle-root commitment.
    #[must_use]
    pub const fn event_merkle_root(&self) -> [u8; 32] {
        self.event_merkle_root
    }

    /// Returns the data-availability-root commitment.
    #[must_use]
    pub const fn data_availability_root(&self) -> [u8; 32] {
        self.data_availability_root
    }

    /// Returns the oracle-root commitment.
    #[must_use]
    pub const fn oracle_root(&self) -> [u8; 32] {
        self.oracle_root
    }

    /// Returns the sequencer identity committed by the header.
    #[must_use]
    pub const fn sequencer_id(&self) -> [u8; 32] {
        self.sequencer_id
    }
}

fn batch_field(decoder: &mut Decoder<'_>, expected: u8) -> Result<(), WireError> {
    let offset = decoder.offset();
    let actual = decoder
        .tag(BATCH_FIELDS)
        .map_err(|_| WireError::known(KnownResult::UnknownField, offset))?;
    if actual != expected {
        return Err(WireError::known(KnownResult::UnknownField, offset));
    }
    Ok(())
}

/// Decodes a batch header and its exact field order.
///
/// # Errors
///
/// Rejects unknown versions, fields, non-exact digests, and trailing bytes.
pub fn decode_batch_header(bytes: &[u8]) -> Result<BatchHeader, WireError> {
    if bytes.len() > BATCH_ENCODED_BYTES {
        return Err(WireError::known(
            KnownResult::TrailingBytes,
            BATCH_ENCODED_BYTES,
        ));
    }
    let mut decoder = Decoder::new(bytes, 0);
    decoder.structure_header(BATCH_TAG)?;
    if decoder.u8()? != BATCH_FIELDS {
        return Err(WireError::known(
            KnownResult::NonCanonical,
            decoder.offset(),
        ));
    }
    batch_field(&mut decoder, 1)?;
    let protocol_version = decoder.u16()?;
    batch_field(&mut decoder, 2)?;
    let network_id = decoder.u32()?;
    batch_field(&mut decoder, 3)?;
    let epoch = decoder.u64()?;
    batch_field(&mut decoder, 4)?;
    let batch_number = decoder.u64()?;
    batch_field(&mut decoder, 5)?;
    let first_sequence = decoder.u64()?;
    batch_field(&mut decoder, 6)?;
    let last_sequence = decoder.u64()?;
    batch_field(&mut decoder, 7)?;
    let previous_state_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 8)?;
    let resulting_state_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 9)?;
    let activity_merkle_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 10)?;
    let receipt_merkle_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 11)?;
    let event_merkle_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 12)?;
    let data_availability_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 13)?;
    let oracle_root = bounded_array(&mut decoder)?;
    batch_field(&mut decoder, 14)?;
    let timestamp_ms = decoder.u64()?;
    batch_field(&mut decoder, 15)?;
    let sequencer_id = bounded_array(&mut decoder)?;
    decoder.finish()?;
    Ok(BatchHeader {
        protocol_version,
        network_id,
        epoch,
        batch_number,
        first_sequence,
        last_sequence,
        previous_state_root,
        resulting_state_root,
        activity_merkle_root,
        receipt_merkle_root,
        event_merkle_root,
        data_availability_root,
        oracle_root,
        timestamp_ms,
        sequencer_id,
    })
}

/// Encodes the exact 354-byte batch header.
///
/// # Errors
///
/// Returns typed codec errors and a fatal invariant if width diverges.
pub fn encode_batch_header(header: &BatchHeader) -> Result<Vec<u8>, WireError> {
    let mut encoder = Encoder::new(BATCH_ENCODED_BYTES);
    encoder.structure_header(BATCH_TAG)?;
    encoder.u8(BATCH_FIELDS)?;
    macro_rules! field {
        ($id:expr, $value:expr) => {{
            encoder.tag($id, BATCH_FIELDS)?;
            $value?;
        }};
    }
    field!(1, encoder.u16(header.protocol_version));
    field!(2, encoder.u32(header.network_id));
    field!(3, encoder.u64(header.epoch));
    field!(4, encoder.u64(header.batch_number));
    field!(5, encoder.u64(header.first_sequence));
    field!(6, encoder.u64(header.last_sequence));
    field!(7, encoder.bytes(&header.previous_state_root, 32));
    field!(8, encoder.bytes(&header.resulting_state_root, 32));
    field!(9, encoder.bytes(&header.activity_merkle_root, 32));
    field!(10, encoder.bytes(&header.receipt_merkle_root, 32));
    field!(11, encoder.bytes(&header.event_merkle_root, 32));
    field!(12, encoder.bytes(&header.data_availability_root, 32));
    field!(13, encoder.bytes(&header.oracle_root, 32));
    field!(14, encoder.u64(header.timestamp_ms));
    field!(15, encoder.bytes(&header.sequencer_id, 32));
    let bytes = encoder.finish();
    if bytes.len() != BATCH_ENCODED_BYTES {
        return Err(WireError::known(KnownResult::FatalInvariant, bytes.len()));
    }
    Ok(bytes)
}

/// Canonical Merkle proof fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MerkleProof {
    leaf_index: u32,
    leaf_count: u32,
    siblings: Vec<[u8; 32]>,
}

fn proof_depth(mut count: u32) -> usize {
    let mut depth = 0;
    while count > 1 {
        count = count.div_ceil(2);
        depth += 1;
    }
    depth
}

/// Decodes the canonical Merkle proof structure.
///
/// # Errors
///
/// Rejects invalid leaf bounds, depth, sibling width, and trailing bytes.
pub fn decode_merkle_proof(bytes: &[u8]) -> Result<MerkleProof, WireError> {
    let mut decoder = Decoder::new(bytes, MAX_PROOF_DEPTH * 32);
    decoder.structure_header(PROOF_TAG)?;
    let leaf_index = decoder.u32()?;
    let leaf_count = decoder.u32()?;
    let depth = usize::from(decoder.u8()?);
    if leaf_count == 0
        || leaf_index >= leaf_count
        || depth > MAX_PROOF_DEPTH
        || depth != proof_depth(leaf_count)
    {
        return Err(WireError::known(
            KnownResult::NonCanonical,
            decoder.offset(),
        ));
    }
    let sibling_bytes = decoder.bytes_owned(MAX_PROOF_DEPTH * 32)?;
    if sibling_bytes.len() != depth * 32 {
        return Err(WireError::known(
            KnownResult::NonCanonical,
            decoder.offset(),
        ));
    }
    let mut siblings = Vec::with_capacity(depth);
    for sibling in sibling_bytes.chunks_exact(32) {
        let value: [u8; 32] = sibling
            .try_into()
            .map_err(|_| WireError::known(KnownResult::NonCanonical, decoder.offset()))?;
        siblings.push(value);
    }
    decoder.finish()?;
    Ok(MerkleProof {
        leaf_index,
        leaf_count,
        siblings,
    })
}

/// Encodes a canonical Merkle proof.
///
/// # Errors
///
/// Rejects inconsistent leaf bounds or proof depth.
pub fn encode_merkle_proof(proof: &MerkleProof) -> Result<Vec<u8>, WireError> {
    if proof.leaf_count == 0
        || proof.leaf_index >= proof.leaf_count
        || proof.siblings.len() != proof_depth(proof.leaf_count)
        || proof.siblings.len() > MAX_PROOF_DEPTH
    {
        return Err(WireError::known(KnownResult::NonCanonical, 0));
    }
    let mut encoder = Encoder::new(4 + 4 + 4 + 1 + 4 + MAX_PROOF_DEPTH * 32);
    encoder.structure_header(PROOF_TAG)?;
    encoder.u32(proof.leaf_index)?;
    encoder.u32(proof.leaf_count)?;
    encoder.u8(u8::try_from(proof.siblings.len())
        .map_err(|_| WireError::known(KnownResult::LengthLimit, 0))?)?;
    let mut siblings = Vec::with_capacity(proof.siblings.len() * 32);
    for sibling in &proof.siblings {
        siblings.extend_from_slice(sibling);
    }
    encoder.bytes(&siblings, MAX_PROOF_DEPTH * 32)?;
    Ok(encoder.finish())
}

/// Canonical checkpoint certificate material used by the guarantor layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointCertificate {
    header: BatchHeader,
    validity_proof: Vec<u8>,
    guarantor_signatures: Vec<([u8; 32], [u8; 64])>,
    threshold: u32,
    settlement_reference: Vec<u8>,
}

/// Decodes the canonical checkpoint body, sorted signature set, threshold, and
/// settlement reference.
///
/// # Errors
///
/// Rejects invalid batch bytes, unsorted guarantors, threshold violations, and
/// exceeded proof/reference bounds.
pub fn decode_checkpoint(bytes: &[u8]) -> Result<CheckpointCertificate, WireError> {
    let mut decoder = Decoder::new(bytes, MAX_MESSAGE_BYTES + 1024);
    let header_bytes = decoder.fixed(BATCH_ENCODED_BYTES)?;
    let header = decode_batch_header(header_bytes)?;
    let validity_proof = decoder.bytes_owned(MAX_MESSAGE_BYTES)?;
    let signature_count = decoder.sequence_length(32)?;
    let mut guarantor_signatures = Vec::with_capacity(signature_count);
    let mut previous: Option<[u8; 32]> = None;
    for _ in 0..signature_count {
        let guarantor_id = bounded_array(&mut decoder)?;
        if previous.is_some_and(|value| value >= guarantor_id) {
            return Err(WireError::known(
                KnownResult::UnsortedSequence,
                decoder.offset(),
            ));
        }
        let signature = bounded_array(&mut decoder)?;
        previous = Some(guarantor_id);
        guarantor_signatures.push((guarantor_id, signature));
    }
    let threshold = decoder.u32()?;
    if threshold == 0 || usize::try_from(threshold).map_or(true, |value| value > signature_count) {
        return Err(WireError::known(
            KnownResult::AttestationThreshold,
            decoder.offset(),
        ));
    }
    let settlement_reference = decoder.bytes_owned(1024)?;
    decoder.finish()?;
    Ok(CheckpointCertificate {
        header,
        validity_proof,
        guarantor_signatures,
        threshold,
        settlement_reference,
    })
}

/// Encodes checkpoint certificate material byte-exactly.
///
/// # Errors
///
/// Rejects invalid thresholds, ordering, and bounds.
pub fn encode_checkpoint(certificate: &CheckpointCertificate) -> Result<Vec<u8>, WireError> {
    let threshold = usize::try_from(certificate.threshold)
        .map_err(|_| WireError::known(KnownResult::AttestationThreshold, 0))?;
    if threshold == 0 || threshold > certificate.guarantor_signatures.len() {
        return Err(WireError::known(KnownResult::AttestationThreshold, 0));
    }
    let keys: Vec<_> = certificate
        .guarantor_signatures
        .iter()
        .map(|(identifier, _)| identifier.as_slice())
        .collect();
    crate::check_ordered_keys(&keys)?;
    let mut encoder = Encoder::new(MAX_MESSAGE_BYTES + BATCH_ENCODED_BYTES + 4096);
    encoder.fixed(&encode_batch_header(&certificate.header)?)?;
    encoder.bytes(&certificate.validity_proof, MAX_MESSAGE_BYTES)?;
    encoder.sequence_length(certificate.guarantor_signatures.len(), 32)?;
    for (guarantor_id, signature) in &certificate.guarantor_signatures {
        encoder.bytes(guarantor_id, 32)?;
        encoder.bytes(signature, 64)?;
    }
    encoder.u32(certificate.threshold)?;
    encoder.bytes(&certificate.settlement_reference, 1024)?;
    Ok(encoder.finish())
}
