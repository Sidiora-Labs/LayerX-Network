//! Passkey-first authentication, browser sessions and restricted fallback access.
#![allow(clippy::missing_errors_doc)]
//!
//! `WebAuthn` ceremony verification is delegated to `passkey-auth`; this module
//! owns the server-side state, principal scoping, replay prevention, opaque
//! browser tokens, device inventory and rate limits around that verifier.

mod stepup;

pub use stepup::{OperationDigest, StepUp, StepUpChallenge, StepUpEvidence};

use std::fmt::{Display, Formatter};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use passkey_auth::{
    AuthenticationResponse, AuthenticationState, CredentialId, PasskeyCredential,
    RegistrationResponse, RegistrationState, Webauthn,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::{Zeroize as _, Zeroizing};

use crate::store::{PrincipalId, PrincipalScope, RowKey, StoreError, Table, TenancyMap};

const PASSKEY_ROW_PREFIX: &str = "auth-passkey-";
const REGISTRATION_ROW_PREFIX: &str = "auth-registration-";
const ASSERTION_ROW_PREFIX: &str = "auth-assertion-";
const VERIFIED_ASSERTION_ROW_PREFIX: &str = "auth-verified-";
const SESSION_ROW_PREFIX: &str = "auth-session-";
const DEVICE_ROW_PREFIX: &str = "auth-device-";
const FALLBACK_ROW: &str = "auth-fallback-primary";
const SESSION_EPOCH_ROW: &str = "auth-epoch-sessions";
const RATE_ROW_PREFIX: &str = "auth-rate-";
const NEW_DEVICE_ROW_PREFIX: &str = "auth-new-device-";
const TOKEN_DOMAIN: &[u8] = b"layerx-human-session-token/v1";
const TOKEN_ROUTE_DOMAIN: &[u8] = b"layerx-human-session-route/v1";
const FALLBACK_DOMAIN: &[u8] = b"layerx-human-fallback/v1";
const USER_HANDLE_DOMAIN: &[u8] = b"layerx-human-passkey-user/v1";
const IDENTIFIER_ENTROPY_BYTES: usize = 16;
const SECRET_BYTES: usize = 32;
const TEXT_LIMIT: usize = 256;

/// Declared per-principal request limiting policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RateLimit {
    /// Maximum admitted attempts in one window.
    pub attempts: u32,
    /// Window duration in seconds.
    pub window_secs: u64,
}

/// Security-relevant authentication and session policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthConfig {
    /// Bare `WebAuthn` relying-party domain.
    pub rp_id: String,
    /// Name browsers display for the relying party.
    pub rp_name: String,
    /// Exact HTTPS browser origin accepted by `WebAuthn` verification.
    pub origin: String,
    /// Server-side registration/assertion challenge lifetime.
    pub ceremony_ttl_secs: u64,
    /// Lifetime of a verified assertion before it must open a session.
    pub assertion_ttl_secs: u64,
    /// Access-token lifetime.
    pub session_ttl_secs: u64,
    /// Refresh-token lifetime.
    pub refresh_ttl_secs: u64,
    /// Fresh step-up evidence lifetime.
    pub step_up_ttl_secs: u64,
    /// Per-principal limiter applied independently to each auth purpose.
    pub rate_limit: RateLimit,
}

impl AuthConfig {
    fn validate(&self) -> Result<(), AuthError> {
        let rp_valid = !self.rp_id.is_empty()
            && self.rp_id.len() <= TEXT_LIMIT
            && !self.rp_id.contains("://")
            && !self.rp_id.contains('/')
            && !self.rp_id.contains(':')
            && self.rp_id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
            });
        let origin_valid = self.origin == format!("https://{}", self.rp_id);
        let durations_valid = self.ceremony_ttl_secs > 0
            && self.ceremony_ttl_secs <= 300
            && self.assertion_ttl_secs > 0
            && self.session_ttl_secs > 0
            && self.refresh_ttl_secs >= self.session_ttl_secs
            && self.step_up_ttl_secs > 0
            && self.step_up_ttl_secs <= self.ceremony_ttl_secs;
        if !rp_valid
            || self.rp_name.is_empty()
            || self.rp_name.len() > TEXT_LIMIT
            || !origin_valid
            || !durations_valid
            || self.rate_limit.attempts == 0
            || self.rate_limit.window_secs == 0
        {
            return Err(AuthError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Validated application identity used only to construct a registration
/// ceremony. The email/display strings are never stored in session tokens or
/// telemetry by this module.
pub struct AccountIdentity {
    username: String,
    display_name: String,
}

impl AccountIdentity {
    /// Creates the account identity `WebAuthn` shows during registration.
    pub fn new(
        username: impl Into<String>,
        display_name: impl Into<String>,
    ) -> Result<Self, AuthError> {
        let username = username.into();
        let display_name = display_name.into();
        if !valid_display_text(&username) || !valid_display_text(&display_name) {
            return Err(AuthError::InvalidInput("invalid account identity"));
        }
        Ok(Self {
            username,
            display_name,
        })
    }
}

impl std::fmt::Debug for AccountIdentity {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AccountIdentity([redacted])")
    }
}

/// A registered passkey's application identifier and safe metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PasskeyRecord {
    passkey_id: String,
    label: String,
    created_at: u64,
    last_used_at: Option<u64>,
}

impl PasskeyRecord {
    /// Application-level passkey identifier.
    #[must_use]
    pub fn passkey_id(&self) -> &str {
        &self.passkey_id
    }

    /// User-visible passkey label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Registration time.
    #[must_use]
    pub const fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Most recent cryptographically verified use.
    #[must_use]
    pub const fn last_used_at(&self) -> Option<u64> {
        self.last_used_at
    }
}

/// Opaque registration ceremony returned to the browser layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationChallenge {
    /// Server-side handle for finishing this ceremony exactly once.
    pub registration_id: String,
    /// Base64url-wrapped `WebAuthn` creation-options JSON.
    pub ceremony: String,
    /// Injected service-clock expiry.
    pub expires_at: u64,
}

/// Opaque assertion ceremony returned to the browser layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertionChallenge {
    /// Server-side handle for finishing this ceremony exactly once.
    pub assertion_id: String,
    /// Base64url-wrapped `WebAuthn` request-options JSON.
    pub ceremony: String,
    /// Injected service-clock expiry.
    pub expires_at: u64,
}

/// A verified assertion that may open one session before its expiry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssertionProof {
    /// The assertion handle consumed by [`Passkeys::open_session`].
    pub assertion_id: String,
    /// Passkey that produced the verified signature.
    pub passkey_id: String,
    /// Deadline for opening the session.
    pub expires_at: u64,
}

/// One browser or native-shell device recorded in the inventory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Device {
    id: String,
    label: String,
    platform: String,
}

impl Device {
    /// Creates a bounded device descriptor.
    pub fn new(
        device_id: impl Into<String>,
        label: impl Into<String>,
        platform: impl Into<String>,
    ) -> Result<Self, AuthError> {
        let device_id = device_id.into();
        let label = label.into();
        let platform = platform.into();
        if !valid_prefixed_id(&device_id, "dev_")
            || !valid_display_text(&label)
            || !valid_machine_label(&platform)
        {
            return Err(AuthError::InvalidInput("invalid device"));
        }
        Ok(Self {
            id: device_id,
            label,
            platform,
        })
    }

    /// Device identifier.
    #[must_use]
    pub fn device_id(&self) -> &str {
        &self.id
    }

    /// User-visible device label.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Stable platform label.
    #[must_use]
    pub fn platform(&self) -> &str {
        &self.platform
    }
}

/// Secret value returned once. Its debug representation is always redacted.
#[derive(Clone, Eq, PartialEq)]
pub struct OpaqueSecret(Zeroizing<String>);

impl OpaqueSecret {
    /// Returns the secret for placement into the appropriate protected client
    /// storage or header.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for OpaqueSecret {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("OpaqueSecret([redacted])")
    }
}

/// Browser credentials issued together and rotated together.
#[derive(Clone, Eq, PartialEq)]
pub struct SessionGrant {
    session_id: String,
    access_token: OpaqueSecret,
    refresh_token: OpaqueSecret,
    csrf_token: OpaqueSecret,
    access_expires_at: u64,
    refresh_expires_at: u64,
}

impl SessionGrant {
    /// Session identifier shown in inventory.
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Opaque access token.
    #[must_use]
    pub const fn access_token(&self) -> &OpaqueSecret {
        &self.access_token
    }

    /// Opaque single-generation refresh token.
    #[must_use]
    pub const fn refresh_token(&self) -> &OpaqueSecret {
        &self.refresh_token
    }

    /// Browser anti-forgery token.
    #[must_use]
    pub const fn csrf_token(&self) -> &OpaqueSecret {
        &self.csrf_token
    }

    /// Access expiry.
    #[must_use]
    pub const fn access_expires_at(&self) -> u64 {
        self.access_expires_at
    }

    /// Refresh expiry.
    #[must_use]
    pub const fn refresh_expires_at(&self) -> u64 {
        self.refresh_expires_at
    }
}

impl std::fmt::Debug for SessionGrant {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionGrant")
            .field("session_id", &self.session_id)
            .field("access_token", &self.access_token)
            .field("refresh_token", &self.refresh_token)
            .field("csrf_token", &self.csrf_token)
            .field("access_expires_at", &self.access_expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

/// Inventory projection for one session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionView {
    /// Session identifier.
    pub session_id: String,
    /// Device holding it.
    pub device: Device,
    /// Initial issue time.
    pub opened_at: u64,
    /// Most recent successful use.
    pub last_active_at: u64,
    /// Whether this is the caller's session.
    pub current: bool,
    /// Whether the session can mutate state.
    pub restricted: bool,
}

/// Revocation result returned by single-session and sign-out-everywhere paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRevocation {
    /// Sessions made unusable for both access and refresh.
    pub revoked_session_ids: Vec<String>,
    /// Effective time.
    pub revoked_at: u64,
}

/// A one-time high-entropy fallback credential. It can only create a
/// read-only recovery session and can never satisfy step-up.
#[derive(Debug)]
pub struct FallbackCredential {
    secret: OpaqueSecret,
    expires_at: u64,
}

impl FallbackCredential {
    /// One-time fallback secret.
    #[must_use]
    pub const fn secret(&self) -> &OpaqueSecret {
        &self.secret
    }

    /// Fallback expiry.
    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Classes of authenticated operations. The designated classes require a
/// fresh passkey ceremony bound to the exact operation digest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OperationClass {
    Read,
    Mutation,
    MoneyMovement,
    Approval,
    Withdrawal,
    Exit,
    SecuritySettings,
    SecretReveal,
    WalletRebind,
    AgentArchive,
}

impl OperationClass {
    const fn mutates(self) -> bool {
        !matches!(self, Self::Read)
    }

    /// Returns whether this class requires fresh step-up evidence.
    #[must_use]
    pub const fn requires_step_up(self) -> bool {
        matches!(
            self,
            Self::Approval
                | Self::Withdrawal
                | Self::Exit
                | Self::SecuritySettings
                | Self::SecretReveal
                | Self::WalletRebind
                | Self::AgentArchive
        )
    }
}

/// One authorization decision request.
#[derive(Clone, Debug)]
pub struct AuthorizationRequest<'a> {
    /// Operation class.
    pub operation: OperationClass,
    /// Exact canonical operation digest for a designated step-up operation.
    pub digest: Option<OperationDigest>,
    /// Fresh evidence, when required.
    pub step_up: Option<&'a StepUpEvidence>,
    /// Destination restored after re-authentication if the session expired.
    pub intended_destination: &'a str,
}

/// Authenticated session context passed to authorized service operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionContext {
    /// Session identifier.
    pub session_id: String,
    /// Device identifier.
    pub device_id: String,
}

/// Honest decision for an expired or active browser session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccessDecision {
    /// The requested operation may proceed.
    Authorized(SessionContext),
    /// The browser must re-authenticate and then return to this destination.
    Reauthenticate { intended_destination: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
enum Assurance {
    Passkey,
    FallbackRestricted,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPasskey {
    record: PasskeyRecord,
    credential: PasskeyCredential,
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistrationRecord {
    state: RegistrationState,
    label: String,
    expires_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssertionRecord {
    state: AuthenticationState,
    candidate_passkeys: Vec<String>,
    expires_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct VerifiedAssertion {
    passkey_id: String,
    expires_at: u64,
    consumed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionRecord {
    session_id: String,
    device: Device,
    opened_at: u64,
    last_active_at: u64,
    access_expires_at: u64,
    refresh_expires_at: u64,
    access_digest: [u8; 32],
    refresh_digest: [u8; 32],
    csrf_digest: [u8; 32],
    epoch: u64,
    revoked_at: Option<u64>,
    assurance: Assurance,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceRecord {
    device: Device,
    first_seen_at: u64,
    last_active_at: u64,
}

#[derive(Debug, Serialize)]
struct NewDeviceNotification {
    notification_id: String,
    class: &'static str,
    title_copy_key: &'static str,
    body_copy_key: &'static str,
    deep_link: &'static str,
    action_copy_key: &'static str,
    device_id: String,
    read: bool,
    created_at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct FallbackRecord {
    salt: [u8; 32],
    digest: [u8; 32],
    expires_at: u64,
    consumed_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
struct SessionEpoch {
    value: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct RateWindow {
    started_at: u64,
    attempts: u32,
}

#[derive(Clone, Copy)]
enum RatePurpose {
    Register,
    Assert,
    Fallback,
    StepUp,
}

impl RatePurpose {
    const fn label(self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Assert => "assert",
            Self::Fallback => "fallback",
            Self::StepUp => "stepup",
        }
    }
}

/// Passkey-first authentication service.
#[derive(Clone, Debug)]
pub struct Passkeys {
    webauthn: Webauthn,
    config: AuthConfig,
}

impl Passkeys {
    /// Builds a production `WebAuthn` verifier. User verification and strict
    /// base64url are mandatory.
    pub fn new(config: AuthConfig) -> Result<Self, AuthError> {
        config.validate()?;
        let webauthn = Webauthn::new(&config.rp_id, &config.rp_name, &config.origin)
            .require_user_verification(true)
            .require_user_handle(true)
            .strict_base64(true);
        Ok(Self { webauthn, config })
    }

    /// Resolves the non-secret routing commitment carried by a session token
    /// against the authenticated tenancy map. The caller must still verify the
    /// token digest inside the returned principal scope before authorizing it.
    pub fn principal_for_token(
        &self,
        token: &str,
        tenancy: &TenancyMap,
    ) -> Result<PrincipalId, AuthError> {
        let route = token_principal_route(token)?;
        let mut matched = None;
        for principal in tenancy.principals() {
            let candidate = principal_route(&principal);
            if candidate.len() == route.len()
                && bool::from(candidate.as_bytes().ct_eq(route.as_bytes()))
            {
                if matched.replace(principal).is_some() {
                    return Err(AuthError::CorruptState);
                }
            }
        }
        matched.ok_or(AuthError::Unauthenticated)
    }

    /// Starts passkey registration for the authenticated application account.
    pub fn begin_registration(
        &self,
        scope: &mut PrincipalScope<'_>,
        account: &AccountIdentity,
        label: &str,
        now: u64,
    ) -> Result<RegistrationChallenge, AuthError> {
        self.check_rate_limit(scope, RatePurpose::Register, now)?;
        if !valid_display_text(label) {
            return Err(AuthError::InvalidInput("invalid passkey label"));
        }
        let existing = load_passkeys(scope)?;
        let credential_ids: Vec<CredentialId> = existing
            .iter()
            .map(|(_, passkey)| passkey.credential.id.clone())
            .collect();
        let (challenge, state) = self.webauthn.start_registration(
            &user_handle(scope),
            &account.username,
            &account.display_name,
            &credential_ids,
        );
        let registration_id = mint_identifier("reg_")?;
        let expires_at = now.saturating_add(self.config.ceremony_ttl_secs);
        let record = RegistrationRecord {
            state,
            label: label.to_owned(),
            expires_at,
        };
        put_json(
            scope,
            Table::Journeys,
            row_key(REGISTRATION_ROW_PREFIX, &registration_id)?,
            now,
            &record,
        )?;
        Ok(RegistrationChallenge {
            registration_id,
            ceremony: encode_opaque(&challenge)?,
            expires_at,
        })
    }

    /// Completes registration by verifying the browser's real `WebAuthn`
    /// attestation and storing only the resulting public credential.
    pub fn finish_registration(
        &self,
        scope: &mut PrincipalScope<'_>,
        registration_id: &str,
        credential: &str,
        now: u64,
    ) -> Result<PasskeyRecord, AuthError> {
        self.check_rate_limit(scope, RatePurpose::Register, now)?;
        if !valid_prefixed_id(registration_id, "reg_") {
            return Err(AuthError::InvalidInput("invalid registration identifier"));
        }
        let key = row_key(REGISTRATION_ROW_PREFIX, registration_id)?;
        let stored = get_json::<RegistrationRecord>(scope, Table::Journeys, &key)?
            .ok_or(AuthError::ChallengeNotFound)?;
        if now > stored.expires_at {
            scope.remove(Table::Journeys, &key)?;
            return Err(AuthError::ChallengeExpired);
        }
        let response: RegistrationResponse = decode_opaque(credential)?;
        let verified = self
            .webauthn
            .finish_registration(&stored.state, &response)
            .map_err(AuthError::Passkey)?;
        if load_passkeys(scope)?
            .iter()
            .any(|(_, passkey)| passkey.credential.id == verified.id)
        {
            return Err(AuthError::CredentialConflict);
        }
        let passkey_id = mint_identifier("pky_")?;
        let record = PasskeyRecord {
            passkey_id: passkey_id.clone(),
            label: stored.label,
            created_at: now,
            last_used_at: None,
        };
        put_json(
            scope,
            Table::Cache,
            row_key(PASSKEY_ROW_PREFIX, &passkey_id)?,
            now,
            &StoredPasskey {
                record: record.clone(),
                credential: verified,
            },
        )?;
        scope.remove(Table::Journeys, &key)?;
        Ok(record)
    }

    /// Lists safe passkey metadata for the authenticated principal.
    pub fn list_passkeys(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        now: u64,
    ) -> Result<Vec<PasskeyRecord>, AuthError> {
        let decision = self.authorize(
            scope,
            access_token,
            None,
            &AuthorizationRequest {
                operation: OperationClass::Read,
                digest: None,
                step_up: None,
                intended_destination: "/app/settings/security",
            },
            now,
        )?;
        if matches!(decision, AccessDecision::Reauthenticate { .. }) {
            return Err(AuthError::SessionExpired);
        }
        let mut passkeys = load_passkeys(scope)?
            .into_iter()
            .map(|(_, stored)| stored.record)
            .collect::<Vec<_>>();
        passkeys.sort_by(|left, right| left.passkey_id.cmp(&right.passkey_id));
        Ok(passkeys)
    }

    /// Starts an additional-passkey registration only after a fresh ceremony
    /// confirms this exact security mutation.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_security_registration(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        account: &AccountIdentity,
        label: &str,
        now: u64,
    ) -> Result<RegistrationChallenge, AuthError> {
        self.require_security_step_up(
            scope,
            access_token,
            csrf_token,
            operation_digest,
            step_up,
            now,
        )?;
        self.begin_registration(scope, account, label, now)
    }

    /// Finishes an additional-passkey registration under the same fresh,
    /// operation-bound evidence used to begin it.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_security_registration(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        registration_id: &str,
        credential: &str,
        now: u64,
    ) -> Result<PasskeyRecord, AuthError> {
        self.require_security_step_up(
            scope,
            access_token,
            csrf_token,
            operation_digest,
            step_up,
            now,
        )?;
        self.finish_registration(scope, registration_id, credential, now)
    }

    /// Revokes one passkey after fresh confirmation while refusing to remove
    /// the final credential that can perform future step-up ceremonies.
    #[allow(clippy::too_many_arguments)]
    pub fn revoke_passkey(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        passkey_id: &str,
        now: u64,
    ) -> Result<Vec<PasskeyRecord>, AuthError> {
        self.require_security_step_up(
            scope,
            access_token,
            csrf_token,
            operation_digest,
            step_up,
            now,
        )?;
        let passkeys = load_passkeys(scope)?;
        if passkeys.len() <= 1 {
            return Err(AuthError::LastPasskey);
        }
        let key = row_key(PASSKEY_ROW_PREFIX, passkey_id)?;
        if !passkeys.iter().any(|(candidate, _)| candidate == &key) {
            return Err(AuthError::CredentialNotFound);
        }
        scope.remove(Table::Cache, &key)?;
        let mut remaining = passkeys
            .into_iter()
            .filter(|(candidate, _)| candidate != &key)
            .map(|(_, stored)| stored.record)
            .collect::<Vec<_>>();
        remaining.sort_by(|left, right| left.passkey_id.cmp(&right.passkey_id));
        Ok(remaining)
    }

    /// Starts a username-resolved assertion scoped to this principal's
    /// registered passkeys.
    pub fn begin_assertion(
        &self,
        scope: &mut PrincipalScope<'_>,
        now: u64,
    ) -> Result<AssertionChallenge, AuthError> {
        self.check_rate_limit(scope, RatePurpose::Assert, now)?;
        let passkeys = load_passkeys(scope)?;
        if passkeys.is_empty() {
            return Err(AuthError::NoPasskeys);
        }
        let credentials: Vec<PasskeyCredential> = passkeys
            .iter()
            .map(|(_, passkey)| passkey.credential.clone())
            .collect();
        let (challenge, state) = self
            .webauthn
            .start_authentication_with_creds_for_user(&user_handle(scope), &credentials);
        let assertion_id = mint_identifier("asr_")?;
        let expires_at = now.saturating_add(self.config.ceremony_ttl_secs);
        put_json(
            scope,
            Table::Journeys,
            row_key(ASSERTION_ROW_PREFIX, &assertion_id)?,
            now,
            &AssertionRecord {
                state,
                candidate_passkeys: passkeys
                    .iter()
                    .map(|(_, passkey)| passkey.record.passkey_id.clone())
                    .collect(),
                expires_at,
            },
        )?;
        Ok(AssertionChallenge {
            assertion_id,
            ceremony: encode_opaque(&challenge)?,
            expires_at,
        })
    }

    /// Verifies a signed `WebAuthn` assertion, advances its authenticator
    /// counter and returns a one-use session-opening proof.
    pub fn finish_assertion(
        &self,
        scope: &mut PrincipalScope<'_>,
        assertion_id: &str,
        credential: &str,
        now: u64,
    ) -> Result<AssertionProof, AuthError> {
        self.check_rate_limit(scope, RatePurpose::Assert, now)?;
        if !valid_prefixed_id(assertion_id, "asr_") {
            return Err(AuthError::InvalidInput("invalid assertion identifier"));
        }
        let challenge_key = row_key(ASSERTION_ROW_PREFIX, assertion_id)?;
        let challenge = get_json::<AssertionRecord>(scope, Table::Journeys, &challenge_key)?
            .ok_or(AuthError::ChallengeNotFound)?;
        if now > challenge.expires_at {
            scope.remove(Table::Journeys, &challenge_key)?;
            return Err(AuthError::ChallengeExpired);
        }
        let response: AuthenticationResponse = decode_opaque(credential)?;
        let (passkey_key, mut stored) =
            find_candidate_passkey(scope, &challenge.candidate_passkeys, &response.id)?;
        let outcome = self
            .webauthn
            .finish_authentication(&challenge.state, &response, &stored.credential)
            .map_err(AuthError::Passkey)?;
        if !outcome.user_verified {
            return Err(AuthError::Passkey(passkey_auth::Error::UserNotVerified));
        }
        stored.credential.counter = outcome.new_counter;
        stored.record.last_used_at = Some(now);
        put_json(scope, Table::Cache, passkey_key, now, &stored)?;
        scope.remove(Table::Journeys, &challenge_key)?;
        let expires_at = now.saturating_add(self.config.assertion_ttl_secs);
        put_json(
            scope,
            Table::Cache,
            row_key(VERIFIED_ASSERTION_ROW_PREFIX, assertion_id)?,
            now,
            &VerifiedAssertion {
                passkey_id: stored.record.passkey_id.clone(),
                expires_at,
                consumed: false,
            },
        )?;
        Ok(AssertionProof {
            assertion_id: assertion_id.to_owned(),
            passkey_id: stored.record.passkey_id,
            expires_at,
        })
    }

    /// Consumes one verified assertion and issues a principal-bound browser
    /// session with separately rotated access, refresh and CSRF secrets.
    pub fn open_session(
        &self,
        scope: &mut PrincipalScope<'_>,
        assertion_id: &str,
        device: Device,
        now: u64,
    ) -> Result<SessionGrant, AuthError> {
        let key = row_key(VERIFIED_ASSERTION_ROW_PREFIX, assertion_id)?;
        let mut assertion = get_json::<VerifiedAssertion>(scope, Table::Cache, &key)?
            .ok_or(AuthError::AssertionNotVerified)?;
        if assertion.consumed {
            return Err(AuthError::AssertionSpent);
        }
        if now > assertion.expires_at {
            return Err(AuthError::ChallengeExpired);
        }
        assertion.consumed = true;
        put_json(scope, Table::Cache, key, now, &assertion)?;
        self.issue_session(scope, Assurance::Passkey, device, now)
    }

    /// Rotates a valid refresh generation. Revoked, signed-out and expired
    /// sessions cannot refresh.
    pub fn refresh_session(
        &self,
        scope: &mut PrincipalScope<'_>,
        refresh_token: &str,
        csrf_token: &str,
        now: u64,
    ) -> Result<SessionGrant, AuthError> {
        let session_id = token_session_id(refresh_token)?;
        let key = row_key(SESSION_ROW_PREFIX, session_id)?;
        let mut record = get_json::<SessionRecord>(scope, Table::Cache, &key)?
            .ok_or(AuthError::Unauthenticated)?;
        let epoch = session_epoch(scope)?;
        if record.revoked_at.is_some() || record.epoch != epoch {
            return Err(AuthError::Unauthenticated);
        }
        if now > record.refresh_expires_at {
            return Err(AuthError::SessionExpired);
        }
        if !digest_matches(&record.refresh_digest, refresh_token)
            || !digest_matches(&record.csrf_digest, csrf_token)
        {
            return Err(AuthError::ForgeryRefused);
        }
        let grant = rotate_session_secrets(&mut record, &self.config, scope.principal(), now)?;
        put_json(scope, Table::Cache, key, now, &record)?;
        touch_device(scope, &record.device, now)?;
        Ok(grant)
    }

    /// Validates a refresh generation without rotating it. The caller must
    /// consume the same token through [`Self::refresh_session`] when executing
    /// the refresh operation.
    pub fn authorize_refresh(
        &self,
        scope: &PrincipalScope<'_>,
        refresh_token: &str,
        csrf_token: &str,
        now: u64,
    ) -> Result<SessionContext, AuthError> {
        let session_id = token_session_id(refresh_token)?;
        let key = row_key(SESSION_ROW_PREFIX, session_id)?;
        let record = get_json::<SessionRecord>(scope, Table::Cache, &key)?
            .ok_or(AuthError::Unauthenticated)?;
        if record.revoked_at.is_some()
            || record.epoch != session_epoch(scope)?
            || now > record.refresh_expires_at
            || !digest_matches(&record.refresh_digest, refresh_token)
            || !digest_matches(&record.csrf_digest, csrf_token)
        {
            return Err(AuthError::Unauthenticated);
        }
        Ok(SessionContext {
            session_id: record.session_id,
            device_id: record.device.id,
        })
    }

    /// Authorizes one browser request, returning an explicit re-authentication
    /// decision with its intended destination when the session expired.
    #[allow(clippy::unused_self)]
    pub fn authorize(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: Option<&str>,
        request: &AuthorizationRequest<'_>,
        now: u64,
    ) -> Result<AccessDecision, AuthError> {
        let (key, mut record) = load_session(scope, access_token)?;
        let epoch = session_epoch(scope)?;
        if record.revoked_at.is_some()
            || record.epoch != epoch
            || !digest_matches(&record.access_digest, access_token)
        {
            return Err(AuthError::Unauthenticated);
        }
        if now > record.access_expires_at {
            return Ok(AccessDecision::Reauthenticate {
                intended_destination: request.intended_destination.to_owned(),
            });
        }
        if request.operation.mutates()
            && !csrf_token.is_some_and(|token| digest_matches(&record.csrf_digest, token))
        {
            return Err(AuthError::ForgeryRefused);
        }
        if record.assurance == Assurance::FallbackRestricted && request.operation.mutates() {
            return Err(AuthError::FallbackRestricted);
        }
        if request.operation.requires_step_up() {
            let digest = request.digest.ok_or(AuthError::StepUpRequired)?;
            let evidence = request.step_up.ok_or(AuthError::StepUpRequired)?;
            Self::validate_step_up(scope, evidence, digest, now)?;
        }
        record.last_active_at = now;
        let context = SessionContext {
            session_id: record.session_id.clone(),
            device_id: record.device.id.clone(),
        };
        put_json(scope, Table::Cache, key, now, &record)?;
        touch_device(scope, &record.device, now)?;
        Ok(AccessDecision::Authorized(context))
    }

    /// Lists this principal's sessions after authenticating the caller.
    pub fn list_sessions(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        now: u64,
    ) -> Result<Vec<SessionView>, AuthError> {
        let current_id = token_session_id(access_token)?.to_owned();
        let decision = self.authorize(
            scope,
            access_token,
            None,
            &AuthorizationRequest {
                operation: OperationClass::Read,
                digest: None,
                step_up: None,
                intended_destination: "/settings/sessions",
            },
            now,
        )?;
        if matches!(decision, AccessDecision::Reauthenticate { .. }) {
            return Err(AuthError::SessionExpired);
        }
        let epoch = session_epoch(scope)?;
        let mut sessions = Vec::new();
        for key in scope.keys(Table::Cache) {
            if !key.as_str().starts_with(SESSION_ROW_PREFIX) {
                continue;
            }
            let Some(record) = get_json::<SessionRecord>(scope, Table::Cache, &key)? else {
                continue;
            };
            if record.revoked_at.is_none() && record.epoch == epoch {
                sessions.push(SessionView {
                    current: record.session_id == current_id,
                    session_id: record.session_id,
                    device: record.device,
                    opened_at: record.opened_at,
                    last_active_at: record.last_active_at,
                    restricted: record.assurance == Assurance::FallbackRestricted,
                });
            }
        }
        sessions.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        Ok(sessions)
    }

    /// Revokes one session after authenticating the caller and validating the
    /// browser anti-forgery token.
    #[allow(clippy::unused_self)]
    pub fn revoke_session(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        target_session_id: &str,
        now: u64,
    ) -> Result<SessionRevocation, AuthError> {
        Self::authorize_base_mutation(scope, access_token, csrf_token, now)?;
        let key = row_key(SESSION_ROW_PREFIX, target_session_id)?;
        let mut record = get_json::<SessionRecord>(scope, Table::Cache, &key)?
            .ok_or(AuthError::SessionNotFound)?;
        record.revoked_at = Some(now);
        put_json(scope, Table::Cache, key, now, &record)?;
        Ok(SessionRevocation {
            revoked_session_ids: vec![target_session_id.to_owned()],
            revoked_at: now,
        })
    }

    /// Invalidates every access and refresh path for this principal with one
    /// durable epoch increment.
    #[allow(clippy::unused_self)]
    pub fn sign_out_everywhere(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        now: u64,
    ) -> Result<SessionRevocation, AuthError> {
        Self::authorize_base_mutation(scope, access_token, csrf_token, now)?;
        let current = session_epoch(scope)?;
        let next = current.checked_add(1).ok_or(AuthError::SizeOverflow)?;
        put_json(
            scope,
            Table::Cache,
            RowKey::new(SESSION_EPOCH_ROW)?,
            now,
            &SessionEpoch { value: next },
        )?;
        let mut revoked = Vec::new();
        for key in scope.keys(Table::Cache) {
            if !key.as_str().starts_with(SESSION_ROW_PREFIX) {
                continue;
            }
            if let Some(record) = get_json::<SessionRecord>(scope, Table::Cache, &key)? {
                if record.revoked_at.is_none() && record.epoch == current {
                    revoked.push(record.session_id);
                }
            }
        }
        revoked.sort();
        Ok(SessionRevocation {
            revoked_session_ids: revoked,
            revoked_at: now,
        })
    }

    /// Revokes one browser session only after a fresh passkey ceremony bound
    /// to the exact target session.
    #[allow(clippy::too_many_arguments)]
    pub fn revoke_session_with_step_up(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        target_session_id: &str,
        now: u64,
    ) -> Result<SessionRevocation, AuthError> {
        self.require_security_step_up(
            scope,
            access_token,
            csrf_token,
            operation_digest,
            step_up,
            now,
        )?;
        self.revoke_session(scope, access_token, csrf_token, target_session_id, now)
    }

    /// Invalidates every access and refresh path after fresh confirmation of
    /// the sign-out-everywhere operation.
    #[allow(clippy::too_many_arguments)]
    pub fn sign_out_everywhere_with_step_up(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        now: u64,
    ) -> Result<SessionRevocation, AuthError> {
        self.require_security_step_up(
            scope,
            access_token,
            csrf_token,
            operation_digest,
            step_up,
            now,
        )?;
        self.sign_out_everywhere(scope, access_token, csrf_token, now)
    }

    /// Replaces the one-time fallback credential. A passkey-authenticated
    /// session and fresh step-up evidence for this exact security mutation
    /// are both required.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_fallback_credential(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        now: u64,
        valid_for_secs: u64,
    ) -> Result<FallbackCredential, AuthError> {
        if valid_for_secs == 0 {
            return Err(AuthError::InvalidInput("invalid fallback lifetime"));
        }
        let decision = self.authorize(
            scope,
            access_token,
            Some(csrf_token),
            &AuthorizationRequest {
                operation: OperationClass::SecuritySettings,
                digest: Some(operation_digest),
                step_up: Some(step_up),
                intended_destination: "/settings/security",
            },
            now,
        )?;
        if matches!(decision, AccessDecision::Reauthenticate { .. }) {
            return Err(AuthError::SessionExpired);
        }
        let mut raw = [0_u8; SECRET_BYTES];
        let mut salt = [0_u8; SECRET_BYTES];
        fill_random(&mut raw)?;
        fill_random(&mut salt)?;
        let secret_text = URL_SAFE_NO_PAD.encode(raw);
        let digest = fallback_digest(&salt, &raw);
        raw.zeroize();
        let record = FallbackRecord {
            salt,
            digest,
            expires_at: now.saturating_add(valid_for_secs),
            consumed_at: None,
        };
        put_json(
            scope,
            Table::Cache,
            RowKey::new(FALLBACK_ROW)?,
            now,
            &record,
        )?;
        Ok(FallbackCredential {
            secret: OpaqueSecret(Zeroizing::new(secret_text)),
            expires_at: record.expires_at,
        })
    }

    /// Consumes a one-time fallback credential and issues a restricted
    /// read-only recovery session. It never calls the passkey assertion path,
    /// never creates step-up evidence and cannot authorize a mutation.
    pub fn authenticate_fallback(
        &self,
        scope: &mut PrincipalScope<'_>,
        secret: &str,
        device: Device,
        now: u64,
    ) -> Result<SessionGrant, AuthError> {
        self.check_rate_limit(scope, RatePurpose::Fallback, now)?;
        let key = RowKey::new(FALLBACK_ROW)?;
        let mut record = get_json::<FallbackRecord>(scope, Table::Cache, &key)?
            .ok_or(AuthError::FallbackRefused)?;
        if record.consumed_at.is_some() || now > record.expires_at {
            return Err(AuthError::FallbackRefused);
        }
        let mut raw = URL_SAFE_NO_PAD
            .decode(secret)
            .map_err(|_| AuthError::FallbackRefused)?;
        let candidate = fallback_digest(&record.salt, &raw);
        raw.zeroize();
        if !bool::from(record.digest.ct_eq(&candidate)) {
            return Err(AuthError::FallbackRefused);
        }
        record.consumed_at = Some(now);
        put_json(scope, Table::Cache, key, now, &record)?;
        self.issue_session(scope, Assurance::FallbackRestricted, device, now)
    }

    fn authorize_base_mutation(
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        now: u64,
    ) -> Result<SessionContext, AuthError> {
        let (key, mut record) = load_session(scope, access_token)?;
        if record.revoked_at.is_some()
            || record.epoch != session_epoch(scope)?
            || now > record.access_expires_at
            || !digest_matches(&record.access_digest, access_token)
            || !digest_matches(&record.csrf_digest, csrf_token)
        {
            return Err(AuthError::Unauthenticated);
        }
        if record.assurance != Assurance::Passkey {
            return Err(AuthError::FallbackRestricted);
        }
        record.last_active_at = now;
        let context = SessionContext {
            session_id: record.session_id.clone(),
            device_id: record.device.id.clone(),
        };
        put_json(scope, Table::Cache, key, now, &record)?;
        touch_device(scope, &record.device, now)?;
        Ok(context)
    }

    fn require_security_step_up(
        &self,
        scope: &mut PrincipalScope<'_>,
        access_token: &str,
        csrf_token: &str,
        operation_digest: OperationDigest,
        step_up: &StepUpEvidence,
        now: u64,
    ) -> Result<(), AuthError> {
        let decision = self.authorize(
            scope,
            access_token,
            Some(csrf_token),
            &AuthorizationRequest {
                operation: OperationClass::SecuritySettings,
                digest: Some(operation_digest),
                step_up: Some(step_up),
                intended_destination: "/app/settings/security",
            },
            now,
        )?;
        if matches!(decision, AccessDecision::Reauthenticate { .. }) {
            return Err(AuthError::SessionExpired);
        }
        Ok(())
    }

    fn issue_session(
        &self,
        scope: &mut PrincipalScope<'_>,
        assurance: Assurance,
        device: Device,
        now: u64,
    ) -> Result<SessionGrant, AuthError> {
        record_device(scope, &device, now)?;
        let epoch = session_epoch(scope)?;
        let session_id = mint_identifier("ses_")?;
        let (access_token, access_digest) = mint_token(&session_id, scope.principal())?;
        let (refresh_token, refresh_digest) = mint_token(&session_id, scope.principal())?;
        let csrf_token = mint_secret()?;
        let csrf_digest = token_digest(csrf_token.expose());
        let access_expires_at = now.saturating_add(self.config.session_ttl_secs);
        let refresh_expires_at = now.saturating_add(self.config.refresh_ttl_secs);
        let record = SessionRecord {
            session_id: session_id.clone(),
            device,
            opened_at: now,
            last_active_at: now,
            access_expires_at,
            refresh_expires_at,
            access_digest,
            refresh_digest,
            csrf_digest,
            epoch,
            revoked_at: None,
            assurance,
        };
        put_json(
            scope,
            Table::Cache,
            row_key(SESSION_ROW_PREFIX, &session_id)?,
            now,
            &record,
        )?;
        Ok(SessionGrant {
            session_id,
            access_token,
            refresh_token,
            csrf_token,
            access_expires_at,
            refresh_expires_at,
        })
    }

    fn check_rate_limit(
        &self,
        scope: &mut PrincipalScope<'_>,
        purpose: RatePurpose,
        now: u64,
    ) -> Result<(), AuthError> {
        let key = RowKey::new(format!("{RATE_ROW_PREFIX}{}", purpose.label()))?;
        let mut window = get_json::<RateWindow>(scope, Table::Cache, &key)?.unwrap_or(RateWindow {
            started_at: now,
            attempts: 0,
        });
        if now.saturating_sub(window.started_at) >= self.config.rate_limit.window_secs {
            window = RateWindow {
                started_at: now,
                attempts: 0,
            };
        }
        if window.attempts >= self.config.rate_limit.attempts {
            let retry_at = window
                .started_at
                .saturating_add(self.config.rate_limit.window_secs);
            return Err(AuthError::RateLimited {
                retry_at,
                retry_after_secs: retry_at.saturating_sub(now),
            });
        }
        window.attempts = window.attempts.saturating_add(1);
        put_json(scope, Table::Cache, key, now, &window)
    }
}

fn valid_display_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= TEXT_LIMIT && !value.chars().any(char::is_control)
}

fn valid_machine_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

fn valid_prefixed_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == IDENTIFIER_ENTROPY_BYTES * 2
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn fill_random(bytes: &mut [u8]) -> Result<(), AuthError> {
    getrandom::fill(bytes).map_err(|_| AuthError::EntropyUnavailable)
}

fn mint_identifier(prefix: &str) -> Result<String, AuthError> {
    let mut random = [0_u8; IDENTIFIER_ENTROPY_BYTES];
    fill_random(&mut random)?;
    let mut identifier = String::with_capacity(prefix.len() + random.len() * 2);
    identifier.push_str(prefix);
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut identifier, "{byte:02x}").map_err(|_| AuthError::SizeOverflow)?;
    }
    Ok(identifier)
}

fn mint_secret() -> Result<OpaqueSecret, AuthError> {
    let mut random = [0_u8; SECRET_BYTES];
    fill_random(&mut random)?;
    let encoded = URL_SAFE_NO_PAD.encode(random);
    random.zeroize();
    Ok(OpaqueSecret(Zeroizing::new(encoded)))
}

fn mint_token(
    session_id: &str,
    principal: &PrincipalId,
) -> Result<(OpaqueSecret, [u8; 32]), AuthError> {
    let secret = mint_secret()?;
    let route = principal_route(principal);
    let token = OpaqueSecret(Zeroizing::new(format!(
        "{session_id}.{route}.{}",
        secret.expose()
    )));
    let digest = token_digest(token.expose());
    Ok((token, digest))
}

fn token_session_id(token: &str) -> Result<&str, AuthError> {
    let mut fields = token.split('.');
    let session_id = fields.next().ok_or(AuthError::Unauthenticated)?;
    let route = fields.next().ok_or(AuthError::Unauthenticated)?;
    let secret = fields.next().ok_or(AuthError::Unauthenticated)?;
    if fields.next().is_some()
        || !valid_prefixed_id(session_id, "ses_")
        || !valid_token_route(route)
        || secret.is_empty()
    {
        return Err(AuthError::Unauthenticated);
    }
    Ok(session_id)
}

fn token_principal_route(token: &str) -> Result<&str, AuthError> {
    let mut fields = token.split('.');
    let session_id = fields.next().ok_or(AuthError::Unauthenticated)?;
    let route = fields.next().ok_or(AuthError::Unauthenticated)?;
    let secret = fields.next().ok_or(AuthError::Unauthenticated)?;
    if fields.next().is_some()
        || !valid_prefixed_id(session_id, "ses_")
        || !valid_token_route(route)
        || secret.is_empty()
    {
        return Err(AuthError::Unauthenticated);
    }
    Ok(route)
}

fn valid_token_route(route: &str) -> bool {
    route.len() == 43
        && route.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
        })
}

fn principal_route(principal: &PrincipalId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(TOKEN_ROUTE_DOMAIN);
    hasher.update(principal.as_str().as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn token_digest(token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(TOKEN_DOMAIN);
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

fn digest_matches(expected: &[u8; 32], token: &str) -> bool {
    bool::from(expected.ct_eq(&token_digest(token)))
}

fn fallback_digest(salt: &[u8; 32], secret: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(FALLBACK_DOMAIN);
    hasher.update(salt);
    hasher.update(secret);
    hasher.finalize().into()
}

fn user_handle(scope: &PrincipalScope<'_>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(USER_HANDLE_DOMAIN);
    hasher.update(scope.principal().as_str().as_bytes());
    hasher.finalize().into()
}

fn row_key(prefix: &str, identifier: &str) -> Result<RowKey, AuthError> {
    let suffix = identifier
        .split_once('_')
        .map(|(_, suffix)| suffix)
        .ok_or(AuthError::InvalidInput("invalid identifier"))?;
    Ok(RowKey::new(format!("{prefix}{suffix}"))?)
}

fn encode_opaque<T: Serialize>(value: &T) -> Result<String, AuthError> {
    let json = serde_json::to_vec(value).map_err(|_| AuthError::Encoding)?;
    Ok(URL_SAFE_NO_PAD.encode(json))
}

fn decode_opaque<T: DeserializeOwned>(value: &str) -> Result<T, AuthError> {
    let json = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AuthError::MalformedCredential)?;
    serde_json::from_slice(&json).map_err(|_| AuthError::MalformedCredential)
}

fn put_json<T: Serialize>(
    scope: &mut PrincipalScope<'_>,
    table: Table,
    key: RowKey,
    now: u64,
    value: &T,
) -> Result<(), AuthError> {
    let bytes = serde_json::to_vec(value).map_err(|_| AuthError::Encoding)?;
    scope.put(table, key, now, bytes)?;
    Ok(())
}

fn get_json<T: DeserializeOwned>(
    scope: &PrincipalScope<'_>,
    table: Table,
    key: &RowKey,
) -> Result<Option<T>, AuthError> {
    let Some(row) = scope.get(table, key) else {
        return Ok(None);
    };
    serde_json::from_slice(row.bytes())
        .map(Some)
        .map_err(|_| AuthError::CorruptState)
}

fn load_passkeys(scope: &PrincipalScope<'_>) -> Result<Vec<(RowKey, StoredPasskey)>, AuthError> {
    let mut passkeys = Vec::new();
    for key in scope.keys(Table::Cache) {
        if key.as_str().starts_with(PASSKEY_ROW_PREFIX) {
            if let Some(passkey) = get_json(scope, Table::Cache, &key)? {
                passkeys.push((key, passkey));
            }
        }
    }
    Ok(passkeys)
}

fn find_candidate_passkey(
    scope: &PrincipalScope<'_>,
    candidates: &[String],
    response_id: &str,
) -> Result<(RowKey, StoredPasskey), AuthError> {
    for passkey_id in candidates {
        let key = row_key(PASSKEY_ROW_PREFIX, passkey_id)?;
        if let Some(passkey) = get_json::<StoredPasskey>(scope, Table::Cache, &key)? {
            if passkey.credential.id.to_b64url() == response_id {
                return Ok((key, passkey));
            }
        }
    }
    Err(AuthError::CredentialNotFound)
}

fn session_epoch(scope: &PrincipalScope<'_>) -> Result<u64, AuthError> {
    Ok(
        get_json::<SessionEpoch>(scope, Table::Cache, &RowKey::new(SESSION_EPOCH_ROW)?)?
            .unwrap_or_default()
            .value,
    )
}

fn load_session(
    scope: &PrincipalScope<'_>,
    token: &str,
) -> Result<(RowKey, SessionRecord), AuthError> {
    let session_id = token_session_id(token)?;
    let key = row_key(SESSION_ROW_PREFIX, session_id)?;
    let record = get_json(scope, Table::Cache, &key)?.ok_or(AuthError::Unauthenticated)?;
    Ok((key, record))
}

fn rotate_session_secrets(
    record: &mut SessionRecord,
    config: &AuthConfig,
    principal: &PrincipalId,
    now: u64,
) -> Result<SessionGrant, AuthError> {
    let (access_token, access_digest) = mint_token(&record.session_id, principal)?;
    let (refresh_token, refresh_digest) = mint_token(&record.session_id, principal)?;
    let csrf_token = mint_secret()?;
    record.access_digest = access_digest;
    record.refresh_digest = refresh_digest;
    record.csrf_digest = token_digest(csrf_token.expose());
    record.last_active_at = now;
    record.access_expires_at = now.saturating_add(config.session_ttl_secs);
    Ok(SessionGrant {
        session_id: record.session_id.clone(),
        access_token,
        refresh_token,
        csrf_token,
        access_expires_at: record.access_expires_at,
        refresh_expires_at: record.refresh_expires_at,
    })
}

fn device_key(device_id: &str) -> Result<RowKey, AuthError> {
    row_key(DEVICE_ROW_PREFIX, device_id)
}

fn record_device(
    scope: &mut PrincipalScope<'_>,
    device: &Device,
    now: u64,
) -> Result<(), AuthError> {
    let key = device_key(&device.id)?;
    let existing = get_json::<DeviceRecord>(scope, Table::Cache, &key)?;
    let first_seen_at = existing.as_ref().map_or(now, |record| record.first_seen_at);
    put_json(
        scope,
        Table::Cache,
        key,
        now,
        &DeviceRecord {
            device: device.clone(),
            first_seen_at,
            last_active_at: now,
        },
    )?;
    if existing.is_none() {
        let suffix = device
            .id
            .strip_prefix("dev_")
            .ok_or(AuthError::CorruptState)?;
        put_json(
            scope,
            Table::Notifications,
            RowKey::new(format!("{NEW_DEVICE_ROW_PREFIX}{suffix}"))?,
            now,
            &NewDeviceNotification {
                notification_id: format!("ntf_{suffix}"),
                class: "security",
                title_copy_key: "notification.new-device.title",
                body_copy_key: "notification.new-device.body",
                deep_link: "/app/settings/devices",
                action_copy_key: "notification.action.review-devices",
                device_id: device.id.clone(),
                read: false,
                created_at: now,
            },
        )?;
    }
    Ok(())
}

fn touch_device(
    scope: &mut PrincipalScope<'_>,
    device: &Device,
    now: u64,
) -> Result<(), AuthError> {
    let key = device_key(&device.id)?;
    let mut record =
        get_json::<DeviceRecord>(scope, Table::Cache, &key)?.ok_or(AuthError::CorruptState)?;
    record.last_active_at = now;
    put_json(scope, Table::Cache, key, now, &record)
}

/// Authentication refusal taxonomy. No variant contains credentials, tokens,
/// personal data or `WebAuthn` response bytes.
pub enum AuthError {
    InvalidConfiguration,
    InvalidInput(&'static str),
    EntropyUnavailable,
    Encoding,
    CorruptState,
    MalformedCredential,
    Passkey(passkey_auth::Error),
    Store(StoreError),
    ChallengeNotFound,
    ChallengeExpired,
    CredentialConflict,
    CredentialNotFound,
    NoPasskeys,
    LastPasskey,
    AssertionNotVerified,
    AssertionSpent,
    Unauthenticated,
    SessionNotFound,
    SessionExpired,
    ForgeryRefused,
    FallbackRefused,
    FallbackRestricted,
    StepUpRequired,
    StepUpMismatch,
    StepUpExpired,
    RateLimited {
        retry_at: u64,
        retry_after_secs: u64,
    },
    SizeOverflow,
}

impl std::fmt::Debug for AuthError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(self, formatter)
    }
}

impl Display for AuthError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfiguration => {
                formatter.write_str("invalid authentication configuration")
            }
            Self::InvalidInput(reason) => {
                write!(formatter, "invalid authentication input: {reason}")
            }
            Self::EntropyUnavailable => formatter.write_str("cryptographic entropy is unavailable"),
            Self::Encoding => formatter.write_str("authentication state encoding failed"),
            Self::CorruptState => formatter.write_str("authentication state is corrupt"),
            Self::MalformedCredential => formatter.write_str("credential response is malformed"),
            Self::Passkey(_) => formatter.write_str("passkey ceremony was refused"),
            Self::Store(error) => write!(formatter, "authentication store failure: {error}"),
            Self::ChallengeNotFound => {
                formatter.write_str("authentication challenge does not exist")
            }
            Self::ChallengeExpired => formatter.write_str("authentication challenge expired"),
            Self::CredentialConflict => formatter.write_str("passkey is already registered"),
            Self::CredentialNotFound => {
                formatter.write_str("passkey does not belong to this principal")
            }
            Self::NoPasskeys => formatter.write_str("principal has no registered passkey"),
            Self::LastPasskey => formatter.write_str("the final passkey cannot be removed"),
            Self::AssertionNotVerified => formatter.write_str("assertion has not been verified"),
            Self::AssertionSpent => formatter.write_str("assertion already opened a session"),
            Self::Unauthenticated => formatter.write_str("session is not authenticated"),
            Self::SessionNotFound => formatter.write_str("session does not exist"),
            Self::SessionExpired => {
                formatter.write_str("session expired; re-authentication is required")
            }
            Self::ForgeryRefused => formatter.write_str("browser anti-forgery validation failed"),
            Self::FallbackRefused => formatter.write_str("fallback credential was refused"),
            Self::FallbackRestricted => {
                formatter.write_str("fallback sessions cannot perform this operation")
            }
            Self::StepUpRequired => formatter.write_str("fresh passkey step-up is required"),
            Self::StepUpMismatch => {
                formatter.write_str("step-up evidence confirms another operation")
            }
            Self::StepUpExpired => formatter.write_str("step-up evidence expired"),
            Self::RateLimited {
                retry_after_secs, ..
            } => write!(
                formatter,
                "authentication rate limit exceeded; retry in {retry_after_secs} seconds"
            ),
            Self::SizeOverflow => {
                formatter.write_str("authentication state exceeds supported bounds")
            }
        }
    }
}

impl std::error::Error for AuthError {}

impl From<StoreError> for AuthError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
