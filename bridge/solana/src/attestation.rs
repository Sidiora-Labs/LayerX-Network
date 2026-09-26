//! The outbound attestation on Solana.
//!
//! A release pays out what a Paxeer burn attested, and the attestation is the
//! one every other destination of this bridge verifies: keccak256 of the
//! 185-byte `PAXEERX_BRIDGE_OUT_V1` preimage, signed with secp256k1 by at least
//! threshold of the current attestors, in strictly ascending signer order,
//! every s at or below half the curve order. This program carries no
//! elliptic-curve code. The native secp256k1 program verifies each signature in
//! the instruction placed directly before the release, and this module reads
//! that instruction back through the instructions sysvar and requires that it
//! verified exactly the attestation the release needs.

use solana_program::account_info::AccountInfo;
use solana_program::entrypoint::ProgramResult;
use solana_program::program_error::ProgramError;
use solana_program::sysvar::instructions::{
    self as instructions_sysvar, load_current_index_checked, load_instruction_at_checked,
};
use solana_sdk_ids::secp256k1_program;

use crate::identity::HANDLE_BYTES;
use crate::BridgeError;

/// The domain the outbound preimage opens with.
pub const OUTBOUND_DOMAIN: &[u8; 21] = b"PAXEERX_BRIDGE_OUT_V1";
/// The exact length of the outbound preimage.
pub const OUTBOUND_PREIMAGE_BYTES: usize = 185;
/// The width of a signature inside a secp256k1 instruction: r then s.
pub const SIGNATURE_BYTES: usize = 64;
/// The width of one entry of the secp256k1 instruction's offsets table.
pub const OFFSETS_BYTES: usize = 11;
/// Half the secp256k1 group order, big-endian. A signature whose s is above it
/// is the malleable twin of one whose s is below, and is refused.
pub const HALF_ORDER: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0x5d, 0x57, 0x6e, 0x73, 0x57, 0xa4, 0x50, 0x1d, 0xdf, 0xe9, 0x2f, 0x46, 0x68, 0x1b, 0x20, 0xa0,
];

/// The fields of one outbound attestation, as a release on Solana names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outbound {
    /// The chain id Paxeer registers for Solana.
    pub chain_id: u64,
    /// The handle of the vault-authority PDA.
    pub vault: [u8; HANDLE_BYTES],
    /// The Paxeer transaction hash of the burn.
    pub paxeer_tx_hash: [u8; 32],
    /// The Paxeer bridge nonce of the burn.
    pub paxeer_nonce: u64,
    /// The handle of the 32-byte Solana pubkey the release pays.
    pub recipient: [u8; HANDLE_BYTES],
    /// The 20-byte asset id the registry holds for the mint.
    pub asset: [u8; HANDLE_BYTES],
    /// The amount in the mint's base units.
    pub amount: u64,
}

/// The 185-byte outbound preimage: the domain, the chain id as a uint256, the
/// vault, the paxeerTxHash, the paxeerNonce as a uint64, the recipient, the
/// asset and the amount as a uint256, all big-endian and packed with no
/// padding between fields. keccak256 of these bytes is the digest the
/// attestors sign, with no EIP-191 prefix and no EIP-712 domain.
pub fn outbound_preimage(outbound: &Outbound) -> [u8; OUTBOUND_PREIMAGE_BYTES] {
    let mut out = [0_u8; OUTBOUND_PREIMAGE_BYTES];
    out[..21].copy_from_slice(OUTBOUND_DOMAIN);
    out[45..53].copy_from_slice(&outbound.chain_id.to_be_bytes());
    out[53..73].copy_from_slice(&outbound.vault);
    out[73..105].copy_from_slice(&outbound.paxeer_tx_hash);
    out[105..113].copy_from_slice(&outbound.paxeer_nonce.to_be_bytes());
    out[113..133].copy_from_slice(&outbound.recipient);
    out[133..153].copy_from_slice(&outbound.asset);
    out[177..185].copy_from_slice(&outbound.amount.to_be_bytes());
    out
}

/// One signature entry of a secp256k1 instruction, resolved inside that
/// instruction's own data.
struct Entry<'a> {
    signature: &'a [u8],
    address: &'a [u8],
    message: &'a [u8],
}

/// Require the instruction directly before the current one to be the native
/// secp256k1 program's, carrying at least `threshold` signature entries, every
/// one of them over exactly `preimage`, with s at or below half the order, and
/// signed by `attestors` in strictly ascending order.
///
/// The native program has already recovered each entry's signer and matched it
/// against the address the entry names, or the transaction would not have
/// reached this program, so the addresses read here are the recovered signers.
pub fn verify_attestation(
    instructions: &AccountInfo<'_>,
    preimage: &[u8],
    attestors: &[[u8; HANDLE_BYTES]],
    threshold: u8,
) -> ProgramResult {
    if instructions.key != &instructions_sysvar::ID {
        return Err(BridgeError::Account.into());
    }
    let current = load_current_index_checked(instructions)?;
    let index = current
        .checked_sub(1)
        .ok_or(BridgeError::AttestationMissing)?;
    let secp = load_instruction_at_checked(usize::from(index), instructions)?;
    if secp.program_id != secp256k1_program::id() {
        return Err(BridgeError::AttestationMissing.into());
    }
    let own = u8::try_from(index).map_err(|_| BridgeError::AttestationMalformed)?;
    let entries = entries(&secp.data, own)?;

    if threshold == 0 || entries.len() < usize::from(threshold) {
        return Err(BridgeError::AttestationThreshold.into());
    }
    let mut previous: Option<&[u8]> = None;
    for entry in &entries {
        if entry.message != preimage {
            return Err(BridgeError::AttestationMessage.into());
        }
        if !low_s(&entry.signature[32..SIGNATURE_BYTES]) {
            return Err(BridgeError::AttestationMalleable.into());
        }
        if !attestors
            .iter()
            .any(|attestor| attestor.as_slice() == entry.address)
        {
            return Err(BridgeError::AttestationSigner.into());
        }
        if let Some(last) = previous {
            if entry.address <= last {
                return Err(BridgeError::AttestationSigner.into());
            }
        }
        previous = Some(entry.address);
    }
    Ok(())
}

/// Read the signature entries of a secp256k1 instruction's `data`, requiring
/// every entry to name `own` - the instruction's own index in the transaction -
/// for its signature, its address and its message, and every offset to resolve
/// inside `data`.
fn entries(data: &[u8], own: u8) -> Result<Vec<Entry<'_>>, ProgramError> {
    let count = usize::from(*data.first().ok_or(BridgeError::AttestationMalformed)?);
    let table = count
        .checked_mul(OFFSETS_BYTES)
        .and_then(|bytes| bytes.checked_add(1))
        .ok_or(BridgeError::AttestationMalformed)?;
    if data.len() < table {
        return Err(BridgeError::AttestationMalformed.into());
    }
    let mut out = Vec::with_capacity(count);
    for row in data[1..table].chunks_exact(OFFSETS_BYTES) {
        let signature_offset = usize::from(u16::from_le_bytes([row[0], row[1]]));
        let signature_index = row[2];
        let address_offset = usize::from(u16::from_le_bytes([row[3], row[4]]));
        let address_index = row[5];
        let message_offset = usize::from(u16::from_le_bytes([row[6], row[7]]));
        let message_size = usize::from(u16::from_le_bytes([row[8], row[9]]));
        let message_index = row[10];
        if signature_index != own || address_index != own || message_index != own {
            return Err(BridgeError::AttestationMalformed.into());
        }
        let signed = slice(data, signature_offset, SIGNATURE_BYTES + 1)?;
        let (signature, recovery) = signed.split_at(SIGNATURE_BYTES);
        if recovery != [0] && recovery != [1] {
            return Err(BridgeError::AttestationMalformed.into());
        }
        out.push(Entry {
            signature,
            address: slice(data, address_offset, HANDLE_BYTES)?,
            message: slice(data, message_offset, message_size)?,
        });
    }
    Ok(out)
}

fn slice(data: &[u8], offset: usize, length: usize) -> Result<&[u8], ProgramError> {
    let end = offset
        .checked_add(length)
        .ok_or(BridgeError::AttestationMalformed)?;
    data.get(offset..end)
        .ok_or(BridgeError::AttestationMalformed.into())
}

/// Whether the big-endian scalar `s` is at or below half the secp256k1 order.
pub fn low_s(s: &[u8]) -> bool {
    s <= HALF_ORDER.as_slice()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{handle, hex, SIDIORA_ASSET_ID, SOLANA_CHAIN_ID};
    use solana_program::keccak;

    fn bytes<const N: usize>(text: &str) -> [u8; N] {
        let mut out = [0_u8; N];
        assert_eq!(text.len(), N * 2, "{text} is not {N} bytes of hex");
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                .expect("the vector is hexadecimal");
        }
        out
    }

    /// The outbound vector bridge/ATTESTATION-SOLANA.md pins: a release of 4.2
    /// SID to the key its label derives, answering the burn its label derives.
    #[test]
    fn the_outbound_preimage_is_the_pinned_vector() {
        let recipient_key = keccak::hash(b"PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT").to_bytes();
        let burn = keccak::hash(b"PAXEERX_BRIDGE_SOLANA_VECTOR_BURN").to_bytes();
        assert_eq!(
            hex(&burn),
            "6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f"
        );
        let outbound = Outbound {
            chain_id: SOLANA_CHAIN_ID,
            vault: bytes("334121a65b47bd45c3f6381537d9180e98e445bc"),
            paxeer_tx_hash: burn,
            paxeer_nonce: 11,
            recipient: handle(&recipient_key),
            asset: SIDIORA_ASSET_ID,
            amount: 4_200_000,
        };
        assert_eq!(
            hex(&outbound.recipient),
            "fb02125a3275d53a9f6538626b49894d2aae80cc"
        );
        let preimage = outbound_preimage(&outbound);
        assert_eq!(
            hex(&preimage),
            concat!(
                "504158454552585f4252494447455f4f55545f5631",
                "0000000000000000000000000000000000000000000000000000534f4c414e41",
                "334121a65b47bd45c3f6381537d9180e98e445bc",
                "6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f",
                "000000000000000b",
                "fb02125a3275d53a9f6538626b49894d2aae80cc",
                "21f7b20a555199fa73a238b1a91fd0f549068fee",
                "0000000000000000000000000000000000000000000000000000000000401640",
            )
        );
        assert_eq!(
            hex(&keccak::hash(&preimage).to_bytes()),
            "c583652dd9b59e0fcef102cfc8866a52beadb77b82de25445d86900baf6d1c4e"
        );
    }

    #[test]
    fn the_amount_fills_the_low_bytes_of_a_uint256() {
        let outbound = Outbound {
            chain_id: u64::MAX,
            vault: [0x11; HANDLE_BYTES],
            paxeer_tx_hash: [0x22; 32],
            paxeer_nonce: u64::MAX,
            recipient: [0x33; HANDLE_BYTES],
            asset: [0x44; HANDLE_BYTES],
            amount: u64::MAX,
        };
        let preimage = outbound_preimage(&outbound);
        assert_eq!(&preimage[..21], OUTBOUND_DOMAIN);
        assert_eq!(preimage[21..45], [0_u8; 24]);
        assert_eq!(preimage[45..53], [0xff_u8; 8]);
        assert_eq!(preimage[153..177], [0_u8; 24]);
        assert_eq!(preimage[177..185], [0xff_u8; 8]);
    }

    #[test]
    fn half_the_order_is_the_highest_s_admitted() {
        assert!(low_s(&HALF_ORDER));
        let mut above = HALF_ORDER;
        above[31] += 1;
        assert!(!low_s(&above));
        assert!(low_s(&[0_u8; 32]));
        assert!(!low_s(&[0xff_u8; 32]));
    }
}
