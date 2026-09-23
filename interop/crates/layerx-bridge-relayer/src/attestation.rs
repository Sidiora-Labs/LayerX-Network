//! The two PaxeerX bridge attestation digests and the signature rules both
//! verifiers enforce, byte for byte as
//! `interop/contracts/ethereum-bridge/ATTESTATION.md` and
//! `modules/layerxbridge/ATTESTATION.md` specify them.

use std::collections::BTreeMap;
use std::fmt;

use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha3::{Digest as _, Keccak256};

/// Ethereum -> Paxeer domain, ASCII with no length prefix or terminator.
pub const DOMAIN_IN: &[u8; 20] = b"PAXEERX_BRIDGE_IN_V1";
/// Paxeer -> Ethereum domain, ASCII with no length prefix or terminator.
pub const DOMAIN_OUT: &[u8; 21] = b"PAXEERX_BRIDGE_OUT_V1";
pub const IN_PREIMAGE_LENGTH: usize = 20 + 32 + 20 + 32 + 8 + 32 + 20 + 32;
pub const OUT_PREIMAGE_LENGTH: usize = 21 + 32 + 20 + 32 + 8 + 20 + 20 + 32;

/// `n / 2` of secp256k1, the largest `s` either verifier accepts.
const SECP256K1_HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

/// A `uint256` as its 32 big-endian bytes.
#[must_use]
pub fn uint256_from_u64(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

/// A `BridgeDeposit` log on a `PaxeerXVault`, attested for `bridgeIn` on
/// Paxeer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InboundAttestation {
    pub chain_id: u64,
    pub vault: [u8; 20],
    pub tx_hash: [u8; 32],
    pub log_index: u64,
    pub recipient: [u8; 32],
    pub asset: [u8; 20],
    pub amount: [u8; 32],
}

impl InboundAttestation {
    /// `abi.encodePacked(DOMAIN_IN, uint256 chainId, vault, txHash,
    /// uint64 logIndex, recipient, asset, amount)`.
    #[must_use]
    pub fn preimage(&self) -> [u8; IN_PREIMAGE_LENGTH] {
        let mut output = [0_u8; IN_PREIMAGE_LENGTH];
        output[0..20].copy_from_slice(DOMAIN_IN);
        output[20..52].copy_from_slice(&uint256_from_u64(self.chain_id));
        output[52..72].copy_from_slice(&self.vault);
        output[72..104].copy_from_slice(&self.tx_hash);
        output[104..112].copy_from_slice(&self.log_index.to_be_bytes());
        output[112..144].copy_from_slice(&self.recipient);
        output[144..164].copy_from_slice(&self.asset);
        output[164..196].copy_from_slice(&self.amount);
        output
    }

    /// The raw keccak256 digest attestors sign.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        Keccak256::digest(self.preimage()).into()
    }

    /// The Paxeer EVM address in the low 20 bytes of `recipient`, when the
    /// high 12 bytes are zero and the address is not zero, the only form
    /// `bridgeIn` accepts.
    #[must_use]
    pub fn paxeer_recipient(&self) -> Option<[u8; 20]> {
        if self.recipient[..12] != [0; 12] || self.recipient[12..] == [0; 20] {
            return None;
        }
        let mut address = [0_u8; 20];
        address.copy_from_slice(&self.recipient[12..]);
        Some(address)
    }
}

/// A Paxeer `BridgeOut` burn, attested for `PaxeerXVault.release`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutboundAttestation {
    pub chain_id: u64,
    pub vault: [u8; 20],
    pub paxeer_tx_hash: [u8; 32],
    pub paxeer_nonce: u64,
    pub recipient: [u8; 20],
    pub asset: [u8; 20],
    pub amount: [u8; 32],
}

impl OutboundAttestation {
    /// `abi.encodePacked(DOMAIN_OUT, uint256 chainId, vault, paxeerTxHash,
    /// uint64 paxeerNonce, recipient, asset, amount)`.
    #[must_use]
    pub fn preimage(&self) -> [u8; OUT_PREIMAGE_LENGTH] {
        let mut output = [0_u8; OUT_PREIMAGE_LENGTH];
        output[0..21].copy_from_slice(DOMAIN_OUT);
        output[21..53].copy_from_slice(&uint256_from_u64(self.chain_id));
        output[53..73].copy_from_slice(&self.vault);
        output[73..105].copy_from_slice(&self.paxeer_tx_hash);
        output[105..113].copy_from_slice(&self.paxeer_nonce.to_be_bytes());
        output[113..133].copy_from_slice(&self.recipient);
        output[133..153].copy_from_slice(&self.asset);
        output[153..185].copy_from_slice(&self.amount);
        output
    }

    /// The raw keccak256 digest attestors sign.
    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        Keccak256::digest(self.preimage()).into()
    }

    /// The vault nullifier `keccak256(paxeerTxHash || uint64 paxeerNonce)`.
    #[must_use]
    pub fn nullifier(&self) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(self.paxeer_tx_hash);
        hasher.update(self.paxeer_nonce.to_be_bytes());
        hasher.finalize().into()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureError {
    /// `v` is not 27 or 28 (or the recovery id is not 0 or 1).
    RecoveryByte,
    /// `s` is above `secp256k1n / 2`.
    HighS,
    /// The signature does not recover to a public key.
    Unrecoverable,
    /// Fewer distinct current attestors signed than the threshold.
    BelowThreshold { signatures: usize, threshold: usize },
    /// A threshold of zero is never a valid attestor policy.
    ZeroThreshold,
}

impl fmt::Display for SignatureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RecoveryByte => formatter.write_str("signature recovery byte is not 27 or 28"),
            Self::HighS => formatter.write_str("signature s is above secp256k1n / 2"),
            Self::Unrecoverable => formatter.write_str("signature does not recover a signer"),
            Self::BelowThreshold {
                signatures,
                threshold,
            } => write!(
                formatter,
                "{signatures} attestor signatures, threshold {threshold}"
            ),
            Self::ZeroThreshold => formatter.write_str("attestor threshold is zero"),
        }
    }
}

impl std::error::Error for SignatureError {}

/// The Ethereum address of a secp256k1 public key.
#[must_use]
pub fn ethereum_address(key: &VerifyingKey) -> [u8; 20] {
    let point = key.to_encoded_point(false);
    let digest = Keccak256::digest(&point.as_bytes()[1..]);
    let mut address = [0_u8; 20];
    address.copy_from_slice(&digest[12..]);
    address
}

/// Converts a remote signer's `r || s || recovery_id` (0 or 1) into the
/// verifiers' `r || s || v` with `v` 27 or 28.
///
/// # Errors
///
/// Refuses a recovery id other than 0 or 1 and a high `s`.
pub fn to_attestor_signature(recoverable: [u8; 65]) -> Result<[u8; 65], SignatureError> {
    if recoverable[64] > 1 {
        return Err(SignatureError::RecoveryByte);
    }
    let mut signature = recoverable;
    signature[64] += 27;
    if signature[32..64] > SECP256K1_HALF_ORDER[..] {
        return Err(SignatureError::HighS);
    }
    Ok(signature)
}

/// Recovers the signer of `digest` under exactly the rules `PaxeerXVault`
/// and `bridgeIn` apply: 65 bytes, `v` 27 or 28, low `s`.
///
/// # Errors
///
/// Refuses any signature either verifier would refuse.
pub fn recover_signer(digest: &[u8; 32], signature: &[u8; 65]) -> Result<[u8; 20], SignatureError> {
    let recovery = match signature[64] {
        27 => 0,
        28 => 1,
        _ => return Err(SignatureError::RecoveryByte),
    };
    if signature[32..64] > SECP256K1_HALF_ORDER[..] {
        return Err(SignatureError::HighS);
    }
    let parsed =
        Signature::from_slice(&signature[..64]).map_err(|_| SignatureError::Unrecoverable)?;
    let recovery = RecoveryId::from_byte(recovery).ok_or(SignatureError::RecoveryByte)?;
    let key = VerifyingKey::recover_from_prehash(digest, &parsed, recovery)
        .map_err(|_| SignatureError::Unrecoverable)?;
    Ok(ethereum_address(&key))
}

/// Selects the signatures to submit: every candidate that recovers to a
/// current attestor, one per signer, ordered by strictly ascending signer
/// address, truncated to the `threshold` lowest signers so that every relayer
/// holding the same candidates submits identical calldata. Candidates are
/// untrusted: invalid ones and non-attestors are dropped, never fatal.
///
/// # Errors
///
/// Returns `BelowThreshold` while fewer than `threshold` distinct attestors
/// signed, and `ZeroThreshold` for a zero threshold.
pub fn assemble_signatures<I>(
    digest: &[u8; 32],
    candidates: I,
    attestors: &[[u8; 20]],
    threshold: usize,
) -> Result<Vec<[u8; 65]>, SignatureError>
where
    I: IntoIterator<Item = [u8; 65]>,
{
    if threshold == 0 {
        return Err(SignatureError::ZeroThreshold);
    }
    let mut by_signer = BTreeMap::new();
    for candidate in candidates {
        let Ok(signer) = recover_signer(digest, &candidate) else {
            continue;
        };
        if attestors.contains(&signer) {
            by_signer.entry(signer).or_insert(candidate);
        }
    }
    if by_signer.len() < threshold {
        return Err(SignatureError::BelowThreshold {
            signatures: by_signer.len(),
            threshold,
        });
    }
    Ok(by_signer.into_values().take(threshold).collect())
}

#[cfg(test)]
mod tests {
    use k256::ecdsa::SigningKey;

    use super::*;
    use crate::hex;

    fn repeated<const N: usize>(byte: u8) -> [u8; N] {
        [byte; N]
    }

    fn vector_amount() -> [u8; 32] {
        let mut amount = [0_u8; 32];
        amount[16..].copy_from_slice(&1_000_000_000_000_000_000_u128.to_be_bytes());
        amount
    }

    fn vector_outbound() -> OutboundAttestation {
        OutboundAttestation {
            chain_id: 1,
            vault: repeated(0x11),
            paxeer_tx_hash: repeated(0x22),
            paxeer_nonce: 7,
            recipient: repeated(0x33),
            asset: repeated(0x44),
            amount: vector_amount(),
        }
    }

    fn vector_inbound() -> InboundAttestation {
        InboundAttestation {
            chain_id: 1,
            vault: repeated(0x11),
            tx_hash: repeated(0x22),
            log_index: 7,
            recipient: repeated(0x55),
            asset: repeated(0x44),
            amount: vector_amount(),
        }
    }

    fn key(byte: u8) -> SigningKey {
        let mut secret = [0_u8; 32];
        secret[31] = byte;
        SigningKey::from_slice(&secret).unwrap_or_else(|error| panic!("test key: {error}"))
    }

    fn sign(key: &SigningKey, digest: &[u8; 32]) -> [u8; 65] {
        let (signature, recovery) = key
            .sign_prehash_recoverable(digest)
            .unwrap_or_else(|error| panic!("signing: {error}"));
        let mut output = [0_u8; 65];
        output[..64].copy_from_slice(&signature.to_bytes());
        output[64] = recovery.to_byte();
        to_attestor_signature(output).unwrap_or_else(|error| panic!("low s: {error}"))
    }

    // The two vectors of interop/contracts/ethereum-bridge/ATTESTATION.md and
    // their Paxeer-side copy in modules/layerxbridge/ATTESTATION.md, which
    // state the same inputs and the same digests.
    #[test]
    fn outbound_digest_matches_both_attestation_documents() {
        assert_eq!(
            hex::prefixed(&vector_outbound().digest()),
            "0xbd35888e4b158986238ce7abe73957702e2f6e78fe6157197878ebd13edf5b37"
        );
    }

    #[test]
    fn inbound_digest_matches_both_attestation_documents() {
        assert_eq!(
            hex::prefixed(&vector_inbound().digest()),
            "0x511964ae9566f9536604258667400b0d76335e2a6e60ab0f700bf2433bc97918"
        );
    }

    #[test]
    fn outbound_preimage_follows_the_documented_offsets() {
        let preimage = vector_outbound().preimage();
        assert_eq!(preimage.len(), 185);
        assert_eq!(&preimage[0..21], b"PAXEERX_BRIDGE_OUT_V1");
        assert_eq!(preimage[21..52], [0; 31]);
        assert_eq!(preimage[52], 1);
        assert_eq!(preimage[53..73], [0x11; 20]);
        assert_eq!(preimage[73..105], [0x22; 32]);
        assert_eq!(preimage[105..113], 7_u64.to_be_bytes());
        assert_eq!(preimage[113..133], [0x33; 20]);
        assert_eq!(preimage[133..153], [0x44; 20]);
        assert_eq!(preimage[153..185], vector_amount());
    }

    #[test]
    fn inbound_preimage_follows_the_documented_offsets() {
        let preimage = vector_inbound().preimage();
        assert_eq!(preimage.len(), 196);
        assert_eq!(&preimage[0..20], b"PAXEERX_BRIDGE_IN_V1");
        assert_eq!(preimage[20..51], [0; 31]);
        assert_eq!(preimage[51], 1);
        assert_eq!(preimage[52..72], [0x11; 20]);
        assert_eq!(preimage[72..104], [0x22; 32]);
        assert_eq!(preimage[104..112], 7_u64.to_be_bytes());
        assert_eq!(preimage[112..144], [0x55; 32]);
        assert_eq!(preimage[144..164], [0x44; 20]);
        assert_eq!(preimage[164..196], vector_amount());
    }

    #[test]
    fn the_vector_recipient_is_not_a_paxeer_address_but_a_padded_one_is() {
        assert_eq!(vector_inbound().paxeer_recipient(), None);
        let mut padded = vector_inbound();
        padded.recipient = [0; 32];
        padded.recipient[12..].copy_from_slice(&[0x55; 20]);
        assert_eq!(padded.paxeer_recipient(), Some([0x55; 20]));
        padded.recipient = [0; 32];
        assert_eq!(padded.paxeer_recipient(), None);
    }

    #[test]
    fn the_outbound_nullifier_is_keccak_of_hash_and_big_endian_nonce() {
        assert_eq!(
            hex::prefixed(&vector_outbound().nullifier()),
            "0x82ae345d928fefd71690c94b9d952bce885d03a41c7d68664822adcedfc368cd"
        );
    }

    #[test]
    fn signatures_recover_under_the_verifier_rules() {
        let digest = vector_inbound().digest();
        let signer = key(0xa1);
        let signature = sign(&signer, &digest);
        assert!(signature[64] == 27 || signature[64] == 28);
        assert_eq!(
            recover_signer(&digest, &signature),
            Ok(ethereum_address(signer.verifying_key()))
        );
        assert_eq!(
            hex::prefixed(&ethereum_address(signer.verifying_key())),
            "0xd2431ca38735c2fd438e2caa23f094191d89675b"
        );
        let mut raw_recovery = signature;
        raw_recovery[64] -= 27;
        assert_eq!(
            recover_signer(&digest, &raw_recovery),
            Err(SignatureError::RecoveryByte)
        );
        let mut high = signature;
        high[32..64].copy_from_slice(&[0xff; 32]);
        assert_eq!(recover_signer(&digest, &high), Err(SignatureError::HighS));
        assert_eq!(
            to_attestor_signature(high),
            Err(SignatureError::RecoveryByte)
        );
    }

    #[test]
    fn assembly_orders_by_ascending_signer_and_drops_outsiders_and_duplicates() {
        let digest = vector_outbound().digest();
        let first = key(0xa1);
        let second = key(0xa2);
        let outsider = key(0xbd);
        let first_address = ethereum_address(first.verifying_key());
        let second_address = ethereum_address(second.verifying_key());
        assert!(second_address < first_address);
        let attestors = [first_address, second_address];
        let first_signature = sign(&first, &digest);
        let second_signature = sign(&second, &digest);
        let mut garbage = first_signature;
        garbage[64] = 0;
        let candidates = [
            first_signature,
            sign(&outsider, &digest),
            garbage,
            first_signature,
            second_signature,
        ];
        assert_eq!(
            assemble_signatures(&digest, candidates, &attestors, 2),
            Ok(vec![second_signature, first_signature])
        );
        assert_eq!(
            assemble_signatures(&digest, candidates, &attestors, 1),
            Ok(vec![second_signature])
        );
        assert_eq!(
            assemble_signatures(&digest, [first_signature], &attestors, 2),
            Err(SignatureError::BelowThreshold {
                signatures: 1,
                threshold: 2
            })
        );
        assert_eq!(
            assemble_signatures(&digest, [first_signature], &attestors, 0),
            Err(SignatureError::ZeroThreshold)
        );
    }
}
