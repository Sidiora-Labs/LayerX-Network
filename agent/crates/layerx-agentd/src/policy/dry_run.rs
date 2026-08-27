//! Side-effect-bounded dry-run evaluation and stable explanations.

use super::{
    evaluate, Decision, DecisionReason, EvaluationInput, Explanation, Outcome, PolicyRegistry,
    PolicySet,
};

pub const LOCAL_ALLOW_NOTICE: &str =
    "local policy has no objection; this is a local restriction result, not protocol authorisation";
pub const LOCAL_DENY_NOTICE: &str =
    "local restriction refused the request; this is not protocol authorisation and the protocol was not consulted";

/// Whether an explanation was produced for live or dry-run evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationMode {
    Live,
    DryRun,
}

/// Dry-run output; the audit insertion is its only mutable effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DryRunResult {
    pub decision: Decision,
    pub explanation: Explanation,
}

pub(crate) fn evaluate_dry_run(
    registry: &mut PolicyRegistry,
    request_id: [u8; 32],
    policy: &PolicySet,
    input: &EvaluationInput<'_>,
) -> DryRunResult {
    let decision = evaluate(policy, input);
    let explanation = explain_decision(&decision, EvaluationMode::DryRun);
    registry.record_decision(request_id, decision.clone());
    DryRunResult {
        decision,
        explanation,
    }
}

pub(crate) fn explain_decision(decision: &Decision, mode: EvaluationMode) -> Explanation {
    Explanation {
        schema_version: 1,
        mode,
        outcome: decision.outcome,
        policy_version: decision.policy_version.clone(),
        matched_rules: decision.matched_rules.clone(),
        deciding_rule: decision.deciding_rule.clone(),
        reason: decision.reason,
        authority_statement: match decision.outcome {
            Outcome::Allow => LOCAL_ALLOW_NOTICE,
            Outcome::Deny => LOCAL_DENY_NOTICE,
        },
    }
}

pub(crate) fn encode_explanation(explanation: &Explanation) -> Vec<u8> {
    let mut output = Vec::new();
    push_line(
        &mut output,
        "schema",
        &explanation.schema_version.to_string(),
    );
    push_line(
        &mut output,
        "mode",
        match explanation.mode {
            EvaluationMode::Live => "live",
            EvaluationMode::DryRun => "dry_run",
        },
    );
    push_line(
        &mut output,
        "outcome",
        match explanation.outcome {
            Outcome::Allow => "allow",
            Outcome::Deny => "deny",
        },
    );
    push_line(&mut output, "policy_version", &explanation.policy_version);
    push_line(
        &mut output,
        "matched_count",
        &explanation.matched_rules.len().to_string(),
    );
    for (index, rule) in explanation.matched_rules.iter().enumerate() {
        push_line(&mut output, &format!("matched_{index}"), rule);
    }
    push_line(
        &mut output,
        "deciding_rule",
        explanation.deciding_rule.as_deref().unwrap_or(""),
    );
    push_line(&mut output, "reason", reason_name(explanation.reason));
    push_line(
        &mut output,
        "authority_statement",
        explanation.authority_statement,
    );
    output
}

fn push_line(output: &mut Vec<u8>, key: &str, value: &str) {
    output.extend_from_slice(key.as_bytes());
    output.push(b'=');
    output.extend_from_slice(value.len().to_string().as_bytes());
    output.push(b':');
    output.extend_from_slice(value.as_bytes());
    output.push(b'\n');
}

const fn reason_name(reason: DecisionReason) -> &'static str {
    match reason {
        DecisionReason::PermittedByRule => "permitted_by_rule",
        DecisionReason::ExplicitDeny => "explicit_deny",
        DecisionReason::ApprovalRequired => "approval_required",
        DecisionReason::NoPermittingRule => "no_permitting_rule",
        DecisionReason::InvalidContext => "invalid_context",
        DecisionReason::EvaluationFailure => "evaluation_failure",
    }
}
