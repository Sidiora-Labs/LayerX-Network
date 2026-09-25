use crate::config::StationConfig;
use crate::quote::{keccak, Address, Word};
use k256::ecdsa::SigningKey;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignerError {
    KeySource,
    InvalidKey,
    Signing,
}
impl std::fmt::Display for SignerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "relayer signer refused: {self:?}")
    }
}
impl std::error::Error for SignerError {}

pub trait QuoteSigner {
    fn address(&self) -> Address;
    /// # Errors
    /// Returns a sanitized refusal with no key material.
    fn sign_digest(&self, digest: Word) -> Result<[u8; 65], SignerError>;
}

pub struct LocalSigner {
    key: SigningKey,
}
impl LocalSigner {
    /// # Errors
    /// Refuses an absent or invalid key from the configuration's named environment variable.
    pub fn from_config(config: &StationConfig) -> Result<Self, SignerError> {
        config.validate().map_err(|_| SignerError::KeySource)?;
        let secret = Zeroizing::new(
            std::env::var(&config.relayer_key_env).map_err(|_| SignerError::KeySource)?,
        );
        Self::decode(&secret)
    }

    fn decode(secret: &str) -> Result<Self, SignerError> {
        let raw = secret.strip_prefix("0x").unwrap_or(secret);
        if raw.len() != 64 || !raw.is_ascii() {
            return Err(SignerError::InvalidKey);
        }
        let mut bytes = Zeroizing::new([0_u8; 32]);
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
                .map_err(|_| SignerError::InvalidKey)?;
        }
        let key = SigningKey::from_slice(bytes.as_ref()).map_err(|_| SignerError::InvalidKey)?;
        Ok(Self { key })
    }
}
impl QuoteSigner for LocalSigner {
    fn address(&self) -> Address {
        let point = self.key.verifying_key().to_encoded_point(false);
        let digest = keccak(&point.as_bytes()[1..]);
        let mut address = [0; 20];
        address.copy_from_slice(&digest[12..]);
        address
    }
    fn sign_digest(&self, digest: Word) -> Result<[u8; 65], SignerError> {
        let (signature, recovery) = self
            .key
            .sign_prehash_recoverable(&digest)
            .map_err(|_| SignerError::Signing)?;
        if recovery.to_byte() > 1 {
            return Err(SignerError::Signing);
        }
        let mut result = [0; 65];
        result[..64].copy_from_slice(&signature.to_bytes());
        result[64] = recovery.to_byte() + 27;
        Ok(result)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

    pub(crate) fn signer() -> Result<LocalSigner, SignerError> {
        LocalSigner::decode(&format!("{:064x}", 1))
    }
    #[test]
    fn actual_signature_recovers_sponsor() -> Result<(), Box<dyn std::error::Error>> {
        let signer = signer()?;
        let digest = keccak(b"quote digest");
        let bytes = signer.sign_digest(digest)?;
        let signature = Signature::from_slice(&bytes[..64])?;
        let recovery = RecoveryId::try_from(bytes[64] - 27)?;
        let recovered = VerifyingKey::recover_from_prehash(&digest, &signature, recovery)?;
        assert_eq!(&recovered, signer.key.verifying_key());
        assert!(signature.normalize_s().is_none());
        assert_eq!(
            signer.address(),
            [
                0x7e, 0x5f, 0x45, 0x52, 0x09, 0x1a, 0x69, 0x12, 0x5d, 0x5d, 0xfc, 0xb7, 0xb8, 0xc2,
                0x65, 0x90, 0x29, 0x39, 0x5b, 0xdf
            ]
        );
        Ok(())
    }
    #[test]
    fn errors_never_echo_key_material() {
        for invalid in [
            String::new(),
            "00".repeat(32),
            "ff".repeat(32),
            "sensitive-value".into(),
        ] {
            assert!(matches!(
                LocalSigner::decode(&invalid),
                Err(SignerError::InvalidKey)
            ));
        }
        assert_eq!(
            SignerError::InvalidKey.to_string(),
            "relayer signer refused: InvalidKey"
        );
        let mut config = crate::config::tests::config();
        config.relayer_key_env.clear();
        assert!(matches!(
            LocalSigner::from_config(&config),
            Err(SignerError::KeySource)
        ));
    }
}
