use std::collections::BTreeSet;

use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::policy::{
    activate, evaluate, load_policy_source, DecisionReason, EvaluationInput, Outcome,
    PolicyRegistry, PolicyRequest, PolicySet, PolicyValidationError, Rule, RuleConstraints,
    RuleEffect,
};
use layerx_agentd::session::{OpenRequest, SessionId, SessionRecord};
use layerx_agentd::store::TenantId;
use layerx_types::ids::Did;

fn policy(version: &str, effect: RuleEffect) -> PolicySet {
    PolicySet {
        version: version.to_owned(),
        rules: vec![Rule {
            id: format!("{version}-rule"),
            effect,
            constraints: RuleConstraints::default(),
        }],
        evaluation_step_limit: 10,
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error}"))
}

fn session(version: &str) -> SessionRecord {
    SessionRecord {
        request: OpenRequest {
            session_id: SessionId([3; 32]),
            token_id: [2; 32],
            tenant: tenant(),
            agent: Did::new(b"did:layerx:versioned")
                .unwrap_or_else(|error| panic!("did: {error:?}")),
            authority: ProtocolAuthority::SessionKey([1; 32]),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["prepare".to_owned()]),
            expiry_sequence: 200,
            opening_client: "version-test".to_owned(),
            policy_version: version.to_owned(),
        },
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
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
fn activation_only_changes_requests_received_after_it() {
    let mut registry = PolicyRegistry::new(policy("v1", RuleEffect::Permit))
        .unwrap_or_else(|error| panic!("registry: {error:?}"));
    let in_flight = registry.begin_request();
    let activation = activate(&mut registry, policy("v2", RuleEffect::Deny))
        .unwrap_or_else(|error| panic!("activate: {error:?}"));
    let later = registry.begin_request();
    assert_eq!(activation.previous_version, "v1");
    assert_eq!(in_flight.version(), "v1");
    assert_eq!(later.version(), "v2");

    let request = request();
    let capability = capability();
    let old_session = session("v1");
    let old_input =
        EvaluationInput::without_protocol_budget(&request, &old_session, &capability);
    let old_decision = evaluate(in_flight.policy(), &old_input);
    assert_eq!(old_decision.outcome, Outcome::Deny);
    assert_eq!(old_decision.reason, DecisionReason::InvalidContext);

    let new_session = session("v2");
    let new_input =
        EvaluationInput::without_protocol_budget(&request, &new_session, &capability);
    let new_decision = evaluate(later.policy(), &new_input);
    assert_eq!(new_decision.outcome, Outcome::Deny);
    assert_eq!(new_decision.reason, DecisionReason::InvalidContext);

    registry.record_decision([9; 32], new_decision.clone());
    assert_eq!(
        registry.audit_entry([9; 32]).map(|entry| &entry.decision),
        Some(&new_decision)
    );
    assert!(registry.retained("v1").is_some());
    assert!(registry.retained("v2").is_some());
}

#[test]
fn invalid_activation_preserves_the_previous_version() {
    let mut registry = PolicyRegistry::new(policy("v1", RuleEffect::Permit))
        .unwrap_or_else(|error| panic!("registry: {error:?}"));
    let mut invalid = policy("v2", RuleEffect::Deny);
    invalid.rules.push(invalid.rules[0].clone());
    assert!(matches!(
        activate(&mut registry, invalid),
        Err(PolicyValidationError::DuplicateRuleId(_))
    ));
    assert_eq!(registry.active_version(), "v1");
    assert!(registry.retained("v2").is_none());
}

#[test]
fn bounded_policy_source_loader_rejects_malformed_input() {
    let loaded = load_policy_source(b"version=v3\nsteps=2\nrule=permit-all,permit\n")
        .unwrap_or_else(|error| panic!("load: {error:?}"));
    assert_eq!(loaded.version, "v3");
    assert!(load_policy_source(b"version=v3\nsteps=nope\n").is_err());
    assert!(load_policy_source(&vec![b'x'; 1_048_577]).is_err());
}
