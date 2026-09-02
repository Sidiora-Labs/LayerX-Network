use std::collections::BTreeSet;

use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::policy::{
    dry_run, evaluate, explain, DecisionReason, EvaluationInput, EvaluationMode, Outcome,
    PolicyRegistry, PolicyRequest, PolicySet, Rule, RuleConstraints, RuleEffect,
};
use layerx_agentd::session::{OpenRequest, SessionId, SessionRecord};
use layerx_agentd::store::TenantId;
use layerx_types::ids::Did;

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn policy() -> PolicySet {
    PolicySet {
        version: "v1".to_owned(),
        rules: vec![Rule {
            id: "permit-research".to_owned(),
            effect: RuleEffect::Permit,
            constraints: RuleConstraints::default(),
        }],
        evaluation_step_limit: 10,
    }
}

fn session() -> SessionRecord {
    SessionRecord {
        request: OpenRequest {
            session_id: SessionId([3; 32]),
            token_id: [2; 32],
            tenant: tenant(),
            agent: Did::new(b"did:layerx:dry-run").unwrap_or_else(|error| panic!("did: {error:?}")),
            authority: ProtocolAuthority::SessionKey([1; 32]),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["prepare".to_owned()]),
            expiry_sequence: 200,
            opening_client: "dry-run-test".to_owned(),
            policy_version: "v1".to_owned(),
        },
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
        generation: 1,
        retired_token_ids: BTreeSet::new(),
    }
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

#[test]
fn dry_run_matches_live_fail_closed_budget_result_and_adds_its_audit_entry() {
    let policy = policy();
    let mut registry =
        PolicyRegistry::new(policy.clone()).unwrap_or_else(|error| panic!("registry: {error:?}"));
    let request = request();
    let session = session();
    let capability = capability();
    let input = EvaluationInput::without_protocol_budget(&request, &session, &capability);

    let live = evaluate(&policy, &input);
    let result = dry_run(&mut registry, [7; 32], &policy, &input);
    assert_eq!(result.decision, live);
    assert_eq!(result.decision.outcome, Outcome::Deny);
    assert_eq!(result.decision.reason, DecisionReason::InvalidContext);
    assert_eq!(
        registry.audit_entry([7; 32]).map(|entry| &entry.decision),
        Some(&live)
    );
    assert!(result
        .explanation
        .authority_statement
        .contains("local restriction"));
    assert!(result
        .explanation
        .authority_statement
        .contains("not protocol authorisation"));
}

#[test]
fn explanation_is_stable_machine_readable_and_complete() {
    let policy = policy();
    let request = request();
    let session = session();
    let capability = capability();
    let input = EvaluationInput::without_protocol_budget(&request, &session, &capability);
    let decision = evaluate(&policy, &input);
    let first = explain(&decision, EvaluationMode::Live);
    let second = explain(&decision, EvaluationMode::Live);
    assert_eq!(first, second);
    assert!(first.matched_rules.is_empty());
    assert_eq!(first.deciding_rule, None);
    let encoded = String::from_utf8(first.machine_bytes())
        .unwrap_or_else(|error| panic!("machine explanation: {error}"));
    assert!(encoded.contains("policy_version=2:v1\n"));
    assert!(encoded.contains("matched_count=1:0\n"));
    assert!(encoded.contains("reason=15:invalid_context\n"));
}
