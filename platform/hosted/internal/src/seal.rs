//! Authenticated AES-256-GCM sealing for private material at rest.

use crate::secret::{hex, unhex};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const MAX_PLAINTEXT_BYTES: usize = 4096;

/// The derived seal keys.
pub struct SealKey {
    encryption: Zeroizing<[u8; 32]>,
}

impl SealKey {
    /// Derives the seal keys from a secret under a domain label.
    #[must_use]
    pub fn derive(label: &[u8], secret: &[u8]) -> Self {
        Self {
            encryption: Zeroizing::new(labelled_digest(label, b"encryption", secret)),
        }
    }

    /// Seals plaintext under a fresh nonce and returns hex.
    ///
    /// # Errors
    /// Returns a description when the plaintext exceeds its bound or entropy
    /// is unavailable.
    pub fn seal(&self, plaintext: &[u8]) -> Result<String, String> {
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err("sealed value exceeds its bound".to_owned());
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| "entropy is unavailable".to_owned())?;
        let mut ciphertext = plaintext.to_vec();
        self.aead()?
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut ciphertext,
            )
            .map_err(|_| "sealing failed".to_owned())?;
        let mut sealed = Vec::with_capacity(NONCE_BYTES + ciphertext.len());
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        Ok(hex(&sealed))
    }

    /// Authenticates and opens a sealed hex value.
    ///
    /// # Errors
    /// Returns a description when the value is malformed or fails
    /// authentication.
    pub fn open(&self, sealed: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let bytes =
            Zeroizing::new(unhex(sealed).ok_or_else(|| "sealed value is not hex".to_owned())?);
        if bytes.len() < NONCE_BYTES + TAG_BYTES
            || bytes.len() > NONCE_BYTES + TAG_BYTES + MAX_PLAINTEXT_BYTES
        {
            return Err("sealed value has an invalid length".to_owned());
        }
        let (nonce, ciphertext) = bytes.split_at(NONCE_BYTES);
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| "invalid nonce".to_owned())?;
        let mut plaintext = Zeroizing::new(ciphertext.to_vec());
        let length = self
            .aead()?
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::empty(),
                &mut plaintext,
            )
            .map_err(|_| "sealed value failed authentication".to_owned())?
            .len();
        plaintext.truncate(length);
        Ok(plaintext)
    }

    fn aead(&self) -> Result<LessSafeKey, String> {
        UnboundKey::new(&AES_256_GCM, self.encryption.as_slice())
            .map(LessSafeKey::new)
            .map_err(|_| "invalid seal key".to_owned())
    }
}

fn labelled_digest(label: &[u8], purpose: &[u8], secret: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(label);
    digest.update([0]);
    digest.update(purpose);
    digest.update([0]);
    digest.update(secret);
    digest.finalize().into()
}

/// HMAC-SHA-256 over the concatenation of `parts`.
#[must_use]
pub fn hmac_sha256(key: &[u8; 32], parts: &[&[u8]]) -> [u8; 32] {
    let mut inner_key = [0x36_u8; 64];
    let mut outer_key = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_key[index] ^= byte;
        outer_key[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_key);
    for part in parts {
        inner.update(part);
    }
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_digest);
    inner_key.zeroize();
    outer_key.zeroize();
    outer.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_round_trips_and_binds_the_key() {
        let key = SealKey::derive(b"layerx-kms", b"seal secret one");
        let sealed = key
            .seal(b"ed25519 seed")
            .unwrap_or_else(|error| panic!("seal: {error}"));
        assert_ne!(sealed, hex(b"ed25519 seed"));
        let opened = key
            .open(&sealed)
            .unwrap_or_else(|error| panic!("open: {error}"));
        assert_eq!(opened.as_slice(), b"ed25519 seed");
        assert!(SealKey::derive(b"layerx-kms", b"seal secret two")
            .open(&sealed)
            .is_err());
        assert!(SealKey::derive(b"other-label", b"seal secret one")
            .open(&sealed)
            .is_err());
    }

    #[test]
    fn seal_uses_a_fresh_nonce_and_rejects_tampering() {
        let key = SealKey::derive(b"layerx-kms", b"seal secret");
        let first = key
            .seal(b"value")
            .unwrap_or_else(|error| panic!("seal: {error}"));
        let second = key
            .seal(b"value")
            .unwrap_or_else(|error| panic!("seal: {error}"));
        assert_ne!(first, second);
        let mut tampered = unhex(&first).unwrap_or_default();
        tampered[NONCE_BYTES] ^= 0x01;
        assert!(key.open(&hex(&tampered)).is_err());
        assert!(key.open("zz").is_err());
        assert!(key.open(&hex(&[0_u8; 10])).is_err());
    }

    #[test]
    fn hmac_matches_rfc_4231_case_two() {
        let mut padded_key = [0_u8; 32];
        padded_key[..4].copy_from_slice(b"Jefe");
        let tag = hmac_sha256(&padded_key, &[b"what do ya want ", b"for nothing?"]);
        assert_eq!(
            hex(&tag),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
