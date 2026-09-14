use layerx_crypto::ed25519;
use layerx_wire::handover::{decode_evidence, sequencer_id, MAX_TRANSITIONS};
use layerx_wire::receipt::decode_batch_header;
use sha2::{Digest as _, Sha256};

use crate::inclusion::{verify_header, SequencerAuthorization, VerifiedBatchHeader};
use crate::state_witness::StateWitness;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignedAuthorityError {
    Genesis,
    Header,
    Continuity,
    Certificate,
    Bounds,
    UnverifiedRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedAuthorityInterval {
    epoch: u64,
    public_key: [u8; 32],
    first_batch: u64,
    last_batch: u64,
    first_sequence: u64,
    last_sequence: u64,
}

impl SignedAuthorityInterval {
    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    #[must_use]
    pub const fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    #[must_use]
    pub const fn first_batch(&self) -> u64 {
        self.first_batch
    }

    #[must_use]
    pub const fn last_batch(&self) -> u64 {
        self.last_batch
    }

    fn authorization(&self) -> Result<SequencerAuthorization, SignedAuthorityError> {
        Ok(SequencerAuthorization::new(
            sequencer_id(&self.public_key).map_err(|_| SignedAuthorityError::Certificate)?,
            self.public_key,
            self.first_batch,
            self.last_batch,
        ))
    }
}

/// Signed authority provenance; this type does not assert independent chain finality.
/// Production clients export it only after their complete availability and publication checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedAuthorityHistory {
    network_id: u32,
    canonical_genesis_root: [u8; 32],
    genesis_receipt_root: [u8; 32],
    governance_key: [u8; 32],
    initial_key: [u8; 32],
    intervals: Vec<SignedAuthorityInterval>,
    head: Option<VerifiedBatchHeader>,
    head_signature: [u8; 64],
    latest_evidence: Option<[u8; 32]>,
}

impl SignedAuthorityHistory {
    /// # Errors
    /// Refuses a substituted or absent genesis-committed governance authority.
    pub fn from_genesis(
        network_id: u32,
        canonical_genesis_root: [u8; 32],
        initial_key: [u8; 32],
        governance_witness: &[u8],
    ) -> Result<Self, SignedAuthorityError> {
        let witness =
            StateWitness::decode(governance_witness).map_err(|_| SignedAuthorityError::Genesis)?;
        witness
            .verify(canonical_genesis_root)
            .map_err(|_| SignedAuthorityError::Genesis)?;
        let mut key = [0_u8; 32];
        key[..18].copy_from_slice(b"handover-authority");
        let governance_key: [u8; 32] = witness
            .value
            .as_slice()
            .try_into()
            .map_err(|_| SignedAuthorityError::Genesis)?;
        sequencer_id(&initial_key).map_err(|_| SignedAuthorityError::Genesis)?;
        sequencer_id(&governance_key).map_err(|_| SignedAuthorityError::Genesis)?;
        if network_id == 0
            || canonical_genesis_root == [0; 32]
            || witness.module_id != 7
            || witness.key != key
            || governance_key == initial_key
        {
            return Err(SignedAuthorityError::Genesis);
        }
        let mut hash = Sha256::new();
        hash.update(b"LXP/v1/genesis-receipt-root\0");
        hash.update(network_id.to_be_bytes());
        hash.update(canonical_genesis_root);
        Ok(Self {
            network_id,
            canonical_genesis_root,
            genesis_receipt_root: hash.finalize().into(),
            governance_key,
            initial_key,
            intervals: Vec::new(),
            head: None,
            head_signature: [0; 64],
            latest_evidence: None,
        })
    }

    /// Verifies the next contiguous signed header and any exact governance certificate.
    /// # Errors
    /// Refuses missing parents, changed roots, unsigned transitions and spliced evidence.
    pub fn advance(
        &mut self,
        canonical: &[u8],
        signature: &[u8; 64],
        evidence_bytes: Option<&[u8]>,
    ) -> Result<(), SignedAuthorityError> {
        let header = decode_batch_header(canonical).map_err(|_| SignedAuthorityError::Header)?;
        let (batch, sequence, root) =
            self.head
                .as_ref()
                .map_or((0, 0, self.genesis_receipt_root), |head| {
                    (
                        head.header().batch_number(),
                        head.header().last_sequence(),
                        head.header().resulting_state_root(),
                    )
                });
        let epoch = self.intervals.last().map_or(1, |entry| entry.epoch);
        let old_key = self
            .intervals
            .last()
            .map_or(self.initial_key, |entry| entry.public_key);
        if header.protocol_version() != 3
            || header.network_id() != self.network_id
            || batch.checked_add(1) != Some(header.batch_number())
            || sequence.checked_add(1) != Some(header.first_sequence())
            || header.last_sequence() < header.first_sequence()
            || header.last_sequence() == u64::MAX
            || header.previous_state_root() != root
            || (header.epoch() != epoch && epoch.checked_add(1) != Some(header.epoch()))
        {
            return Err(SignedAuthorityError::Continuity);
        }
        let transition = header.epoch() != epoch;
        let actual_digest = evidence_bytes.map(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)));
        let public_key = if transition {
            if self.intervals.len() > MAX_TRANSITIONS {
                return Err(SignedAuthorityError::Bounds);
            }
            let evidence =
                decode_evidence(evidence_bytes.ok_or(SignedAuthorityError::Certificate)?)
                    .map_err(|_| SignedAuthorityError::Certificate)?;
            let certificate = &evidence.certificate;
            let predecessor = self.head.as_ref().ok_or(SignedAuthorityError::Continuity)?;
            if certificate.network_id != self.network_id
                || certificate.old_epoch != epoch
                || certificate.new_epoch != header.epoch()
                || certificate.old_public_key != old_key
                || certificate.activation_batch != header.batch_number()
                || certificate.predecessor_last_sequence.checked_add(1)
                    != Some(header.first_sequence())
                || evidence.predecessor_header != predecessor.canonical_bytes()
                || evidence.predecessor_signature != self.head_signature
                || certificate.new_public_key == self.governance_key
            {
                return Err(SignedAuthorityError::Certificate);
            }
            ed25519::verify_message(
                &self.governance_key,
                &certificate.governance_signature,
                evidence.signed_certificate,
            )
            .map_err(|_| SignedAuthorityError::Certificate)?;
            certificate.new_public_key
        } else {
            if actual_digest != self.latest_evidence {
                return Err(SignedAuthorityError::Certificate);
            }
            old_key
        };
        let authorization = SequencerAuthorization::new(
            sequencer_id(&public_key).map_err(|_| SignedAuthorityError::Certificate)?,
            public_key,
            header.batch_number(),
            header.batch_number(),
        );
        let verified = verify_header(canonical, signature, &authorization)
            .map_err(|_| SignedAuthorityError::Header)?;
        if transition || self.intervals.is_empty() {
            self.intervals.push(SignedAuthorityInterval {
                epoch: header.epoch(),
                public_key,
                first_batch: header.batch_number(),
                last_batch: header.batch_number(),
                first_sequence: header.first_sequence(),
                last_sequence: header.last_sequence(),
            });
        } else {
            let interval = self
                .intervals
                .last_mut()
                .ok_or(SignedAuthorityError::Genesis)?;
            interval.last_batch = header.batch_number();
            interval.last_sequence = header.last_sequence();
        }
        self.latest_evidence = actual_digest;
        self.head = Some(verified);
        self.head_signature = *signature;
        Ok(())
    }

    /// # Errors
    /// Refuses signatures, domains, epochs or sequences outside the authenticated prefix.
    pub fn verify_header(
        &self,
        canonical: &[u8],
        signature: &[u8; 64],
    ) -> Result<VerifiedBatchHeader, SignedAuthorityError> {
        let header = decode_batch_header(canonical).map_err(|_| SignedAuthorityError::Header)?;
        let interval = self
            .intervals
            .iter()
            .find(|interval| {
                (interval.first_batch..=interval.last_batch).contains(&header.batch_number())
            })
            .ok_or(SignedAuthorityError::UnverifiedRange)?;
        if header.protocol_version() != 3
            || header.network_id() != self.network_id
            || header.epoch() != interval.epoch
            || header.first_sequence() < interval.first_sequence
            || header.last_sequence() > interval.last_sequence
        {
            return Err(SignedAuthorityError::Header);
        }
        verify_header(canonical, signature, &interval.authorization()?)
            .map_err(|_| SignedAuthorityError::Header)
    }

    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    #[must_use]
    pub const fn canonical_genesis_root(&self) -> [u8; 32] {
        self.canonical_genesis_root
    }

    #[must_use]
    pub const fn initial_public_key(&self) -> [u8; 32] {
        self.initial_key
    }

    #[must_use]
    pub fn intervals(&self) -> &[SignedAuthorityInterval] {
        &self.intervals
    }

    #[must_use]
    pub const fn verified_head(&self) -> Option<&VerifiedBatchHeader> {
        self.head.as_ref()
    }
}
