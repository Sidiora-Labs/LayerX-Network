use layerx_types::{account::AccountId, ids::Did};
use layerx_wire::{decode::Decoder, encode::Encoder, hash};

const MAX_BYTES: usize = 512;
const DOMAIN: &[u8] = b"LX:SETTLE:RECIPIENT:v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecipientBindingError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipientAuthorization {
    pub network_id: u32,
    pub binding_digest: [u8; 32],
    pub did: Did,
    pub public_key: [u8; 32],
    pub asset: [u8; 32],
    pub checkpoint: [u8; 32],
    pub recipient: [u8; 20],
}

impl RecipientAuthorization {
    /// # Errors
    /// Refuses malformed keys or signatures and any changed recipient-binding message.
    pub fn verify_signature(&self, signature: &[u8; 64]) -> Result<(), RecipientBindingError> {
        let key = ed25519_dalek::VerifyingKey::from_bytes(&self.public_key)
            .map_err(|_| RecipientBindingError)?;
        if key.is_weak() {
            return Err(RecipientBindingError);
        }
        key.verify_strict(
            &self.message()?,
            &ed25519_dalek::Signature::from_bytes(signature),
        )
        .map_err(|_| RecipientBindingError)
    }

    /// # Errors
    /// Refuses an invalid native MAIN account or incomplete recipient scope.
    pub fn account(&self) -> Result<[u8; 32], RecipientBindingError> {
        let did = std::str::from_utf8(self.did.as_bytes()).map_err(|_| RecipientBindingError)?;
        let account =
            AccountId::parse(&format!("agent:{did}:main")).map_err(|_| RecipientBindingError)?;
        hash::account_id_for_protocol(&account, 3).map_err(|_| RecipientBindingError)
    }

    /// # Errors
    /// Refuses malformed identities or missing network, custody, asset and checkpoint pins.
    pub fn message(&self) -> Result<Vec<u8>, RecipientBindingError> {
        if self.network_id == 0
            || self.binding_digest == [0; 32]
            || self.asset == [0; 32]
            || self.checkpoint == [0; 32]
            || self.recipient == [0; 20]
            || !crate::ed25519::public_key_is_canonical(&self.public_key)
        {
            return Err(RecipientBindingError);
        }
        let mut message = DOMAIN.to_vec();
        message.extend_from_slice(&self.network_id.to_be_bytes());
        message.extend_from_slice(&self.account()?);
        message.extend_from_slice(&self.asset);
        message.extend_from_slice(&self.recipient);
        message.extend_from_slice(&self.checkpoint);
        Ok(message)
    }

    /// # Errors
    /// Refuses incomplete or oversized custody-bound authorizations.
    pub fn encode(&self) -> Result<Vec<u8>, RecipientBindingError> {
        self.message()?;
        let mut out = Encoder::new(MAX_BYTES);
        out.u16(1).map_err(|_| RecipientBindingError)?;
        out.u32(self.network_id)
            .map_err(|_| RecipientBindingError)?;
        out.fixed(&self.binding_digest)
            .map_err(|_| RecipientBindingError)?;
        out.bytes(self.did.as_bytes(), 256)
            .map_err(|_| RecipientBindingError)?;
        for value in [self.public_key, self.asset, self.checkpoint] {
            out.fixed(&value).map_err(|_| RecipientBindingError)?;
        }
        out.fixed(&self.recipient)
            .map_err(|_| RecipientBindingError)?;
        Ok(out.finish())
    }

    /// # Errors
    /// Refuses unknown versions, truncated or trailing data and noncanonical identity bindings.
    pub fn decode(bytes: &[u8]) -> Result<Self, RecipientBindingError> {
        if bytes.len() > MAX_BYTES {
            return Err(RecipientBindingError);
        }
        let mut input = Decoder::new(bytes, MAX_BYTES);
        if input.u16().map_err(|_| RecipientBindingError)? != 1 {
            return Err(RecipientBindingError);
        }
        let value = Self {
            network_id: input.u32().map_err(|_| RecipientBindingError)?,
            binding_digest: fixed(&mut input)?,
            did: Did::new(&input.bytes_owned(256).map_err(|_| RecipientBindingError)?)
                .map_err(|_| RecipientBindingError)?,
            public_key: fixed(&mut input)?,
            asset: fixed(&mut input)?,
            checkpoint: fixed(&mut input)?,
            recipient: fixed(&mut input)?,
        };
        input.finish().map_err(|_| RecipientBindingError)?;
        if value.encode()? != bytes {
            return Err(RecipientBindingError);
        }
        Ok(value)
    }
}

fn fixed<const N: usize>(reader: &mut Decoder<'_>) -> Result<[u8; N], RecipientBindingError> {
    reader
        .fixed(N)
        .map_err(|_| RecipientBindingError)?
        .try_into()
        .map_err(|_| RecipientBindingError)
}
