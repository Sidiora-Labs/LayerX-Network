//! ABI of the native `LayerX` custody precompile `layerxCustody`.
//!
//! Every selector, topic and layout here is the one declared by
//! `precompiles/layerxcustody/abi.json`. Amounts are bank base units, one to
//! one with the `LayerX` u128 amount; the native coin moves as
//! `amount * WEI_PER_BASE_UNIT` wei and a wei remainder is refused.

use layerx_types::intent::EvmAddress;
use sha2::{Digest as _, Sha256};

use crate::client::LogRecord;

const WORD: usize = 32;

/// `0x0000000000000000000000000000000000001013`.
pub const CUSTODY_PRECOMPILE: EvmAddress = EvmAddress::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x13,
]);

/// Wei carried by one bank base unit of the native coin.
pub const WEI_PER_BASE_UNIT: u128 = 1_000_000_000_000;

/// Largest evidence argument the precompile accepts.
pub const MAX_EVIDENCE_BYTES: usize = 65_536;

pub const SELECTOR_DEPOSIT: [u8; 4] = [0xb2, 0x14, 0xfa, 0xa5];
pub const SELECTOR_DEPOSIT_TOKEN: [u8; 4] = [0x84, 0xab, 0xac, 0x95];
pub const SELECTOR_REQUEST_WITHDRAWAL: [u8; 4] = [0x3e, 0x4e, 0x8e, 0x1a];
pub const SELECTOR_FINALISE_WITHDRAWAL: [u8; 4] = [0x22, 0xda, 0xf6, 0x9e];
pub const SELECTOR_REQUEST_FORCED_EXIT: [u8; 4] = [0x79, 0xa9, 0xe2, 0x9c];
pub const SELECTOR_EXECUTE_FORCED_EXIT: [u8; 4] = [0x9a, 0x97, 0x95, 0xcd];
pub const SELECTOR_GET_CLAIM: [u8; 4] = [0xc9, 0x10, 0x0b, 0xcb];
pub const SELECTOR_NULLIFIER_STATUS: [u8; 4] = [0x5c, 0x4f, 0x76, 0x20];
pub const SELECTOR_GET_ASSET: [u8; 4] = [0x2c, 0xc3, 0xce, 0x80];
pub const SELECTOR_EXIT_ELIGIBLE: [u8; 4] = [0xa8, 0x7a, 0x6e, 0x98];
pub const SELECTOR_NATIVE_ASSET_ID: [u8; 4] = [0xaa, 0xfc, 0xde, 0x84];

pub const CUSTODY_DEPOSIT_TOPIC: [u8; 32] = [
    0x7e, 0xdb, 0x71, 0xc9, 0x10, 0x0c, 0x65, 0x68, 0x47, 0x89, 0x6d, 0x0b, 0x5b, 0x19, 0x4f, 0x69,
    0xf7, 0xda, 0x28, 0x7e, 0xb5, 0x79, 0x64, 0xa8, 0x1e, 0x7f, 0x80, 0x7a, 0x6a, 0x94, 0x40, 0x28,
];
pub const CLAIM_QUEUED_TOPIC: [u8; 32] = [
    0xc7, 0x32, 0xa8, 0x7b, 0x48, 0x0b, 0xe9, 0x51, 0xee, 0x9f, 0x6c, 0x11, 0x51, 0xf3, 0x77, 0x7c,
    0x75, 0x8a, 0x03, 0xaf, 0x59, 0xfa, 0x46, 0x78, 0x9b, 0xe6, 0x90, 0x8b, 0x76, 0xac, 0xa0, 0x98,
];
pub const CLAIM_FINALISED_TOPIC: [u8; 32] = [
    0xc0, 0x1c, 0xc7, 0x28, 0xe6, 0x7a, 0x51, 0x18, 0x15, 0xa1, 0x5f, 0x0f, 0x00, 0x30, 0xfc, 0x5a,
    0xc8, 0xdf, 0xc1, 0x5f, 0x7d, 0x0d, 0x45, 0xbe, 0x09, 0xef, 0xe2, 0xd4, 0x50, 0x85, 0x75, 0x82,
];
pub const CUSTODY_RELEASE_TOPIC: [u8; 32] = [
    0x37, 0x56, 0x7a, 0x5b, 0x2d, 0xe5, 0x43, 0xb7, 0x07, 0x16, 0x2e, 0xf2, 0x36, 0x9f, 0x95, 0xd3,
    0x9e, 0x53, 0xee, 0x43, 0x62, 0x1b, 0xd3, 0x29, 0xa9, 0x96, 0xc2, 0xb7, 0x26, 0xf9, 0x0f, 0x43,
];
pub const EMERGENCY_EXIT_EXECUTED_TOPIC: [u8; 32] = [
    0x4f, 0x80, 0x4c, 0xfb, 0x16, 0xd0, 0x2c, 0xd6, 0xd2, 0x54, 0x6b, 0x0c, 0x49, 0x34, 0x5e, 0x7f,
    0x0d, 0x40, 0xc2, 0x65, 0x6f, 0x14, 0xac, 0xaf, 0xb3, 0xd2, 0x27, 0xbe, 0xe9, 0xf9, 0x5b, 0x12,
];

const WITHDRAWAL_CLAIM_DOMAIN: &[u8] = b"LXP/Paxeer/withdrawal-claim/v1";
const EXIT_CLAIM_DOMAIN: &[u8] = b"LXP/Paxeer/emergency-exit/v1";
const NULLIFIER_DOMAIN: &[u8] = b"LX:WITHDRAWAL:v1";
const EXIT_WITHDRAWAL_DOMAIN: &[u8] = b"LXP/v1/emergency-withdrawal-id\x00";
const EXIT_RECIPIENT_DOMAIN: &[u8] = b"LX:SETTLE:RECIPIENT:v1";

/// Why precompile bytes were refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustodyAbiError {
    /// The native value is not a whole number of bank base units.
    WeiRemainder,
    /// `amount * WEI_PER_BASE_UNIT` does not fit the claimed width.
    AmountOverflow,
    /// An evidence argument is empty or above the custody bound.
    EvidenceBounds(&'static str),
    /// A return value or log does not have the declared ABI layout.
    Layout(&'static str),
}

/// A withdrawal exactly as `requestWithdrawal` and `finaliseWithdrawal` take it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalMaterial {
    /// Canonical `LayerX` receipt bytes of the native withdrawal.
    pub receipt: Vec<u8>,
    /// Wire Merkle inclusion proof (`0x4d50` form) under the header's receipt root.
    pub proof: Vec<u8>,
    /// Canonical batch header bytes.
    pub header: Vec<u8>,
    /// Sequencer signature over the batch header.
    pub header_signature: [u8; 64],
}

/// A forced exit exactly as `requestForcedExit` and `executeForcedExit` take it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ForcedExitMaterial {
    /// Native state witness of the account under the latest finalized state root.
    pub witness: Vec<u8>,
    pub batch_number: u64,
    pub account: [u8; 32],
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    /// Account authority signature over [`exit_recipient_message`].
    pub recipient_signature: [u8; 64],
}

/// `ILayerXCustody.Claim`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyClaim {
    pub claim_id: [u8; 32],
    /// 1 withdrawal, 2 forced exit.
    pub kind: u8,
    /// 0 none, 1 pending, 2 paid, 3 cancelled.
    pub status: u8,
    pub nullifier: [u8; 32],
    pub withdrawal_id: [u8; 32],
    pub account: [u8; 32],
    pub asset_id: [u8; 32],
    pub denom: String,
    pub recipient: EvmAddress,
    pub amount: u128,
    pub batch_number: u64,
    pub anchor: [u8; 32],
    pub available_at: u64,
}

/// `ILayerXCustody.Asset`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyAsset {
    pub asset_id: [u8; 32],
    pub denom: String,
    pub pointer: EvmAddress,
    pub enabled: bool,
    pub paused: bool,
    pub minimum_deposit: u128,
    pub custody_cap: u128,
    pub custodied: u128,
    pub released: u128,
    pub pending: u128,
}

/// `ClaimQueued` as emitted by the precompile; `anchor` is the third indexed
/// topic the interface names `checkpointHash`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaimQueued {
    pub claim_id: [u8; 32],
    pub nullifier: [u8; 32],
    pub anchor: [u8; 32],
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    pub amount: u128,
    pub available_at: u64,
}

/// `ClaimFinalised`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClaimFinalised {
    pub claim_id: [u8; 32],
    pub nullifier: [u8; 32],
}

/// `CustodyRelease`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustodyRelease {
    pub claim_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    pub amount: u128,
    pub settlement_module: EvmAddress,
}

/// `EmergencyExitExecuted`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmergencyExitExecuted {
    pub claim_id: [u8; 32],
    pub nullifier: [u8; 32],
    pub anchor: [u8; 32],
    pub account: [u8; 32],
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    pub amount: u128,
}

fn number_word(big_endian: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    for (slot, byte) in word
        .iter_mut()
        .skip(WORD.saturating_sub(big_endian.len()))
        .zip(big_endian)
    {
        *slot = *byte;
    }
    word
}

fn usize_word(value: usize) -> [u8; 32] {
    number_word(&value.to_be_bytes())
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    number_word(&address.bytes())
}

fn padded(bytes: &[u8]) -> Vec<u8> {
    let mut out = usize_word(bytes.len()).to_vec();
    out.extend_from_slice(bytes);
    out.resize(WORD + bytes.len().div_ceil(WORD) * WORD, 0);
    out
}

enum Argument<'a> {
    Word([u8; 32]),
    Bytes(&'a [u8]),
}

fn encode(selector: Option<[u8; 4]>, arguments: &[Argument<'_>]) -> Vec<u8> {
    let mut head = Vec::new();
    let mut tail = Vec::new();
    let head_bytes = arguments.len() * WORD;
    for argument in arguments {
        match argument {
            Argument::Word(word) => head.extend_from_slice(word),
            Argument::Bytes(bytes) => {
                head.extend_from_slice(&usize_word(head_bytes + tail.len()));
                tail.extend_from_slice(&padded(bytes));
            }
        }
    }
    let mut out = selector.map_or_else(Vec::new, |value| value.to_vec());
    out.extend_from_slice(&head);
    out.extend_from_slice(&tail);
    out
}

fn word_call(selector: [u8; 4], words: &[[u8; 32]]) -> Vec<u8> {
    let arguments = words
        .iter()
        .map(|word| Argument::Word(*word))
        .collect::<Vec<_>>();
    encode(Some(selector), &arguments)
}

/// The wei a native deposit of `amount` base units must carry, as a uint256.
///
/// # Errors
/// Refuses an amount whose wei value exceeds u128.
pub fn native_value_wei(amount: u128) -> Result<[u8; 32], CustodyAbiError> {
    amount
        .checked_mul(WEI_PER_BASE_UNIT)
        .map(|wei| number_word(&wei.to_be_bytes()))
        .ok_or(CustodyAbiError::AmountOverflow)
}

/// The bank base units a native transaction value custodies.
///
/// # Errors
/// Refuses a value with a wei remainder, exactly as `deposit()` does.
pub fn base_units_from_wei(value: &[u8; 32]) -> Result<u128, CustodyAbiError> {
    if value[..16] != [0; 16] {
        return Err(CustodyAbiError::AmountOverflow);
    }
    let mut low = [0_u8; 16];
    low.copy_from_slice(&value[16..]);
    let wei = u128::from_be_bytes(low);
    if wei % WEI_PER_BASE_UNIT != 0 {
        return Err(CustodyAbiError::WeiRemainder);
    }
    Ok(wei / WEI_PER_BASE_UNIT)
}

/// `deposit(bytes32 beneficiary)`; the amount travels as the transaction value.
#[must_use]
pub fn deposit_calldata(beneficiary: [u8; 32]) -> Vec<u8> {
    word_call(SELECTOR_DEPOSIT, &[beneficiary])
}

/// `depositToken(address pointer, uint256 amount, bytes32 beneficiary)`.
#[must_use]
pub fn deposit_token_calldata(pointer: EvmAddress, amount: u128, beneficiary: [u8; 32]) -> Vec<u8> {
    word_call(
        SELECTOR_DEPOSIT_TOKEN,
        &[
            address_word(pointer),
            number_word(&amount.to_be_bytes()),
            beneficiary,
        ],
    )
}

fn bounded(name: &'static str, bytes: &[u8]) -> Result<(), CustodyAbiError> {
    if bytes.is_empty() || bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(CustodyAbiError::EvidenceBounds(name));
    }
    Ok(())
}

impl WithdrawalMaterial {
    /// Assembles the material from a verified `LayerX` receipt inclusion as
    /// the node serves it: the receipt, its Merkle path under the header's
    /// receipt root, the canonical batch header and the sequencer signature.
    /// The path is encoded in the wire (`0x4d50`) form the precompile decodes.
    ///
    /// # Errors
    ///
    /// Refuses a path the wire codec does not round-trip and material outside
    /// the custody evidence bounds.
    pub fn from_inclusion(
        receipt: Vec<u8>,
        proof: &layerx_proof::merkle::Proof,
        header: Vec<u8>,
        header_signature: [u8; 64],
    ) -> Result<Self, CustodyAbiError> {
        Self {
            receipt,
            proof: wire_merkle_proof(proof)?,
            header,
            header_signature,
        }
        .validated()
    }

    /// # Errors
    /// Refuses empty or oversized evidence.
    pub fn validated(self) -> Result<Self, CustodyAbiError> {
        bounded("receipt", &self.receipt)?;
        bounded("proof", &self.proof)?;
        bounded("header", &self.header)?;
        if self.header_signature == [0; 64] {
            return Err(CustodyAbiError::EvidenceBounds("header_signature"));
        }
        Ok(self)
    }

    fn call(&self, selector: [u8; 4]) -> Vec<u8> {
        encode(
            Some(selector),
            &[
                Argument::Bytes(&self.receipt),
                Argument::Bytes(&self.proof),
                Argument::Bytes(&self.header),
                Argument::Bytes(&self.header_signature),
            ],
        )
    }

    /// `requestWithdrawal(receipt, proof, header, headerSignature)`.
    #[must_use]
    pub fn request_calldata(&self) -> Vec<u8> {
        self.call(SELECTOR_REQUEST_WITHDRAWAL)
    }

    /// `finaliseWithdrawal(receipt, proof, header, headerSignature)`.
    #[must_use]
    pub fn finalise_calldata(&self) -> Vec<u8> {
        self.call(SELECTOR_FINALISE_WITHDRAWAL)
    }
}

impl ForcedExitMaterial {
    /// # Errors
    /// Refuses empty identifiers, recipient, signature or witness bounds.
    pub fn validated(self) -> Result<Self, CustodyAbiError> {
        bounded("witness", &self.witness)?;
        if self.batch_number == 0 {
            return Err(CustodyAbiError::EvidenceBounds("batch_number"));
        }
        if self.account == [0; 32] {
            return Err(CustodyAbiError::EvidenceBounds("account"));
        }
        if self.asset_id == [0; 32] {
            return Err(CustodyAbiError::EvidenceBounds("asset_id"));
        }
        if self.recipient.bytes() == [0; 20] {
            return Err(CustodyAbiError::EvidenceBounds("recipient"));
        }
        if self.recipient_signature == [0; 64] {
            return Err(CustodyAbiError::EvidenceBounds("recipient_signature"));
        }
        Ok(self)
    }

    fn call(&self, selector: [u8; 4]) -> Vec<u8> {
        encode(
            Some(selector),
            &[
                Argument::Bytes(&self.witness),
                Argument::Word(number_word(&self.batch_number.to_be_bytes())),
                Argument::Word(self.account),
                Argument::Word(self.asset_id),
                Argument::Word(address_word(self.recipient)),
                Argument::Bytes(&self.recipient_signature),
            ],
        )
    }

    /// `requestForcedExit(witness, batchNumber, account, assetId, recipient, recipientSignature)`.
    #[must_use]
    pub fn request_calldata(&self) -> Vec<u8> {
        self.call(SELECTOR_REQUEST_FORCED_EXIT)
    }

    /// `executeForcedExit(witness, batchNumber, account, assetId, recipient, recipientSignature)`.
    #[must_use]
    pub fn execute_calldata(&self) -> Vec<u8> {
        self.call(SELECTOR_EXECUTE_FORCED_EXIT)
    }
}

#[must_use]
pub fn get_claim_calldata(claim_id: [u8; 32]) -> Vec<u8> {
    word_call(SELECTOR_GET_CLAIM, &[claim_id])
}

#[must_use]
pub fn nullifier_status_calldata(nullifier: [u8; 32]) -> Vec<u8> {
    word_call(SELECTOR_NULLIFIER_STATUS, &[nullifier])
}

#[must_use]
pub fn get_asset_calldata(asset_id: [u8; 32]) -> Vec<u8> {
    word_call(SELECTOR_GET_ASSET, &[asset_id])
}

#[must_use]
pub fn exit_eligible_calldata() -> Vec<u8> {
    SELECTOR_EXIT_ELIGIBLE.to_vec()
}

#[must_use]
pub fn native_asset_id_calldata() -> Vec<u8> {
    SELECTOR_NATIVE_ASSET_ID.to_vec()
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn packed_digest(domain: &[u8], words: &[[u8; 32]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(usize_word((words.len() + 1) * WORD));
    for word in words {
        hasher.update(word);
    }
    hasher.update(padded(domain));
    hasher.finalize().into()
}

/// `PaxeerWithdrawalCodec.nullifier`.
#[must_use]
pub fn withdrawal_nullifier(
    network_id: u32,
    withdrawal_id: &[u8; 32],
    account: &[u8; 32],
    asset_id: &[u8; 32],
    amount: u128,
    anchor: &[u8; 32],
) -> [u8; 32] {
    digest(&[
        NULLIFIER_DOMAIN,
        &network_id.to_be_bytes(),
        withdrawal_id,
        account,
        asset_id,
        &amount.to_be_bytes(),
        anchor,
    ])
}

/// The withdrawal identifier a forced exit of `account` at `anchor` must use.
#[must_use]
pub fn exit_withdrawal_id(
    network_id: u32,
    account: &[u8; 32],
    asset_id: &[u8; 32],
    anchor: &[u8; 32],
) -> [u8; 32] {
    digest(&[
        EXIT_WITHDRAWAL_DOMAIN,
        &network_id.to_be_bytes(),
        account,
        asset_id,
        anchor,
    ])
}

/// `sha256(abi.encode("LXP/Paxeer/withdrawal-claim/v1", chainid, 0x…1013, nullifier, recipient))`.
#[must_use]
pub fn withdrawal_claim_id(chain_id: u64, nullifier: [u8; 32], recipient: EvmAddress) -> [u8; 32] {
    packed_digest(
        WITHDRAWAL_CLAIM_DOMAIN,
        &[
            number_word(&chain_id.to_be_bytes()),
            address_word(CUSTODY_PRECOMPILE),
            nullifier,
            address_word(recipient),
        ],
    )
}

/// `sha256(abi.encode("LXP/Paxeer/emergency-exit/v1", chainid, 0x…1013, nullifier))`.
#[must_use]
pub fn exit_claim_id(chain_id: u64, nullifier: [u8; 32]) -> [u8; 32] {
    packed_digest(
        EXIT_CLAIM_DOMAIN,
        &[
            number_word(&chain_id.to_be_bytes()),
            address_word(CUSTODY_PRECOMPILE),
            nullifier,
        ],
    )
}

/// The exact bytes the account authority signs to name a forced-exit
/// recipient: `verify.ExitRecipientMessage`.
#[must_use]
pub fn exit_recipient_message(
    network_id: u32,
    account: &[u8; 32],
    asset_id: &[u8; 32],
    recipient: EvmAddress,
    anchor: &[u8; 32],
) -> Vec<u8> {
    let mut out = EXIT_RECIPIENT_DOMAIN.to_vec();
    out.push(0);
    out.extend_from_slice(&network_id.to_be_bytes());
    out.extend_from_slice(account);
    out.extend_from_slice(asset_id);
    out.extend_from_slice(&recipient.bytes());
    out.extend_from_slice(anchor);
    out
}

struct Words<'a>(&'a [u8]);

impl<'a> Words<'a> {
    fn word(&self, index: usize, what: &'static str) -> Result<[u8; 32], CustodyAbiError> {
        let start = index
            .checked_mul(WORD)
            .ok_or(CustodyAbiError::Layout(what))?;
        self.0
            .get(start..start + WORD)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(CustodyAbiError::Layout(what))
    }

    fn text(&self, index: usize, what: &'static str) -> Result<String, CustodyAbiError> {
        let offset = small(&self.word(index, what)?, what)?;
        if offset % WORD != 0 {
            return Err(CustodyAbiError::Layout(what));
        }
        let length = small(&self.word(offset / WORD, what)?, what)?;
        let start = offset + WORD;
        let bytes = self
            .0
            .get(
                start
                    ..start
                        .checked_add(length)
                        .ok_or(CustodyAbiError::Layout(what))?,
            )
            .ok_or(CustodyAbiError::Layout(what))?;
        String::from_utf8(bytes.to_vec()).map_err(|_| CustodyAbiError::Layout(what))
    }

    fn tuple(bytes: &'a [u8], what: &'static str) -> Result<Self, CustodyAbiError> {
        if bytes.len() % WORD != 0 {
            return Err(CustodyAbiError::Layout(what));
        }
        let offset = small(&Words(bytes).word(0, what)?, what)?;
        if offset != WORD {
            return Err(CustodyAbiError::Layout(what));
        }
        Ok(Self(&bytes[WORD..]))
    }
}

fn small(word: &[u8; 32], what: &'static str) -> Result<usize, CustodyAbiError> {
    usize::try_from(u64_of(word, what)?).map_err(|_| CustodyAbiError::Layout(what))
}

fn u64_of(word: &[u8; 32], what: &'static str) -> Result<u64, CustodyAbiError> {
    if word[..24] != [0; 24] {
        return Err(CustodyAbiError::Layout(what));
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&word[24..]);
    Ok(u64::from_be_bytes(bytes))
}

fn u8_of(word: &[u8; 32], what: &'static str) -> Result<u8, CustodyAbiError> {
    u8::try_from(u64_of(word, what)?).map_err(|_| CustodyAbiError::Layout(what))
}

fn bool_of(word: &[u8; 32], what: &'static str) -> Result<bool, CustodyAbiError> {
    match u64_of(word, what)? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(CustodyAbiError::Layout(what)),
    }
}

fn u128_of(word: &[u8; 32], what: &'static str) -> Result<u128, CustodyAbiError> {
    if word[..16] != [0; 16] {
        return Err(CustodyAbiError::Layout(what));
    }
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&word[16..]);
    Ok(u128::from_be_bytes(bytes))
}

fn address_of(word: &[u8; 32], what: &'static str) -> Result<EvmAddress, CustodyAbiError> {
    if word[..12] != [0; 12] {
        return Err(CustodyAbiError::Layout(what));
    }
    let mut bytes = [0_u8; 20];
    bytes.copy_from_slice(&word[12..]);
    Ok(EvmAddress::new(bytes))
}

/// Decodes the return of `getClaim(bytes32)`.
///
/// # Errors
/// Refuses any layout other than the declared `Claim` tuple.
pub fn decode_claim(bytes: &[u8]) -> Result<CustodyClaim, CustodyAbiError> {
    let words = Words::tuple(bytes, "getClaim")?;
    Ok(CustodyClaim {
        claim_id: words.word(0, "claim.claimId")?,
        kind: u8_of(&words.word(1, "claim.kind")?, "claim.kind")?,
        status: u8_of(&words.word(2, "claim.status")?, "claim.status")?,
        nullifier: words.word(3, "claim.nullifier")?,
        withdrawal_id: words.word(4, "claim.withdrawalId")?,
        account: words.word(5, "claim.account")?,
        asset_id: words.word(6, "claim.assetId")?,
        denom: words.text(7, "claim.denom")?,
        recipient: address_of(&words.word(8, "claim.recipient")?, "claim.recipient")?,
        amount: u128_of(&words.word(9, "claim.amount")?, "claim.amount")?,
        batch_number: u64_of(&words.word(10, "claim.batchNumber")?, "claim.batchNumber")?,
        anchor: words.word(11, "claim.anchor")?,
        available_at: u64_of(&words.word(12, "claim.availableAt")?, "claim.availableAt")?,
    })
}

/// Decodes the return of `getAsset(bytes32)`.
///
/// # Errors
/// Refuses any layout other than the declared `Asset` tuple.
pub fn decode_asset(bytes: &[u8]) -> Result<CustodyAsset, CustodyAbiError> {
    let words = Words::tuple(bytes, "getAsset")?;
    Ok(CustodyAsset {
        asset_id: words.word(0, "asset.assetId")?,
        denom: words.text(1, "asset.denom")?,
        pointer: address_of(&words.word(2, "asset.pointer")?, "asset.pointer")?,
        enabled: bool_of(&words.word(3, "asset.enabled")?, "asset.enabled")?,
        paused: bool_of(&words.word(4, "asset.paused")?, "asset.paused")?,
        minimum_deposit: u128_of(
            &words.word(5, "asset.minimumDeposit")?,
            "asset.minimumDeposit",
        )?,
        custody_cap: u128_of(&words.word(6, "asset.custodyCap")?, "asset.custodyCap")?,
        custodied: u128_of(&words.word(7, "asset.custodied")?, "asset.custodied")?,
        released: u128_of(&words.word(8, "asset.released")?, "asset.released")?,
        pending: u128_of(&words.word(9, "asset.pending")?, "asset.pending")?,
    })
}

/// Decodes a single-word `uint8` view (`nullifierStatus`).
///
/// # Errors
/// Refuses anything but one canonical word.
pub fn decode_status(bytes: &[u8]) -> Result<u8, CustodyAbiError> {
    let word: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CustodyAbiError::Layout("status"))?;
    u8_of(&word, "status")
}

/// Decodes a single-word `bool` view (`exitEligible`).
///
/// # Errors
/// Refuses anything but one canonical boolean word.
pub fn decode_bool(bytes: &[u8]) -> Result<bool, CustodyAbiError> {
    let word: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CustodyAbiError::Layout("bool"))?;
    bool_of(&word, "bool")
}

fn shaped(
    log: &LogRecord,
    topic: [u8; 32],
    topics: usize,
    data: usize,
    what: &'static str,
) -> Result<(), CustodyAbiError> {
    if log.address != CUSTODY_PRECOMPILE
        || log.topics.first() != Some(&topic)
        || log.topics.len() != topics
        || log.data.len() != data
    {
        return Err(CustodyAbiError::Layout(what));
    }
    Ok(())
}

/// # Errors
/// Refuses a log that is not the precompile's `ClaimQueued`.
pub fn decode_claim_queued(log: &LogRecord) -> Result<ClaimQueued, CustodyAbiError> {
    shaped(log, CLAIM_QUEUED_TOPIC, 4, 4 * WORD, "ClaimQueued")?;
    let data = Words(&log.data);
    Ok(ClaimQueued {
        claim_id: log.topics[1],
        nullifier: log.topics[2],
        anchor: log.topics[3],
        asset_id: data.word(0, "ClaimQueued.assetId")?,
        recipient: address_of(
            &data.word(1, "ClaimQueued.recipient")?,
            "ClaimQueued.recipient",
        )?,
        amount: u128_of(&data.word(2, "ClaimQueued.amount")?, "ClaimQueued.amount")?,
        available_at: u64_of(
            &data.word(3, "ClaimQueued.availableAt")?,
            "ClaimQueued.availableAt",
        )?,
    })
}

/// # Errors
/// Refuses a log that is not the precompile's `ClaimFinalised`.
pub fn decode_claim_finalised(log: &LogRecord) -> Result<ClaimFinalised, CustodyAbiError> {
    shaped(log, CLAIM_FINALISED_TOPIC, 3, 0, "ClaimFinalised")?;
    Ok(ClaimFinalised {
        claim_id: log.topics[1],
        nullifier: log.topics[2],
    })
}

/// # Errors
/// Refuses a log that is not the precompile's `CustodyRelease`.
pub fn decode_custody_release(log: &LogRecord) -> Result<CustodyRelease, CustodyAbiError> {
    shaped(log, CUSTODY_RELEASE_TOPIC, 4, 2 * WORD, "CustodyRelease")?;
    let data = Words(&log.data);
    Ok(CustodyRelease {
        claim_id: log.topics[1],
        asset_id: log.topics[2],
        recipient: address_of(&log.topics[3], "CustodyRelease.recipient")?,
        amount: u128_of(
            &data.word(0, "CustodyRelease.amount")?,
            "CustodyRelease.amount",
        )?,
        settlement_module: address_of(
            &data.word(1, "CustodyRelease.settlementModule")?,
            "CustodyRelease.settlementModule",
        )?,
    })
}

/// # Errors
/// Refuses a log that is not the precompile's `EmergencyExitExecuted`.
pub fn decode_emergency_exit_executed(
    log: &LogRecord,
) -> Result<EmergencyExitExecuted, CustodyAbiError> {
    shaped(
        log,
        EMERGENCY_EXIT_EXECUTED_TOPIC,
        4,
        4 * WORD,
        "EmergencyExitExecuted",
    )?;
    let data = Words(&log.data);
    Ok(EmergencyExitExecuted {
        claim_id: log.topics[1],
        nullifier: log.topics[2],
        anchor: log.topics[3],
        account: data.word(0, "EmergencyExitExecuted.account")?,
        asset_id: data.word(1, "EmergencyExitExecuted.assetId")?,
        recipient: address_of(
            &data.word(2, "EmergencyExitExecuted.recipient")?,
            "EmergencyExitExecuted.recipient",
        )?,
        amount: u128_of(
            &data.word(3, "EmergencyExitExecuted.amount")?,
            "EmergencyExitExecuted.amount",
        )?,
    })
}

/// The single log of `topic` the precompile emitted among `logs`.
///
/// # Errors
/// Refuses an absent or repeated event.
pub fn unique_custody_log<'a>(
    logs: &'a [LogRecord],
    topic: [u8; 32],
    what: &'static str,
) -> Result<&'a LogRecord, CustodyAbiError> {
    let mut matches = logs
        .iter()
        .filter(|log| log.address == CUSTODY_PRECOMPILE && log.topics.first() == Some(&topic));
    let first = matches.next().ok_or(CustodyAbiError::Layout(what))?;
    if matches.next().is_some() {
        return Err(CustodyAbiError::Layout(what));
    }
    Ok(first)
}

/// Encodes a Merkle path in the wire (`0x4d50`) form: structure header, leaf
/// index, leaf count, depth and the length-prefixed siblings. The result is
/// accepted only if the canonical wire codec decodes and re-encodes it to the
/// same bytes.
///
/// # Errors
///
/// Refuses a path the wire codec does not round-trip.
pub fn wire_merkle_proof(proof: &layerx_proof::merkle::Proof) -> Result<Vec<u8>, CustodyAbiError> {
    use layerx_wire::encode::Encoder;
    use layerx_wire::receipt::{decode_merkle_proof, encode_merkle_proof};

    let refused = |_| CustodyAbiError::Layout("merkle proof");
    let mut encoder = Encoder::new(4 + 4 + 4 + 1 + 4 + 32 * 32);
    encoder.structure_header(0x4d50).map_err(refused)?;
    encoder.u32(proof.leaf_index()).map_err(refused)?;
    encoder.u32(proof.leaf_count()).map_err(refused)?;
    encoder
        .u8(u8::try_from(proof.siblings().len())
            .map_err(|_| CustodyAbiError::Layout("merkle proof"))?)
        .map_err(refused)?;
    let siblings: Vec<u8> = proof.siblings().iter().flatten().copied().collect();
    encoder.bytes(&siblings, 32 * 32).map_err(refused)?;
    let bytes = encoder.finish();
    let decoded = decode_merkle_proof(&bytes).map_err(refused)?;
    if encode_merkle_proof(&decoded).map_err(refused)? != bytes {
        return Err(CustodyAbiError::Layout("merkle proof"));
    }
    Ok(bytes)
}
