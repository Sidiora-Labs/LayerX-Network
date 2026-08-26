#![forbid(unsafe_code)]
//! Porting kit carrying a Solana program onto the `LayerX` programs ABI.
//!
//! The kit keeps three things exact and refuses to fake the rest. Account data
//! stays byte-identical - Anchor discriminator first, `borsh` fields after -
//! so an exported account snapshot imports byte for byte. Instruction and
//! event discriminators stay byte-identical, so an existing client or indexer
//! keeps matching. Everything the account model assumes and `LayerX` does not
//! provide - a program-held lamport balance, a program-derived signing
//! authority over somebody else's funds, an account another program may
//! mutate - is refused by name at translation time instead of being emulated
//! into something that looks like the original but no longer means the same
//! thing.
//!
//! The reference port in [`reference`] is a complete, runnable program: it
//! emits a real deterministic module, deploys through the real lifecycle, is
//! rebuilt from published source through the real reproducible-build pipeline
//! and executes under the real metered executor.

pub mod account;
pub mod anchor;
pub mod error;
pub mod hash;
pub mod monetary;
pub mod pubkey;
pub mod qualify;
pub mod reference;
pub mod shared_pool;
pub mod wasm;

pub use account::{
    ported_account, AccountMapping, AccountRole, AccountSchema, Field, FieldType, FieldValue,
};
pub use anchor::{
    account_discriminator, cross_program_invocation, event_discriminator,
    instruction_discriminator, AnchorEvent, CpiRequest, FailureMapping, InstructionAbi,
    RuntimeOutcome,
};
pub use error::PortRefusal;
pub use monetary::{
    translate_all, ProgramAccountTransferPlan, Transfer402Plan, TranslatedValueFlow, ValueFlow,
};
pub use pubkey::{per_signer_import, AccountHolder, MigrationCell, Pubkey, SeedPath};
pub use qualify::{
    build_plan, deploy_and_verify, execute_mint, execute_mint_count, execute_mint_remaining,
    import_accounts, published_source, settle, source_archive, validated_module, AbsentReceipts,
    DeployedGuard, Invocation, PortBuildRunner, Publication,
};
pub use reference::{GuardTerms, MintLimitPort};

/// Identifies the Solana porting kit and the ABI version it targets.
#[must_use]
pub const fn programs_porting_solana() -> &'static str {
    "programs/porting/solana targeting layerx_v1 ABI version 1"
}
