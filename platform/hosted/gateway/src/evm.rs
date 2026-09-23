//! The EVM half of the unified network endpoint: which Paxeer JSON-RPC
//! methods the public endpoint relays, and the ABI of the native precompiles
//! the `px_*` namespace reads.
//!
//! Every selector and layout here is the one declared by the precompiles'
//! own `abi.json` (`precompiles/addr`, `precompiles/layerxcustody`,
//! `precompiles/layerxanchor`, `precompiles/bank`) and is pinned by vectors
//! produced from those files with the go-ethereum ABI codec the precompiles
//! run.

/// Method namespaces the unified endpoint relays to the Paxeer node.
pub const EVM_NAMESPACES: [&str; 3] = ["eth_", "net_", "web3_"];

/// Node-side signing and account methods the public endpoint never relays.
/// A hosted node holds no caller keys, so answering these would either lie or
/// expose the operator's accounts.
pub const NODE_SIGNING_METHODS: [&str; 8] = [
    "eth_accounts",
    "eth_coinbase",
    "eth_sendTransaction",
    "eth_sign",
    "eth_signTransaction",
    "eth_signTypedData",
    "eth_signTypedData_v4",
    "eth_mining",
];

/// Subscription methods that need a live upstream socket rather than the
/// request/response relay.
pub const SUBSCRIPTION_METHODS: [&str; 2] = ["eth_subscribe", "eth_unsubscribe"];

const MAX_METHOD_BYTES: usize = 64;

/// `0x0000000000000000000000000000000000001001`.
pub const BANK_PRECOMPILE: [u8; 20] = precompile(0x10, 0x01);
/// `0x0000000000000000000000000000000000001004`.
pub const ADDR_PRECOMPILE: [u8; 20] = precompile(0x10, 0x04);
/// `0x0000000000000000000000000000000000001013`.
pub const CUSTODY_PRECOMPILE: [u8; 20] = precompile(0x10, 0x13);
/// `0x0000000000000000000000000000000000001014`.
pub const ANCHOR_PRECOMPILE: [u8; 20] = precompile(0x10, 0x14);
/// `0x0000000000000000000000000000000000001015`, live from the Paxeer X fork.
pub const EXCHANGE_PRECOMPILE: [u8; 20] = precompile(0x10, 0x15);
/// `0x0000000000000000000000000000000000001016`, live from the Paxeer X fork.
pub const BRIDGE_PRECOMPILE: [u8; 20] = precompile(0x10, 0x16);
/// `0x0000000000000000000000000000000000001017`, live from the Paxeer X fork.
pub const LAUNCHPAD_PRECOMPILE: [u8; 20] = precompile(0x10, 0x17);

const fn precompile(high: u8, low: u8) -> [u8; 20] {
    let mut address = [0_u8; 20];
    address[18] = high;
    address[19] = low;
    address
}

/// `getUnifiedAccount(address)`.
pub const SELECTOR_GET_UNIFIED_ACCOUNT: [u8; 4] = [0x35, 0x7f, 0xee, 0xd6];
/// `getLayerXDid(address)`.
pub const SELECTOR_GET_LAYERX_DID: [u8; 4] = [0xd3, 0x60, 0x0b, 0x24];
/// `getEvmAddrByLayerX(bytes32)`.
pub const SELECTOR_GET_EVM_ADDR_BY_LAYERX: [u8; 4] = [0xc4, 0x13, 0xba, 0xd3];
/// `layerXBindNonce(address)`.
pub const SELECTOR_LAYERX_BIND_NONCE: [u8; 4] = [0xce, 0xdd, 0x9b, 0xa2];
/// `getAsset(bytes32)`.
pub const SELECTOR_GET_ASSET: [u8; 4] = [0x2c, 0xc3, 0xce, 0x80];
/// `assetByPointer(address)`.
pub const SELECTOR_ASSET_BY_POINTER: [u8; 4] = [0xca, 0x65, 0x02, 0x1c];
/// `nativeAssetId()`.
pub const SELECTOR_NATIVE_ASSET_ID: [u8; 4] = [0xaa, 0xfc, 0xde, 0x84];
/// `latestFinalized()`.
pub const SELECTOR_LATEST_FINALIZED: [u8; 4] = [0x6c, 0xdd, 0x45, 0xae];
/// `statusOf(uint64)`.
pub const SELECTOR_STATUS_OF: [u8; 4] = [0x4e, 0xb4, 0x77, 0x10];
/// `checkpoint(uint64)`.
pub const SELECTOR_CHECKPOINT: [u8; 4] = [0x2d, 0x58, 0x8b, 0x18];
/// `balance(address,string)`.
pub const SELECTOR_BANK_BALANCE: [u8; 4] = [0x16, 0xca, 0xde, 0xab];

/// Largest ABI string the endpoint decodes from a precompile answer.
pub const MAX_ABI_STRING_BYTES: usize = 4096;

/// True when the method belongs to a relayed EVM namespace, whether or not
/// the endpoint is willing to relay it.
#[must_use]
pub fn is_evm_namespace(method: &str) -> bool {
    EVM_NAMESPACES
        .iter()
        .any(|namespace| method.starts_with(namespace))
}

/// True when the unified endpoint forwards the method to the Paxeer node.
/// Node-side signing methods and methods outside the wire charset are refused
/// here rather than at the node.
#[must_use]
pub fn relayable_method(method: &str) -> bool {
    if method.len() > MAX_METHOD_BYTES
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || NODE_SIGNING_METHODS.contains(&method)
        || SUBSCRIPTION_METHODS.contains(&method)
    {
        return false;
    }
    is_evm_namespace(method)
}

/// Why precompile bytes were refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiError {
    /// The answer is shorter than the declared layout.
    Truncated,
    /// A word carries bits the declared type cannot hold.
    Width,
    /// A dynamic offset points outside the answer.
    Offset,
    /// A string is not valid UTF-8 or exceeds its bound.
    Text,
}

/// `IAddr.getUnifiedAccount`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedAccount {
    pub evm: [u8; 20],
    pub pax_address: String,
    pub did_public_key: [u8; 32],
    pub layerx_main_account_id: [u8; 32],
}

/// `ILayerXCustody.Asset`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyAsset {
    pub asset_id: [u8; 32],
    pub denom: String,
    pub pointer: [u8; 20],
    pub enabled: bool,
    pub paused: bool,
    pub minimum_deposit: [u8; 32],
    pub custody_cap: [u8; 32],
    pub custodied: [u8; 32],
    pub released: [u8; 32],
    pub pending: [u8; 32],
}

/// `ILayerXAnchor.Checkpoint`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorCheckpoint {
    pub batch_number: u64,
    pub checkpoint_id: [u8; 32],
    pub header_digest: [u8; 32],
    pub epoch: u64,
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub previous_state_root: [u8; 32],
    pub state_root: [u8; 32],
    pub receipt_root: [u8; 32],
    pub data_availability_root: [u8; 32],
    pub sequencer_id: [u8; 32],
    pub timestamp_ms: u64,
    pub status: u8,
    pub signers: u8,
    pub availability_mask: u8,
    pub open_challenges: u32,
    pub submitted_height: u64,
    pub finalized_height: u64,
}

/// The anchor's status ladder as `ILayerXAnchor` declares it.
#[must_use]
pub fn anchor_status_name(status: u8) -> &'static str {
    match status {
        1 => "submitted",
        2 => "final",
        _ => "unknown",
    }
}

fn calldata_head(selector: [u8; 4], words: usize) -> Vec<u8> {
    let mut calldata = Vec::with_capacity(4 + words * 32);
    calldata.extend_from_slice(&selector);
    calldata
}

/// `selector()` with no arguments.
#[must_use]
pub fn calldata_empty(selector: [u8; 4]) -> Vec<u8> {
    calldata_head(selector, 0)
}

/// `selector(address)`.
#[must_use]
pub fn calldata_address(selector: [u8; 4], address: &[u8; 20]) -> Vec<u8> {
    let mut calldata = calldata_head(selector, 1);
    calldata.extend_from_slice(&[0_u8; 12]);
    calldata.extend_from_slice(address);
    calldata
}

/// `selector(bytes32)`.
#[must_use]
pub fn calldata_word(selector: [u8; 4], word: &[u8; 32]) -> Vec<u8> {
    let mut calldata = calldata_head(selector, 1);
    calldata.extend_from_slice(word);
    calldata
}

/// `selector(uint64)`.
#[must_use]
pub fn calldata_u64(selector: [u8; 4], value: u64) -> Vec<u8> {
    let mut calldata = calldata_head(selector, 1);
    calldata.extend_from_slice(&[0_u8; 24]);
    calldata.extend_from_slice(&value.to_be_bytes());
    calldata
}

/// `selector(address,string)`.
#[must_use]
pub fn calldata_address_string(selector: [u8; 4], address: &[u8; 20], text: &str) -> Vec<u8> {
    let mut calldata = calldata_head(selector, 2);
    calldata.extend_from_slice(&[0_u8; 12]);
    calldata.extend_from_slice(address);
    calldata.extend_from_slice(&word_u64(64));
    calldata.extend_from_slice(&word_u64(u64::try_from(text.len()).unwrap_or(u64::MAX)));
    calldata.extend_from_slice(text.as_bytes());
    let remainder = text.len() % 32;
    if remainder != 0 {
        calldata.extend(std::iter::repeat_n(0_u8, 32 - remainder));
    }
    calldata
}

fn word_u64(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

/// One ABI answer read word by word.
pub struct Answer<'a> {
    bytes: &'a [u8],
}

impl<'a> Answer<'a> {
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// # Errors
    /// Refuses an answer shorter than the requested word.
    pub fn word(&self, index: usize) -> Result<[u8; 32], AbiError> {
        let start = index.checked_mul(32).ok_or(AbiError::Offset)?;
        let end = start.checked_add(32).ok_or(AbiError::Offset)?;
        let mut word = [0_u8; 32];
        word.copy_from_slice(self.bytes.get(start..end).ok_or(AbiError::Truncated)?);
        Ok(word)
    }

    /// # Errors
    /// Refuses a word whose high bytes are set.
    pub fn address(&self, index: usize) -> Result<[u8; 20], AbiError> {
        let word = self.word(index)?;
        if word[..12].iter().any(|byte| *byte != 0) {
            return Err(AbiError::Width);
        }
        let mut address = [0_u8; 20];
        address.copy_from_slice(&word[12..]);
        Ok(address)
    }

    /// # Errors
    /// Refuses a word wider than 64 bits.
    pub fn u64(&self, index: usize) -> Result<u64, AbiError> {
        let word = self.word(index)?;
        if word[..24].iter().any(|byte| *byte != 0) {
            return Err(AbiError::Width);
        }
        let mut value = [0_u8; 8];
        value.copy_from_slice(&word[24..]);
        Ok(u64::from_be_bytes(value))
    }

    /// # Errors
    /// Refuses a word wider than 32 bits.
    pub fn u32(&self, index: usize) -> Result<u32, AbiError> {
        u32::try_from(self.u64(index)?).map_err(|_| AbiError::Width)
    }

    /// # Errors
    /// Refuses a word wider than 8 bits.
    pub fn u8(&self, index: usize) -> Result<u8, AbiError> {
        u8::try_from(self.u64(index)?).map_err(|_| AbiError::Width)
    }

    /// # Errors
    /// Refuses a word that is neither zero nor one.
    pub fn bool(&self, index: usize) -> Result<bool, AbiError> {
        match self.u64(index)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(AbiError::Width),
        }
    }

    /// Reads a dynamic string whose offset word sits at `index` and whose
    /// offset is measured from `base` bytes into the answer.
    ///
    /// # Errors
    /// Refuses offsets outside the answer and text that is not UTF-8 or
    /// exceeds [`MAX_ABI_STRING_BYTES`].
    pub fn string(&self, index: usize, base: usize) -> Result<String, AbiError> {
        let offset = usize::try_from(self.u64(index)?).map_err(|_| AbiError::Offset)?;
        let start = base.checked_add(offset).ok_or(AbiError::Offset)?;
        if start % 32 != 0 {
            return Err(AbiError::Offset);
        }
        let length =
            usize::try_from(Self::new(self.bytes.get(start..).ok_or(AbiError::Offset)?).u64(0)?)
                .map_err(|_| AbiError::Offset)?;
        if length > MAX_ABI_STRING_BYTES {
            return Err(AbiError::Text);
        }
        let text_start = start.checked_add(32).ok_or(AbiError::Offset)?;
        let text_end = text_start.checked_add(length).ok_or(AbiError::Offset)?;
        let padded = text_end
            .checked_add((32 - length % 32) % 32)
            .ok_or(AbiError::Offset)?;
        if self.bytes.len() < padded {
            return Err(AbiError::Truncated);
        }
        let text = self
            .bytes
            .get(text_start..text_end)
            .ok_or(AbiError::Truncated)?;
        String::from_utf8(text.to_vec()).map_err(|_| AbiError::Text)
    }
}

/// Decodes `getUnifiedAccount(address)`.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_unified_account(answer: &[u8]) -> Result<UnifiedAccount, AbiError> {
    let answer = Answer::new(answer);
    Ok(UnifiedAccount {
        evm: answer.address(0)?,
        pax_address: answer.string(1, 0)?,
        did_public_key: answer.word(2)?,
        layerx_main_account_id: answer.word(3)?,
    })
}

/// Decodes `getLayerXDid(address)` into the DID public key and rendered DID.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_layerx_did(answer: &[u8]) -> Result<([u8; 32], String), AbiError> {
    let answer = Answer::new(answer);
    Ok((answer.word(0)?, answer.string(1, 0)?))
}

/// Decodes one `address` answer.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_address(answer: &[u8]) -> Result<[u8; 20], AbiError> {
    Answer::new(answer).address(0)
}

/// Decodes one `bytes32` answer.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_word(answer: &[u8]) -> Result<[u8; 32], AbiError> {
    Answer::new(answer).word(0)
}

/// Decodes one `uint64` answer.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_u64(answer: &[u8]) -> Result<u64, AbiError> {
    Answer::new(answer).u64(0)
}

/// Decodes one `uint8` answer.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_u8(answer: &[u8]) -> Result<u8, AbiError> {
    Answer::new(answer).u8(0)
}

/// Decodes `latestFinalized()`.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_latest_finalized(answer: &[u8]) -> Result<(u64, bool), AbiError> {
    let answer = Answer::new(answer);
    Ok((answer.u64(0)?, answer.bool(1)?))
}

/// Decodes `getAsset(bytes32)`.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_custody_asset(answer: &[u8]) -> Result<CustodyAsset, AbiError> {
    let outer = Answer::new(answer);
    let base = usize::try_from(outer.u64(0)?).map_err(|_| AbiError::Offset)?;
    if base % 32 != 0 || base >= answer.len() {
        return Err(AbiError::Offset);
    }
    let tuple = Answer::new(answer.get(base..).ok_or(AbiError::Offset)?);
    Ok(CustodyAsset {
        asset_id: tuple.word(0)?,
        denom: tuple.string(1, 0)?,
        pointer: tuple.address(2)?,
        enabled: tuple.bool(3)?,
        paused: tuple.bool(4)?,
        minimum_deposit: tuple.word(5)?,
        custody_cap: tuple.word(6)?,
        custodied: tuple.word(7)?,
        released: tuple.word(8)?,
        pending: tuple.word(9)?,
    })
}

/// Decodes `checkpoint(uint64)`.
///
/// # Errors
/// Refuses answers that do not carry the declared layout.
pub fn decode_checkpoint(answer: &[u8]) -> Result<AnchorCheckpoint, AbiError> {
    let answer = Answer::new(answer);
    Ok(AnchorCheckpoint {
        batch_number: answer.u64(0)?,
        checkpoint_id: answer.word(1)?,
        header_digest: answer.word(2)?,
        epoch: answer.u64(3)?,
        first_sequence: answer.u64(4)?,
        last_sequence: answer.u64(5)?,
        previous_state_root: answer.word(6)?,
        state_root: answer.word(7)?,
        receipt_root: answer.word(8)?,
        data_availability_root: answer.word(9)?,
        sequencer_id: answer.word(10)?,
        timestamp_ms: answer.u64(11)?,
        status: answer.u8(12)?,
        signers: answer.u8(13)?,
        availability_mask: answer.u8(14)?,
        open_challenges: answer.u32(15)?,
        submitted_height: answer.u64(16)?,
        finalized_height: answer.u64(17)?,
    })
}

/// Renders one 256-bit word as its exact decimal value.
#[must_use]
pub fn uint256_decimal(word: &[u8; 32]) -> String {
    let mut digits: Vec<u8> = Vec::with_capacity(78);
    let mut value = *word;
    while value.iter().any(|byte| *byte != 0) {
        let mut remainder = 0_u16;
        for byte in &mut value {
            let current = (remainder << 8) | u16::from(*byte);
            *byte = u8::try_from(current / 10).unwrap_or(0);
            remainder = current % 10;
        }
        digits.push(u8::try_from(remainder).unwrap_or(0) + b'0');
    }
    if digits.is_empty() {
        return "0".to_owned();
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_else(|_| "0".to_owned())
}

/// Renders one 256-bit word as the minimal `0x` JSON-RPC quantity.
#[must_use]
pub fn uint256_quantity(word: &[u8; 32]) -> String {
    let mut text = String::with_capacity(66);
    text.push_str("0x");
    let start = word.iter().position(|byte| *byte != 0);
    match start {
        None => text.push('0'),
        Some(start) => {
            for (index, byte) in word[start..].iter().enumerate() {
                if index == 0 && *byte < 16 {
                    text.push(char::from_digit(u32::from(*byte), 16).unwrap_or('0'));
                } else {
                    text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
                    text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
                }
            }
        }
    }
    text
}

/// Renders 20 address bytes as a lowercase `0x` address.
#[must_use]
pub fn address_hex(address: &[u8; 20]) -> String {
    let mut text = String::with_capacity(42);
    text.push_str("0x");
    for byte in address {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    text
}

/// Parses a `0x`-prefixed 20-byte address, or `None` when the text is not
/// exactly 40 hexadecimal characters.
#[must_use]
pub fn parse_address(value: &str) -> Option<[u8; 20]> {
    let digits = value.strip_prefix("0x").unwrap_or(value);
    if digits.len() != 40 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut address = [0_u8; 20];
    for (index, byte) in address.iter_mut().enumerate() {
        let pair = digits.get(index * 2..index * 2 + 2)?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vectors() -> Value {
        serde_json::from_slice(include_bytes!("../tests/fixtures/paxeer-abi-vectors.json"))
            .unwrap_or_else(|error| panic!("{error}"))
    }

    fn bytes(text: &str) -> Vec<u8> {
        (0..text.len() / 2)
            .map(|index| {
                u8::from_str_radix(&text[index * 2..index * 2 + 2], 16)
                    .unwrap_or_else(|error| panic!("{error}"))
            })
            .collect()
    }

    fn case(name: &str) -> (Vec<u8>, Vec<u8>, Value) {
        let document = vectors();
        let cases = document["cases"]
            .as_array()
            .unwrap_or_else(|| panic!("cases missing"))
            .clone();
        let found = cases
            .into_iter()
            .find(|case| case["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        (
            bytes(found["calldata"].as_str().unwrap_or_default()),
            bytes(found["result"].as_str().unwrap_or_default()),
            found["out"].clone(),
        )
    }

    fn word32(text: &str) -> [u8; 32] {
        let mut word = [0_u8; 32];
        word.copy_from_slice(&bytes(text));
        word
    }

    #[test]
    fn precompile_addresses_and_selectors_match_the_generated_vectors() {
        let document = vectors();
        assert_eq!(
            document["precompiles"]["bank"],
            Value::String(address_hex(&BANK_PRECOMPILE))
        );
        assert_eq!(
            document["precompiles"]["addr"],
            Value::String(address_hex(&ADDR_PRECOMPILE))
        );
        assert_eq!(
            document["precompiles"]["custody"],
            Value::String(address_hex(&CUSTODY_PRECOMPILE))
        );
        assert_eq!(
            document["precompiles"]["anchor"],
            Value::String(address_hex(&ANCHOR_PRECOMPILE))
        );
        for (name, selector) in [
            ("addr.getUnifiedAccount", SELECTOR_GET_UNIFIED_ACCOUNT),
            ("addr.getLayerXDid", SELECTOR_GET_LAYERX_DID),
            ("addr.getEvmAddrByLayerX", SELECTOR_GET_EVM_ADDR_BY_LAYERX),
            ("addr.layerXBindNonce", SELECTOR_LAYERX_BIND_NONCE),
            ("custody.getAsset", SELECTOR_GET_ASSET),
            ("custody.assetByPointer", SELECTOR_ASSET_BY_POINTER),
            ("custody.nativeAssetId", SELECTOR_NATIVE_ASSET_ID),
            ("anchor.latestFinalized", SELECTOR_LATEST_FINALIZED),
            ("anchor.statusOf", SELECTOR_STATUS_OF),
            ("anchor.checkpoint", SELECTOR_CHECKPOINT),
            ("bank.balance", SELECTOR_BANK_BALANCE),
        ] {
            let (calldata, _, _) = case(name);
            assert_eq!(calldata.get(..4), Some(selector.as_slice()), "{name}");
        }
    }

    #[test]
    fn calldata_matches_the_go_ethereum_encoding() {
        let account = parse_address("0x102132435465768798a9bacbdcedfe0f1e2d3c4b")
            .unwrap_or_else(|| panic!("address"));
        let pointer = parse_address("0x00000000000000000000000000000000000000a1")
            .unwrap_or_else(|| panic!("address"));
        let did = [0x61_u8; 32];
        let asset = [0x2b_u8; 32];
        for (name, calldata) in [
            (
                "addr.getUnifiedAccount",
                calldata_address(SELECTOR_GET_UNIFIED_ACCOUNT, &account),
            ),
            (
                "addr.getLayerXDid",
                calldata_address(SELECTOR_GET_LAYERX_DID, &account),
            ),
            (
                "addr.getEvmAddrByLayerX",
                calldata_word(SELECTOR_GET_EVM_ADDR_BY_LAYERX, &did),
            ),
            (
                "addr.layerXBindNonce",
                calldata_address(SELECTOR_LAYERX_BIND_NONCE, &account),
            ),
            (
                "custody.getAsset",
                calldata_word(SELECTOR_GET_ASSET, &asset),
            ),
            (
                "custody.assetByPointer",
                calldata_address(SELECTOR_ASSET_BY_POINTER, &pointer),
            ),
            (
                "custody.nativeAssetId",
                calldata_empty(SELECTOR_NATIVE_ASSET_ID),
            ),
            (
                "anchor.latestFinalized",
                calldata_empty(SELECTOR_LATEST_FINALIZED),
            ),
            ("anchor.statusOf", calldata_u64(SELECTOR_STATUS_OF, 918)),
            ("anchor.checkpoint", calldata_u64(SELECTOR_CHECKPOINT, 918)),
            (
                "bank.balance",
                calldata_address_string(SELECTOR_BANK_BALANCE, &account, "ulxp"),
            ),
        ] {
            let (expected, _, _) = case(name);
            assert_eq!(calldata, expected, "{name}");
        }
    }

    #[test]
    fn account_answers_decode_exactly() {
        let (_, result, out) = case("addr.getUnifiedAccount");
        let account = decode_unified_account(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(address_hex(&account.evm), out["evm"]);
        assert_eq!(Value::String(account.pax_address), out["paxAddr"]);
        assert_eq!(
            Value::String(hex_lower(&account.did_public_key)),
            out["didPublicKey"]
        );
        assert_eq!(
            Value::String(hex_lower(&account.layerx_main_account_id)),
            out["layerxMainAccountId"]
        );
        let (_, result, out) = case("addr.getLayerXDid");
        let (key, did) = decode_layerx_did(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(hex_lower(&key)), out["didPublicKey"]);
        assert_eq!(Value::String(did), out["did"]);
        let (_, result, out) = case("addr.getEvmAddrByLayerX");
        let address = decode_address(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(address_hex(&address), out["evmAddr"]);
        let (_, result, out) = case("addr.layerXBindNonce");
        let nonce = decode_u64(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(nonce.to_string()), out["nonce"]);
    }

    #[test]
    fn asset_and_anchor_answers_decode_exactly() {
        let (_, result, out) = case("custody.getAsset");
        let asset = decode_custody_asset(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(hex_lower(&asset.asset_id)), out["assetId"]);
        assert_eq!(Value::String(asset.denom), out["denom"]);
        assert_eq!(address_hex(&asset.pointer), out["pointer"]);
        assert_eq!(Value::Bool(asset.enabled), out["enabled"]);
        assert_eq!(Value::Bool(asset.paused), out["paused"]);
        assert_eq!(
            Value::String(uint256_decimal(&asset.minimum_deposit)),
            out["minimumDeposit"]
        );
        assert_eq!(
            Value::String(uint256_decimal(&asset.custody_cap)),
            out["custodyCap"]
        );
        assert_eq!(
            Value::String(uint256_decimal(&asset.custodied)),
            out["custodied"]
        );
        assert_eq!(
            Value::String(uint256_decimal(&asset.released)),
            out["released"]
        );
        assert_eq!(
            Value::String(uint256_decimal(&asset.pending)),
            out["pending"]
        );
        let (_, result, out) = case("custody.assetByPointer");
        assert_eq!(
            Value::String(hex_lower(
                &decode_word(&result).unwrap_or_else(|error| panic!("{error:?}"))
            )),
            out["assetId"]
        );
        let (_, result, out) = case("anchor.latestFinalized");
        let (batch, exists) =
            decode_latest_finalized(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(batch.to_string()), out["batchNumber"]);
        assert_eq!(Value::Bool(exists), out["exists"]);
        let (_, result, out) = case("anchor.statusOf");
        let status = decode_u8(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(status.to_string()), out["status"]);
        assert_eq!(anchor_status_name(status), "final");
        let (_, result, out) = case("anchor.checkpoint");
        let checkpoint = decode_checkpoint(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            Value::String(checkpoint.batch_number.to_string()),
            out["batchNumber"]
        );
        assert_eq!(
            Value::String(hex_lower(&checkpoint.state_root)),
            out["stateRoot"]
        );
        assert_eq!(
            Value::String(hex_lower(&checkpoint.receipt_root)),
            out["receiptRoot"]
        );
        assert_eq!(
            Value::String(checkpoint.timestamp_ms.to_string()),
            out["timestampMs"]
        );
        assert_eq!(Value::String(checkpoint.status.to_string()), out["status"]);
        assert_eq!(
            Value::String(checkpoint.open_challenges.to_string()),
            out["openChallenges"]
        );
        assert_eq!(
            Value::String(checkpoint.finalized_height.to_string()),
            out["finalizedHeight"]
        );
        let (_, result, out) = case("bank.balance");
        let balance = decode_word(&result).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(Value::String(uint256_decimal(&balance)), out["amount"]);
    }

    fn hex_lower(bytes: &[u8]) -> String {
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
        }
        text
    }

    #[test]
    fn quantities_and_addresses_render_without_loss() {
        assert_eq!(uint256_decimal(&[0_u8; 32]), "0");
        assert_eq!(uint256_quantity(&[0_u8; 32]), "0x0");
        assert_eq!(uint256_decimal(&word32(&"ff".repeat(32))), u256_max());
        assert_eq!(
            uint256_quantity(&word32(&"ff".repeat(32))),
            format!("0x{}", "ff".repeat(32))
        );
        let mut one = [0_u8; 32];
        one[31] = 1;
        assert_eq!(uint256_quantity(&one), "0x1");
        assert_eq!(uint256_decimal(&one), "1");
        let mut sixteen = [0_u8; 32];
        sixteen[31] = 16;
        assert_eq!(uint256_quantity(&sixteen), "0x10");
        assert_eq!(
            parse_address("0x00000000000000000000000000000000000000a1"),
            Some([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xa1])
        );
        for invalid in ["0x", "0xzz", "00", &"ab".repeat(21)] {
            assert_eq!(parse_address(invalid), None);
        }
    }

    fn u256_max() -> String {
        "115792089237316195423570985008687907853269984665640564039457584007913129639935".to_owned()
    }

    #[test]
    fn relayed_methods_exclude_node_signing_and_subscriptions() {
        for method in [
            "eth_blockNumber",
            "eth_call",
            "eth_sendRawTransaction",
            "eth_getBalance",
            "net_version",
            "web3_clientVersion",
        ] {
            assert!(relayable_method(method), "{method}");
        }
        for method in [
            "eth_accounts",
            "eth_coinbase",
            "eth_sendTransaction",
            "eth_sign",
            "eth_signTransaction",
            "eth_signTypedData",
            "eth_signTypedData_v4",
            "eth_mining",
            "eth_subscribe",
            "eth_unsubscribe",
            "lx_getNodeInfo",
            "px_getNetwork",
            "eth_call ",
            "eth_",
        ] {
            assert_eq!(relayable_method(method), method == "eth_", "{method}");
        }
        assert!(!relayable_method(&format!("eth_{}", "a".repeat(64))));
        assert!(is_evm_namespace("eth_subscribe"));
        assert!(!is_evm_namespace("px_getAccount"));
    }

    #[test]
    fn malformed_answers_are_refused() {
        assert_eq!(decode_u64(&[0_u8; 31]), Err(AbiError::Truncated));
        assert_eq!(decode_u64(&[0xff_u8; 32]), Err(AbiError::Width));
        assert_eq!(decode_address(&[0xff_u8; 32]), Err(AbiError::Width));
        assert_eq!(decode_custody_asset(&[0_u8; 32]), Err(AbiError::Truncated));
        let mut outside = [0_u8; 32];
        outside[31] = 64;
        assert_eq!(decode_custody_asset(&outside), Err(AbiError::Offset));
        let mut unaligned = [0_u8; 64];
        unaligned[31] = 8;
        assert_eq!(decode_custody_asset(&unaligned), Err(AbiError::Offset));
        let mut latest = [0_u8; 64];
        latest[63] = 2;
        assert_eq!(decode_latest_finalized(&latest), Err(AbiError::Width));
        let (_, result, _) = case("addr.getUnifiedAccount");
        assert_eq!(
            decode_unified_account(&result[..result.len() - 32]),
            Err(AbiError::Truncated)
        );
    }
}
