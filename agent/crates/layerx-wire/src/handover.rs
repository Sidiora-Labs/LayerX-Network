use crate::hash::{batch_header_digest, sha256};
use crate::receipt::decode_batch_header;

pub const CERTIFICATE_BYTES: usize = 392;
pub const MAX_EVIDENCE_BYTES: usize = 1_048_576;
pub const MAX_RECOVERY_BYTES: usize = 16_777_216;
pub const MAX_TRANSITIONS: usize = 512;
const CERTIFICATE_DOMAIN: &[u8] = b"LXP/sequencer-handover/v1\0";
const EVIDENCE_DOMAIN: &[u8] = b"LXP/sequencer-handover-evidence/v1\0";
const FINALITY_DOMAIN: &[u8] = b"LXP/handover-finality/v1\0";
const RECOVERY_DOMAIN: &[u8] = b"LXP/recovery-with-handover/v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HandoverError {
    Encoding,
    Bounds,
    Certificate,
    Predecessor,
    Finality,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Certificate {
    pub network_id: u32,
    pub protocol_version: u16,
    pub old_epoch: u64,
    pub new_epoch: u64,
    pub old_sequencer_id: [u8; 32],
    pub old_public_key: [u8; 32],
    pub new_sequencer_id: [u8; 32],
    pub new_public_key: [u8; 32],
    pub predecessor_batch: u64,
    pub predecessor_last_sequence: u64,
    pub predecessor_header_hash: [u8; 32],
    pub predecessor_state_root: [u8; 32],
    pub predecessor_checkpoint_id: [u8; 32],
    pub activation_batch: u64,
    pub finality_evidence_digest: [u8; 32],
    pub governance_signature: [u8; 64],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evidence<'a> {
    pub certificate: Certificate,
    pub signed_certificate: &'a [u8],
    pub predecessor_header: &'a [u8],
    pub predecessor_signature: [u8; 64],
    pub checkpoint_payload: &'a [u8],
    pub finality_proof: &'a [u8],
}

/// # Errors
/// Refuses hashing inputs outside the native bound.
pub fn sequencer_id(public_key: &[u8; 32]) -> Result<[u8; 32], HandoverError> {
    let mut preimage = [0_u8; 81];
    preimage[..17].copy_from_slice(b"layerx-sequencer:\0");
    let digits = b"0123456789abcdef";
    for (index, byte) in public_key.iter().enumerate() {
        preimage[17 + index * 2] = digits[usize::from(byte >> 4)];
        preimage[18 + index * 2] = digits[usize::from(byte & 15)];
    }
    sha256(&preimage).map_err(|_| HandoverError::Bounds)
}

/// # Errors
/// Refuses unsupported, noncanonical, overflowing or misbound certificates.
pub fn decode_certificate(bytes: &[u8]) -> Result<Certificate, HandoverError> {
    if bytes.len() != CERTIFICATE_BYTES {
        return Err(HandoverError::Bounds);
    }
    let mut reader = Reader(bytes);
    reader.domain(CERTIFICATE_DOMAIN)?;
    let value = Certificate {
        network_id: u32::from_be_bytes(reader.array()?),
        protocol_version: u16::from_be_bytes(reader.array()?),
        old_epoch: reader.u64()?,
        new_epoch: reader.u64()?,
        old_sequencer_id: reader.array()?,
        old_public_key: reader.array()?,
        new_sequencer_id: reader.array()?,
        new_public_key: reader.array()?,
        predecessor_batch: reader.u64()?,
        predecessor_last_sequence: reader.u64()?,
        predecessor_header_hash: reader.array()?,
        predecessor_state_root: reader.array()?,
        predecessor_checkpoint_id: reader.array()?,
        activation_batch: reader.u64()?,
        finality_evidence_digest: reader.array()?,
        governance_signature: reader.array()?,
    };
    reader.finish()?;
    if value.network_id == 0
        || value.protocol_version != 3
        || value.old_epoch == 0
        || value.old_epoch.checked_add(1) != Some(value.new_epoch)
        || value.predecessor_batch == 0
        || value.predecessor_batch.checked_add(1) != Some(value.activation_batch)
        || matches!(value.predecessor_last_sequence, 0 | u64::MAX)
        || value.old_public_key == value.new_public_key
        || value.predecessor_header_hash == [0; 32]
        || value.predecessor_state_root == [0; 32]
        || value.predecessor_checkpoint_id == [0; 32]
        || value.finality_evidence_digest == [0; 32]
        || sequencer_id(&value.old_public_key)? != value.old_sequencer_id
        || sequencer_id(&value.new_public_key)? != value.new_sequencer_id
    {
        return Err(HandoverError::Certificate);
    }
    Ok(value)
}

/// # Errors
/// Refuses malformed evidence, predecessor substitutions and mismatched finality bytes.
pub fn decode_evidence(bytes: &[u8]) -> Result<Evidence<'_>, HandoverError> {
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(HandoverError::Bounds);
    }
    let mut reader = Reader(bytes);
    reader.domain(EVIDENCE_DOMAIN)?;
    let encoded = reader.take(CERTIFICATE_BYTES)?;
    let value = Evidence {
        certificate: decode_certificate(encoded)?,
        signed_certificate: &encoded[..CERTIFICATE_BYTES - 64],
        predecessor_header: reader.span(354)?,
        predecessor_signature: reader.array()?,
        checkpoint_payload: reader.span(MAX_EVIDENCE_BYTES)?,
        finality_proof: reader.span(MAX_EVIDENCE_BYTES)?,
    };
    reader.finish()?;
    let certificate = &value.certificate;
    let header =
        decode_batch_header(value.predecessor_header).map_err(|_| HandoverError::Predecessor)?;
    if header.protocol_version() != certificate.protocol_version
        || header.network_id() != certificate.network_id
        || header.epoch() != certificate.old_epoch
        || header.batch_number() != certificate.predecessor_batch
        || header.last_sequence() != certificate.predecessor_last_sequence
        || header.sequencer_id() != certificate.old_sequencer_id
        || header.resulting_state_root() != certificate.predecessor_state_root
        || batch_header_digest(value.predecessor_header).map_err(|_| HandoverError::Predecessor)?
            != certificate.predecessor_header_hash
    {
        return Err(HandoverError::Predecessor);
    }
    let mut input = FINALITY_DOMAIN.to_vec();
    for part in [value.checkpoint_payload, value.finality_proof] {
        input.extend_from_slice(
            &u32::try_from(part.len())
                .map_err(|_| HandoverError::Bounds)?
                .to_be_bytes(),
        );
        input.extend_from_slice(part);
    }
    if sha256(&input).map_err(|_| HandoverError::Bounds)? != certificate.finality_evidence_digest {
        return Err(HandoverError::Finality);
    }
    Ok(value)
}

/// # Errors
/// Refuses nested envelopes, malformed native recovery fields and spliced evidence.
pub fn decode_recovery(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>), HandoverError> {
    if bytes.len() > MAX_RECOVERY_BYTES {
        return Err(HandoverError::Bounds);
    }
    if !bytes.starts_with(RECOVERY_DOMAIN) {
        validate_recovery(bytes)?;
        return Ok((bytes, None));
    }
    let mut reader = Reader(bytes);
    reader.domain(RECOVERY_DOMAIN)?;
    let recovery = reader.span(MAX_RECOVERY_BYTES)?;
    let evidence = reader.span(MAX_EVIDENCE_BYTES)?;
    reader.finish()?;
    if recovery.starts_with(RECOVERY_DOMAIN) {
        return Err(HandoverError::Encoding);
    }
    validate_recovery(recovery)?;
    decode_evidence(evidence)?;
    Ok((recovery, Some(evidence)))
}

fn validate_recovery(bytes: &[u8]) -> Result<(), HandoverError> {
    let mut reader = Reader(bytes);
    let count = u32::from_be_bytes(reader.array()?);
    if count > 256 {
        return Err(HandoverError::Bounds);
    }
    let mut previous = None;
    for _ in 0..count {
        let module = u16::from_be_bytes(reader.array()?);
        if previous.is_some_and(|last| module <= last) || reader.span(32)?.len() != 32 {
            return Err(HandoverError::Encoding);
        }
        previous = Some(module);
    }
    let frontier =
        usize::try_from(u32::from_be_bytes(reader.array()?)).map_err(|_| HandoverError::Bounds)?;
    if frontier > 1_048_576 {
        return Err(HandoverError::Bounds);
    }
    reader.take(frontier)?;
    let next = reader.u64()?;
    let receipt = reader.u64()?;
    let projection = reader.u64()?;
    if next == 0 || receipt >= next || projection > receipt {
        return Err(HandoverError::Encoding);
    }
    reader.finish()
}

struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], HandoverError> {
        let part = self.0.get(..count).ok_or(HandoverError::Encoding)?;
        self.0 = &self.0[count..];
        Ok(part)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], HandoverError> {
        self.take(N)?
            .try_into()
            .map_err(|_| HandoverError::Encoding)
    }
    fn u64(&mut self) -> Result<u64, HandoverError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn domain(&mut self, domain: &[u8]) -> Result<(), HandoverError> {
        if self.take(domain.len())? == domain {
            Ok(())
        } else {
            Err(HandoverError::Encoding)
        }
    }
    fn span(&mut self, maximum: usize) -> Result<&'a [u8], HandoverError> {
        let count = usize::try_from(u32::from_be_bytes(self.array()?))
            .map_err(|_| HandoverError::Bounds)?;
        if count == 0 || count > maximum {
            return Err(HandoverError::Bounds);
        }
        self.take(count)
    }
    fn finish(self) -> Result<(), HandoverError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(HandoverError::Encoding)
        }
    }
}
