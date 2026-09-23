//! ABI of the two bridge endpoints the relayer drives: the layerxBridge
//! precompile at `0x…1016` on Paxeer (`precompiles/layerxbridge/abi.json`) and
//! `PaxeerXVault` on each Ethereum chain
//! (`interop/contracts/ethereum-bridge/src/PaxeerXVault.sol`).

use std::fmt;

use serde_json::Value;

use crate::attestation::{uint256_from_u64, InboundAttestation, OutboundAttestation};
use crate::hex;

/// The layerxBridge precompile on Paxeer.
pub const LAYERX_BRIDGE_PRECOMPILE: [u8; 20] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x16,
];

/// `keccak256("BridgeDeposit(address,uint256,address,bytes32,uint64)")`.
pub const BRIDGE_DEPOSIT_TOPIC: [u8; 32] = [
    0x19, 0xa8, 0x71, 0x3a, 0x45, 0x94, 0xd9, 0x88, 0x24, 0x32, 0x35, 0x71, 0xa6, 0x87, 0x83, 0x8a,
    0xa7, 0x6b, 0x4d, 0x73, 0x0c, 0xfe, 0xdd, 0xe1, 0x78, 0x5e, 0xe8, 0xbb, 0x92, 0x19, 0x51, 0x90,
];
/// `keccak256("BridgeOut(uint64,address,uint256,address,uint64)")`.
pub const BRIDGE_OUT_TOPIC: [u8; 32] = [
    0x3e, 0x99, 0x0e, 0xb5, 0x40, 0x09, 0xdc, 0xdc, 0xa5, 0x3d, 0x8f, 0xa8, 0x73, 0x07, 0x21, 0x0f,
    0x07, 0x09, 0x7f, 0x37, 0xdc, 0xf6, 0x18, 0x5d, 0xee, 0x71, 0xa4, 0x2f, 0x8e, 0x7d, 0x52, 0x4e,
];

/// `bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])`.
pub const BRIDGE_IN_SELECTOR: [u8; 4] = [0x09, 0x06, 0x3e, 0x9c];
/// `isNullified(uint64,bytes32,uint64)`.
pub const IS_NULLIFIED_SELECTOR: [u8; 4] = [0xca, 0xa5, 0x8b, 0xc9];
/// `getAttestors()`.
pub const GET_ATTESTORS_SELECTOR: [u8; 4] = [0x1a, 0x15, 0x92, 0x2d];
/// `getChain(uint64)`.
pub const GET_CHAIN_SELECTOR: [u8; 4] = [0xa1, 0xa6, 0xd5, 0x08];
/// `release(address,uint256,address,bytes32,uint64,bytes[])`.
pub const RELEASE_SELECTOR: [u8; 4] = [0x05, 0xeb, 0x76, 0x6d];
/// `threshold()`.
pub const THRESHOLD_SELECTOR: [u8; 4] = [0x42, 0xcd, 0xe4, 0xe8];
/// `attestors()`.
pub const ATTESTORS_SELECTOR: [u8; 4] = [0xe7, 0xeb, 0x46, 0x6f];
/// `nullified(bytes32)`.
pub const NULLIFIED_SELECTOR: [u8; 4] = [0xe7, 0x3b, 0xdb, 0x5e];

const MAX_ATTESTORS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiError {
    /// Return data is too short, misaligned or points outside itself.
    Malformed,
    /// A word that must hold a narrower type has non-zero high bytes.
    OutOfRange,
    /// A log is not the expected event from the expected contract.
    UnexpectedLog,
    /// The node reports the log as removed by a reorganisation.
    RemovedLog,
}

impl fmt::Display for AbiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Malformed => "malformed ABI data",
            Self::OutOfRange => "ABI word out of range for its type",
            Self::UnexpectedLog => "log is not the expected bridge event",
            Self::RemovedLog => "log was removed by a reorganisation",
        })
    }
}

impl std::error::Error for AbiError {}

/// A left-padded address word.
#[must_use]
pub fn address_word(address: &[u8; 20]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(address);
    word
}

fn word_usize(value: usize) -> [u8; 32] {
    uint256_from_u64(u64::try_from(value).unwrap_or(u64::MAX))
}

fn encode_bytes_array(signatures: &[[u8; 65]], output: &mut Vec<u8>) {
    output.extend_from_slice(&word_usize(signatures.len()));
    // Each element is a 32-byte length followed by 65 bytes padded to 96.
    let element = 32 + 96;
    for index in 0..signatures.len() {
        output.extend_from_slice(&word_usize(signatures.len() * 32 + index * element));
    }
    for signature in signatures {
        output.extend_from_slice(&word_usize(signature.len()));
        output.extend_from_slice(signature);
        output.extend_from_slice(&[0_u8; 96 - 65]);
    }
}

/// Calldata of `bridgeIn` on the precompile for one inbound attestation.
#[must_use]
pub fn encode_bridge_in(attestation: &InboundAttestation, signatures: &[[u8; 65]]) -> Vec<u8> {
    let mut output = Vec::with_capacity(4 + 32 * 10 + signatures.len() * 160);
    output.extend_from_slice(&BRIDGE_IN_SELECTOR);
    output.extend_from_slice(&uint256_from_u64(attestation.chain_id));
    output.extend_from_slice(&address_word(&attestation.vault));
    output.extend_from_slice(&attestation.tx_hash);
    output.extend_from_slice(&uint256_from_u64(attestation.log_index));
    output.extend_from_slice(&attestation.recipient);
    output.extend_from_slice(&address_word(&attestation.asset));
    output.extend_from_slice(&attestation.amount);
    output.extend_from_slice(&word_usize(8 * 32));
    encode_bytes_array(signatures, &mut output);
    output
}

/// Calldata of `PaxeerXVault.release` for one outbound attestation.
#[must_use]
pub fn encode_release(attestation: &OutboundAttestation, signatures: &[[u8; 65]]) -> Vec<u8> {
    let mut output = Vec::with_capacity(4 + 32 * 8 + signatures.len() * 160);
    output.extend_from_slice(&RELEASE_SELECTOR);
    output.extend_from_slice(&address_word(&attestation.asset));
    output.extend_from_slice(&attestation.amount);
    output.extend_from_slice(&address_word(&attestation.recipient));
    output.extend_from_slice(&attestation.paxeer_tx_hash);
    output.extend_from_slice(&uint256_from_u64(attestation.paxeer_nonce));
    output.extend_from_slice(&word_usize(6 * 32));
    encode_bytes_array(signatures, &mut output);
    output
}

/// Calldata of the precompile's `isNullified(chain, txHash, logIndex)`.
#[must_use]
pub fn encode_is_nullified(chain_id: u64, tx_hash: &[u8; 32], log_index: u64) -> Vec<u8> {
    let mut output = Vec::with_capacity(4 + 96);
    output.extend_from_slice(&IS_NULLIFIED_SELECTOR);
    output.extend_from_slice(&uint256_from_u64(chain_id));
    output.extend_from_slice(tx_hash);
    output.extend_from_slice(&uint256_from_u64(log_index));
    output
}

/// Calldata of the precompile's `getChain(chain)`.
#[must_use]
pub fn encode_get_chain(chain_id: u64) -> Vec<u8> {
    let mut output = GET_CHAIN_SELECTOR.to_vec();
    output.extend_from_slice(&uint256_from_u64(chain_id));
    output
}

/// Calldata of the vault's public `nullified(bytes32)` getter.
#[must_use]
pub fn encode_nullified(nullifier: &[u8; 32]) -> Vec<u8> {
    let mut output = NULLIFIED_SELECTOR.to_vec();
    output.extend_from_slice(nullifier);
    output
}

fn word(data: &[u8], index: usize) -> Result<&[u8], AbiError> {
    let start = index.checked_mul(32).ok_or(AbiError::Malformed)?;
    data.get(start..start + 32).ok_or(AbiError::Malformed)
}

fn word_at(data: &[u8], offset: usize) -> Result<&[u8], AbiError> {
    data.get(offset..offset.checked_add(32).ok_or(AbiError::Malformed)?)
        .ok_or(AbiError::Malformed)
}

fn as_u64(word: &[u8]) -> Result<u64, AbiError> {
    if word.len() != 32 || word[..24] != [0; 24] {
        return Err(AbiError::OutOfRange);
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&word[24..]);
    Ok(u64::from_be_bytes(bytes))
}

fn as_usize(word: &[u8]) -> Result<usize, AbiError> {
    usize::try_from(as_u64(word)?).map_err(|_| AbiError::OutOfRange)
}

fn as_address(word: &[u8]) -> Result<[u8; 20], AbiError> {
    if word.len() != 32 || word[..12] != [0; 12] {
        return Err(AbiError::OutOfRange);
    }
    let mut address = [0_u8; 20];
    address.copy_from_slice(&word[12..]);
    Ok(address)
}

fn as_bool(word: &[u8]) -> Result<bool, AbiError> {
    match as_u64(word)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(AbiError::OutOfRange),
    }
}

fn as_word(word: &[u8]) -> Result<[u8; 32], AbiError> {
    word.try_into().map_err(|_| AbiError::Malformed)
}

fn address_array(data: &[u8], offset: usize) -> Result<Vec<[u8; 20]>, AbiError> {
    let length = as_usize(word_at(data, offset)?)?;
    if length > MAX_ATTESTORS {
        return Err(AbiError::OutOfRange);
    }
    (0..length)
        .map(|index| as_address(word_at(data, offset + 32 + index * 32)?))
        .collect()
}

/// Decodes a single `bool` return value.
///
/// # Errors
///
/// Refuses anything but exactly one canonical boolean word.
pub fn decode_bool(data: &[u8]) -> Result<bool, AbiError> {
    if data.len() != 32 {
        return Err(AbiError::Malformed);
    }
    as_bool(data)
}

/// Decodes the vault's `threshold()` return value.
///
/// # Errors
///
/// Refuses anything but one word that fits in 64 bits.
pub fn decode_threshold(data: &[u8]) -> Result<u64, AbiError> {
    if data.len() != 32 {
        return Err(AbiError::Malformed);
    }
    as_u64(data)
}

/// Decodes the vault's `attestors()` return value.
///
/// # Errors
///
/// Refuses malformed or oversized arrays and non-canonical addresses.
pub fn decode_address_list(data: &[u8]) -> Result<Vec<[u8; 20]>, AbiError> {
    let offset = as_usize(word(data, 0)?)?;
    address_array(data, offset)
}

/// The precompile's `getAttestors()`: the current set and its threshold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttestorSet {
    pub signers: Vec<[u8; 20]>,
    pub threshold: u64,
}

/// Decodes `getAttestors() returns (address[], uint256[], uint32)`.
///
/// # Errors
///
/// Refuses malformed arrays and a threshold wider than 32 bits.
pub fn decode_get_attestors(data: &[u8]) -> Result<AttestorSet, AbiError> {
    let signers = address_array(data, as_usize(word(data, 0)?)?)?;
    let threshold = as_u64(word(data, 2)?)?;
    if threshold > u64::from(u32::MAX) {
        return Err(AbiError::OutOfRange);
    }
    Ok(AttestorSet { signers, threshold })
}

/// The precompile's registration of one external chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChainRegistration {
    pub registered: bool,
    pub vault: [u8; 20],
    pub finality_depth: u64,
    pub enabled: bool,
}

/// Decodes `getChain(chain) returns (bool, address, uint64, bool)`.
///
/// # Errors
///
/// Refuses anything but four canonical words.
pub fn decode_get_chain(data: &[u8]) -> Result<ChainRegistration, AbiError> {
    if data.len() != 4 * 32 {
        return Err(AbiError::Malformed);
    }
    Ok(ChainRegistration {
        registered: as_bool(word(data, 0)?)?,
        vault: as_address(word(data, 1)?)?,
        finality_depth: as_u64(word(data, 2)?)?,
        enabled: as_bool(word(data, 3)?)?,
    })
}

/// Where a log sits on its chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogPosition {
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub tx_hash: [u8; 32],
    pub log_index: u64,
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str, AbiError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(AbiError::Malformed)
}

fn log_parts(
    value: &Value,
    emitter: &[u8; 20],
    topic: &[u8; 32],
) -> Result<(LogPosition, Vec<[u8; 32]>, Vec<u8>), AbiError> {
    if value.get("removed").and_then(Value::as_bool) == Some(true) {
        return Err(AbiError::RemovedLog);
    }
    let address: [u8; 20] = hex::fixed(text(value, "address")?).map_err(|_| AbiError::Malformed)?;
    if address != *emitter {
        return Err(AbiError::UnexpectedLog);
    }
    let topics = value
        .get("topics")
        .and_then(Value::as_array)
        .ok_or(AbiError::Malformed)?
        .iter()
        .map(|topic| {
            topic
                .as_str()
                .ok_or(AbiError::Malformed)
                .and_then(|topic| hex::fixed::<32>(topic).map_err(|_| AbiError::Malformed))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if topics.len() != 4 || topics[0] != *topic {
        return Err(AbiError::UnexpectedLog);
    }
    let data = hex::decode(text(value, "data")?).map_err(|_| AbiError::Malformed)?;
    if data.len() != 64 {
        return Err(AbiError::UnexpectedLog);
    }
    let position = LogPosition {
        block_number: hex::parse_quantity(text(value, "blockNumber")?)
            .map_err(|_| AbiError::Malformed)?,
        block_hash: hex::fixed(text(value, "blockHash")?).map_err(|_| AbiError::Malformed)?,
        tx_hash: hex::fixed(text(value, "transactionHash")?).map_err(|_| AbiError::Malformed)?,
        log_index: hex::parse_quantity(text(value, "logIndex")?)
            .map_err(|_| AbiError::Malformed)?,
    };
    Ok((position, topics, data))
}

/// A decoded `BridgeDeposit` log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepositLog {
    pub position: LogPosition,
    pub asset: [u8; 20],
    pub sender: [u8; 20],
    pub paxeer_recipient: [u8; 32],
    pub amount: [u8; 32],
    pub nonce: u64,
}

impl DepositLog {
    #[must_use]
    pub const fn attestation(&self, chain_id: u64, vault: [u8; 20]) -> InboundAttestation {
        InboundAttestation {
            chain_id,
            vault,
            tx_hash: self.position.tx_hash,
            log_index: self.position.log_index,
            recipient: self.paxeer_recipient,
            asset: self.asset,
            amount: self.amount,
        }
    }
}

/// Decodes one `eth_getLogs` entry as `BridgeDeposit(address indexed asset,
/// uint256 amount, address indexed sender, bytes32 indexed paxeerRecipient,
/// uint64 nonce)` emitted by `vault`.
///
/// # Errors
///
/// Refuses removed logs, other emitters or events, and non-canonical words.
pub fn decode_deposit_log(value: &Value, vault: &[u8; 20]) -> Result<DepositLog, AbiError> {
    let (position, topics, data) = log_parts(value, vault, &BRIDGE_DEPOSIT_TOPIC)?;
    Ok(DepositLog {
        position,
        asset: as_address(&topics[1])?,
        sender: as_address(&topics[2])?,
        paxeer_recipient: topics[3],
        amount: as_word(word(&data, 0)?)?,
        nonce: as_u64(word(&data, 1)?)?,
    })
}

/// A decoded Paxeer `BridgeOut` log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BurnLog {
    pub position: LogPosition,
    pub chain_id: u64,
    pub asset: [u8; 20],
    pub amount: [u8; 32],
    pub recipient: [u8; 20],
    pub nonce: u64,
}

impl BurnLog {
    #[must_use]
    pub const fn attestation(&self, vault: [u8; 20]) -> OutboundAttestation {
        OutboundAttestation {
            chain_id: self.chain_id,
            vault,
            paxeer_tx_hash: self.position.tx_hash,
            paxeer_nonce: self.nonce,
            recipient: self.recipient,
            asset: self.asset,
            amount: self.amount,
        }
    }
}

/// Decodes one `eth_getLogs` entry as `BridgeOut(uint64 indexed chain,
/// address indexed asset, uint256 amount, address recipient,
/// uint64 indexed nonce)` emitted by the precompile.
///
/// # Errors
///
/// Refuses removed logs, other emitters or events, and non-canonical words.
pub fn decode_burn_log(value: &Value) -> Result<BurnLog, AbiError> {
    let (position, topics, data) = log_parts(value, &LAYERX_BRIDGE_PRECOMPILE, &BRIDGE_OUT_TOPIC)?;
    Ok(BurnLog {
        position,
        chain_id: as_u64(&topics[1])?,
        asset: as_address(&topics[2])?,
        amount: as_word(word(&data, 0)?)?,
        recipient: as_address(word(&data, 1)?)?,
        nonce: as_u64(&topics[3])?,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use sha3::{Digest as _, Keccak256};

    use super::*;

    fn selector(signature: &str) -> [u8; 4] {
        let digest = Keccak256::digest(signature.as_bytes());
        [digest[0], digest[1], digest[2], digest[3]]
    }

    fn amount() -> [u8; 32] {
        let mut amount = [0_u8; 32];
        amount[16..].copy_from_slice(&1_000_000_000_000_000_000_u128.to_be_bytes());
        amount
    }

    fn signature(fill: u8, v: u8) -> [u8; 65] {
        let mut signature = [fill; 65];
        signature[64] = v;
        signature
    }

    #[test]
    fn selectors_and_topics_are_the_keccak_of_their_signatures() {
        let topic =
            |signature: &str| -> [u8; 32] { Keccak256::digest(signature.as_bytes()).into() };
        assert_eq!(
            topic("BridgeDeposit(address,uint256,address,bytes32,uint64)"),
            BRIDGE_DEPOSIT_TOPIC
        );
        assert_eq!(
            topic("BridgeOut(uint64,address,uint256,address,uint64)"),
            BRIDGE_OUT_TOPIC
        );
        assert_eq!(
            selector("bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])"),
            BRIDGE_IN_SELECTOR
        );
        assert_eq!(
            selector("isNullified(uint64,bytes32,uint64)"),
            IS_NULLIFIED_SELECTOR
        );
        assert_eq!(selector("getAttestors()"), GET_ATTESTORS_SELECTOR);
        assert_eq!(selector("getChain(uint64)"), GET_CHAIN_SELECTOR);
        assert_eq!(
            selector("release(address,uint256,address,bytes32,uint64,bytes[])"),
            RELEASE_SELECTOR
        );
        assert_eq!(selector("threshold()"), THRESHOLD_SELECTOR);
        assert_eq!(selector("attestors()"), ATTESTORS_SELECTOR);
        assert_eq!(selector("nullified(bytes32)"), NULLIFIED_SELECTOR);
    }

    // Reference calldata produced by Foundry `cast calldata` for the same
    // arguments, independent of this encoder.
    #[test]
    fn bridge_in_calldata_matches_the_solidity_abi_encoder() {
        let attestation = InboundAttestation {
            chain_id: 1,
            vault: [0x11; 20],
            tx_hash: [0x22; 32],
            log_index: 7,
            recipient: [0x55; 32],
            asset: [0x44; 20],
            amount: amount(),
        };
        let encoded = encode_bridge_in(
            &attestation,
            &[signature(0x11, 0x1b), signature(0x22, 0x1c)],
        );
        assert_eq!(
            hex::prefixed(&encoded),
            concat!(
                "0x09063e9c",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000001111111111111111111111111111111111111111",
                "2222222222222222222222222222222222222222222222222222222222222222",
                "0000000000000000000000000000000000000000000000000000000000000007",
                "5555555555555555555555555555555555555555555555555555555555555555",
                "0000000000000000000000004444444444444444444444444444444444444444",
                "0000000000000000000000000000000000000000000000000de0b6b3a7640000",
                "0000000000000000000000000000000000000000000000000000000000000100",
                "0000000000000000000000000000000000000000000000000000000000000002",
                "0000000000000000000000000000000000000000000000000000000000000040",
                "00000000000000000000000000000000000000000000000000000000000000c0",
                "0000000000000000000000000000000000000000000000000000000000000041",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "1b00000000000000000000000000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000041",
                "2222222222222222222222222222222222222222222222222222222222222222",
                "2222222222222222222222222222222222222222222222222222222222222222",
                "1c00000000000000000000000000000000000000000000000000000000000000",
            )
        );
    }

    #[test]
    fn release_calldata_matches_the_solidity_abi_encoder() {
        let attestation = OutboundAttestation {
            chain_id: 1,
            vault: [0x11; 20],
            paxeer_tx_hash: [0x22; 32],
            paxeer_nonce: 7,
            recipient: [0x33; 20],
            asset: [0x44; 20],
            amount: amount(),
        };
        let encoded = encode_release(&attestation, &[signature(0x11, 0x1b)]);
        assert_eq!(
            hex::prefixed(&encoded),
            concat!(
                "0x05eb766d",
                "0000000000000000000000004444444444444444444444444444444444444444",
                "0000000000000000000000000000000000000000000000000de0b6b3a7640000",
                "0000000000000000000000003333333333333333333333333333333333333333",
                "2222222222222222222222222222222222222222222222222222222222222222",
                "0000000000000000000000000000000000000000000000000000000000000007",
                "00000000000000000000000000000000000000000000000000000000000000c0",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000020",
                "0000000000000000000000000000000000000000000000000000000000000041",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "1111111111111111111111111111111111111111111111111111111111111111",
                "1b00000000000000000000000000000000000000000000000000000000000000",
            )
        );
    }

    #[test]
    fn view_calls_and_returns_match_the_solidity_abi_encoder() {
        assert_eq!(
            hex::prefixed(&encode_is_nullified(1, &[0xaa; 32], 3)),
            concat!(
                "0xcaa58bc9",
                "0000000000000000000000000000000000000000000000000000000000000001",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "0000000000000000000000000000000000000000000000000000000000000003",
            )
        );
        assert_eq!(
            hex::prefixed(&encode_get_chain(1)),
            "0xa1a6d5080000000000000000000000000000000000000000000000000000000000000001"
        );
        let registration = hex::decode(concat!(
            "0x",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "0000000000000000000000001111111111111111111111111111111111111111",
            "000000000000000000000000000000000000000000000000000000000000000c",
            "0000000000000000000000000000000000000000000000000000000000000001",
        ))
        .unwrap_or_default();
        assert_eq!(
            decode_get_chain(&registration),
            Ok(ChainRegistration {
                registered: true,
                vault: [0x11; 20],
                finality_depth: 12,
                enabled: true,
            })
        );
        let attestors = hex::decode(concat!(
            "0x",
            "0000000000000000000000000000000000000000000000000000000000000060",
            "00000000000000000000000000000000000000000000000000000000000000c0",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "000000000000000000000000d2431ca38735c2fd438e2caa23f094191d89675b",
            "000000000000000000000000612b7be154a64292aae070aaa86fcd66ba218071",
            "0000000000000000000000000000000000000000000000000000000000000002",
            "00000000000000000000000000000000000000000000000000000000000003e8",
            "00000000000000000000000000000000000000000000000000000000000003e8",
        ))
        .unwrap_or_default();
        let set = decode_get_attestors(&attestors);
        assert_eq!(set.as_ref().map(|set| set.threshold), Ok(2));
        assert_eq!(
            set.map(|set| set
                .signers
                .iter()
                .map(|signer| hex::prefixed(signer))
                .collect::<Vec<_>>()),
            Ok(vec![
                "0xd2431ca38735c2fd438e2caa23f094191d89675b".to_owned(),
                "0x612b7be154a64292aae070aaa86fcd66ba218071".to_owned(),
            ])
        );
        let list = hex::decode(concat!(
            "0x",
            "0000000000000000000000000000000000000000000000000000000000000020",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "000000000000000000000000d2431ca38735c2fd438e2caa23f094191d89675b",
        ))
        .unwrap_or_default();
        assert_eq!(decode_address_list(&list).map(|list| list.len()), Ok(1));
        assert_eq!(
            hex::prefixed(&encode_nullified(&[0x82; 32])),
            format!("0xe73bdb5e{}", "82".repeat(32))
        );
        assert_eq!(decode_bool(&[0; 32]), Ok(false));
        let mut two = [0_u8; 32];
        two[31] = 2;
        assert_eq!(decode_bool(&two), Err(AbiError::OutOfRange));
        assert_eq!(decode_bool(&[0; 31]), Err(AbiError::Malformed));
    }

    #[test]
    fn deposit_and_burn_logs_decode_and_foreign_logs_are_refused() {
        let vault = [0x11; 20];
        let deposit = json!({
            "address": "0x1111111111111111111111111111111111111111",
            "topics": [
                hex::prefixed(&BRIDGE_DEPOSIT_TOPIC),
                "0x0000000000000000000000004444444444444444444444444444444444444444",
                "0x0000000000000000000000009999999999999999999999999999999999999999",
                "0x0000000000000000000000005555555555555555555555555555555555555555"
            ],
            "data": "0x0000000000000000000000000000000000000000000000000de0b6b3a76400000000000000000000000000000000000000000000000000000000000000000004",
            "blockNumber": "0x64",
            "blockHash": format!("0x{}", "bb".repeat(32)),
            "transactionHash": format!("0x{}", "aa".repeat(32)),
            "logIndex": "0x3",
            "removed": false
        });
        let decoded = decode_deposit_log(&deposit, &vault);
        assert_eq!(decoded.map(|log| log.nonce), Ok(4));
        assert_eq!(decoded.map(|log| log.position.log_index), Ok(3));
        assert_eq!(decoded.map(|log| log.asset), Ok([0x44; 20]));
        assert_eq!(decoded.map(|log| log.amount), Ok(amount()));
        assert_eq!(
            decode_deposit_log(&deposit, &[0x12; 20]),
            Err(AbiError::UnexpectedLog)
        );
        let mut removed = deposit.clone();
        removed["removed"] = json!(true);
        assert_eq!(
            decode_deposit_log(&removed, &vault),
            Err(AbiError::RemovedLog)
        );
        let mut dirty = deposit.clone();
        dirty["topics"][1] =
            json!("0x0000000000000000000000014444444444444444444444444444444444444444");
        assert_eq!(
            decode_deposit_log(&dirty, &vault),
            Err(AbiError::OutOfRange)
        );

        let burn = json!({
            "address": "0x0000000000000000000000000000000000001016",
            "topics": [
                hex::prefixed(&BRIDGE_OUT_TOPIC),
                "0x0000000000000000000000000000000000000000000000000000000000000001",
                "0x0000000000000000000000004444444444444444444444444444444444444444",
                "0x0000000000000000000000000000000000000000000000000000000000000007"
            ],
            "data": "0x0000000000000000000000000000000000000000000000000de0b6b3a76400000000000000000000000000003333333333333333333333333333333333333333",
            "blockNumber": "0x35",
            "blockHash": format!("0x{}", "dd".repeat(32)),
            "transactionHash": format!("0x{}", "22".repeat(32)),
            "logIndex": "0x0"
        });
        let decoded = decode_burn_log(&burn);
        assert_eq!(
            decoded.map(|log| log.attestation(vault).digest()),
            Ok(hex::fixed::<32>(
                "0xbd35888e4b158986238ce7abe73957702e2f6e78fe6157197878ebd13edf5b37"
            )
            .unwrap_or_default())
        );
        assert_eq!(decode_burn_log(&deposit), Err(AbiError::UnexpectedLog));
    }
}
