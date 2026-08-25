//! SHA-256 hashing over type-gated canonical bytes and exact core domain tags.

use layerx_types::account::AccountId;
use layerx_types::ids::Did;
use layerx_types::payload::Payload;
use layerx_types::result::KnownResult;

use crate::activity::{encode_signed, Activity};
use crate::WireError;

/// Every hashing purpose declared by `lxp_domain_tag_id`, in core order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Domain {
    /// Canonical activity identifier.
    ActivityId,
    /// Canonical module payload digest.
    PayloadHash,
    /// Signature-preimage digest.
    SignaturePreimage,
    /// Authority representation digest.
    AuthorityHash,
    /// Authorization context digest.
    ContextHash,
    /// Merkle leaf digest.
    MerkleLeaf,
    /// Merkle internal-node digest.
    MerkleInternal,
    /// Batch header digest.
    BatchHeader,
    /// Receipt digest.
    Receipt,
    /// Checkpoint certificate digest.
    CheckpointCertificate,
    /// Account identifier digest.
    AccountId,
    /// DID identifier digest.
    DidId,
    /// EVM payout binding digest.
    EvmPayoutBinding,
    /// State leaf digest.
    StateLeaf,
    /// State internal-node digest.
    StateNode,
    /// State-root chain digest.
    StateRootChain,
    /// Snapshot digest.
    Snapshot,
    /// Availability chunk digest.
    DaChunk,
    /// Availability challenge digest.
    DaChallenge,
    /// Domain-bound guarantor checkpoint attestation digest.
    GuarantorAttestation,
}

impl Domain {
    /// Returns the exact NUL-terminated core domain tag.
    #[must_use]
    pub const fn tag(self) -> &'static [u8] {
        match self {
            Self::ActivityId => b"LXP/v1/activity-id\0",
            Self::PayloadHash => b"LXP/v1/payload-hash\0",
            Self::SignaturePreimage => b"LXP/v1/signature-preimage\0",
            Self::AuthorityHash => b"LXP/v1/authority-hash\0",
            Self::ContextHash => b"LXP/v1/context-hash\0",
            Self::MerkleLeaf => b"LXP/v1/merkle-leaf\0",
            Self::MerkleInternal => b"LXP/v1/merkle-internal\0",
            Self::BatchHeader => b"LXP/v1/batch-header\0",
            Self::Receipt => b"LXP/v1/receipt\0",
            Self::CheckpointCertificate => b"LXP/v1/checkpoint-certificate\0",
            Self::AccountId => b"LXP/v1/account-id\0",
            Self::DidId => b"LXP/v1/did-id\0",
            Self::EvmPayoutBinding => b"LXP/v1/evm-payout-binding\0",
            Self::StateLeaf => b"LXP/v1/state-leaf\0",
            Self::StateNode => b"LXP/v1/state-node\0",
            Self::StateRootChain => b"LXP/v1/state-root-chain\0",
            Self::Snapshot => b"LXP/v1/snapshot\0",
            Self::DaChunk => b"LXP/v1/da-chunk\0",
            Self::DaChallenge => b"LXP/v1/da-challenge\0",
            Self::GuarantorAttestation => b"LXP/v1/guarantor-attestation\0",
        }
    }
}

/// Bytes admitted to a consensus hash because they came from a canonical wire
/// encoder or a successfully decoded canonical payload.
#[derive(Clone, Eq, PartialEq)]
pub struct CanonicalBytes(Vec<u8>);

impl CanonicalBytes {
    pub(crate) fn from_wire(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Borrows the exact canonical bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for CanonicalBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CanonicalBytes")
            .field("length", &self.0.len())
            .finish_non_exhaustive()
    }
}

/// Converts one successfully decoded activity back to canonical submitted bytes.
///
/// # Errors
///
/// Returns a typed encoding error if a protocol bound cannot be reproduced.
pub fn canonical_activity(activity: &Activity) -> Result<CanonicalBytes, WireError> {
    encode_signed(activity).map(CanonicalBytes::from_wire)
}

/// Marks the payload bytes of a successfully decoded activity as canonical.
#[must_use]
pub fn canonical_payload(activity: &Activity) -> CanonicalBytes {
    CanonicalBytes::from_wire(activity.payload().to_vec())
}

/// Computes `SHA256(exact_domain_tag || canonical_bytes)` exactly as core.
///
/// # Errors
///
/// Returns a length-limit error when the combined input length cannot be
/// represented safely.
pub fn domain(purpose: Domain, canonical: &CanonicalBytes) -> Result<[u8; 32], WireError> {
    let tag = purpose.tag();
    let Some(length) = tag.len().checked_add(canonical.0.len()) else {
        return Err(WireError::known(KnownResult::LengthLimit, 0));
    };
    let mut input = Vec::with_capacity(length);
    input.extend_from_slice(tag);
    input.extend_from_slice(&canonical.0);
    sha256(&input)
}

/// Computes the core activity identifier over canonical signed bytes.
///
/// # Errors
///
/// Returns a typed encoding or hash length failure.
pub fn activity_id(activity: &Activity) -> Result<[u8; 32], WireError> {
    domain(Domain::ActivityId, &canonical_activity(activity)?)
}

/// Computes the core payload digest over canonical module payload bytes.
///
/// # Errors
///
/// Returns a hash length failure when the input cannot be represented safely.
pub fn payload_hash(activity: &Activity) -> Result<[u8; 32], WireError> {
    domain(Domain::PayloadHash, &canonical_payload(activity))
}

/// Computes the core payload digest before an envelope is constructed.
///
/// # Errors
///
/// Returns a hash length failure when the payload cannot be represented safely.
pub fn payload_hash_for(payload: &Payload) -> Result<[u8; 32], WireError> {
    domain(
        Domain::PayloadHash,
        &CanonicalBytes::from_wire(payload.as_bytes().to_vec()),
    )
}

/// Derives the exact core account identifier from a validated namespace.
///
/// # Errors
///
/// Returns a hash length failure when the canonical account bytes cannot be
/// represented safely.
pub fn account_id(account: &AccountId) -> Result<[u8; 32], WireError> {
    domain(
        Domain::AccountId,
        &CanonicalBytes::from_wire(account.canonical().as_bytes().to_vec()),
    )
}

/// Derives the exact core DID identifier from a bounded DID.
///
/// # Errors
///
/// Returns a hash length failure when the DID bytes cannot be represented
/// safely.
pub fn did_id(did: &Did) -> Result<[u8; 32], WireError> {
    domain(
        Domain::DidId,
        &CanonicalBytes::from_wire(did.as_bytes().to_vec()),
    )
}

/// Hashes canonical leaf bytes under the protocol Merkle-leaf domain.
///
/// This entry point exists for independent proof verification. It does not
/// admit the bytes to any signing or identifier path.
///
/// # Errors
///
/// Returns a length-limit error when the domain-prefixed input length cannot
/// be represented safely.
pub fn merkle_leaf(bytes: &[u8]) -> Result<[u8; 32], WireError> {
    domain(
        Domain::MerkleLeaf,
        &CanonicalBytes::from_wire(bytes.to_vec()),
    )
}

/// Hashes two already verified child digests under the internal-node domain.
///
/// # Errors
///
/// Returns a length-limit error when the fixed domain-prefixed input cannot
/// be represented safely.
pub fn merkle_internal(left: &[u8; 32], right: &[u8; 32]) -> Result<[u8; 32], WireError> {
    let mut pair = Vec::with_capacity(64);
    pair.extend_from_slice(left);
    pair.extend_from_slice(right);
    domain(Domain::MerkleInternal, &CanonicalBytes::from_wire(pair))
}

/// Computes the core receipt-signature digest over an unsigned canonical
/// receipt encoding.
///
/// # Errors
///
/// Returns a length-limit error when the domain-prefixed input length cannot
/// be represented safely.
pub fn receipt_digest(unsigned_receipt: &[u8]) -> Result<[u8; 32], WireError> {
    domain(
        Domain::Receipt,
        &CanonicalBytes::from_wire(unsigned_receipt.to_vec()),
    )
}

/// Computes the core batch-header digest over the exact canonical header.
///
/// # Errors
///
/// Returns a length-limit error when the domain-prefixed input length cannot
/// be represented safely.
pub fn batch_header_digest(header: &[u8]) -> Result<[u8; 32], WireError> {
    domain(
        Domain::BatchHeader,
        &CanonicalBytes::from_wire(header.to_vec()),
    )
}

/// Computes the execution identifier placed in receipts by the current C producer.
///
/// The fixed preimage is `previous_state_root || activity_id ||
/// global_sequence_be || batch_number_be` under the protocol context-hash domain.
/// This identifier is distinct from the digest of the later sealed batch header.
///
/// # Errors
///
/// Returns a hash length failure if the domain-prefixed fixed preimage cannot be
/// represented by the hashing implementation.
pub fn execution_batch_id(
    previous_state_root: [u8; 32],
    activity_id: [u8; 32],
    global_sequence: u64,
    batch_number: u64,
) -> Result<[u8; 32], WireError> {
    let mut preimage = [0_u8; 80];
    preimage[..32].copy_from_slice(&previous_state_root);
    preimage[32..64].copy_from_slice(&activity_id);
    preimage[64..72].copy_from_slice(&global_sequence.to_be_bytes());
    preimage[72..].copy_from_slice(&batch_number.to_be_bytes());
    domain(
        Domain::ContextHash,
        &CanonicalBytes::from_wire(preimage.to_vec()),
    )
}

/// Computes the exact checkpoint identifier over canonical header bytes,
/// big-endian validity-proof length, and validity-proof bytes.
///
/// # Errors
///
/// Returns a length-limit error when any combined length cannot be represented
/// safely or when the validity proof exceeds the protocol `u32` framing.
pub fn checkpoint_id(header: &[u8], validity_proof: &[u8]) -> Result<[u8; 32], WireError> {
    let proof_length = u32::try_from(validity_proof.len())
        .map_err(|_| WireError::known(KnownResult::LengthLimit, 0))?;
    let capacity = header
        .len()
        .checked_add(4)
        .and_then(|value| value.checked_add(validity_proof.len()))
        .ok_or_else(|| WireError::known(KnownResult::LengthLimit, 0))?;
    let mut canonical = Vec::with_capacity(capacity);
    canonical.extend_from_slice(header);
    canonical.extend_from_slice(&proof_length.to_be_bytes());
    canonical.extend_from_slice(validity_proof);
    domain(
        Domain::CheckpointCertificate,
        &CanonicalBytes::from_wire(canonical),
    )
}

/// Computes the exact domain-bound guarantor-attestation digest.
///
/// # Errors
///
/// Returns a length-limit error when the domain-prefixed message cannot be
/// represented safely.
pub fn checkpoint_attestation_digest(message: &[u8]) -> Result<[u8; 32], WireError> {
    domain(
        Domain::GuarantorAttestation,
        &CanonicalBytes::from_wire(message.to_vec()),
    )
}

/// Computes the C17 data-availability chunk digest over its 25-byte metadata
/// prefix and exact served bytes.
///
/// # Errors
///
/// Returns a length-limit error when the byte length exceeds `u32` framing or
/// when the domain-prefixed input length cannot be represented safely.
pub fn availability_chunk_digest(
    batch_number: u64,
    chunk_index: u32,
    availability_class: u8,
    class_offset: u64,
    bytes: &[u8],
) -> Result<[u8; 32], WireError> {
    let byte_length =
        u32::try_from(bytes.len()).map_err(|_| WireError::known(KnownResult::LengthLimit, 0))?;
    let capacity = 25_usize
        .checked_add(bytes.len())
        .ok_or_else(|| WireError::known(KnownResult::LengthLimit, 0))?;
    let mut canonical = Vec::with_capacity(capacity);
    canonical.extend_from_slice(&batch_number.to_be_bytes());
    canonical.extend_from_slice(&chunk_index.to_be_bytes());
    canonical.push(availability_class);
    canonical.extend_from_slice(&class_offset.to_be_bytes());
    canonical.extend_from_slice(&byte_length.to_be_bytes());
    canonical.extend_from_slice(bytes);
    domain(Domain::DaChunk, &CanonicalBytes::from_wire(canonical))
}

const INITIAL_STATE: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

const ROUND_CONSTANTS: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

fn compress(block: &[u8], state: &mut [u32; 8]) {
    let mut words = [0_u32; 64];
    for (index, word) in words[..16].iter_mut().enumerate() {
        let offset = index * 4;
        *word = u32::from_be_bytes([
            block[offset],
            block[offset + 1],
            block[offset + 2],
            block[offset + 3],
        ]);
    }
    for index in 16..64 {
        let first = words[index - 15].rotate_right(7)
            ^ words[index - 15].rotate_right(18)
            ^ (words[index - 15] >> 3);
        let second = words[index - 2].rotate_right(17)
            ^ words[index - 2].rotate_right(19)
            ^ (words[index - 2] >> 10);
        words[index] = words[index - 16]
            .wrapping_add(first)
            .wrapping_add(words[index - 7])
            .wrapping_add(second);
    }
    let mut working = *state;
    for index in 0..64 {
        let sigma_one =
            working[4].rotate_right(6) ^ working[4].rotate_right(11) ^ working[4].rotate_right(25);
        let choice = (working[4] & working[5]) ^ ((!working[4]) & working[6]);
        let temporary_one = working[7]
            .wrapping_add(sigma_one)
            .wrapping_add(choice)
            .wrapping_add(ROUND_CONSTANTS[index])
            .wrapping_add(words[index]);
        let sigma_zero =
            working[0].rotate_right(2) ^ working[0].rotate_right(13) ^ working[0].rotate_right(22);
        let majority =
            (working[0] & working[1]) ^ (working[0] & working[2]) ^ (working[1] & working[2]);
        let temporary_two = sigma_zero.wrapping_add(majority);
        working = [
            temporary_one.wrapping_add(temporary_two),
            working[0],
            working[1],
            working[2],
            working[3].wrapping_add(temporary_one),
            working[4],
            working[5],
            working[6],
        ];
    }
    for (value, addition) in state.iter_mut().zip(working) {
        *value = value.wrapping_add(addition);
    }
}

fn sha256(input: &[u8]) -> Result<[u8; 32], WireError> {
    let bit_length = u64::try_from(input.len())
        .ok()
        .and_then(|length| length.checked_mul(8))
        .ok_or_else(|| WireError::known(KnownResult::LengthLimit, 0))?;
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_length.to_be_bytes());
    let mut state = INITIAL_STATE;
    for block in padded.chunks_exact(64) {
        compress(block, &mut state);
    }
    let mut digest = [0_u8; 32];
    for (index, word) in state.into_iter().enumerate() {
        let offset = index * 4;
        digest[offset..offset + 4].copy_from_slice(&word.to_be_bytes());
    }
    Ok(digest)
}
