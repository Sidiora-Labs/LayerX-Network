use std::collections::BTreeSet;

use layerx_agentd::capability::{Capability, CapabilityDimensions, CapabilityId, RateCeiling};
use layerx_agentd::identity::ProtocolAuthority;
use layerx_agentd::policy::{
    Outcome, PolicyRequest, PolicySet, Rule, RuleConstraints, RuleEffect, SequenceWindow,
};
use layerx_agentd::session::{OpenRequest, SessionId, SessionRecord};
use layerx_agentd::store::TenantId;
use layerx_types::ids::Did;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ConstraintDimension {
    ActivityType,
    Counterparty,
    Asset,
    Amount,
    CumulativeRate,
    Purpose,
    Capability,
    Session,
    Agent,
    Tenant,
    TimeWindow,
    RequiredApproval,
}

#[derive(Clone)]
pub struct CorpusCase {
    pub name: &'static str,
    pub policy: PolicySet,
    pub request: PolicyRequest,
    pub session: SessionRecord,
    pub capability: Capability,
    pub expected: Outcome,
    pub blocked_dimensions: BTreeSet<ConstraintDimension>,
}

pub fn required_blocked_constraints() -> BTreeSet<ConstraintDimension> {
    BTreeSet::from([
        ConstraintDimension::ActivityType,
        ConstraintDimension::Counterparty,
        ConstraintDimension::Asset,
        ConstraintDimension::Amount,
        ConstraintDimension::CumulativeRate,
        ConstraintDimension::Purpose,
        ConstraintDimension::Capability,
        ConstraintDimension::Session,
        ConstraintDimension::Agent,
        ConstraintDimension::Tenant,
        ConstraintDimension::TimeWindow,
        ConstraintDimension::RequiredApproval,
    ])
}

pub fn agent_policy_adversarial_corpus() -> Vec<CorpusCase> {
    let base = base_case();
    let mut cases = vec![base.clone()];

    let mut activity = base.clone();
    activity.name = "activity-type-widening";
    activity.request.activity_type = 8;
    deny_for(&mut activity, ConstraintDimension::ActivityType);
    cases.push(activity);

    let mut counterparty = base.clone();
    counterparty.name = "crafted-counterparty";
    counterparty.request.counterparty = [0xff; 32];
    deny_for(&mut counterparty, ConstraintDimension::Counterparty);
    cases.push(counterparty);

    let mut asset = base.clone();
    asset.name = "asset-substitution";
    asset.request.asset = [0xaa; 32];
    deny_for(&mut asset, ConstraintDimension::Asset);
    cases.push(asset);

    let mut amount = base.clone();
    amount.name = "amount-boundary-plus-one";
    amount.request.amount = 101;
    deny_for(&mut amount, ConstraintDimension::Amount);
    cases.push(amount);

    let mut cumulative = base.clone();
    cumulative.name = "cumulative-rate-producer-unavailable";
    deny_for(&mut cumulative, ConstraintDimension::CumulativeRate);
    cases.push(cumulative);

    let mut purpose = base.clone();
    purpose.name = "unicode-purpose-confusable";
    purpose.request.purpose = "reѕearch".to_owned();
    deny_for(&mut purpose, ConstraintDimension::Purpose);
    cases.push(purpose);

    let mut oversized = base.clone();
    oversized.name = "oversized-purpose";
    oversized.request.purpose = "research".repeat(8_192);
    deny_for(&mut oversized, ConstraintDimension::Purpose);
    cases.push(oversized);

    let mut capability = base.clone();
    capability.name = "capability-substitution";
    capability.capability.id = CapabilityId([0x44; 32]);
    deny_for(&mut capability, ConstraintDimension::Capability);
    cases.push(capability);

    let mut session = base.clone();
    session.name = "session-substitution";
    session.session.request.session_id = SessionId([0x33; 32]);
    deny_for(&mut session, ConstraintDimension::Session);
    cases.push(session);

    let mut agent = base.clone();
    agent.name = "agent-substitution";
    agent.session.request.agent = did(b"did:layerx:other-agent");
    deny_for(&mut agent, ConstraintDimension::Agent);
    cases.push(agent);

    let mut tenant = base.clone();
    tenant.name = "cross-tenant-substitution";
    tenant.session.request.tenant = tenant_id("tenant-b");
    deny_for(&mut tenant, ConstraintDimension::Tenant);
    cases.push(tenant);

    let mut time = base.clone();
    time.name = "outside-sequence-window";
    time.request.core_sequence = 151;
    deny_for(&mut time, ConstraintDimension::TimeWindow);
    cases.push(time);

    let mut approval = base;
    approval.name = "approval-evidence-integration-unavailable";
    deny_for(&mut approval, ConstraintDimension::RequiredApproval);
    cases.push(approval);

    cases
}

fn deny_for(case: &mut CorpusCase, dimension: ConstraintDimension) {
    case.expected = Outcome::Deny;
    case.blocked_dimensions = BTreeSet::from([dimension]);
}

fn base_case() -> CorpusCase {
    let tenant = tenant_id("tenant-a");
    let agent = did(b"did:layerx:policy-corpus");
    let session_id = SessionId([3; 32]);
    let capability_id = CapabilityId([4; 32]);
    let constraints = RuleConstraints {
        activity_types: BTreeSet::from([7]),
        counterparties: BTreeSet::from([[8; 32]]),
        assets: BTreeSet::from([[9; 32]]),
        maximum_amount: Some(100),
        maximum_cumulative_amount: Some(300),
        maximum_cumulative_count: Some(3),
        purposes: BTreeSet::from(["research".to_owned()]),
        capability_ids: BTreeSet::from([capability_id]),
        session_ids: BTreeSet::from([session_id]),
        agents: BTreeSet::from([agent.clone()]),
        tenants: BTreeSet::from([tenant.clone()]),
        sequence_window: Some(SequenceWindow {
            first: 100,
            last: 150,
        }),
        required_approval: true,
    };
    let policy = PolicySet {
        version: "corpus-v1".to_owned(),
        rules: vec![Rule {
            id: "intended-research".to_owned(),
            effect: RuleEffect::Permit,
            constraints,
        }],
        evaluation_step_limit: 10,
    };
    let request = PolicyRequest {
        activity_type: 7,
        counterparty: [8; 32],
        asset: [9; 32],
        amount: 100,
        purpose: "research".to_owned(),
        core_sequence: 120,
    };
    let session = SessionRecord {
        request: OpenRequest {
            session_id,
            token_id: [2; 32],
            tenant: tenant.clone(),
            agent,
            authority: ProtocolAuthority::SessionKey([1; 32]),
            permitted_activity_types: BTreeSet::from([7]),
            scopes: BTreeSet::from(["prepare".to_owned()]),
            expiry_sequence: 200,
            opening_client: "policy-harness".to_owned(),
            policy_version: "corpus-v1".to_owned(),
        },
        open: true,
        sequence: 0,
        budget_reserved: 0,
        subscription_cursor: 0,
    };
    let capability = Capability::new(
        capability_id,
        tenant,
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
    .unwrap_or_else(|error| panic!("corpus capability: {error:?}"));
    CorpusCase {
        name: "intended-control",
        policy,
        request,
        session,
        capability,
        expected: Outcome::Deny,
        blocked_dimensions: BTreeSet::new(),
    }
}

fn tenant_id(value: &str) -> TenantId {
    TenantId::new(value).unwrap_or_else(|error| panic!("corpus tenant: {error}"))
}

fn did(value: &[u8]) -> Did {
    Did::new(value).unwrap_or_else(|error| panic!("corpus DID: {error:?}"))
}
