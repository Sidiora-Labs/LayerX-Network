//! Verified checkpoint certificates and root-provenance proof bundles.

use layerx_proof::checkpoint::{
    verify_certificate, Certificate, CheckpointError, GuarantorKey, SettlementDomain,
};
use layerx_proof::inclusion::{
    verify_activity, verify_state, InclusionError, SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_types::verify::VerificationLevel;
use layerx_wire::receipt::decode_batch_header;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeaderCommitments {
    pub batch_number: u64,
    pub previous_state_root: [u8; 32],
    pub resulting_state_root: [u8; 32],
    pub activity_root: [u8; 32],
    pub receipt_root: [u8; 32],
    pub event_root: [u8; 32],
    pub availability_root: [u8; 32],
    pub oracle_root: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuarantorSignature {
    pub guarantor_id: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedCheckpoint {
    pub checkpoint_id: [u8; 32],
    pub header_bytes: Vec<u8>,
    pub commitments: HeaderCommitments,
    pub guarantor_signatures: Vec<GuarantorSignature>,
    pub achieved: usize,
    pub threshold: usize,
    pub settlement_reference: Option<Vec<u8>>,
    pub verification_level: VerificationLevel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProofBundleKind {
    Activity,
    State,
}

pub struct ProofBundleRequest<'a> {
    pub kind: ProofBundleKind,
    pub canonical_leaf_bytes: &'a [u8],
    pub proof: &'a Proof,
    pub named_root: [u8; 32],
    pub header_bytes: &'a [u8],
    pub header_signature: [u8; 64],
    pub authorization: &'a SequencerAuthorization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServedProofBundle {
    pub kind: ProofBundleKind,
    pub canonical_leaf_bytes: Vec<u8>,
    pub proof_bytes: Vec<u8>,
    pub named_root: [u8; 32],
    pub batch_number: u64,
    pub signed_header_digest: [u8; 32],
    pub verification_level: VerificationLevel,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckpointReadError {
    AvailabilityUnavailable { checkpoint_id: [u8; 32] },
    Certificate(CheckpointError),
    Inclusion(InclusionError),
    Header,
    RootMismatch,
    Arithmetic,
}

/// Verifies a certificate before deriving every served field from it.
pub(crate) fn serve_checkpoint(
    certificate: &Certificate,
    bonded_set: &[GuarantorKey],
    registered_checkpoint_id: [u8; 32],
    expected_settlement_domain: SettlementDomain,
    registered_settlement_reference: Option<&[u8]>,
    availability_obtained: bool,
) -> Result<ServedCheckpoint, CheckpointReadError> {
    if !availability_obtained {
        return Err(CheckpointReadError::AvailabilityUnavailable {
            checkpoint_id: registered_checkpoint_id,
        });
    }
    let report = verify_certificate(
        certificate,
        bonded_set,
        &registered_checkpoint_id,
        expected_settlement_domain,
        registered_settlement_reference,
    )
    .map_err(CheckpointReadError::Certificate)?;
    let header_bytes = certificate.checkpoint().header_bytes();
    let header = decode_batch_header(header_bytes).map_err(|_| CheckpointReadError::Header)?;
    Ok(ServedCheckpoint {
        checkpoint_id: registered_checkpoint_id,
        header_bytes: header_bytes.to_vec(),
        commitments: HeaderCommitments {
            batch_number: header.batch_number(),
            previous_state_root: header.previous_state_root(),
            resulting_state_root: header.resulting_state_root(),
            activity_root: header.activity_merkle_root(),
            receipt_root: header.receipt_merkle_root(),
            event_root: header.event_merkle_root(),
            availability_root: header.data_availability_root(),
            oracle_root: header.oracle_root(),
        },
        guarantor_signatures: certificate
            .attestations()
            .iter()
            .map(|attestation| GuarantorSignature {
                guarantor_id: attestation.guarantor_id(),
                signature: attestation.signature(),
            })
            .collect(),
        achieved: report.achieved,
        threshold: report.required,
        settlement_reference: report.evidence().settlement_reference().map(<[u8]>::to_vec),
        verification_level: report.level(),
    })
}

/// Verifies and serves one inclusion proof with its signed-header root provenance.
///
/// # Errors
///
/// Returns the inclusion failure when the proof or signed header does not verify, `RootMismatch`
/// when the header commits a different root than the named one, and `Arithmetic` when the
/// sibling count exceeds its encoded bound.
pub fn proof_bundle(
    request: &ProofBundleRequest<'_>,
) -> Result<ServedProofBundle, CheckpointReadError> {
    let evidence = match request.kind {
        ProofBundleKind::Activity => verify_activity(
            request.canonical_leaf_bytes,
            request.proof,
            request.header_bytes,
            &request.header_signature,
            request.authorization,
        )
        .map_err(CheckpointReadError::Inclusion)?,
        ProofBundleKind::State => verify_state(
            request.canonical_leaf_bytes,
            request.proof,
            &request.named_root,
            request.header_bytes,
            &request.header_signature,
            request.authorization,
        )
        .map_err(CheckpointReadError::Inclusion)?,
    };
    let committed_root = match request.kind {
        ProofBundleKind::Activity => evidence.header().header().activity_merkle_root(),
        ProofBundleKind::State => evidence.header().header().resulting_state_root(),
    };
    if committed_root != request.named_root {
        return Err(CheckpointReadError::RootMismatch);
    }
    Ok(ServedProofBundle {
        kind: request.kind,
        canonical_leaf_bytes: request.canonical_leaf_bytes.to_vec(),
        proof_bytes: encode_proof(request.proof)?,
        named_root: request.named_root,
        batch_number: evidence.header().header().batch_number(),
        signed_header_digest: evidence.header().digest(),
        verification_level: evidence.level(),
    })
}

fn encode_proof(proof: &Proof) -> Result<Vec<u8>, CheckpointReadError> {
    let sibling_count =
        u32::try_from(proof.siblings().len()).map_err(|_| CheckpointReadError::Arithmetic)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&proof.leaf_index().to_be_bytes());
    bytes.extend_from_slice(&proof.leaf_count().to_be_bytes());
    bytes.extend_from_slice(&sibling_count.to_be_bytes());
    for sibling in proof.siblings() {
        bytes.extend_from_slice(sibling);
    }
    Ok(bytes)
}
