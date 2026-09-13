use layerx_crypto::payments::Payment;
use layerx_types::payload::ModuleId;

use crate::{IntentError, IntentErrorReason, IntentField};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeReceive {
    payload: Box<[u8; 733]>,
}

impl NativeReceive {
    /// # Errors
    /// Refuses noncanonical fields, missing authority, invalid grant or receiver signatures.
    pub fn new(payload: &[u8]) -> Result<Self, IntentError> {
        let invalid = || IntentError {
            field: IntentField::AuthorizationKey,
            reason: IntentErrorReason::InvalidCanonicalEncoding,
        };
        if !matches!(
            Payment::decode(ModuleId::Asset, 6, payload, b""),
            Ok(Payment::Receive { .. })
        ) {
            return Err(invalid());
        }
        Ok(Self {
            payload: Box::new(payload.try_into().map_err(|_| invalid())?),
        })
    }

    #[must_use]
    pub fn payload(&self) -> &[u8; 733] {
        &self.payload
    }
    #[must_use]
    pub fn from(&self) -> [u8; 32] {
        self.field(4)
    }
    #[must_use]
    pub fn to(&self) -> [u8; 32] {
        self.field(36)
    }
    #[must_use]
    pub fn asset(&self) -> [u8; 32] {
        self.field(68)
    }
    #[must_use]
    pub fn amount(&self) -> u128 {
        u128::from_be_bytes(self.field(100))
    }
    #[must_use]
    pub fn payer_grant(&self) -> [u8; 32] {
        self.field(116)
    }
    #[must_use]
    pub fn receiver_sequence(&self) -> u64 {
        u64::from_be_bytes(self.field(148))
    }
    #[must_use]
    pub fn idempotency_key(&self) -> [u8; 32] {
        self.field(156)
    }
    #[must_use]
    pub fn context_hash(&self) -> [u8; 32] {
        self.field(188)
    }
    #[must_use]
    pub fn network_id(&self) -> u32 {
        u32::from_be_bytes(self.field(381))
    }
    #[must_use]
    pub fn protocol_version(&self) -> u16 {
        u16::from_be_bytes(self.field(385))
    }
    fn field<const N: usize>(&self, offset: usize) -> [u8; N] {
        let mut out = [0; N];
        out.copy_from_slice(&self.payload[offset..offset + N]);
        out
    }
}
