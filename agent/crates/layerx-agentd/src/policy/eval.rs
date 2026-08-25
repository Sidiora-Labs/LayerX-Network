//! Pure, bounded policy evaluation.

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

use layerx_types::ids::Did;

use crate::budget::ReconciliationState;
use crate::capability::{self, Capability, CapabilityId, PreparedIntent};
use crate::session::{SessionId, SessionRecord};
use crate::store::TenantId;

use super::{Decision, DecisionReason, Outcome};

/// Exact request fields consumed by policy evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRequest {
    pub activity_type: u16,
    pub counterparty: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub cumulative_amount: u128,
    pub cumulative_count: u64,
    pub purpose: String,
    pub core_sequence: u64,
    pub approval_present: bool,
}

/// Inclusive deterministic protocol-sequence window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceWindow {
    pub first: u64,
    pub last: u64,
}

/// Every constraint supported by the policy vocabulary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RuleConstraints {
    pub activity_types: BTreeSet<u16>,
    pub counterparties: BTreeSet<[u8; 32]>,
    pub assets: BTreeSet<[u8; 32]>,
    pub maximum_amount: Option<u128>,
    pub maximum_cumulative_amount: Option<u128>,
    pub maximum_cumulative_count: Option<u64>,
    pub purposes: BTreeSet<String>,
    pub capability_ids: BTreeSet<CapabilityId>,
    pub session_ids: BTreeSet<SessionId>,
    pub agents: BTreeSet<Did>,
    pub tenants: BTreeSet<TenantId>,
    pub sequence_window: Option<SequenceWindow>,
    pub required_approval: bool,
}

/// A matching rule either permits locally or refuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleEffect {
    Permit,
    Deny,
}

/// One named deterministic policy rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub id: String,
    pub effect: RuleEffect,
    pub constraints: RuleConstraints,
}

/// Loaded immutable policy version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicySet {
    pub version: String,
    pub rules: Vec<Rule>,
    pub evaluation_step_limit: u64,
}

/// Inputs admitted to deterministic local evaluation.
///
/// The budget context is private: callers either supply an opaque result issued
/// by protocol reconciliation or explicitly evaluate without one and receive a
/// fail-closed invalid-context denial.
pub struct EvaluationInput<'a> {
    pub request: &'a PolicyRequest,
    pub session: &'a SessionRecord,
    pub capability: &'a Capability,
    budget: BudgetContext<'a>,
}

#[derive(Clone, Copy)]
enum BudgetContext<'a> {
    Unavailable,
    Verified(&'a ReconciliationState),
}

impl<'a> EvaluationInput<'a> {
    /// Creates an input with no canonical protocol-budget authority.
    ///
    /// Evaluation of this input always denies with `InvalidContext`.
    #[must_use]
    pub const fn without_protocol_budget(
        request: &'a PolicyRequest,
        session: &'a SessionRecord,
        capability: &'a Capability,
    ) -> Self {
        Self {
            request,
            session,
            capability,
            budget: BudgetContext::Unavailable,
        }
    }

    /// Binds an opaque result issued only by protocol-budget reconciliation.
    #[must_use]
    pub const fn with_verified_protocol_budget(
        request: &'a PolicyRequest,
        session: &'a SessionRecord,
        capability: &'a Capability,
        budget: &'a ReconciliationState,
    ) -> Self {
        Self {
            request,
            session,
            capability,
            budget: BudgetContext::Verified(budget),
        }
    }

    const fn verified_budget(&self) -> Option<&ReconciliationState> {
        match self.budget {
            BudgetContext::Unavailable => None,
            BudgetContext::Verified(budget) => Some(budget),
        }
    }
}

/// Typed internal failure. Every variant maps to a denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationFailure {
    InvalidRule,
    StepLimitExceeded,
    Internal,
}

/// Rule-matching boundary used by the fail-closed evaluator.
pub trait RuleMatcher {
    /// Reports whether one rule's constraints all admit the evaluation input.
    ///
    /// # Errors
    ///
    /// Returns `EvaluationFailure::InvalidRule` for a malformed rule such as one with an empty
    /// identifier; every variant is caught by the evaluator and turned into a fail-closed deny.
    fn matches(&self, rule: &Rule, input: &EvaluationInput<'_>) -> Result<bool, EvaluationFailure>;
}

struct DeterministicMatcher;

impl RuleMatcher for DeterministicMatcher {
    fn matches(&self, rule: &Rule, input: &EvaluationInput<'_>) -> Result<bool, EvaluationFailure> {
        let constraints = &rule.constraints;
        let request = input.request;
        let session = &input.session.request;
        if rule.id.is_empty() {
            return Err(EvaluationFailure::InvalidRule);
        }
        Ok((constraints.activity_types.is_empty()
            || constraints.activity_types.contains(&request.activity_type))
            && (constraints.counterparties.is_empty()
                || constraints.counterparties.contains(&request.counterparty))
            && (constraints.assets.is_empty() || constraints.assets.contains(&request.asset))
            && constraints
                .maximum_amount
                .is_none_or(|maximum| request.amount <= maximum)
            && constraints
                .maximum_cumulative_amount
                .is_none_or(|maximum| request.cumulative_amount <= maximum)
            && constraints
                .maximum_cumulative_count
                .is_none_or(|maximum| request.cumulative_count <= maximum)
            && (constraints.purposes.is_empty() || constraints.purposes.contains(&request.purpose))
            && (constraints.capability_ids.is_empty()
                || constraints.capability_ids.contains(&input.capability.id))
            && (constraints.session_ids.is_empty()
                || constraints.session_ids.contains(&session.session_id))
            && (constraints.agents.is_empty() || constraints.agents.contains(&session.agent))
            && (constraints.tenants.is_empty() || constraints.tenants.contains(&session.tenant))
            && constraints.sequence_window.is_none_or(|window| {
                request.core_sequence >= window.first && request.core_sequence <= window.last
            }))
    }
}

pub(crate) fn evaluate_policy(policy: &PolicySet, input: &EvaluationInput<'_>) -> Decision {
    evaluate_with(policy, input, &DeterministicMatcher)
}

pub(crate) fn evaluate_with(
    policy: &PolicySet,
    input: &EvaluationInput<'_>,
    matcher: &dyn RuleMatcher,
) -> Decision {
    match catch_unwind(AssertUnwindSafe(|| evaluate_inner(policy, input, matcher))) {
        Ok(Ok(decision)) => decision,
        Ok(Err(_)) | Err(_) => Decision::deny(&policy.version, DecisionReason::EvaluationFailure),
    }
}

fn evaluate_inner(
    policy: &PolicySet,
    input: &EvaluationInput<'_>,
    matcher: &dyn RuleMatcher,
) -> Result<Decision, EvaluationFailure> {
    if !valid_context(policy, input) {
        return Ok(Decision::deny(
            &policy.version,
            DecisionReason::InvalidContext,
        ));
    }

    let mut ordered: Vec<&Rule> = policy.rules.iter().collect();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));
    let mut matched_rules = Vec::new();
    let mut permitted = Vec::new();
    let mut denied = Vec::new();
    let mut approval_missing = Vec::new();
    for (index, rule) in ordered.into_iter().enumerate() {
        let steps = u64::try_from(index + 1).map_err(|_| EvaluationFailure::StepLimitExceeded)?;
        if steps > policy.evaluation_step_limit {
            return Err(EvaluationFailure::StepLimitExceeded);
        }
        if !matcher.matches(rule, input)? {
            continue;
        }
        matched_rules.push(rule.id.clone());
        match rule.effect {
            RuleEffect::Deny => denied.push(rule.id.clone()),
            RuleEffect::Permit
                if rule.constraints.required_approval && !input.request.approval_present =>
            {
                approval_missing.push(rule.id.clone());
            }
            RuleEffect::Permit => permitted.push(rule.id.clone()),
        }
    }

    let (outcome, deciding_rule, reason) = if let Some(rule) = denied.first() {
        (
            Outcome::Deny,
            Some(rule.clone()),
            DecisionReason::ExplicitDeny,
        )
    } else if let Some(rule) = approval_missing.first() {
        (
            Outcome::Deny,
            Some(rule.clone()),
            DecisionReason::ApprovalRequired,
        )
    } else if let Some(rule) = permitted.first() {
        (
            Outcome::Allow,
            Some(rule.clone()),
            DecisionReason::PermittedByRule,
        )
    } else {
        (Outcome::Deny, None, DecisionReason::NoPermittingRule)
    };
    Ok(Decision {
        outcome,
        policy_version: policy.version.clone(),
        matched_rules,
        deciding_rule,
        reason,
    })
}

fn valid_context(policy: &PolicySet, input: &EvaluationInput<'_>) -> bool {
    let Some(budget) = input.verified_budget() else {
        return false;
    };
    let session = &input.session.request;
    if policy.version.is_empty()
        || policy.version != session.policy_version
        || !input.session.open
        || session.tenant != input.capability.tenant
        || input.request.amount > budget.remaining()
    {
        return false;
    }
    let intent = PreparedIntent {
        activity_type: input.request.activity_type,
        counterparty: input.request.counterparty,
        asset: input.request.asset,
        amount: input.request.amount,
        purpose: input.request.purpose.clone(),
        core_sequence: input.request.core_sequence,
        uses_in_window: input.request.cumulative_count,
    };
    capability::evaluate(input.capability, &intent) == capability::Decision::Allow
}
