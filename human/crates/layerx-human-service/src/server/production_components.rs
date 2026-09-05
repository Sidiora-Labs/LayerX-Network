//! Production composition root for the privileged human component process.

use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use layerx_agent_api::identity::{AgentDid, AuthorityRef};
use layerx_client::lni::transport::{Limits, MutualTlsConfig};
use layerx_crypto::local::LocalSigner;
use layerx_crypto::session::{issue_session_key, SessionKeyRequest};
use layerx_crypto::signer::Signer as _;
use layerx_intents::{
    BudgetCreate, BudgetDefund, Intent, IntentKind, KeyRotation,
    SessionGrant as ProtocolSessionGrant, SessionRevoke,
};
use layerx_paxeer_client::{
    raw_call, EmergencyExit, EndpointConfig, EndpointTransport, ExitConfig, ExitEligibility,
};
use layerx_proof::checkpoint::SettlementDomain;
use layerx_proof::export::OfflineExport;
use layerx_types::account::AccountId;
use layerx_types::activity::TimestampBound;
use layerx_types::amount::Amount as ProtocolAmount;
use layerx_types::ids::Did;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::EvmAddress;
use layerx_types::intent::{AuthorityGrantId, PublicKey, SessionRevocationReason};
use layerx_types::intent::{
    BudgetId, PeriodLength, PurposeHash, RolloverPolicy, Sequence as ProtocolSequence,
    TimestampSeconds,
};
use layerx_types::payload::ModuleId;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::RootCertStore;
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::agent_creation::ProductionAgentCreation;
use crate::activity::{
    verification_status, AppliedFilters, EvidenceBundle, EvidenceExport, Feed, FeedCursor,
    FilterDraft, PageRequest, ReceiptAuthority,
};
use crate::agents::{
    CreateAgentRequest, CreationContext, CreationJourney, PurposePresetCatalog,
    ScopedAgentCreationContract, SessionProvision,
};
use crate::approvals::{
    AgentApprovalRecord, AgentApprovalState, AgentDecisionStatus, ApprovalBoundary,
};
use crate::auth::{AccountIdentity, AuthConfig, Passkeys, RateLimit};
use crate::binding::BindingJourney;
use crate::custody::{
    CustodyError, CustodySigner, KeyClass, KeyId, Keystore, RemoteKmsProvider, SigningLimits,
};
use crate::notify::{
    ActivityEntryId, Channel, DeepLinks, Dispatcher, NotificationId, NotificationSummary,
    Preferences,
};
use crate::onboarding::OnboardingJourney;
use crate::security::{
    AuthenticatorMethod, AuthenticatorProvider, AuthenticatorStatus, RecoveryEvidenceProvider,
};
use crate::store::{
    PrincipalStore, RetentionPeriod, RetentionPolicy, RowKey, Table, TenancyDigest,
};
use crate::support::{CreateConversation, Shell, SupportService, Topic};
use crate::trace::TraceId;

use super::agent_runtime::AgentRuntime;
use super::backend::{
    ApiFailure, BackendResponse, ComponentState, HumanApiComponents, PrincipalContext, Readiness,
    ScopedRequest, SessionCredentials, SessionSecrets,
};
use super::identity_dispatch::{self, IdentityProviderConfig, RemoteIdentityProvider};
use super::movement_provider::{MovementProviderConfig, NativeMovementCodec, UnixMovementProvider};
use super::production_auth::{
    authorize_execution, authorize_refresh_execution, consume_context, AuthDiscoveryIndex,
    AuthorizationDisclosure, IndexAuthenticationKey, RemoteSecurityProvider,
    SecurityProviderConfig,
};
use super::schema::{ApiSchema, Operation};

const PRODUCTION_OPERATIONS: &[&str] = &[
    "account.balance",
    "account.create",
    "activity.entry",
    "activity.export.evidence",
    "activity.export.statement",
    "activity.query",
    "agent.archive",
    "agent.create",
    "agent.get",
    "agent.limit",
    "agent.list",
    "agent.pause",
    "agent.reclaim",
    "agent.recover",
    "agent.resume",
    "agent.rotate",
    "approval.approve",
    "approval.get",
    "approval.list",
    "approval.reject",
    "authenticator.backup.rotate",
    "authenticator.disable",
    "authenticator.setup.begin",
    "authenticator.setup.finish",
    "authenticator.status",
    "binding.rebind",
    "binding.rebind.action",
    "binding.statement",
    "binding.status",
    "binding.submit",
    "deposit.confirm",
    "deposit.start",
    "evidence.get",
    "exit.eligibility",
    "exit.start",
    "home.summary",
    "journey.get",
    "journey.list",
    "move.commit",
    "move.quote",
    "notification.list",
    "notification.preferences.get",
    "notification.preferences.set",
    "notification.read",
    "onboarding.resume",
    "onboarding.status",
    "passkey.assert.begin",
    "passkey.assert.finish",
    "passkey.register.begin",
    "passkey.register.finish",
    "profile.get",
    "profile.update",
    "security.action",
    "security.passkey.list",
    "security.passkey.register.begin",
    "security.passkey.register.finish",
    "security.passkey.revoke",
    "security.recovery.reveal",
    "security.session.revoke",
    "security.session.revoke-all",
    "session.list",
    "session.open",
    "session.refresh",
    "session.revoke",
    "session.revoke-all",
    "stepup.begin",
    "stepup.finish",
    "stream.next",
    "stream.open",
    "support.create",
    "support.feedback",
    "support.list",
    "support.read",
    "support.reply",
    "support.status",
    "version",
    "withdraw.claim",
    "withdraw.start",
];
fn managed_protocol_identity(value: &str) -> Result<[u8; 32], ApiFailure> {
    let encoded = value
        .strip_prefix("agt_")
        .ok_or_else(ApiFailure::upstream_degraded)?;
    if encoded.len() != 64 {
        return Err(ApiFailure::upstream_degraded());
    }
    let mut out = [0; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&encoded[index * 2..index * 2 + 2], 16)
            .map_err(|_| ApiFailure::upstream_degraded())?;
    }
    Ok(out)
}
use super::component::ComponentMaintenance;

/// Mandatory, bounded production settings. There are deliberately no defaults:
/// an omitted trust root, retention limit or transport bound refuses startup.
pub struct ProductionComponentsConfig {
    store_root: PathBuf,
    custody_root: PathBuf,
    auth_index_root: PathBuf,
    tenancy_digest: [u8; 32],
    auth_index_key: [u8; 32],
    stream_cursor_key: [u8; 32],
    auth: AuthConfig,
    retention: RetentionPolicy,
    capability_ttl_seconds: u64,
    agent_socket: PathBuf,
    agent_limits: Limits,
    security: SecurityProviderConfig,
    identity: IdentityProviderConfig,
    movement: MovementProviderConfig,
    kms_provider_reference: String,
    kms_endpoint: SocketAddr,
    kms_server_name: String,
    kms_root_certificate: PathBuf,
    kms_client_certificate: PathBuf,
    kms_client_private_key: PathBuf,
    kms_limits: Limits,
    network_id: u32,
    protocol_version: u16,
    signing_limits: SigningLimits,
    agent_actor: AgentDid,
    agent_authority: AuthorityRef,
    agent_timestamp_span_seconds: u64,
    agent_fee_limit: u128,
    binding_statement_ttl_seconds: u64,
    agent_purpose_catalog: PathBuf,
    agent_owner_account: String,
    agent_recovery_root: [u8; 32],
    agent_recovery_threshold: u16,
    paxeer_endpoint: EndpointConfig,
    exit_contract: EvmAddress,
    withdrawal_claims_contract: EvmAddress,
    exit_required_confirmations: u64,
    activity_freshness_seconds: u64,
    activity_export_maximum_bytes: usize,
    settlement_chain_id: u64,
    exit_poll_cadence: Duration,
    exit_delayed_after_polls: u64,
    continuation_unknown_deadline_seconds: u64,
}

impl ProductionComponentsConfig {
    pub fn from_environment() -> Result<Self, String> {
        let bounded_limits = |prefix: &str| -> Result<Limits, String> {
            Ok(Limits {
                maximum_frame_bytes: number(&format!("{prefix}_MAX_FRAME_BYTES"))?,
                maximum_connections: number(&format!("{prefix}_MAX_CONNECTIONS"))?,
                maximum_streams: number(&format!("{prefix}_MAX_STREAMS"))?,
                maximum_queued_bytes: number(&format!("{prefix}_MAX_QUEUED_BYTES"))?,
                deadline: Duration::from_secs(number(&format!("{prefix}_DEADLINE_SECONDS"))?),
            })
        };
        let retention = |name: &str| number(name).map(RetentionPeriod::new);
        Ok(Self {
            store_root: absolute("LAYERX_HUMAN_STORE_ROOT")?,
            custody_root: absolute("LAYERX_HUMAN_CUSTODY_ROOT")?,
            auth_index_root: absolute("LAYERX_HUMAN_AUTH_INDEX_ROOT")?,
            tenancy_digest: secret32("LAYERX_HUMAN_TENANCY_DIGEST")?,
            auth_index_key: secret32("LAYERX_HUMAN_AUTH_INDEX_KEY")?,
            stream_cursor_key: secret32("LAYERX_HUMAN_STREAM_CURSOR_KEY")?,
            auth: AuthConfig {
                rp_id: required("LAYERX_HUMAN_RP_ID")?,
                rp_name: required("LAYERX_HUMAN_RP_NAME")?,
                origin: required("LAYERX_HUMAN_ORIGIN")?,
                ceremony_ttl_secs: number("LAYERX_HUMAN_CEREMONY_TTL_SECONDS")?,
                assertion_ttl_secs: number("LAYERX_HUMAN_ASSERTION_TTL_SECONDS")?,
                session_ttl_secs: number("LAYERX_HUMAN_SESSION_TTL_SECONDS")?,
                refresh_ttl_secs: number("LAYERX_HUMAN_REFRESH_TTL_SECONDS")?,
                step_up_ttl_secs: number("LAYERX_HUMAN_STEP_UP_TTL_SECONDS")?,
                rate_limit: RateLimit {
                    attempts: number("LAYERX_HUMAN_AUTH_RATE_ATTEMPTS")?,
                    window_secs: number("LAYERX_HUMAN_AUTH_RATE_WINDOW_SECONDS")?,
                },
            },
            retention: RetentionPolicy {
                journeys: retention("LAYERX_HUMAN_RETENTION_JOURNEYS_SECONDS")?,
                notifications: retention("LAYERX_HUMAN_RETENTION_NOTIFICATIONS_SECONDS")?,
                audit: retention("LAYERX_HUMAN_RETENTION_AUDIT_SECONDS")?,
                telemetry: retention("LAYERX_HUMAN_RETENTION_TELEMETRY_SECONDS")?,
                cache: retention("LAYERX_HUMAN_RETENTION_CACHE_SECONDS")?,
            },
            capability_ttl_seconds: number("LAYERX_HUMAN_CAPABILITY_TTL_SECONDS")?,
            agent_socket: absolute("LAYERX_HUMAN_AGENT_SOCKET")?,
            agent_limits: bounded_limits("LAYERX_HUMAN_AGENT")?,
            security: SecurityProviderConfig {
                socket: absolute("LAYERX_HUMAN_SECURITY_SOCKET")?,
                deadline: Duration::from_secs(number("LAYERX_HUMAN_SECURITY_DEADLINE_SECONDS")?),
                maximum_frame_bytes: number("LAYERX_HUMAN_SECURITY_MAX_FRAME_BYTES")?,
            },
            identity: IdentityProviderConfig {
                socket: absolute("LAYERX_HUMAN_IDENTITY_SOCKET")?,
                deadline: Duration::from_secs(number("LAYERX_HUMAN_IDENTITY_DEADLINE_SECONDS")?),
                maximum_frame_bytes: number("LAYERX_HUMAN_IDENTITY_MAX_FRAME_BYTES")?,
                peer_uid: number("LAYERX_HUMAN_IDENTITY_PEER_UID")?,
                peer_gid: number("LAYERX_HUMAN_IDENTITY_PEER_GID")?,
            },
            movement: MovementProviderConfig::from_environment()
                .map_err(|_| "movement provider configuration was refused".to_owned())?,
            kms_provider_reference: required("LAYERX_HUMAN_KMS_PROVIDER_REFERENCE")?,
            kms_endpoint: required("LAYERX_HUMAN_KMS_ENDPOINT")?
                .parse()
                .map_err(|_| "LAYERX_HUMAN_KMS_ENDPOINT is invalid".to_owned())?,
            kms_server_name: required("LAYERX_HUMAN_KMS_SERVER_NAME")?,
            kms_root_certificate: absolute("LAYERX_HUMAN_KMS_ROOT_CERTIFICATE_DER")?,
            kms_client_certificate: absolute("LAYERX_HUMAN_KMS_CLIENT_CERTIFICATE_DER")?,
            kms_client_private_key: absolute("LAYERX_HUMAN_KMS_CLIENT_PRIVATE_KEY_DER")?,
            kms_limits: bounded_limits("LAYERX_HUMAN_KMS")?,
            network_id: number("LAYERX_HUMAN_NETWORK_ID")?,
            protocol_version: configured_protocol()?,
            signing_limits: SigningLimits::new(
                number("LAYERX_HUMAN_SIGNING_RATE_MAXIMUM")?,
                number("LAYERX_HUMAN_SIGNING_RATE_WINDOW_SECONDS")?,
            )
            .map_err(|_| "invalid custody signing limits".to_owned())?,
            agent_actor: AgentDid::new(required("LAYERX_HUMAN_AGENT_ACTOR")?)
                .map_err(|_| "LAYERX_HUMAN_AGENT_ACTOR is invalid".to_owned())?,
            agent_authority: AuthorityRef::new(required("LAYERX_HUMAN_AGENT_AUTHORITY")?)
                .map_err(|_| "LAYERX_HUMAN_AGENT_AUTHORITY is invalid".to_owned())?,
            agent_timestamp_span_seconds: number("LAYERX_HUMAN_AGENT_TIMESTAMP_SPAN_SECONDS")?,
            agent_fee_limit: number("LAYERX_HUMAN_AGENT_FEE_LIMIT")?,
            binding_statement_ttl_seconds: number("LAYERX_HUMAN_BINDING_STATEMENT_TTL_SECONDS")?,
            agent_purpose_catalog: absolute("LAYERX_HUMAN_AGENT_PURPOSE_CATALOG")?,
            agent_owner_account: required("LAYERX_HUMAN_AGENT_OWNER_ACCOUNT")?,
            agent_recovery_root: secret32("LAYERX_HUMAN_AGENT_RECOVERY_ROOT")?,
            agent_recovery_threshold: number("LAYERX_HUMAN_AGENT_RECOVERY_THRESHOLD")?,
            paxeer_endpoint: EndpointConfig {
                url: required("LAYERX_HUMAN_PAXEER_RPC_URL")?,
                request_timeout: Duration::from_secs(number(
                    "LAYERX_HUMAN_PAXEER_RPC_TIMEOUT_SECONDS",
                )?),
                transport: EndpointTransport::PinnedTls {
                    trust_anchor_der: read_nonempty(&absolute(
                        "LAYERX_HUMAN_PAXEER_TRUST_ANCHOR_DER",
                    )?)?,
                },
                expected_chain_id: number("LAYERX_HUMAN_PAXEER_CHAIN_ID")?,
            },
            exit_contract: EvmAddress::new(hex20("LAYERX_HUMAN_PAXEER_EXIT_CONTRACT")?),
            withdrawal_claims_contract: EvmAddress::new(hex20(
                "LAYERX_HUMAN_PAXEER_WITHDRAWAL_CLAIMS_CONTRACT",
            )?),
            exit_required_confirmations: number("LAYERX_HUMAN_EXIT_REQUIRED_CONFIRMATIONS")?,
            activity_freshness_seconds: number("LAYERX_HUMAN_ACTIVITY_FRESHNESS_SECONDS")?,
            activity_export_maximum_bytes: number("LAYERX_HUMAN_ACTIVITY_EXPORT_MAXIMUM_BYTES")?,
            settlement_chain_id: number("LAYERX_HUMAN_PAXEER_CHAIN_ID")?,
            exit_poll_cadence: Duration::from_secs(number(
                "LAYERX_HUMAN_EXIT_POLL_CADENCE_SECONDS",
            )?),
            exit_delayed_after_polls: number("LAYERX_HUMAN_EXIT_DELAYED_AFTER_POLLS")?,
            continuation_unknown_deadline_seconds: number(
                "LAYERX_HUMAN_CONTINUATION_UNKNOWN_DEADLINE_SECONDS",
            )?,
        })
    }
}

/// In-process owners used by the privileged component listener.
pub struct ProductionComponents {
    store: Arc<Mutex<PrincipalStore>>,
    passkeys: Passkeys,
    auth_index: AuthDiscoveryIndex,
    capability_ttl_seconds: u64,
    agent: Mutex<AgentRuntime>,
    agent_contract: layerx_sdk::Client,
    agent_limits: Limits,
    custody: CustodySigner,
    security: Mutex<RemoteSecurityProvider>,
    stream: super::stream_journal::StreamJournal,
    feed: Feed,
    activity_export_maximum_bytes: usize,
    settlement_domain: SettlementDomain,
    identity: RemoteIdentityProvider,
    movement: Mutex<UnixMovementProvider>,
    paxeer_endpoint: EndpointConfig,
    emergency_exit: EmergencyExit,
    agent_actor: AgentDid,
    agent_authority: AuthorityRef,
    agent_timestamp_span_seconds: u64,
    agent_fee_limit: u128,
    network_id: u32,
    binding_statement_ttl_seconds: u64,
    agent_purpose_catalog: PurposePresetCatalog,
    agent_owner_account: String,
    agent_recovery_root: [u8; 32],
    agent_recovery_threshold: u16,
    continuation_unknown_deadline_seconds: u64,
    maintenance_healthy: AtomicBool,
}

impl ProductionComponents {
    pub fn open(config: ProductionComponentsConfig) -> Result<Self, String> {
        let schema = ApiSchema::v1().map_err(|_| "human API schema is invalid".to_owned())?;
        if schema.operations().len() != PRODUCTION_OPERATIONS.len()
            || schema
                .operations()
                .iter()
                .any(|operation| !PRODUCTION_OPERATIONS.contains(&operation.name.as_str()))
        {
            return Err("human API operation dispatch is incomplete".to_owned());
        }
        if !(1..=60).contains(&config.capability_ttl_seconds) {
            return Err("LAYERX_HUMAN_CAPABILITY_TTL_SECONDS must be between 1 and 60".to_owned());
        }
        if config.agent_timestamp_span_seconds == 0 || config.agent_fee_limit == 0 {
            return Err("agent preparation bounds must be non-zero".to_owned());
        }
        if config.continuation_unknown_deadline_seconds == 0 {
            return Err(
                "LAYERX_HUMAN_CONTINUATION_UNKNOWN_DEADLINE_SECONDS must be non-zero".to_owned(),
            );
        }
        let passkeys =
            Passkeys::new(config.auth).map_err(|_| "invalid passkey configuration".to_owned())?;
        let agent_purpose_catalog =
            PurposePresetCatalog::from_json(&read_nonempty(&config.agent_purpose_catalog)?)
                .map_err(|_| "agent purpose catalog was refused".to_owned())?;
        let store = Arc::new(Mutex::new(
            PrincipalStore::open(
                config.store_root,
                config.retention,
                TenancyDigest::new(config.tenancy_digest),
            )
            .map_err(|_| "principal store refused startup".to_owned())?,
        ));
        let auth_index = AuthDiscoveryIndex::open(
            config.auth_index_root,
            IndexAuthenticationKey::new(config.auth_index_key)
                .map_err(|_| "authentication index key is invalid".to_owned())?,
        )
        .map_err(|_| "authentication index refused startup".to_owned())?;
        let agent_contract = layerx_sdk::Client::daemon(
            config.agent_socket.clone(),
            layerx_agent_api::agent_api_schema_v1().version,
        )
        .map_err(|_| "agent SDK contract refused startup".to_owned())?;
        let agent = AgentRuntime::connect(config.agent_socket, config.agent_limits)
            .map_err(|_| "agent boundary refused startup".to_owned())?;
        let tls = mutual_tls(
            &config.kms_root_certificate,
            &config.kms_client_certificate,
            &config.kms_client_private_key,
        )?;
        let provider = RemoteKmsProvider::new(
            config.kms_provider_reference,
            config.kms_endpoint,
            config.kms_server_name,
            tls,
            config.kms_limits,
        )
        .map_err(|_| "KMS configuration was refused".to_owned())?;
        let keystore = Keystore::open_production(config.custody_root, config.network_id, provider)
            .map_err(|_| "KMS or custody storage refused startup".to_owned())?;
        let custody = CustodySigner::new_shared(
            keystore,
            Arc::clone(&store),
            agent.registry().clone(),
            config.signing_limits,
        );
        let security = RemoteSecurityProvider::new(config.security)
            .map_err(|_| "security provider configuration was refused".to_owned())?;
        let identity = RemoteIdentityProvider::new(config.identity)
            .map_err(|_| "identity provider configuration was refused".to_owned())?;
        let withdrawal_boundary = layerx_paxeer_client::WithdrawalBoundary::new_for_protocol(
            layerx_paxeer_client::WithdrawalConfig {
                endpoints: vec![config.paxeer_endpoint.clone()],
                minimum_endpoint_agreement: 1,
                claims_contract: config.withdrawal_claims_contract,
                required_confirmations: config.exit_required_confirmations,
                poll_cadence: config.exit_poll_cadence,
                delayed_after_polls: config.exit_delayed_after_polls,
            },
            config.protocol_version,
        )
        .map_err(|_| "Paxeer withdrawal boundary refused startup".to_owned())?;
        let movement = UnixMovementProvider::new(
            config.movement,
            Arc::new(
                NativeMovementCodec::for_protocol(config.protocol_version)
                    .map_err(|_| "unsupported Human protocol version".to_owned())?,
            ),
            withdrawal_boundary,
        )
        .map_err(|_| "movement provider refused startup".to_owned())?;
        raw_call(&config.paxeer_endpoint, "eth_chainId", &[])
            .map_err(|_| "Paxeer boundary refused startup".to_owned())?;
        let emergency_exit = EmergencyExit::new(ExitConfig {
            endpoints: vec![config.paxeer_endpoint.clone()],
            minimum_endpoint_agreement: 1,
            exit_contract: config.exit_contract,
            required_confirmations: config.exit_required_confirmations,
            poll_cadence: config.exit_poll_cadence,
            delayed_after_polls: config.exit_delayed_after_polls,
        })
        .map_err(|_| "Paxeer exit boundary refused startup".to_owned())?;
        let settlement_domain =
            SettlementDomain::new(config.settlement_chain_id, config.exit_contract.bytes());
        Ok(Self {
            store,
            passkeys,
            auth_index,
            capability_ttl_seconds: config.capability_ttl_seconds,
            agent: Mutex::new(agent),
            agent_contract,
            agent_limits: config.agent_limits,
            custody,
            security: Mutex::new(security),
            stream: super::stream_journal::StreamJournal::new(
                config.stream_cursor_key,
                settlement_domain,
            ),
            feed: Feed::new(config.activity_freshness_seconds)
                .map_err(|_| "activity freshness bound is invalid".to_owned())?,
            activity_export_maximum_bytes: config.activity_export_maximum_bytes,
            settlement_domain,
            identity,
            movement: Mutex::new(movement),
            paxeer_endpoint: config.paxeer_endpoint,
            emergency_exit,
            agent_actor: config.agent_actor,
            agent_authority: config.agent_authority,
            agent_timestamp_span_seconds: config.agent_timestamp_span_seconds,
            agent_fee_limit: config.agent_fee_limit,
            network_id: config.network_id,
            binding_statement_ttl_seconds: config.binding_statement_ttl_seconds,
            agent_purpose_catalog,
            agent_owner_account: config.agent_owner_account,
            agent_recovery_root: config.agent_recovery_root,
            agent_recovery_threshold: config.agent_recovery_threshold,
            continuation_unknown_deadline_seconds: config.continuation_unknown_deadline_seconds,
            maintenance_healthy: AtomicBool::new(true),
        })
    }

    fn revoke_browser_grants(
        &self,
        scope: &mut crate::store::PrincipalScope<'_>,
        request: &ScopedRequest<'_>,
        grants: &[(String, [u8; 32])],
    ) -> Result<(), ApiFailure> {
        let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
        let registry = agent.registry().clone();
        let trace =
            TraceId::parse(&request.trace).map_err(|_| ApiFailure::invalid_request(None))?;
        for (session_id, grant_id) in grants {
            let effective = agent
                .head()
                .map_err(agent_failure)?
                .chain_sequence
                .checked_add(1)
                .ok_or_else(ApiFailure::upstream_degraded)?;
            let intent = Intent::v1(IntentKind::SessionRevoke(
                SessionRevoke::new(
                    AuthorityGrantId::new(*grant_id),
                    SessionRevocationReason::SignedOut,
                    ProtocolSequence::from_u64(effective),
                )
                .map_err(|_| ApiFailure::upstream_degraded())?,
            ));
            let key = action_key(&format!("{}:{session_id}", required_idempotency(request)?));
            let current = now()?;
            let mut adapter = ProductionAgentCreation::new(
                &mut agent,
                &self.agent_contract,
                &self.custody,
                &trace,
                self.agent_actor.clone(),
                self.agent_authority.clone(),
                self.agent_timestamp_span_seconds,
                self.agent_fee_limit,
            )
            .map_err(|_| ApiFailure::upstream_degraded())?;
            let evidence = adapter
                .submit_lifecycle_intent(
                    scope,
                    &registry,
                    intent,
                    key,
                    KeyId::new("human-primary").map_err(|_| ApiFailure::upstream_degraded())?,
                    current,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
            ProductionAgentCreation::finalization_evidence(
                &evidence,
                ModuleId::Governance,
                6,
                current,
            )
            .map_err(|_| ApiFailure::upstream_degraded())?;
        }
        Ok(())
    }
}

impl HumanApiComponents for ProductionComponents {
    fn authorize(
        &self,
        operation: &Operation,
        credentials: SessionCredentials<'_>,
        trace: &str,
    ) -> Result<PrincipalContext, ApiFailure> {
        let now = now()?;
        let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
        if operation.name == "session.refresh" {
            let csrf = credentials.csrf_token.ok_or_else(ApiFailure::forbidden)?;
            return authorize_refresh_execution(
                &mut store,
                &self.passkeys,
                &self.auth_index,
                credentials.access_token,
                csrf,
                AuthorizationDisclosure {
                    operation,
                    destination: credentials.intended_destination,
                    path_parameters: credentials.path_parameters,
                    body: credentials.body,
                    idempotency_key: credentials.idempotency_key,
                    trace,
                },
                now,
                self.capability_ttl_seconds,
            )
            .map_err(auth_failure);
        }
        let principal = self
            .auth_index
            .resolve_access_token(credentials.access_token, now)
            .map_err(auth_failure)?;
        let step_up_id = credentials
            .body
            .get("step_up")
            .and_then(|value| value.get("challenge_id"))
            .and_then(|value| value.as_str())
            .or_else(|| {
                credentials
                    .body
                    .get("step_up_evidence")
                    .and_then(|value| value.as_str())
            });
        let step_up = if let Some(challenge_id) = step_up_id {
            let mut scope = store
                .principal(&principal)
                .map_err(|_| ApiFailure::unavailable())?;
            Some(
                self.passkeys
                    .load_step_up_evidence(&mut scope, challenge_id, now)
                    .map_err(|_| ApiFailure::forbidden())?,
            )
        } else {
            None
        };
        let capability = authorize_execution(
            &mut store,
            &self.passkeys,
            &self.auth_index,
            credentials.access_token,
            credentials.csrf_token,
            step_up.as_ref(),
            AuthorizationDisclosure {
                operation,
                destination: credentials.intended_destination,
                path_parameters: credentials.path_parameters,
                body: credentials.body,
                idempotency_key: credentials.idempotency_key,
                trace,
            },
            now,
            self.capability_ttl_seconds,
        )
        .map_err(auth_failure)?;
        if capability.request_disclosure() != credentials.request_digest
            || capability.body_disclosure() != credentials.disclosure_digest
        {
            return Err(ApiFailure::forbidden());
        }
        capability.into_context()
    }

    fn execute(&self, request: ScopedRequest<'_>) -> Result<BackendResponse, ApiFailure> {
        if request.operation.name == "version" {
            return Ok(BackendResponse {
                result: json!({"schema": {"major": 1, "minor": 0}, "service": "layerx-human"}),
                session: None,
            });
        }
        if request.operation.is_public_bootstrap() {
            let now = now()?;
            let (principal, result, session) = match request.operation.name.as_str() {
                "account.create" => {
                    let email = text_field(&request.body, "email")?;
                    let display_name = text_field(&request.body, "display_name")?;
                    let provisioned = self
                        .identity
                        .provision(email, display_name, required_idempotency(&request)?, now)
                        .map_err(identity_failure)?;
                    self.auth_index
                        .bind_account(email, &provisioned.principal)
                        .map_err(auth_failure)?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&provisioned.principal)
                        .map_err(|_| ApiFailure::unavailable())?;
                    let journey =
                        OnboardingJourney::start(&mut scope, &provisioned.onboarding, now)
                            .map_err(|_| ApiFailure::upstream_degraded())?;
                    identity_dispatch::update_profile(
                        &mut scope,
                        &json!({"display_name": display_name}),
                        now,
                    )
                    .map_err(identity_failure)?;
                    let result = json!({"account_id": provisioned.principal.as_str(),
                        "onboarding": identity_dispatch::onboarding_status(&journey.status())});
                    (provisioned.principal, result, None)
                }
                "passkey.register.begin" => {
                    let principal =
                        crate::store::PrincipalId::new(text_field(&request.body, "account_id")?)
                            .map_err(|_| ApiFailure::invalid_request(Some("account_id")))?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&principal)
                        .map_err(|_| ApiFailure::unauthenticated())?;
                    let profile = identity_dispatch::profile(&scope).map_err(identity_failure)?;
                    let account = AccountIdentity::new(
                        principal.as_str(),
                        text_field(&profile, "display_name")?,
                    )
                    .map_err(auth_api_failure)?;
                    let challenge = self
                        .passkeys
                        .begin_registration(&mut scope, &account, "Primary passkey", now)
                        .map_err(auth_api_failure)?;
                    self.auth_index
                        .bind_registration(
                            &challenge.registration_id,
                            &principal,
                            challenge.expires_at,
                        )
                        .map_err(auth_failure)?;
                    let result = json!({"registration_id": challenge.registration_id,
                        "ceremony": challenge.ceremony, "expires_at": challenge.expires_at});
                    (principal, result, None)
                }
                "passkey.register.finish" => {
                    let registration_id = path(&request, "registration_id")?;
                    let principal = self
                        .auth_index
                        .resolve_registration(registration_id, now)
                        .map_err(auth_failure)?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&principal)
                        .map_err(|_| ApiFailure::unavailable())?;
                    let passkey = self
                        .passkeys
                        .finish_registration(
                            &mut scope,
                            registration_id,
                            text_field(&request.body, "credential")?,
                            now,
                        )
                        .map_err(auth_api_failure)?;
                    let result = serde_json::to_value(passkey)
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                    (principal, result, None)
                }
                "passkey.assert.begin" => {
                    let email = text_field(&request.body, "email")?;
                    let principal = self
                        .auth_index
                        .resolve_account(email)
                        .map_err(auth_failure)?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&principal)
                        .map_err(|_| ApiFailure::unauthenticated())?;
                    let challenge = self
                        .passkeys
                        .begin_assertion(&mut scope, now)
                        .map_err(auth_api_failure)?;
                    self.auth_index
                        .bind_assertion(&challenge.assertion_id, &principal, challenge.expires_at)
                        .map_err(auth_failure)?;
                    let result = json!({"assertion_id": challenge.assertion_id,
                        "ceremony": challenge.ceremony, "expires_at": challenge.expires_at});
                    (principal, result, None)
                }
                "passkey.assert.finish" => {
                    let assertion_id = path(&request, "assertion_id")?;
                    let principal = self
                        .auth_index
                        .resolve_assertion(assertion_id, now)
                        .map_err(auth_failure)?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&principal)
                        .map_err(|_| ApiFailure::unavailable())?;
                    let proof = self
                        .passkeys
                        .finish_assertion(
                            &mut scope,
                            assertion_id,
                            text_field(&request.body, "credential")?,
                            now,
                        )
                        .map_err(auth_api_failure)?;
                    let result = json!({"assertion_id": proof.assertion_id, "passkey_id": proof.passkey_id,
                        "completed_at": now, "expires_at": proof.expires_at});
                    (principal, result, None)
                }
                "session.open" => {
                    let assertion_id = text_field(&request.body, "assertion_id")?;
                    let principal = self
                        .auth_index
                        .resolve_assertion(assertion_id, now)
                        .map_err(auth_failure)?;
                    let device = request
                        .body
                        .get("device")
                        .and_then(serde_json::Value::as_object)
                        .ok_or_else(|| ApiFailure::invalid_request(Some("device")))?;
                    let device_label = device
                        .get("label")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| ApiFailure::invalid_request(Some("device.label")))?;
                    let device_platform = device
                        .get("platform")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| ApiFailure::invalid_request(Some("device.platform")))?;
                    let idempotency = required_idempotency(&request)?;
                    let action = action_key(idempotency);
                    let recovery_seed = self
                        .auth_index
                        .browser_session_seed(&principal, assertion_id, idempotency)
                        .map_err(auth_failure)?;
                    let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
                    let mut scope = store
                        .principal(&principal)
                        .map_err(|_| ApiFailure::unavailable())?;
                    let mut prepared = self
                        .passkeys
                        .prepare_open_session_replayable(
                            &mut scope,
                            assertion_id,
                            device_label,
                            device_platform,
                            action,
                            recovery_seed,
                            now,
                        )
                        .map_err(auth_api_failure)?;
                    let mut session_seed: [u8; 32] = Sha256::digest(
                        [
                            b"layerx-human/browser-session-signer/v1\0".as_slice(),
                            recovery_seed.as_slice(),
                        ]
                        .concat(),
                    )
                    .into();
                    let session_public_key = LocalSigner::new(session_seed).public_key();
                    session_seed.fill(0);
                    let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                    let authenticated = agent.balance().map_err(agent_failure)?;
                    let identity = agent
                        .identity_resolve(self.agent_actor.as_str())
                        .map_err(agent_failure)?;
                    let registry = agent.registry().clone();
                    let activity_types = registry
                        .registrations()
                        .iter()
                        .filter(|registration| registration.module() == ModuleId::Governance)
                        .flat_map(|registration| registration.activity_types().iter().copied())
                        .collect::<Vec<_>>();
                    if activity_types.is_empty() {
                        return Err(ApiFailure::upstream_degraded());
                    }
                    let issued = issue_session_key(&SessionKeyRequest {
                        grantor: authenticated.account,
                        session_public_key,
                        not_before: prepared.opened_at(),
                        expires_at: Some(prepared.refresh_expires_at()),
                        permitted_activity_types: activity_types,
                        revocation_sequence: Some(identity.revocation_sequence),
                    })
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                    let intent = Intent::v1(IntentKind::SessionGrant(
                        ProtocolSessionGrant::new(issued.registration_payload)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                    ));
                    let trace = TraceId::parse(&request.trace)
                        .map_err(|_| ApiFailure::invalid_request(None))?;
                    let mut adapter = ProductionAgentCreation::new(
                        &mut agent,
                        &self.agent_contract,
                        &self.custody,
                        &trace,
                        self.agent_actor.clone(),
                        self.agent_authority.clone(),
                        self.agent_timestamp_span_seconds,
                        self.agent_fee_limit,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                    let evidence = adapter
                        .submit_lifecycle_intent(
                            &mut scope,
                            &registry,
                            intent,
                            action,
                            KeyId::new("human-primary")
                                .map_err(|_| ApiFailure::upstream_degraded())?,
                            prepared.opened_at(),
                        )
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                    ProductionAgentCreation::finalization_evidence(
                        &evidence,
                        ModuleId::Governance,
                        5,
                        now,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                    prepared
                        .bind_protocol_grant(issued.grant_id)
                        .map_err(auth_api_failure)?;
                    let grant = self
                        .passkeys
                        .commit_open_session(&mut scope, prepared, now)
                        .map_err(auth_api_failure)?;
                    self.auth_index
                        .bind_session(&grant, &principal)
                        .map_err(auth_failure)?;
                    let session_view = grant.session();
                    let result = json!({"session_id": grant.session_id(), "device": {"device_id": session_view.device.device_id(),
                        "label": session_view.device.label(), "platform": session_view.device.platform()}, "opened_at": session_view.opened_at,
                        "last_active_at": session_view.last_active_at, "current": true});
                    let session = SessionSecrets {
                        access_token: grant.access_token().expose().to_owned(),
                        refresh_token: grant.refresh_token().expose().to_owned(),
                        csrf_token: grant.csrf_token().expose().to_owned(),
                        access_max_age_seconds: grant.access_expires_at().saturating_sub(now),
                        refresh_max_age_seconds: grant.refresh_expires_at().saturating_sub(now),
                    };
                    (principal, result, Some(session))
                }
                _ => return Err(ApiFailure::not_found()),
            };
            let _ = principal;
            return Ok(BackendResponse { result, session });
        }
        let context = request
            .principal
            .as_ref()
            .ok_or_else(ApiFailure::unauthenticated)?;
        consume_context(
            &self.auth_index,
            context,
            AuthorizationDisclosure {
                operation: request.operation,
                destination: context.destination(),
                path_parameters: &request.path_parameters,
                body: &request.body,
                idempotency_key: request.idempotency_key.as_deref(),
                trace: &request.trace,
            },
            now()?,
        )
        .map_err(auth_failure)?;
        let principal = context.principal.clone();
        let session_id = context.session_id.clone();
        let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        match request.operation.name.as_str() {
            "agent.create" => {
                let current = now()?;
                let limit = request
                    .body
                    .get("monthly_limit")
                    .ok_or_else(|| ApiFailure::invalid_request(Some("monthly_limit")))?;
                let amount = text_field(limit, "amount")?
                    .parse::<u128>()
                    .map_err(|_| ApiFailure::invalid_request(Some("monthly_limit")))?;
                let creation = CreateAgentRequest::new(
                    text_field(&request.body, "name")?,
                    text_field(&request.body, "purpose")?,
                    amount,
                    text_field(limit, "currency")?,
                )
                .map_err(|_| ApiFailure::invalid_request(None))?;
                let idempotency_key: [u8; 32] =
                    Sha256::digest(required_idempotency(&request)?.as_bytes()).into();
                let creation_context = CreationContext {
                    idempotency_key,
                    owner_account: self.agent_owner_account.clone(),
                    human_recovery_root: self.agent_recovery_root,
                    recovery_threshold: self.agent_recovery_threshold,
                    network_id: self.network_id,
                    protocol_time: current,
                };
                let mut journey = CreationJourney::start(
                    &mut scope,
                    &creation,
                    &creation_context,
                    &self.agent_purpose_catalog,
                    current,
                )
                .map_err(agent_creation_failure)?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let mut runtime = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let registry = runtime.registry().clone();
                let mut adapter = ProductionAgentCreation::new(
                    &mut runtime,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    self.agent_actor.clone(),
                    self.agent_authority.clone(),
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let status = journey
                    .resume(
                        &mut scope,
                        self.custody.creation_keystore(),
                        &registry,
                        &mut adapter,
                        current,
                    )
                    .map_err(agent_creation_failure)?;
                if matches!(status.state, crate::agents::CreationState::Active) {
                    adapter
                        .publish_creation(&journey.projection())
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                }
                Ok(BackendResponse {
                    result: agent_creation_json(&journey, &status),
                    session: None,
                })
            }
            "agent.list" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let page = agent.agent_list(None, 100).map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: json!({"agents": page.agents.iter().map(managed_agent_json).collect::<Vec<_>>(), "next_cursor": page.next_cursor.map(|value| URL_SAFE_NO_PAD.encode(value)).unwrap_or_default()}),
                    session: None,
                })
            }
            "agent.get" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let value = agent
                    .agent_get(path(&request, "agent_id")?)
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_agent_json(&value),
                    session: None,
                })
            }
            "agent.pause" | "agent.resume" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let agent_id = path(&request, "agent_id")?;
                if request.operation.name == "agent.resume" {
                    let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                    let identity = agent
                        .identity_resolve(&context.agent_did)
                        .map_err(agent_failure)?;
                    if identity.frozen {
                        return Err(ApiFailure::upstream_degraded());
                    }
                    let operation_key = action_key(required_idempotency(&request)?);
                    let current = now()?;
                    let trace = TraceId::parse(&request.trace)
                        .map_err(|_| ApiFailure::invalid_request(None))?;
                    let registry = agent.registry().clone();
                    let mut adapter = ProductionAgentCreation::new(
                        &mut agent,
                        &self.agent_contract,
                        &self.custody,
                        &trace,
                        AgentDid::new(context.seed.actor.clone())
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AuthorityRef::new(context.seed.primary_authority.clone())
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        self.agent_timestamp_span_seconds,
                        self.agent_fee_limit,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                    let lifetime = context
                        .seed
                        .session_expiry_unix_seconds
                        .checked_sub(context.seed.created_at)
                        .ok_or_else(ApiFailure::upstream_degraded)?;
                    let expires_at = current
                        .checked_add(lifetime)
                        .ok_or_else(ApiFailure::upstream_degraded)?;
                    let evidence = adapter
                        .provision_session_scoped(
                            &mut scope,
                            &registry,
                            SessionProvision {
                                action_key: operation_key,
                                did: Did::new(context.agent_did.as_bytes())
                                    .map_err(|_| ApiFailure::upstream_degraded())?,
                                activity_types: context
                                    .seed
                                    .activity_types
                                    .iter()
                                    .map(|value| {
                                        layerx_types::payload::ActivityType::from_u32(*value)
                                    })
                                    .collect::<Result<Vec<_>, _>>()
                                    .map_err(|_| ApiFailure::upstream_degraded())?,
                                daemon_scopes: context.seed.session_scopes.clone(),
                                expires_at,
                                primary_authority: context.seed.custody_public_key,
                                grantor: managed_protocol_identity(&context.seed.agent_id)?,
                                custody_key: KeyId::new(context.seed.custody_key.clone())
                                    .map_err(|_| ApiFailure::upstream_degraded())?,
                                revocation_sequence: identity.revocation_sequence,
                            },
                        )
                        .map_err(agent_failure_from_creation_contract)?;
                    let (token_id, generation) = adapter
                        .take_latest_session_credential()
                        .map_err(agent_failure_from_creation_contract)?;
                    let observation = agent
                        .agent_session_bind(agent_id, operation_key, token_id, operation_key)
                        .map_err(agent_failure)?;
                    if observation.generation != generation {
                        return Err(ApiFailure::upstream_degraded());
                    }
                    let finalization = super::agent_runtime::AgentFinalizationEvidence {
                        action_key: operation_key,
                        activity_id: operation_key,
                        receipt_digest: evidence.receipt_digest,
                        observed_sequence: evidence.observed_sequence,
                        verification: evidence.verification_level.wire_rank(),
                        finalized_at: current,
                    };
                    let value = agent
                        .agent_control(agent_id, true, observation.evidence_digest, finalization)
                        .map_err(agent_failure)?;
                    return Ok(BackendResponse {
                        result: managed_agent_json(&value),
                        session: None,
                    });
                }
                let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                let session = agent
                    .agent_session_snapshot(agent_id)
                    .map_err(agent_failure)?;
                if session.agent_did != context.agent_did {
                    return Err(ApiFailure::upstream_degraded());
                }
                let operation_key = action_key(required_idempotency(&request)?);
                let effective_sequence = agent
                    .head()
                    .map_err(agent_failure)?
                    .chain_sequence
                    .checked_add(1)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let intent = Intent::v1(IntentKind::SessionRevoke(
                    SessionRevoke::new(
                        AuthorityGrantId::new(context.protocol_grant_id),
                        SessionRevocationReason::Paused,
                        ProtocolSequence::from_u64(effective_sequence),
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?,
                ));
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let registry = agent.registry().clone();
                let current = now()?;
                let mut adapter = ProductionAgentCreation::new(
                    &mut agent,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    AgentDid::new(context.seed.actor.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    AuthorityRef::new(context.seed.primary_authority.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt = adapter
                    .submit_lifecycle_intent(
                        &mut scope,
                        &registry,
                        intent,
                        operation_key,
                        KeyId::new(context.seed.custody_key)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        current,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let evidence = ProductionAgentCreation::finalization_evidence(
                    &receipt,
                    ModuleId::Governance,
                    6,
                    now()?,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let observation = agent
                    .agent_session_suspend(agent_id, operation_key)
                    .map_err(agent_failure)?;
                let value = agent
                    .agent_control(agent_id, false, observation.evidence_digest, evidence)
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_agent_json(&value),
                    session: None,
                })
            }
            "agent.limit" => {
                let (amount, currency) = money_field(&request.body, "monthly_limit")?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let agent_id = path(&request, "agent_id")?;
                let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                if context.seed.currency != currency {
                    return Err(ApiFailure::invalid_request(Some("monthly_limit")));
                }
                let operation_key = action_key(required_idempotency(&request)?);
                let replacement_budget_id: [u8; 32] = Sha256::digest(
                    [
                        b"layerx-human/agent-limit-budget/v1".as_slice(),
                        context.active_budget_id.as_slice(),
                        operation_key.as_slice(),
                    ]
                    .concat(),
                )
                .into();
                let intent = Intent::v1(IntentKind::BudgetCreate(
                    BudgetCreate::new(
                        BudgetId::new(replacement_budget_id),
                        AccountId::parse(&context.seed.owner_account)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AccountId::parse(&context.seed.budget_account)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AssetId::new(context.seed.budget_asset),
                        ProtocolAmount::from_u128(amount),
                        PeriodLength::new(context.seed.budget_period_seconds)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        RolloverPolicy::None,
                        ProtocolAmount::ZERO,
                        PurposeHash::new(context.seed.purpose_hash),
                        TimestampSeconds::from_u64(
                            now()?
                                .checked_add(context.seed.budget_expiry_seconds)
                                .ok_or_else(ApiFailure::upstream_degraded)?,
                        ),
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?,
                ));
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let registry = agent.registry().clone();
                let current = now()?;
                let mut adapter = ProductionAgentCreation::new(
                    &mut agent,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    AgentDid::new(context.seed.actor.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    AuthorityRef::new(context.seed.primary_authority.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt = adapter
                    .submit_lifecycle_intent(
                        &mut scope,
                        &registry,
                        intent,
                        operation_key,
                        KeyId::new(context.seed.custody_key)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        current,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let evidence = ProductionAgentCreation::finalization_evidence(
                    &receipt,
                    ModuleId::Budget,
                    1,
                    now()?,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let value = agent
                    .agent_limit(agent_id, amount, currency, replacement_budget_id, evidence)
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_agent_json(&value),
                    session: None,
                })
            }
            "agent.reclaim" => {
                let (amount, currency) = money_field(&request.body, "money")?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let agent_id = path(&request, "agent_id")?;
                let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                if context.seed.currency != currency {
                    return Err(ApiFailure::invalid_request(Some("money")));
                }
                let operation_key = action_key(required_idempotency(&request)?);
                let budget_state = agent
                    .agent_budget_state(context.active_budget_id)
                    .map_err(agent_failure)?;
                if budget_state.asset != context.seed.budget_asset
                    || budget_state.remaining < amount
                {
                    return Err(ApiFailure::invalid_request(Some("money")));
                }
                let intent = Intent::v1(IntentKind::BudgetDefund(
                    BudgetDefund::new(
                        BudgetId::new(context.active_budget_id),
                        AccountId::parse(&context.seed.budget_account)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AccountId::parse(&context.seed.owner_account)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AssetId::new(context.seed.budget_asset),
                        ProtocolAmount::from_u128(amount),
                        ProtocolSequence::from_u64(budget_state.revocation_sequence),
                        IdempotencyKey::new(operation_key),
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?,
                ));
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let registry = agent.registry().clone();
                let current = now()?;
                let mut adapter = ProductionAgentCreation::new(
                    &mut agent,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    AgentDid::new(context.seed.actor.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    AuthorityRef::new(context.seed.primary_authority.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt = adapter
                    .submit_lifecycle_intent(
                        &mut scope,
                        &registry,
                        intent,
                        operation_key,
                        KeyId::new(context.seed.custody_key)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        current,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let evidence = ProductionAgentCreation::finalization_evidence(
                    &receipt,
                    ModuleId::Budget,
                    7,
                    now()?,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let post = agent
                    .agent_budget_state(context.active_budget_id)
                    .map_err(agent_failure)?;
                if post.asset != budget_state.asset
                    || post.observed_head_sequence < evidence.observed_sequence
                {
                    return Err(ApiFailure::upstream_degraded());
                }
                let value = agent
                    .agent_reclaim(
                        agent_id,
                        amount,
                        currency,
                        budget_state.evidence_digest,
                        post.evidence_digest,
                        evidence,
                    )
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_journey_json(&value),
                    session: None,
                })
            }
            "agent.rotate" | "agent.recover" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let agent_id = path(&request, "agent_id")?;
                let recover = request.operation.name == "agent.recover";
                if recover {
                    return Err(ApiFailure::upstream_degraded());
                }
                let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                let policy = agent
                    .agent_key_policy(&context.agent_did, false)
                    .map_err(agent_failure)?;
                let operation_key = action_key(required_idempotency(&request)?);
                let pending_key_id =
                    KeyId::new(format!("agent-rotation-{}", hex_bytes(&operation_key)))
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                let pending_public_key = match self
                    .custody
                    .creation_keystore()
                    .describe(&principal, &pending_key_id)
                {
                    Ok(descriptor) if descriptor.class == KeyClass::AgentPrimary => {
                        descriptor.public_key
                    }
                    Ok(_) => return Err(ApiFailure::upstream_degraded()),
                    Err(CustodyError::KeyNotFound) => self
                        .custody
                        .creation_keystore()
                        .create(&principal, &pending_key_id, KeyClass::AgentPrimary)
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    Err(_) => return Err(ApiFailure::upstream_degraded()),
                };
                let current = now()?;
                let effective_at = current
                    .checked_add(policy.required_delay_seconds)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let lapse_at = effective_at
                    .checked_add(policy.required_delay_seconds)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let intent = Intent::v1(IntentKind::KeyRotation(
                    KeyRotation::new(
                        Did::new(context.agent_did.as_bytes())
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        PublicKey::new(pending_public_key),
                        TimestampBound::new(effective_at, lapse_at)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        ProtocolSequence::from_u64(policy.effective_sequence),
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?,
                ));
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let registry = agent.registry().clone();
                let mut adapter = ProductionAgentCreation::new(
                    &mut agent,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    AgentDid::new(context.seed.actor.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    AuthorityRef::new(context.seed.primary_authority.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt = adapter
                    .submit_lifecycle_intent(
                        &mut scope,
                        &registry,
                        intent,
                        operation_key,
                        KeyId::new(context.seed.custody_key)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        current,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let finalized_at = now()?;
                let evidence = ProductionAgentCreation::finalization_evidence(
                    &receipt,
                    ModuleId::Governance,
                    2,
                    finalized_at,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let ready_at = finalized_at
                    .checked_add(policy.required_delay_seconds)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let value = agent
                    .agent_key_change(
                        agent_id,
                        false,
                        policy.required_delay_seconds,
                        ready_at,
                        evidence,
                    )
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_challenge_json(&value),
                    session: None,
                })
            }
            "agent.archive" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let agent_id = path(&request, "agent_id")?;
                let confirm_name = text_field(&request.body, "confirm_name")?;
                let context = agent.agent_context(agent_id).map_err(agent_failure)?;
                let operation_key = action_key(required_idempotency(&request)?);
                let pre = agent
                    .agent_budget_state(context.active_budget_id)
                    .map_err(agent_failure)?;
                if pre.asset != context.seed.budget_asset {
                    return Err(ApiFailure::upstream_degraded());
                }
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let registry = agent.registry().clone();
                if pre.remaining != 0 {
                    let defund_key: [u8; 32] = Sha256::digest(
                        [
                            b"layerx-human/agent-archive-defund/v1\0".as_slice(),
                            operation_key.as_slice(),
                        ]
                        .concat(),
                    )
                    .into();
                    let intent = Intent::v1(IntentKind::BudgetDefund(
                        BudgetDefund::new(
                            BudgetId::new(context.active_budget_id),
                            AccountId::parse(&context.seed.budget_account)
                                .map_err(|_| ApiFailure::upstream_degraded())?,
                            AccountId::parse(&context.seed.owner_account)
                                .map_err(|_| ApiFailure::upstream_degraded())?,
                            AssetId::new(context.seed.budget_asset),
                            ProtocolAmount::from_u128(pre.remaining),
                            ProtocolSequence::from_u64(pre.revocation_sequence),
                            IdempotencyKey::new(defund_key),
                        )
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    ));
                    let current = now()?;
                    let mut adapter = ProductionAgentCreation::new(
                        &mut agent,
                        &self.agent_contract,
                        &self.custody,
                        &trace,
                        AgentDid::new(context.seed.actor.clone())
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        AuthorityRef::new(context.seed.primary_authority.clone())
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        self.agent_timestamp_span_seconds,
                        self.agent_fee_limit,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                    let receipt = adapter
                        .submit_lifecycle_intent(
                            &mut scope,
                            &registry,
                            intent,
                            defund_key,
                            KeyId::new(context.seed.custody_key.clone())
                                .map_err(|_| ApiFailure::upstream_degraded())?,
                            current,
                        )
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                    ProductionAgentCreation::finalization_evidence(
                        &receipt,
                        ModuleId::Budget,
                        7,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                }
                let session = agent
                    .agent_session_snapshot(agent_id)
                    .map_err(agent_failure)?;
                if session.agent_did != context.agent_did {
                    return Err(ApiFailure::upstream_degraded());
                }
                let effective_sequence = agent
                    .head()
                    .map_err(agent_failure)?
                    .chain_sequence
                    .checked_add(1)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let intent = Intent::v1(IntentKind::SessionRevoke(
                    SessionRevoke::new(
                        AuthorityGrantId::new(context.protocol_grant_id),
                        SessionRevocationReason::Archived,
                        ProtocolSequence::from_u64(effective_sequence),
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?,
                ));
                let current = now()?;
                let mut adapter = ProductionAgentCreation::new(
                    &mut agent,
                    &self.agent_contract,
                    &self.custody,
                    &trace,
                    AgentDid::new(context.seed.actor.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    AuthorityRef::new(context.seed.primary_authority.clone())
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    self.agent_timestamp_span_seconds,
                    self.agent_fee_limit,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt = adapter
                    .submit_lifecycle_intent(
                        &mut scope,
                        &registry,
                        intent,
                        operation_key,
                        KeyId::new(context.seed.custody_key)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        current,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let evidence = ProductionAgentCreation::finalization_evidence(
                    &receipt,
                    ModuleId::Governance,
                    6,
                    now()?,
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let session_observation = agent
                    .agent_session_suspend(agent_id, operation_key)
                    .map_err(agent_failure)?;
                let post = agent
                    .agent_budget_state(context.active_budget_id)
                    .map_err(agent_failure)?;
                if post.asset != pre.asset
                    || post.remaining != 0
                    || post.observed_head_sequence < evidence.observed_sequence
                {
                    return Err(ApiFailure::upstream_degraded());
                }
                let value = agent
                    .agent_archive(
                        agent_id,
                        confirm_name,
                        pre.evidence_digest,
                        post.evidence_digest,
                        session_observation.evidence_digest,
                        evidence,
                    )
                    .map_err(agent_failure)?;
                Ok(BackendResponse {
                    result: managed_journey_json(&value),
                    session: None,
                })
            }
            "security.action" => {
                let digest = identity_dispatch::security_digest(
                    &scope,
                    text_field(&request.body, "action")?,
                    request
                        .body
                        .get("target_id")
                        .and_then(serde_json::Value::as_str),
                )
                .map_err(identity_failure)?;
                Ok(BackendResponse {
                    result: json!({"confirms": format!("opd_{}", URL_SAFE_NO_PAD.encode(digest.bytes()))}),
                    session: None,
                })
            }
            "stepup.begin" => {
                let encoded = text_field(&request.body, "confirms")?
                    .strip_prefix("opd_")
                    .ok_or_else(|| ApiFailure::invalid_request(Some("confirms")))?;
                let bytes = URL_SAFE_NO_PAD
                    .decode(encoded)
                    .map_err(|_| ApiFailure::invalid_request(Some("confirms")))?;
                let digest = crate::auth::OperationDigest::new(
                    bytes
                        .try_into()
                        .map_err(|_| ApiFailure::invalid_request(Some("confirms")))?,
                );
                let challenge = self
                    .passkeys
                    .begin_step_up_authorized(&mut scope, digest, now()?)
                    .map_err(auth_api_failure)?;
                self.auth_index
                    .bind_step_up(&challenge.challenge_id, &principal, challenge.expires_at)
                    .map_err(auth_failure)?;
                Ok(BackendResponse {
                    result: identity_dispatch::step_up_challenge(&challenge),
                    session: None,
                })
            }
            "stepup.finish" => {
                let evidence = self
                    .passkeys
                    .finish_step_up(
                        &mut scope,
                        path(&request, "challenge_id")?,
                        text_field(&request.body, "credential")?,
                        now()?,
                    )
                    .map_err(auth_api_failure)?;
                Ok(BackendResponse {
                    result: identity_dispatch::step_up_evidence(&evidence),
                    session: None,
                })
            }
            "profile.get" => Ok(BackendResponse {
                result: identity_dispatch::profile(&scope).map_err(identity_failure)?,
                session: None,
            }),
            "profile.update" => Ok(BackendResponse {
                result: identity_dispatch::update_profile(&mut scope, &request.body, now()?)
                    .map_err(identity_failure)?,
                session: None,
            }),
            "onboarding.status" => {
                let journey = OnboardingJourney::load(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .ok_or_else(ApiFailure::not_found)?;
                Ok(BackendResponse {
                    result: identity_dispatch::onboarding_status(&journey.status()),
                    session: None,
                })
            }
            "onboarding.resume" => {
                let mut journey = OnboardingJourney::load(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .ok_or_else(ApiFailure::not_found)?;
                let status = self
                    .custody
                    .resume_onboarding_local(&mut journey, &mut scope, now()?)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .account_sequence(&self.agent_actor, &self.agent_authority)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let registry = agent.registry().clone();
                let mut engine = journey
                    .start_durable_engine(
                        &mut scope,
                        &registry,
                        self.agent_actor.clone(),
                        self.agent_authority.clone(),
                        sequence,
                        now()?,
                        now()?
                            .checked_add(self.agent_timestamp_span_seconds)
                            .ok_or_else(ApiFailure::unavailable)?,
                        self.agent_fee_limit,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let _ = super::executor::poll_once_ready(engine.advance(
                    &mut scope,
                    &self.agent_contract,
                    &mut *agent,
                    &self.custody,
                    &registry,
                    &trace,
                    now()?,
                ))
                .map_err(|_| ApiFailure::upstream_degraded())?
                .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: identity_dispatch::onboarding_status(&status),
                    session: None,
                })
            }
            "binding.statement" => {
                let journey = OnboardingJourney::load(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .ok_or_else(ApiFailure::not_found)?;
                let did = journey.did().map_err(|_| ApiFailure::upstream_degraded())?;
                let address = text_field(&request.body, "address")?;
                let bytes = decode_hex_20(address)
                    .map_err(|_| ApiFailure::invalid_request(Some("address")))?;
                let statement = BindingJourney::issue_statement(
                    &did,
                    layerx_types::intent::NetworkId::new(self.network_id)
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    EvmAddress::new(bytes),
                    now()?,
                    self.binding_statement_ttl_seconds,
                )
                .map_err(|_| ApiFailure::invalid_request(Some("address")))?;
                scope
                    .put(
                        Table::Journeys,
                        RowKey::new("wallet-binding-issued")
                            .map_err(|_| ApiFailure::unavailable())?,
                        now()?,
                        serde_json::to_vec(&statement)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                Ok(BackendResponse {
                    result: json!({"statement": statement.text(),
                    "address": statement.checksummed_address(), "expires_at": statement.expires_at()}),
                    session: None,
                })
            }
            "binding.submit" => {
                let row =
                    RowKey::new("wallet-binding-issued").map_err(|_| ApiFailure::unavailable())?;
                let statement: crate::binding::BindingStatement = serde_json::from_slice(
                    scope
                        .get(Table::Journeys, &row)
                        .ok_or_else(ApiFailure::not_found)?
                        .bytes(),
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                if statement.text() != text_field(&request.body, "statement")?
                    || statement.checksummed_address() != text_field(&request.body, "address")?
                {
                    return Err(ApiFailure::forbidden());
                }
                let signature = decode_hex(text_field(&request.body, "signature")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("signature")))?;
                let key: [u8; 32] =
                    sha2::Sha256::digest(required_idempotency(&request)?.as_bytes()).into();
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .account_sequence(&self.agent_actor, &self.agent_authority)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let registry = agent.registry().clone();
                let binding = BindingJourney::new(registry.clone());
                let mut engine = binding
                    .start_durable(
                        &mut scope,
                        &statement,
                        &signature,
                        layerx_types::ids::IdempotencyKey::new(key),
                        self.agent_actor.clone(),
                        self.agent_authority.clone(),
                        sequence,
                        now()?,
                        now()?
                            .checked_add(self.agent_timestamp_span_seconds)
                            .ok_or_else(ApiFailure::unavailable)?,
                        self.agent_fee_limit,
                        false,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let status = super::executor::poll_once_ready(engine.advance(
                    &mut scope,
                    &self.agent_contract,
                    &mut *agent,
                    &self.custody,
                    &registry,
                    &trace,
                    now()?,
                ))
                .map_err(|_| ApiFailure::upstream_degraded())?
                .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: journey_status_json(&status),
                    session: None,
                })
            }
            "binding.status" => {
                let engine = crate::journeys::JourneyEngine::list(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .into_iter()
                    .find(|value| value.kind() == crate::journeys::JourneyKind::WalletBinding);
                match engine {
                    Some(engine) => {
                        let status = engine
                            .status()
                            .map_err(|_| ApiFailure::upstream_degraded())?;
                        let identity = engine.verified_identity(0).map(|(submission, activity)| json!({
                        "submission_id": submission, "activity_id": URL_SAFE_NO_PAD.encode(activity)}));
                        Ok(BackendResponse {
                            result: json!({"state": if identity.is_some() {"bound"} else {"binding"},
                        "journey": journey_status_json(&status), "verified_identity": identity}),
                            session: None,
                        })
                    }
                    None => Ok(BackendResponse {
                        result: json!({"state":"none"}),
                        session: None,
                    }),
                }
            }
            "binding.rebind.action" => {
                let engine = crate::journeys::JourneyEngine::list(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .into_iter()
                    .find(|value| value.kind() == crate::journeys::JourneyKind::WalletBinding)
                    .ok_or_else(ApiFailure::not_found)?;
                let status = engine
                    .status()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let receipt_digest = status
                    .receipt_digests()
                    .first()
                    .copied()
                    .flatten()
                    .ok_or_else(ApiFailure::forbidden)?;
                let prior: crate::binding::BindingStatement = serde_json::from_slice(
                    scope
                        .get(
                            Table::Journeys,
                            &RowKey::new("wallet-binding-issued")
                                .map_err(|_| ApiFailure::unavailable())?,
                        )
                        .ok_or_else(ApiFailure::not_found)?
                        .bytes(),
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                let journey = OnboardingJourney::load(&scope)
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .ok_or_else(ApiFailure::not_found)?;
                let address = decode_hex_20(text_field(&request.body, "address")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("address")))?;
                let statement = BindingJourney::issue_statement(
                    &journey.did().map_err(|_| ApiFailure::upstream_degraded())?,
                    layerx_types::intent::NetworkId::new(self.network_id)
                        .map_err(|_| ApiFailure::upstream_degraded())?,
                    EvmAddress::new(address),
                    now()?,
                    self.binding_statement_ttl_seconds,
                )
                .map_err(|_| ApiFailure::invalid_request(Some("address")))?;
                let confirms = BindingJourney::rebind_operation_digest_verified(
                    receipt_digest,
                    prior.address(),
                    &statement,
                );
                scope
                    .put(
                        Table::Journeys,
                        RowKey::new("wallet-rebinding-issued")
                            .map_err(|_| ApiFailure::unavailable())?,
                        now()?,
                        serde_json::to_vec(&(statement.clone(), confirms))
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                Ok(BackendResponse {
                    result: json!({"binding":{"statement":statement.text(),
                    "address":statement.checksummed_address(),"expires_at":statement.expires_at()},
                    "confirms":format!("opd_{}",URL_SAFE_NO_PAD.encode(confirms.bytes()))}),
                    session: None,
                })
            }
            "binding.rebind" => {
                let (statement, confirms): (
                    crate::binding::BindingStatement,
                    crate::auth::OperationDigest,
                ) = serde_json::from_slice(
                    scope
                        .get(
                            Table::Journeys,
                            &RowKey::new("wallet-rebinding-issued")
                                .map_err(|_| ApiFailure::unavailable())?,
                        )
                        .ok_or_else(ApiFailure::not_found)?
                        .bytes(),
                )
                .map_err(|_| ApiFailure::upstream_degraded())?;
                if statement.text() != text_field(&request.body, "statement")?
                    || statement.checksummed_address() != text_field(&request.body, "address")?
                {
                    return Err(ApiFailure::forbidden());
                }
                let signature = decode_hex(text_field(&request.body, "signature")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("signature")))?;
                let key: [u8; 32] =
                    sha2::Sha256::digest(required_idempotency(&request)?.as_bytes()).into();
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .account_sequence(&self.agent_actor, &self.agent_authority)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let registry = agent.registry().clone();
                let binding = BindingJourney::new(registry.clone());
                let mut engine = binding
                    .start_durable(
                        &mut scope,
                        &statement,
                        &signature,
                        layerx_types::ids::IdempotencyKey::new(key),
                        self.agent_actor.clone(),
                        self.agent_authority.clone(),
                        sequence,
                        now()?,
                        now()?
                            .checked_add(self.agent_timestamp_span_seconds)
                            .ok_or_else(ApiFailure::unavailable)?,
                        self.agent_fee_limit,
                        true,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let phase = engine
                    .status()
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .phases()
                    .get(
                        engine
                            .status()
                            .map_err(|_| ApiFailure::upstream_degraded())?
                            .current_leg(),
                    )
                    .copied();
                let custody_evidence = if phase == Some(crate::journeys::JourneyPhase::Prepared) {
                    let prepared = engine
                        .prepared_disclosure_digest(&self.agent_contract, &mut *agent, &registry)
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                    let challenge = request
                        .body
                        .get("step_up")
                        .and_then(|value| value.get("challenge_id"))
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(ApiFailure::forbidden)?;
                    let auth = self
                        .passkeys
                        .load_step_up_evidence(&mut scope, challenge, now()?)
                        .map_err(auth_api_failure)?;
                    Some(
                        self.custody
                            .bind_authenticated_step_up(
                                &self.passkeys,
                                &mut scope,
                                &auth,
                                confirms,
                                crate::custody::Operation::WalletRebinding,
                                prepared,
                                context.request_digest(),
                                now()?,
                            )
                            .map_err(|_| ApiFailure::forbidden())?,
                    )
                } else {
                    None
                };
                let status = super::executor::poll_once_ready(engine.advance_authorized(
                    &mut scope,
                    &self.agent_contract,
                    &mut *agent,
                    &self.custody,
                    &registry,
                    &trace,
                    custody_evidence.as_ref(),
                    now()?,
                ))
                .map_err(|_| ApiFailure::upstream_degraded())?
                .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: journey_status_json(&status),
                    session: None,
                })
            }
            "move.quote" => {
                let planning =
                    movement_request(&request, &principal, scope.tenant().clone(), now()?)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let quote = movement
                    .quote_move(&mut scope, planning)
                    .map_err(movement_failure)?;
                Ok(BackendResponse {
                    result: move_quote_json(&quote),
                    session: None,
                })
            }
            "move.commit" => {
                let quote_id = text_field(&request.body, "quote_id")?;
                let movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let plan = movement
                    .load_move_quote(
                        &scope,
                        quote_id,
                        action_key(required_idempotency(&request)?),
                        now()?,
                    )
                    .map_err(movement_failure)?;
                let registry = self
                    .agent
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?
                    .registry()
                    .clone();
                let mut journey = crate::journeys::MoveJourney::commit(
                    &mut scope,
                    &plan,
                    crate::journeys::MoveAuthorization::Allowed,
                    &registry,
                    now()?,
                )
                .map_err(move_journey_failure)?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let status = super::executor::poll_once_ready(journey.advance(
                    &mut scope,
                    &self.agent_contract,
                    &mut *agent,
                    &self.custody,
                    &registry,
                    &trace,
                    now()?,
                ))
                .map_err(|_| ApiFailure::upstream_degraded())?
                .map_err(move_journey_failure)?;
                schedule_continuation(&mut scope, "move", status.journey_id(), now()?)?;
                Ok(BackendResponse {
                    result: move_public_json(&scope, self.settlement_domain, &status, now()?)?,
                    session: None,
                })
            }
            "deposit.start" => {
                let planning =
                    movement_request(&request, &principal, scope.tenant().clone(), now()?)?;
                let movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let plan = movement.deposit_plan(planning).map_err(movement_failure)?;
                let binding = crate::binding::BindingJourney::new(
                    self.agent
                        .lock()
                        .map_err(|_| ApiFailure::unavailable())?
                        .registry()
                        .clone(),
                );
                let journey =
                    crate::journeys::DepositJourney::start(&mut scope, &binding, &plan, now()?)
                        .map_err(deposit_journey_failure)?;
                let status = journey.status().map_err(deposit_journey_failure)?;
                Ok(BackendResponse {
                    result: deposit_public_json(&scope, self.settlement_domain, &status, now()?)?,
                    session: None,
                })
            }
            "deposit.confirm" => {
                let journey_id =
                    crate::notify::JourneyId::new(path(&request, "journey_id")?.to_owned())
                        .map_err(|_| ApiFailure::invalid_request(Some("journey_id")))?;
                let transaction = layerx_paxeer_client::TransactionHash::new(
                    decode_hex_32(text_field(&request.body, "wallet_transaction")?)
                        .map_err(|_| ApiFailure::invalid_request(Some("wallet_transaction")))?,
                );
                let mut journey = crate::journeys::DepositJourney::load(&scope, &journey_id)
                    .map_err(deposit_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                journey
                    .confirm_external_transaction(&mut scope, &mut *movement, transaction, now()?)
                    .map_err(deposit_journey_failure)?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let registry = agent.registry().clone();
                let status = movement
                    .advance_deposit(
                        &mut scope,
                        &mut journey,
                        &self.agent_contract,
                        &mut *agent,
                        &self.custody,
                        &registry,
                        &trace,
                        now()?,
                    )
                    .map_err(deposit_journey_failure)?;
                schedule_continuation(&mut scope, "deposit", status.journey_id(), now()?)?;
                Ok(BackendResponse {
                    result: deposit_public_json(&scope, self.settlement_domain, &status, now()?)?,
                    session: None,
                })
            }
            "withdraw.start" => {
                let planning =
                    movement_request(&request, &principal, scope.tenant().clone(), now()?)?;
                let challenge = request
                    .body
                    .get("step_up")
                    .and_then(|value| value.get("challenge_id"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(ApiFailure::forbidden)?;
                let step_up = self
                    .passkeys
                    .load_step_up_evidence(&mut scope, challenge, now()?)
                    .map_err(auth_api_failure)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let plan = movement
                    .withdrawal_plan(planning)
                    .map_err(movement_failure)?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let registry = agent.registry().clone();
                let observed_at = now()?;
                let mut journey = movement
                    .start_withdrawal(&mut scope, &plan, observed_at)
                    .map_err(withdrawal_journey_failure)?;
                let mut status = journey.status().map_err(withdrawal_journey_failure)?;
                for _ in 0..3 {
                    let custody_evidence = journey
                        .prepared_debit_disclosure_digest(
                            &scope,
                            &self.agent_contract,
                            &mut *agent,
                            &registry,
                        )
                        .map_err(withdrawal_journey_failure)?
                        .map(|prepared| {
                            self.custody.bind_authenticated_step_up(
                                &self.passkeys,
                                &mut scope,
                                &step_up,
                                step_up.confirms(),
                                crate::custody::Operation::Withdrawal,
                                prepared,
                                context.request_digest(),
                                observed_at,
                            )
                        })
                        .transpose()
                        .map_err(|_| ApiFailure::forbidden())?;
                    status = movement
                        .advance_withdrawal(
                            &mut scope,
                            &mut journey,
                            &self.agent_contract,
                            &mut *agent,
                            &self.custody,
                            &registry,
                            &trace,
                            custody_evidence.as_ref(),
                            observed_at,
                        )
                        .map_err(withdrawal_journey_failure)?;
                    if custody_evidence.is_some()
                        || !matches!(status.stage(), crate::journeys::WithdrawalStage::Processing)
                    {
                        break;
                    }
                }
                schedule_continuation(&mut scope, "withdraw", status.journey_id(), now()?)?;
                Ok(BackendResponse {
                    result: withdrawal_public_json(
                        &scope,
                        self.settlement_domain,
                        &status,
                        now()?,
                    )?,
                    session: None,
                })
            }
            "withdraw.claim" => {
                let journey_id =
                    crate::notify::JourneyId::new(path(&request, "journey_id")?.to_owned())
                        .map_err(|_| ApiFailure::invalid_request(Some("journey_id")))?;
                let signature = decode_hex(text_field(&request.body, "claim_signature")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("claim_signature")))?;
                let mut journey = crate::journeys::WithdrawalJourney::load(&mut scope, &journey_id)
                    .map_err(withdrawal_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = movement
                    .claim_withdrawal(&mut scope, &mut journey, &signature, now()?)
                    .map_err(withdrawal_journey_failure)?;
                schedule_continuation(&mut scope, "withdraw", status.journey_id(), now()?)?;
                Ok(BackendResponse {
                    result: withdrawal_public_json(
                        &scope,
                        self.settlement_domain,
                        &status,
                        now()?,
                    )?,
                    session: None,
                })
            }
            "exit.start" => {
                let confirmation = crate::journeys::IrreversibleExitConfirmation::parse(
                    text_field(&request.body, "confirmation")?,
                )
                .map_err(|_| ApiFailure::invalid_request(Some("confirmation")))?;
                let trace = TraceId::parse(&request.trace)
                    .map_err(|_| ApiFailure::invalid_request(None))?;
                let planning =
                    movement_request(&request, &principal, scope.tenant().clone(), now()?)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let plan = movement.exit_plan(planning).map_err(movement_failure)?;
                let status = movement
                    .start_exit(
                        &mut scope,
                        &trace,
                        &self.emergency_exit,
                        &plan,
                        confirmation,
                        now()?,
                    )
                    .map_err(exit_journey_failure)?;
                schedule_continuation(&mut scope, "exit", status.journey_id(), now()?)?;
                Ok(BackendResponse {
                    result: exit_public_json(&scope, &status, now()?)?,
                    session: None,
                })
            }
            "exit.eligibility" => {
                let result = self
                    .emergency_exit
                    .eligibility()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let response = match result {
                    ExitEligibility::Eligible { .. } => {
                        json!({"eligible": true, "copy_key": "exit.eligible", "settlement_domain": "paxeer"})
                    }
                    ExitEligibility::NetworkOperatingNormally { .. } => json!({"eligible": false,
                        "copy_key": "exit.network-operating-normally", "withdraw_instead_path": "/app/withdraw",
                        "settlement_domain": "paxeer"}),
                    ExitEligibility::NoFinalisedCheckpoint => json!({"eligible": false,
                        "copy_key": "exit.no-finalised-checkpoint", "settlement_domain": "paxeer"}),
                };
                Ok(BackendResponse {
                    result: response,
                    session: None,
                })
            }
            "security.passkey.list" => {
                let passkeys = self
                    .passkeys
                    .list_passkeys_authorized(&scope)
                    .map_err(auth_api_failure)?;
                Ok(BackendResponse {
                    result: json!({"passkeys": passkeys}),
                    session: None,
                })
            }
            "security.passkey.register.begin" => {
                let profile = identity_dispatch::profile(&scope).map_err(identity_failure)?;
                let account =
                    AccountIdentity::new(principal.as_str(), text_field(&profile, "display_name")?)
                        .map_err(auth_api_failure)?;
                let challenge = self
                    .passkeys
                    .begin_registration(
                        &mut scope,
                        &account,
                        text_field(&request.body, "label")?,
                        now()?,
                    )
                    .map_err(auth_api_failure)?;
                self.auth_index
                    .bind_registration(&challenge.registration_id, &principal, challenge.expires_at)
                    .map_err(auth_failure)?;
                Ok(BackendResponse {
                    result: json!({"registration_id": challenge.registration_id,
                    "ceremony": challenge.ceremony, "expires_at": challenge.expires_at}),
                    session: None,
                })
            }
            "security.passkey.register.finish" => {
                let registration_id = path(&request, "registration_id")?;
                if self
                    .auth_index
                    .resolve_registration(registration_id, now()?)
                    .map_err(auth_failure)?
                    != principal
                {
                    return Err(ApiFailure::forbidden());
                }
                let passkey = self
                    .passkeys
                    .finish_registration(
                        &mut scope,
                        registration_id,
                        text_field(&request.body, "credential")?,
                        now()?,
                    )
                    .map_err(auth_api_failure)?;
                response(passkey)
            }
            "security.passkey.revoke" => {
                let passkeys = self
                    .passkeys
                    .revoke_passkey_authorized(&mut scope, path(&request, "passkey_id")?)
                    .map_err(auth_api_failure)?;
                Ok(BackendResponse {
                    result: json!({"passkeys": passkeys}),
                    session: None,
                })
            }
            "session.refresh" => {
                let (refresh, csrf) = context
                    .refresh_credentials()
                    .ok_or_else(ApiFailure::unauthenticated)?;
                let grant = self
                    .passkeys
                    .refresh_authorized(&mut scope, refresh, csrf, now()?)
                    .map_err(auth_api_failure)?;
                self.auth_index
                    .bind_session(&grant, &principal)
                    .map_err(auth_failure)?;
                let current = self
                    .passkeys
                    .list_sessions_authorized(&scope, grant.session_id())
                    .map_err(auth_api_failure)?
                    .into_iter()
                    .find(|value| value.current)
                    .ok_or_else(ApiFailure::unavailable)?;
                let result = json!({"session_id": grant.session_id(), "device": {"device_id": current.device.device_id(),
                    "label": current.device.label(), "platform": current.device.platform()}, "opened_at": current.opened_at,
                    "last_active_at": current.last_active_at, "current": true});
                Ok(BackendResponse {
                    result,
                    session: Some(SessionSecrets {
                        access_token: grant.access_token().expose().to_owned(),
                        refresh_token: grant.refresh_token().expose().to_owned(),
                        csrf_token: grant.csrf_token().expose().to_owned(),
                        access_max_age_seconds: grant.access_expires_at().saturating_sub(now()?),
                        refresh_max_age_seconds: grant.refresh_expires_at().saturating_sub(now()?),
                    }),
                })
            }
            "authenticator.status" => {
                let provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = provider
                    .status(&principal)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: authenticator_status_json(&status),
                    session: None,
                })
            }
            "authenticator.setup.begin" => {
                let mut provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let challenge = provider
                    .begin_setup(&principal, text_field(&request.body, "label")?, now()?)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: json!({"setup_id": challenge.setup_id,
                    "secret": timed_secret_json(&challenge.secret), "otpauth_uri": timed_secret_json(&challenge.otpauth_uri),
                    "expires_at": challenge.expires_at}),
                    session: None,
                })
            }
            "authenticator.setup.finish" => {
                let mut provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let result = provider
                    .finish_setup(
                        &principal,
                        path(&request, "setup_id")?,
                        text_field(&request.body, "code")?,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: json!({"method": authenticator_method_json(&result.method),
                    "backup_codes": {"codes": result.backup_codes.expose(), "remask_at": result.backup_codes.remask_at(),
                        "copyable": true}}),
                    session: None,
                })
            }
            "authenticator.disable" => {
                let mut provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = provider
                    .disable(&principal, path(&request, "authenticator_id")?, now()?)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: authenticator_status_json(&status),
                    session: None,
                })
            }
            "authenticator.backup.rotate" => {
                let mut provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let codes = provider
                    .rotate_backup_codes(&principal, now()?)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: json!({"codes": codes.expose(), "remask_at": codes.remask_at(),
                    "copyable": true}),
                    session: None,
                })
            }
            "security.recovery.reveal" => {
                let provider = self
                    .security
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let secret = provider
                    .reveal_verified_receipt(
                        &principal,
                        text_field(&request.body, "evidence_id")?,
                        now()?,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                Ok(BackendResponse {
                    result: timed_secret_json(&secret),
                    session: None,
                })
            }
            "session.list" => {
                let sessions = self
                    .passkeys
                    .list_sessions_authorized(&scope, &session_id)
                    .map_err(auth_api_failure)?;
                let sessions = sessions.into_iter().map(|session| json!({
                    "session_id": session.session_id,
                    "device": {"device_id": session.device.device_id(), "label": session.device.label(),
                        "platform": session.device.platform()},
                    "opened_at": session.opened_at, "last_active_at": session.last_active_at,
                    "current": session.current
                })).collect::<Vec<_>>();
                Ok(BackendResponse {
                    result: json!({"sessions": sessions}),
                    session: None,
                })
            }
            "session.revoke" | "security.session.revoke" => {
                let target = path(&request, "session_id")?;
                let grant = self
                    .passkeys
                    .protocol_grant_for_session(&scope, target)
                    .map_err(auth_api_failure)?;
                self.revoke_browser_grants(&mut scope, &request, &[(target.to_owned(), grant)])?;
                let revoked = self
                    .passkeys
                    .revoke_session_authorized(&mut scope, target, now()?)
                    .map_err(auth_api_failure)?;
                Ok(BackendResponse {
                    result: json!({"revoked_session_ids": revoked.revoked_session_ids,
                    "revoked_at": revoked.revoked_at}),
                    session: None,
                })
            }
            "session.revoke-all" | "security.session.revoke-all" => {
                let grants = self
                    .passkeys
                    .active_protocol_session_grants(&scope)
                    .map_err(auth_api_failure)?;
                self.revoke_browser_grants(&mut scope, &request, &grants)?;
                let revoked = self
                    .passkeys
                    .revoke_all_sessions_authorized(&mut scope, now()?)
                    .map_err(auth_api_failure)?;
                Ok(BackendResponse {
                    result: json!({"revoked_session_ids": revoked.revoked_session_ids,
                    "revoked_at": revoked.revoked_at}),
                    session: None,
                })
            }
            "approval.list" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .chain_sequence;
                let page = agent
                    .approval_list(sequence, None, 100)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let mut approvals = Vec::with_capacity(page.approvals.len());
                for hold in &page.approvals {
                    let budget = agent
                        .verified_budget_after(hold, sequence)
                        .map_err(|_| ApiFailure::upstream_degraded())?;
                    let value = approval_json(hold, budget);
                    append_approval_stream(
                        &self.stream,
                        &mut scope,
                        hold,
                        value.clone(),
                        sequence,
                    )?;
                    approvals.push(value);
                }
                Ok(BackendResponse {
                    result: json!({"approvals": approvals, "next_cursor": page.next_cursor.map(|value| URL_SAFE_NO_PAD.encode(value))}),
                    session: None,
                })
            }
            "approval.get" => {
                let approval_id = decode_id(path(&request, "approval_id")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("approval_id")))?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .chain_sequence;
                let hold = agent
                    .approval_get(approval_id, sequence)
                    .map_err(|_| ApiFailure::not_found())?;
                let budget = agent
                    .verified_budget_after(&hold, sequence)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let value = approval_json(&hold, budget);
                append_approval_stream(&self.stream, &mut scope, &hold, value.clone(), sequence)?;
                Ok(BackendResponse {
                    result: value,
                    session: None,
                })
            }
            "approval.approve" | "approval.reject" => {
                let approval_id = decode_id(path(&request, "approval_id")?)
                    .map_err(|_| ApiFailure::invalid_request(Some("approval_id")))?;
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let sequence = agent
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?
                    .chain_sequence;
                let hold = agent
                    .approval_get(approval_id, sequence)
                    .map_err(|_| ApiFailure::not_found())?;
                let decision = agent
                    .approval_decide(
                        request.operation.name == "approval.approve",
                        approval_id,
                        hold.canonical_bytes_digest,
                        required_idempotency(&request)?,
                        sequence,
                    )
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let state = match decision.status {
                    AgentDecisionStatus::Approved { .. } => "approved",
                    AgentDecisionStatus::Rejected => "rejected",
                    AgentDecisionStatus::Expired => "expired",
                    AgentDecisionStatus::Defective => "defective",
                };
                let value = json!({"approval_id": URL_SAFE_NO_PAD.encode(approval_id), "state": state,
                    "state_copy_key": format!("approval.state.{state}"), "money_moved": false,
                    "moved_copy_key": "approval.decision.no-money-moved", "evidence": []});
                self.stream.append(
                    &mut scope,
                    &format!(
                        "approval-decision:{}:{state}",
                        URL_SAFE_NO_PAD.encode(approval_id)
                    ),
                    match state {
                        "approved" => "approval-approved",
                        "rejected" => "approval-rejected",
                        _ => "approval-expired",
                    },
                    sequence,
                    json!({"approval":value.clone()}),
                )?;
                Ok(BackendResponse {
                    result: value,
                    session: None,
                })
            }
            "support.list" => {
                let conversations = SupportService::list(&scope).map_err(support_failure)?;
                Ok(BackendResponse {
                    result: json!({"conversations": conversations}),
                    session: None,
                })
            }
            "support.create" => {
                let mut create = CreateConversation::new(
                    text_field(&request.body, "body")?,
                    decode_field::<Shell>(&request.body, "shell")?,
                )
                .map_err(|_| ApiFailure::invalid_request(Some("body")))?;
                if let Some(topic) = optional_decode_field::<Topic>(&request.body, "topic")? {
                    create = create.with_topic(topic);
                }
                if let Some(trace) = request
                    .body
                    .get("trace_id")
                    .and_then(|value| value.as_str())
                {
                    create = create.with_trace(
                        TraceId::parse(trace)
                            .map_err(|_| ApiFailure::invalid_request(Some("trace_id")))?,
                    );
                }
                let value = SupportService::create(
                    &mut scope,
                    now()?,
                    required_idempotency(&request)?,
                    &create,
                )
                .map_err(support_failure)?;
                response(value)
            }
            "support.reply" => {
                let value = SupportService::reply(
                    &mut scope,
                    now()?,
                    path(&request, "conversation_id")?,
                    required_idempotency(&request)?,
                    text_field(&request.body, "body")?,
                )
                .map_err(support_failure)?;
                response(value)
            }
            "support.read" => {
                let conversation = SupportService::mark_read(
                    &mut scope,
                    now()?,
                    path(&request, "conversation_id")?,
                    text_field(&request.body, "through_message_id")?,
                )
                .map_err(support_failure)?;
                Ok(BackendResponse {
                    result: json!({
                        "conversation_id": conversation.conversation_id(), "state": conversation.state(),
                        "unread_count": conversation.unread_count(), "updated_at": conversation.updated_at()
                    }),
                    session: None,
                })
            }
            "support.status" => {
                let conversation_id = path(&request, "conversation_id")?;
                let (state, unread_count, updated_at) =
                    SupportService::status(&scope, conversation_id).map_err(support_failure)?;
                Ok(BackendResponse {
                    result: json!({"conversation_id": conversation_id, "state": state,
                    "unread_count": unread_count, "updated_at": updated_at}),
                    session: None,
                })
            }
            "support.feedback" => {
                let helpful = request
                    .body
                    .get("helpful")
                    .and_then(|value| value.as_bool())
                    .ok_or_else(|| ApiFailure::invalid_request(Some("helpful")))?;
                let value = SupportService::feedback(
                    &mut scope,
                    now()?,
                    path(&request, "conversation_id")?,
                    text_field(&request.body, "message_id")?,
                    helpful,
                )
                .map_err(support_failure)?;
                response(value)
            }
            "notification.list" => {
                let inventory = DeepLinks::inventory(&scope, now()?).map_err(notify_failure)?;
                let groups = inventory.groups().iter().map(|group| {
                    let notifications = group.notifications().iter().map(notification_json)
                        .collect::<Result<Vec<_>, ApiFailure>>()?;
                    Ok(json!({"recency": group.recency().as_str(), "notifications": notifications}))
                }).collect::<Result<Vec<_>, ApiFailure>>()?;
                Ok(BackendResponse {
                    result: json!({"groups": groups, "next_cursor": "cur_end", "unread_count": inventory.unread_count()}),
                    session: None,
                })
            }
            "notification.read" => {
                let id = NotificationId::new(path(&request, "notification_id")?)
                    .map_err(notify_failure)?;
                let summary =
                    DeepLinks::mark_read(&mut scope, now()?, &id).map_err(notify_failure)?;
                let value = notification_json(&summary)?;
                self.stream.append(
                    &mut scope,
                    &format!("notification-read:{}", summary.notification_id().as_str()),
                    "notification",
                    summary.created_at(),
                    json!({"notification": value.clone()}),
                )?;
                Ok(BackendResponse {
                    result: value,
                    session: None,
                })
            }
            "notification.preferences.get" => {
                let preferences = Dispatcher::preferences(&scope).map_err(notify_failure)?;
                Ok(BackendResponse {
                    result: preferences_json(&preferences),
                    session: None,
                })
            }
            "notification.preferences.set" => {
                let preferences = parse_preferences(&request.body)?;
                Dispatcher::update_preferences(&mut scope, now()?, &preferences)
                    .map_err(notify_failure)?;
                Ok(BackendResponse {
                    result: preferences_json(&preferences),
                    session: None,
                })
            }
            "home.summary" => {
                let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
                let balance = agent
                    .balance()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let mut material = Vec::with_capacity(
                    4 + balance.canonical_bytes.len() + balance.proof_material.len(),
                );
                material.extend_from_slice(&(balance.observed_at.len() as u32).to_be_bytes());
                material.extend_from_slice(balance.observed_at.as_bytes());
                material.extend_from_slice(&balance.age_seconds.to_be_bytes());
                material.extend_from_slice(
                    &u32::try_from(balance.canonical_bytes.len())
                        .map_err(|_| ApiFailure::upstream_degraded())?
                        .to_be_bytes(),
                );
                material.extend_from_slice(&balance.canonical_bytes);
                material.extend_from_slice(&balance.proof_material);
                let evidence_digest: [u8; 32] = Sha256::digest(&material).into();
                scope
                    .put(
                        Table::Cache,
                        RowKey::new(format!("state-proof-{}", hex_bytes(&evidence_digest)))
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        now()?,
                        material,
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                let verification = if balance.verification >= 4 {
                    "checkpoint-finalised"
                } else {
                    "receipt-verified"
                };
                let balance_json = json!({"account_id":format!("act_{}",hex_bytes(&balance.account)),"money":{"amount":balance.amount.to_string(),"currency":balance.currency},"verification":verification,"freshness":{"observed_at":balance.observed_at,"age_seconds":balance.age_seconds,"source_head":balance.observed_head_sequence.to_string(),"within_bound":balance.observed_head_sequence==balance.global_sequence,"checkpoint":hex_bytes(&balance.observed_checkpoint)},"evidence":[{"evidence_id":format!("evd_{}",hex_bytes(&evidence_digest)),"class":if balance.verification>=4{"checkpoint-proof"}else{"layerx-receipt"},"verification":verification}]});
                let agents = agent
                    .agent_list(None, 100)
                    .map_err(agent_failure)?
                    .agents
                    .iter()
                    .map(managed_agent_json)
                    .collect::<Vec<_>>();
                let sequence = agent.head().map_err(agent_failure)?.chain_sequence;
                let page = agent
                    .approval_list(sequence, None, 100)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let mut approvals = Vec::with_capacity(page.approvals.len());
                for hold in &page.approvals {
                    approvals.push(approval_json(
                        hold,
                        agent
                            .verified_budget_after(hold, sequence)
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                    ));
                }
                drop(agent);
                let filters =
                    Feed::apply_filters(FilterDraft::new()).map_err(activity_feed_failure)?;
                let recent_page = self
                    .feed
                    .page(&scope, PageRequest::new(20, filters), now()?, sequence)
                    .map_err(activity_feed_failure)?;
                let recent = recent_page
                    .entries()
                    .iter()
                    .map(|entry| {
                        super::production_reads::activity_entry_json(
                            self.feed,
                            self.settlement_domain,
                            &scope,
                            entry.entry_id(),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(BackendResponse {
                    result: json!({"balance":balance_json,"agents":agents,"approvals":approvals,"recent_activity":recent}),
                    session: None,
                })
            }
            "account.balance" => {
                let balance = self
                    .agent
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?
                    .balance()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let mut material = Vec::with_capacity(
                    4 + balance.canonical_bytes.len() + balance.proof_material.len(),
                );
                material.extend_from_slice(&(balance.observed_at.len() as u32).to_be_bytes());
                material.extend_from_slice(balance.observed_at.as_bytes());
                material.extend_from_slice(&balance.age_seconds.to_be_bytes());
                material.extend_from_slice(
                    &u32::try_from(balance.canonical_bytes.len())
                        .map_err(|_| ApiFailure::upstream_degraded())?
                        .to_be_bytes(),
                );
                material.extend_from_slice(&balance.canonical_bytes);
                material.extend_from_slice(&balance.proof_material);
                let evidence_digest: [u8; 32] = sha2::Sha256::digest(&material).into();
                let evidence_id = format!("evd_{}", hex_bytes(&evidence_digest));
                scope
                    .put(
                        Table::Cache,
                        RowKey::new(format!("state-proof-{}", hex_bytes(&evidence_digest)))
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        now()?,
                        material,
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                let verification = if balance.verification >= 4 {
                    "checkpoint-finalised"
                } else {
                    "receipt-verified"
                };
                Ok(BackendResponse {
                    result: json!({"account_id": format!("act_{}", hex_bytes(&balance.account)),
                        "money": {"amount": balance.amount.to_string(), "currency": balance.currency}, "verification": verification,
                        "freshness": {"observed_at": balance.observed_at, "age_seconds": balance.age_seconds,
                            "source_head": balance.observed_head_sequence.to_string(), "within_bound": balance.observed_head_sequence == balance.global_sequence,
                            "checkpoint": hex_bytes(&balance.observed_checkpoint)},
                        "evidence": [{"evidence_id": evidence_id, "class": if balance.verification >= 4 {"checkpoint-proof"} else {"layerx-receipt"}, "verification": verification}]
                    }),
                    session: None,
                })
            }
            "stream.open" => Ok(BackendResponse {
                result: self.stream.open(&scope)?,
                session: None,
            }),
            "stream.next" => Ok(BackendResponse {
                result: self.stream.next(&scope, path(&request, "cursor")?)?,
                session: None,
            }),
            "activity.export.statement" => {
                let filters = activity_filters(&request.body)?;
                let head = self
                    .agent
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let statement = EvidenceExport::new(self.feed, self.activity_export_maximum_bytes)
                    .map_err(activity_export_failure)?
                    .statement(&scope, &filters, now()?, head.chain_sequence)
                    .map_err(activity_export_failure)?;
                let digest: [u8; 32] = sha2::Sha256::digest(statement.content()).into();
                let digest_text = hex_bytes(&digest);
                scope
                    .put(
                        Table::Cache,
                        RowKey::new(format!("activity-export-{digest_text}"))
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        now()?,
                        statement.content().to_vec(),
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                Ok(BackendResponse {
                    result: json!({"export_id":format!("exp_{digest_text}"),"kind":"statement","download_path":format!("/v1/evidence/evd_{digest_text}"),"content_type":"text/csv; charset=utf-8","created_at":now()?.to_string(),"evidence":[]}),
                    session: None,
                })
            }
            "activity.export.evidence" => {
                let filters = activity_filters(&request.body)?;
                let ids = request
                    .body
                    .get("entry_ids")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(|| ApiFailure::invalid_request(Some("entry_ids")))?
                    .iter()
                    .map(|value| {
                        ActivityEntryId::new(
                            value
                                .as_str()
                                .ok_or_else(|| ApiFailure::invalid_request(Some("entry_ids")))?
                                .to_owned(),
                        )
                        .map_err(|_| ApiFailure::invalid_request(Some("entry_ids")))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut exports = Vec::new();
                let mut entries = Vec::new();
                for id in &ids {
                    let entry = self
                        .feed
                        .entry(&scope, id)
                        .map_err(activity_feed_failure)?
                        .ok_or_else(ApiFailure::not_found)?;
                    for receipt in entry.receipts() {
                        let canonical = receipt
                            .canonical()
                            .ok_or_else(ApiFailure::upstream_degraded)?
                            .to_vec();
                        let authority = receipt
                            .authority()
                            .copied()
                            .ok_or_else(ApiFailure::upstream_degraded)?;
                        let digest: [u8; 32] = sha2::Sha256::digest(&canonical).into();
                        if hex_bytes(&digest) != receipt.reference() {
                            return Err(ApiFailure::upstream_degraded());
                        }
                        let fact = EvidenceBundle::receipt_fact(id, canonical, authority)
                            .map_err(|_| ApiFailure::upstream_degraded())?;
                        exports.push(OfflineExport {
                            receipts: vec![fact],
                            inclusions: Vec::new(),
                            checkpoints: Vec::new(),
                            derived_aggregates: Vec::new(),
                        });
                    }
                    entries.push(entry);
                }
                let receipt_authority = ReceiptAuthority::from_entries(&entries);
                let head = self
                    .agent
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let (bundle, _) =
                    EvidenceExport::new(self.feed, self.activity_export_maximum_bytes)
                        .map_err(activity_export_failure)?
                        .evidence(
                            &scope,
                            &filters,
                            &ids,
                            exports,
                            self.settlement_domain,
                            &receipt_authority,
                            now()?,
                            head.chain_sequence,
                        )
                        .map_err(activity_export_failure)?;
                let bytes = bundle.encode().map_err(activity_export_failure)?;
                let digest: [u8; 32] = sha2::Sha256::digest(&bytes).into();
                let text = hex_bytes(&digest);
                let verification = super::production_reads::verification_label(
                    &verification_status(bundle.verify(
                        digest,
                        scope.principal(),
                        self.settlement_domain,
                        &receipt_authority,
                    )),
                )?;
                scope
                    .put(
                        Table::Cache,
                        RowKey::new(format!("activity-evidence-{text}"))
                            .map_err(|_| ApiFailure::upstream_degraded())?,
                        now()?,
                        bytes,
                    )
                    .map_err(|_| ApiFailure::unavailable())?;
                Ok(BackendResponse {
                    result: json!({"export_id":format!("exp_{text}"),"kind":"evidence-bundle","download_path":format!("/v1/evidence/evd_{text}"),"content_type":"application/vnd.layerx.evidence-bundle","created_at":now()?.to_string(),"evidence":bundle.entries().iter().flat_map(|entry|entry.receipt_references().iter()).map(|reference|json!({"evidence_id":format!("evd_{reference}"),"class":"layerx-receipt","verification":verification})).collect::<Vec<_>>() }),
                    session: None,
                })
            }
            "activity.entry" => super::production_reads::activity_entry(
                self.feed,
                self.settlement_domain,
                &scope,
                &request,
            ),
            "activity.query" => {
                let filters = activity_filters(&request.body)?;
                let limit = request
                    .body
                    .get("page_limit")
                    .and_then(serde_json::Value::as_u64)
                    .map_or(Ok(50usize), |value| {
                        usize::try_from(value)
                            .map_err(|_| ApiFailure::invalid_request(Some("page_limit")))
                    })?;
                let mut page_request = PageRequest::new(limit, filters);
                if let Some(cursor) = request
                    .body
                    .get("cursor")
                    .and_then(serde_json::Value::as_str)
                {
                    page_request = page_request
                        .after(FeedCursor::parse(cursor).map_err(activity_feed_failure)?)
                }
                let head = self
                    .agent
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?
                    .head()
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let page = self
                    .feed
                    .page(&scope, page_request, now()?, head.chain_sequence)
                    .map_err(activity_feed_failure)?;
                let mut groups = std::collections::BTreeMap::<
                    u64,
                    (u128, u128, String, Vec<serde_json::Value>),
                >::new();
                for entry in page.entries() {
                    let mut amount = 0u128;
                    let mut currency = None;
                    for receipt in entry.receipts() {
                        let bytes = receipt
                            .canonical()
                            .ok_or_else(ApiFailure::upstream_degraded)?;
                        let actual =
                            crate::activity::detail::ReceiptActual::from_verified_journey_bytes(
                                bytes,
                                receipt.reference(),
                            )
                            .map_err(|_| ApiFailure::upstream_degraded())?;
                        amount = amount
                            .checked_add(actual.amount())
                            .ok_or_else(ApiFailure::upstream_degraded)?;
                        let asset = hex_bytes(&actual.asset());
                        if currency.as_ref().is_some_and(|value| value != &asset) {
                            return Err(ApiFailure::upstream_degraded());
                        }
                        currency = Some(asset)
                    }
                    let bucket = entry.occurred_at() / 2_629_800;
                    let group = groups.entry(bucket).or_insert((
                        0,
                        0,
                        currency.clone().unwrap_or_default(),
                        Vec::new(),
                    ));
                    if let Some(currency) = currency {
                        if !group.2.is_empty() && group.2 != currency {
                            return Err(ApiFailure::upstream_degraded());
                        }
                        group.2 = currency
                    }
                    match entry.kind() {
                        crate::activity::ActivityKind::Deposit => {
                            group.0 = group
                                .0
                                .checked_add(amount)
                                .ok_or_else(ApiFailure::upstream_degraded)?
                        }
                        crate::activity::ActivityKind::Withdrawal => {
                            group.1 = group
                                .1
                                .checked_add(amount)
                                .ok_or_else(ApiFailure::upstream_degraded)?
                        }
                        _ => {
                            if amount != 0 {
                                return Err(ApiFailure::upstream_degraded());
                            }
                        }
                    }
                    group.3.push(json!({"entry_id":entry.entry_id().as_str(),"kind":activity_kind_label(entry.kind()),"state":activity_status_label(entry.status()),"state_copy_key":format!("activity.state.{}",activity_status_label(entry.status())),"summary_copy_key":format!("activity.summary.{}",activity_kind_label(entry.kind())),"occurred_at":entry.occurred_at()}));
                }
                let groups=groups.into_iter().rev().map(|(month,(incoming,outgoing,currency,entries))|json!({"month":format!("unix-month-{month}"),"subtotal_in":{"amount":incoming.to_string(),"currency":currency},"subtotal_out":{"amount":outgoing.to_string(),"currency":currency},"entries":entries})).collect::<Vec<_>>();
                Ok(BackendResponse {
                    result: json!({"groups":groups,"next_cursor":page.next().map(|value|value.as_str()).unwrap_or(""),"filter":{"kinds":page.applied_filters().kinds().iter().map(|value|activity_kind_label(*value)).collect::<Vec<_>>(),"agent_id":page.applied_filters().agent(),"from":page.applied_filters().from(),"to":page.applied_filters().through()}}),
                    session: None,
                })
            }
            _ => super::production_reads::execute(
                self.feed,
                self.settlement_domain,
                &scope,
                &request,
            )
            .unwrap_or_else(|| Err(ApiFailure::not_found())),
        }
    }

    fn readiness(&self, _trace: &str) -> Result<Readiness, ApiFailure> {
        let custody = self.custody.status();
        let custody_ready = matches!(custody.kms, crate::custody::Availability::Available)
            && matches!(custody.storage, crate::custody::Availability::Available)
            && matches!(
                custody.key_references,
                crate::custody::KeyReferenceIntegrity::Verified
            )
            && matches!(custody.rotation, crate::custody::RotationState::Stable);
        let agent_ready = self
            .agent
            .lock()
            .ok()
            .is_some_and(|agent| agent.probe().is_ok());
        let core_ready = self
            .agent_contract
            .daemon_endpoint()
            .is_some_and(|endpoint| {
                AgentRuntime::connect(endpoint, self.agent_limits)
                    .and_then(|mut runtime| runtime.head())
                    .is_ok()
            });
        let security_ready = self
            .security
            .lock()
            .ok()
            .is_some_and(|security| security.probe().is_ok());
        let identity_ready = self.identity.probe().is_ok();
        let movement_ready = self
            .movement
            .lock()
            .ok()
            .is_some_and(|movement| movement.ready());
        let store_ready = self
            .store
            .lock()
            .ok()
            .is_some_and(|store| store.probe().is_ok());
        let paxeer_ready = raw_call(&self.paxeer_endpoint, "eth_chainId", &[]).is_ok();
        Ok(Readiness {
            human_service: if store_ready
                && security_ready
                && identity_ready
                && movement_ready
                && self.maintenance_healthy.load(Ordering::Acquire)
            {
                ComponentState::Ready
            } else {
                ComponentState::Unavailable
            },
            custody: if custody_ready {
                ComponentState::Ready
            } else {
                ComponentState::Unavailable
            },
            agent: if agent_ready {
                ComponentState::Ready
            } else {
                ComponentState::Unavailable
            },
            core: if core_ready {
                ComponentState::Ready
            } else {
                ComponentState::Unavailable
            },
            paxeer: if paxeer_ready && movement_ready {
                ComponentState::Ready
            } else {
                ComponentState::Unavailable
            },
        })
    }
}

impl ComponentMaintenance for ProductionComponents {
    fn maintain(&self, maximum_items: usize, observed_at: u64) -> Result<usize, ApiFailure> {
        if maximum_items == 0 {
            return Err(ApiFailure::invalid_request(None));
        }
        let mut principals = self
            .auth_index
            .active_principals(observed_at)
            .map_err(auth_failure)?;
        let mut store = self.store.lock().map_err(|_| ApiFailure::unavailable())?;
        for principal in store.tenancy().principals() {
            if !principals.contains(&principal) {
                principals.push(principal);
            }
        }
        principals.sort();
        let mut advanced = 0usize;
        for principal in principals {
            if advanced == maximum_items {
                break;
            }
            let mut scope = store
                .principal(&principal)
                .map_err(|_| ApiFailure::unavailable())?;
            let keys = scope.keys(Table::Journeys);
            for key in keys
                .into_iter()
                .filter(|key| key.as_str().starts_with("continuation-"))
            {
                if advanced == maximum_items {
                    break;
                }
                let row = scope
                    .get(Table::Journeys, &key)
                    .ok_or_else(ApiFailure::upstream_degraded)?;
                let mut continuation: Continuation = serde_json::from_slice(row.bytes())
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                if continuation.next_attempt_at > observed_at || continuation.terminal {
                    continue;
                }
                let journey_id = crate::notify::JourneyId::new(continuation.journey_id.clone())
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                let trace = maintenance_trace(&principal, &journey_id, continuation.attempts);
                let outcome = self.advance_continuation(
                    &mut scope,
                    &continuation.kind,
                    &journey_id,
                    &trace,
                    observed_at,
                );
                advanced = advanced.saturating_add(1);
                match outcome {
                    Ok(terminal) => {
                        continuation.updated_at = observed_at;
                        continuation.last_error = None;
                        continuation.unknown_since = None;
                        continuation.unknown_deadline_at = None;
                        continuation.terminal = terminal;
                        continuation.next_attempt_at = if terminal {
                            u64::MAX
                        } else {
                            observed_at.saturating_add(1)
                        };
                    }
                    Err(_) => {
                        continuation.attempts = continuation.attempts.saturating_add(1);
                        continuation.updated_at = observed_at;
                        continuation.last_error = Some("boundary-outcome-unknown".to_owned());
                        let unknown_since = continuation.unknown_since.get_or_insert(observed_at);
                        let deadline = unknown_since
                            .saturating_add(self.continuation_unknown_deadline_seconds);
                        continuation.unknown_deadline_at = Some(deadline);
                        let exponent = continuation.attempts.min(8);
                        let retry = 1_u64.checked_shl(exponent).unwrap_or(256).min(300);
                        continuation.next_attempt_at = if observed_at >= deadline {
                            observed_at.saturating_add(300)
                        } else {
                            observed_at.saturating_add(retry).min(deadline)
                        };
                    }
                }
                let bytes = serde_json::to_vec(&continuation)
                    .map_err(|_| ApiFailure::upstream_degraded())?;
                scope
                    .put(Table::Journeys, key, observed_at, bytes)
                    .map_err(|_| ApiFailure::unavailable())?;
            }
        }
        Ok(advanced)
    }

    fn set_maintenance_health(&self, healthy: bool) {
        self.maintenance_healthy.store(healthy, Ordering::Release);
    }
}

impl ProductionComponents {
    fn advance_continuation(
        &self,
        scope: &mut crate::store::PrincipalScope<'_>,
        kind: &str,
        id: &crate::notify::JourneyId,
        trace: &TraceId,
        observed_at: u64,
    ) -> Result<bool, ApiFailure> {
        let mut agent = self.agent.lock().map_err(|_| ApiFailure::unavailable())?;
        let registry = agent.registry().clone();
        match kind {
            "move" => {
                let mut journey = crate::journeys::MoveJourney::load(scope, id)
                    .map_err(move_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let status = crate::server::poll_once_ready(journey.advance(
                    scope,
                    &self.agent_contract,
                    &mut *agent,
                    &self.custody,
                    &registry,
                    trace,
                    observed_at,
                ))
                .map_err(|_| ApiFailure::upstream_degraded())?
                .map_err(move_journey_failure)?;
                Ok(matches!(
                    status.stage(),
                    crate::journeys::MoveStage::Done | crate::journeys::MoveStage::Refused
                ))
            }
            "deposit" => {
                let mut journey = crate::journeys::DepositJourney::load(scope, id)
                    .map_err(deposit_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = movement
                    .advance_deposit(
                        scope,
                        &mut journey,
                        &self.agent_contract,
                        &mut *agent,
                        &self.custody,
                        &registry,
                        trace,
                        observed_at,
                    )
                    .map_err(deposit_journey_failure)?;
                Ok(matches!(
                    status.stage(),
                    crate::journeys::DepositStage::Done | crate::journeys::DepositStage::Failed(_)
                ))
            }
            "withdraw" => {
                let mut journey = crate::journeys::WithdrawalJourney::load(scope, id)
                    .map_err(withdrawal_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = movement
                    .advance_withdrawal(
                        scope,
                        &mut journey,
                        &self.agent_contract,
                        &mut *agent,
                        &self.custody,
                        &registry,
                        trace,
                        None,
                        observed_at,
                    )
                    .map_err(withdrawal_journey_failure)?;
                Ok(matches!(
                    status.stage(),
                    crate::journeys::WithdrawalStage::PaidOut(_)
                        | crate::journeys::WithdrawalStage::Cancelled(_)
                ))
            }
            "exit" => {
                drop(agent);
                let mut journey = crate::journeys::ExitJourney::load(scope, id)
                    .map_err(exit_journey_failure)?
                    .ok_or_else(ApiFailure::not_found)?;
                let mut movement = self
                    .movement
                    .lock()
                    .map_err(|_| ApiFailure::unavailable())?;
                let status = movement
                    .advance_exit(
                        scope,
                        trace,
                        &self.emergency_exit,
                        &mut journey,
                        observed_at,
                    )
                    .map_err(exit_journey_failure)?;
                Ok(matches!(
                    status.stage(),
                    crate::journeys::ExitStage::Done(_)
                        | crate::journeys::ExitStage::Failed(_)
                        | crate::journeys::ExitStage::UnavailableWhileNetworkOperatingNormally { .. }
                ))
            }
            _ => Err(ApiFailure::upstream_degraded()),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Continuation {
    kind: String,
    journey_id: String,
    started_at: u64,
    updated_at: u64,
    next_attempt_at: u64,
    attempts: u32,
    #[serde(default)]
    unknown_since: Option<u64>,
    #[serde(default)]
    unknown_deadline_at: Option<u64>,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    terminal: bool,
}

fn maintenance_trace(
    principal: &crate::store::PrincipalId,
    id: &crate::notify::JourneyId,
    attempt: u32,
) -> TraceId {
    let digest: [u8; 32] = Sha256::digest(
        [
            b"layerx-human/continuation-trace/v1\0".as_slice(),
            principal.as_str().as_bytes(),
            id.as_str().as_bytes(),
            &attempt.to_be_bytes(),
        ]
        .concat(),
    )
    .into();
    TraceId::mint(
        digest[..16]
            .try_into()
            .expect("digest prefix has fixed length"),
    )
}

fn response(value: impl serde::Serialize) -> Result<BackendResponse, ApiFailure> {
    Ok(BackendResponse {
        result: serde_json::to_value(value).map_err(|_| ApiFailure::upstream_degraded())?,
        session: None,
    })
}

fn activity_filters(body: &serde_json::Value) -> Result<AppliedFilters, ApiFailure> {
    let Some(filter) = body.get("filter") else {
        return Feed::apply_filters(FilterDraft::new()).map_err(activity_feed_failure);
    };
    let object = filter
        .as_object()
        .ok_or_else(|| ApiFailure::invalid_request(Some("filter")))?;
    let mut draft = FilterDraft::new();
    if let Some(kinds) = object.get("kinds") {
        let kinds = kinds
            .as_array()
            .ok_or_else(|| ApiFailure::invalid_request(Some("kinds")))?
            .iter()
            .map(|value| match value.as_str() {
                Some("deposit") => Ok(crate::activity::ActivityKind::Deposit),
                Some("withdrawal") => Ok(crate::activity::ActivityKind::Withdrawal),
                Some("movement") => Ok(crate::activity::ActivityKind::Movement),
                Some("agent-action") => Ok(crate::activity::ActivityKind::AgentAction),
                Some("approval") => Ok(crate::activity::ActivityKind::Approval),
                Some("security-event") => Ok(crate::activity::ActivityKind::Security),
                _ => Err(ApiFailure::invalid_request(Some("kinds"))),
            })
            .collect::<Result<Vec<_>, _>>()?;
        draft = draft.with_kinds(kinds);
    }
    if let Some(agent) = object.get("agent_id").and_then(serde_json::Value::as_str) {
        draft = draft.with_agent(agent);
    }
    let from = object.get("from").and_then(serde_json::Value::as_u64);
    let through = object.get("to").and_then(serde_json::Value::as_u64);
    draft = draft.with_dates(from, through);
    Feed::apply_filters(draft).map_err(activity_feed_failure)
}

fn activity_feed_failure(error: crate::activity::FeedError) -> ApiFailure {
    match error {
        crate::activity::FeedError::Store(_) => ApiFailure::unavailable(),
        _ => ApiFailure::invalid_request(Some("filter")),
    }
}

fn activity_export_failure(error: crate::activity::ExportError) -> ApiFailure {
    match error {
        crate::activity::ExportError::Feed(value) => activity_feed_failure(value),
        crate::activity::ExportError::Audit(_) => ApiFailure::upstream_degraded(),
        _ => ApiFailure::upstream_degraded(),
    }
}
fn activity_kind_label(value: crate::activity::ActivityKind) -> &'static str {
    match value {
        crate::activity::ActivityKind::Deposit => "deposit",
        crate::activity::ActivityKind::Withdrawal => "withdrawal",
        crate::activity::ActivityKind::Movement => "movement",
        crate::activity::ActivityKind::AgentAction => "agent-action",
        crate::activity::ActivityKind::Approval => "approval",
        crate::activity::ActivityKind::Security => "security-event",
    }
}
fn activity_status_label(value: crate::activity::ActivityStatus) -> &'static str {
    use crate::activity::{ActivityStatus as S, DepositStage as D, WithdrawalStage as W};
    match value {
        S::GettingReady => "getting-ready",
        S::Sending => "sending",
        S::Processing
        | S::Deposit(D::ConfirmingOnPaxeer)
        | S::Deposit(D::Crediting)
        | S::Withdrawal(W::Processing)
        | S::Withdrawal(W::WaitingForSettlement) => "processing",
        S::StillChecking => "still-checking",
        S::WaitingForYou | S::Deposit(D::WaitingForWallet) | S::Withdrawal(W::ReadyToClaim) => {
            "waiting-for-you"
        }
        S::Done | S::Deposit(D::Done) | S::Withdrawal(W::PaidOut) => "done",
        S::DoneFinalised => "done-finalised",
        S::DidntGoThrough { .. } => "refused",
    }
}

fn path<'a>(request: &'a ScopedRequest<'_>, name: &str) -> Result<&'a str, ApiFailure> {
    request
        .path_parameters
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))
}

fn text_field<'a>(body: &'a serde_json::Value, name: &str) -> Result<&'a str, ApiFailure> {
    body.get(name)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))
}

fn money_field<'a>(body: &'a serde_json::Value, name: &str) -> Result<(u128, &'a str), ApiFailure> {
    let money = body
        .get(name)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?;
    let amount = money
        .get("amount")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?
        .parse::<u128>()
        .map_err(|_| ApiFailure::invalid_request(Some(name)))?;
    let currency = money
        .get("currency")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?;
    if amount == 0 {
        return Err(ApiFailure::invalid_request(Some(name)));
    }
    Ok((amount, currency))
}

fn movement_request(
    request: &ScopedRequest<'_>,
    principal: &crate::store::PrincipalId,
    tenant: crate::store::AgentTenantId,
    observed_at: u64,
) -> Result<super::movement_provider::PlanningRequest, ApiFailure> {
    let canonical_body =
        serde_json::to_vec(&request.body).map_err(|_| ApiFailure::invalid_request(None))?;
    let trace = TraceId::parse(&request.trace).map_err(|_| ApiFailure::invalid_request(None))?;
    Ok(super::movement_provider::PlanningRequest {
        principal: principal.clone(),
        tenant,
        operation: request.operation.name.clone(),
        idempotency_key: request
            .idempotency_key
            .as_deref()
            .map(action_key)
            .unwrap_or_else(|| {
                action_key(&format!(
                    "{}:{}",
                    request.operation.name,
                    hex_bytes(&sha2::Sha256::digest(&canonical_body))
                ))
            }),
        canonical_body,
        trace,
        now: observed_at,
    })
}

fn action_key(value: &str) -> [u8; 32] {
    sha2::Sha256::digest(
        [
            b"layerx-human/movement-action/v1\0".as_slice(),
            value.as_bytes(),
        ]
        .concat(),
    )
    .into()
}

fn move_quote_json(value: &super::movement_provider::AuthorizedMovePlan) -> serde_json::Value {
    let quote = value.plan.quote();
    let mechanism = value
        .plan
        .route()
        .legs()
        .first()
        .map(|leg| leg.term().as_str())
        .unwrap_or("transfer");
    json!({"quote_id": value.quote_id, "description_copy_key": "movement.review.resolved-route",
        "mechanism": mechanism, "money": {"amount": quote.amount().to_string(), "currency": quote.asset_label()},
        "fee_estimate": {"amount": quote.fee_estimate().to_string(), "currency": quote.asset_label()},
        "fee_ceiling": {"amount": quote.fee_ceiling().to_string(), "currency": quote.asset_label()},
        "arrival_estimate": quote.arrival_expectation(), "expires_at": value.expires_at,
        "irreversibility_copy_key": "movement.review.irreversible"})
}

fn movement_failure(_: super::movement_provider::MovementProviderError) -> ApiFailure {
    ApiFailure::upstream_degraded()
}
fn move_journey_failure(_: crate::journeys::MoveJourneyError) -> ApiFailure {
    ApiFailure::upstream_degraded()
}
fn deposit_journey_failure(_: crate::journeys::DepositJourneyError) -> ApiFailure {
    ApiFailure::upstream_degraded()
}
fn withdrawal_journey_failure(_: crate::journeys::WithdrawalJourneyError) -> ApiFailure {
    ApiFailure::upstream_degraded()
}
fn exit_journey_failure(_: crate::journeys::ExitJourneyError) -> ApiFailure {
    ApiFailure::upstream_degraded()
}

fn decode_hex_32(value: &str) -> Result<[u8; 32], ()> {
    layerx_paxeer_client::TransactionHash::from_hex(value)
        .map(layerx_paxeer_client::TransactionHash::bytes)
        .map_err(|_| ())
}

fn schedule_continuation(
    scope: &mut crate::store::PrincipalScope<'_>,
    kind: &str,
    id: &crate::notify::JourneyId,
    observed_at: u64,
) -> Result<(), ApiFailure> {
    let key = RowKey::new(format!("continuation-{kind}-{}", id.as_str()))
        .map_err(|_| ApiFailure::upstream_degraded())?;
    let continuation = match scope.get(Table::Journeys, &key) {
        Some(row) => {
            let existing: Continuation =
                serde_json::from_slice(row.bytes()).map_err(|_| ApiFailure::upstream_degraded())?;
            if existing.kind != kind || existing.journey_id != id.as_str() {
                return Err(ApiFailure::upstream_degraded());
            }
            existing
        }
        None => Continuation {
            kind: kind.to_owned(),
            journey_id: id.as_str().to_owned(),
            started_at: observed_at,
            updated_at: observed_at,
            next_attempt_at: observed_at,
            attempts: 0,
            unknown_since: None,
            unknown_deadline_at: None,
            last_error: None,
            terminal: false,
        },
    };
    let bytes = serde_json::to_vec(&continuation).map_err(|_| ApiFailure::upstream_degraded())?;
    scope
        .put(Table::Journeys, key, observed_at, bytes)
        .map_err(|_| ApiFailure::unavailable())
}
fn continuation_times(
    scope: &crate::store::PrincipalScope<'_>,
    kind: &str,
    id: &crate::notify::JourneyId,
    now: u64,
) -> Result<(u64, u64), ApiFailure> {
    let key = RowKey::new(format!("continuation-{kind}-{}", id.as_str()))
        .map_err(|_| ApiFailure::upstream_degraded())?;
    let value: serde_json::Value = serde_json::from_slice(
        scope
            .get(Table::Journeys, &key)
            .ok_or_else(ApiFailure::upstream_degraded)?
            .bytes(),
    )
    .map_err(|_| ApiFailure::upstream_degraded())?;
    Ok((
        value
            .get("started_at")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(ApiFailure::upstream_degraded)?,
        value
            .get("updated_at")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(now),
    ))
}
fn public_journey(
    scope: &crate::store::PrincipalScope<'_>,
    kind: &str,
    id: &crate::notify::JourneyId,
    state: &str,
    copy: &str,
    evidence: Vec<serde_json::Value>,
    now: u64,
) -> Result<serde_json::Value, ApiFailure> {
    let (started_at, updated_at) = continuation_times(scope, kind, id, now)?;
    Ok(
        json!({"journey_id":id.as_str(),"kind":kind,"state":state,"state_copy_key":format!("status.{state}"),
        "stages":[{"stage_id":format!("stg_{kind}"),"copy_key":copy,"state":state,"evidence":evidence}],
        "evidence":evidence,"started_at":started_at,"updated_at":updated_at}),
    )
}
fn move_public_json(
    scope: &crate::store::PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    status: &crate::journeys::MoveStatus,
    now: u64,
) -> Result<serde_json::Value, ApiFailure> {
    let state = match status.stage() {
        crate::journeys::MoveStage::Committed => "getting-ready",
        crate::journeys::MoveStage::Moving => "processing",
        crate::journeys::MoveStage::StillChecking => "still-checking",
        crate::journeys::MoveStage::Done => "done",
        crate::journeys::MoveStage::Refused => "refused",
    };
    let evidence = status
        .receipt_references()
        .iter()
        .map(|receipt| {
            let verification = super::production_reads::custody_receipt_label(
                scope,
                settlement_domain,
                receipt.digest(),
            )?;
            Ok(
                json!({"evidence_id":receipt.reference(),"class":"layerx-receipt","verification":verification}),
            )
        })
        .collect::<Result<Vec<_>, ApiFailure>>()?;
    public_journey(
        scope,
        "move",
        status.journey_id(),
        state,
        "movement.stage.progress",
        evidence,
        now,
    )
}
fn deposit_public_json(
    scope: &crate::store::PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    status: &crate::journeys::DepositStatus,
    now: u64,
) -> Result<serde_json::Value, ApiFailure> {
    let (state, copy) = match status.stage() {
        crate::journeys::DepositStage::WaitingForWallet => {
            ("waiting-for-you", "deposit.stage.wallet")
        }
        crate::journeys::DepositStage::ConfirmingPaxeer { .. } => {
            ("processing", "deposit.stage.confirming")
        }
        crate::journeys::DepositStage::CreditingLayerX => ("processing", "deposit.stage.crediting"),
        crate::journeys::DepositStage::Done => ("done", "deposit.stage.done"),
        crate::journeys::DepositStage::Failed(_) => ("refused", "deposit.stage.refused"),
    };
    let evidence = match status.activity() {
        Some(activity) => {
            let verification = super::production_reads::custody_receipt_label(
                scope,
                settlement_domain,
                activity.credit_receipt_digest,
            )?;
            vec![
                json!({"evidence_id":format!("evd_{}",hex_bytes(&activity.credit_receipt_digest)),"class":"layerx-receipt","verification":verification}),
            ]
        }
        None => Vec::new(),
    };
    public_journey(
        scope,
        "deposit",
        status.journey_id(),
        state,
        copy,
        evidence,
        now,
    )
}
fn withdrawal_public_json(
    scope: &crate::store::PrincipalScope<'_>,
    settlement_domain: SettlementDomain,
    status: &crate::journeys::WithdrawalStatus,
    now: u64,
) -> Result<serde_json::Value, ApiFailure> {
    let state = match status.stage() {
        crate::journeys::WithdrawalStage::ReadyToClaim => "waiting-for-you",
        crate::journeys::WithdrawalStage::PaidOut(_) => "done",
        crate::journeys::WithdrawalStage::Cancelled(_) => "refused",
        _ => "processing",
    };
    let evidence = match status.debit_receipt_reference() {
        Some(digest) => {
            let verification =
                super::production_reads::custody_receipt_label(scope, settlement_domain, digest)?;
            vec![
                json!({"evidence_id":format!("evd_{}",hex_bytes(&digest)),"class":"layerx-receipt","verification":verification}),
            ]
        }
        None => Vec::new(),
    };
    public_journey(
        scope,
        "withdraw",
        status.journey_id(),
        state,
        "withdraw.stage.progress",
        evidence,
        now,
    )
}
fn exit_public_json(
    scope: &crate::store::PrincipalScope<'_>,
    status: &crate::journeys::ExitStatus,
    now: u64,
) -> Result<serde_json::Value, ApiFailure> {
    let (state, evidence) = match status.stage() {
        crate::journeys::ExitStage::Done(value) => (
            "done",
            vec![
                json!({"evidence_id":format!("evd_{}",hex_bytes(&value.transaction)),"class":"paxeer-finality","verification":"paxeer-finalised","settlement_domain":"paxeer"}),
            ],
        ),
        crate::journeys::ExitStage::Failed(_)
        | crate::journeys::ExitStage::UnavailableWhileNetworkOperatingNormally { .. } => {
            ("refused", Vec::new())
        }
        crate::journeys::ExitStage::WaitingForWallet => ("waiting-for-you", Vec::new()),
        _ => ("processing", Vec::new()),
    };
    public_journey(
        scope,
        "exit",
        status.journey_id(),
        state,
        "exit.stage.progress",
        evidence,
        now,
    )
}

fn verification_label(value: u8) -> &'static str {
    match value {
        0 => "unverified",
        1 => "sequencer-signed",
        2 => "batch-included",
        3 => "state-proven",
        4 => "checkpoint-finalised",
        5 => "settlement-anchored",
        _ => unreachable!(),
    }
}
fn managed_evidence_json(value: &super::agent_runtime::ManagedAgentEvidence) -> serde_json::Value {
    json!({"evidence_id": value.evidence_id, "class": value.class, "verification": verification_label(value.verification)})
}
fn managed_agent_json(value: &super::agent_runtime::ManagedAgentView) -> serde_json::Value {
    let state = ["creating", "active", "paused", "archiving", "archived"][usize::from(value.state)];
    let enforcement = ["protocol", "app"][usize::from(value.limit_enforcement)];
    json!({"agent_id": value.agent_id, "name": value.name, "purpose": value.purpose, "state": state, "state_copy_key": format!("agent.state.{state}"), "limit": {"monthly": {"amount": value.monthly_limit.to_string(), "currency": value.currency}, "enforcement": enforcement, "enforcement_copy_key": if value.limit_enforcement == 0 { "agent.limit.protocol-backed" } else { "agent.limit.app-enforced" }}, "spend": {"period_start": value.period_start, "period_end": value.period_end, "spent": {"amount": value.spent.to_string(), "currency": value.currency}, "remaining": {"amount": value.remaining.to_string(), "currency": value.currency}, "verification": verification_label(value.spend_verification)}, "evidence": value.evidence.iter().map(managed_evidence_json).collect::<Vec<_>>(), "created_at": value.created_at, "updated_at": value.updated_at})
}
fn managed_journey_json(value: &super::agent_runtime::ManagedAgentJourney) -> serde_json::Value {
    let state = ["getting-ready", "processing", "done", "refused"][usize::from(value.state)];
    json!({"journey_id": value.journey_id, "kind": value.kind, "state": state, "state_copy_key": format!("status.{state}"), "stages": value.stages.iter().map(|stage| { let stage_state = ["pending", "processing", "done", "refused"][usize::from(stage.state)]; json!({"stage_id": stage.stage_id, "copy_key": stage.copy_key, "state": stage_state, "evidence": stage.evidence.iter().map(managed_evidence_json).collect::<Vec<_>>()}) }).collect::<Vec<_>>(), "evidence": value.evidence.iter().map(managed_evidence_json).collect::<Vec<_>>(), "started_at": value.started_at, "updated_at": value.updated_at})
}
fn managed_challenge_json(
    value: &super::agent_runtime::ManagedAgentChallenge,
) -> serde_json::Value {
    let kind = ["rotate", "recover"][usize::from(value.kind)];
    json!({"agent_id": value.agent_id, "kind": kind, "delay_copy_key": format!("agent.keys.{kind}-delay"), "delay_seconds": value.delay_seconds, "ready_at": value.ready_at, "evidence": value.evidence.iter().map(managed_evidence_json).collect::<Vec<_>>()})
}
fn agent_failure(error: crate::journeys::AgentBoundaryError) -> ApiFailure {
    match error {
        crate::journeys::AgentBoundaryError::Refused => ApiFailure::not_found(),
        crate::journeys::AgentBoundaryError::Unavailable
        | crate::journeys::AgentBoundaryError::CorruptResponse => ApiFailure::upstream_degraded(),
    }
}

fn decode_field<T: serde::de::DeserializeOwned>(
    body: &serde_json::Value,
    name: &str,
) -> Result<T, ApiFailure> {
    serde_json::from_value(
        body.get(name)
            .cloned()
            .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?,
    )
    .map_err(|_| ApiFailure::invalid_request(Some(name)))
}

fn optional_decode_field<T: serde::de::DeserializeOwned>(
    body: &serde_json::Value,
    name: &str,
) -> Result<Option<T>, ApiFailure> {
    body.get(name)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| ApiFailure::invalid_request(Some(name)))
}

fn required_idempotency<'a>(request: &'a ScopedRequest<'_>) -> Result<&'a str, ApiFailure> {
    request
        .idempotency_key
        .as_deref()
        .ok_or_else(|| ApiFailure::invalid_request(Some("Idempotency-Key")))
}

fn support_failure(error: crate::support::SupportError) -> ApiFailure {
    use crate::support::SupportError::*;
    match error {
        ConversationUnknown | MessageUnknown => ApiFailure::not_found(),
        Store(_) => ApiFailure::unavailable(),
        InvalidBody => ApiFailure::invalid_request(Some("body")),
        InvalidIdempotencyKey | Conflict | ConversationFull | ConversationResolved | Corrupt => {
            ApiFailure::invalid_request(None)
        }
    }
}

fn auth_api_failure(error: crate::auth::AuthError) -> ApiFailure {
    match error {
        crate::auth::AuthError::Unauthenticated => ApiFailure::unauthenticated(),
        crate::auth::AuthError::SessionExpired => ApiFailure::session_expired(),
        crate::auth::AuthError::SessionNotFound => ApiFailure::not_found(),
        crate::auth::AuthError::Store(_) => ApiFailure::unavailable(),
        _ => ApiFailure::forbidden(),
    }
}

fn agent_creation_failure(error: crate::agents::AgentCreationError) -> ApiFailure {
    use crate::agents::AgentCreationError;
    match error {
        AgentCreationError::InvalidRequest
        | AgentCreationError::InvalidContext
        | AgentCreationError::UnknownPurpose => ApiFailure::invalid_request(None),
        AgentCreationError::IdempotencyConflict | AgentCreationError::EvidenceConflict => {
            ApiFailure::forbidden()
        }
        AgentCreationError::Agent(crate::agents::AgentFailure::Unavailable) => {
            ApiFailure::upstream_degraded()
        }
        AgentCreationError::Agent(crate::agents::AgentFailure::Refused(_)) => {
            ApiFailure::forbidden()
        }
        _ => ApiFailure::upstream_degraded(),
    }
}

fn agent_failure_from_creation_contract(error: crate::agents::AgentFailure) -> ApiFailure {
    match error {
        crate::agents::AgentFailure::Unavailable => ApiFailure::upstream_degraded(),
        crate::agents::AgentFailure::Refused(_) => ApiFailure::forbidden(),
    }
}

fn agent_creation_json(
    journey: &CreationJourney,
    status: &crate::agents::CreationStatus,
) -> serde_json::Value {
    use crate::agents::{CreationState, StageState};
    let projection = journey.projection();
    json!({
        "journey_id": format!("jrn_{}", URL_SAFE_NO_PAD.encode(status.agent_id)),
        "agent_id": URL_SAFE_NO_PAD.encode(status.agent_id),
        "kind": "agent-create",
        "name": projection.name,
        "purpose": projection.purpose,
        "monthly_limit": projection.monthly_limit.to_string(),
        "state": match status.state { CreationState::GettingReady => "getting-ready", CreationState::Partial => "partial", CreationState::Active => "active" },
        "stages": status.stages.iter().map(|(stage, state)| json!({
            "stage": format!("{stage:?}").to_ascii_lowercase(),
            "state": match state { StageState::Pending => "pending", StageState::LocalComplete => "local-complete",
                StageState::Unavailable => "unavailable", StageState::Refused => "refused", StageState::ReceiptVerified => "receipt-verified" }
        })).collect::<Vec<_>>(),
        "started_at": projection.started_at,
    })
}

fn identity_failure(error: identity_dispatch::IdentityDispatchError) -> ApiFailure {
    match error {
        identity_dispatch::IdentityDispatchError::InvalidInput => ApiFailure::invalid_request(None),
        identity_dispatch::IdentityDispatchError::NotFound => ApiFailure::not_found(),
        identity_dispatch::IdentityDispatchError::ProviderRefused
        | identity_dispatch::IdentityDispatchError::ProviderAuthentication => {
            ApiFailure::forbidden()
        }
        identity_dispatch::IdentityDispatchError::InvalidConfiguration
        | identity_dispatch::IdentityDispatchError::Corrupt
        | identity_dispatch::IdentityDispatchError::ProviderUnavailable
        | identity_dispatch::IdentityDispatchError::ProviderEvidence
        | identity_dispatch::IdentityDispatchError::Store(_) => ApiFailure::upstream_degraded(),
    }
}

fn notify_failure(error: crate::notify::NotifyError) -> ApiFailure {
    match error {
        crate::notify::NotifyError::NotificationNotFound => ApiFailure::not_found(),
        crate::notify::NotifyError::Store(_) | crate::notify::NotifyError::Audit(_) => {
            ApiFailure::unavailable()
        }
        _ => ApiFailure::upstream_degraded(),
    }
}

fn notification_json(summary: &NotificationSummary) -> Result<serde_json::Value, ApiFailure> {
    let mut payload: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(summary.delivery().payload())
            .map_err(|_| ApiFailure::upstream_degraded())?;
    payload.insert(
        "notification_id".to_owned(),
        json!(summary.notification_id().as_str()),
    );
    payload.insert("class".to_owned(), json!(summary.class().as_str()));
    payload.insert("deep_link".to_owned(), json!(summary.deep_link()));
    payload.insert("read".to_owned(), json!(summary.read()));
    payload.insert("created_at".to_owned(), json!(summary.created_at()));
    Ok(serde_json::Value::Object(payload))
}

fn preferences_json(preferences: &Preferences) -> serde_json::Value {
    let channel = |channel: Channel| {
        json!({
            "enabled": preferences.channel(channel).enabled(),
            "classes": crate::notify::NotificationClass::ALL.into_iter().map(|class| json!({
                "class": class.as_str(), "enabled": preferences.channel(channel).class_enabled(class)
            })).collect::<Vec<_>>()
        })
    };
    json!({"push": channel(Channel::Push), "email": channel(Channel::Email),
        "in_app": channel(Channel::InApp), "detail": preferences.detail().as_str()})
}

fn parse_preferences(body: &serde_json::Value) -> Result<Preferences, ApiFailure> {
    use crate::notify::{DetailLevel, NotificationClass};
    let mut preferences = Preferences::default();
    preferences.set_detail(match text_field(body, "detail")? {
        "full" => DetailLevel::Full,
        "summary" => DetailLevel::Summary,
        "minimal" => DetailLevel::Minimal,
        _ => return Err(ApiFailure::invalid_request(Some("detail"))),
    });
    for (name, channel) in [
        ("push", Channel::Push),
        ("email", Channel::Email),
        ("in_app", Channel::InApp),
    ] {
        let value = body
            .get(name)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?;
        let enabled = value
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?;
        preferences.set_channel(channel, enabled);
        let classes = value
            .get("classes")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| ApiFailure::invalid_request(Some(name)))?;
        if classes.len() != NotificationClass::ALL.len() {
            return Err(ApiFailure::invalid_request(Some(name)));
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in classes {
            let class_name = text_field(entry, "class")?;
            let class = NotificationClass::ALL
                .into_iter()
                .find(|candidate| candidate.as_str() == class_name)
                .ok_or_else(|| ApiFailure::invalid_request(Some("class")))?;
            if !seen.insert(class.as_str()) {
                return Err(ApiFailure::invalid_request(Some("class")));
            }
            let selected = entry
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| ApiFailure::invalid_request(Some("enabled")))?;
            preferences.set_class(channel, class, selected);
        }
    }
    Ok(preferences)
}

fn authenticator_method_json(method: &AuthenticatorMethod) -> serde_json::Value {
    let mut value = json!({"authenticator_id": method.id, "label": method.label, "enabled_at": method.enabled_at});
    if let Some(last_used_at) = method.last_used_at {
        value["last_used_at"] = json!(last_used_at);
    }
    value
}

fn authenticator_status_json(status: &AuthenticatorStatus) -> serde_json::Value {
    json!({"methods": status.methods.iter().map(authenticator_method_json).collect::<Vec<_>>(),
        "backup_codes_remaining": status.backup_codes_remaining})
}

fn timed_secret_json(secret: &crate::security::TimedSecret) -> serde_json::Value {
    json!({"value": secret.expose(), "remask_at": secret.remask_at(), "copyable": secret.copyable()})
}

fn auth_failure(error: super::production_auth::ProductionAuthError) -> ApiFailure {
    use super::production_auth::ProductionAuthError::*;
    match error {
        Unauthenticated => ApiFailure::unauthenticated(),
        SessionExpired => ApiFailure::session_expired(),
        InvalidDisclosure | UnclassifiedOperation => ApiFailure::invalid_request(None),
        CapabilitySpent | CapabilityRefused | Conflict => ApiFailure::forbidden(),
        InvalidConfiguration | IndexAuthentication | Entropy | Unavailable | Io(_) | Store(_)
        | Auth(_) => ApiFailure::unavailable(),
    }
}

fn now() -> Result<u64, ApiFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| ApiFailure::unavailable())
}

fn mutual_tls(
    root: &PathBuf,
    certificate: &PathBuf,
    key: &PathBuf,
) -> Result<MutualTlsConfig, String> {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(read_nonempty(root)?))
        .map_err(|_| "KMS trust root DER is invalid".to_owned())?;
    let certificate = CertificateDer::from(read_nonempty(certificate)?);
    let key = PrivateKeyDer::try_from(read_nonempty(key)?)
        .map_err(|_| "KMS client private key DER is invalid".to_owned())?;
    MutualTlsConfig::new(roots, vec![certificate], key)
        .map_err(|_| "KMS mutual TLS configuration is invalid".to_owned())
}

fn read_nonempty(path: &PathBuf) -> Result<Vec<u8>, String> {
    fs::read(path)
        .map_err(|_| format!("cannot read {}", path.display()))
        .and_then(|bytes| {
            if bytes.is_empty() {
                Err(format!("{} is empty", path.display()))
            } else {
                Ok(bytes)
            }
        })
}

fn required(name: &str) -> Result<String, String> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

fn absolute(name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(required(name)?);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{name} must be absolute"))
    }
}

fn number<T: std::str::FromStr>(name: &str) -> Result<T, String> {
    required(name)?
        .parse()
        .map_err(|_| format!("{name} is invalid"))
}

fn secret32(name: &str) -> Result<[u8; 32], String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(required(name)?)
        .map_err(|_| format!("{name} is invalid"))?;
    bytes
        .try_into()
        .map_err(|_| format!("{name} must encode exactly 32 bytes"))
}

fn decode_hex_20(value: &str) -> Result<[u8; 20], ()> {
    let value = value.strip_prefix("0x").ok_or(())?;
    if value.len() != 40 {
        return Err(());
    }
    let mut out = [0; 20];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| ())?;
    }
    Ok(out)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ()> {
    let value = value.strip_prefix("0x").ok_or(())?;
    if value.is_empty() || value.len() % 2 != 0 || value.len() > 512 {
        return Err(());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

fn journey_status_json(status: &crate::journeys::JourneyStatus) -> serde_json::Value {
    use crate::journeys::JourneyState;
    json!({"journey_id": status.journey_id().as_str(), "kind":"wallet-binding",
        "state": match status.state() { JourneyState::GettingReady => "getting-ready", JourneyState::Sending => "sending",
            JourneyState::Processing => "processing", JourneyState::StillChecking => "still-checking",
            JourneyState::Done => "done", JourneyState::Refused => "refused" },
        "current_leg": status.current_leg(),
        "receipt_verified": status.receipt_digests().iter().all(Option::is_some)})
}

fn decode_id(value: &str) -> Result<[u8; 32], ()> {
    let value = value.strip_prefix("apr_").unwrap_or(value);
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| ())?
        .try_into()
        .map_err(|_| ())
}

fn approval_json(
    hold: &AgentApprovalRecord,
    budget: crate::approvals::VerifiedBudgetAfter,
) -> serde_json::Value {
    let state = match hold.state {
        AgentApprovalState::AwaitingApproval => "pending",
        AgentApprovalState::Approved { .. } => "approved",
        AgentApprovalState::Rejected => "rejected",
        AgentApprovalState::Expired => "expired",
        AgentApprovalState::Defective => "defective",
    };
    let counterparty = hold
        .held_activity
        .counterparties
        .values()
        .first()
        .map(|value| value.as_str())
        .unwrap_or("unavailable");
    let amount = hold
        .held_activity
        .amounts
        .values()
        .first()
        .map_or(0, |value| value.amount.0);
    let asset = hold.held_activity.asset.as_str();
    json!({"approval_id": URL_SAFE_NO_PAD.encode(hold.approval_id), "agent_id": hold.held_activity.actor.as_str(),
        "agent_name": hold.held_activity.actor.as_str(), "counterparty": counterparty,
        "amount": {"amount": amount.to_string(), "currency": asset}, "reason_copy_key": hold.hold_reason_code,
        "expires_at": hold.expires_at_sequence, "created_at": hold.created_at_sequence, "state": state,
        "state_copy_key": format!("approval.state.{state}"), "facts": {"amount": {"amount": amount.to_string(), "currency": asset},
            "counterparty": counterparty, "asset": asset, "fees": {"amount": hold.held_activity.fee_limit.0.to_string(), "currency": asset},
            "expires_at": hold.expires_at_sequence},
        "budget_remaining_after": {"money": {"amount": budget.remaining.to_string(), "currency": asset},
            "verification": format!("{:?}", budget.level).to_ascii_lowercase()},
        "evidence": [{"kind": "approval-hold", "digest": URL_SAFE_NO_PAD.encode(hold.canonical_bytes_digest)},
            {"kind": "budget-read", "digest": URL_SAFE_NO_PAD.encode(budget.evidence_digest)}]})
}

fn append_approval_stream(
    stream: &super::stream_journal::StreamJournal,
    scope: &mut crate::store::PrincipalScope<'_>,
    hold: &AgentApprovalRecord,
    value: serde_json::Value,
    sequence: u64,
) -> Result<(), ApiFailure> {
    let (state, kind) = match hold.state {
        AgentApprovalState::AwaitingApproval => ("pending", "approval-created"),
        AgentApprovalState::Approved { .. } => ("approved", "approval-approved"),
        AgentApprovalState::Rejected => ("rejected", "approval-rejected"),
        AgentApprovalState::Expired | AgentApprovalState::Defective => {
            ("expired", "approval-expired")
        }
    };
    stream.append(
        scope,
        &format!(
            "approval:{}:{state}",
            URL_SAFE_NO_PAD.encode(hold.approval_id)
        ),
        kind,
        sequence,
        json!({"approval":value}),
    )
}

fn hex_bytes(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(H[usize::from(byte >> 4)]),
                char::from(H[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn hex20(name: &str) -> Result<[u8; 20], String> {
    let value = required(name)?;
    let digits = value
        .strip_prefix("0x")
        .ok_or_else(|| format!("{name} is invalid"))?;
    if digits.len() != 40 {
        return Err(format!("{name} is invalid"));
    }
    let mut output = [0; 20];
    for (slot, pair) in output.iter_mut().zip(digits.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).map_err(|_| format!("{name} is invalid"))?;
        *slot = u8::from_str_radix(pair, 16).map_err(|_| format!("{name} is invalid"))?;
    }
    Ok(output)
}

fn selected_protocol(value: Option<&str>) -> Result<u16, String> {
    let protocol = value.map_or(Ok(layerx_wire::limits::PROTOCOL_VERSION), |value| {
        value
            .parse::<u16>()
            .map_err(|_| "LAYERX_HUMAN_PROTOCOL_VERSION is invalid".to_owned())
    })?;
    NativeMovementCodec::for_protocol(protocol)
        .map_err(|_| "LAYERX_HUMAN_PROTOCOL_VERSION is unsupported".to_owned())?;
    Ok(protocol)
}

#[cfg(test)]
mod protocol_tests {
    use super::selected_protocol;
    #[test]
    fn explicit_beta_protocol_preserves_legacy_default_and_refuses_unknown_versions() {
        assert_eq!(selected_protocol(None), Ok(2));
        assert_eq!(selected_protocol(Some("2")), Ok(2));
        assert_eq!(selected_protocol(Some("3")), Ok(3));
        for invalid in ["", "0", "1", "4", "65536", "three"] {
            assert!(selected_protocol(Some(invalid)).is_err());
        }
    }
}

fn configured_protocol() -> Result<u16, String> {
    match env::var("LAYERX_HUMAN_PROTOCOL_VERSION") {
        Ok(value) => selected_protocol(Some(&value)),
        Err(env::VarError::NotPresent) => selected_protocol(None),
        Err(env::VarError::NotUnicode(_)) => {
            Err("LAYERX_HUMAN_PROTOCOL_VERSION is invalid".to_owned())
        }
    }
}
