use super::{
    account_address_for_protocol, decode_native_inclusion, AdmittedCustody, CreditFault,
    CustodyDeposit, DepositFailure, DepositNativeError, DepositReader,
};
use crate::{AttestedNativeCustodyCredit, ExecutionOutcome, NativeCustodyEvidence,
    NativeCustodyExpectation, TransactionHash, TransactionInclusion};
use layerx_intents::Intent;
use layerx_types::{account::{AccountId, AccountNamespace}, intent::EvmAddress};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeDepositAdmission {
    transaction: TransactionHash,
    inclusion: TransactionInclusion,
    confirmations: u64,
    required: u64,
    custody_reference: [u8; 32],
    credit: AttestedNativeCustodyCredit,
}

impl NativeDepositAdmission {
    /// # Errors
    /// Refuses any difference between quorum-admitted custody and its native attestation.
    pub fn new(custody: &AdmittedCustody, credit: AttestedNativeCustodyCredit)
        -> Result<Self, DepositFailure>
    {
        let profile = credit.profile_bytes();
        if credit.custody() != &custody.custody()
            || profile[5..13] != custody.chain_id().to_be_bytes()
            || profile[13..33] != custody.vault().bytes()
            || profile[201..205] != custody.network_id().to_be_bytes()
            || custody.protocol_version() != 3
        {
            return Err(binding());
        }
        let value = Self {
            transaction: custody.transaction(), inclusion: custody.inclusion(),
            confirmations: custody.confirmations(), required: custody.required_confirmations(),
            custody_reference: custody.custody_reference(), credit,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), DepositFailure> {
        if self.transaction.bytes() == [0; 32] || self.custody_reference == [0; 32]
            || self.inclusion.block.number == 0 || self.inclusion.block.hash == [0; 32]
            || self.inclusion.execution != ExecutionOutcome::Succeeded
            || self.inclusion.deployed_contract.is_some()
            || self.required == 0 || self.confirmations < self.required
            || self.credit.profile_bytes()[161..169] != self.required.to_be_bytes()
        {
            return Err(binding());
        }
        match self.credit.evidence() {
            NativeCustodyEvidence::EthereumReceipt { inclusion_height, block_hash,
                transaction_hash, .. } => {
                if *inclusion_height != self.inclusion.block.number
                    || *block_hash != self.inclusion.block.hash
                    || *transaction_hash != self.transaction.bytes()
                { return Err(binding()); }
            }
            NativeCustodyEvidence::CometState { state_height, .. } => {
                if *state_height < self.inclusion.block.number { return Err(binding()); }
            }
        }
        Ok(())
    }

    #[must_use]
    pub const fn transaction(&self) -> TransactionHash { self.transaction }
    #[must_use]
    pub const fn inclusion(&self) -> TransactionInclusion { self.inclusion }
    #[must_use]
    pub fn custody(&self) -> CustodyDeposit { *self.credit.custody() }
    #[must_use]
    pub fn nullifier(&self) -> [u8; 32] { self.credit.nullifier() }
    #[must_use]
    pub fn chain_id(&self) -> u64 {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&self.credit.profile_bytes()[5..13]);
        u64::from_be_bytes(bytes)
    }
    #[must_use]
    pub fn network_id(&self) -> u32 {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(&self.credit.profile_bytes()[201..205]);
        u32::from_be_bytes(bytes)
    }
    #[must_use]
    pub fn vault(&self) -> EvmAddress {
        let mut bytes = [0; 20];
        bytes.copy_from_slice(&self.credit.profile_bytes()[13..33]);
        EvmAddress::new(bytes)
    }
    #[must_use]
    pub const fn native_credit(&self) -> &AttestedNativeCustodyCredit { &self.credit }

    /// # Errors
    /// Refuses an unrelated reserve or recipient; only the attested native payload is submitted.
    pub fn credit_intent(&self, reserve: &AccountId, recipient: &AccountId)
        -> Result<Intent, DepositFailure>
    {
        if reserve.namespace() != AccountNamespace::SystemPaxeerReserve {
            return Err(DepositFailure::CreditRefused(CreditFault::ReserveNamespace));
        }
        let address = account_address_for_protocol(recipient, 3).map_err(|_| binding())?;
        if address != self.custody().beneficiary {
            return Err(DepositFailure::CreditRefused(CreditFault::BeneficiaryMismatch {
                beneficiary: self.custody().beneficiary, recipient: address,
            }));
        }
        let native = layerx_intents::NativeCustodyCredit::new(
            self.credit.canonical_bytes(), reserve.clone(), recipient.clone(),
        ).map_err(|_| binding())?;
        Ok(Intent::v1(layerx_intents::IntentKind::NativeCustodyCredit(native)))
    }

    pub(crate) fn encode_native(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.transaction.bytes());
        out.extend_from_slice(&self.inclusion.block.number.to_be_bytes());
        out.extend_from_slice(&self.inclusion.block.hash);
        out.extend_from_slice(&self.inclusion.transaction_index.to_be_bytes());
        out.extend_from_slice(&[1, 0]);
        out.extend_from_slice(&[0; 20]);
        out.extend_from_slice(&self.confirmations.to_be_bytes());
        out.extend_from_slice(&self.required.to_be_bytes());
        out.extend_from_slice(&self.custody_reference);
        out.extend_from_slice(self.credit.profile_bytes());
        out.extend_from_slice(self.credit.canonical_bytes());
        out
    }

    pub(crate) fn decode_native(bytes: &[u8]) -> Result<Self, DepositNativeError> {
        let mut reader = DepositReader::new(bytes);
        let transaction = TransactionHash::new(reader.array()?);
        let inclusion = decode_native_inclusion(&mut reader)?;
        let confirmations = reader.u64()?;
        let required = reader.u64()?;
        let custody_reference = reader.array()?;
        let profile = reader.take(207)?;
        let payload = reader.take(427)?;
        let credit = AttestedNativeCustodyCredit::verify(profile, payload,
            NativeCustodyExpectation {
                network_id: u32::from_be_bytes(profile[201..205].try_into()
                    .map_err(|_| DepositNativeError::Encoding)?),
                beneficiary: payload[107..139].try_into()
                    .map_err(|_| DepositNativeError::Encoding)?,
                owner_key: payload[139..171].try_into()
                    .map_err(|_| DepositNativeError::Encoding)?,
            }).map_err(|_| DepositNativeError::Encoding)?;
        reader.finish()?;
        let value = Self { transaction, inclusion, confirmations, required,
            custody_reference, credit };
        value.validate().map_err(|_| DepositNativeError::Encoding)?;
        Ok(value)
    }
}

fn binding() -> DepositFailure {
    DepositFailure::CreditRefused(CreditFault::NativeBinding)
}
