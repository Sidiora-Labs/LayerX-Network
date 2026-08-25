use std::collections::BTreeSet;

use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::policy::{
    evaluate, evaluate_with_matcher, DecisionReason, EvaluationFailure, EvaluationInput, Outcome,
    PolicyRequest, PolicySet, Rule, RuleConstraints, RuleEffect, RuleMatcher, SequenceWindow,
};
use layerx_agentd::session::{OpenRequest, SessionId, SessionRecord};
use layerx_agentd::store::TenantId;
use layerx_types::ids::Did;

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn agent() -> Did {
    Did::new(b"did:layerx:policy-agent").unwrap_or_else(|error| panic!("did: {error:?}"))
}

fn capability() -> Capability {
    Capability::new(
        CapabilityId([4; 32]),
        tenant(),
        CapabilityDimensions {
            activity_types: BTreeSet::from([7]),
            counterparties: BTreeSet::from([[8; 32]]),
            assets: BTreeSet::from([[9; 32]]),
            amount_ceiling: 500,
            rate_ceiling: RateCeiling {
                maximum_uses: 10,
                window_sequences: 100,
            },
            purposes: BTreeSet::from(["research".to_owned()]),
            expiry_sequence: 200,
        },
    )
    .unwrap_or_else(|error| panic!("capability: {error:?}"))
}

fn session() -> SessionRecord {
    SessionRecord {
        request: OpenRequest {
            session_id: SessionId([3; 32]),
            token_id: [2; 32],
            tenant: tenant(),
            agent: agent(),
            authority: ProtocolAuthority::SessionKey([1; 32]),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["prepare".to_owned()]),
            expiry_sequence: 200,
            opening_client: "policy-test".to_owned(),
            policy_version: "policy-v1".to_owned(),
        },
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
    }
}

fn request() -> PolicyRequest {
    PolicyRequest {
        activity_type: 7,
        counterparty: [8; 32],
        asset: [9; 32],
        amount: 100,
        purpose: "research".to_owned(),
        core_sequence: 120,
    }
}

fn complete_constraints() -> RuleConstraints {
    RuleConstraints {
        activity_types: BTreeSet::from([7]),
        counterparties: BTreeSet::from([[8; 32]]),
        assets: BTreeSet::from([[9; 32]]),
        maximum_amount: Some(100),
        maximum_cumulative_amount: Some(250),
        maximum_cumulative_count: Some(3),
        purposes: BTreeSet::from(["research".to_owned()]),
        capability_ids: BTreeSet::from([CapabilityId([4; 32])]),
        session_ids: BTreeSet::from([SessionId([3; 32])]),
        agents: BTreeSet::from([agent()]),
        tenants: BTreeSet::from([tenant()]),
        sequence_window: Some(SequenceWindow {
            first: 100,
            last: 199,
        }),
        required_approval: true,
    }
}

fn policy(rules: Vec<Rule>) -> PolicySet {
    PolicySet {
        version: "policy-v1".to_owned(),
        rules,
        evaluation_step_limit: 100,
    }
}

#[test]
fn unavailable_protocol_budget_denies_before_any_permit_rule() {
    let session = session();
    let capability = capability();
    let request = request();
    let input = EvaluationInput::without_protocol_budget(&request, &session, &capability);
    let empty = evaluate(&policy(Vec::new()), &input);
    assert_eq!(empty.outcome, Outcome::Deny);
    assert_eq!(empty.reason, DecisionReason::InvalidContext);

    let allowed = evaluate(
        &policy(vec![Rule {
            id: "permit-research".to_owned(),
            effect: RuleEffect::Permit,
            constraints: complete_constraints(),
        }]),
        &input,
    );
    assert_eq!(allowed.outcome, Outcome::Deny);
    assert_eq!(allowed.reason, DecisionReason::InvalidContext);
    assert_eq!(allowed.policy_version, "policy-v1");
}

#[test]
fn unavailable_budget_denial_is_stable_across_conflicting_rules() {
    let session = session();
    let capability = capability();
    let request = request();
    let input = EvaluationInput::without_protocol_budget(&request, &session, &capability);
    let rules = vec![
        Rule {
            id: "z-permit".to_owned(),
            effect: RuleEffect::Permit,
            constraints: complete_constraints(),
        },
        Rule {
            id: "a-deny".to_owned(),
            effect: RuleEffect::Deny,
            constraints: complete_constraints(),
        },
    ];
    let expected = evaluate(&policy(rules.clone()), &input);
    assert_eq!(expected.outcome, Outcome::Deny);
    assert_eq!(expected.reason, DecisionReason::InvalidContext);
    assert_eq!(expected.deciding_rule, None);
    for _ in 0..2_000 {
        assert_eq!(evaluate(&policy(rules.clone()), &input), expected);
    }
}

struct PanickingMatcher;

impl RuleMatcher for PanickingMatcher {
    fn matches(
        &self,
        _rule: &Rule,
        _input: &EvaluationInput<'_>,
    ) -> Result<bool, EvaluationFailure> {
        panic!("injected policy engine panic")
    }
}

#[test]
fn unavailable_budget_precedes_matcher_and_step_evaluation() {
    let session = session();
    let capability = capability();
    let request = request();
    let input = EvaluationInput::without_protocol_budget(&request, &session, &capability);
    let permit = Rule {
        id: "permit".to_owned(),
        effect: RuleEffect::Permit,
        constraints: complete_constraints(),
    };
    let panicked = evaluate_with_matcher(&policy(vec![permit.clone()]), &input, &PanickingMatcher);
    assert_eq!(panicked.outcome, Outcome::Deny);
    assert_eq!(panicked.reason, DecisionReason::InvalidContext);

    let mut bounded = policy(vec![permit]);
    bounded.evaluation_step_limit = 0;
    let timed_out = evaluate(&bounded, &input);
    assert_eq!(timed_out.outcome, Outcome::Deny);
    assert_eq!(timed_out.reason, DecisionReason::InvalidContext);
}
