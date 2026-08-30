//! Authenticated, identity-bound encryption for local private keys.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead as _, KeyInit as _, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::redact::Secret;

const FORMAT_VERSION: u8 = 1;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
const PRIVATE_KEY_BYTES: usize = 32;
const MAX_IDENTITY_BYTES: usize = 4096;
const CIPHERTEXT_BYTES: usize = PRIVATE_KEY_BYTES + 16;
const STORAGE_MAGIC: &[u8; 4] = b"LXKS";
const ARGON_MEMORY_KIB: u32 = 19 * 1024;
const ARGON_ITERATIONS: u32 = 2;
const ARGON_LANES: u32 = 1;

/// Encrypted private-key material plus its public authenticated context.
#[derive(Clone, Eq, PartialEq)]
pub struct Keystore {
    identity: Vec<u8>,
    network_id: u32,
    salt: [u8; SALT_BYTES],
    nonce: [u8; NONCE_BYTES],
    ciphertext: Vec<u8>,
}

/// Unique caller-supplied entropy for one encrypted keystore record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeystoreEntropy {
    salt: [u8; SALT_BYTES],
    nonce: [u8; NONCE_BYTES],
}

impl KeystoreEntropy {
    /// Accepts independently generated salt and nonce bytes.
    ///
    /// # Errors
    ///
    /// Refuses either all-zero value, which is never valid generated entropy.
    pub fn new(salt: [u8; SALT_BYTES], nonce: [u8; NONCE_BYTES]) -> Result<Self, KeystoreError> {
        if ct_nonzero(&salt) && ct_nonzero(&nonce) {
            Ok(Self { salt, nonce })
        } else {
            Err(KeystoreError::InvalidInput)
        }
    }
}

/// Typed keystore construction and opening failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeystoreError {
    /// Operator secret, identity, key, or network scope was invalid.
    InvalidInput,
    /// The stored identity differs from the one the operator requested.
    IdentityMismatch,
    /// The stored network differs from the one the operator requested.
    NetworkMismatch,
    /// Key derivation failed under the fixed Argon2id profile.
    KeyDerivation,
    /// The secret or authenticated ciphertext was wrong.
    AuthenticationFailed,
    /// Persisted bytes were truncated, oversized, or used another format.
    MalformedStorage,
}

impl fmt::Display for KeystoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "keystore input is invalid",
            Self::IdentityMismatch => "keystore identity does not match expected identity",
            Self::NetworkMismatch => "keystore network does not match expected network",
            Self::KeyDerivation => "keystore key derivation failed",
            Self::AuthenticationFailed => "keystore authentication failed",
            Self::MalformedStorage => "keystore storage bytes are malformed",
        })
    }
}

impl std::error::Error for KeystoreError {}

impl fmt::Debug for Keystore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Keystore")
            .field("identity_length", &self.identity.len())
            .field("network_id", &self.network_id)
            .field("ciphertext_length", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

fn authenticated_data(identity: &[u8], network_id: u32) -> Result<Vec<u8>, KeystoreError> {
    let identity_length = u32::try_from(identity.len()).map_err(|_| KeystoreError::InvalidInput)?;
    let mut output = Vec::with_capacity(1 + 4 + identity.len() + 4);
    output.push(FORMAT_VERSION);
    output.extend_from_slice(&identity_length.to_be_bytes());
    output.extend_from_slice(identity);
    output.extend_from_slice(&network_id.to_be_bytes());
    Ok(output)
}

fn derive_key(secret: &[u8], salt: &[u8; SALT_BYTES]) -> Result<Secret<[u8; 32]>, KeystoreError> {
    let params = Params::new(ARGON_MEMORY_KIB, ARGON_ITERATIONS, ARGON_LANES, Some(32))
        .map_err(|_| KeystoreError::KeyDerivation)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0_u8; 32];
    argon
        .hash_password_into(secret, salt, &mut output)
        .map_err(|_| KeystoreError::KeyDerivation)?;
    Ok(Secret::new(output))
}

fn ct_nonzero<const N: usize>(bytes: &[u8; N]) -> bool {
    !crate::ct::eq_fixed(bytes, &[0_u8; N])
}

impl Keystore {
    /// Opens a key under its exact authenticated context and lends the
    /// plaintext seed only to the supplied callback. The temporary plaintext
    /// remains in zeroizing storage and cannot be returned by reference.
    ///
    /// # Errors
    ///
    /// Returns the same identity, network, authentication and decoding
    /// failures as [`Self::open`].
    pub fn open_with<R>(
        &self,
        operator_secret: &[u8],
        expected_identity: &[u8],
        expected_network_id: u32,
        operation: impl FnOnce(&[u8; PRIVATE_KEY_BYTES]) -> R,
    ) -> Result<R, KeystoreError> {
        let key = self.open(operator_secret, expected_identity, expected_network_id)?;
        Ok(key.expose(operation))
    }

    /// Encrypts one Ed25519 seed under an operator secret and bound identity.
    ///
    /// # Errors
    ///
    /// Rejects empty secrets or identities, network zero, invalid supplied
    /// entropy, and cryptographic failures without exposing key material.
    pub fn seal(
        private_key: &[u8; PRIVATE_KEY_BYTES],
        operator_secret: &[u8],
        identity: &[u8],
        network_id: u32,
        entropy: KeystoreEntropy,
    ) -> Result<Self, KeystoreError> {
        if operator_secret.is_empty()
            || identity.is_empty()
            || identity.len() > MAX_IDENTITY_BYTES
            || network_id == 0
        {
            return Err(KeystoreError::InvalidInput);
        }
        let KeystoreEntropy { salt, nonce } = entropy;
        let derived = derive_key(operator_secret, &salt)?;
        let cipher = derived
            .expose(|value| XChaCha20Poly1305::new_from_slice(value))
            .map_err(|_| KeystoreError::KeyDerivation)?;
        let nonce_value = XNonce::from(nonce);
        let aad = authenticated_data(identity, network_id)?;
        let ciphertext = cipher
            .encrypt(
                &nonce_value,
                Payload {
                    msg: private_key,
                    aad: &aad,
                },
            )
            .map_err(|_| KeystoreError::AuthenticationFailed)?;
        Ok(Self {
            identity: identity.to_vec(),
            network_id,
            salt,
            nonce,
            ciphertext,
        })
    }

    /// Opens a key only under the exact expected identity and network.
    ///
    /// # Errors
    ///
    /// Returns explicit identity or network mismatch errors before attempting
    /// decryption, and one non-oracular authentication failure otherwise.
    pub fn open(
        &self,
        operator_secret: &[u8],
        expected_identity: &[u8],
        expected_network_id: u32,
    ) -> Result<Secret<[u8; PRIVATE_KEY_BYTES]>, KeystoreError> {
        if self.identity != expected_identity {
            return Err(KeystoreError::IdentityMismatch);
        }
        if self.network_id != expected_network_id {
            return Err(KeystoreError::NetworkMismatch);
        }
        if operator_secret.is_empty() {
            return Err(KeystoreError::InvalidInput);
        }
        let derived = derive_key(operator_secret, &self.salt)?;
        let cipher = derived
            .expose(|value| XChaCha20Poly1305::new_from_slice(value))
            .map_err(|_| KeystoreError::KeyDerivation)?;
        let nonce_value = XNonce::from(self.nonce);
        let aad = authenticated_data(&self.identity, self.network_id)?;
        let plaintext = Secret::new(
            cipher
                .decrypt(
                    &nonce_value,
                    Payload {
                        msg: &self.ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| KeystoreError::AuthenticationFailed)?,
        );
        let key = plaintext
            .expose(|value| value.as_slice().try_into())
            .map_err(|_| KeystoreError::AuthenticationFailed)?;
        Ok(Secret::new(key))
    }

    /// Borrows the identity covered by authenticated encryption.
    #[must_use]
    pub fn identity(&self) -> &[u8] {
        &self.identity
    }

    /// Returns the network covered by authenticated encryption.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Serializes the encrypted container without exposing plaintext material.
    ///
    /// # Errors
    ///
    /// Returns malformed-storage if the in-memory container cannot fit the
    /// canonical length fields.
    pub fn to_bytes(&self) -> Result<Vec<u8>, KeystoreError> {
        let identity_length =
            u32::try_from(self.identity.len()).map_err(|_| KeystoreError::MalformedStorage)?;
        let ciphertext_length =
            u32::try_from(self.ciphertext.len()).map_err(|_| KeystoreError::MalformedStorage)?;
        let mut output = Vec::with_capacity(
            4 + 1
                + 4
                + self.identity.len()
                + 4
                + SALT_BYTES
                + NONCE_BYTES
                + 4
                + self.ciphertext.len(),
        );
        output.extend_from_slice(STORAGE_MAGIC);
        output.push(FORMAT_VERSION);
        output.extend_from_slice(&identity_length.to_be_bytes());
        output.extend_from_slice(&self.identity);
        output.extend_from_slice(&self.network_id.to_be_bytes());
        output.extend_from_slice(&self.salt);
        output.extend_from_slice(&self.nonce);
        output.extend_from_slice(&ciphertext_length.to_be_bytes());
        output.extend_from_slice(&self.ciphertext);
        Ok(output)
    }

    /// Parses a persisted encrypted container without attempting decryption.
    ///
    /// # Errors
    ///
    /// Rejects every unknown version, truncated field, oversized identity,
    /// wrong ciphertext size, and trailing byte.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, KeystoreError> {
        let mut reader = Reader::new(bytes);
        if reader.take(4)? != STORAGE_MAGIC || reader.byte()? != FORMAT_VERSION {
            return Err(KeystoreError::MalformedStorage);
        }
        let identity_length = reader.length()?;
        if identity_length == 0 || identity_length > MAX_IDENTITY_BYTES {
            return Err(KeystoreError::MalformedStorage);
        }
        let identity = reader.take(identity_length)?.to_vec();
        let network_id = reader.u32()?;
        if network_id == 0 {
            return Err(KeystoreError::MalformedStorage);
        }
        let salt = reader
            .take(SALT_BYTES)?
            .try_into()
            .map_err(|_| KeystoreError::MalformedStorage)?;
        let nonce = reader
            .take(NONCE_BYTES)?
            .try_into()
            .map_err(|_| KeystoreError::MalformedStorage)?;
        if reader.length()? != CIPHERTEXT_BYTES {
            return Err(KeystoreError::MalformedStorage);
        }
        let ciphertext = reader.take(CIPHERTEXT_BYTES)?.to_vec();
        if !reader.is_finished() {
            return Err(KeystoreError::MalformedStorage);
        }
        Ok(Self {
            identity,
            network_id,
            salt,
            nonce,
            ciphertext,
        })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], KeystoreError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(KeystoreError::MalformedStorage)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(KeystoreError::MalformedStorage)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, KeystoreError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, KeystoreError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| KeystoreError::MalformedStorage)?;
        Ok(u32::from_be_bytes(bytes))
    }

    fn length(&mut self) -> Result<usize, KeystoreError> {
        usize::try_from(self.u32()?).map_err(|_| KeystoreError::MalformedStorage)
    }

    const fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}
