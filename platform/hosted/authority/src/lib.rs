//! Verification core of the hosted receipt authority.
//!
//! Every fact the service answers is derived here from three inputs only: the
//! canonical receipt bytes named by an activity, the independent replica's
//! batch evidence for that receipt, and the pinned sequencer authorisation.
//! Nothing in this module reads the sequencer daemon's own store.

use layerx_proof::inclusion::{verify_receipt, InclusionError, SequencerAuthorization};
use layerx_proof::merkle::decode_proof;
use layerx_proof::receipt::{verify_outcome, AuthorizedBatch, ReceiptCheck};
use layerx_wire::hash::{execution_batch_id, receipt_digest};
use layerx_wire::receipt::{decode, encode_unsigned};
use serde::Deserialize;

/// Lower-case hexadecimal helpers shared by the service and its tests.
/// Refusal of hexadecimal text that is not well formed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HexError;

impl core::fmt::Display for HexError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("text is not well-formed hexadecimal")
    }
}

impl std::error::Error for HexError {}

pub mod hex {
    use super::HexError;

    /// Encodes bytes as lower-case hexadecimal.
    #[must_use]
    pub fn encode(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            text.push(char::from(DIGITS[usize::from(byte >> 4)]));
            text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        text
    }

    /// Decodes hexadecimal text of either case into bytes.
    ///
    /// # Errors
    ///
    /// Returns `HexError` for odd lengths or non-hexadecimal characters.
    pub fn decode(text: &str) -> Result<Vec<u8>, HexError> {
        let bytes = text.as_bytes();
        if !bytes.len().is_multiple_of(2) {
            return Err(HexError);
        }
        bytes
            .chunks(2)
            .map(|pair| {
                let high = nibble(pair[0])?;
                let low = nibble(pair[1])?;
                Ok((high << 4) | low)
            })
            .collect()
    }

    /// Decodes exactly thirty-two bytes of hexadecimal text.
    ///
    /// # Errors
    ///
    /// Returns `HexError` unless the text is sixty-four hexadecimal characters.
    pub fn decode32(text: &str) -> Result<[u8; 32], HexError> {
        if text.len() != 64 {
            return Err(HexError);
        }
        decode(text)?.as_slice().try_into().map_err(|_| HexError)
    }

    /// Returns whether the text is exactly sixty-four hexadecimal characters.
    #[must_use]
    pub fn is_hex32(text: &str) -> bool {
        text.len() == 64 && text.bytes().all(|byte| nibble(byte).is_ok())
    }

    fn nibble(byte: u8) -> Result<u8, HexError> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            _ => Err(HexError),
        }
    }
}

/// The replica's evidence for one receipt, decoded from its JSON document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchEvidence {
    /// Canonical batch header bytes.
    pub header: Vec<u8>,
    /// Sequencer signature over the batch header digest.
    pub header_signature: [u8; 64],
    /// Encoded index-aware Merkle proof of the receipt under the header's
    /// receipt root.
    pub receipt_proof: Vec<u8>,
}

/// The exact reason an authority answer was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvidenceRefusal {
    /// The receipt bytes are not canonically decodable.
    ReceiptDecode,
    /// The receipt carries no protocol receipt.
    ReceiptShape,
    /// The receipt names a different activity than the one requested.
    ActivityMismatch,
    /// The replica document is not the expected JSON shape.
    ReplicaDocument,
    /// The replica document names a different replica identity.
    ReplicaIdentity,
    /// The replica document names a sequencer key other than the pinned key.
    SequencerKey,
    /// The replica evidence is not decodable.
    EvidenceEncoding,
    /// Header or Merkle inclusion verification failed.
    Inclusion(InclusionError),
    /// The receipt and header disagree on the protocol version.
    ProtocolVersion,
    /// The receipt's global sequence lies outside the header range.
    SequenceRange,
    /// The receipt batch identity is not the re-derived execution batch id.
    BatchIdentity,
    /// The receipt failed outcome verification under the derived facts.
    Receipt(ReceiptCheck),
}

/// The eight facts one authority answer carries, before hexadecimal encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorityFacts {
    /// Activity identity carried by the receipt.
    pub activity_id: [u8; 32],
    /// Re-derived execution batch identity.
    pub batch_id: [u8; 32],
    /// Asset the receipt settles.
    pub asset: [u8; 32],
    /// Previous state root from the signed header.
    pub previous_state_root: [u8; 32],
    /// Resulting state root from the signed header.
    pub resulting_state_root: [u8; 32],
    /// Pinned sequencer public key that signed the header and the receipt.
    pub sequencer_public_key: [u8; 32],
    /// Global sequence of the receipt within the header range.
    pub global_sequence: u64,
    /// Batch number of the signed header.
    pub batch_number: u64,
}

/// Identifiers the replica lookup for a receipt is keyed by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptLocator {
    /// Activity identity carried by the receipt.
    pub activity_id: [u8; 32],
    /// Batch identity carried by the receipt.
    pub batch_id: [u8; 32],
    /// Digest of the unsigned canonical receipt.
    pub receipt_digest: [u8; 32],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplicaDocument {
    authority_replica_id: String,
    sequencer_public_key: String,
    batch_evidence: ReplicaBatchEvidence,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplicaBatchEvidence {
    header_hex: String,
    header_signature: String,
    receipt_proof_hex: String,
}

/// Extracts the activity, batch identity and receipt digest a replica lookup
/// needs from canonical receipt bytes.
///
/// # Errors
///
/// Refuses receipts that do not decode or carry no protocol receipt.
pub fn receipt_locator(receipt_bytes: &[u8]) -> Result<ReceiptLocator, EvidenceRefusal> {
    let receipt = decode(receipt_bytes).map_err(|_| EvidenceRefusal::ReceiptDecode)?;
    let protocol = receipt.protocol().ok_or(EvidenceRefusal::ReceiptShape)?;
    let unsigned = encode_unsigned(&receipt).map_err(|_| EvidenceRefusal::ReceiptDecode)?;
    let digest = receipt_digest(&unsigned).map_err(|_| EvidenceRefusal::ReceiptDecode)?;
    Ok(ReceiptLocator {
        activity_id: protocol.activity_id(),
        batch_id: protocol.batch_id(),
        receipt_digest: digest,
    })
}

/// Parses the replica's receipt-authority document and pins its identities.
///
/// # Errors
///
/// Refuses documents with unknown or missing fields, a replica identity other
/// than `expected_replica_id`, a sequencer key other than `expected_key`, or
/// non-hexadecimal evidence.
pub fn parse_replica_evidence(
    document: &[u8],
    expected_replica_id: [u8; 32],
    expected_key: [u8; 32],
) -> Result<BatchEvidence, EvidenceRefusal> {
    let document: ReplicaDocument =
        serde_json::from_slice(document).map_err(|_| EvidenceRefusal::ReplicaDocument)?;
    let replica_id = hex::decode32(&document.authority_replica_id)
        .map_err(|HexError| EvidenceRefusal::ReplicaDocument)?;
    if replica_id != expected_replica_id {
        return Err(EvidenceRefusal::ReplicaIdentity);
    }
    let key = hex::decode32(&document.sequencer_public_key)
        .map_err(|HexError| EvidenceRefusal::ReplicaDocument)?;
    if key != expected_key {
        return Err(EvidenceRefusal::SequencerKey);
    }
    let header = hex::decode(&document.batch_evidence.header_hex)
        .map_err(|HexError| EvidenceRefusal::EvidenceEncoding)?;
    let signature = hex::decode(&document.batch_evidence.header_signature)
        .map_err(|HexError| EvidenceRefusal::EvidenceEncoding)?;
    let header_signature: [u8; 64] = signature
        .as_slice()
        .try_into()
        .map_err(|_| EvidenceRefusal::EvidenceEncoding)?;
    let receipt_proof = hex::decode(&document.batch_evidence.receipt_proof_hex)
        .map_err(|HexError| EvidenceRefusal::EvidenceEncoding)?;
    if header.is_empty() || receipt_proof.is_empty() {
        return Err(EvidenceRefusal::EvidenceEncoding);
    }
    Ok(BatchEvidence {
        header,
        header_signature,
        receipt_proof,
    })
}

/// Derives the authorised batch facts for one activity exactly as
/// `layerx-agentd`'s protocol evidence does: the header signature and the
/// receipt's Merkle inclusion are verified with `layerx-proof`, the execution
/// batch id is re-derived from the signed header and must equal the receipt's,
/// and the receipt outcome is verified under the derived facts.
///
/// # Errors
///
/// Returns the exact check that refused the evidence; no partial facts are
/// returned.
pub fn authorized_batch_by_activity(
    activity_id: [u8; 32],
    receipt_bytes: &[u8],
    evidence: &BatchEvidence,
    authorization: &SequencerAuthorization,
) -> Result<AuthorityFacts, EvidenceRefusal> {
    let receipt = decode(receipt_bytes).map_err(|_| EvidenceRefusal::ReceiptDecode)?;
    let protocol = receipt.protocol().ok_or(EvidenceRefusal::ReceiptShape)?;
    if protocol.activity_id() != activity_id {
        return Err(EvidenceRefusal::ActivityMismatch);
    }
    let proof =
        decode_proof(&evidence.receipt_proof).map_err(|_| EvidenceRefusal::EvidenceEncoding)?;
    let inclusion = verify_receipt(
        receipt_bytes,
        &proof,
        &evidence.header,
        &evidence.header_signature,
        authorization,
    )
    .map_err(EvidenceRefusal::Inclusion)?;
    let header = inclusion.header().header();
    if protocol.protocol_version() != header.protocol_version() {
        return Err(EvidenceRefusal::ProtocolVersion);
    }
    if protocol.global_sequence() < header.first_sequence()
        || protocol.global_sequence() > header.last_sequence()
    {
        return Err(EvidenceRefusal::SequenceRange);
    }
    let expected = execution_batch_id(
        header.previous_state_root(),
        protocol.activity_id(),
        protocol.global_sequence(),
        header.batch_number(),
    )
    .map_err(|_| EvidenceRefusal::BatchIdentity)?;
    if protocol.batch_id() != expected {
        return Err(EvidenceRefusal::BatchIdentity);
    }
    let authorised = AuthorizedBatch::new(
        expected,
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        authorization.public_key(),
    );
    verify_outcome(receipt_bytes, &authorised)
        .map_err(|failure| EvidenceRefusal::Receipt(failure.check))?;
    Ok(AuthorityFacts {
        activity_id,
        batch_id: expected,
        asset: protocol.asset(),
        previous_state_root: header.previous_state_root(),
        resulting_state_root: header.resulting_state_root(),
        sequencer_public_key: authorization.public_key(),
        global_sequence: protocol.global_sequence(),
        batch_number: header.batch_number(),
    })
}
