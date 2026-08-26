//! The ported reference contract: the donation contract from the `CosmWasm`
//! book, carried onto the programs ABI.
//!
//! The contract was chosen because every part of it lands on a different edge
//! of the port. Its configuration is an `Item` written once on instantiation,
//! so it becomes a descriptor the deployment pins. Its donor ledger is a `Map`
//! keyed by `info.sender`, so it collapses onto the per-principal namespace.
//! Its payment arrives as `info.funds` and leaves as a `BankMsg` in the same
//! message, so it survives the monetary law intact. Its queries take the donor
//! as an argument, because a `CosmWasm` query has no sender, and the port drops
//! that argument because a `LayerX` query does.

use std::collections::BTreeMap;

use layerx_programs::hex;
use layerx_programs_runtime::{Capability, CapabilitySet, ProgramId, ABI_MODULE};

use crate::error::PortRefusal;
use crate::json::{FieldSchema, FieldValue, RecordSchema, ValueType};
use crate::messages::{ContractEvent, EntryPoint, MessageVariant};
use crate::monetary::{ProgramAccountTransferPlan, Transfer402Plan};
use crate::storage::{item_key, map_prefix};
use crate::wasm::{
    Code, ModuleBuilder, ELSE, I32, I32_EQZ, I32_GT_S, I32_LT_S, I32_NE, I32_WRAP_I64, I64,
    I64_ADD, I64_EQ, I64_GT_S, I64_LOAD, I64_LT_S, I64_MUL, I64_STORE, I64_SUB, IF, RETURN,
    VOID_BLOCK,
};

/// The contract name carried by the published descriptor.
pub const CONTRACT_NAME: &str = "donation";
/// Archive path of the `CosmWasm` source the port reproduces.
pub const SOURCE_PATH: &str = "src/contract.rs";
/// Archive path of the canonical port descriptor, which is the build input.
pub const DESCRIPTOR_PATH: &str = "port/donation.port";
/// Archive path of the pinned toolchain manifest.
pub const TOOLCHAIN_PATH: &str = "toolchain/porting-cosmwasm.toolchain";
/// Archive path of the pinned dependency lock.
pub const DEPENDENCY_LOCK_PATH: &str = "toolchain/porting-cosmwasm.lock";
/// Path of the artifact the pinned build produces.
pub const ARTIFACT_PATH: &str = "build/donation.wasm";
/// The pinned build command, whose last word names the descriptor to compile.
pub const BUILD_COMMAND: &str = "layerx-porting-cosmwasm emit port/donation.port";

/// Maps a contract-funded `BankMsg::Send` onto one public, rederivable LayerX account.
///
/// # Errors
///
/// Refuses any owner, seed, source or monetary field that does not form the
/// exact bounded contract payout.
pub fn contract_bank_send(
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

/// Namespace of the configuration `Item`, whose raw key the port keeps.
pub const CONFIG_ITEM: &str = "config";
/// Namespace of the donor `Map`, whose raw key prefix the port keeps.
pub const DONATIONS_MAP: &str = "donations";
/// Name of the stored donor record.
pub const RECORD_NAME: &str = "DonationRecord";
/// Name of the record's single field.
pub const COUNT_FIELD: &str = "count";
/// `JSON` name of the donate execute variant.
pub const DONATE_VARIANT: &str = "donate";
/// `JSON` name of the donor-count query variant.
pub const DONATIONS_VARIANT: &str = "donations";
/// `JSON` name of the remaining-headroom query variant.
pub const REMAINING_VARIANT: &str = "remaining";
/// `JSON` name of the donate variant's single argument.
pub const TIMES_FIELD: &str = "times";
/// `JSON` name of the query argument the port drops, because the invoking
/// principal already names the donor.
pub const DONOR_FIELD: &str = "donor";
/// Type of the emitted contract event, which a chain prefixes with `wasm-`.
pub const DONATION_EVENT: &str = "donation";

/// Export invoked by an activity to record donations.
pub const DONATE_EXPORT: &str = "donate";
/// Export answering how many donations the invoking principal has made.
pub const DONATIONS_EXPORT: &str = "donations";
/// Export answering how many donations the invoking principal has left.
pub const REMAINING_EXPORT: &str = "remaining";
/// Export a calling program uses to reserve the message region.
pub const RESERVE_EXPORT: &str = "layerx_reserve";
/// Export a calling program enters with a ported message.
pub const CALL_ENTRY_EXPORT: &str = "layerx_call";
/// Export name of the linear memory the host reads guest buffers from.
pub const MEMORY_EXPORT: &str = "memory";

/// Upper bound on the donations one principal may record, bounded so the
/// composition entry point can return the count as a non-negative result code.
pub const MAX_CAP_BOUND: u64 = 2_147_483_647;

const RECORD_BYTES: usize = 8;
const MEMORY_PAGES: u32 = 1;
const KEY_POINTER: u32 = 0;
const ASSET_POINTER: u32 = 32;
const BENEFICIARY_POINTER: u32 = 64;
const TOPIC_POINTER: u32 = 96;
const VALUE_POINTER: u32 = 128;
const EVENT_POINTER: u32 = 160;
const INPUT_POINTER: u32 = 1_024;
const INPUT_CAPACITY: i32 = 256;
const RECORD_LENGTH: i32 = 8;
const STORED_LENGTH: i32 = 9;
const VALUE_CAPACITY: i32 = 16;
const ASSET_LENGTH: i32 = 32;
const TAG_CALLDATA: i32 = 8;
const DONATE_CALLDATA: i32 = 16;
const TIMES_OFFSET: u32 = 8;
const DESCRIPTOR_VERSION: &str = "1";
const DESCRIPTOR_KEYS: [&str; 6] = [
    "asset",
    "beneficiary",
    "cap",
    "contract",
    "price",
    "version",
];

/// The `CosmWasm` contract the port reproduces, published beside the artifact
/// as the provenance of the descriptor.
pub const COSMWASM_SOURCE: &str = r#"use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    coins, entry_point, to_json_binary, Addr, BankMsg, Binary, Deps, DepsMut, Env, Event,
    MessageInfo, Response, StdError, StdResult, Uint128,
};
use cw_storage_plus::{Item, Map};

pub const CONFIG: Item<Config> = Item::new("config");
pub const DONATIONS: Map<&Addr, DonationRecord> = Map::new("donations");

#[cw_serde]
pub struct Config {
    pub beneficiary: Addr,
    pub denom: String,
    pub minimal_donation: Uint128,
    pub donation_cap: u64,
}

#[cw_serde]
pub struct DonationRecord {
    pub count: u64,
}

#[cw_serde]
pub struct InstantiateMsg {
    pub beneficiary: String,
    pub denom: String,
    pub minimal_donation: Uint128,
    pub donation_cap: u64,
}

#[cw_serde]
pub enum ExecuteMsg {
    Donate { times: u64 },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(DonationsResponse)]
    Donations { donor: String },
    #[returns(DonationsResponse)]
    Remaining { donor: String },
}

#[cw_serde]
pub struct DonationsResponse {
    pub count: u64,
}

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    if msg.donation_cap == 0 || msg.minimal_donation.is_zero() {
        return Err(StdError::generic_err("configuration out of range"));
    }
    CONFIG.save(
        deps.storage,
        &Config {
            beneficiary: deps.api.addr_validate(&msg.beneficiary)?,
            denom: msg.denom,
            minimal_donation: msg.minimal_donation,
            donation_cap: msg.donation_cap,
        },
    )?;
    Ok(Response::new().add_attribute("action", "instantiate"))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        ExecuteMsg::Donate { times } => donate(deps, info, times),
    }
}

fn donate(deps: DepsMut, info: MessageInfo, times: u64) -> StdResult<Response> {
    let config = CONFIG.load(deps.storage)?;
    if times == 0 || times > config.donation_cap {
        return Err(StdError::generic_err("donation count out of range"));
    }
    let due = config
        .minimal_donation
        .checked_mul(Uint128::from(times))
        .map_err(|_| StdError::generic_err("donation overflows"))?;
    let sent = info
        .funds
        .iter()
        .find(|coin| coin.denom == config.denom)
        .map(|coin| coin.amount)
        .unwrap_or_default();
    if sent != due {
        return Err(StdError::generic_err("sent funds do not equal the donation"));
    }
    let held = DONATIONS
        .may_load(deps.storage, &info.sender)?
        .map(|record| record.count)
        .unwrap_or_default();
    let total = held
        .checked_add(times)
        .ok_or_else(|| StdError::generic_err("donation count out of range"))?;
    if total > config.donation_cap {
        return Err(StdError::generic_err("donation cap reached"));
    }
    DONATIONS.save(deps.storage, &info.sender, &DonationRecord { count: total })?;
    Ok(Response::new()
        .add_message(BankMsg::Send {
            to_address: config.beneficiary.to_string(),
            amount: coins(due.u128(), config.denom),
        })
        .add_event(Event::new("donation").add_attribute("count", total.to_string())))
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Donations { donor } => {
            let donor = deps.api.addr_validate(&donor)?;
            let count = DONATIONS
                .may_load(deps.storage, &donor)?
                .map(|record| record.count)
                .unwrap_or_default();
            to_json_binary(&DonationsResponse { count })
        }
        QueryMsg::Remaining { donor } => {
            let config = CONFIG.load(deps.storage)?;
            let donor = deps.api.addr_validate(&donor)?;
            let count = DONATIONS
                .may_load(deps.storage, &donor)?
                .map(|record| record.count)
                .unwrap_or_default();
            to_json_binary(&DonationsResponse {
                count: config.donation_cap.saturating_sub(count),
            })
        }
    }
}
"#;

/// The pinned toolchain manifest published inside the archive. The build plan
/// carries its digest, so a verifier that rebuilds the source is rebuilding it
/// with exactly this emitter and this frozen ABI.
pub const TOOLCHAIN_MANIFEST: &str =
    "kit = layerx-porting-cosmwasm\nemitter = cosmwasm-port-emitter/1\nabi = layerx_v1/1\nsubset = deterministic-integer-wasm/1\n";

/// The pinned dependency lock published inside the archive.
pub const DEPENDENCY_LOCK: &str =
    "layerx-programs-runtime = 0.1.0\nlayerx-programs-registry = 0.1.0\n";

/// A complete donation-contract port: the `Config` a `CosmWasm` deployment
/// writes on instantiation, resolved against `LayerX` account identifiers and
/// pinned into the module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonationPort {
    asset: [u8; 32],
    beneficiary: [u8; 32],
    price: u64,
    cap: u64,
}

/// The instantiate arguments of one port, in `Config` declaration order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DonationTerms {
    /// The 402LXP asset that stands in for the contract's denom, since a
    /// program is paid in an authenticated asset rather than in bank coins.
    pub asset: [u8; 32],
    /// The account every donation credits.
    pub beneficiary: [u8; 32],
    /// The minimal donation, which is the price of one recorded donation.
    pub price: u64,
    /// The most donations one principal may record.
    pub cap: u64,
}

struct HostImports {
    storage_read: u32,
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
}

struct DonateImports {
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
    read_count: u32,
    key_length: i32,
    topic_length: i32,
    event_length: i32,
    count_offset: u32,
}

struct DispatchTargets {
    donate_tag: i64,
    donations_tag: i64,
    remaining_tag: i64,
    donate: u32,
    donations: u32,
    remaining: u32,
}

impl DonationPort {
    /// Resolves the instantiate arguments into a port.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero asset and beneficiary, a zero price, a cap
    /// outside the declared range and any pair of terms whose product would
    /// leave the signed 64-bit domain the ABI carries amounts in.
    pub fn new(terms: DonationTerms) -> Result<Self, PortRefusal> {
        if terms.asset == [0u8; 32] || terms.beneficiary == [0u8; 32] {
            return Err(PortRefusal::EmptyAddress);
        }
        if terms.price == 0 || terms.cap == 0 || terms.cap > MAX_CAP_BOUND {
            return Err(PortRefusal::OutOfRange);
        }
        let ceiling = u64::try_from(i64::MAX).unwrap_or(u64::MAX);
        if terms
            .price
            .checked_mul(terms.cap)
            .is_none_or(|total| total > ceiling)
        {
            return Err(PortRefusal::OutOfRange);
        }
        Ok(Self {
            asset: terms.asset,
            beneficiary: terms.beneficiary,
            price: terms.price,
            cap: terms.cap,
        })
    }

    /// Returns the asset the contract is priced in.
    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    /// Returns the account every donation credits.
    #[must_use]
    pub const fn beneficiary(&self) -> [u8; 32] {
        self.beneficiary
    }

    /// Returns the minimal donation.
    #[must_use]
    pub const fn price(&self) -> u64 {
        self.price
    }

    /// Returns the most donations one principal may record.
    #[must_use]
    pub const fn cap(&self) -> u64 {
        self.cap
    }

    /// Returns the namespaced-storage key the ported donor ledger occupies.
    ///
    /// The `Map` is keyed by `info.sender` and namespaced storage is already
    /// partitioned by principal, so the entry collapses onto the map's raw
    /// namespace prefix and no address is composed into the key at execution
    /// time.
    ///
    /// # Errors
    ///
    /// Refuses a key the storage bounds reject.
    pub fn storage_key() -> Result<Vec<u8>, PortRefusal> {
        map_prefix(DONATIONS_MAP)
    }

    /// Returns the raw key the configuration `Item` occupied on the chain. The
    /// port pins the configuration into the module instead of storing it, so
    /// the key is published for state comparison rather than written.
    ///
    /// # Errors
    ///
    /// Refuses a key the storage bounds reject.
    pub fn config_key() -> Result<Vec<u8>, PortRefusal> {
        item_key(CONFIG_ITEM)
    }

    /// Returns the exact amount due for `times` donations.
    ///
    /// # Errors
    ///
    /// Refuses a zero count and any count beyond the declared cap, exactly as
    /// the contract's range check does.
    pub fn due(&self, times: u64) -> Result<u128, PortRefusal> {
        if times == 0 || times > self.cap {
            return Err(PortRefusal::OutOfRange);
        }
        self.price
            .checked_mul(times)
            .map(u128::from)
            .ok_or(PortRefusal::OutOfRange)
    }

    /// Returns the single 402LXP leg a donation produces, which is the ported
    /// `BankMsg::Send` funded by the coins the caller attached.
    ///
    /// # Errors
    ///
    /// Refuses a count outside the declared bounds.
    pub fn payment(&self, times: u64) -> Result<Transfer402Plan, PortRefusal> {
        Transfer402Plan::new(self.asset, self.beneficiary, self.due(times)?)
    }

    /// Returns the exact authority an activity must carry to record `times`
    /// donations: namespaced reads and writes, event emission and one capped
    /// transfer to the beneficiary. Nothing else is granted, and no grant
    /// admits a payment larger than the amount due.
    ///
    /// # Errors
    ///
    /// Refuses a count outside the declared bounds or an invalid grant.
    pub fn donate_capabilities(&self, times: u64) -> Result<CapabilitySet, PortRefusal> {
        let payment = self.payment(times)?;
        Ok(CapabilitySet::new([
            Capability::StorageRead,
            Capability::StorageWrite,
            Capability::EmitEvent,
            payment.capability(),
        ])?)
    }

    /// Returns the event payload every donation emits under the `wasm-donation`
    /// topic, carrying the `count` attribute.
    ///
    /// # Errors
    ///
    /// Refuses a malformed event declaration and a payload beyond the ABI
    /// bound.
    pub fn donation_payload(count: u64) -> Result<Vec<u8>, PortRefusal> {
        donation_event()?.data(&[FieldValue::U64(count)])
    }

    /// Encodes the canonical port descriptor, the document the reproducible
    /// build compiles.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "version = {DESCRIPTOR_VERSION}\ncontract = {CONTRACT_NAME}\nasset = {}\nbeneficiary = {}\nprice = {}\ncap = {}\n",
            hex::encode(&self.asset),
            hex::encode(&self.beneficiary),
            self.price,
            self.cap,
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
        Self::new(DonationTerms {
            asset: digest(&fields, "asset")?,
            beneficiary: digest(&fields, "beneficiary")?,
            price: number(&fields, "price")?,
            cap: number(&fields, "cap")?,
        })
    }

    /// Emits the deterministic `WebAssembly` module for this port.
    ///
    /// # Errors
    ///
    /// Refuses a malformed declaration, a key or topic the declared bounds
    /// reject and a module beyond the runtime's declared byte bound.
    pub fn code(&self) -> Result<Vec<u8>, PortRefusal> {
        let key = Self::storage_key()?;
        let key_length = i32::try_from(key.len()).map_err(|_| PortRefusal::KeyTooLong)?;
        let event = donation_event()?;
        let topic = event.topic().to_vec();
        let topic_length = i32::try_from(topic.len()).map_err(|_| PortRefusal::TopicTooLarge)?;
        let template = event.data(&[FieldValue::U64(0)])?;
        let event_length =
            i32::try_from(template.len()).map_err(|_| PortRefusal::EventDataTooLarge)?;
        let count_offset = u32::try_from(template.len().saturating_sub(RECORD_BYTES))
            .map_err(|_| PortRefusal::EventDataTooLarge)?;
        let mut builder = ModuleBuilder::new(MEMORY_PAGES);
        let host_type = builder.signature(&[I32, I32, I32, I32], &[I32]);
        let transfer_type = builder.signature(&[I64, I64, I32, I32, I32, I32], &[I32]);
        let count_type = builder.signature(&[], &[I64]);
        let donate_type = builder.signature(&[I64], &[I64]);
        let reserve_type = builder.signature(&[I32], &[I32]);
        let entry_type = builder.signature(&[I32, I32], &[I32]);
        let hosts = HostImports {
            storage_read: builder.import(ABI_MODULE, "storage_read", host_type),
            storage_write: builder.import(ABI_MODULE, "storage_write", host_type),
            event_emit: builder.import(ABI_MODULE, "event_emit", host_type),
            transfer_402: builder.import(ABI_MODULE, "transfer_402", transfer_type),
        };
        builder.segment(KEY_POINTER, &key);
        builder.segment(ASSET_POINTER, &self.asset);
        builder.segment(BENEFICIARY_POINTER, &self.beneficiary);
        builder.segment(TOPIC_POINTER, &topic);
        builder.segment(EVENT_POINTER, &template);
        let read_count = emit_read_count(&mut builder, count_type, hosts.storage_read, key_length);
        let donate = self.emit_donate(
            &mut builder,
            donate_type,
            &DonateImports {
                storage_write: hosts.storage_write,
                event_emit: hosts.event_emit,
                transfer_402: hosts.transfer_402,
                read_count,
                key_length,
                topic_length,
                event_length,
                count_offset,
            },
        );
        let donations = emit_donations(&mut builder, count_type, read_count);
        let remaining = self.emit_remaining(&mut builder, count_type, donations);
        let reserve = emit_reserve(&mut builder, reserve_type);
        let entry = emit_call_entry(
            &mut builder,
            entry_type,
            &DispatchTargets {
                donate_tag: donate_message()?.dispatch_word(),
                donations_tag: donations_message()?.dispatch_word(),
                remaining_tag: remaining_message()?.dispatch_word(),
                donate,
                donations,
                remaining,
            },
        );
        builder.export_memory(MEMORY_EXPORT);
        builder.export_function(DONATE_EXPORT, donate);
        builder.export_function(DONATIONS_EXPORT, donations);
        builder.export_function(REMAINING_EXPORT, remaining);
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
    /// digest the deployment activity authenticates and the registry compares a
    /// hermetic rebuild against.
    ///
    /// # Errors
    ///
    /// Refuses whatever [`Self::code`] refuses.
    pub fn code_hash(&self) -> Result<[u8; 32], PortRefusal> {
        Ok(crate::hash::sha256(&self.code()?))
    }

    fn emit_donate(
        &self,
        builder: &mut ModuleBuilder,
        signature: u32,
        imports: &DonateImports,
    ) -> u32 {
        let cap = i64::try_from(self.cap).unwrap_or(i64::MAX);
        let price = i64::try_from(self.price).unwrap_or(i64::MAX);
        let mut code = Code::new();
        code.local_get(0);
        code.i64_const(1);
        code.op(I64_LT_S);
        code.trap_if();
        code.local_get(0);
        code.i64_const(cap);
        code.op(I64_GT_S);
        code.trap_if();
        code.call(imports.read_count);
        code.local_set(1);
        code.local_get(1);
        code.i64_const(0);
        code.op(I64_LT_S);
        code.block(IF, I64);
        code.i64_const(0);
        code.op(ELSE);
        code.local_get(1);
        code.end();
        code.local_set(2);
        code.local_get(2);
        code.local_get(0);
        code.op(I64_ADD);
        code.local_set(3);
        code.local_get(3);
        code.i64_const(cap);
        code.op(I64_GT_S);
        code.trap_if();
        code.pointer(VALUE_POINTER);
        code.local_get(3);
        code.memory(I64_STORE, 0);
        code.pointer(KEY_POINTER);
        code.i32_const(imports.key_length);
        code.pointer(VALUE_POINTER);
        code.i32_const(RECORD_LENGTH);
        code.call(imports.storage_write);
        code.trap_unless_ok();
        code.i64_const(0);
        code.local_get(0);
        code.i64_const(price);
        code.op(I64_MUL);
        code.pointer(ASSET_POINTER);
        code.i32_const(ASSET_LENGTH);
        code.pointer(BENEFICIARY_POINTER);
        code.i32_const(ASSET_LENGTH);
        code.call(imports.transfer_402);
        code.trap_unless_ok();
        code.pointer(EVENT_POINTER);
        code.local_get(3);
        code.memory(I64_STORE, imports.count_offset);
        code.pointer(TOPIC_POINTER);
        code.i32_const(imports.topic_length);
        code.pointer(EVENT_POINTER);
        code.i32_const(imports.event_length);
        code.call(imports.event_emit);
        code.trap_unless_ok();
        code.local_get(3);
        code.end();
        builder.function(signature, &[(3, I64)], &code)
    }

    fn emit_remaining(&self, builder: &mut ModuleBuilder, signature: u32, donations: u32) -> u32 {
        let cap = i64::try_from(self.cap).unwrap_or(i64::MAX);
        let mut code = Code::new();
        code.i64_const(cap);
        code.call(donations);
        code.op(I64_SUB);
        code.end();
        builder.function(signature, &[], &code)
    }
}

/// Returns the stored donor record's declared shape.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn donation_record() -> Result<RecordSchema, PortRefusal> {
    RecordSchema::new(
        RECORD_NAME,
        vec![FieldSchema {
            name: COUNT_FIELD.to_owned(),
            kind: ValueType::U64,
        }],
    )
}

/// Returns the donate execute variant, whose `JSON` name the port keeps.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn donate_message() -> Result<MessageVariant, PortRefusal> {
    MessageVariant::new(
        EntryPoint::Execute,
        DONATE_VARIANT,
        RecordSchema::new(
            DONATE_VARIANT,
            vec![FieldSchema {
                name: TIMES_FIELD.to_owned(),
                kind: ValueType::U64,
            }],
        )?,
    )
}

/// Returns the donor-count query variant. Its `donor` argument is gone: a
/// `CosmWasm` query has no sender, so the contract had to be told whose count
/// to read, and a ported query is invoked by a principal the runtime already
/// authenticated.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn donations_message() -> Result<MessageVariant, PortRefusal> {
    MessageVariant::new(
        EntryPoint::Query,
        DONATIONS_VARIANT,
        RecordSchema::new(DONATIONS_VARIANT, Vec::new())?,
    )
}

/// Returns the remaining-headroom query variant, whose `donor` argument the
/// port drops for the same reason.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn remaining_message() -> Result<MessageVariant, PortRefusal> {
    MessageVariant::new(
        EntryPoint::Query,
        REMAINING_VARIANT,
        RecordSchema::new(REMAINING_VARIANT, Vec::new())?,
    )
}

/// Returns the query variant as the chain accepted it, carrying the `donor`
/// argument. An adapter at the edge reads this shape from an existing client
/// and drops the argument, because the invoking principal supplies it.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn chain_query_message(variant: &str) -> Result<MessageVariant, PortRefusal> {
    MessageVariant::new(
        EntryPoint::Query,
        variant,
        RecordSchema::new(
            variant,
            vec![FieldSchema {
                name: DONOR_FIELD.to_owned(),
                kind: ValueType::Text,
            }],
        )?,
    )
}

/// Returns the emitted contract event with the chain's own event type as its
/// topic, so an indexer filtering on `wasm-donation` keeps matching.
///
/// # Errors
///
/// Refuses a declaration the declared bounds reject.
pub fn donation_event() -> Result<ContractEvent, PortRefusal> {
    ContractEvent::custom(
        DONATION_EVENT,
        vec![FieldSchema {
            name: COUNT_FIELD.to_owned(),
            kind: ValueType::U64,
        }],
    )
}

/// Returns the authority a read-only query needs: namespaced reads and nothing
/// else.
///
/// # Errors
///
/// Refuses an invalid grant.
pub fn query_capabilities() -> Result<CapabilitySet, PortRefusal> {
    Ok(CapabilitySet::new([Capability::StorageRead])?)
}

/// Returns the stored value of a donor record holding `count` donations, in
/// the canonical framing the emitted module reads and writes.
///
/// # Errors
///
/// Refuses a malformed record declaration.
pub fn stored_record(count: u64) -> Result<Vec<u8>, PortRefusal> {
    donation_record()?.encode(&[FieldValue::U64(count)])
}

/// Returns the `JSON` value `cw-storage-plus` stored for the same record, which
/// is what an exported state dump contains and what a migration transcodes.
///
/// # Errors
///
/// Refuses a malformed record declaration.
pub fn exported_record(count: u64) -> Result<String, PortRefusal> {
    donation_record()?.encode_json(&[FieldValue::U64(count)])
}

fn emit_read_count(
    builder: &mut ModuleBuilder,
    signature: u32,
    storage_read: u32,
    key_length: i32,
) -> u32 {
    let mut code = Code::new();
    code.pointer(KEY_POINTER);
    code.i32_const(key_length);
    code.pointer(VALUE_POINTER);
    code.i32_const(VALUE_CAPACITY);
    code.call(storage_read);
    code.local_set(0);
    code.local_get(0);
    code.i32_const(0);
    code.op(I32_LT_S);
    code.trap_if();
    code.local_get(0);
    code.op(I32_EQZ);
    code.block(IF, VOID_BLOCK);
    code.i64_const(-1);
    code.op(RETURN);
    code.end();
    code.local_get(0);
    code.i32_const(STORED_LENGTH);
    code.op(I32_NE);
    code.trap_if();
    code.pointer(VALUE_POINTER);
    code.memory(I64_LOAD, 0);
    code.local_tee(1);
    code.i64_const(0);
    code.op(I64_LT_S);
    code.trap_if();
    code.local_get(1);
    code.end();
    builder.function(signature, &[(1, I32), (1, I64)], &code)
}

fn emit_donations(builder: &mut ModuleBuilder, signature: u32, read_count: u32) -> u32 {
    let mut code = Code::new();
    code.call(read_count);
    code.local_tee(0);
    code.i64_const(0);
    code.op(I64_LT_S);
    code.block(IF, I64);
    code.i64_const(0);
    code.op(ELSE);
    code.local_get(0);
    code.end();
    code.end();
    builder.function(signature, &[(1, I64)], &code)
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

fn emit_call_entry(builder: &mut ModuleBuilder, signature: u32, targets: &DispatchTargets) -> u32 {
    let mut code = Code::new();
    code.local_get(1);
    code.i32_const(TAG_CALLDATA);
    code.op(I32_LT_S);
    code.trap_if();
    code.local_get(0);
    code.pointer(INPUT_POINTER);
    code.op(I32_NE);
    code.trap_if();
    code.local_get(0);
    code.memory(I64_LOAD, 0);
    code.local_set(2);
    code.local_get(2);
    code.i64_const(targets.donate_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(DONATE_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.local_get(0);
    code.memory(I64_LOAD, TIMES_OFFSET);
    code.call(targets.donate);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i64_const(targets.donations_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(TAG_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.donations);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i64_const(targets.remaining_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(TAG_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.remaining);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.trap();
    code.end();
    builder.function(signature, &[(1, I64)], &code)
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
