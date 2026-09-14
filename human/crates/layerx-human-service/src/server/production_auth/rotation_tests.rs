use super::{step_up_digest, AgentTenantId, PrincipalId};
use crate::server::schema::ApiSchema;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("rotation disclosure: {error:?}"))
}

#[test]
fn rotation_step_up_binds_owner_tenant_target_timing_and_retry_key() {
    let schema = checked(ApiSchema::v1());
    let operation = schema
        .operation("agent.rotation.start")
        .unwrap_or_else(|| panic!("rotation operation"));
    let principal = checked(PrincipalId::new("owner"));
    let tenant = checked(AgentTenantId::new("owner-tenant"));
    let fields = BTreeMap::from([("agent_id".to_owned(), "agt_rotation".to_owned())]);
    let destination = "/v1/agents/agt_rotation/rotation";
    let body = json!({"delay_seconds": 86400, "window_seconds": 3600});
    let digest = |owner: &PrincipalId,
                  scope: &AgentTenantId,
                  path: &str,
                  parameters: &BTreeMap<String, String>,
                  request: &Value,
                  key: &str| {
        checked(step_up_digest(
            owner,
            scope,
            operation,
            path,
            parameters,
            request,
            Some(key),
        ))
    };
    let expected = digest(&principal, &tenant, destination, &fields, &body, "original");
    for changed in [
        json!({"delay_seconds":86401,"window_seconds":3600}),
        json!({"delay_seconds":86400,"window_seconds":3601}),
    ] {
        assert_ne!(
            digest(
                &principal,
                &tenant,
                destination,
                &fields,
                &changed,
                "original"
            ),
            expected
        );
    }
    assert_ne!(
        digest(
            &checked(PrincipalId::new("another")),
            &tenant,
            destination,
            &fields,
            &body,
            "original"
        ),
        expected
    );
    assert_ne!(
        digest(
            &principal,
            &checked(AgentTenantId::new("another")),
            destination,
            &fields,
            &body,
            "original"
        ),
        expected
    );
    assert_ne!(
        digest(
            &principal,
            &tenant,
            "/v1/agents/agt_another/rotation",
            &fields,
            &body,
            "original"
        ),
        expected
    );
    let other_fields = BTreeMap::from([("agent_id".to_owned(), "agt_another".to_owned())]);
    assert_ne!(
        digest(
            &principal,
            &tenant,
            destination,
            &other_fields,
            &body,
            "original"
        ),
        expected
    );
    assert_ne!(
        digest(&principal, &tenant, destination, &fields, &body, "another"),
        expected
    );
    let mut with_evidence = body.clone();
    with_evidence["step_up"] = json!({"challenge_id":"chg_bound"});
    assert_eq!(
        digest(
            &principal,
            &tenant,
            destination,
            &fields,
            &with_evidence,
            "original"
        ),
        expected
    );
}
