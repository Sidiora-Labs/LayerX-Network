#![forbid(unsafe_code)]

#[path = "compile.rs"]
mod compiler;
mod disclosure;
pub mod golden;
mod reject;
#[cfg(feature = "test-vectors")]
pub mod vectors;
mod vocabulary;

pub use compiler::{compile, CompileError, CompileErrorReason, CompileField, CompiledIntent};
pub use disclosure::{DisclosureCheck, DisclosureCheckError, DisclosureField};
pub use reject::{inspect_intent, IntentHeader, IntentKindTag, RejectReason, RejectedIntent};

/// Names of the committed, scheduled fuzz surfaces shipped by this crate.
pub mod fuzz_targets {
    pub const INTENT_PARSING_AND_COMPILATION: &str = "intent";
}

pub use vocabulary::{
    BridgeDepositCredit, BridgeWithdrawRequest, BudgetCreate, BudgetDefund, BudgetFund,
    DidRegistration, EvmPayoutBinding, Intent, IntentError, IntentErrorReason, IntentField,
    IntentKind, IntentVersion, KeyRotation, LxpReceive, LxpSend, PayerGrantRegistration,
    RecoveryRegistration, SessionGrant, SessionRevoke,
};

/// Stable identity of the sole human-plane payload authority.
pub const CRATE_IDENTITY: &str = "layerx-intents";
