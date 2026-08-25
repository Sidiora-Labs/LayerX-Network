//! Deterministic local policy restrictions for daemon write requests.

pub mod approval;
#[path = "dry_run.rs"]
mod dry_run_evaluation;
mod eval;
#[path = "version.rs"]
mod versioning;

pub use dry_run_evaluation::{DryRunResult, EvaluationMode};
pub use eval::{
    EvaluationFailure, EvaluationInput, PolicyRequest, PolicySet, Rule, RuleConstraints,
    RuleEffect, RuleMatcher, SequenceWindow,
};
pub use versioning::{
    load_policy_source, Activation, PolicyAuditEntry, PolicyRegistry, PolicySnapshot,
    PolicySourceError, PolicyValidationError, MAX_POLICY_SOURCE_BYTES,
};

/// The local policy outcome. An allow is never protocol authorisation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Allow,
    Deny,
}

/// Stable reason for the final policy result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionReason {
    PermittedByRule,
    ExplicitDeny,
    ApprovalRequired,
    NoPermittingRule,
    InvalidContext,
    EvaluationFailure,
}

/// Complete deterministic policy decision, including the loaded version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Decision {
    pub outcome: Outcome,
    pub policy_version: String,
    pub matched_rules: Vec<String>,
    pub deciding_rule: Option<String>,
    pub reason: DecisionReason,
}

/// Stable, machine-readable explanation of one policy decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Explanation {
    pub schema_version: u16,
    pub mode: EvaluationMode,
    pub outcome: Outcome,
    pub policy_version: String,
    pub matched_rules: Vec<String>,
    pub deciding_rule: Option<String>,
    pub reason: DecisionReason,
    pub authority_statement: &'static str,
}

impl Explanation {
    /// Encodes a deterministic length-delimited record suitable for audit diffs.
    #[must_use]
    pub fn machine_bytes(&self) -> Vec<u8> {
        dry_run_evaluation::encode_explanation(self)
    }
}

impl Decision {
    fn deny(policy_version: &str, reason: DecisionReason) -> Self {
        Self {
            outcome: Outcome::Deny,
            policy_version: policy_version.to_owned(),
            matched_rules: Vec::new(),
            deciding_rule: None,
            reason,
        }
    }
}

/// Evaluates a request with no ambient inputs and denies on every failure.
/// An input without an opaque verified protocol-budget result is invalid.
#[must_use]
pub fn evaluate(policy: &PolicySet, input: &EvaluationInput<'_>) -> Decision {
    eval::evaluate_policy(policy, input)
}

/// Evaluates through an alternate matcher while retaining fail-closed containment.
#[must_use]
pub fn evaluate_with_matcher(
    policy: &PolicySet,
    input: &EvaluationInput<'_>,
    matcher: &dyn RuleMatcher,
) -> Decision {
    eval::evaluate_with(policy, input, matcher)
}

/// Validates a policy set before it can become active.
///
/// # Errors
///
/// Returns the first violation: an empty version, a zero step limit, a step limit below the rule
/// count, an empty or duplicated rule identifier, an empty purpose, or an inverted sequence
/// window.
pub fn validate(policy: &PolicySet) -> Result<(), PolicyValidationError> {
    versioning::validate_policy(policy)
}

/// Atomically activates a validated policy version for future requests.
///
/// # Errors
///
/// Returns the validation failure of the offered set, or `VersionAlreadyRetained` when the
/// registry already holds that version.
pub fn activate(
    registry: &mut PolicyRegistry,
    policy: PolicySet,
) -> Result<Activation, PolicyValidationError> {
    versioning::activate_policy(registry, policy)
}

/// Evaluates without creating a preparation, signature or submission.
pub fn dry_run(
    registry: &mut PolicyRegistry,
    request_id: [u8; 32],
    policy: &PolicySet,
    input: &EvaluationInput<'_>,
) -> DryRunResult {
    dry_run_evaluation::evaluate_dry_run(registry, request_id, policy, input)
}

/// Builds the same stable explanation for a live decision.
#[must_use]
pub fn explain(decision: &Decision, mode: EvaluationMode) -> Explanation {
    dry_run_evaluation::explain_decision(decision, mode)
}
