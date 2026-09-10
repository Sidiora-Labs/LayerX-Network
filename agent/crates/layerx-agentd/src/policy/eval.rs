//! Pure, bounded policy evaluation.

use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};

use layerx_types::ids::Did;

use crate::budget::ReconciliationState;
use crate::capability::{self, Capability, CapabilityId, PreparedIntent};
use crate::protocol_evidence::AuthenticatedCumulativeUse;
use crate::session::{SessionId, SessionRecord};
use crate::store::TenantId;

use super::{Decision, DecisionReason, Outcome};

/// Current activity intent fields consumed by policy evaluation.
///
/// Historical spend, usage counts, and approval status are deliberately absent:
/// those facts must come from daemon-owned verified context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRequest {
    pub activity_type: u16,
    pub counterparty: [u8; 32],
    pub asset: [u8; 32],
    pub amount: u128,
    pub purpose: String,
    pub core_sequence: u64,
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
/// Cumulative contexts are private. Callers may supply only opaque facts issued
/// by protocol reconciliation or complete authenticated activity/receipt
/// windows; arbitrary caller-provided totals are not accepted.
pub struct EvaluationInput<'a> {
    pub request: &'a PolicyRequest,
    pub session: &'a SessionRecord,
    pub capability: &'a Capability,
    cumulative: CumulativeContext<'a>,
}

#[derive(Clone, Copy)]
enum CumulativeContext<'a> {
    Unavailable,
    ProtocolBudget(&'a ReconciliationState),
    Authenticated(&'a AuthenticatedCumulativeUse),
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
            cumulative: CumulativeContext::Unavailable,
        }
    }

    /// Binds an opaque result issued only by protocol-budget reconciliation.
    /// Cumulative count and approval evidence remain unavailable, so this input
    /// cannot currently yield an allow decision.
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
            cumulative: CumulativeContext::ProtocolBudget(budget),
        }
    }

    /// Binds cumulative amount and count issued from a complete authenticated
    /// protocol-sequence window.
    #[must_use]
    pub const fn with_authenticated_cumulative_use(
        request: &'a PolicyRequest,
        session: &'a SessionRecord,
        capability: &'a Capability,
        cumulative: &'a AuthenticatedCumulativeUse,
    ) -> Self {
        Self {
            request,
            session,
            capability,
            cumulative: CumulativeContext::Authenticated(cumulative),
        }
    }

    const fn authenticated_cumulative_amount(&self) -> Option<u128> {
        match self.cumulative {
            CumulativeContext::Authenticated(cumulative) => Some(cumulative.amount()),
            CumulativeContext::ProtocolBudget(budget) => Some(budget.protocol_consumed()),
            CumulativeContext::Unavailable => None,
        }
    }

    const fn authenticated_cumulative_count(&self) -> Option<u64> {
        match self.cumulative {
            CumulativeContext::Authenticated(cumulative) => Some(cumulative.count()),
            CumulativeContext::ProtocolBudget(_) | CumulativeContext::Unavailable => None,
        }
    }

    const fn authenticated_window(&self) -> Option<&AuthenticatedCumulativeUse> {
        match self.cumulative {
            CumulativeContext::Authenticated(cumulative) => Some(cumulative),
            CumulativeContext::ProtocolBudget(_) | CumulativeContext::Unavailable => None,
        }
    }
}

/// Typed internal failure. Every variant maps to a denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationFailure {
    InvalidRule,
    ProtocolBudgetUnavailable,
    CumulativeCountUnavailable,
    StepLimitExceeded,
    Internal,
}

/// Rule-matching boundary used by the fail-closed evaluator.
pub trait RuleMatcher {
    /// Reports whether one rule's constraints all admit the evaluation input.
    ///
    /// # Errors
    ///
    /// Returns typed missing-authority failures for cumulative facts which have
    /// no daemon-owned producer, or `InvalidRule` for malformed rules. Every
    /// variant is caught by the evaluator and turned into a fail-closed deny.
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
        let cumulative_amount = input
            .authenticated_cumulative_amount()
            .ok_or(EvaluationFailure::ProtocolBudgetUnavailable)?;
        if constraints.maximum_cumulative_count.is_some()
            && input.authenticated_cumulative_count().is_none()
        {
            return Err(EvaluationFailure::CumulativeCountUnavailable);
        }
        Ok((constraints.activity_types.is_empty()
            || constraints.activity_types.contains(&request.activity_type))
            && (constraints.counterparties.is_empty()
                || constraints.counterparties.contains(&request.counterparty))
            && (constraints.assets.is_empty() || constraints.assets.contains(&request.asset))
            && constraints
                .maximum_amount
                .is_none_or(|maximum| request.amount <= maximum)
            && constraints.maximum_cumulative_amount.is_none_or(|maximum| {
                cumulative_amount
                    .checked_add(request.amount)
                    .is_some_and(|projected| projected <= maximum)
            })
            && constraints.maximum_cumulative_count.is_none_or(|maximum| {
                input
                    .authenticated_cumulative_count()
                    .and_then(|count| count.checked_add(1))
                    .is_some_and(|projected| projected <= maximum)
            })
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
            RuleEffect::Permit if rule.constraints.required_approval => {
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
    let Some(cumulative) = input.authenticated_window() else {
        return false;
    };
    let Some(cumulative_count) = input.authenticated_cumulative_count() else {
        return false;
    };
    let session = &input.session.request;
    if policy.version.is_empty()
        || policy.version != session.policy_version
        || !input.session.open
        || session.tenant != input.capability.tenant
        || cumulative.actor() != &session.agent
    {
        return false;
    }
    let Some(expected_last) = input.request.core_sequence.checked_sub(1) else {
        return false;
    };
    let Some(expected_first) = input
        .request
        .core_sequence
        .checked_sub(input.capability.dimensions.rate_ceiling.window_sequences)
    else {
        return false;
    };
    if cumulative.window()
        != (crate::protocol_evidence::CumulativeUseWindow {
            first: expected_first,
            last: expected_last,
        })
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
        uses_in_window: cumulative_count,
    };
    capability::evaluate(input.capability, &intent) == capability::Decision::Allow
}
