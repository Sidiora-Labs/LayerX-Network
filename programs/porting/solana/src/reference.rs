//! The ported reference contract: the mint-limit and SOL-payment guards in the
//! shape Metaplex Candy Guard uses, carried onto the programs ABI.
//!
//! The program was chosen because every part of it lands on a different edge of
//! the port. Its counter lives at a program-derived address seeded by the payer
//! and the guard account, so it collapses onto the per-principal namespace. Its
//! payment is a `system_program::transfer` signed by the payer, so it survives
//! the monetary law intact. Its account carries an Anchor discriminator and its
//! event carries an Anchor event discriminator, so both have to stay
//! byte-identical for existing clients and indexers. And its guard
//! configuration lives in a separate account, which a deployment pins into the
//! module instead, because on `LayerX` each deployment is its own program.

use std::collections::BTreeMap;

use layerx_programs::hex;
use layerx_programs_runtime::{Capability, CapabilitySet, ProgramId, ABI_MODULE};

use crate::account::{AccountSchema, Field, FieldType, FieldValue};
use crate::anchor::{
    account_discriminator, instruction_discriminator, AnchorEvent, InstructionAbi,
};
use crate::error::PortRefusal;
use crate::monetary::{ProgramAccountTransferPlan, Transfer402Plan};
use crate::pubkey::{Pubkey, SeedPath, PUBKEY_BYTES};
use crate::wasm::{
    Code, ModuleBuilder, ELSE, I32, I32_EQZ, I32_GT_S, I32_LOAD16_U, I32_LT_S, I32_NE, I32_STORE16,
    I32_WRAP_I64, I64, I64_ADD, I64_EQ, I64_EXTEND_I32_U, I64_GT_S, I64_LOAD, I64_LT_S, I64_MUL,
    I64_NE, I64_STORE, I64_SUB, IF, RETURN, VOID_BLOCK,
};

/// The program name carried by the published descriptor.
pub const PROGRAM_NAME: &str = "mint_limit";
/// Archive path of the Anchor source the port reproduces.
pub const SOURCE_PATH: &str = "programs/mint-limit/src/lib.rs";
/// Archive path of the canonical port descriptor, which is the build input.
pub const DESCRIPTOR_PATH: &str = "port/mint-limit.port";
/// Archive path of the pinned toolchain manifest.
pub const TOOLCHAIN_PATH: &str = "toolchain/porting-solana.toolchain";
/// Archive path of the pinned dependency lock.
pub const DEPENDENCY_LOCK_PATH: &str = "toolchain/porting-solana.lock";
/// Path of the artifact the pinned build produces.
pub const ARTIFACT_PATH: &str = "build/mint-limit.wasm";
/// The pinned build command, whose last word names the descriptor to compile.
pub const BUILD_COMMAND: &str = "layerx-porting-solana emit port/mint-limit.port";

/// Maps an `invoke_signed` PDA payout onto one public, rederivable LayerX account.
///
/// # Errors
///
/// Refuses any owner, seed, source or monetary field that does not form the
/// exact bounded PDA payout.
pub fn program_account_payout(
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

/// Name of the counter account, whose Anchor discriminator the ported cell
/// keeps byte for byte.
pub const COUNTER_ACCOUNT: &str = "MintCounter";
/// Name of the guard configuration account a Solana deployment reads its terms
/// from and a `LayerX` deployment pins into the module.
pub const CONFIG_ACCOUNT: &str = "GuardConfig";
/// Name of the emitted event, whose Anchor discriminator becomes the topic.
pub const MINT_EVENT: &str = "MintPerformed";
/// Handler name of the ported mint instruction.
pub const MINT_INSTRUCTION: &str = "mint";
/// Handler name of the caller-scoped counter query.
pub const MINT_COUNT_INSTRUCTION: &str = "mint_count";
/// Handler name of the caller-scoped headroom query.
pub const MINT_REMAINING_INSTRUCTION: &str = "mint_remaining";

/// Export invoked by an activity to take mints against the limit.
pub const MINT_EXPORT: &str = "mint";
/// Export answering how many mints the invoking principal has taken.
pub const MINT_COUNT_EXPORT: &str = "mint_count";
/// Export answering how many mints the invoking principal has left.
pub const MINT_REMAINING_EXPORT: &str = "mint_remaining";
/// Export a calling program uses to reserve the instruction-data region.
pub const RESERVE_EXPORT: &str = "layerx_reserve";
/// Export a calling program enters with Anchor-shaped instruction data.
pub const CALL_ENTRY_EXPORT: &str = "layerx_call";
/// Export name of the linear memory the host reads guest buffers from.
pub const MEMORY_EXPORT: &str = "memory";

/// The literal seed prefix the Anchor source derives the counter with.
pub const COUNTER_SEED: &[u8] = b"mint_limit";
/// The guard's fixed identifier byte, the second seed of the derivation.
pub const GUARD_ID: u8 = 3;
/// Indices of the seeds the runtime already supplies: the payer, which is the
/// invoking principal, and the guard account, which is the program itself.
pub const ENVELOPE_SEEDS: [usize; 2] = [2, 3];
/// Name of the single field the counter account carries.
pub const COUNT_FIELD: &str = "count";
/// Upper bound on the mints one key may take, bounded by the `u16` the Anchor
/// account declares.
pub const MAX_LIMIT_BOUND: u64 = 65_535;

const MEMORY_PAGES: u32 = 1;
const KEY_POINTER: u32 = 0;
const ASSET_POINTER: u32 = 32;
const DESTINATION_POINTER: u32 = 64;
const TOPIC_POINTER: u32 = 96;
const ACCOUNT_POINTER: u32 = 128;
const EVENT_POINTER: u32 = 160;
const INPUT_POINTER: u32 = 1_024;
const INPUT_CAPACITY: i32 = 256;
const COUNT_OFFSET: u32 = 8;
const ACCOUNT_LENGTH: i32 = 10;
const STORED_LENGTH: i32 = 11;
const ACCOUNT_CAPACITY: i32 = 16;
const PUBKEY_LENGTH: i32 = 32;
const TOPIC_LENGTH: i32 = 8;
const EVENT_LENGTH: i32 = 2;
const DISCRIMINATOR_CALLDATA: i32 = 8;
const MINT_CALLDATA: i32 = 10;
const DESCRIPTOR_VERSION: &str = "1";
const DESCRIPTOR_KEYS: [&str; 6] = [
    "asset",
    "destination",
    "limit",
    "price",
    "program",
    "version",
];

/// The Anchor program the port reproduces, published beside the artifact as the
/// provenance of the descriptor.
pub const ANCHOR_SOURCE: &str = r#"use anchor_lang::prelude::*;
use anchor_lang::system_program;

declare_id!("MintL1m1tGuard1111111111111111111111111111111");

pub const COUNTER_SEED: &[u8] = b"mint_limit";
pub const GUARD_ID: u8 = 3;

#[program]
pub mod mint_limit {
    use super::*;

    pub fn mint(ctx: Context<Mint>, amount: u16) -> Result<u16> {
        let config = &ctx.accounts.candy_guard;
        require!(amount > 0, GuardError::ZeroAmount);
        require!(amount <= config.limit, GuardError::MintLimitExceeded);
        let counter = &mut ctx.accounts.mint_counter;
        let taken = counter
            .count
            .checked_add(amount)
            .ok_or(GuardError::MintLimitExceeded)?;
        require!(taken <= config.limit, GuardError::MintLimitExceeded);
        counter.count = taken;
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.destination.to_account_info(),
                },
            ),
            config
                .lamports
                .checked_mul(u64::from(amount))
                .ok_or(GuardError::MintLimitExceeded)?,
        )?;
        emit!(MintPerformed { count: taken });
        Ok(taken)
    }

    pub fn mint_count(ctx: Context<Query>) -> Result<u16> {
        Ok(ctx.accounts.mint_counter.count)
    }

    pub fn mint_remaining(ctx: Context<Query>) -> Result<u16> {
        let config = &ctx.accounts.candy_guard;
        Ok(config.limit.saturating_sub(ctx.accounts.mint_counter.count))
    }
}

#[derive(Accounts)]
pub struct Mint<'info> {
    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + 2,
        seeds = [
            COUNTER_SEED,
            &[GUARD_ID],
            payer.key().as_ref(),
            candy_guard.key().as_ref()
        ],
        bump
    )]
    pub mint_counter: Account<'info, MintCounter>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// CHECK: the configured payment destination, checked against the guard
    #[account(mut, address = candy_guard.destination)]
    pub destination: UncheckedAccount<'info>,
    pub candy_guard: Account<'info, GuardConfig>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Query<'info> {
    #[account(
        seeds = [
            COUNTER_SEED,
            &[GUARD_ID],
            payer.key().as_ref(),
            candy_guard.key().as_ref()
        ],
        bump
    )]
    pub mint_counter: Account<'info, MintCounter>,
    pub payer: Signer<'info>,
    pub candy_guard: Account<'info, GuardConfig>,
}

#[account]
pub struct MintCounter {
    pub count: u16,
}

#[account]
pub struct GuardConfig {
    pub guard_id: u8,
    pub limit: u16,
    pub lamports: u64,
    pub destination: Pubkey,
}

#[event]
pub struct MintPerformed {
    pub count: u16,
}

#[error_code]
pub enum GuardError {
    #[msg("mint amount must be greater than zero")]
    ZeroAmount,
    #[msg("mint limit exceeded")]
    MintLimitExceeded,
}
"#;

/// The pinned toolchain manifest published inside the archive. The build plan
/// carries its digest, so a verifier that rebuilds the source is rebuilding it
/// with exactly this emitter and this frozen ABI.
pub const TOOLCHAIN_MANIFEST: &str =
    "kit = layerx-porting-solana\nemitter = solana-port-emitter/1\nabi = layerx_v1/1\nsubset = deterministic-integer-wasm/1\n";

/// The pinned dependency lock published inside the archive.
pub const DEPENDENCY_LOCK: &str =
    "layerx-programs-runtime = 0.1.0\nlayerx-programs-registry = 0.1.0\n";

/// A complete mint-limit port: the `GuardConfig` a Solana deployment stores in
/// the candy guard account, resolved against `LayerX` account identifiers and
/// pinned into the module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintLimitPort {
    asset: [u8; PUBKEY_BYTES],
    destination: [u8; PUBKEY_BYTES],
    limit: u64,
    price: u64,
}

/// The guard configuration of one port, in `GuardConfig` declaration order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuardTerms {
    /// The 402LXP asset that stands in for lamports, since a program is paid in
    /// an authenticated asset rather than in the chain's native balance.
    pub asset: [u8; PUBKEY_BYTES],
    /// The account every mint payment credits.
    pub destination: [u8; PUBKEY_BYTES],
    /// The most mints one principal may take.
    pub limit: u64,
    /// The price of one mint.
    pub price: u64,
}

struct HostImports {
    storage_read: u32,
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
}

struct MintImports {
    storage_write: u32,
    event_emit: u32,
    transfer_402: u32,
    read_count: u32,
    account_tag: i64,
    key_length: i32,
}

struct DispatchTargets {
    mint_tag: i64,
    mint_count_tag: i64,
    mint_remaining_tag: i64,
    mint: u32,
    mint_count: u32,
    mint_remaining: u32,
}

impl MintLimitPort {
    /// Resolves a guard configuration into a port.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero asset and destination, a zero price, a limit
    /// outside the `u16` the counter account declares and any pair of terms
    /// whose product would leave the signed 64-bit domain the ABI carries
    /// amounts in.
    pub fn new(terms: GuardTerms) -> Result<Self, PortRefusal> {
        if terms.asset == [0u8; PUBKEY_BYTES] || terms.destination == [0u8; PUBKEY_BYTES] {
            return Err(PortRefusal::ZeroPubkey);
        }
        if terms.price == 0 || terms.limit == 0 || terms.limit > MAX_LIMIT_BOUND {
            return Err(PortRefusal::OutOfRange);
        }
        let ceiling = u64::try_from(i64::MAX).unwrap_or(u64::MAX);
        if terms
            .price
            .checked_mul(terms.limit)
            .is_none_or(|total| total > ceiling)
        {
            return Err(PortRefusal::OutOfRange);
        }
        Ok(Self {
            asset: terms.asset,
            destination: terms.destination,
            limit: terms.limit,
            price: terms.price,
        })
    }

    /// Returns the asset the guard is priced in.
    #[must_use]
    pub const fn asset(&self) -> [u8; PUBKEY_BYTES] {
        self.asset
    }

    /// Returns the account every mint payment credits.
    #[must_use]
    pub const fn destination(&self) -> [u8; PUBKEY_BYTES] {
        self.destination
    }

    /// Returns the most mints one principal may take.
    #[must_use]
    pub const fn limit(&self) -> u64 {
        self.limit
    }

    /// Returns the price of one mint.
    #[must_use]
    pub const fn price(&self) -> u64 {
        self.price
    }

    /// Returns the Solana derivation the counter account lives at, as the
    /// Anchor `seeds` constraint writes it.
    ///
    /// # Errors
    ///
    /// Refuses a seed path the declared bounds reject.
    pub fn solana_seeds(payer: Pubkey, candy_guard: Pubkey) -> Result<SeedPath, PortRefusal> {
        SeedPath::new(vec![
            COUNTER_SEED.to_vec(),
            vec![GUARD_ID],
            payer.bytes().to_vec(),
            candy_guard.bytes().to_vec(),
        ])
    }

    /// Returns the namespaced-storage key the ported counter occupies.
    ///
    /// The last two seeds carry the payer and the guard account, and namespaced
    /// storage is already partitioned by principal and by program, so those two
    /// seeds collapse away and no derivation runs at execution time.
    ///
    /// # Errors
    ///
    /// Refuses a framed key beyond the storage key bound.
    pub fn storage_key() -> Result<Vec<u8>, PortRefusal> {
        SeedPath::new(vec![COUNTER_SEED.to_vec(), vec![GUARD_ID]])?.storage_key()
    }

    /// Returns the exact price of taking `amount` mints.
    ///
    /// # Errors
    ///
    /// Refuses a zero amount and any amount beyond the declared limit, exactly
    /// as the Anchor `require!` statements do.
    pub fn amount_price(&self, amount: u64) -> Result<u128, PortRefusal> {
        if amount == 0 || amount > self.limit {
            return Err(PortRefusal::OutOfRange);
        }
        self.price
            .checked_mul(amount)
            .map(u128::from)
            .ok_or(PortRefusal::OutOfRange)
    }

    /// Returns the single 402LXP leg a mint produces, which is the ported
    /// `system_program::transfer` the payer signs.
    ///
    /// # Errors
    ///
    /// Refuses an amount outside the declared bounds.
    pub fn payment(&self, amount: u64) -> Result<Transfer402Plan, PortRefusal> {
        Transfer402Plan::new(self.asset, self.destination, self.amount_price(amount)?)
    }

    /// Returns the exact authority an activity must carry to take `amount`
    /// mints: namespaced reads and writes, event emission and one capped
    /// transfer to the configured destination. Nothing else is granted, and no
    /// grant admits a payment larger than the price.
    ///
    /// # Errors
    ///
    /// Refuses an amount outside the declared bounds or an invalid grant.
    pub fn mint_capabilities(&self, amount: u64) -> Result<CapabilitySet, PortRefusal> {
        let payment = self.payment(amount)?;
        Ok(CapabilitySet::new([
            Capability::StorageRead,
            Capability::StorageWrite,
            Capability::EmitEvent,
            payment.capability(),
        ])?)
    }

    /// Returns the event payload every mint emits under the Anchor event
    /// discriminator, carrying the new counter value.
    ///
    /// # Errors
    ///
    /// Refuses a count the counter account's `u16` cannot carry.
    pub fn mint_payload(count: u64) -> Result<Vec<u8>, PortRefusal> {
        let count = u16::try_from(count).map_err(|_| PortRefusal::OutOfRange)?;
        mint_event()?.data(&[FieldValue::U16(count)])
    }

    /// Encodes the canonical port descriptor, the document the reproducible
    /// build compiles.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "version = {DESCRIPTOR_VERSION}\nprogram = {PROGRAM_NAME}\nasset = {}\ndestination = {}\nlimit = {}\nprice = {}\n",
            hex::encode(&self.asset),
            hex::encode(&self.destination),
            self.limit,
            self.price,
        )
    }

    /// Parses the canonical port descriptor.
    ///
    /// # Errors
    ///
    /// Refuses malformed lines, unknown keys, repeated keys, missing keys, a
    /// foreign descriptor version, a foreign program name and any term the port
    /// constructor rejects.
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
            || field(&fields, "program")? != PROGRAM_NAME
        {
            return Err(PortRefusal::InvalidDescriptor);
        }
        Self::new(GuardTerms {
            asset: digest(&fields, "asset")?,
            destination: digest(&fields, "destination")?,
            limit: number(&fields, "limit")?,
            price: number(&fields, "price")?,
        })
    }

    /// Emits the deterministic `WebAssembly` module for this port.
    ///
    /// # Errors
    ///
    /// Refuses a seed path the declared bounds reject and a module beyond the
    /// runtime's declared byte bound.
    pub fn code(&self) -> Result<Vec<u8>, PortRefusal> {
        let key = Self::storage_key()?;
        let key_length = i32::try_from(key.len()).map_err(|_| PortRefusal::InvalidSeeds)?;
        let account_tag = i64::from_le_bytes(account_discriminator(COUNTER_ACCOUNT));
        let mut builder = ModuleBuilder::new(MEMORY_PAGES);
        let host_type = builder.signature(&[I32, I32, I32, I32], &[I32]);
        let transfer_type = builder.signature(&[I64, I64, I32, I32, I32, I32], &[I32]);
        let count_type = builder.signature(&[], &[I64]);
        let mint_type = builder.signature(&[I64], &[I64]);
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
        builder.segment(DESTINATION_POINTER, &self.destination);
        builder.segment(TOPIC_POINTER, &mint_event()?.topic());
        let read_count = emit_read_count(
            &mut builder,
            count_type,
            hosts.storage_read,
            key_length,
            account_tag,
        );
        let mint = self.emit_mint(
            &mut builder,
            mint_type,
            &MintImports {
                storage_write: hosts.storage_write,
                event_emit: hosts.event_emit,
                transfer_402: hosts.transfer_402,
                read_count,
                account_tag,
                key_length,
            },
        );
        let mint_count = emit_mint_count(&mut builder, count_type, read_count);
        let mint_remaining = self.emit_mint_remaining(&mut builder, count_type, mint_count);
        let reserve = emit_reserve(&mut builder, reserve_type);
        let entry = emit_call_entry(
            &mut builder,
            entry_type,
            &DispatchTargets {
                mint_tag: dispatch_word(MINT_INSTRUCTION),
                mint_count_tag: dispatch_word(MINT_COUNT_INSTRUCTION),
                mint_remaining_tag: dispatch_word(MINT_REMAINING_INSTRUCTION),
                mint,
                mint_count,
                mint_remaining,
            },
        );
        builder.export_memory(MEMORY_EXPORT);
        builder.export_function(MINT_EXPORT, mint);
        builder.export_function(MINT_COUNT_EXPORT, mint_count);
        builder.export_function(MINT_REMAINING_EXPORT, mint_remaining);
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

    fn emit_mint(&self, builder: &mut ModuleBuilder, signature: u32, imports: &MintImports) -> u32 {
        let limit = i64::try_from(self.limit).unwrap_or(i64::MAX);
        let price = i64::try_from(self.price).unwrap_or(i64::MAX);
        let mut code = Code::new();
        code.local_get(0);
        code.i64_const(1);
        code.op(I64_LT_S);
        code.trap_if();
        code.local_get(0);
        code.i64_const(limit);
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
        code.i64_const(limit);
        code.op(I64_GT_S);
        code.trap_if();
        code.pointer(ACCOUNT_POINTER);
        code.i64_const(imports.account_tag);
        code.memory(I64_STORE, 0);
        code.pointer(ACCOUNT_POINTER);
        code.local_get(3);
        code.op(I32_WRAP_I64);
        code.memory(I32_STORE16, COUNT_OFFSET);
        code.pointer(KEY_POINTER);
        code.i32_const(imports.key_length);
        code.pointer(ACCOUNT_POINTER);
        code.i32_const(ACCOUNT_LENGTH);
        code.call(imports.storage_write);
        code.trap_unless_ok();
        code.i64_const(0);
        code.local_get(0);
        code.i64_const(price);
        code.op(I64_MUL);
        code.pointer(ASSET_POINTER);
        code.i32_const(PUBKEY_LENGTH);
        code.pointer(DESTINATION_POINTER);
        code.i32_const(PUBKEY_LENGTH);
        code.call(imports.transfer_402);
        code.trap_unless_ok();
        code.pointer(EVENT_POINTER);
        code.local_get(3);
        code.op(I32_WRAP_I64);
        code.memory(I32_STORE16, 0);
        code.pointer(TOPIC_POINTER);
        code.i32_const(TOPIC_LENGTH);
        code.pointer(EVENT_POINTER);
        code.i32_const(EVENT_LENGTH);
        code.call(imports.event_emit);
        code.trap_unless_ok();
        code.local_get(3);
        code.end();
        builder.function(signature, &[(3, I64)], &code)
    }

    fn emit_mint_remaining(
        &self,
        builder: &mut ModuleBuilder,
        signature: u32,
        mint_count: u32,
    ) -> u32 {
        let limit = i64::try_from(self.limit).unwrap_or(i64::MAX);
        let mut code = Code::new();
        code.i64_const(limit);
        code.call(mint_count);
        code.op(I64_SUB);
        code.end();
        builder.function(signature, &[], &code)
    }
}

/// Returns the counter account's declared shape, whose eight-byte
/// discriminator an existing client already checks for.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn counter_schema() -> Result<AccountSchema, PortRefusal> {
    AccountSchema::new(
        COUNTER_ACCOUNT,
        vec![Field {
            name: COUNT_FIELD.to_owned(),
            kind: FieldType::U16,
        }],
    )
}

/// Returns the emitted event with its Anchor discriminator preserved, so an
/// existing indexer decodes the ported payload with the generated type.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn mint_event() -> Result<AnchorEvent, PortRefusal> {
    AnchorEvent::new(MINT_EVENT, vec![FieldType::U16])
}

/// Returns the mint instruction, whose eight-byte discriminator and `borsh`
/// argument an existing client already sends unchanged.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn mint_instruction() -> Result<InstructionAbi, PortRefusal> {
    InstructionAbi::new(MINT_INSTRUCTION, vec![FieldType::U16])
}

/// Returns the counter query instruction.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn mint_count_instruction() -> Result<InstructionAbi, PortRefusal> {
    InstructionAbi::new(MINT_COUNT_INSTRUCTION, Vec::new())
}

/// Returns the headroom query instruction.
///
/// # Errors
///
/// Refuses a schema the declared bounds reject.
pub fn mint_remaining_instruction() -> Result<InstructionAbi, PortRefusal> {
    InstructionAbi::new(MINT_REMAINING_INSTRUCTION, Vec::new())
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

/// Returns the stored value of a counter holding `count` mints. The value stays
/// the Anchor discriminator followed by the `borsh` field, so an exported
/// account imports byte for byte and a later read decodes with the generated
/// type.
///
/// # Errors
///
/// Refuses a count the counter account's `u16` cannot carry.
pub fn stored_counter(count: u64) -> Result<Vec<u8>, PortRefusal> {
    let count = u16::try_from(count).map_err(|_| PortRefusal::OutOfRange)?;
    counter_schema()?.encode(&[FieldValue::U16(count)])
}

fn dispatch_word(name: &str) -> i64 {
    i64::from_le_bytes(instruction_discriminator(name))
}

fn emit_read_count(
    builder: &mut ModuleBuilder,
    signature: u32,
    storage_read: u32,
    key_length: i32,
    account_tag: i64,
) -> u32 {
    let mut code = Code::new();
    code.pointer(KEY_POINTER);
    code.i32_const(key_length);
    code.pointer(ACCOUNT_POINTER);
    code.i32_const(ACCOUNT_CAPACITY);
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
    code.pointer(ACCOUNT_POINTER);
    code.memory(I64_LOAD, 0);
    code.i64_const(account_tag);
    code.op(I64_NE);
    code.trap_if();
    code.pointer(ACCOUNT_POINTER);
    code.memory(I32_LOAD16_U, COUNT_OFFSET);
    code.op(I64_EXTEND_I32_U);
    code.end();
    builder.function(signature, &[(1, I32)], &code)
}

fn emit_mint_count(builder: &mut ModuleBuilder, signature: u32, read_count: u32) -> u32 {
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
    code.i32_const(DISCRIMINATOR_CALLDATA);
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
    code.i64_const(targets.mint_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(MINT_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.local_get(0);
    code.memory(I32_LOAD16_U, COUNT_OFFSET);
    code.op(I64_EXTEND_I32_U);
    code.call(targets.mint);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i64_const(targets.mint_count_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(DISCRIMINATOR_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.mint_count);
    code.op(I32_WRAP_I64);
    code.op(RETURN);
    code.end();
    code.local_get(2);
    code.i64_const(targets.mint_remaining_tag);
    code.op(I64_EQ);
    code.block(IF, VOID_BLOCK);
    code.local_get(1);
    code.i32_const(DISCRIMINATOR_CALLDATA);
    code.op(I32_NE);
    code.trap_if();
    code.call(targets.mint_remaining);
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

fn digest(fields: &BTreeMap<&str, &str>, key: &str) -> Result<[u8; PUBKEY_BYTES], PortRefusal> {
    Ok(hex::decode_digest(field(fields, key)?)?)
}

fn number(fields: &BTreeMap<&str, &str>, key: &str) -> Result<u64, PortRefusal> {
    field(fields, key)?
        .parse()
        .map_err(|_| PortRefusal::InvalidDescriptor)
}
