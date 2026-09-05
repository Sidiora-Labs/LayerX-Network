use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

const NONCE_BYTES: usize = 16;
const TAG_BYTES: usize = 32;
const BLOCK_BYTES: usize = 32;
const MAX_PLAINTEXT_BYTES: usize = 4096;

pub struct StoreKey {
    encryption: Zeroizing<[u8; 32]>,
    authentication: Zeroizing<[u8; 32]>,
}

impl StoreKey {
    #[must_use]
    pub fn derive(secret: &[u8]) -> Self {
        Self {
            encryption: Zeroizing::new(labelled_digest(b"layerx-identity-seal-encryption", secret)),
            authentication: Zeroizing::new(labelled_digest(
                b"layerx-identity-seal-authentication",
                secret,
            )),
        }
    }

    pub fn seal(&self, plaintext: &[u8]) -> Result<String, String> {
        if plaintext.len() > MAX_PLAINTEXT_BYTES {
            return Err("sealed value exceeds its bound".to_owned());
        }
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| "entropy is unavailable".to_owned())?;
        let mut ciphertext = plaintext.to_vec();
        self.apply_keystream(&nonce, &mut ciphertext);
        let tag = self.tag(&nonce, &ciphertext);
        let mut sealed = Vec::with_capacity(NONCE_BYTES + ciphertext.len() + TAG_BYTES);
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(&ciphertext);
        sealed.extend_from_slice(&tag);
        Ok(hex(&sealed))
    }

    pub fn open(&self, sealed: &str) -> Result<Zeroizing<Vec<u8>>, String> {
        let bytes =
            Zeroizing::new(unhex(sealed).ok_or_else(|| "sealed value is not hex".to_owned())?);
        if bytes.len() < NONCE_BYTES + TAG_BYTES
            || bytes.len() > NONCE_BYTES + TAG_BYTES + MAX_PLAINTEXT_BYTES
        {
            return Err("sealed value has an invalid length".to_owned());
        }
        let (nonce, rest) = bytes.split_at(NONCE_BYTES);
        let (ciphertext, tag) = rest.split_at(rest.len() - TAG_BYTES);
        let expected = self.tag(nonce, ciphertext);
        if expected.ct_eq(tag).unwrap_u8() != 1 {
            return Err("sealed value failed authentication".to_owned());
        }
        let mut plaintext = Zeroizing::new(ciphertext.to_vec());
        self.apply_keystream(nonce, &mut plaintext);
        Ok(plaintext)
    }

    fn apply_keystream(&self, nonce: &[u8], buffer: &mut [u8]) {
        for (index, block) in buffer.chunks_mut(BLOCK_BYTES).enumerate() {
            let mut digest = Sha256::new();
            digest.update(self.encryption.as_slice());
            digest.update(nonce);
            digest.update((index as u64).to_be_bytes());
            let mut keystream: [u8; 32] = digest.finalize().into();
            for (byte, key) in block.iter_mut().zip(keystream.iter()) {
                *byte ^= key;
            }
            keystream.zeroize();
        }
    }

    fn tag(&self, nonce: &[u8], ciphertext: &[u8]) -> [u8; TAG_BYTES] {
        hmac_sha256(&self.authentication, &[nonce, ciphertext])
    }
}

fn labelled_digest(label: &[u8], secret: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(label);
    digest.update([0]);
    digest.update(secret);
    digest.finalize().into()
}

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

#[must_use]
pub fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    out
}

#[must_use]
pub fn unhex(value: &str) -> Option<Vec<u8>> {
    if value.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for pair in bytes.chunks(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_round_trips_and_binds_the_key() {
        let key = StoreKey::derive(b"store key one");
        let sealed = key
            .seal(b"csrf-token-value")
            .unwrap_or_else(|error| panic!("seal: {error}"));
        assert_ne!(sealed, hex(b"csrf-token-value"));
        let opened = key
            .open(&sealed)
            .unwrap_or_else(|error| panic!("open: {error}"));
        assert_eq!(opened.as_slice(), b"csrf-token-value");
        let other = StoreKey::derive(b"store key two");
        assert!(other.open(&sealed).is_err());
    }

    #[test]
    fn seal_uses_a_fresh_nonce_and_rejects_tampering() {
        let key = StoreKey::derive(b"store key");
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
        let mut key = [0_u8; 32];
        key[..4].copy_from_slice(b"Jefe");
        let mut padded_key = [0_u8; 32];
        padded_key[..4].copy_from_slice(b"Jefe");
        let tag = hmac_sha256(&padded_key, &[b"what do ya want ", b"for nothing?"]);
        assert_eq!(
            hex(&tag),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(key, padded_key);
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
        assert_eq!(unhex("00abff"), Some(vec![0x00, 0xab, 0xff]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("ABCD"), None);
        assert_eq!(sha256_hex(b"").len(), 64);
    }
}
