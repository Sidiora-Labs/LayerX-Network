//! Keys the relayer uses, all held by the remote signer of
//! `interop/deploy/mirror/signer-protocol.md`: the attestor key and one
//! transaction-fee key per destination chain, the Solana one an ed25519 key. The relayer holds opaque
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
/// Policy domain of Solana release transaction messages signed by the fee
/// payer.
pub const SOLANA_TRANSACTION_DOMAIN: &[u8] = b"LayerX/bridge/solana-tx/v1";

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

/// The Solana fee payer: an ed25519 key the remote signer holds, the only
/// signer of every release transaction. It signs the exact message bytes under
/// [`SOLANA_TRANSACTION_DOMAIN`] and never an attestation.
pub struct FeePayer {
    signer: RemoteChainSigner,
    public_key: [u8; 32],
}

impl FeePayer {
    /// # Errors
    ///
    /// Refuses a signer that is not configured for ed25519: a secp256k1
    /// signer is the only kind with an Ethereum address.
    pub fn new(signer: RemoteChainSigner) -> Result<Self, SignerError> {
        if signer.ethereum_address().is_ok() {
            return Err(SignerError::Configuration);
        }
        let public_key = signer
            .public_key()
            .try_into()
            .map_err(|_| SignerError::Configuration)?;
        Ok(Self { signer, public_key })
    }

    /// The fee payer's Solana account key.
    #[must_use]
    pub const fn public_key(&self) -> [u8; 32] {
        self.public_key
    }

    /// Signs a Solana transaction message, returning the 64-byte signature the
    /// remote signer's answer was verified against.
    ///
    /// # Errors
    ///
    /// Returns the signer's refusal or a signature that fails verification.
    pub fn sign_message(&self, message: &[u8]) -> Result<[u8; 64], KeyError> {
        match self
            .signer
            .sign_message(SOLANA_TRANSACTION_DOMAIN, message)?
        {
            ChainSignature::Ed25519(signature) => Ok(signature),
            ChainSignature::Secp256k1(_) => Err(KeyError::Signer(SignerError::InvalidSignature)),
        }
    }
}

/// The algorithm every relayer key but the Solana fee payer uses.
pub const ALGORITHM: SigningAlgorithm = SigningAlgorithm::Secp256k1Recoverable;

/// The algorithm of the Solana fee payer.
pub const FEE_PAYER_ALGORITHM: SigningAlgorithm = SigningAlgorithm::Ed25519;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use layerx_mirror::signer::{RemoteSignerConfig, SignerEndpoint};

    use super::*;

    fn remote(algorithm: SigningAlgorithm, public_key: Vec<u8>) -> RemoteChainSigner {
        RemoteChainSigner::new(RemoteSignerConfig {
            endpoint: SignerEndpoint::Uds {
                socket: PathBuf::from("signer.sock"),
            },
            algorithm,
            key_handle: "bridge-solana-fees".to_owned(),
            public_key,
            timeout: Duration::from_secs(1),
        })
        .unwrap_or_else(|error| panic!("remote signer: {error:?}"))
    }

    #[test]
    fn the_fee_payer_is_an_ed25519_key_under_its_own_domain() {
        let ed25519 = ed25519_dalek::SigningKey::from_bytes(&[7; 32]).verifying_key();
        let payer = FeePayer::new(remote(FEE_PAYER_ALGORITHM, ed25519.to_bytes().to_vec()))
            .unwrap_or_else(|error| panic!("fee payer: {error:?}"));
        assert_eq!(payer.public_key(), ed25519.to_bytes());
        let secp256k1 = k256::ecdsa::SigningKey::from_slice(&[7; 32])
            .unwrap_or_else(|error| panic!("{error}"))
            .verifying_key()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec();
        assert!(matches!(
            FeePayer::new(remote(ALGORITHM, secp256k1.clone())),
            Err(SignerError::Configuration)
        ));
        assert!(Attestor::new(remote(ALGORITHM, secp256k1)).is_ok());
        assert!(Attestor::new(remote(FEE_PAYER_ALGORITHM, ed25519.to_bytes().to_vec())).is_err());
        assert_eq!(SOLANA_TRANSACTION_DOMAIN, b"LayerX/bridge/solana-tx/v1");
        for domain in [
            ATTEST_INBOUND_DOMAIN,
            ATTEST_OUTBOUND_DOMAIN,
            PAXEER_TRANSACTION_DOMAIN,
            ETHEREUM_TRANSACTION_DOMAIN,
        ] {
            assert_ne!(domain, SOLANA_TRANSACTION_DOMAIN);
        }
    }
}
