pub use layerx_proof::state_witness::{AccountPath, StateProofError, StateWitness};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeEvidence {
    pub request_anchor: [u8; 32],
    pub inclusion_checkpoint: [u8; 32],
    pub network_id: u32,
    pub witness: Vec<u8>,
    pub recipient_signature: Vec<u8>,
}

impl NativeEvidence {
    /// # Errors
    /// Refuses empty domains, malformed signatures and mismatched native state roots.
    pub fn decoded(&self, root: [u8; 32]) -> Result<StateWitness, StateProofError> {
        if self.request_anchor == [0; 32]
            || self.inclusion_checkpoint == [0; 32]
            || self.network_id == 0
            || !matches!(self.recipient_signature.len(), 0 | 64)
        {
            return Err(StateProofError::Encoding);
        }
        let witness = StateWitness::decode(&self.witness)?;
        witness.verify(root)?;
        Ok(witness)
    }
}
