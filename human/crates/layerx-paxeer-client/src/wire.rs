//! Canonical bounded native representations for privilege-boundary exchange.

use layerx_types::intent::EvmAddress;

use crate::deposit::{DepositNativeError, DEPOSIT_NATIVE_PAYLOAD_MAX};
use crate::{
    ClaimRefusal, DebitExpectation, DebitFault, DepositFailure, DepositProof, ForcedExitMaterial,
    WithdrawalMaterial,
};

const VERSION: u8 = 1;
const DEBIT_TAG: u8 = 1;
const CHECKPOINT_TAG: u8 = 2;
const FINALITY_TAG: u8 = 3;
const DEPOSIT_TAG: u8 = 4;
const DEPOSIT_FAILURE_TAG: u8 = 5;
const LEGACY_DEPOSIT_PROOF_BYTES: usize = 2 + DEPOSIT_NATIVE_PAYLOAD_MAX;
pub const MAX_DEPOSIT_PROOF_BYTES: usize = LEGACY_DEPOSIT_PROOF_BYTES
    + 4
    + crate::NATIVE_CUSTODY_PROFILE_BYTES
    + crate::NATIVE_CUSTODY_CREDIT_MAX_BYTES;
pub const MAX_DEPOSIT_FAILURE_BYTES: usize = 65_538;
const FORCED_EXIT_TAG: u8 = 6;
const DEBIT_BYTES: usize = 1 + 1 + 32 + 4 + 32 + 32 + 32 + 32 + 16 + 20;

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

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
pub fn encode_deposit_proof(
    value: &DepositProof,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    if maximum_bytes == 0 {
        return Err(NativeWireError::Limit);
    }
    let overhead = value.native_credit().map_or(0, |credit| {
        4 + crate::NATIVE_CUSTODY_PROFILE_BYTES + credit.canonical_bytes().len()
    });
    let payload_limit = maximum_bytes
        .checked_sub(overhead)
        .ok_or(NativeWireError::Limit)?
        .min(LEGACY_DEPOSIT_PROOF_BYTES)
        .checked_sub(2)
        .ok_or(NativeWireError::Limit)?;
    let payload = value
        .encode_native(payload_limit)
        .map_err(map_deposit_error)?;
    let mut out = Vec::with_capacity(2 + payload.len());
    if let Some(credit) = value.native_credit() {
        out.extend_from_slice(&[2, DEPOSIT_TAG]);
        out.extend_from_slice(
            &u32::try_from(payload.len())
                .map_err(|_| NativeWireError::Limit)?
                .to_be_bytes(),
        );
        out.extend_from_slice(&payload);
        out.extend_from_slice(credit.profile_bytes());
        out.extend_from_slice(credit.canonical_bytes());
    } else {
        out.extend_from_slice(&[VERSION, DEPOSIT_TAG]);
        out.extend_from_slice(&payload);
    }
    bounded(out, maximum_bytes.min(MAX_DEPOSIT_PROOF_BYTES))
}

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
pub fn decode_deposit_proof(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<DepositProof, NativeWireError> {
    if maximum_bytes == 0 || bytes.len() > maximum_bytes || bytes.len() > MAX_DEPOSIT_PROOF_BYTES {
        return Err(NativeWireError::Encoding);
    }
    match bytes.get(..2) {
        Some([VERSION, DEPOSIT_TAG]) => {
            DepositProof::decode_native(&bytes[2..]).map_err(map_deposit_error)
        }
        Some([2, DEPOSIT_TAG]) => {
            let length = u32::from_be_bytes(
                bytes
                    .get(2..6)
                    .ok_or(NativeWireError::Encoding)?
                    .try_into()
                    .map_err(|_| NativeWireError::Encoding)?,
            ) as usize;
            let end = 6_usize.checked_add(length).ok_or(NativeWireError::Limit)?;
            let credit_start = end
                .checked_add(crate::NATIVE_CUSTODY_PROFILE_BYTES)
                .ok_or(NativeWireError::Limit)?;
            if length > DEPOSIT_NATIVE_PAYLOAD_MAX
                || bytes.len() <= credit_start + crate::NATIVE_CUSTODY_CREDIT_HEAD_BYTES
                || bytes.len() - credit_start > crate::NATIVE_CUSTODY_CREDIT_MAX_BYTES
            {
                return Err(NativeWireError::Encoding);
            }
            let proof = DepositProof::decode_native(&bytes[6..end]).map_err(map_deposit_error)?;
            let profile = &bytes[end..credit_start];
            let raw = &bytes[credit_start..];
            let owner_key = raw[139..171]
                .try_into()
                .map_err(|_| NativeWireError::Encoding)?;
            let credit = crate::NativeCustodyCredit::verify(
                profile,
                raw,
                crate::NativeCustodyExpectation {
                    network_id: proof.network_id(),
                    beneficiary: proof.custody().beneficiary,
                    owner_key,
                },
            )
            .map_err(|_| NativeWireError::Encoding)?;
            proof
                .with_native_credit(credit)
                .map_err(NativeWireError::Deposit)
        }
        _ => Err(NativeWireError::Encoding),
    }
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

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
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

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
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

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
pub fn encode_finality_report(
    value: &crate::FinalityReport,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    crate::finality::encode_wire(value, maximum_bytes, VERSION, FINALITY_TAG)
}

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
pub fn decode_finality_report(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<crate::FinalityReport, NativeWireError> {
    crate::finality::decode_wire(bytes, maximum_bytes, VERSION, FINALITY_TAG)
}

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
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

/// # Errors
/// Refuses invalid wire material and values exceeding the declared bounds.
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
    let value = DebitExpectation {
        activity_id: r.array()?,
        network_id: r.u32()?,
        withdrawal_id: r.array()?,
        account: r.array()?,
        withdrawals_account: r.array()?,
        asset_id: r.array()?,
        amount: r.u128()?,
        recipient: EvmAddress::new(r.array()?),
    }
    .validated()
    .map_err(NativeWireError::Debit)?;
    r.finish()?;
    Ok(value)
}

/// Encodes withdrawal material for privilege-boundary exchange: version,
/// tag, then the receipt, `0x4d50` proof and header as u32-length-prefixed
/// byte strings and the 64-byte sequencer header signature.
///
/// # Errors
/// Refuses material outside the custody evidence bounds and oversized output.
pub fn encode_withdrawal_material(
    value: &WithdrawalMaterial,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    let value = value
        .clone()
        .validated()
        .map_err(|_| NativeWireError::Checkpoint(ClaimRefusal::Material("bounds")))?;
    let mut out = vec![VERSION, CHECKPOINT_TAG];
    for part in [&value.receipt, &value.proof, &value.header] {
        out.extend_from_slice(
            &u32::try_from(part.len())
                .map_err(|_| NativeWireError::Limit)?
                .to_be_bytes(),
        );
        out.extend_from_slice(part);
    }
    out.extend_from_slice(&value.header_signature);
    bounded(out, maximum_bytes)
}

/// # Errors
/// Refuses malformed, noncanonical or oversized withdrawal material.
pub fn decode_withdrawal_material(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<WithdrawalMaterial, NativeWireError> {
    if bytes.len() > maximum_bytes {
        return Err(NativeWireError::Limit);
    }
    let mut r = Reader::new(bytes);
    if r.u8()? != VERSION || r.u8()? != CHECKPOINT_TAG {
        return Err(NativeWireError::Encoding);
    }
    let mut parts: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for part in &mut parts {
        let length = usize::try_from(r.u32()?).map_err(|_| NativeWireError::Limit)?;
        if length > r.remaining() {
            return Err(NativeWireError::Encoding);
        }
        *part = r.take(length)?.to_vec();
    }
    let header_signature = r.array()?;
    r.finish()?;
    let [receipt, proof, header] = parts;
    WithdrawalMaterial {
        receipt,
        proof,
        header,
        header_signature,
    }
    .validated()
    .map_err(|_| NativeWireError::Checkpoint(ClaimRefusal::Material("bounds")))
}

/// Encodes forced-exit material for privilege-boundary exchange.
///
/// # Errors
/// Refuses material outside the custody evidence bounds and oversized output.
pub fn encode_forced_exit_material(
    value: &ForcedExitMaterial,
    maximum_bytes: usize,
) -> Result<Vec<u8>, NativeWireError> {
    let value = value
        .clone()
        .validated()
        .map_err(|_| NativeWireError::Encoding)?;
    let mut out = vec![VERSION, FORCED_EXIT_TAG];
    out.extend_from_slice(&value.batch_number.to_be_bytes());
    out.extend_from_slice(&value.account);
    out.extend_from_slice(&value.asset_id);
    out.extend_from_slice(&value.recipient.bytes());
    out.extend_from_slice(&value.recipient_signature);
    out.extend_from_slice(
        &u32::try_from(value.witness.len())
            .map_err(|_| NativeWireError::Limit)?
            .to_be_bytes(),
    );
    out.extend_from_slice(&value.witness);
    bounded(out, maximum_bytes)
}

/// # Errors
/// Refuses malformed, noncanonical or oversized forced-exit material.
pub fn decode_forced_exit_material(
    bytes: &[u8],
    maximum_bytes: usize,
) -> Result<ForcedExitMaterial, NativeWireError> {
    if bytes.len() > maximum_bytes {
        return Err(NativeWireError::Limit);
    }
    let mut r = Reader::new(bytes);
    if r.u8()? != VERSION || r.u8()? != FORCED_EXIT_TAG {
        return Err(NativeWireError::Encoding);
    }
    let batch_number = r.u64()?;
    let account = r.array()?;
    let asset_id = r.array()?;
    let recipient = EvmAddress::new(r.array()?);
    let recipient_signature = r.array()?;
    let length = usize::try_from(r.u32()?).map_err(|_| NativeWireError::Limit)?;
    if length != r.remaining() {
        return Err(NativeWireError::Encoding);
    }
    let witness = r.take(length)?.to_vec();
    r.finish()?;
    ForcedExitMaterial {
        witness,
        batch_number,
        account,
        asset_id,
        recipient,
        recipient_signature,
    }
    .validated()
    .map_err(|_| NativeWireError::Encoding)
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
    fn u32(&mut self) -> Result<u32, NativeWireError> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, NativeWireError> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn u128(&mut self) -> Result<u128, NativeWireError> {
        Ok(u128::from_be_bytes(self.array()?))
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
