//! One secret, one account on both sides of the Paxeer X Network.
//!
//! A Paxeer EVM account is a secp256k1 key and a `LayerX` identity is an
//! Ed25519 key. Both are derived here from one user secret:
//!
//! * a BIP-39 mnemonic: the EVM key at `m/44'/60'/0'/0/i` (BIP-32) and the
//!   `LayerX` key at `m/44'/19544'/i'/0'` (SLIP-0010 Ed25519), the same
//!   account index on both sides;
//! * an external wallet that never reveals its seed: the wallet signs one
//!   fixed EIP-712 message and the `LayerX` seed is HKDF-SHA256 over the
//!   canonical 65-byte signature.

use std::fmt::Write as _;

use bip39::{Language, Mnemonic};
use ed25519_dalek::SigningKey;
use hkdf::Hkdf;
use hmac::{Hmac, Mac as _};
use k256::ecdsa::{RecoveryId, Signature, SigningKey as EvmSigningKey, VerifyingKey};
use k256::elliptic_curve::PrimeField as _;
use k256::Scalar;
use sha2::{Sha256, Sha512};
use sha3::{Digest as _, Keccak256};
use zeroize::{Zeroize as _, Zeroizing};

use crate::secp256k1::evm_address;

/// SLIP-0044 style coin type of the `LayerX` branch: `0x4c58`, ASCII `LX`.
/// The value is unregistered in SLIP-0044 and collides with no listed coin.
pub const LAYERX_COIN_TYPE: u32 = 19_544;
/// BIP-44 coin type of every EVM account.
pub const EVM_COIN_TYPE: u32 = 60;
/// The origin the published wallet message names.
pub const CANONICAL_ORIGIN: &str = "https://app.layerx.network";
/// EIP-712 domain name of the wallet message.
pub const DOMAIN_NAME: &str = "Paxeer X Network";
/// EIP-712 domain version of the wallet message.
pub const DOMAIN_VERSION: &str = "1";
/// The `purpose` field of the wallet message.
pub const PURPOSE: &str = "Derive your LayerX account key";
/// The `version` field of the wallet message and of this derivation.
pub const DERIVATION_VERSION: u32 = 1;
/// HKDF salt of the wallet-signature derivation.
pub const HKDF_SALT: &[u8] = b"paxeer-x-network/layerx-account-key/v1";
/// Prefix of the HKDF info string, followed by chain id, address and index.
pub const HKDF_INFO_PREFIX: &[u8] = b"LX:ACCOUNT-KEY:v1";

const HARDENED: u32 = 0x8000_0000;
const MAX_CHAIN_ID: u64 = (1 << 53) - 1;
const DOMAIN_TYPE: &[u8] = b"EIP712Domain(string name,string version,uint256 chainId)";
const MESSAGE_TYPE: &[u8] = b"LayerXKeyDerivation(string purpose,string warning,address address,uint32 index,uint32 version)";

type HmacSha512 = Hmac<Sha512>;

/// Why a derivation was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DerivationError {
    /// The phrase is not a checksummed English BIP-39 mnemonic.
    InvalidMnemonic,
    /// The seed is outside the 16 to 64 bytes BIP-32 and SLIP-0010 accept.
    InvalidSeed,
    /// The account index does not fit a hardened path component.
    IndexOutOfRange,
    /// A BIP-32 step produced an unusable scalar; use the next index.
    InvalidChild,
    /// The chain id exceeds `2^53 - 1`, the largest value exact in JSON.
    InvalidChainId,
    /// The origin is not a plain lower-case `http(s)://host[:port]`.
    InvalidOrigin,
    /// The wallet signature is malformed or carries an unknown recovery byte.
    InvalidSignature,
    /// The signature does not recover to the address the message names.
    SignerMismatch,
}

impl std::fmt::Display for DerivationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMnemonic => "invalid_mnemonic",
            Self::InvalidSeed => "invalid_seed",
            Self::IndexOutOfRange => "index_out_of_range",
            Self::InvalidChild => "invalid_child_key",
            Self::InvalidChainId => "invalid_chain_id",
            Self::InvalidOrigin => "invalid_origin",
            Self::InvalidSignature => "invalid_wallet_signature",
            Self::SignerMismatch => "wallet_signer_mismatch",
        })
    }
}

impl std::error::Error for DerivationError {}

/// The pair of keys one secret yields: a Paxeer EVM account and a `LayerX`
/// identity. The EVM secret is absent when an external wallet holds it.
pub struct DerivedAccount {
    index: u32,
    evm_address: [u8; 20],
    evm_secret: Option<Zeroizing<[u8; 32]>>,
    layerx_seed: Zeroizing<[u8; 32]>,
    layerx_public_key: [u8; 32],
}

impl DerivedAccount {
    fn new(
        index: u32,
        evm_address: [u8; 20],
        evm_secret: Option<Zeroizing<[u8; 32]>>,
        layerx_seed: Zeroizing<[u8; 32]>,
    ) -> Self {
        let layerx_public_key = SigningKey::from_bytes(&layerx_seed)
            .verifying_key()
            .to_bytes();
        Self {
            index,
            evm_address,
            evm_secret,
            layerx_seed,
            layerx_public_key,
        }
    }

    /// The account index shared by both keys.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The 20-byte Paxeer EVM address.
    #[must_use]
    pub const fn evm_address(&self) -> [u8; 20] {
        self.evm_address
    }

    /// The EIP-55 checksummed `0x` form of the EVM address.
    #[must_use]
    pub fn evm_address_text(&self) -> String {
        checksum_address(&self.evm_address)
    }

    /// The `LayerX` Ed25519 public key.
    #[must_use]
    pub const fn layerx_public_key(&self) -> [u8; 32] {
        self.layerx_public_key
    }

    /// The `did:layerx:<64 hex>` identifier of the `LayerX` key.
    #[must_use]
    pub fn did(&self) -> String {
        format!("did:layerx:{}", hexadecimal(&self.layerx_public_key))
    }

    /// The private Ed25519 seed. Never print or persist it in the clear.
    #[must_use]
    pub fn layerx_seed(&self) -> &[u8; 32] {
        &self.layerx_seed
    }

    /// The private secp256k1 scalar, present only for mnemonic derivations.
    #[must_use]
    pub fn evm_secret(&self) -> Option<&[u8; 32]> {
        self.evm_secret.as_deref()
    }
}

impl std::fmt::Debug for DerivedAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DerivedAccount")
            .field("index", &self.index)
            .field("evm_address", &self.evm_address_text())
            .field("did", &self.did())
            .finish_non_exhaustive()
    }
}

fn hexadecimal(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Renders an EVM address with its EIP-55 checksum.
#[must_use]
pub fn checksum_address(address: &[u8; 20]) -> String {
    let lower = hexadecimal(address);
    let digest = Keccak256::digest(lower.as_bytes());
    let mut text = String::with_capacity(42);
    text.push_str("0x");
    for (position, character) in lower.chars().enumerate() {
        let nibble = (digest[position / 2] >> (4 * (1 - position % 2))) & 0x0f;
        if nibble >= 8 {
            text.push(character.to_ascii_uppercase());
        } else {
            text.push(character);
        }
    }
    text
}

fn hmac_sha512(key: &[u8], parts: &[&[u8]]) -> Result<Zeroizing<[u8; 64]>, DerivationError> {
    let mut mac = HmacSha512::new_from_slice(key).map_err(|_| DerivationError::InvalidSeed)?;
    for part in parts {
        mac.update(part);
    }
    let mut output = Zeroizing::new([0_u8; 64]);
    output.copy_from_slice(&mac.finalize().into_bytes());
    Ok(output)
}

fn check_seed(seed: &[u8]) -> Result<(), DerivationError> {
    if (16..=64).contains(&seed.len()) {
        Ok(())
    } else {
        Err(DerivationError::InvalidSeed)
    }
}

fn split(node: &[u8; 64]) -> (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>) {
    let mut key = Zeroizing::new([0_u8; 32]);
    let mut chain = Zeroizing::new([0_u8; 32]);
    key.copy_from_slice(&node[..32]);
    chain.copy_from_slice(&node[32..]);
    (key, chain)
}

/// SLIP-0010 Ed25519 private key at a hardened-only path. Every component is
/// given without the hardening bit, which is always applied.
///
/// # Errors
///
/// Refuses a seed outside 16 to 64 bytes and a component of `2^31` or more.
pub fn slip10_ed25519(seed: &[u8], path: &[u32]) -> Result<Zeroizing<[u8; 32]>, DerivationError> {
    check_seed(seed)?;
    let (mut key, mut chain) = split(&*hmac_sha512(b"ed25519 seed", &[seed])?);
    for component in path {
        if *component >= HARDENED {
            return Err(DerivationError::IndexOutOfRange);
        }
        let index = (component | HARDENED).to_be_bytes();
        let node = hmac_sha512(&chain[..], &[&[0_u8], &key[..], &index])?;
        (key, chain) = split(&node);
    }
    Ok(key)
}

fn secp256k1_scalar(bytes: &[u8; 32]) -> Result<Scalar, DerivationError> {
    Option::<Scalar>::from(Scalar::from_repr((*bytes).into()))
        .filter(|scalar| !bool::from(scalar.is_zero()))
        .ok_or(DerivationError::InvalidChild)
}

fn compressed_public_key(secret: &[u8; 32]) -> Result<[u8; 33], DerivationError> {
    let secret = EvmSigningKey::from_slice(secret).map_err(|_| DerivationError::InvalidChild)?;
    let point = secret.verifying_key().to_encoded_point(true);
    <[u8; 33]>::try_from(point.as_bytes()).map_err(|_| DerivationError::InvalidChild)
}

/// BIP-32 secp256k1 private key at a path whose components carry their own
/// hardening bit.
///
/// # Errors
///
/// Refuses a seed outside 16 to 64 bytes and the negligible case of an
/// unusable intermediate scalar.
pub fn bip32_secp256k1(seed: &[u8], path: &[u32]) -> Result<Zeroizing<[u8; 32]>, DerivationError> {
    check_seed(seed)?;
    let (mut key, mut chain) = split(&*hmac_sha512(b"Bitcoin seed", &[seed])?);
    secp256k1_scalar(&key)?;
    for component in path {
        let index = component.to_be_bytes();
        let node = if component & HARDENED == 0 {
            let public = compressed_public_key(&key)?;
            hmac_sha512(&chain[..], &[&public, &index])?
        } else {
            hmac_sha512(&chain[..], &[&[0_u8], &key[..], &index])?
        };
        let (tweak, next_chain) = split(&node);
        let mut child = secp256k1_scalar(&tweak)? + secp256k1_scalar(&key)?;
        if bool::from(child.is_zero()) {
            return Err(DerivationError::InvalidChild);
        }
        key.copy_from_slice(&child.to_repr());
        child.zeroize();
        chain = next_chain;
    }
    Ok(key)
}

/// The BIP-32 path of the EVM key for an account index.
#[must_use]
pub const fn evm_path(index: u32) -> [u32; 5] {
    [44 | HARDENED, EVM_COIN_TYPE | HARDENED, HARDENED, 0, index]
}

/// The hardened-only SLIP-0010 path of the `LayerX` key for an account index.
#[must_use]
pub const fn layerx_path(index: u32) -> [u32; 4] {
    [44, LAYERX_COIN_TYPE, index, 0]
}

/// Derives both keys of account `index` from a BIP-32 seed.
///
/// # Errors
///
/// Refuses an index of `2^31` or more, a seed of the wrong size and an
/// unusable BIP-32 child.
pub fn derive_from_seed(seed: &[u8], index: u32) -> Result<DerivedAccount, DerivationError> {
    if index >= HARDENED {
        return Err(DerivationError::IndexOutOfRange);
    }
    let evm_secret = bip32_secp256k1(seed, &evm_path(index))?;
    let public = compressed_public_key(&evm_secret)?;
    let address = evm_address(&public).map_err(|_| DerivationError::InvalidChild)?;
    let layerx_seed = slip10_ed25519(seed, &layerx_path(index))?;
    Ok(DerivedAccount::new(
        index,
        address,
        Some(evm_secret),
        layerx_seed,
    ))
}

/// Derives both keys of account `index` from an English BIP-39 mnemonic and
/// its optional passphrase (empty for none).
///
/// # Errors
///
/// Refuses a phrase whose words or checksum are wrong, and everything
/// [`derive_from_seed`] refuses.
pub fn derive_from_mnemonic(
    mnemonic: &str,
    passphrase: &str,
    index: u32,
) -> Result<DerivedAccount, DerivationError> {
    let parsed = Mnemonic::parse_in_normalized(Language::English, mnemonic.trim())
        .map_err(|_| DerivationError::InvalidMnemonic)?;
    let seed = Zeroizing::new(parsed.to_seed(passphrase));
    derive_from_seed(&seed[..], index)
}

/// The one message an external wallet signs to derive a `LayerX` key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyDerivationRequest {
    chain_id: u64,
    address: [u8; 20],
    index: u32,
    origin: String,
}

fn origin_is_plain(origin: &str) -> bool {
    let Some(rest) = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))
    else {
        return false;
    };
    let (host, port) = rest
        .split_once(':')
        .map_or((rest, None), |(host, port)| (host, Some(port)));
    !host.is_empty()
        && host.len() <= 253
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
        })
        && port.is_none_or(|port| {
            (1..=5).contains(&port.len()) && port.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn word_u64(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn keccak(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

impl KeyDerivationRequest {
    /// The message for `address` on chain `chain_id`, account `index`, shown
    /// under [`CANONICAL_ORIGIN`].
    ///
    /// # Errors
    ///
    /// Refuses an index of `2^31` or more.
    pub fn new(chain_id: u64, address: [u8; 20], index: u32) -> Result<Self, DerivationError> {
        Self::with_origin(chain_id, address, index, CANONICAL_ORIGIN)
    }

    /// The same message naming another origin. A different origin is a
    /// different message and therefore a different `LayerX` key.
    ///
    /// # Errors
    ///
    /// Refuses an index of `2^31` or more, a chain id above `2^53 - 1` and an
    /// origin that is not a plain lower-case `http(s)://host[:port]`.
    pub fn with_origin(
        chain_id: u64,
        address: [u8; 20],
        index: u32,
        origin: &str,
    ) -> Result<Self, DerivationError> {
        if index >= HARDENED {
            return Err(DerivationError::IndexOutOfRange);
        }
        if chain_id > MAX_CHAIN_ID {
            return Err(DerivationError::InvalidChainId);
        }
        if !origin_is_plain(origin) {
            return Err(DerivationError::InvalidOrigin);
        }
        Ok(Self {
            chain_id,
            address,
            index,
            origin: origin.to_owned(),
        })
    }

    /// The chain the message is bound to.
    #[must_use]
    pub const fn chain_id(&self) -> u64 {
        self.chain_id
    }

    /// The EVM address that must sign.
    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    /// The account index.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The `warning` field exactly as the wallet renders it.
    #[must_use]
    pub fn warning(&self) -> String {
        format!(
            "Only sign this on {}. Anyone holding this signature controls your LayerX account.",
            self.origin
        )
    }

    /// The `eth_signTypedData_v4` document.
    #[must_use]
    pub fn typed_data_json(&self) -> String {
        format!(
            concat!(
                "{{\"types\":{{\"EIP712Domain\":[{{\"name\":\"name\",\"type\":\"string\"}},",
                "{{\"name\":\"version\",\"type\":\"string\"}},{{\"name\":\"chainId\",\"type\":\"uint256\"}}],",
                "\"LayerXKeyDerivation\":[{{\"name\":\"purpose\",\"type\":\"string\"}},",
                "{{\"name\":\"warning\",\"type\":\"string\"}},{{\"name\":\"address\",\"type\":\"address\"}},",
                "{{\"name\":\"index\",\"type\":\"uint32\"}},{{\"name\":\"version\",\"type\":\"uint32\"}}]}},",
                "\"primaryType\":\"LayerXKeyDerivation\",",
                "\"domain\":{{\"name\":\"{}\",\"version\":\"{}\",\"chainId\":{}}},",
                "\"message\":{{\"purpose\":\"{}\",\"warning\":\"{}\",\"address\":\"0x{}\",\"index\":{},\"version\":{}}}}}"
            ),
            DOMAIN_NAME,
            DOMAIN_VERSION,
            self.chain_id,
            PURPOSE,
            self.warning(),
            hexadecimal(&self.address),
            self.index,
            DERIVATION_VERSION,
        )
    }

    /// The EIP-712 digest the wallet signs.
    #[must_use]
    pub fn signing_hash(&self) -> [u8; 32] {
        let domain = keccak(&[
            &keccak(&[DOMAIN_TYPE]),
            &keccak(&[DOMAIN_NAME.as_bytes()]),
            &keccak(&[DOMAIN_VERSION.as_bytes()]),
            &word_u64(self.chain_id),
        ]);
        let mut address = [0_u8; 32];
        address[12..].copy_from_slice(&self.address);
        let message = keccak(&[
            &keccak(&[MESSAGE_TYPE]),
            &keccak(&[PURPOSE.as_bytes()]),
            &keccak(&[self.warning().as_bytes()]),
            &address,
            &word_u64(u64::from(self.index)),
            &word_u64(u64::from(DERIVATION_VERSION)),
        ]);
        keccak(&[&[0x19, 0x01], &domain, &message])
    }

    fn hkdf_info(&self) -> Vec<u8> {
        let mut info = Vec::with_capacity(HKDF_INFO_PREFIX.len() + 56);
        info.extend_from_slice(HKDF_INFO_PREFIX);
        info.extend_from_slice(&word_u64(self.chain_id));
        info.extend_from_slice(&self.address);
        info.extend_from_slice(&self.index.to_be_bytes());
        info
    }
}

/// Brings a 65-byte `r || s || v` wallet signature to its one canonical
/// encoding: low `s` and `v` of 27 or 28. A high-`s` signature is reflected
/// and its recovery bit flipped; `v` of 0 or 1 is lifted by 27.
///
/// # Errors
///
/// Refuses a length other than 65 bytes, a zero or non-reduced scalar and any
/// other recovery byte.
pub fn normalize_wallet_signature(signature: &[u8]) -> Result<[u8; 65], DerivationError> {
    if signature.len() != 65 {
        return Err(DerivationError::InvalidSignature);
    }
    let mut parity = match signature[64] {
        0 | 27 => 0_u8,
        1 | 28 => 1,
        _ => return Err(DerivationError::InvalidSignature),
    };
    let mut parsed =
        Signature::from_slice(&signature[..64]).map_err(|_| DerivationError::InvalidSignature)?;
    if let Some(low) = parsed.normalize_s() {
        parsed = low;
        parity ^= 1;
    }
    let mut canonical = [0_u8; 65];
    canonical[..64].copy_from_slice(&parsed.to_bytes());
    canonical[64] = 27 + parity;
    Ok(canonical)
}

/// Derives the `LayerX` key an external wallet's signature over `request`
/// stands for. The signature is normalised, checked to recover to the
/// address the message names, and only then fed to HKDF-SHA256.
///
/// # Errors
///
/// Refuses a malformed signature and one that does not recover to
/// `request.address()`.
pub fn derive_from_wallet_signature(
    request: &KeyDerivationRequest,
    signature: &[u8],
) -> Result<DerivedAccount, DerivationError> {
    let canonical = Zeroizing::new(normalize_wallet_signature(signature)?);
    let parsed =
        Signature::from_slice(&canonical[..64]).map_err(|_| DerivationError::InvalidSignature)?;
    let recovery =
        RecoveryId::try_from(canonical[64] - 27).map_err(|_| DerivationError::InvalidSignature)?;
    let recovered = VerifyingKey::recover_from_prehash(&request.signing_hash(), &parsed, recovery)
        .map_err(|_| DerivationError::InvalidSignature)?;
    let signer = evm_address(recovered.to_encoded_point(false).as_bytes())
        .map_err(|_| DerivationError::InvalidSignature)?;
    if signer != request.address {
        return Err(DerivationError::SignerMismatch);
    }
    let mut seed = Zeroizing::new([0_u8; 32]);
    Hkdf::<Sha256>::new(Some(HKDF_SALT), &canonical[..])
        .expand(&request.hkdf_info(), &mut seed[..])
        .map_err(|_| DerivationError::InvalidSignature)?;
    Ok(DerivedAccount::new(
        request.index,
        request.address,
        None,
        seed,
    ))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use ed25519_dalek::SigningKey;
    use k256::ecdsa::SigningKey as EvmSigningKey;
    use serde_json::Value;

    use super::{
        derive_from_mnemonic, derive_from_wallet_signature, hexadecimal,
        normalize_wallet_signature, slip10_ed25519, DerivationError, KeyDerivationRequest,
        LAYERX_COIN_TYPE,
    };

    type Outcome = Result<(), Box<dyn Error>>;

    const FIXTURE: &str =
        include_str!("../../../../platform/sdk/conformance/fixtures/account-derivation-v1.json");

    fn bytes(text: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let text = text.strip_prefix("0x").unwrap_or(text);
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(Into::into))
            .collect()
    }

    fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
        value[key]
            .as_str()
            .ok_or_else(|| format!("fixture omits {key}").into())
    }

    fn list<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>, Box<dyn Error>> {
        value[key]
            .as_array()
            .ok_or_else(|| format!("fixture omits {key}").into())
    }

    fn number(value: &Value, key: &str) -> Result<u64, Box<dyn Error>> {
        value[key]
            .as_u64()
            .ok_or_else(|| format!("fixture omits {key}").into())
    }

    #[test]
    fn slip10_matches_the_official_ed25519_vectors() -> Outcome {
        let seed = bytes("000102030405060708090a0b0c0d0e0f")?;
        let cases: [(&[u32], &str, &str); 3] = [
            (
                &[],
                "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7",
                "a4b2856bfec510abab89753fac1ac0e1112364e7d250545963f135f2a33188ed",
            ),
            (
                &[0, 1],
                "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2",
                "1932a5270f335bed617d5b935c80aedb1a35bd9fc1e31acafd5372c30f5c1187",
            ),
            (
                &[0, 1, 2, 2, 1_000_000_000],
                "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793",
                "3c24da049451555d51a7014a37337aa4e12d41e485abccfa46b47dfb2af54b7a",
            ),
        ];
        for (path, private, public) in cases {
            let key = slip10_ed25519(&seed, path)?;
            assert_eq!(hexadecimal(&key[..]), private);
            assert_eq!(
                hexadecimal(&SigningKey::from_bytes(&key).verifying_key().to_bytes()),
                public
            );
        }
        Ok(())
    }

    #[test]
    fn mnemonic_vectors_match_the_shared_fixture() -> Outcome {
        let fixture: Value = serde_json::from_str(FIXTURE)?;
        assert_eq!(
            number(&fixture, "layerx_coin_type")?,
            u64::from(LAYERX_COIN_TYPE)
        );
        let mut seen = 0;
        for vector in list(&fixture, "mnemonic_vectors")? {
            for account in list(vector, "accounts")? {
                let index = u32::try_from(number(account, "index")?)?;
                let derived = derive_from_mnemonic(
                    text(vector, "mnemonic")?,
                    text(vector, "passphrase")?,
                    index,
                )?;
                assert_eq!(derived.evm_address_text(), text(account, "evm_address")?);
                assert_eq!(
                    hexadecimal(&derived.layerx_public_key()),
                    text(account, "layerx_public_key")?
                );
                assert_eq!(derived.did(), text(account, "did")?);
                assert!(derived.evm_secret().is_some());
                seen += 1;
            }
        }
        assert_eq!(seen, 4);
        let first = derive_from_mnemonic(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "",
            0,
        )?;
        assert_eq!(
            first.evm_address_text(),
            "0x9858EfFD232B4033E47d90003D41EC34EcaEda94"
        );
        Ok(())
    }

    #[test]
    fn bad_phrases_and_indices_are_refused() {
        assert_eq!(
            derive_from_mnemonic("abandon abandon abandon", "", 0).err(),
            Some(DerivationError::InvalidMnemonic)
        );
        assert_eq!(
            derive_from_mnemonic(
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon",
                "",
                0
            )
            .err(),
            Some(DerivationError::InvalidMnemonic)
        );
        assert_eq!(
            derive_from_mnemonic(
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
                "",
                0x8000_0000
            )
            .err(),
            Some(DerivationError::IndexOutOfRange)
        );
        assert_eq!(
            KeyDerivationRequest::new(1 << 53, [0; 20], 0).err(),
            Some(DerivationError::InvalidChainId)
        );
        assert_eq!(
            KeyDerivationRequest::with_origin(1, [0; 20], 0, "https://Evil.example/\"").err(),
            Some(DerivationError::InvalidOrigin)
        );
    }

    #[test]
    fn wallet_signature_vectors_match_the_shared_fixture() -> Outcome {
        let fixture: Value = serde_json::from_str(FIXTURE)?;
        let chain_id = number(&fixture, "chain_id")?;
        let wallet = &fixture["wallet_signature"];
        let address = <[u8; 20]>::try_from(bytes(text(wallet, "evm_address")?)?.as_slice())?;
        let signer = EvmSigningKey::from_slice(&bytes(text(wallet, "private_key")?)?)?;
        for vector in list(wallet, "vectors")? {
            let index = u32::try_from(number(vector, "index")?)?;
            let request = KeyDerivationRequest::new(chain_id, address, index)?;
            let document: Value = serde_json::from_str(&request.typed_data_json())?;
            assert_eq!(document, vector["typed_data"]);
            assert_eq!(
                hexadecimal(&request.signing_hash()),
                text(vector, "eip712_hash")?
            );
            let (signature, recovery) = signer.sign_prehash_recoverable(&request.signing_hash())?;
            let mut produced = signature.to_bytes().to_vec();
            produced.push(27 + recovery.to_byte());
            assert_eq!(hexadecimal(&produced), text(vector, "signature")?);

            let published = bytes(text(vector, "signature")?)?;
            let derived = derive_from_wallet_signature(&request, &published)?;
            assert_eq!(derived.did(), text(vector, "did")?);
            assert_eq!(derived.evm_address(), address);
            assert!(derived.evm_secret().is_none());
            for equivalent in list(vector, "equivalent_signatures")? {
                let equivalent = bytes(equivalent.as_str().ok_or("signature is not text")?)?;
                assert_ne!(equivalent, published);
                assert_eq!(normalize_wallet_signature(&equivalent)?.to_vec(), published);
                assert_eq!(
                    derive_from_wallet_signature(&request, &equivalent)?.did(),
                    derived.did()
                );
            }

            let mut other = address;
            other[19] ^= 1;
            let stranger = KeyDerivationRequest::new(chain_id, other, index)?;
            assert!(derive_from_wallet_signature(&stranger, &published).is_err());
            let elsewhere = KeyDerivationRequest::new(chain_id + 1, address, index)?;
            assert_eq!(
                derive_from_wallet_signature(&elsewhere, &published).err(),
                Some(DerivationError::SignerMismatch)
            );
            let mut bad_v = published.clone();
            bad_v[64] = 29;
            assert_eq!(
                derive_from_wallet_signature(&request, &bad_v).err(),
                Some(DerivationError::InvalidSignature)
            );
        }
        Ok(())
    }

    #[test]
    fn debug_output_never_shows_a_private_key() -> Outcome {
        let derived = derive_from_mnemonic(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "",
            0,
        )?;
        let shown = format!("{derived:?}");
        assert!(!shown.contains(&hexadecimal(derived.layerx_seed())));
        let secret = derived
            .evm_secret()
            .ok_or("mnemonic accounts hold the EVM key")?;
        assert!(!shown.contains(&hexadecimal(secret)));
        Ok(())
    }
}
