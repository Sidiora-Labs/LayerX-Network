use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::amount::Amount;
use layerx_types::ids::AssetId;
use layerx_wire::hash;
use sha2::{Digest as _, Sha256};

use crate::{IntentError, IntentErrorReason, IntentField};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCustodyCredit {
    payload: Box<[u8; 427]>,
    reserve: AccountId,
    recipient: AccountId,
    asset: AssetId,
    amount: Amount,
    deposit_id: [u8; 32],
    nullifier: [u8; 32],
}

impl NativeCustodyCredit {
    /// # Errors
    /// Refuses malformed native credit bytes, invalid value, or account bindings.
    pub fn new(
        payload: &[u8; 427],
        reserve: AccountId,
        recipient: AccountId,
    ) -> Result<Self, IntentError> {
        let invalid = || IntentError {
            field: IntentField::DepositProof,
            reason: IntentErrorReason::InvalidCanonicalEncoding,
        };
        if !matches!(&payload[..5], b"LXDC1" | b"LXDC2")
            || payload[37..41] == [0; 4]
            || payload[41..43] != 3_u16.to_be_bytes()
            || reserve.namespace() != AccountNamespace::SystemPaxeerReserve
            || reserve == recipient
        {
            return Err(invalid());
        }
        let beneficiary = hash::account_id_for_protocol(&recipient, 3).map_err(|_| invalid())?;
        if payload[107..139] != beneficiary {
            return Err(IntentError {
                field: IntentField::Destination,
                reason: IntentErrorReason::InvalidCanonicalEncoding,
            });
        }
        let deposit_id = payload[43..75].try_into().map_err(|_| invalid())?;
        let asset = AssetId::new(payload[75..107].try_into().map_err(|_| invalid())?);
        let amount = Amount::from_be_bytes(payload[191..207].try_into().map_err(|_| invalid())?);
        if deposit_id == [0; 32] || asset.bytes() == [0; 32] || amount.value() == 0 {
            return Err(invalid());
        }
        let mut digest = Sha256::new();
        digest.update(b"LX:DEPOSIT:NULLIFIER:v1");
        digest.update(deposit_id);
        let nullifier = digest.finalize().into();
        Ok(Self {
            payload: Box::new(*payload),
            reserve,
            recipient,
            asset,
            amount,
            deposit_id,
            nullifier,
        })
    }

    #[must_use]
    pub fn payload(&self) -> &[u8; 427] {
        &self.payload
    }

    #[must_use]
    pub const fn reserve(&self) -> &AccountId {
        &self.reserve
    }

    #[must_use]
    pub const fn recipient(&self) -> &AccountId {
        &self.recipient
    }

    #[must_use]
    pub const fn asset(&self) -> AssetId {
        self.asset
    }

    #[must_use]
    pub const fn amount(&self) -> Amount {
        self.amount
    }

    #[must_use]
    pub const fn deposit_id(&self) -> [u8; 32] {
        self.deposit_id
    }

    #[must_use]
    pub const fn nullifier(&self) -> [u8; 32] {
        self.nullifier
    }
}
