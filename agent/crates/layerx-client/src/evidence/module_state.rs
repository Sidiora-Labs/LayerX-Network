use layerx_proof::inclusion::verify_header;
use layerx_proof::state_witness::StateWitness;

use super::{
    bind_selector, checked_checkpoint, decode_signed_header, AccountEvidencePolicy, EvidenceError,
    Reader, RootSelector, SignedHeader, VerificationLevel, ATTESTATION_BYTES,
    MAX_GUARANTORS, MAX_HEADER_BYTES, MAX_VALIDITY_PROOF_BYTES,
};

const MAX_WITNESS_BYTES: usize = 35 + 129 + 1_048_576 + 96 * 32;

#[derive(Clone, Debug)]
pub struct VerifiedModuleEvidence {
    state_root: [u8; 32],
    level: VerificationLevel,
    signed_header: SignedHeader,
    checkpoint_id: Option<[u8; 32]>,
}

impl VerifiedModuleEvidence {
    #[must_use]
    pub const fn state_root(&self) -> [u8; 32] {
        self.state_root
    }

    #[must_use]
    pub const fn level(&self) -> VerificationLevel {
        self.level
    }

    #[must_use]
    pub const fn signed_header(&self) -> &SignedHeader {
        &self.signed_header
    }

    #[must_use]
    pub const fn checkpoint_id(&self) -> Option<[u8; 32]> {
        self.checkpoint_id
    }
}

/// Verifies exact native module state against the selected signed composite root.
///
/// # Errors
/// Refuses selector substitution, malformed witnesses, incorrect roots, signatures,
/// network/version mismatches, and unbound checkpoint evidence.
pub fn verify_module_evidence(
    canonical_value: &[u8],
    proof_material: &[u8],
    module_id: u16,
    key: &[u8],
    policy: AccountEvidencePolicy,
) -> Result<VerifiedModuleEvidence, EvidenceError> {
    if policy.expected_protocol_version != 3 || module_id > 9 || key.is_empty() || key.len() > 129 {
        return Err(EvidenceError::SelectorMismatch);
    }
    let mut reader = Reader::new(proof_material);
    if reader.u16()? != 1 || reader.u8()? != 4 {
        return Err(EvidenceError::Malformed);
    }
    let selector = RootSelector::decode(&mut reader)?;
    if selector != policy.root_selector {
        return Err(EvidenceError::SelectorMismatch);
    }
    let witness = StateWitness::decode(reader.length_prefixed(MAX_WITNESS_BYTES)?)
        .map_err(|_| EvidenceError::Malformed)?;
    if witness.module_id != module_id || witness.key != key || witness.value != canonical_value {
        return Err(EvidenceError::SelectorMismatch);
    }
    let signed_header = decode_signed_header(&mut reader)?;
    let authorization = signed_header.pinned_key_authorization(
        policy.handshake_sequencer_key,
        policy.expected_protocol_version,
        policy.expected_network_id,
    )?;
    let header = verify_header(
        &signed_header.canonical_bytes,
        &signed_header.signature,
        &authorization,
    )
    .map_err(EvidenceError::Inclusion)?;
    let state_root = header.header().resulting_state_root();
    witness.verify(state_root).map_err(|_| EvidenceError::Malformed)?;
    let checkpoint = match reader.u8()? {
        0 => None,
        1 => {
            let bytes = reader.length_prefixed(MAX_VALIDITY_PROOF_BYTES + MAX_HEADER_BYTES + 16 + MAX_GUARANTORS * ATTESTATION_BYTES)?.to_vec();
            let context = reader.length_prefixed(128 * 1024)?.to_vec();
            Some(checked_checkpoint(
                bytes,
                context,
                policy.expected_protocol_version,
                policy.expected_network_id,
            )?)
        }
        _ => return Err(EvidenceError::Malformed),
    };
    reader.finish()?;
    bind_selector(selector, &signed_header, checkpoint.as_ref())?;
    Ok(VerifiedModuleEvidence {
        state_root,
        level: checkpoint.as_ref().map_or(VerificationLevel::STATE_PROVEN, |value| value.report().level()),
        checkpoint_id: checkpoint.as_ref().and_then(|value| value.report().evidence().checkpoint_id()),
        signed_header,
    })
}
