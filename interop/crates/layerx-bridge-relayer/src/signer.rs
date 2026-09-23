//! Keys the relayer uses, all held by the remote signer of
//! `interop/deploy/mirror/signer-protocol.md`: the attestor key and one
//! transaction-fee key per destination chain. The relayer holds opaque
//! handles and independently configured public keys only; chain private keys
//! never enter this process. Every request names a policy domain so the
//! signer can bind each handle to exactly the digests it may sign.

use layerx_mirror::signer::{ChainSignature, RemoteChainSigner, SignerError, SigningAlgorithm};

use crate::attestation::{
    to_attestor_signature, InboundAttestation, OutboundAttestation, SignatureError,
};

/// Policy domain of attestor signatures over inbound deposit digests.
pub const ATTEST_INBOUND_DOMAIN: &[u8] = b"LayerX/bridge/attest-inbound/v1";
/// Policy domain of attestor signatures over outbound burn digests.
pub const ATTEST_OUTBOUND_DOMAIN: &[u8] = b"LayerX/bridge/attest-outbound/v1";
/// Policy domain of EIP-1559 `bridgeIn` transactions on Paxeer.
pub const PAXEER_TRANSACTION_DOMAIN: &[u8] = b"LayerX/bridge/paxeer-eip1559/v1";
/// Policy domain of EIP-1559 `release` transactions on Ethereum chains.
pub const ETHEREUM_TRANSACTION_DOMAIN: &[u8] = b"LayerX/bridge/ethereum-eip1559/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyError {
    Signer(SignerError),
    Signature(SignatureError),
}

impl std::fmt::Display for KeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Signer(error) => write!(formatter, "remote signer: {error:?}"),
            Self::Signature(error) => write!(formatter, "signature: {error}"),
        }
    }
}

impl std::error::Error for KeyError {}

impl From<SignerError> for KeyError {
    fn from(value: SignerError) -> Self {
        Self::Signer(value)
    }
}

impl From<SignatureError> for KeyError {
    fn from(value: SignatureError) -> Self {
        Self::Signature(value)
    }
}

fn secp256k1(
    signer: &RemoteChainSigner,
    domain: &[u8],
    digest: [u8; 32],
) -> Result<[u8; 65], KeyError> {
    match signer.sign_digest(domain, digest)? {
        ChainSignature::Secp256k1(signature) => Ok(signature),
        ChainSignature::Ed25519(_) => Err(KeyError::Signer(SignerError::InvalidSignature)),
    }
}

fn address(signer: &RemoteChainSigner) -> Result<[u8; 20], SignerError> {
    signer.ethereum_address()
}

/// This relayer instance's attestor key.
pub struct Attestor {
    signer: RemoteChainSigner,
    address: [u8; 20],
}

impl Attestor {
    /// # Errors
    ///
    /// Refuses a signer that is not configured for recoverable secp256k1.
    pub fn new(signer: RemoteChainSigner) -> Result<Self, SignerError> {
        let address = address(&signer)?;
        Ok(Self { signer, address })
    }

    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    /// Signs the inbound digest, returning `r || s || v` with `v` 27 or 28.
    ///
    /// # Errors
    ///
    /// Returns the signer's refusal or a signature the verifiers would refuse.
    pub fn sign_inbound(&self, attestation: &InboundAttestation) -> Result<[u8; 65], KeyError> {
        let signature = secp256k1(&self.signer, ATTEST_INBOUND_DOMAIN, attestation.digest())?;
        Ok(to_attestor_signature(signature)?)
    }

    /// Signs the outbound digest, returning `r || s || v` with `v` 27 or 28.
    ///
    /// # Errors
    ///
    /// Returns the signer's refusal or a signature the verifiers would refuse.
    pub fn sign_outbound(&self, attestation: &OutboundAttestation) -> Result<[u8; 65], KeyError> {
        let signature = secp256k1(&self.signer, ATTEST_OUTBOUND_DOMAIN, attestation.digest())?;
        Ok(to_attestor_signature(signature)?)
    }
}

/// A transaction-fee account on one destination chain.
pub struct Submitter {
    signer: RemoteChainSigner,
    address: [u8; 20],
    domain: &'static [u8],
}

impl Submitter {
    /// # Errors
    ///
    /// Refuses a signer that is not configured for recoverable secp256k1.
    pub fn new(signer: RemoteChainSigner, domain: &'static [u8]) -> Result<Self, SignerError> {
        let address = address(&signer)?;
        Ok(Self {
            signer,
            address,
            domain,
        })
    }

    /// A submitter for `bridgeIn` transactions on Paxeer.
    ///
    /// # Errors
    ///
    /// Refuses a signer that is not configured for recoverable secp256k1.
    pub fn paxeer(signer: RemoteChainSigner) -> Result<Self, SignerError> {
        Self::new(signer, PAXEER_TRANSACTION_DOMAIN)
    }

    /// A submitter for `release` transactions on an Ethereum chain.
    ///
    /// # Errors
    ///
    /// Refuses a signer that is not configured for recoverable secp256k1.
    pub fn ethereum(signer: RemoteChainSigner) -> Result<Self, SignerError> {
        Self::new(signer, ETHEREUM_TRANSACTION_DOMAIN)
    }

    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    /// Signs a transaction signing hash, returning `r || s || y_parity`.
    ///
    /// # Errors
    ///
    /// Returns the signer's refusal or a signature that fails verification.
    pub fn sign_transaction_hash(&self, digest: [u8; 32]) -> Result<[u8; 65], KeyError> {
        secp256k1(&self.signer, self.domain, digest)
    }
}

/// The algorithm every relayer key uses.
pub const ALGORITHM: SigningAlgorithm = SigningAlgorithm::Secp256k1Recoverable;
