//! Raw protocol evidence ingress and opaque locally verified facts.

use layerx_proof::inclusion::{
    verify_receipt as verify_receipt_inclusion, verify_state, InclusionError, InclusionEvidence,
    SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_proof::receipt::{
    verify_outcome, AuthorizedBatch, ReceiptCheck, VerificationFailure,
    VerifiedReceipt as ProofVerifiedReceipt,
};
use layerx_wire::receipt::decode;
use sha2::{Digest, Sha256};

/// Raw receipt material returned by a core boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawReceiptEvidence {
    canonical_receipt: Vec<u8>,
    proof: Proof,
    canonical_header: Vec<u8>,
    header_signature: [u8; 64],
    authorization: SequencerAuthorization,
}

impl RawReceiptEvidence {
    #[must_use]
    pub fn new(
        canonical_receipt: Vec<u8>,
        proof: Proof,
        canonical_header: Vec<u8>,
        header_signature: [u8; 64],
        authorization: SequencerAuthorization,
    ) -> Self {
        Self {
            canonical_receipt,
            proof,
            canonical_header,
            header_signature,
            authorization,
        }
    }

    #[must_use]
    pub fn canonical_receipt(&self) -> &[u8] {
        &self.canonical_receipt
    }

    #[must_use]
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn header_signature(&self) -> [u8; 64] {
        self.header_signature
    }

    #[must_use]
    pub const fn authorization(&self) -> SequencerAuthorization {
        self.authorization
    }
}

/// Exact verification stage which refused raw receipt evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReceiptEvidenceError {
    Inclusion(InclusionError),
    Receipt(VerificationFailure),
    SequenceRange,
}

/// Receipt facts available only after canonical, root-chain, and signature verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedReceiptEvidence {
    verified: ProofVerifiedReceipt,
    inclusion: InclusionEvidence,
    receipt_ref: [u8; 32],
    activity_id: [u8; 32],
    global_sequence: u64,
    result_code: i32,
    amount: u128,
}

impl VerifiedReceiptEvidence {
    #[must_use]
    pub const fn receipt_ref(&self) -> [u8; 32] {
        self.receipt_ref
    }

    #[must_use]
    pub fn canonical_receipt(&self) -> &[u8] {
        self.verified.canonical_bytes()
    }

    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        self.inclusion.level()
    }

    #[must_use]
    pub fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    #[must_use]
    pub fn result_code(&self) -> i32 {
        self.result_code
    }

    #[must_use]
    pub const fn global_sequence(&self) -> u64 {
        self.global_sequence
    }

    #[must_use]
    pub fn amount(&self) -> u128 {
        self.amount
    }
}

/// Locally verifies raw boundary evidence and returns an unforgeable receipt token.
///
/// # Errors
///
/// Returns the exact canonical, invariant, root-chain, or signature failure.
pub fn verify_receipt(
    raw: &RawReceiptEvidence,
) -> Result<VerifiedReceiptEvidence, ReceiptEvidenceError> {
    let inclusion = verify_receipt_inclusion(
        &raw.canonical_receipt,
        &raw.proof,
        &raw.canonical_header,
        &raw.header_signature,
        &raw.authorization,
    )
    .map_err(ReceiptEvidenceError::Inclusion)?;
    let decoded = decode(&raw.canonical_receipt).map_err(|_| {
        ReceiptEvidenceError::Receipt(VerificationFailure {
            check: ReceiptCheck::Decode,
        })
    })?;
    let protocol = decoded.protocol().ok_or(ReceiptEvidenceError::Receipt(
        VerificationFailure {
            check: ReceiptCheck::ReceiptShape,
        },
    ))?;
    let header = inclusion.header().header();
    if protocol.global_sequence() < header.first_sequence()
        || protocol.global_sequence() > header.last_sequence()
    {
        return Err(ReceiptEvidenceError::SequenceRange);
    }
    let authorised = AuthorizedBatch::new(
        protocol.batch_id(),
        protocol.asset(),
        header.previous_state_root(),
        header.resulting_state_root(),
        raw.authorization.public_key(),
    );
    let verified = verify_outcome(&raw.canonical_receipt, &authorised)
        .map_err(ReceiptEvidenceError::Receipt)?;
    let protocol = verified.receipt().protocol().ok_or(ReceiptEvidenceError::Receipt(
        VerificationFailure {
            check: ReceiptCheck::ReceiptShape,
        },
    ))?;
    let receipt_ref = Sha256::digest(verified.canonical_bytes()).into();
    let activity_id = protocol.activity_id();
    let global_sequence = protocol.global_sequence();
    let result_code = protocol.result_code();
    let amount = protocol.amount();
    Ok(VerifiedReceiptEvidence {
        verified,
        inclusion,
        receipt_ref,
        activity_id,
        global_sequence,
        result_code,
        amount,
    })
}

/// Raw state leaf and signed inclusion material returned by a core boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawStateEvidence {
    canonical_state: Vec<u8>,
    proof: Proof,
    resulting_state_root: [u8; 32],
    canonical_header: Vec<u8>,
    header_signature: [u8; 64],
    authorization: SequencerAuthorization,
}

impl RawStateEvidence {
    #[must_use]
    pub fn new(
        canonical_state: Vec<u8>,
        proof: Proof,
        resulting_state_root: [u8; 32],
        canonical_header: Vec<u8>,
        header_signature: [u8; 64],
        authorization: SequencerAuthorization,
    ) -> Self {
        Self {
            canonical_state,
            proof,
            resulting_state_root,
            canonical_header,
            header_signature,
            authorization,
        }
    }

    #[must_use]
    pub fn canonical_state(&self) -> &[u8] {
        &self.canonical_state
    }

    #[must_use]
    pub const fn proof(&self) -> &Proof {
        &self.proof
    }

    #[must_use]
    pub const fn resulting_state_root(&self) -> [u8; 32] {
        self.resulting_state_root
    }

    #[must_use]
    pub fn canonical_header(&self) -> &[u8] {
        &self.canonical_header
    }

    #[must_use]
    pub const fn header_signature(&self) -> [u8; 64] {
        self.header_signature
    }

    #[must_use]
    pub const fn authorization(&self) -> SequencerAuthorization {
        self.authorization
    }
}

/// State bytes available only after signed-header and Merkle verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedStateEvidence {
    canonical_state: Vec<u8>,
    inclusion: InclusionEvidence,
    observed_head_sequence: u64,
}

impl VerifiedStateEvidence {
    #[must_use]
    pub fn canonical_state(&self) -> &[u8] {
        &self.canonical_state
    }

    #[must_use]
    pub const fn level(&self) -> layerx_types::verify::VerificationLevel {
        self.inclusion.level()
    }

    #[must_use]
    pub const fn observed_head_sequence(&self) -> u64 {
        self.observed_head_sequence
    }
}

/// Locally verifies a raw state leaf against a signed canonical batch head.
///
/// # Errors
///
/// Returns the exact header, authority, root, signature, or Merkle failure.
pub fn verify_state_evidence(
    raw: &RawStateEvidence,
) -> Result<VerifiedStateEvidence, InclusionError> {
    let evidence = verify_state(
        &raw.canonical_state,
        &raw.proof,
        &raw.resulting_state_root,
        &raw.canonical_header,
        &raw.header_signature,
        &raw.authorization,
    )?;
    let observed_head_sequence = evidence.header().header().last_sequence();
    Ok(VerifiedStateEvidence {
        canonical_state: raw.canonical_state.clone(),
        inclusion: evidence,
        observed_head_sequence,
    })
}
