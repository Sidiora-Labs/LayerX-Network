use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use zeroize::Zeroize;

use crate::auth::{
    AccessDecision, AuthError, AuthorizationRequest, OperationClass, Passkeys, SessionContext,
};
use crate::store::{PrincipalScope, PrincipalStore};

use super::backend::{
    ApiFailure, BackendResponse, HumanApiComponents, PrincipalContext, Readiness, ScopedRequest,
    SessionCredentials,
};
use super::schema::Operation;

/// Finite lifetime and cardinality for transient authorize-to-execute grants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizationGrantPolicy {
    pub lifetime_seconds: u64,
    pub maximum_outstanding: usize,
}

impl AuthorizationGrantPolicy {
    fn validate(self) -> Result<Self, ApiFailure> {
        if self.lifetime_seconds == 0 || self.maximum_outstanding == 0 {
            Err(ApiFailure::unavailable())
        } else {
            Ok(self)
        }
    }
}

/// One schema-bound operation admitted to the concrete human services.
pub struct ComponentOperationRequest<'operation> {
    pub operation: &'operation Operation,
    pub path_parameters: BTreeMap<String, String>,
    pub body: serde_json::Value,
    pub idempotency_key: Option<String>,
    pub trace: String,
}

/// The exact verified browser session supplied to an authorized operation.
pub struct AuthorizedSession<'credential> {
    pub session: &'credential SessionContext,
    pub token: &'credential str,
    pub csrf_token: Option<&'credential str>,
    pub refresh: bool,
}

/// Concrete service ownership behind the privileged component process.
pub trait PrivilegedHumanServices: Send + 'static {
    fn execute_public(
        &mut self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure>;

    fn execute_authorized(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        passkeys: &Passkeys,
        session: AuthorizedSession<'_>,
        request: ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure>;

    fn readiness(
        &mut self,
        store: &mut PrincipalStore,
        now: u64,
    ) -> Result<Readiness, ApiFailure>;
}

/// Real passkey and principal-scoping middleware for concrete human services.
pub struct PrivilegedHumanComponents<S: PrivilegedHumanServices> {
    state: Mutex<PrivilegedState<S>>,
    policy: AuthorizationGrantPolicy,
}

struct PrivilegedState<S> {
    store: PrincipalStore,
    passkeys: Passkeys,
    services: S,
    grants: BTreeMap<String, AuthorizationGrant>,
}

struct AuthorizationGrant {
    principal: String,
    tenant: String,
    operation: String,
    trace: String,
    token: String,
    csrf_token: Option<String>,
    refresh: bool,
    session: SessionContext,
    expires_at: u64,
}

impl Drop for AuthorizationGrant {
    fn drop(&mut self) {
        self.operation.zeroize();
        self.trace.zeroize();
        self.token.zeroize();
        if let Some(token) = &mut self.csrf_token {
            token.zeroize();
        }
    }
}

impl<S: PrivilegedHumanServices> PrivilegedHumanComponents<S> {
    pub fn new(
        store: PrincipalStore,
        passkeys: Passkeys,
        services: S,
        policy: AuthorizationGrantPolicy,
    ) -> Result<Self, ApiFailure> {
        Ok(Self {
            state: Mutex::new(PrivilegedState {
                store,
                passkeys,
                services,
                grants: BTreeMap::new(),
            }),
            policy: policy.validate()?,
        })
    }
}

impl<S: PrivilegedHumanServices> HumanApiComponents for PrivilegedHumanComponents<S> {
    fn authorize(
        &self,
        operation: &Operation,
        credentials: SessionCredentials<'_>,
        trace: &str,
    ) -> Result<PrincipalContext, ApiFailure> {
        if operation.is_public_bootstrap() {
            return Err(ApiFailure::forbidden());
        }
        let now = unix_seconds()?;
        let mut state = self.state.lock().map_err(|_| ApiFailure::unavailable())?;
        let passkeys = state.passkeys.clone();
        let principal = passkeys
            .principal_for_token(credentials.access_token, state.store.tenancy())
            .map_err(map_auth_error)?;
        let mut scope = state
            .store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let tenant = scope.tenant().clone();
        let session = if credentials.refresh {
            let csrf = credentials.csrf_token.ok_or_else(ApiFailure::forbidden)?;
            passkeys
                .authorize_refresh(&scope, credentials.access_token, csrf, now)
                .map_err(map_auth_error)?
        } else {
            let operation_class = if operation.mutates() {
                OperationClass::Mutation
            } else {
                OperationClass::Read
            };
            match passkeys
                .authorize(
                    &mut scope,
                    credentials.access_token,
                    credentials.csrf_token,
                    &AuthorizationRequest {
                        operation: operation_class,
                        digest: None,
                        step_up: None,
                        intended_destination: credentials.intended_destination,
                    },
                    now,
                )
                .map_err(map_auth_error)?
            {
                AccessDecision::Authorized(session) => session,
                AccessDecision::Reauthenticate { .. } => return Err(ApiFailure::session_expired()),
            }
        };
        drop(scope);
        state.grants.retain(|_, grant| grant.expires_at >= now);
        if state.grants.len() >= self.policy.maximum_outstanding {
            return Err(ApiFailure::unavailable());
        }
        let authorization = mint_authorization()?;
        let expires_at = now
            .checked_add(self.policy.lifetime_seconds)
            .ok_or_else(ApiFailure::unavailable)?;
        state.grants.insert(
            authorization.clone(),
            AuthorizationGrant {
                principal: principal.as_str().to_owned(),
                tenant: tenant.as_str().to_owned(),
                operation: operation.name.clone(),
                trace: trace.to_owned(),
                token: credentials.access_token.to_owned(),
                csrf_token: credentials.csrf_token.map(str::to_owned),
                refresh: credentials.refresh,
                session: session.clone(),
                expires_at,
            },
        );
        Ok(PrincipalContext {
            principal,
            tenant,
            session_id: session.session_id,
            authorization,
        })
    }

    fn execute(&self, request: ScopedRequest<'_>) -> Result<BackendResponse, ApiFailure> {
        let now = unix_seconds()?;
        let mut state = self.state.lock().map_err(|_| ApiFailure::unavailable())?;
        let operation_request = ComponentOperationRequest {
            operation: request.operation,
            path_parameters: request.path_parameters,
            body: request.body,
            idempotency_key: request.idempotency_key,
            trace: request.trace,
        };
        let Some(context) = request.principal else {
            if !request.operation.is_public_bootstrap() {
                return Err(ApiFailure::unauthenticated());
            }
            let passkeys = state.passkeys.clone();
            let PrivilegedState {
                store, services, ..
            } = &mut *state;
            return services.execute_public(store, &passkeys, operation_request, now);
        };
        let grant = state
            .grants
            .remove(&context.authorization)
            .ok_or_else(ApiFailure::unauthenticated)?;
        if grant.expires_at < now
            || grant.principal != context.principal.as_str()
            || grant.tenant != context.tenant.as_str()
            || grant.session.session_id != context.session_id
            || grant.operation != request.operation.name
            || grant.trace != operation_request.trace
        {
            return Err(ApiFailure::forbidden());
        }
        let passkeys = state.passkeys.clone();
        let PrivilegedState {
            store, services, ..
        } = &mut *state;
        let mut scope = store
            .principal(&context.principal)
            .map_err(|_| ApiFailure::unavailable())?;
        if scope.tenant() != &context.tenant {
            return Err(ApiFailure::forbidden());
        }
        let session = if grant.refresh {
            let csrf = grant.csrf_token.as_deref().ok_or_else(ApiFailure::forbidden)?;
            passkeys
                .authorize_refresh(&scope, &grant.token, csrf, now)
                .map_err(map_auth_error)?
        } else {
            let operation_class = if request.operation.mutates() {
                OperationClass::Mutation
            } else {
                OperationClass::Read
            };
            match passkeys
                .authorize(
                    &mut scope,
                    &grant.token,
                    grant.csrf_token.as_deref(),
                    &AuthorizationRequest {
                        operation: operation_class,
                        digest: None,
                        step_up: None,
                        intended_destination: "/",
                    },
                    now,
                )
                .map_err(map_auth_error)?
            {
                AccessDecision::Authorized(session) => session,
                AccessDecision::Reauthenticate { .. } => return Err(ApiFailure::session_expired()),
            }
        };
        if session.session_id != grant.session.session_id {
            return Err(ApiFailure::forbidden());
        }
        services.execute_authorized(
            &mut scope,
            &passkeys,
            AuthorizedSession {
                session: &session,
                token: &grant.token,
                csrf_token: grant.csrf_token.as_deref(),
                refresh: grant.refresh,
            },
            operation_request,
            now,
        )
    }

    fn readiness(&self, _trace: &str) -> Result<Readiness, ApiFailure> {
        let now = unix_seconds()?;
        let mut state = self.state.lock().map_err(|_| ApiFailure::unavailable())?;
        let PrivilegedState {
            store, services, ..
        } = &mut *state;
        services.readiness(store, now)
    }
}

fn mint_authorization() -> Result<String, ApiFailure> {
    let mut entropy = [0_u8; 32];
    getrandom::fill(&mut entropy).map_err(|_| ApiFailure::unavailable())?;
    let encoded = URL_SAFE_NO_PAD.encode(entropy);
    entropy.zeroize();
    Ok(encoded)
}

fn unix_seconds() -> Result<u64, ApiFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiFailure::unavailable())
        .map(|duration| duration.as_secs())
}

fn map_auth_error(error: AuthError) -> ApiFailure {
    match error {
        AuthError::Unauthenticated | AuthError::SessionNotFound => ApiFailure::unauthenticated(),
        AuthError::SessionExpired => ApiFailure::session_expired(),
        AuthError::RateLimited {
            retry_after_secs, ..
        } => ApiFailure::rate_limited(retry_after_secs.saturating_mul(1_000)),
        AuthError::InvalidInput(_)
        | AuthError::MalformedCredential
        | AuthError::Passkey(_)
        | AuthError::ForgeryRefused
        | AuthError::FallbackRefused
        | AuthError::FallbackRestricted => ApiFailure::forbidden(),
        AuthError::StepUpRequired | AuthError::StepUpMismatch | AuthError::StepUpExpired => {
            ApiFailure {
                status: 403,
                code: "step-up-required".to_owned(),
                copy_key: "error.step-up.required".to_owned(),
                retry: "structural".to_owned(),
                retry_after_ms: None,
                field: None,
            }
        }
        AuthError::ChallengeNotFound | AuthError::CredentialNotFound => ApiFailure::not_found(),
        AuthError::ChallengeExpired => ApiFailure {
            status: 409,
            code: "challenge-expired".to_owned(),
            copy_key: "error.challenge.expired".to_owned(),
            retry: "structural".to_owned(),
            retry_after_ms: None,
            field: None,
        },
        AuthError::InvalidConfiguration
        | AuthError::EntropyUnavailable
        | AuthError::Encoding
        | AuthError::CorruptState
        | AuthError::Store(_)
        | AuthError::CredentialConflict
        | AuthError::NoPasskeys
        | AuthError::LastPasskey
        | AuthError::AssertionNotVerified
        | AuthError::AssertionSpent
        | AuthError::SizeOverflow => ApiFailure::unavailable(),
    }
}
