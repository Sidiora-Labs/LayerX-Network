//! Paxeer account binding consent.
//!
//! A `LayerX` DID key claims an EVM address on Paxeer by signing
//! `"LX:PAXEER-BIND:v1" || chain id (u256 big endian) || EVM address ||
//! nonce (u64 big endian)` and handing the public key and signature to the
//! `addr` precompile's `bindLayerX` call. The message layout here is the one
//! the chain enforces in `modules/evm/types/layerx_binding.go`; the signature
//! is the raw Ed25519 domain the strict `LayerX` verifier accepts.

use std::fmt::Write as _;

use ed25519_dalek::{Signer as _, SigningKey};
use layerx_crypto::ed25519::verify_message;
use layerx_crypto::VerifyError;

/// Domain separator every Paxeer binding signature commits to.
pub const BIND_DOMAIN: &[u8; 17] = b"LX:PAXEER-BIND:v1";

const DOMAIN_BYTES: usize = BIND_DOMAIN.len();
const CHAIN_ID_BYTES: usize = 32;
const ADDRESS_BYTES: usize = 20;
const NONCE_BYTES: usize = 8;

/// Exact length of the binding message the chain reconstructs.
pub const BIND_MESSAGE_BYTES: usize = DOMAIN_BYTES + CHAIN_ID_BYTES + ADDRESS_BYTES + NONCE_BYTES;

const CHAIN_ID_AT: usize = DOMAIN_BYTES;
const ADDRESS_AT: usize = CHAIN_ID_AT + CHAIN_ID_BYTES;
const NONCE_AT: usize = ADDRESS_AT + ADDRESS_BYTES;

fn hexadecimal(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// One EVM address a `LayerX` key intends to bind on one Paxeer chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    chain_id: [u8; CHAIN_ID_BYTES],
    evm_address: [u8; ADDRESS_BYTES],
    nonce: u64,
}

impl Binding {
    /// Binds `evm_address` on the chain numbered `chain_id` at `nonce`.
    ///
    /// `nonce` is the value the `addr` precompile reports through
    /// `layerxBindNonce(address)` for that EVM address.
    #[must_use]
    pub fn new(chain_id: u64, evm_address: [u8; ADDRESS_BYTES], nonce: u64) -> Self {
        let mut word = [0_u8; CHAIN_ID_BYTES];
        word[CHAIN_ID_BYTES - NONCE_BYTES..].copy_from_slice(&chain_id.to_be_bytes());
        Self::with_chain_word(word, evm_address, nonce)
    }

    /// Binds `evm_address` on a chain whose id is already a 256-bit big-endian
    /// word, for chains numbered beyond 64 bits.
    #[must_use]
    pub fn with_chain_word(
        chain_id: [u8; CHAIN_ID_BYTES],
        evm_address: [u8; ADDRESS_BYTES],
        nonce: u64,
    ) -> Self {
        Self {
            chain_id,
            evm_address,
            nonce,
        }
    }

    /// The 256-bit big-endian chain id this binding is valid on.
    #[must_use]
    pub fn chain_id(&self) -> [u8; CHAIN_ID_BYTES] {
        self.chain_id
    }

    /// The EVM address being claimed.
    #[must_use]
    pub fn evm_address(&self) -> [u8; ADDRESS_BYTES] {
        self.evm_address
    }

    /// The bind nonce this consent is spent against.
    #[must_use]
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Assembles the exact bytes the chain verifies the signature over.
    #[must_use]
    pub fn message(&self) -> [u8; BIND_MESSAGE_BYTES] {
        let mut message = [0_u8; BIND_MESSAGE_BYTES];
        message[..DOMAIN_BYTES].copy_from_slice(BIND_DOMAIN);
        message[CHAIN_ID_AT..ADDRESS_AT].copy_from_slice(&self.chain_id);
        message[ADDRESS_AT..NONCE_AT].copy_from_slice(&self.evm_address);
        message[NONCE_AT..].copy_from_slice(&self.nonce.to_be_bytes());
        message
    }

    /// Signs the binding with a private Ed25519 seed, yielding the public key
    /// and signature `bindLayerX` takes.
    #[must_use]
    pub fn sign(&self, seed: &[u8; 32]) -> SignedBinding {
        let signing_key = SigningKey::from_bytes(seed);
        let signature = signing_key.sign(&self.message()).to_bytes();
        SignedBinding {
            binding: *self,
            public_key: signing_key.verifying_key().to_bytes(),
            signature,
        }
    }

    /// Checks a signature a key custodian produced elsewhere against this
    /// binding, using the strict `LayerX` Ed25519 rules the chain applies.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError::BadSignature` when the key or signature is
    /// noncanonical, weak, or does not cover this binding.
    pub fn verify(&self, public_key: &[u8; 32], signature: &[u8; 64]) -> Result<(), VerifyError> {
        verify_message(public_key, signature, &self.message())
    }
}

/// A binding together with the `LayerX` key that consented to it.
#[derive(Clone, Copy, Debug)]
pub struct SignedBinding {
    binding: Binding,
    public_key: [u8; 32],
    signature: [u8; 64],
}

impl SignedBinding {
    /// The binding that was signed.
    #[must_use]
    pub fn binding(&self) -> Binding {
        self.binding
    }

    /// The `LayerX` DID public key, the first `bindLayerX` argument.
    #[must_use]
    pub fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// The 64-byte Ed25519 signature, the second `bindLayerX` argument.
    #[must_use]
    pub fn signature(&self) -> [u8; 64] {
        self.signature
    }

    /// The `did:layerx:<hex public key>` identifier the binding publishes.
    #[must_use]
    pub fn did(&self) -> String {
        format!("did:layerx:{}", hexadecimal(&self.public_key))
    }

    /// The canonical `LayerX` main account name owned by this DID.
    #[must_use]
    pub fn main_account_name(&self) -> String {
        format!("agent:{}:main", self.did())
    }

    /// Re-checks the signature with the strict `LayerX` verifier.
    ///
    /// # Errors
    ///
    /// Returns `VerifyError::BadSignature` when the signature does not cover
    /// the binding under this public key.
    pub fn verify(&self) -> Result<(), VerifyError> {
        self.binding.verify(&self.public_key, &self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::{Binding, BIND_MESSAGE_BYTES};

    const SEED: [u8; 32] = [0x61; 32];
    const ADDRESS: [u8; 20] = [
        0x10, 0x21, 0x32, 0x43, 0x54, 0x65, 0x76, 0x87, 0x98, 0xa9, 0xba, 0xcb, 0xdc, 0xed, 0xfe,
        0x0f, 0x1e, 0x2d, 0x3c, 0x4b,
    ];
    const MESSAGE_713714_NONCE_0: &str = "4c583a5041584545522d42494e443a763100000000000000000000000000000000000000000000000000000000000ae3f2102132435465768798a9bacbdcedfe0f1e2d3c4b0000000000000000";
    const SIGNATURE_713714_NONCE_0: &str = "c815ff4b2c34fe06ed50e904133b8f4cc618255ce6493d1a1177480eb2e959ed16f055fbe0e12885c199bf662b430e4f9f131addb2aa705fde963fa5995ece0f";
    const MESSAGE_125_NONCE_2: &str = "4c583a5041584545522d42494e443a7631000000000000000000000000000000000000000000000000000000000000007d102132435465768798a9bacbdcedfe0f1e2d3c4b0000000000000002";
    const DID: &str = "did:layerx:af06a3e3291714e4f356c19c9b15cd1951ec6e6662aa77be07547f289383341d";

    fn hex(bytes: &[u8]) -> String {
        super::hexadecimal(bytes)
    }

    #[test]
    fn messages_match_the_chain_enforced_layout() {
        let binding = Binding::new(713_714, ADDRESS, 0);
        assert_eq!(binding.message().len(), BIND_MESSAGE_BYTES);
        assert_eq!(hex(&binding.message()), MESSAGE_713714_NONCE_0);
        assert_eq!(
            hex(&Binding::new(125, ADDRESS, 2).message()),
            MESSAGE_125_NONCE_2
        );
    }

    #[test]
    fn wide_chain_ids_keep_their_full_word() {
        let mut word = [0_u8; 32];
        word[0] = 0x0f;
        word[31] = 0x7d;
        let wide = Binding::with_chain_word(word, ADDRESS, 2);
        assert_eq!(wide.chain_id(), word);
        assert_ne!(wide.message(), Binding::new(125, ADDRESS, 2).message());
    }

    #[test]
    fn signatures_reproduce_the_published_binding_vectors() {
        let signed = Binding::new(713_714, ADDRESS, 0).sign(&SEED);
        assert_eq!(hex(&signed.signature()), SIGNATURE_713714_NONCE_0);
        assert_eq!(signed.did(), DID);
        assert_eq!(signed.main_account_name(), format!("agent:{DID}:main"));
        assert!(signed.verify().is_ok());
    }

    #[test]
    fn consent_does_not_travel_across_chains_or_nonces() {
        let signed = Binding::new(125, ADDRESS, 2).sign(&SEED);
        assert!(Binding::new(713_714, ADDRESS, 2)
            .verify(&signed.public_key(), &signed.signature())
            .is_err());
        assert!(Binding::new(125, ADDRESS, 3)
            .verify(&signed.public_key(), &signed.signature())
            .is_err());
        let mut other = ADDRESS;
        other[19] ^= 0x01;
        assert!(Binding::new(125, other, 2)
            .verify(&signed.public_key(), &signed.signature())
            .is_err());
    }
}
