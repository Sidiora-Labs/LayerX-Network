use std::collections::BTreeMap;

use serde_json::Value;

use crate::auth::{AccountIdentity, Passkeys};
use crate::store::{PrincipalId, PrincipalScope, PrincipalStore};

use super::backend::{ApiFailure, BackendResponse, ComponentState, Readiness};
use super::identity::IdentityProjector;
use super::privileged::{
    map_auth_error, AuthorizedSession, ComponentOperationRequest, PrivilegedHumanServices,
};

/// One operator-provisioned application identity admitted by the authenticated
/// tenancy map. Provisioning is deliberately separate from public account
/// creation until custody and agent authorities can construct the full
/// receipt-gated onboarding journey.
#[derive(Clone)]
pub struct ProvisionedAccount {
    principal: PrincipalId,
    email: String,
    display_name: String,
    passkey_label: String,
}

impl ProvisionedAccount {
    /// Validates one non-secret account directory entry.
    ///
    /// # Errors
    ///
    /// Refuses malformed account identifiers, non-canonical email addresses,
    /// or display values that the passkey verifier would reject.
    pub fn new(
        account_id: impl Into<String>,
        email: impl Into<String>,
        display_name: impl Into<String>,
        passkey_label: impl Into<String>,
    ) -> Result<Self, ApiFailure> {
        let principal = PrincipalId::new(account_id.into())
            .map_err(|_| ApiFailure::invalid_request(Some("account_id")))?;
        if !principal.as_str().starts_with("act_") {
            return Err(ApiFailure::invalid_request(Some("account_id")));
        }
        let email = email.into();
        let display_name = display_name.into();
        let passkey_label = passkey_label.into();
        if !valid_email(&email)
            || AccountIdentity::new(email.clone(), display_name.clone()).is_err()
            || passkey_label.is_empty()
            || passkey_label.len() > 256
            || passkey_label.chars().any(char::is_control)
        {
            return Err(ApiFailure::invalid_request(None));
        }
        Ok(Self {
            principal,
            email,
            display_name,
            passkey_label,
        })
    }

    #[must_use]
    pub const fn principal(&self) -> &PrincipalId {
        &self.principal
    }

    #[must_use]
    pub fn email(&self) -> &str {
        &self.email
    }

    fn identity(&self) -> Result<AccountIdentity, ApiFailure> {
        AccountIdentity::new(self.email.clone(), self.display_name.clone()).map_err(map_auth_error)
    }
}

/// Immutable startup-authenticated routing for provisioned beta identities.
pub struct ProvisionedAccounts {
    by_principal: BTreeMap<String, ProvisionedAccount>,
    by_email: BTreeMap<String, String>,
}

impl ProvisionedAccounts {
    /// Builds a total, duplicate-free account directory.
    ///
    /// # Errors
    ///
    /// Refuses an empty directory or duplicate account and email routes.
    pub fn new(accounts: impl IntoIterator<Item = ProvisionedAccount>) -> Result<Self, ApiFailure> {
        let mut by_principal = BTreeMap::new();
        let mut by_email = BTreeMap::new();
        for account in accounts {
            let principal = account.principal.as_str().to_owned();
            if by_email
                .insert(account.email.clone(), principal.clone())
                .is_some()
                || by_principal.insert(principal, account).is_some()
            {
                return Err(ApiFailure::invalid_request(None));
            }
        }
        if by_principal.is_empty() {
            return Err(ApiFailure::unavailable());
        }
        Ok(Self {
            by_principal,
            by_email,
        })
    }

    fn account(&self, principal: &PrincipalId) -> Result<&ProvisionedAccount, ApiFailure> {
        self.by_principal
            .get(principal.as_str())
            .ok_or_else(ApiFailure::forbidden)
    }

    fn principal_for_email(&self, email: &str) -> Result<PrincipalId, ApiFailure> {
        let principal = self.by_email.get(email).ok_or_else(ApiFailure::forbidden)?;
        PrincipalId::new(principal.clone()).map_err(|_| ApiFailure::unavailable())
    }

    fn principals(&self) -> impl Iterator<Item = &PrincipalId> {
        self.by_principal.values().map(|account| &account.principal)
    }
}

/// Concrete privileged service for the durable passkey and browser-session
/// plane. Unsupported schema operations fail explicitly instead of falling
/// back to process memory or bypassing their absent production authorities.
pub struct IdentityServices {
    accounts: ProvisionedAccounts,
}

impl IdentityServices {
    #[must_use]
    pub const fn new(accounts: ProvisionedAccounts) -> Self {
        Self { accounts }
    }

    fn registration_begin(
        &self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        let account_id = text(&request.body, "account_id")?;
        let principal = PrincipalId::new(account_id.to_owned())
            .map_err(|_| ApiFailure::invalid_request(Some("account_id")))?;
        let account = self.accounts.account(&principal)?;
        let identity = account.identity()?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let challenge = passkeys
            .begin_registration(&mut scope, &identity, &account.passkey_label, now)
            .map_err(map_auth_error)?;
        Ok(response(IdentityProjector::registration_challenge(
            &challenge,
        )))
    }

    fn registration_finish(
        &self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        let registration_id = path(request, "registration_id")?;
        let principal = Passkeys::principal_for_registration(registration_id, store.tenancy())
            .map_err(map_auth_error)?;
        self.accounts.account(&principal)?;
        let credential = text(&request.body, "credential")?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let passkey = passkeys
            .finish_registration(&mut scope, registration_id, credential, now)
            .map_err(map_auth_error)?;
        Ok(response(IdentityProjector::passkey(&passkey)))
    }

    fn assertion_begin(
        &self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        let email = text(&request.body, "email")?;
        let principal = self.accounts.principal_for_email(email)?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let challenge = passkeys
            .begin_assertion(&mut scope, now)
            .map_err(map_auth_error)?;
        Ok(response(IdentityProjector::assertion_challenge(&challenge)))
    }

    fn assertion_finish(
        &self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        let assertion_id = path(request, "assertion_id")?;
        let principal = Passkeys::principal_for_assertion(assertion_id, store.tenancy())
            .map_err(map_auth_error)?;
        self.accounts.account(&principal)?;
        let credential = text(&request.body, "credential")?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let assertion = passkeys
            .finish_assertion(&mut scope, assertion_id, credential, now)
            .map_err(map_auth_error)?;
        Ok(response(IdentityProjector::assertion(&assertion)))
    }

    fn session_open(
        &self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        let assertion_id = text(&request.body, "assertion_id")?;
        let principal = Passkeys::principal_for_assertion(assertion_id, store.tenancy())
            .map_err(map_auth_error)?;
        self.accounts.account(&principal)?;
        let device = IdentityProjector::mint_session_device(&request.body)?;
        let mut scope = store
            .principal(&principal)
            .map_err(|_| ApiFailure::unavailable())?;
        let grant = passkeys
            .open_session(&mut scope, assertion_id, device, now)
            .map_err(map_auth_error)?;
        IdentityProjector::session_grant(&grant, now)
    }

    fn execute_session(
        passkeys: &Passkeys,
        scope: &mut PrincipalScope<'_>,
        session: &AuthorizedSession<'_>,
        request: &ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        match request.operation.name.as_str() {
            "session.refresh" => {
                let csrf = session.csrf_token.ok_or_else(ApiFailure::forbidden)?;
                let grant = passkeys
                    .refresh_session(scope, session.token, csrf, now)
                    .map_err(map_auth_error)?;
                IdentityProjector::session_grant(&grant, now)
            }
            "session.list" => {
                let sessions = passkeys
                    .list_sessions(scope, session.token, now)
                    .map_err(map_auth_error)?;
                Ok(response(IdentityProjector::sessions(&sessions)))
            }
            "session.revoke" => {
                let csrf = session.csrf_token.ok_or_else(ApiFailure::forbidden)?;
                let target = path(request, "session_id")?;
                let revoked = passkeys
                    .revoke_session(scope, session.token, csrf, target, now)
                    .map_err(map_auth_error)?;
                Ok(response(IdentityProjector::revocation(&revoked)))
            }
            "session.revoke-all" => {
                let csrf = session.csrf_token.ok_or_else(ApiFailure::forbidden)?;
                let revoked = passkeys
                    .sign_out_everywhere(scope, session.token, csrf, now)
                    .map_err(map_auth_error)?;
                Ok(response(IdentityProjector::revocation(&revoked)))
            }
            _ => Err(ApiFailure::unavailable()),
        }
    }
}

impl PrivilegedHumanServices for IdentityServices {
    fn execute_public(
        &mut self,
        store: &mut PrincipalStore,
        passkeys: &Passkeys,
        request: ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        match request.operation.name.as_str() {
            "passkey.register.begin" => self.registration_begin(store, passkeys, &request, now),
            "passkey.register.finish" => self.registration_finish(store, passkeys, &request, now),
            "passkey.assert.begin" => self.assertion_begin(store, passkeys, &request, now),
            "passkey.assert.finish" => self.assertion_finish(store, passkeys, &request, now),
            "session.open" => self.session_open(store, passkeys, &request, now),
            "account.create" => Err(ApiFailure::unavailable()),
            _ => Err(ApiFailure::not_found()),
        }
    }

    fn execute_authorized(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        passkeys: &Passkeys,
        session: AuthorizedSession<'_>,
        request: ComponentOperationRequest<'_>,
        now: u64,
    ) -> Result<BackendResponse, ApiFailure> {
        if request.operation.name.starts_with("session.") {
            Self::execute_session(passkeys, scope, &session, &request, now)
        } else {
            Err(ApiFailure::unavailable())
        }
    }

    fn readiness(
        &mut self,
        store: &mut PrincipalStore,
        _now: u64,
    ) -> Result<Readiness, ApiFailure> {
        if self
            .accounts
            .principals()
            .any(|principal| store.tenancy().tenant_for(principal).is_err())
        {
            return Err(ApiFailure::unavailable());
        }
        Ok(Readiness {
            human_service: ComponentState::Ready,
            custody: ComponentState::Degraded,
            agent: ComponentState::Unavailable,
            core: ComponentState::Unavailable,
            paxeer: ComponentState::Unavailable,
        })
    }
}

fn response(result: Value) -> BackendResponse {
    BackendResponse {
        result,
        session: None,
    }
}

fn text<'value>(value: &'value Value, name: &str) -> Result<&'value str, ApiFailure> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))
}

fn path<'request>(
    request: &'request ComponentOperationRequest<'_>,
    name: &str,
) -> Result<&'request str, ApiFailure> {
    request
        .path_parameters
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| ApiFailure::invalid_request(Some(name)))
}

fn valid_email(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 256
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
        || value.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return false;
    }
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && parts.next().is_none()
        && !domain.starts_with('.')
        && !domain.ends_with('.')
}
