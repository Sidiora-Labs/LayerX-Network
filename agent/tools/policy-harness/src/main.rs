use std::collections::BTreeSet;

use layerx_agentd::policy::{evaluate, DecisionReason, EvaluationInput, Outcome, PolicySet};

#[path = "../../../tests/policy/adversarial.rs"]
mod adversarial;

use adversarial::{
    agent_policy_adversarial_corpus, required_blocked_constraints, ConstraintDimension,
};

#[derive(Clone, Debug, Eq, PartialEq)]
struct HarnessDecision {
    case: &'static str,
    outcome: Outcome,
    reason: DecisionReason,
    policy_version: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HarnessReport {
    decisions: Vec<HarnessDecision>,
    blocked_constraints: BTreeSet<ConstraintDimension>,
}

fn agent_policy_harness() -> Result<HarnessReport, String> {
    let corpus = agent_policy_adversarial_corpus();
    let mut decisions = Vec::new();
    let mut blocked_constraints = BTreeSet::new();
    for case in &corpus {
        blocked_constraints.extend(case.blocked_dimensions.iter().copied());
        let input = EvaluationInput::without_protocol_budget(
            &case.request,
            &case.session,
            &case.capability,
        );
        let decision = evaluate(&case.policy, &input);
        if decision.outcome != case.expected {
            return Err(format!(
                "case {} produced {:?}, expected {:?}",
                case.name, decision.outcome, case.expected
            ));
        }
        let deny_by_default = evaluate(
            &PolicySet {
                version: case.policy.version.clone(),
                rules: Vec::new(),
                evaluation_step_limit: 1,
            },
            &input,
        );
        if deny_by_default.outcome != Outcome::Deny {
            return Err(format!("deny-by-default escaped for {}", case.name));
        }
        decisions.push(HarnessDecision {
            case: case.name,
            outcome: decision.outcome,
            reason: decision.reason,
            policy_version: decision.policy_version,
        });
    }
    let required = required_blocked_constraints();
    if blocked_constraints != required {
        let missing: Vec<_> = required.difference(&blocked_constraints).copied().collect();
        return Err(format!("blocked constraint corpus missing: {missing:?}"));
    }
    Ok(HarnessReport {
        decisions,
        blocked_constraints,
    })
}

fn main() {
    let report = agent_policy_harness().unwrap_or_else(|error| panic!("policy harness: {error}"));
    for decision in &report.decisions {
        println!(
            "case={} outcome={:?} reason={:?} policy_version={}",
            decision.case, decision.outcome, decision.reason, decision.policy_version
        );
    }
    println!(
        "cases={} constraints_blocked={}",
        report.decisions.len(),
        report.blocked_constraints.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_budget_denies_the_complete_adversarial_corpus() {
        let report = agent_policy_harness().unwrap_or_else(|error| panic!("harness: {error}"));
        assert_eq!(report.blocked_constraints, required_blocked_constraints());
        assert_eq!(
            report
                .decisions
                .iter()
                .filter(|decision| decision.outcome == Outcome::Allow)
                .map(|decision| decision.case)
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
    }
}
