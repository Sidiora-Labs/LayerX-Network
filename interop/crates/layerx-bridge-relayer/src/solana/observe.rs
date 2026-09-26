//! The Solana observer: follows the custody program's signatures up to the
//! configured slot depth, reads each deposit's logged record, its receipt PDA
//! and its asset PDA, and turns every deposit into the inbound attestation the
//! Paxeer precompile verifies.
//!
//! A deposit's txHash is keccak256 of the 64-byte signature of the transaction
//! that carried it and its logIndex is the nonce its receipt records. The
//! receipt and the asset record are the program's own state and are
//! authoritative; the logged record must agree with them field for field, and
//! a deposit whose record and receipt disagree is refused, never signed.

use super::rpc::{
    AccountData, Commitment, Instruction, SignatureInfo, SolanaRpc, SolanaTransaction,
};
use super::{base58_encode, base58_fixed, inbound_tx_hash, HANDLE_BYTES};
use crate::attestation::{uint256_from_u64, InboundAttestation};
use crate::hex;
use crate::journal::{Observation, Position};
use crate::relayer::RelayerError;
use crate::rpc::RpcFault;

/// Every custody instruction starts with this prefix and layout version.
pub const INSTRUCTION_MAGIC: &[u8; 4] = b"PXBR";
pub const INSTRUCTION_VERSION: u16 = 1;
/// The custody program's deposit instruction.
pub const OP_DEPOSIT: u8 = 8;
/// magic, version, op, amount, 32-byte Paxeer recipient.
pub const DEPOSIT_DATA_BYTES: usize = 4 + 2 + 1 + 8 + 32;
/// Positions of the accounts the relayer reads in a deposit instruction.
const DEPOSIT_ASSET_ACCOUNT: usize = 2;
const DEPOSIT_MINT_ACCOUNT: usize = 3;
const DEPOSIT_RECEIPT_ACCOUNT: usize = 6;
const DEPOSIT_ACCOUNTS: usize = 9;

pub const LAYOUT_VERSION: u16 = 1;
pub const RECEIPT_MAGIC: &[u8; 8] = b"PXBRRCP0";
pub const RECEIPT_BYTES: usize = 130;
pub const ASSET_MAGIC: &[u8; 8] = b"PXBRAST0";
pub const ASSET_BYTES: usize = 89;

/// The record a deposit logs, and its prefix in the program's log.
pub const DEPOSIT_RECORD: &str = "PXBR/deposit/v1";
const PROGRAM_LOG: &str = "Program log: ";

/// The page size of every `getSignaturesForAddress` call.
pub const SIGNATURE_PAGE: usize = 1000;

/// The reason a deposit whose logged record and receipt disagree is refused.
pub const DISAGREEMENT: &str = "the logged deposit record disagrees with its receipt";

/// One observed Solana custody program.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SolanaSettings {
    /// [`super::SOLANA_CHAIN_ID`]; the chain the precompile registered.
    pub chain_id: u64,
    /// The handle of the program's vault-authority PDA, as registered.
    pub vault: [u8; HANDLE_BYTES],
    pub program_id: [u8; 32],
    /// How many slots below the head at `commitment` a deposit must be.
    pub finality_depth: u64,
    pub start_slot: u64,
    pub max_slot_range: u64,
    pub commitment: Commitment,
}

/// A deposit's logged record, as the program's `deposit_record` writes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepositRecord {
    pub nonce: u64,
    pub mint: [u8; 32],
    pub asset: [u8; HANDLE_BYTES],
    pub amount: u64,
    pub recipient: [u8; HANDLE_BYTES],
    pub depositor: [u8; 32],
    pub slot: u64,
}

/// A deposit-receipt PDA, as the program's state layout writes it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DepositReceipt {
    pub nonce: u64,
    pub mint: [u8; 32],
    pub amount: u64,
    pub paxeer_recipient: [u8; 32],
    pub depositor: [u8; 32],
    pub slot: u64,
}

/// The fields of an asset PDA the relayer reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AssetRecord {
    pub mint: [u8; 32],
    pub asset_id: [u8; HANDLE_BYTES],
    pub decimals: u8,
}

/// What the observer found for one deposit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Finding {
    /// A deposit whose record, receipt and asset agree: attest and submit it.
    Deposit(Observation),
    /// A deposit read from its receipt that must never be attested.
    Refused {
        observation: Observation,
        reason: String,
    },
}

fn malformed() -> RelayerError {
    RelayerError::Rpc(RpcFault::Malformed)
}

fn be_u64(bytes: &[u8]) -> Option<u64> {
    bytes.try_into().ok().map(u64::from_be_bytes)
}

fn be_u16(bytes: &[u8]) -> Option<u16> {
    bytes.try_into().ok().map(u16::from_be_bytes)
}

fn array<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.try_into().ok()
}

impl DepositReceipt {
    /// Decodes a receipt record; `None` for any other length, magic or
    /// layout version.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != RECEIPT_BYTES
            || &bytes[..8] != RECEIPT_MAGIC
            || be_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return None;
        }
        Some(Self {
            nonce: be_u64(&bytes[10..18])?,
            mint: array(&bytes[18..50])?,
            amount: be_u64(&bytes[50..58])?,
            paxeer_recipient: array(&bytes[58..90])?,
            depositor: array(&bytes[90..122])?,
            slot: be_u64(&bytes[122..130])?,
        })
    }
}

impl AssetRecord {
    /// Decodes an asset record; `None` for any other length, magic or layout
    /// version.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != ASSET_BYTES
            || &bytes[..8] != ASSET_MAGIC
            || be_u16(&bytes[8..10])? != LAYOUT_VERSION
        {
            return None;
        }
        Some(Self {
            mint: array(&bytes[10..42])?,
            asset_id: array(&bytes[42..62])?,
            decimals: bytes[62],
        })
    }
}

/// A 20-byte handle as the program logs it: forty lowercase hex digits, no
/// prefix.
fn logged_handle(text: &str) -> Option<[u8; HANDLE_BYTES]> {
    if !text
        .bytes()
        .all(|digit| digit.is_ascii_digit() || (b'a'..=b'f').contains(&digit))
    {
        return None;
    }
    hex::fixed::<HANDLE_BYTES>(&format!("0x{text}")).ok()
}

impl DepositRecord {
    /// Parses `PXBR/deposit/v1 nonce=… mint=… asset=… amount=… recipient=…
    /// depositor=… slot=…`, every field present once, in that order.
    #[must_use]
    pub fn parse(line: &str) -> Option<Self> {
        let mut words = line.split(' ');
        if words.next()? != DEPOSIT_RECORD {
            return None;
        }
        let mut value = |name: &str| -> Option<&str> {
            words
                .next()?
                .strip_prefix(name)?
                .strip_prefix('=')
                .filter(|value| !value.is_empty())
        };
        let nonce = value("nonce")?.parse().ok()?;
        let mint = base58_fixed::<32>(value("mint")?).ok()?;
        let asset = logged_handle(value("asset")?)?;
        let amount = value("amount")?.parse().ok()?;
        let recipient = logged_handle(value("recipient")?)?;
        let depositor = base58_fixed::<32>(value("depositor")?).ok()?;
        let slot = value("slot")?.parse().ok()?;
        if words.next().is_some() {
            return None;
        }
        Some(Self {
            nonce,
            mint,
            asset,
            amount,
            recipient,
            depositor,
            slot,
        })
    }
}

/// The log lines `program` itself wrote, in order. Solana prefixes every
/// program's `msg!` output identically, so a line is attributed to the program
/// on top of the invocation stack the `invoke` / `success` / `failed` lines
/// describe; another program cannot write a line that counts as the custody
/// program's. A truncated or unbalanced log is malformed.
///
/// # Errors
///
/// Returns `Malformed` for a truncated log or an unbalanced invocation stack.
pub fn program_log_lines<'a>(
    logs: &'a [String],
    program: &[u8; 32],
) -> Result<Vec<&'a str>, RelayerError> {
    let program = base58_encode(program);
    let mut stack: Vec<&str> = Vec::new();
    let mut lines = Vec::new();
    for line in logs {
        if line == "Log truncated" {
            return Err(malformed());
        }
        if let Some(message) = line.strip_prefix(PROGRAM_LOG) {
            if stack.last() == Some(&program.as_str()) {
                lines.push(message);
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("Program ") else {
            continue;
        };
        let Some((id, event)) = rest.split_once(' ') else {
            continue;
        };
        if event.starts_with("invoke [") {
            stack.push(id);
        } else if (event == "success" || event.starts_with("failed")) && stack.pop() != Some(id) {
            return Err(malformed());
        }
    }
    if !stack.is_empty() {
        return Err(malformed());
    }
    Ok(lines)
}

/// The deposit records `program` logged, in execution order.
///
/// # Errors
///
/// Returns `Malformed` for an unreadable log or a record line that does not
/// parse.
pub fn deposit_records(
    logs: &[String],
    program: &[u8; 32],
) -> Result<Vec<DepositRecord>, RelayerError> {
    program_log_lines(logs, program)?
        .into_iter()
        .filter(|line| line.split(' ').next() == Some(DEPOSIT_RECORD))
        .map(|line| DepositRecord::parse(line).ok_or_else(malformed))
        .collect()
}

/// The amount and 32-byte recipient of a deposit instruction's data, or
/// `None` when the data is not a deposit of this layout version.
#[must_use]
pub fn deposit_instruction(data: &[u8]) -> Option<(u64, [u8; 32])> {
    if data.len() != DEPOSIT_DATA_BYTES
        || &data[..4] != INSTRUCTION_MAGIC
        || be_u16(&data[4..6])? != INSTRUCTION_VERSION
        || data[6] != OP_DEPOSIT
    {
        return None;
    }
    Some((be_u64(&data[7..15])?, array(&data[15..47])?))
}

fn padded(address: &[u8; HANDLE_BYTES]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[32 - HANDLE_BYTES..].copy_from_slice(address);
    word
}

fn program_account(
    rpc: &SolanaRpc,
    settings: &SolanaSettings,
    address: &[u8; 32],
) -> Result<AccountData, RelayerError> {
    let account = rpc
        .get_account_info(address, settings.commitment)?
        .ok_or_else(malformed)?;
    if account.owner != settings.program_id {
        return Err(malformed());
    }
    Ok(account)
}

struct DepositCall<'a> {
    instruction: &'a Instruction,
    amount: u64,
    recipient: [u8; 32],
}

/// Whether the record, the receipt, the asset and the instruction describe
/// one deposit in one slot.
fn agrees(
    record: &DepositRecord,
    receipt: &DepositReceipt,
    asset: &AssetRecord,
    call: &DepositCall<'_>,
    slot: u64,
) -> bool {
    let mint = call.instruction.accounts[DEPOSIT_MINT_ACCOUNT];
    record.nonce == receipt.nonce
        && record.mint == receipt.mint
        && record.amount == receipt.amount
        && padded(&record.recipient) == receipt.paxeer_recipient
        && record.depositor == receipt.depositor
        && record.slot == receipt.slot
        && record.asset == asset.asset_id
        && asset.mint == receipt.mint
        && mint == receipt.mint
        && call.amount == receipt.amount
        && call.recipient == receipt.paxeer_recipient
        && receipt.slot == slot
}

/// Every deposit one successful transaction carried.
fn transaction_findings(
    rpc: &SolanaRpc,
    settings: &SolanaSettings,
    info: &SignatureInfo,
    transaction: &SolanaTransaction,
) -> Result<Vec<Finding>, RelayerError> {
    if transaction.slot != info.slot
        || transaction.failed
        || transaction.signatures.first() != Some(&info.signature)
    {
        return Err(malformed());
    }
    let calls: Vec<DepositCall<'_>> = transaction
        .instructions
        .iter()
        .filter(|instruction| instruction.program == settings.program_id)
        .filter_map(|instruction| {
            deposit_instruction(&instruction.data).map(|(amount, recipient)| DepositCall {
                instruction,
                amount,
                recipient,
            })
        })
        .collect();
    if calls
        .iter()
        .any(|call| call.instruction.accounts.len() != DEPOSIT_ACCOUNTS)
    {
        return Err(malformed());
    }
    let records = deposit_records(&transaction.log_messages, &settings.program_id)?;
    let tx_hash = inbound_tx_hash(&info.signature);
    let mut findings = Vec::with_capacity(calls.len());
    for (index, call) in calls.iter().enumerate() {
        let receipt = DepositReceipt::decode(
            &program_account(
                rpc,
                settings,
                &call.instruction.accounts[DEPOSIT_RECEIPT_ACCOUNT],
            )?
            .data,
        )
        .ok_or_else(malformed)?;
        let asset = AssetRecord::decode(
            &program_account(
                rpc,
                settings,
                &call.instruction.accounts[DEPOSIT_ASSET_ACCOUNT],
            )?
            .data,
        )
        .ok_or_else(malformed)?;
        let observation = Observation::inbound(
            &InboundAttestation {
                chain_id: settings.chain_id,
                vault: settings.vault,
                tx_hash,
                log_index: receipt.nonce,
                recipient: receipt.paxeer_recipient,
                asset: asset.asset_id,
                amount: uint256_from_u64(receipt.amount),
            },
            // Solana has no block hash at a slot in the transaction answer; the
            // position pins the slot and the blockhash the transaction was
            // built against, which a rescan reads back identically.
            Position {
                block_number: transaction.slot,
                block_hash: transaction.recent_blockhash,
            },
        );
        let consistent = records.len() == calls.len()
            && agrees(&records[index], &receipt, &asset, call, transaction.slot);
        findings.push(if consistent {
            Finding::Deposit(observation)
        } else {
            Finding::Refused {
                observation,
                reason: DISAGREEMENT.to_owned(),
            }
        });
    }
    Ok(findings)
}

/// The successful signatures of the program in slots `from..=to`, oldest
/// first, paging back from the newest until a slot below `from`.
fn signatures_in(
    rpc: &SolanaRpc,
    settings: &SolanaSettings,
    from: u64,
    to: u64,
) -> Result<Vec<SignatureInfo>, RelayerError> {
    let mut found = Vec::new();
    let mut before: Option<[u8; 64]> = None;
    let mut previous_slot = u64::MAX;
    loop {
        let page = rpc.get_signatures_for_address(
            &settings.program_id,
            before.as_ref(),
            SIGNATURE_PAGE,
            settings.commitment,
        )?;
        let mut reached_start = false;
        for info in &page {
            if info.slot > previous_slot {
                return Err(malformed());
            }
            previous_slot = info.slot;
            if info.slot < from {
                reached_start = true;
                break;
            }
            if info.slot <= to && !info.failed {
                found.push(*info);
            }
        }
        match page.last() {
            Some(last) if !reached_start && page.len() == SIGNATURE_PAGE => {
                if before == Some(last.signature) {
                    return Err(malformed());
                }
                before = Some(last.signature);
            }
            _ => break,
        }
    }
    found.reverse();
    Ok(found)
}

/// Observes every deposit into the custody program in slots `from..=to`.
/// The caller bounds `to` by the head at the configured commitment minus the
/// slot finality depth, and a transaction the cluster places above `to` is
/// never read, so only deposits confirmed to that depth are observed.
///
/// # Errors
///
/// Returns the first RPC fault, or `Malformed` for a transaction, receipt or
/// asset record that cannot be read as the program writes it; the scan is
/// retried from the same slot on the next pass.
pub fn observe_deposits(
    rpc: &SolanaRpc,
    settings: &SolanaSettings,
    from: u64,
    to: u64,
) -> Result<Vec<Finding>, RelayerError> {
    if from > to {
        return Ok(Vec::new());
    }
    let mut findings = Vec::new();
    for info in signatures_in(rpc, settings, from, to)? {
        let transaction = rpc
            .get_transaction(&info.signature, settings.commitment)?
            .ok_or(RelayerError::Rpc(RpcFault::Unavailable))?;
        findings.extend(transaction_findings(rpc, settings, &info, &transaction)?);
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROGRAM: &str = "A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9";
    const TOKEN: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    const RECORD: &str = "PXBR/deposit/v1 nonce=7 mint=5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump asset=21f7b20a555199fa73a238b1a91fd0f549068fee amount=12345678 recipient=b65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5 depositor=4XijNxiwmXKJFacayVqi3WD66fnzEDgvGpMD57dyQyBb slot=1050";

    fn program() -> [u8; 32] {
        base58_fixed::<32>(PROGRAM).unwrap_or_else(|error| panic!("program: {error}"))
    }

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    #[test]
    fn the_record_parses_field_for_field() {
        let record = DepositRecord::parse(RECORD).unwrap_or_else(|| panic!("record"));
        assert_eq!(record.nonce, 7);
        assert_eq!(record.amount, 12_345_678);
        assert_eq!(record.slot, 1050);
        assert_eq!(
            hex::prefixed(&record.asset),
            "0x21f7b20a555199fa73a238b1a91fd0f549068fee"
        );
        assert_eq!(
            hex::prefixed(&record.recipient),
            "0xb65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5"
        );
        assert_eq!(
            base58_encode(&record.mint),
            "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump"
        );
    }

    #[test]
    fn a_record_with_a_missing_extra_or_reordered_field_does_not_parse() {
        assert_eq!(
            DepositRecord::parse(&RECORD.replace(" slot=1050", "")),
            None
        );
        assert_eq!(DepositRecord::parse(&format!("{RECORD} extra=1")), None);
        assert_eq!(
            DepositRecord::parse(&RECORD.replace("nonce=7 mint=", "mint=")),
            None
        );
        assert_eq!(
            DepositRecord::parse(&RECORD.replace("amount=12345678", "amount=")),
            None
        );
        assert_eq!(
            DepositRecord::parse(&RECORD.replace("PXBR/deposit/v1", "PXBR/deposit/v2")),
            None
        );
        assert_eq!(
            DepositRecord::parse(&RECORD.replace("asset=21f7", "asset=21F7")),
            None
        );
        assert_eq!(
            DepositRecord::parse(&RECORD.replace("asset=21f7", "asset=0x21f7")),
            None
        );
    }

    #[test]
    fn only_lines_the_program_itself_wrote_are_its_records() {
        let spoof = format!("Program log: {RECORD}");
        let logs = lines(&[
            &format!("Program {TOKEN} invoke [1]"),
            &spoof,
            &format!("Program {TOKEN} success"),
            &format!("Program {PROGRAM} invoke [1]"),
            &format!("Program {TOKEN} invoke [2]"),
            &spoof,
            &format!("Program {TOKEN} success"),
            &spoof,
            &format!("Program {PROGRAM} consumed 41000 of 200000 compute units"),
            &format!("Program {PROGRAM} success"),
        ]);
        let records =
            deposit_records(&logs, &program()).unwrap_or_else(|error| panic!("records: {error}"));
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].nonce, 7);
    }

    #[test]
    fn a_truncated_or_unbalanced_log_is_malformed() {
        let truncated = lines(&[&format!("Program {PROGRAM} invoke [1]"), "Log truncated"]);
        assert_eq!(
            program_log_lines(&truncated, &program()),
            Err(RelayerError::Rpc(RpcFault::Malformed))
        );
        let unbalanced = lines(&[
            &format!("Program {PROGRAM} invoke [1]"),
            &format!("Program {TOKEN} success"),
        ]);
        assert_eq!(
            program_log_lines(&unbalanced, &program()),
            Err(RelayerError::Rpc(RpcFault::Malformed))
        );
        let open = lines(&[&format!("Program {PROGRAM} invoke [1]")]);
        assert_eq!(
            program_log_lines(&open, &program()),
            Err(RelayerError::Rpc(RpcFault::Malformed))
        );
    }

    #[test]
    fn deposit_instruction_data_is_read_only_in_its_own_layout() {
        let mut data = b"PXBR".to_vec();
        data.extend_from_slice(&[0, 1, OP_DEPOSIT]);
        data.extend_from_slice(&600_u64.to_be_bytes());
        data.extend_from_slice(&[0x55; 32]);
        assert_eq!(deposit_instruction(&data), Some((600, [0x55; 32])));
        let mut other = data.clone();
        other[6] = 9;
        assert_eq!(deposit_instruction(&other), None);
        let mut version = data.clone();
        version[5] = 2;
        assert_eq!(deposit_instruction(&version), None);
        assert_eq!(deposit_instruction(&data[..46]), None);
    }

    #[test]
    fn receipts_and_assets_decode_only_their_own_layout() {
        let mut receipt = RECEIPT_MAGIC.to_vec();
        receipt.extend_from_slice(&[0, 1]);
        receipt.extend_from_slice(&7_u64.to_be_bytes());
        receipt.extend_from_slice(&[0x11; 32]);
        receipt.extend_from_slice(&12_345_678_u64.to_be_bytes());
        receipt.extend_from_slice(&[0x22; 32]);
        receipt.extend_from_slice(&[0x33; 32]);
        receipt.extend_from_slice(&1050_u64.to_be_bytes());
        let decoded = DepositReceipt::decode(&receipt).unwrap_or_else(|| panic!("receipt"));
        assert_eq!(
            decoded,
            DepositReceipt {
                nonce: 7,
                mint: [0x11; 32],
                amount: 12_345_678,
                paxeer_recipient: [0x22; 32],
                depositor: [0x33; 32],
                slot: 1050,
            }
        );
        assert_eq!(AssetRecord::decode(&receipt), None);
        let mut version = receipt.clone();
        version[9] = 2;
        assert_eq!(DepositReceipt::decode(&version), None);
        assert_eq!(DepositReceipt::decode(&receipt[..129]), None);

        let mut asset = ASSET_MAGIC.to_vec();
        asset.extend_from_slice(&[0, 1]);
        asset.extend_from_slice(&[0x11; 32]);
        asset.extend_from_slice(&[0x21; 20]);
        asset.extend_from_slice(&[6, 1, 254]);
        asset.extend_from_slice(&[0; 24]);
        assert_eq!(
            AssetRecord::decode(&asset),
            Some(AssetRecord {
                mint: [0x11; 32],
                asset_id: [0x21; 20],
                decimals: 6,
            })
        );
        assert_eq!(DepositReceipt::decode(&asset), None);
    }
}
