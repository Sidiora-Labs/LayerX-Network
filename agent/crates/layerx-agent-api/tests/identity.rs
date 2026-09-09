use layerx_agent_api::identity::{
    ActivityType, AgentDid, Asset, AuthorityDescription, AuthorityRef, BudgetCreate,
    BudgetEnforcement, BudgetFund, BudgetId, CapabilityDimensions, ClientId, ContractError,
    ExplicitSet, PolicyVersion, SessionContext, TenantId,
};
use layerx_agent_api::{Amount, BudgetLimit, TimestampSeconds};

const SCHEMA: &str = include_str!("../../../schema/agent-api/identity.kvx");

fn context(expiry: u64) -> Result<SessionContext, ContractError> {
    SessionContext::new(
        TenantId::new("tenant-a")?,
        AgentDid::new("did:layerx:agent-a")?,
        AuthorityRef::new("authority-a")?,
        ExplicitSet::allow(vec![ActivityType(7)]),
        TimestampSeconds(expiry),
        ClientId::new("sdk-test")?,
        PolicyVersion::new("policy-v3")?,
    )
}

#[test]
fn session_context_requires_every_dimension_and_rejects_invalid_values() {
    let value = context(1_900_000_000).unwrap_or_else(|error| panic!("valid context: {error:?}"));
    assert_eq!(value.tenant.as_str(), "tenant-a");
    assert_eq!(value.permitted_activity_types.values(), &[ActivityType(7)]);
    assert_eq!(context(0), Err(ContractError::Zero("expiry")));
    assert_eq!(TenantId::new(""), Err(ContractError::Empty("tenant")));

    let required = "[\"tenant\",\"agent_did\",\"authority_ref\",\"permitted_activity_types\",\"expiry\",\"client\",\"policy_version\"]";
    assert!(SCHEMA.contains(required));
}

#[test]
fn capability_dimensions_are_explicit_and_have_no_open_default() {
    let dimensions = CapabilityDimensions {
        activity_types: ExplicitSet::deny_all(),
        counterparties: ExplicitSet::deny_all(),
        assets: ExplicitSet::allow(vec![
            Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}"))
        ]),
        amount_ceilings: ExplicitSet::deny_all(),
        rate_ceilings: ExplicitSet::deny_all(),
        purpose_constraints: ExplicitSet::deny_all(),
        expiry: TimestampSeconds(99),
    }
    .validate()
    .unwrap_or_else(|error| panic!("dimensions: {error:?}"));
    assert!(dimensions.activity_types.values().is_empty());
    assert!(SCHEMA.contains("An explicit empty set denies that dimension"));
    assert!(
        SCHEMA.contains("\"amount_ceilings\",\"rate_ceilings\",\"purpose_constraints\",\"expiry\"")
    );
}

#[test]
fn daemon_limit_is_not_protocol_fundable_or_documented_as_protocol_enforced() {
    assert_eq!(
        BudgetEnforcement::ProtocolBudget.guarantee(),
        "protocol_enforced"
    );
    assert_eq!(
        BudgetEnforcement::DaemonLimit.guarantee(),
        "daemon_enforced"
    );
    assert!(BudgetEnforcement::DAEMON_LIMIT_NOTICE.contains("Bypassing the daemon"));

    let funding = BudgetFund {
        tenant: TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error:?}")),
        agent_did: AgentDid::new("did:layerx:agent-a")
            .unwrap_or_else(|error| panic!("did: {error:?}")),
        budget_id: BudgetId::new("budget-a").unwrap_or_else(|error| panic!("budget: {error:?}")),
        amount: Amount(10),
        enforcement: BudgetEnforcement::DaemonLimit,
    };
    assert_eq!(funding.validate(), Err(ContractError::DaemonLimitFunding));
    assert!(SCHEMA.contains(BudgetEnforcement::DAEMON_LIMIT_NOTICE));
}

#[test]
fn authority_is_mandatory_and_budget_bounds_are_finite() {
    let tenant = TenantId::new("tenant-a").unwrap_or_else(|error| panic!("tenant: {error:?}"));
    let did = AgentDid::new("did:layerx:agent-a").unwrap_or_else(|error| panic!("did: {error:?}"));
    let authority =
        AuthorityRef::new("authority-a").unwrap_or_else(|error| panic!("authority: {error:?}"));
    assert_eq!(
        AuthorityDescription::new(tenant.clone(), did.clone(), authority, Vec::new()),
        Err(ContractError::Empty("protocol_authority"))
    );

    let budget = BudgetCreate {
        tenant,
        agent_did: did,
        asset: Asset::new("LXP").unwrap_or_else(|error| panic!("asset: {error:?}")),
        limit: BudgetLimit(0),
        enforcement: BudgetEnforcement::ProtocolBudget,
        expiry: TimestampSeconds(50),
    };
    assert_eq!(budget.validate(), Err(ContractError::Zero("limit")));
    assert!(SCHEMA.contains("required = [\"authority\",\"value\"]"));
}
