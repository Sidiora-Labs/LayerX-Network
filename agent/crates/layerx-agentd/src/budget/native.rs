//! Native Budget evidence and timestamp-bound recovery facts.

use crate::protocol_evidence::{EvidenceAuthority, RawActivityReceiptEvidence};
use layerx_types::ids::Did;

/// Budget identity selected by the daemon from its persisted managed-agent record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBudgetBinding {
    pub budget_id: [u8; 32],
    pub owner_account: [u8; 32],
    pub budget_account: [u8; 32],
    pub asset: [u8; 32],
    pub owner_did: Did,
    pub owner_public_key: [u8; 32],
    pub period_start_ms: u64,
    pub period_length_ms: u64,
    pub expiry_ms: u64,
}

impl NativeBudgetBinding {
    pub(crate) fn advances(&self, next: &Self) -> bool {
        self.budget_id == next.budget_id
            && self.owner_account == next.owner_account
            && self.budget_account == next.budget_account
            && self.asset == next.asset
            && self.owner_did == next.owner_did
            && self.owner_public_key == next.owner_public_key
            && self.period_length_ms == next.period_length_ms
            && self.expiry_ms == next.expiry_ms
            && self.period_length_ms > 0
            && next
                .period_start_ms
                .checked_sub(self.period_start_ms)
                .is_some_and(|delta| delta % self.period_length_ms == 0)
    }
}

/// Exported canonical value and its complete native account proof.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeAccountCandidate {
    pub canonical_value: Vec<u8>,
    pub proof_material: Vec<u8>,
}

/// One checkpoint-bound native Budget, owner and debit-account proof bundle.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeBudgetCandidate {
    pub canonical_record: Vec<u8>,
    pub module_proof_material: Vec<u8>,
    pub owner: NativeAccountCandidate,
    pub account: NativeAccountCandidate,
    pub source: Option<NativeAccountCandidate>,
    pub canonical_header: Vec<u8>,
    pub checkpoint_id: [u8; 32],
}

/// Complete history from a proven zero-spent baseline in the same native period.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBudgetRecoveryEvidence {
    pub baseline: NativeBudgetCandidate,
    pub current: NativeBudgetCandidate,
    pub history: Vec<RawActivityReceiptEvidence>,
    pub maintenance: Vec<crate::protocol_evidence::RawReceiptEvidence>,
}

/// An authenticated Budget Spend outcome retained for exact-once recovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBudgetOutcome {
    pub(crate) binding: NativeBudgetBinding,
    pub(crate) activity_id: [u8; 32],
    pub(crate) receipt_digest: [u8; 32],
    pub(crate) amount: u128,
    pub(crate) succeeded: bool,
    pub(crate) timestamp_ms: u64,
    pub(crate) sequence: u64,
    pub(crate) idempotency_key: [u8; 32],
    pub(crate) result_code: i32,
    pub(crate) canonical_receipt: Vec<u8>,
    pub(crate) proof: RawActivityReceiptEvidence,
}

impl NativeBudgetOutcome {
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }
    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }
    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }
    #[must_use]
    pub const fn succeeded(&self) -> bool {
        self.succeeded
    }
}

/// Opaque native reconciliation, issued only after all proof and history checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeBudgetReconciliation {
    pub(crate) binding: NativeBudgetBinding,
    pub(crate) authority: EvidenceAuthority,
    pub(crate) spent: u128,
    pub(crate) remaining: u128,
    pub(crate) observed_sequence: u64,
    pub(crate) timestamp_ms: u64,
    pub(crate) period_end_ms: u64,
    pub(crate) checkpoint_id: [u8; 32],
    pub(crate) outcomes: Vec<NativeBudgetOutcome>,
    pub(crate) write_eligible: bool,
}

impl NativeBudgetReconciliation {
    #[must_use]
    pub fn binding(&self) -> &NativeBudgetBinding {
        &self.binding
    }
    #[must_use]
    pub const fn spent(&self) -> u128 {
        self.spent
    }
    #[must_use]
    pub const fn remaining(&self) -> u128 {
        self.remaining
    }
    #[must_use]
    pub const fn observed_sequence(&self) -> u64 {
        self.observed_sequence
    }
    #[must_use]
    pub const fn timestamp_ms(&self) -> u64 {
        self.timestamp_ms
    }
    #[must_use]
    pub const fn period_end_ms(&self) -> u64 {
        self.period_end_ms
    }
    #[must_use]
    pub const fn checkpoint_id(&self) -> [u8; 32] {
        self.checkpoint_id
    }
    #[must_use]
    pub const fn write_eligible(&self) -> bool {
        self.write_eligible
    }
    #[must_use]
    pub fn outcomes(&self) -> &[NativeBudgetOutcome] {
        &self.outcomes
    }
}

/// Exact native reconciliation refusal, without accepting legacy raw leaves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeBudgetError {
    Binding,
    Authority,
    StateProof,
    AccountProof,
    Record,
    Checkpoint,
    Window,
    Baseline,
    History,
    Activity,
    Receipt,
    Arithmetic,
    Consumption,
    Store,
    Unavailable,
    Outbox,
}
