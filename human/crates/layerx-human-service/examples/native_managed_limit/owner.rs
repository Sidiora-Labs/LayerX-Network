use super::{
    checked,
    creation::{Created, SESSION_ACTION},
    fixture::Fixture,
    Result,
};
use layerx_agentd::budget::{LimitConfig, LimitId, LimitScope};
use layerx_agentd::human::{HumanAgentLifecycleSeed, HumanOperations, HumanPeer, MutationEnvelope};
use layerx_agentd::human_runtime::{
    HumanAuthorityBoundary, ProductionHumanOperations, RemoteHumanAuthority, UnifiedAgentOwner,
};
use layerx_agentd::session_keys::SessionKeyRegistry;
use layerx_agentd::store::{Store, TenantId};
use layerx_human_service::server::agent_runtime::{AgentOwnerInstall, AgentSessionSeed};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

pub struct Installed {
    pub owner: Option<UnifiedAgentOwner<RemoteHumanAuthority>>,
    store: Option<Arc<Mutex<Store>>>,
    pub tenant: TenantId,
    pub peer: HumanPeer,
    pub created: Created,
    pub seed: HumanAgentLifecycleSeed,
    configuration: Value,
    authority_request: u64,
    receipt_references: Vec<Value>,
}
impl Installed {
    pub fn store(&self) -> Result<MutexGuard<'_, Store>> {
        self.store
            .as_ref()
            .ok_or("managed store is closed")?
            .lock()
            .map_err(|_| "managed store lock poisoned".into())
    }
    pub fn authority(&self) -> Result<RemoteHumanAuthority> {
        checked(RemoteHumanAuthority::connect(
            self.configuration["endpoint"]
                .as_str()
                .ok_or("authority endpoint")?,
            std::fs::read_to_string(
                self.configuration["token"]
                    .as_str()
                    .ok_or("authority token path")?,
            )?,
            Duration::from_secs(15),
            1_048_576,
            &std::fs::read(
                self.configuration["ca"]
                    .as_str()
                    .ok_or("authority CA path")?,
            )?,
        ))
    }
    pub fn retain_receipt(&mut self, receipt: &super::creation::Receipt) -> Result<()> {
        let reference = receipt.reference()?;
        if let Some(existing) = self
            .receipt_references
            .iter()
            .find(|value| value["activity_id"] == reference["activity_id"])
        {
            assert_eq!(existing, &reference);
        } else {
            self.receipt_references.push(reference);
        }
        Ok(())
    }
    pub fn restart(&mut self, fixture: &Fixture) -> Result<()> {
        drop(self.owner.take());
        let old = self.store.take().ok_or("managed store is closed")?;
        assert_eq!(Arc::strong_count(&old), 1);
        drop(old);
        self.store = Some(Arc::new(Mutex::new(Store::open(
            fixture.directory.join("managed-store"),
        )?)));
        self.authority_request = self
            .authority_request
            .checked_add(1)
            .ok_or("authority request overflow")?;
        self.configuration = configure(
            fixture,
            &self.created,
            self.authority_request,
            &self.receipt_references,
        )?;
        let keys = checked(SessionKeyRegistry::open(
            fixture.directory.join("managed-session-keys"),
            self.created.operator_secret.to_vec(),
            77,
            4021,
        ))?;
        self.owner = Some(construct(self, self.authority()?, keys)?);
        checked(layerx_agentd::managed_agent::validate_session_coordinates(
            &*self.store()?,
            &*self
                .owner
                .as_ref()
                .ok_or("owner missing")?
                .sessions
                .read()
                .map_err(|_| "sessions poisoned")?,
            &self.tenant,
        ))?;
        Ok(())
    }
}
fn configure(
    fixture: &Fixture,
    created: &Created,
    number: u64,
    references: &[Value],
) -> Result<Value> {
    if number > 16 {
        return Err("managed authority request count".into());
    }
    let path = fixture
        .directory
        .join(format!("managed-authority-{number}.request.json"));
    let pending = path.with_extension("pending");
    let clock_fields = [
        "LAYERX_RUNTIME_CLOCK_SOCKET",
        "LAYERX_RUNTIME_CLOCK_PID",
        "LAYERX_RUNTIME_CLOCK_UID",
    ]
    .into_iter()
    .map(|name| Ok((name, std::env::var(name)?)))
    .collect::<Result<BTreeMap<_, _>>>()?;
    std::fs::write(
        &pending,
        serde_json::to_vec(
            &json!({"version":1,"policy":created.policy(fixture)?,"binding":created.provider.binding,
            "clock": clock_fields, "receipts": references}),
        )?,
    )?;
    std::fs::set_permissions(&pending, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(pending, path)?;
    let response = fixture
        .directory
        .join(format!("managed-authority-{number}.response.json"));
    let clock = layerx_client::runtime_clock::RuntimeClock::from_environment()?;
    let mut deadline =
        layerx_types::clock::Deadline::start(clock.as_ref(), Duration::from_secs(90))?;
    while !response.exists() {
        if deadline.remaining(clock.as_ref())?.is_zero() {
            return Err("real Authority preparation deadline".into());
        }
        clock.wait(Duration::from_millis(25))?;
    }
    if std::fs::metadata(&response)?.len() > 4096 {
        return Err("Authority configuration bound".into());
    }
    let value: Value = serde_json::from_slice(&std::fs::read(response)?)?;
    assert_eq!(value["version"], 1);
    Ok(value)
}
fn construct(
    installed: &Installed,
    authority: RemoteHumanAuthority,
    keys: SessionKeyRegistry,
) -> Result<UnifiedAgentOwner<RemoteHumanAuthority>> {
    let peers = BTreeMap::from([(
        4021,
        ("owner".to_owned(), "native-managed-limit".to_owned()),
    )]);
    let mut operations = checked(ProductionHumanOperations::new(
        authority,
        Fixture::open()?.client,
        Arc::clone(installed.store.as_ref().ok_or("managed store is closed")?),
        &peers,
        4096,
        30_000,
    ))?;
    checked(operations.configure_native_budget_recovery(
        std::path::Path::new(&std::env::var("LAYERX_TEST_NATIVE_BUDGET_AUTHORITY")?),
        &peers,
    ))?;
    let tenant = Sha256::digest(installed.tenant.as_str().as_bytes()).into();
    checked(UnifiedAgentOwner::new(
        operations,
        Arc::clone(installed.store.as_ref().ok_or("managed store is closed")?),
        &peers,
        vec![LimitConfig {
            id: LimitId(installed.created.budget.budget_id[..16].try_into()?),
            name: "native managed preset".to_owned(),
            scope: LimitScope::Tenant(tenant),
            ceiling: installed.created.budget.per_period_limit,
            consumed: 0,
        }],
        keys,
    ))
}
fn seed(fixture: &Fixture, created: &Created) -> Result<HumanAgentLifecycleSeed> {
    let budget = &created.budget;
    let time = budget.period_start_ms / 1000;
    let evidence = [
        &created.identity,
        &created.rotation,
        &created.recovery,
        &created.creation,
        &created.grant,
    ]
    .iter()
    .map(|receipt| Sha256::digest(&receipt.bytes).into())
    .collect::<Vec<[u8; 32]>>();
    Ok(HumanAgentLifecycleSeed {
        agent_id: format!("agt_{}", layerx_programs::hex::encode(&budget.budget_id)),
        name: "Native limit qualification".to_owned(),
        purpose: "native-managed-limit".to_owned(),
        currency: "LXT".to_owned(),
        monthly_limit: budget.per_period_limit,
        period_start: time,
        period_end: budget.expiry_ms / 1000,
        created_at: time,
        updated_at: time,
        verified_evidence: evidence.clone(),
        actor: std::str::from_utf8(fixture.did.as_bytes())?.to_owned(),
        primary_authority: layerx_programs::hex::encode(&fixture.public),
        custody_key: "native-managed-owner".to_owned(),
        custody_public_key: fixture.public,
        owner_account: budget.source_account.canonical().to_owned(),
        budget_account: budget.budget_account.canonical().to_owned(),
        budget_asset: budget.asset,
        purpose_hash: budget.purpose,
        recovery_root: created.recovery_root,
        recovery_threshold: 1,
        capability_id: created.granted.grant_id,
        activity_types: vec![0x0003_0006],
        counterparties: vec![checked(super::fixture::account(&format!(
            "agent:{}:main",
            std::str::from_utf8(fixture.did.as_bytes())?
        )))?],
        assets: vec![budget.asset],
        amount_ceiling: budget.per_period_limit,
        rate_maximum_uses: 100,
        rate_window_sequences: 10000,
        purposes: vec!["native-managed-limit".to_owned()],
        capability_expiry_sequence: created.session_expiry_sequence,
        session_scopes: vec!["prepare".to_owned(), "submit".to_owned()],
        session_expiry_unix_seconds: created.granted.expires_at / 1000,
        protocol_grant_id: created.granted.grant_id,
        budget_period_seconds: budget.period_length_ms / 1000,
        budget_expiry_seconds: (budget.expiry_ms - budget.period_start_ms) / 1000,
        initial_funding: budget.initial_amount,
        network_id: 77,
        creation_receipt_roots: evidence,
    })
}
fn request(
    installed: &Installed,
    lease: &layerx_agentd::human_runtime::CoreLeaseAttestation,
) -> Result<AgentOwnerInstall> {
    let grant = &installed.created.granted;
    let before = lease.lower_unix_ms.max(grant.not_before);
    let after = lease.upper_unix_ms.min(grant.expires_at);
    if after <= before {
        return Err("actual lease does not intersect native grant".into());
    }
    let raw = AgentOwnerInstall {
        agent: installed.seed.actor.clone(),
        authority_kind: 2,
        authority_id: grant.grant_id,
        session_id: SESSION_ACTION,
        token_id: [0; 32],
        session_public_key: grant.session_public_key,
        registration_payload: grant.registration_payload.clone(),
        grantor: grant.grantor,
        grant_not_before: grant.not_before,
        grant_expires_at: grant.expires_at,
        grant_revocation_sequence: grant.revocation_sequence,
        session_seed: Some(checked(AgentSessionSeed::new(
            installed.created.session_seed,
        ))?),
        permitted_activity_types: vec![6],
        scopes: installed.seed.session_scopes.clone(),
        lease_not_before_unix_ms: before,
        lease_not_after_unix_ms: after,
        opening_client: "native managed qualification".to_owned(),
        policy_version: "native-managed-limit".to_owned(),
        lifecycle: None,
    };
    Ok(raw)
}

pub fn install(fixture: &mut Fixture, mut created: Created) -> Result<Installed> {
    let receipt_references = created.references()?;
    let configuration = configure(fixture, &created, 1, &receipt_references)?;
    let keys = created
        .session_keys
        .take()
        .ok_or("actual session key registry missing")?;
    let namespace = layerx_identity_binding::subject_namespace(
        "native-managed-limit",
        &created.provider.principal,
    )?;
    let tenant = checked(TenantId::new(&namespace))?;
    let peer = HumanPeer {
        uid: 4021,
        principal: created.provider.principal.clone(),
        tenant: namespace,
        subject: Some(layerx_agentd::human::HumanSubject {
            transport_tenant: "native-managed-limit".to_owned(),
            transport_principal: "owner".to_owned(),
            owner: std::str::from_utf8(fixture.did.as_bytes())?.to_owned(),
            account: created.budget.source_account.canonical().to_owned(),
            asset: fixture.asset,
            registration: None,
        }),
    };
    let store = Arc::new(Mutex::new(Store::open(
        fixture.directory.join("managed-store"),
    )?));
    let seed = seed(fixture, &created)?;
    let mut installed = Installed {
        owner: None,
        store: Some(store),
        tenant,
        peer,
        created,
        seed,
        configuration,
        authority_request: 1,
        receipt_references,
    };
    let mut authority = installed.authority()?;
    let lease = checked(authority.lease_attestation(&installed.peer))?;
    let mut owner = construct(&installed, authority, keys)?;
    let request = request(&installed, &lease)?;
    super::transport::install(
        &installed.created.provider.root.join("owner.sock"),
        &mut owner,
        &installed.peer,
        fixture.registry.clone(),
        &request,
    )?;
    let session = owner
        .sessions
        .read()
        .map_err(|_| "session registry lock")?
        .get(
            &installed.tenant,
            layerx_agentd::session::SessionId(SESSION_ACTION),
        )
        .cloned()
        .ok_or("real session installation missing")?;
    assert!(session.open);
    assert_eq!(
        session.request.authority,
        layerx_agentd::identity::ProtocolAuthority::SessionKey(installed.created.granted.grant_id)
    );
    assert_ne!(session.request.token_id, [0; 32]);
    publish(&installed, &mut owner, fixture.public)?;
    installed.owner = Some(owner);
    Ok(installed)
}

fn publish(
    installed: &Installed,
    owner: &mut UnifiedAgentOwner<RemoteHumanAuthority>,
    public: [u8; 32],
) -> Result<()> {
    checked(owner.capability_install(
        &installed.peer,
        layerx_agentd::human::HumanCapabilityInstall {
            action_key: SESSION_ACTION,
            agent: installed.seed.actor.clone(),
            authority_id: public,
            capability_id: installed.seed.capability_id,
            activity_types: vec![6],
            counterparties: installed.seed.counterparties.clone(),
            assets: installed.seed.assets.clone(),
            amount_ceiling: installed.seed.amount_ceiling,
            rate_maximum_uses: installed.seed.rate_maximum_uses,
            rate_window_sequences: installed.seed.rate_window_sequences,
            purposes: installed.seed.purposes.clone(),
            expiry_sequence: installed.seed.capability_expiry_sequence,
        },
    ))?;
    checked(owner.agent_lifecycle_publish(
        &installed.peer,
        MutationEnvelope {
            request_id: 8701,
            key: SESSION_ACTION,
            body_digest: checked(layerx_agentd::managed_agent::lifecycle_publish_digest(
                &installed.seed,
            ))?,
            operation: installed.seed.clone(),
        },
    ))?;
    checked(layerx_agentd::managed_agent::validate_session_coordinates(
        &*installed.store()?,
        &*owner.sessions.read().map_err(|_| "session registry lock")?,
        &installed.tenant,
    ))?;
    Ok(())
}
