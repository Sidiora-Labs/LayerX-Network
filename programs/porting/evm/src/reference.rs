//! The ported reference contract: a paid-membership lock in the shape Unlock
//! Protocol's `PublicLock` uses, carried onto the programs ABI.
//!
//! The contract was chosen because every part of it lands on a different edge
//! of the port. Its key store is a `mapping(address => uint256)` indexed only
//! by `msg.sender`, so it collapses onto the per-principal namespace. Its
//! purchase is funded by the caller in the same call, so it survives the
//! monetary law intact. Its events are the ones a real indexer already watches,
//! so their topics have to stay byte-identical. And its expiry is a timestamp,
//! which the deterministic runtime does not have, so it becomes a period count.

use std::collections::BTreeMap;

use layerx_programs::hex;
use layerx_programs_runtime::{Capability, CapabilitySet, ProgramId, ABI_MODULE};

use crate::error::PortRefusal;
use crate::layout::caller_indexed_key;
use crate::monetary::{ProgramAccountTransferPlan, Transfer402Plan};
use crate::semantics::EventAbi;
use crate::value::Word;
use crate::wasm::{
    Code, ModuleBuilder, I32, I32_ADD, I32_EQ, I32_EQZ, I32_GT_S, I32_LOAD, I32_LOAD8_U, I32_LT_S,
    I32_NE, I32_STORE8, I32_WRAP_I64, I64, I64_ADD, I64_EQZ, I64_EXTEND_I32_U, I64_GT_S, I64_LOAD,
    I64_LT_S, I64_MUL, I64_NE, I64_OR, I64_SHL, I64_SHR_U, I64_STORE, IF, RETURN, VOID_BLOCK,
};

/// The contract name carried by the published descriptor.
pub const CONTRACT_NAME: &str = "PublicLock";
/// Archive path of the Solidity the port reproduces.
pub const SOURCE_PATH: &str = "contracts/PublicLock.sol";
/// Archive path of the canonical port descriptor, which is the build input.
pub const DESCRIPTOR_PATH: &str = "port/public-lock.port";
/// Archive path of the pinned toolchain manifest.
pub const TOOLCHAIN_PATH: &str = "toolchain/porting-evm.toolchain";
/// Archive path of the pinned dependency lock.
pub const DEPENDENCY_LOCK_PATH: &str = "toolchain/porting-evm.lock";
/// Path of the artifact the pinned build produces.
pub const ARTIFACT_PATH: &str = "build/public-lock.wasm";
/// The pinned build command, whose last word names the descriptor to compile.
pub const BUILD_COMMAND: &str = "layerx-porting-evm emit port/public-lock.port";

/// Maps `address(this).balance` custody onto one public, rederivable LayerX account.
///
/// # Errors
///
/// Refuses any owner, seed, source or monetary field that does not form the
/// exact bounded contract payout.
pub fn contract_funded_payout(
    owner_program: ProgramId,
    seed: &[u8],
    derived_account: [u8; 32],
    asset: [u8; 32],
    recipient: [u8; 32],
    amount: u128,
) -> Result<ProgramAccountTransferPlan, PortRefusal> {
    ProgramAccountTransferPlan::new(
        owner_program,
        seed,
        derived_account,
        asset,
        recipient,
        amount,
    )
}

/// Canonical signature of the mint event, kept so an existing `ERC-721`
/// indexer's `topic0` filter still matches the ported program.
pub const TRANSFER_EVENT: &str = "Transfer(address,address,uint256)";
/// Canonical signature of the key-extension event.
pub const KEY_EXTENDED_EVENT: &str = "KeyExtended(uint256,uint256)";
/// Canonical signature of the ported purchase entry point.
pub const PURCHASE_METHOD: &str = "purchase(uint256)";
/// Canonical signature of the caller-scoped validity query.
pub const HAS_VALID_KEY_METHOD: &str = "getHasValidKey()";
/// Canonical signature of the caller-scoped remaining-period query.
pub const REMAINING_PERIODS_METHOD: &str = "remainingPeriods()";

/// Export invoked by an activity to buy or extend a key.
pub const PURCHASE_EXPORT: &str = "purchase";
/// Export answering whether the invoking principal holds a live key.
pub const HAS_VALID_KEY_EXPORT: &str = "getHasValidKey";
/// Export answering how many periods the invoking principal holds.
pub const REMAINING_PERIODS_EXPORT: &str = "remainingPeriods";
/// Export a calling program uses to reserve the calldata region.
pub const RESERVE_EXPORT: &str = "layerx_reserve";
/// Export a calling program enters with `EVM`-shaped calldata.
pub const CALL_ENTRY_EXPORT: &str = "layerx_call";
/// Export name of the linear memory the host reads guest buffers from.
pub const MEMORY_EXPORT: &str = "memory";

/// Declaration-order slot of the key store in the Solidity source.
pub const KEYS_SLOT: u64 = 0;
/// The single membership token identifier the lock mints.
pub const TOKEN_ID: u64 = 1;
/// Upper bound on periods bought in one call, itself bounded so the emitted
/// multiplication cannot overflow the signed 64-bit domain.
pub const MAX_PERIODS_BOUND: u64 = 4_096;
/// Upper bound on the periods one key may hold, bounded so the composition
/// entry point can return the period count as a non-negative result code.
pub const MAX_TOTAL_PERIODS_BOUND: u64 = 2_147_483_647;

const MEMORY_PAGES: u32 = 1;
const KEY_POINTER: u32 = 0;
const ASSET_POINTER: u32 = 32;
const BENEFICIARY_POINTER: u32 = 64;
const TRANSFER_TOPIC_POINTER: u32 = 96;
const EXTENDED_TOPIC_POINTER: u32 = 128;
const TOKEN_POINTER: u32 = 160;
const VALUE_POINTER: u32 = 256;
const EVENT_POINTER: u32 = 320;
const INPUT_POINTER: u32 = 1_024;
const INPUT_CAPACITY: i32 = 256;
const WORD_LENGTH: i32 = 32;
const STORED_LENGTH: i32 = 33;
const PURCHASE_CALLDATA: i32 = 36;
const SELECTOR_CALLDATA: i32 = 4;
const DESCRIPTOR_VERSION: &str = "1";
const DESCRIPTOR_KEYS: [&str; 9] = [
    "asset",
    "beneficiary",
    "contract",
    "key_price",
    "keys_slot",
    "max_periods_per_key",
    "max_periods_per_purchase",
    "token_id",
    "version",
];

/// The Solidity the port reproduces, published beside the artifact as the
/// provenance of the descriptor.
pub const SOLIDITY_SOURCE: &str = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.21;

contract PublicLock {
    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    event KeyExtended(uint256 indexed tokenId, uint256 remainingPeriods);

    uint256 public constant TOKEN_ID = 1;

    mapping(address => uint256) public remainingPeriodsOf;

    address public immutable beneficiary;
    uint256 public immutable keyPrice;
    uint256 public immutable maxPeriodsPerPurchase;
    uint256 public immutable maxPeriodsPerKey;

    constructor(
        address _beneficiary,
        uint256 _keyPrice,
        uint256 _maxPeriodsPerPurchase,
        uint256 _maxPeriodsPerKey
    ) {
        require(_beneficiary != address(0));
        require(_keyPrice > 0);
        require(_maxPeriodsPerPurchase > 0);
        require(_maxPeriodsPerKey >= _maxPeriodsPerPurchase);
        beneficiary = _beneficiary;
        keyPrice = _keyPrice;
        maxPeriodsPerPurchase = _maxPeriodsPerPurchase;
        maxPeriodsPerKey = _maxPeriodsPerKey;
    }

    function purchase(uint256 periods) external payable returns (uint256) {
        require(periods > 0);
        require(periods <= maxPeriodsPerPurchase);
        require(msg.value == keyPrice * periods);
        uint256 held = remainingPeriodsOf[msg.sender];
        uint256 extended = held + periods;
        require(extended <= maxPeriodsPerKey);
        remainingPeriodsOf[msg.sender] = extended;
        (bool paid, ) = beneficiary.call{value: msg.value}("");
        require(paid);
        if (held == 0) {
            emit Transfer(address(0), msg.sender, TOKEN_ID);
        }
        emit KeyExtended(TOKEN_ID, extended);
        return extended;
    }

    function getHasValidKey() external view returns (bool) {
        return remainingPeriodsOf[msg.sender] > 0;
    }

    function remainingPeriods() external view returns (uint256) {
        return remainingPeriodsOf[msg.sender];
    }
}
"#;

/// The pinned toolchain manifest published inside the archive. The build plan
/// carries its digest, so a verifier that rebuilds the source is rebuilding it
/// with exactly this emitter and this frozen ABI.
pub const TOOLCHAIN_MANIFEST: &str =
    "kit = layerx-porting-evm\nemitter = evm-port-emitter/1\nabi = layerx_v1/1\nsubset = deterministic-integer-wasm/1\n";

/// The pinned dependency lock published inside the archive.
pub const DEPENDENCY_LOCK: &str =
    "layerx-programs-runtime = 0.1.0\nlayerx-programs-registry = 0.1.0\n";

/// A complete `PublicLock` port: the immutables the Solidity constructor took,
/// resolved against `LayerX` account identifiers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicLockPort {
    asset: [u8; 32],
    beneficiary: [u8; 32],
    key_price: u64,
    keys_slot: u64,
    max_periods_per_purchase: u64,
    max_periods_per_key: u64,
    token_id: u64,
}

/// The constructor arguments of one port, in Solidity declaration order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockTerms {
    /// The 402LXP asset the lock is priced in.
    pub asset: [u8; 32],
    /// The account every purchase credits.
    pub beneficiary: [u8; 32],
    /// The price of one period.
    pub key_price: u64,
    /// The declaration-order slot of the key store in the Solidity source.
    pub keys_slot: u64,
    /// The most periods one call may buy.
    pub max_periods_per_purchase: u64,
    /// The most periods one key may hold.
    pub max_periods_per_key: u64,
    /// The membership token identifier the lock mints.
    pub token_id: u64,
}

struct HostImports {
    storage_read: u32,
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
}

struct PurchaseImports {
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
    store_word: u32,
    read_periods: u32,
}

struct DispatchTargets {
    load_be64: u32,
    purchase: u32,
    has_valid_key: u32,
    remaining_periods: u32,
}

impl PublicLockPort {
    /// Resolves the Solidity constructor arguments into a port.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero asset and beneficiary, a zero price, bounds
    /// outside the declared ranges, a key bound below the per-call bound and
    /// any term whose product would leave the signed 64-bit domain the ABI
    /// carries amounts in.
    pub fn new(terms: LockTerms) -> Result<Self, PortRefusal> {
        if terms.asset == [0u8; 32] || terms.beneficiary == [0u8; 32] {
            return Err(PortRefusal::ZeroAddress);
        }
        if terms.key_price == 0
            || terms.token_id == 0
            || terms.max_periods_per_purchase == 0
            || terms.max_periods_per_purchase > MAX_PERIODS_BOUND
            || terms.max_periods_per_key < terms.max_periods_per_purchase
            || terms.max_periods_per_key > MAX_TOTAL_PERIODS_BOUND
        {
            return Err(PortRefusal::OutOfRange);
        }
        let ceiling = u64::try_from(i64::MAX).unwrap_or(u64::MAX);
        if terms
            .key_price
            .checked_mul(terms.max_periods_per_purchase)
            .is_none_or(|total| total > ceiling)
        {
            return Err(PortRefusal::OutOfRange);
        }
        Ok(Self {
            asset: terms.asset,
            beneficiary: terms.beneficiary,
            key_price: terms.key_price,
            keys_slot: terms.keys_slot,
            max_periods_per_purchase: terms.max_periods_per_purchase,
            max_periods_per_key: terms.max_periods_per_key,
            token_id: terms.token_id,
        })
    }

    /// Returns the asset the lock is priced in.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    /// Returns the account every purchase credits.
    #[must_use]
    pub const fn beneficiary(&self) -> [u8; 32] {
        self.beneficiary
    }

    /// Returns the price of one period.
    #[must_use]
    pub const fn key_price(&self) -> u64 {
        self.key_price
    }

    /// Returns the namespaced-storage key the ported key store occupies.
    ///
    /// The Solidity mapping is indexed only by `msg.sender` and namespaced
    /// storage is already partitioned by principal, so the mapping collapses
    /// onto its declared slot and no `keccak256` runs at execution time.
    #[must_use]
    pub fn storage_key(&self) -> [u8; 32] {
        caller_indexed_key(self.keys_slot)
    }

    /// Returns the exact price of a purchase of `periods` periods.
    ///
    /// # Errors
    ///
    /// Refuses a zero purchase and any purchase beyond the declared per-call
    /// bound, exactly as the Solidity `require` statements do.
    pub fn price(&self, periods: u64) -> Result<u128, PortRefusal> {
        if periods == 0 || periods > self.max_periods_per_purchase {
            return Err(PortRefusal::OutOfRange);
        }
        self.key_price
            .checked_mul(periods)
            .map(u128::from)
            .ok_or(PortRefusal::OutOfRange)
    }

    /// Returns the single 402LXP leg a purchase produces.
    ///
    /// # Errors
    ///
    /// Refuses a purchase outside the declared bounds.
    pub fn payment(&self, periods: u64) -> Result<Transfer402Plan, PortRefusal> {
        Transfer402Plan::new(self.asset, self.beneficiary, self.price(periods)?)
    }

    /// Returns the exact authority an activity must carry to buy `periods`
    /// periods: namespaced reads and writes, event emission and one capped
    /// transfer to the beneficiary. Nothing else is granted, and no grant
    /// admits a payment larger than the price.
    ///
    /// # Errors
    ///
    /// Refuses a purchase outside the declared bounds or an invalid grant.
    pub fn purchase_capabilities(&self, periods: u64) -> Result<CapabilitySet, PortRefusal> {
        let payment = self.payment(periods)?;
        Ok(CapabilitySet::new([
            Capability::StorageRead,
            Capability::StorageWrite,
            Capability::EmitEvent,
            payment.capability(),
        ])?)
    }

    /// Returns the event payload a first purchase emits under the mint topic.
    ///
    /// # Errors
    ///
    /// Refuses a malformed canonical signature.
    pub fn mint_payload(&self) -> Result<Vec<u8>, PortRefusal> {
        transfer_event()?.data(&[Word::from_u64(self.token_id)])
    }

    /// Returns the event payload every purchase emits under the extension
    /// topic, carrying the token identifier and the new period count.
    ///
    /// # Errors
    ///
    /// Refuses a malformed canonical signature.
    pub fn extension_payload(&self, periods: u64) -> Result<Vec<u8>, PortRefusal> {
        extended_event()?.data(&[Word::from_u64(self.token_id), Word::from_u64(periods)])
    }

    /// Encodes the canonical port descriptor, the document the reproducible
    /// build compiles.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "version = {DESCRIPTOR_VERSION}\ncontract = {CONTRACT_NAME}\nasset = {}\nbeneficiary = {}\nkey_price = {}\nkeys_slot = {}\nmax_periods_per_purchase = {}\nmax_periods_per_key = {}\ntoken_id = {}\n",
            hex::encode(&self.asset),
            hex::encode(&self.beneficiary),
            self.key_price,
            self.keys_slot,
            self.max_periods_per_purchase,
            self.max_periods_per_key,
            self.token_id,
        )
    }

    /// Parses the canonical port descriptor.
    ///
    /// # Errors
    ///
    /// Refuses malformed lines, unknown keys, repeated keys, missing keys, a
    /// foreign descriptor version, a foreign contract name and any term the
    /// port constructor rejects.
    pub fn parse(text: &str) -> Result<Self, PortRefusal> {
        let mut fields = BTreeMap::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let (key, value) = trimmed
                .split_once('=')
                .ok_or(PortRefusal::InvalidDescriptor)?;
            if fields.insert(key.trim(), value.trim()).is_some() {
                return Err(PortRefusal::InvalidDescriptor);
            }
        }
        if fields.keys().any(|key| !DESCRIPTOR_KEYS.contains(key)) {
            return Err(PortRefusal::InvalidDescriptor);
        }
        if field(&fields, "version")? != DESCRIPTOR_VERSION
            || field(&fields, "contract")? != CONTRACT_NAME
        {
            return Err(PortRefusal::InvalidDescriptor);
        }
        Self::new(LockTerms {
            asset: digest(&fields, "asset")?,
            beneficiary: digest(&fields, "beneficiary")?,
            key_price: number(&fields, "key_price")?,
            keys_slot: number(&fields, "keys_slot")?,
            max_periods_per_purchase: number(&fields, "max_periods_per_purchase")?,
            max_periods_per_key: number(&fields, "max_periods_per_key")?,
            token_id: number(&fields, "token_id")?,
        })
    }

    /// Emits the deterministic `WebAssembly` module for this port.
    ///
    /// # Errors
    ///
    /// Refuses a malformed canonical signature and a module beyond the
    /// runtime's declared byte bound.
    pub fn code(&self) -> Result<Vec<u8>, PortRefusal> {
        let mut builder = ModuleBuilder::new(MEMORY_PAGES);
        let host_type = builder.signature(&[I32, I32, I32, I32], &[I32]);
        let transfer_type = builder.signature(&[I64, I64, I32, I32, I32, I32], &[I32]);
        let load_type = builder.signature(&[I32], &[I64]);
        let store_type = builder.signature(&[I32, I64], &[]);
        let periods_type = builder.signature(&[], &[I64]);
        let purchase_type = builder.signature(&[I64], &[I64]);
        let flag_type = builder.signature(&[], &[I32]);
        let reserve_type = builder.signature(&[I32], &[I32]);
        let entry_type = builder.signature(&[I32, I32], &[I32]);
        let hosts = HostImports {
            storage_read: builder.import(ABI_MODULE, "storage_read", host_type),
            storage_write: builder.import(ABI_MODULE, "storage_write", host_type),
            event_emit: builder.import(ABI_MODULE, "event_emit", host_type),
            transfer_402: builder.import(ABI_MODULE, "transfer_402", transfer_type),
        };
        builder.segment(KEY_POINTER, &self.constants()?);
        let load_be64 = emit_load_be64(&mut builder, load_type);
        let store_word = emit_store_word(&mut builder, store_type);
        let read_periods =
            emit_read_periods(&mut builder, periods_type, hosts.storage_read, load_be64);
        let purchase = self.emit_purchase(
            &mut builder,
            purchase_type,
            &PurchaseImports {
                storage_write: hosts.storage_write,
                event_emit: hosts.event_emit,
                transfer_402: hosts.transfer_402,
                store_word,
                read_periods,
            },
        );
        let has_valid_key = emit_has_valid_key(&mut builder, flag_type, read_periods);
        let remaining_periods = emit_remaining_periods(&mut builder, periods_type, read_periods);
        let reserve = emit_reserve(&mut builder, reserve_type);
        let entry = emit_call_entry(
            &mut builder,
            entry_type,
            &DispatchTargets {
                load_be64,
                purchase,
                has_valid_key,
                remaining_periods,
            },
        );
        builder.export_memory(MEMORY_EXPORT);
        builder.export_function(PURCHASE_EXPORT, purchase);
        builder.export_function(HAS_VALID_KEY_EXPORT, has_valid_key);
        builder.export_function(REMAINING_PERIODS_EXPORT, remaining_periods);
        builder.export_function(RESERVE_EXPORT, reserve);
        builder.export_function(CALL_ENTRY_EXPORT, entry);
        let wasm = builder.finish();
        if u64::try_from(wasm.len()).unwrap_or(u64::MAX)
            > layerx_programs_runtime::limits::DEFAULT_MAX_MODULE_BYTES
        {
            return Err(PortRefusal::ModuleTooLarge);
        }
        Ok(wasm)
    }

    /// Returns the `SHA-256` code hash of the emitted module, which is the
    /// digest the deployment activity authenticates and the registry compares
    /// a hermetic rebuild against.
    ///
    /// # Errors
    ///
    /// Refuses whatever [`Self::code`] refuses.
    pub fn code_hash(&self) -> Result<[u8; 32], PortRefusal> {
        Ok(crate::hash::sha256(&self.code()?))
    }

    fn constants(&self) -> Result<Vec<u8>, PortRefusal> {
        let mut constants = Vec::with_capacity(192);
        constants.extend_from_slice(&self.storage_key());
        constants.extend_from_slice(&self.asset);
        constants.extend_from_slice(&self.beneficiary);
        constants.extend_from_slice(&transfer_event()?.topic());
        constants.extend_from_slice(&extended_event()?.topic());
        constants.extend_from_slice(&Word::from_u64(self.token_id).bytes());
        Ok(constants)
    }

    fn emit_purchase(
        &self,
        builder: &mut ModuleBuilder,
        signature: u32,
        imports: &PurchaseImports,
    ) -> u32 {
        let price = i64::try_from(self.key_price).unwrap_or(i64::MAX);
        let per_call = i64::try_from(self.max_periods_per_purchase).unwrap_or(i64::MAX);
        let per_key = i64::try_from(self.max_periods_per_key).unwrap_or(i64::MAX);
        let mut code = Code::new();
        code.local_get(0);
        code.i64_const(1);
        code.op(I64_LT_S);
        code.trap_if();
        code.local_get(0);
        code.i64_const(per_call);
        code.op(I64_GT_S);
        code.trap_if();
        code.call(imports.read_periods);
        code.local_set(1);
        code.local_get(1);
        code.local_get(0);
        code.op(I64_ADD);
        code.local_set(2);
        code.local_get(2);
        code.i64_const(per_key);
        code.op(I64_GT_S);
        code.trap_if();
        code.pointer(VALUE_POINTER);
        code.local_get(2);
        code.call(imports.store_word);
        code.pointer(KEY_POINTER);
        code.i32_const(WORD_LENGTH);
        code.pointer(VALUE_POINTER);
        code.i32_const(WORD_LENGTH);
        code.call(imports.storage_write);
        code.trap_unless_ok();
        code.i64_const(0);
        code.local_get(0);
        code.i64_const(price);
        code.op(I64_MUL);
        code.pointer(ASSET_POINTER);
        code.i32_const(WORD_LENGTH);
        code.pointer(BENEFICIARY_POINTER);
        code.i32_const(WORD_LENGTH);
        code.call(imports.transfer_402);
        code.trap_unless_ok();
        code.local_get(1);
        code.op(I64_EQZ);
        code.block(IF, VOID_BLOCK);
        code.pointer(TRANSFER_TOPIC_POINTER);
        code.i32_const(WORD_LENGTH);
        code.pointer(TOKEN_POINTER);
        code.i32_const(WORD_LENGTH);
        code.call(imports.event_emit);
        code.trap_unless_ok();
        code.end();
        for offset in [0_u32, 8, 16, 24] {
            code.pointer(EVENT_POINTER);
            code.pointer(TOKEN_POINTER);
            code.memory(I64_LOAD, offset);
            code.memory(I64_STORE, offset);
        }
        code.pointer(EVENT_POINTER + 32);
        code.local_get(2);
        code.call(imports.store_word);
        code.pointer(EXTENDED_TOPIC_POINTER);
        code.i32_const(WORD_LENGTH);
        code.pointer(EVENT_POINTER);
        code.i32_const(WORD_LENGTH * 2);
        code.call(imports.event_emit);
        code.trap_unless_ok();
        code.local_get(2);
        code.end();
        builder.function(signature, &[(2, I64)], &code)
    }
}

fn emit_call_entry(builder: &mut ModuleBuilder, signature: u32, targets: &DispatchTargets) -> u32 {
    let mut code = Code::new();
    code.local_get(1);
    code.i32_const(SELECTOR_CALLDATA);
    code.op(I32_LT_S);
    code.trap_if();
    code.local_get(0);
    code.pointer(INPUT_POINTER);
    code.op(I32_NE);
    code.trap_if();
    code.local_get(0);
    code.memory(I32_LOAD, 0);
    code.local_set(2);
    code.local_get(2);
    code.i32_const(selector_code(PURCHASE_METHOD));
    code.op(I32_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(PURCHASE_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    for offset in [4_u32, 12, 20] {
        code.local_get(0);
        code.memory(I64_LOAD, offset);
        if offset != 4 {
            code.op(I64_OR);
        }
    }
    code.i64_const(0);
    code.op(I64_NE);
    code.trap_if();
    code.local_get(0);
    code.i32_const(PURCHASE_CALLDATA - 8);
    code.op(I32_ADD);
    code.call(targets.load_be64);
    code.call(targets.purchase);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i32_const(selector_code(HAS_VALID_KEY_METHOD));
    code.op(I32_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(SELECTOR_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.has_valid_key);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i32_const(selector_code(REMAINING_PERIODS_METHOD));
    code.op(I32_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(SELECTOR_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.remaining_periods);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.trap();
    code.end();
    builder.function(signature, &[(1, I32)], &code)
}

/// Returns the mint event with its Solidity `topic0` preserved, so an
/// existing `ERC-721` indexer's filter matches the ported program unchanged.
///
/// # Errors
///
/// Refuses a malformed canonical signature.
pub fn transfer_event() -> Result<EventAbi, PortRefusal> {
    EventAbi::envelope_derived(TRANSFER_EVENT, 2)
}

/// Returns the key-extension event with its Solidity `topic0` preserved.
///
/// # Errors
///
/// Refuses a malformed canonical signature.
pub fn extended_event() -> Result<EventAbi, PortRefusal> {
    EventAbi::new(KEY_EXTENDED_EVENT)
}

/// Returns the authority a read-only query needs: namespaced reads and
/// nothing else.
///
/// # Errors
///
/// Refuses an invalid grant.
pub fn query_capabilities() -> Result<CapabilitySet, PortRefusal> {
    Ok(CapabilitySet::new([Capability::StorageRead])?)
}

/// Returns the stored value of a key holding `periods` periods. The value
/// stays a 32-byte big-endian `EVM` word, so an exported state dump imports
/// cell for cell and a later export reads back identically.
#[must_use]
pub fn stored_periods(periods: u64) -> [u8; 32] {
    Word::from_u64(periods).bytes()
}

fn emit_load_be64(builder: &mut ModuleBuilder, signature: u32) -> u32 {
    let mut code = Code::new();
    code.i64_const(0);
    code.local_set(1);
    for offset in 0..8_u32 {
        code.local_get(1);
        code.i64_const(8);
        code.op(I64_SHL);
        code.local_get(0);
        code.memory(I32_LOAD8_U, offset);
        code.op(I64_EXTEND_I32_U);
        code.op(I64_OR);
        code.local_set(1);
    }
    code.local_get(1);
    code.end();
    builder.function(signature, &[(1, I64)], &code)
}

fn emit_store_word(builder: &mut ModuleBuilder, signature: u32) -> u32 {
    let mut code = Code::new();
    for offset in [0_u32, 8, 16] {
        code.local_get(0);
        code.i64_const(0);
        code.memory(I64_STORE, offset);
    }
    for index in 0..8_u32 {
        code.local_get(0);
        code.local_get(1);
        code.i64_const(i64::from(56 - index * 8));
        code.op(I64_SHR_U);
        code.op(I32_WRAP_I64);
        code.memory(I32_STORE8, 24 + index);
    }
    code.end();
    builder.function(signature, &[], &code)
}

fn emit_read_periods(
    builder: &mut ModuleBuilder,
    signature: u32,
    storage_read: u32,
    load_be64: u32,
) -> u32 {
    let mut code = Code::new();
    code.pointer(KEY_POINTER);
    code.i32_const(WORD_LENGTH);
    code.pointer(VALUE_POINTER);
    code.i32_const(WORD_LENGTH);
    code.call(storage_read);
    code.local_set(0);
    code.local_get(0);
    code.i32_const(0);
    code.op(I32_LT_S);
    code.trap_if();
    code.local_get(0);
    code.op(I32_EQZ);
    code.block(IF, VOID_BLOCK);
    code.i64_const(0);
    code.op(RETURN);
    code.end();
    code.local_get(0);
    code.i32_const(STORED_LENGTH);
    code.op(I32_NE);
    code.trap_if();
    for offset in [0_u32, 8, 16] {
        code.pointer(VALUE_POINTER);
        code.memory(I64_LOAD, offset);
        if offset != 0 {
            code.op(I64_OR);
        }
    }
    code.i64_const(0);
    code.op(I64_NE);
    code.trap_if();
    code.pointer(VALUE_POINTER + 24);
    code.call(load_be64);
    code.end();
    builder.function(signature, &[(1, I32)], &code)
}

fn emit_has_valid_key(builder: &mut ModuleBuilder, signature: u32, read_periods: u32) -> u32 {
    let mut code = Code::new();
    code.call(read_periods);
    code.i64_const(0);
    code.op(I64_GT_S);
    code.end();
    builder.function(signature, &[], &code)
}

fn emit_remaining_periods(builder: &mut ModuleBuilder, signature: u32, read_periods: u32) -> u32 {
    let mut code = Code::new();
    code.call(read_periods);
    code.end();
    builder.function(signature, &[], &code)
}

fn emit_reserve(builder: &mut ModuleBuilder, signature: u32) -> u32 {
    let mut code = Code::new();
    code.local_get(0);
    code.i32_const(0);
    code.op(I32_LT_S);
    code.block(IF, VOID_BLOCK);
    code.i32_const(-1);
    code.op(RETURN);
    code.end();
    code.local_get(0);
    code.i32_const(INPUT_CAPACITY);
    code.op(I32_GT_S);
    code.block(IF, VOID_BLOCK);
    code.i32_const(-1);
    code.op(RETURN);
    code.end();
    code.pointer(INPUT_POINTER);
    code.end();
    builder.function(signature, &[], &code)
}

fn selector_code(signature: &str) -> i32 {
    i32::from_le_bytes(crate::keccak::selector(signature))
}

fn field<'text>(
    fields: &BTreeMap<&'text str, &'text str>,
    key: &str,
) -> Result<&'text str, PortRefusal> {
    fields
        .get(key)
        .copied()
        .ok_or(PortRefusal::InvalidDescriptor)
}

fn digest(fields: &BTreeMap<&str, &str>, key: &str) -> Result<[u8; 32], PortRefusal> {
    Ok(hex::decode_digest(field(fields, key)?)?)
}

fn number(fields: &BTreeMap<&str, &str>, key: &str) -> Result<u64, PortRefusal> {
    field(fields, key)?
        .parse()
        .map_err(|_| PortRefusal::InvalidDescriptor)
}
