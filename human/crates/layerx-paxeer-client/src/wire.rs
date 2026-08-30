//! Canonical bounded native representations for privilege-boundary exchange.

use layerx_types::intent::EvmAddress;

use crate::deposit::{DepositNativeError, DEPOSIT_NATIVE_PAYLOAD_MAX};
use crate::{
    CheckpointProof, ClaimRefusal, DebitExpectation, DebitFault, DepositFailure, DepositProof,
    WithdrawalAttestation,
};

const VERSION: u8 = 1;
const DEBIT_TAG: u8 = 1;
const CHECKPOINT_TAG: u8 = 2;
const FINALITY_TAG: u8 = 3;
const DEPOSIT_TAG: u8 = 4;
const DEPOSIT_FAILURE_TAG: u8 = 5;
pub const MAX_DEPOSIT_PROOF_BYTES: usize = 2 + DEPOSIT_NATIVE_PAYLOAD_MAX;
pub const MAX_DEPOSIT_FAILURE_BYTES: usize = 65_538;
const MAX_CHECKPOINT_SIBLINGS: usize = 256;
const MAX_CHECKPOINT_ATTESTATIONS: usize = 4096;
const DEBIT_BYTES: usize = 1 + 1 + 32 + 4 + 32 + 32 + 32 + 32 + 16 + 20;
const ATTESTATION_BYTES: usize =
    2 + 4 + 8 + 20 + 8 + 32 + 32 + 32 + 8 + 32 + 1 + 1 + 1 + 8 + 20 + 32 + 32 + 1;

/// A structural or canonical wire refusal. Cryptographic policy failures stay
/// in their owning domain APIs and are never flattened into this error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeWireError {
    Encoding,
    Limit,
    Debit(DebitFault),
    Checkpoint(ClaimRefusal),
    Deposit(DepositFailure),
}

pub fn encode_deposit_proof(
    value: &DepositProof,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    if maximum_bytes == 0 {
        return Err(NativeWireError::Limit);
    }
    let payload_limit = maximum_bytes
        .min(MAX_DEPOSIT_PROOF_BYTES)
        .checked_sub(2)
        .ok_or(NativeWireError::Limit)?;
    let payload = value
        .encode_native(payload_limit)
        .map_err(map_deposit_error)?;
    let mut out = Vec::with_capacity(2 + payload.len());
    out.extend_from_slice(&[VERSION, DEPOSIT_TAG]);
    out.extend_from_slice(&payload);
    bounded(out, maximum_bytes.min(MAX_DEPOSIT_PROOF_BYTES))
}

pub fn decode_deposit_proof(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<DepositProof, NativeWireError> {
    if maximum_bytes == 0
        || bytes.len() > maximum_bytes
        || bytes.len() > MAX_DEPOSIT_PROOF_BYTES
        || bytes.get(..2) != Some(&[VERSION, DEPOSIT_TAG][..])
    {
        return Err(NativeWireError::Encoding);
    }
    DepositProof::decode_native(&bytes[2..]).map_err(map_deposit_error)
}

fn map_deposit_error(error: DepositNativeError) -> NativeWireError {
    match error {
        DepositNativeError::Encoding => NativeWireError::Encoding,
        DepositNativeError::Limit => NativeWireError::Limit,
        DepositNativeError::Custody(error) => {
            NativeWireError::Deposit(DepositFailure::CustodyFailed(error))
        }
        DepositNativeError::Proof(error) => {
            NativeWireError::Deposit(DepositFailure::ProofUnavailable(error))
        }
        DepositNativeError::Merkle(error) => NativeWireError::Deposit(
            DepositFailure::ProofUnavailable(crate::ProofFault::DepositInclusion(error)),
        ),
    }
}

pub fn encode_deposit_failure(
    value: &DepositFailure,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    if maximum_bytes < 3 {
        return Err(NativeWireError::Limit);
    }
    let payload = value
        .encode_failure_native(maximum_bytes.saturating_sub(2))
        .map_err(map_deposit_error)?;
    let mut out = Vec::with_capacity(payload.len().saturating_add(2));
    out.extend_from_slice(&[VERSION, DEPOSIT_FAILURE_TAG]);
    out.extend_from_slice(&payload);
    bounded(out, maximum_bytes.min(MAX_DEPOSIT_FAILURE_BYTES))
}

pub fn decode_deposit_failure(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<DepositFailure, NativeWireError> {
    if bytes.len() > maximum_bytes
        || bytes.len() > MAX_DEPOSIT_FAILURE_BYTES
        || bytes.get(..2) != Some(&[VERSION, DEPOSIT_FAILURE_TAG][..])
    {
        return Err(NativeWireError::Encoding);
    }
    DepositFailure::decode_failure_native(&bytes[2..]).map_err(map_deposit_error)
}

pub fn encode_finality_report(
    value: &crate::FinalityReport,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    crate::finality::encode_wire(value, maximum_bytes, VERSION, FINALITY_TAG)
}

pub fn decode_finality_report(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<crate::FinalityReport, NativeWireError> {
    crate::finality::decode_wire(bytes, maximum_bytes, VERSION, FINALITY_TAG)
}

pub fn encode_debit_expectation(
    value: &DebitExpectation,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    let mut out = Vec::with_capacity(DEBIT_BYTES);
    out.extend_from_slice(&[VERSION, DEBIT_TAG]);
    out.extend_from_slice(&value.activity_id);
    out.extend_from_slice(&value.network_id.to_be_bytes());
    out.extend_from_slice(&value.withdrawal_id);
    out.extend_from_slice(&value.account);
    out.extend_from_slice(&value.withdrawals_account);
    out.extend_from_slice(&value.asset_id);
    out.extend_from_slice(&value.amount.to_be_bytes());
    out.extend_from_slice(&value.recipient.bytes());
    bounded(out, maximum_bytes)
}

pub fn decode_debit_expectation(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<DebitExpectation, NativeWireError> {
    if bytes.len() != DEBIT_BYTES
        || bytes.len() > maximum_bytes
        || bytes[..2] != [VERSION, DEBIT_TAG]
    {
        return Err(NativeWireError::Encoding);
    }
    let mut r = Reader::new(&bytes[2..]);
    let value = DebitExpectation::validated(
        r.array()?,
        r.u32()?,
        r.array()?,
        r.array()?,
        r.array()?,
        r.array()?,
        r.u128()?,
        EvmAddress::new(r.array()?),
    )
    .map_err(NativeWireError::Debit)?;
    r.finish()?;
    Ok(value)
}

pub fn encode_checkpoint_proof(
    value: &CheckpointProof,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    if value.siblings.len() > MAX_CHECKPOINT_SIBLINGS
        || value.attestations.len() > MAX_CHECKPOINT_ATTESTATIONS
    {
        return Err(NativeWireError::Limit);
    }
    CheckpointProof::validated(
        value.checkpoint_hash,
        value.state_root,
        value.epoch,
        value.batch_number,
        value.data_availability_root,
        value.leaf_index,
        value.siblings.clone(),
        value.attestations.clone(),
    )
    .map_err(NativeWireError::Checkpoint)?;
    let mut out = Vec::new();
    out.extend_from_slice(&[VERSION, CHECKPOINT_TAG]);
    out.extend_from_slice(&value.checkpoint_hash);
    out.extend_from_slice(&value.state_root);
    out.extend_from_slice(&value.epoch.to_be_bytes());
    out.extend_from_slice(&value.batch_number.to_be_bytes());
    out.extend_from_slice(&value.data_availability_root);
    out.extend_from_slice(&value.leaf_index.to_be_bytes());
    out.extend_from_slice(&(value.siblings.len() as u16).to_be_bytes());
    for sibling in &value.siblings {
        out.extend_from_slice(sibling);
    }
    out.extend_from_slice(&(value.attestations.len() as u32).to_be_bytes());
    for value in &value.attestations {
        encode_attestation(&mut out, value);
    }
    bounded(out, maximum_bytes)
}

pub fn decode_checkpoint_proof(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<CheckpointProof, NativeWireError> {
    if bytes.len() > maximum_bytes || bytes.len() < 96 || bytes[..2] != [VERSION, CHECKPOINT_TAG] {
        return Err(NativeWireError::Encoding);
    }
    let mut r = Reader::new(&bytes[2..]);
    let checkpoint_hash = r.array()?;
    let state_root = r.array()?;
    let epoch = r.u64()?;
    let batch_number = r.u64()?;
    let data_availability_root = r.array()?;
    let leaf_index = r.u64()?;
    let sibling_count = usize::from(r.u16()?);
    if sibling_count > MAX_CHECKPOINT_SIBLINGS {
        return Err(NativeWireError::Limit);
    }
    let mut siblings = Vec::with_capacity(sibling_count);
    for _ in 0..sibling_count {
        siblings.push(r.array()?)
    }
    let attestation_count = usize::try_from(r.u32()?).map_err(|_| NativeWireError::Limit)?;
    if attestation_count > MAX_CHECKPOINT_ATTESTATIONS {
        return Err(NativeWireError::Limit);
    }
    let required = attestation_count
        .checked_mul(ATTESTATION_BYTES)
        .ok_or(NativeWireError::Limit)?;
    if r.remaining() != required {
        return Err(NativeWireError::Encoding);
    }
    let mut attestations = Vec::with_capacity(attestation_count);
    for _ in 0..attestation_count {
        attestations.push(decode_attestation(&mut r)?)
    }
    r.finish()?;
    CheckpointProof::validated(
        checkpoint_hash,
        state_root,
        epoch,
        batch_number,
        data_availability_root,
        leaf_index,
        siblings,
        attestations,
    )
    .map_err(NativeWireError::Checkpoint)
}

fn encode_attestation(out: &mut Vec<u8>, v: &WithdrawalAttestation) {
    out.extend_from_slice(&v.protocol_version.to_be_bytes());
    out.extend_from_slice(&v.network_id.to_be_bytes());
    out.extend_from_slice(&v.paxeer_chain_id.to_be_bytes());
    out.extend_from_slice(&v.settlement_contract.bytes());
    out.extend_from_slice(&v.epoch.to_be_bytes());
    out.extend_from_slice(&v.checkpoint_id);
    out.extend_from_slice(&v.checkpoint_hash);
    out.extend_from_slice(&v.guarantor_id);
    out.extend_from_slice(&v.batch_number.to_be_bytes());
    out.extend_from_slice(&v.data_availability_root);
    out.push(u8::from(v.replayed));
    out.push(u8::from(v.data_available));
    out.push(v.availability_class_mask);
    out.extend_from_slice(&v.attested_at.to_be_bytes());
    out.extend_from_slice(&v.signer.bytes());
    out.extend_from_slice(&v.signature_r);
    out.extend_from_slice(&v.signature_s);
    out.push(v.signature_v)
}
fn decode_attestation(r: &mut Reader<'_>) -> Result<WithdrawalAttestation, NativeWireError> {
    Ok(WithdrawalAttestation {
        protocol_version: r.u16()?,
        network_id: r.u32()?,
        paxeer_chain_id: r.u64()?,
        settlement_contract: EvmAddress::new(r.array()?),
        epoch: r.u64()?,
        checkpoint_id: r.array()?,
        checkpoint_hash: r.array()?,
        guarantor_id: r.array()?,
        batch_number: r.u64()?,
        data_availability_root: r.array()?,
        replayed: r.boolean()?,
        data_available: r.boolean()?,
        availability_class_mask: r.u8()?,
        attested_at: r.u64()?,
        signer: EvmAddress::new(r.array()?),
        signature_r: r.array()?,
        signature_s: r.array()?,
        signature_v: r.u8()?,
    })
}
fn bounded(bytes: Vec<u8>, maximum: usize) -> Result<Vec<u8>, NativeWireError> {
    if maximum == 0 || bytes.len() > maximum {
        Err(NativeWireError::Limit)
    } else {
        Ok(bytes)
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], NativeWireError> {
        let end = self.at.checked_add(n).ok_or(NativeWireError::Encoding)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(NativeWireError::Encoding)?;
        self.at = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], NativeWireError> {
        self.take(N)?
            .try_into()
            .map_err(|_| NativeWireError::Encoding)
    }
    fn u8(&mut self) -> Result<u8, NativeWireError> {
        Ok(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, NativeWireError> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, NativeWireError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, NativeWireError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, NativeWireError> {
        Ok(u128::from_be_bytes(self.array()?))
    }
    fn boolean(&mut self) -> Result<bool, NativeWireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(NativeWireError::Encoding),
        }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    fn finish(self) -> Result<(), NativeWireError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(NativeWireError::Encoding)
        }
    }
}
