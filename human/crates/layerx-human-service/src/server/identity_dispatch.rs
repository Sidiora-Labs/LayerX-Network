//! Principal-scoped identity projections used by the production dispatcher.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest as _;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use crate::auth::{Device, OperationDigest, StepUpChallenge, StepUpEvidence};
use crate::onboarding::{
    EvidenceClass, EvidenceVerification, OnboardingStage, OnboardingState, OnboardingStatus,
    StageState,
};
use crate::onboarding::{OnboardingStart, RecoveryPolicy};
use crate::security::{security_action_digest, SecurityAction};
use crate::store::{PrincipalId, PrincipalScope, RowKey, StoreError, Table};
use layerx_types::ids::Did;
use layerx_types::intent::{ApprovalThreshold, RecoveryRoot};

const PROFILE_ROW: &str = "identity-profile";
const IDENTITY_MAGIC: &[u8; 4] = b"LXIP";

#[derive(Clone, Debug)]
pub struct IdentityProviderConfig {
    pub socket: PathBuf,
    pub deadline: Duration,
    pub maximum_frame_bytes: usize,
    pub peer_uid: u32,
    pub peer_gid: u32,
}

/// Privileged account and device metadata boundary. The provider is the only
/// component allowed to provision a pre-authorized principal or attest a
/// device; this process never derives either value from user-controlled text.
pub struct RemoteIdentityProvider {
    config: IdentityProviderConfig,
}

pub struct ProvisionedAccount {
    pub principal: PrincipalId,
    pub onboarding: OnboardingStart,
}

impl RemoteIdentityProvider {
    pub fn new(config: IdentityProviderConfig) -> Result<Self, IdentityDispatchError> {
        if !config.socket.is_absolute()
            || config.deadline.is_zero()
            || !(64..=1_048_576).contains(&config.maximum_frame_bytes)
        {
            return Err(IdentityDispatchError::InvalidConfiguration);
        }
        Ok(Self { config })
    }

    pub fn provision(
        &self,
        email: &str,
        display_name: &str,
        idempotency_key: &str,
        now: u64,
    ) -> Result<ProvisionedAccount, IdentityDispatchError> {
        let fields = self.call(
            1,
            &[
                email.as_bytes(),
                display_name.as_bytes(),
                idempotency_key.as_bytes(),
                &now.to_be_bytes(),
            ],
        )?;
        if fields.len() != 5 {
            return Err(IdentityDispatchError::ProviderEvidence);
        }
        let principal = PrincipalId::new(provider_text(&fields[0])?)?;
        let did = Did::new(&fields[1]).map_err(|_| IdentityDispatchError::ProviderEvidence)?;
        let root: [u8; 32] = fields[2]
            .as_slice()
            .try_into()
            .map_err(|_| IdentityDispatchError::ProviderEvidence)?;
        let threshold = u16::from_be_bytes(
            fields[3]
                .as_slice()
                .try_into()
                .map_err(|_| IdentityDispatchError::ProviderEvidence)?,
        );
        let delay = u64::from_be_bytes(
            fields[4]
                .as_slice()
                .try_into()
                .map_err(|_| IdentityDispatchError::ProviderEvidence)?,
        );
        let recovery = RecoveryPolicy::new(
            RecoveryRoot::new(root),
            ApprovalThreshold::new(threshold)
                .map_err(|_| IdentityDispatchError::ProviderEvidence)?,
            delay,
        )
        .map_err(|_| IdentityDispatchError::ProviderEvidence)?;
        let key: [u8; 32] = sha2::Sha256::digest(idempotency_key.as_bytes()).into();
        let onboarding = OnboardingStart::new(key, did, recovery)
            .map_err(|_| IdentityDispatchError::ProviderEvidence)?;
        Ok(ProvisionedAccount {
            principal,
            onboarding,
        })
    }

    pub fn resolve_email(&self, email: &str) -> Result<PrincipalId, IdentityDispatchError> {
        let fields = self.call(2, &[email.as_bytes()])?;
        if fields.len() != 1 {
            return Err(IdentityDispatchError::ProviderEvidence);
        }
        PrincipalId::new(provider_text(&fields[0])?).map_err(Into::into)
    }

    pub fn device_for_assertion(
        &self,
        principal: &PrincipalId,
        assertion_id: &str,
    ) -> Result<Device, IdentityDispatchError> {
        let fields = self.call(3, &[principal.as_str().as_bytes(), assertion_id.as_bytes()])?;
        if fields.len() != 3 {
            return Err(IdentityDispatchError::ProviderEvidence);
        }
        Device::new(
            provider_text(&fields[0])?,
            provider_text(&fields[1])?,
            provider_text(&fields[2])?,
        )
        .map_err(|_| IdentityDispatchError::ProviderEvidence)
    }

    pub fn probe(&self) -> Result<(), IdentityDispatchError> {
        if self.call(0, &[])?.is_empty() {
            Ok(())
        } else {
            Err(IdentityDispatchError::ProviderEvidence)
        }
    }

    fn call(&self, operation: u8, fields: &[&[u8]]) -> Result<Vec<Vec<u8>>, IdentityDispatchError> {
        let metadata = std::fs::symlink_metadata(&self.config.socket)
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _};
        if !metadata.file_type().is_socket()
            || metadata.uid() != self.config.peer_uid
            || metadata.gid() != self.config.peer_gid
            || metadata.mode() & 0o007 != 0
        {
            return Err(IdentityDispatchError::ProviderAuthentication);
        }
        let mut request = Vec::new();
        request.extend_from_slice(IDENTITY_MAGIC);
        request.push(1);
        request.push(operation);
        request.extend_from_slice(
            &u32::try_from(fields.len())
                .map_err(|_| IdentityDispatchError::InvalidInput)?
                .to_be_bytes(),
        );
        for field in fields {
            request.extend_from_slice(
                &u32::try_from(field.len())
                    .map_err(|_| IdentityDispatchError::InvalidInput)?
                    .to_be_bytes(),
            );
            request.extend_from_slice(field);
        }
        if request.len() > self.config.maximum_frame_bytes {
            return Err(IdentityDispatchError::InvalidInput);
        }
        let mut stream = UnixStream::connect(&self.config.socket)
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        let credentials = rustix::net::sockopt::socket_peercred(&stream)
            .map_err(|_| IdentityDispatchError::ProviderAuthentication)?;
        if credentials.uid.as_raw() != self.config.peer_uid
            || credentials.gid.as_raw() != self.config.peer_gid
        {
            return Err(IdentityDispatchError::ProviderAuthentication);
        }
        stream
            .set_read_timeout(Some(self.config.deadline))
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        stream
            .set_write_timeout(Some(self.config.deadline))
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        stream
            .write_all(&(request.len() as u32).to_be_bytes())
            .and_then(|_| stream.write_all(&request))
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        let mut length = [0; 4];
        stream
            .read_exact(&mut length)
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        let length = u32::from_be_bytes(length) as usize;
        if length < 10 || length > self.config.maximum_frame_bytes {
            return Err(IdentityDispatchError::ProviderEvidence);
        }
        let mut response = vec![0; length];
        stream
            .read_exact(&mut response)
            .map_err(|_| IdentityDispatchError::ProviderUnavailable)?;
        decode_provider_response(&response)
    }
}

#[derive(Serialize, Deserialize)]
struct StoredProfile {
    display_name: String,
    avatar_url: Option<String>,
}

pub(crate) fn security_digest(
    scope: &PrincipalScope<'_>,
    action: &str,
    target: Option<&str>,
) -> Result<OperationDigest, IdentityDispatchError> {
    let action = match action {
        "add-passkey" => SecurityAction::AddPasskey,
        "revoke-passkey" => SecurityAction::RevokePasskey,
        "revoke-session" => SecurityAction::RevokeSession,
        "revoke-all-sessions" => SecurityAction::RevokeAllSessions,
        "add-authenticator" => SecurityAction::AddAuthenticator,
        "disable-authenticator" => SecurityAction::DisableAuthenticator,
        "rotate-backup-codes" => SecurityAction::RotateBackupCodes,
        "reveal-recovery-evidence" => SecurityAction::RevealRecoveryEvidence,
        _ => return Err(IdentityDispatchError::InvalidInput),
    };
    security_action_digest(scope.principal(), action, target)
        .map_err(|_| IdentityDispatchError::InvalidInput)
}

pub(crate) fn profile(scope: &PrincipalScope<'_>) -> Result<Value, IdentityDispatchError> {
    let row = RowKey::new(PROFILE_ROW)?;
    let stored = scope
        .get(Table::Cache, &row)
        .ok_or(IdentityDispatchError::NotFound)?;
    let profile: StoredProfile =
        serde_json::from_slice(stored.bytes()).map_err(|_| IdentityDispatchError::Corrupt)?;
    Ok(profile_json(&profile))
}

pub(crate) fn update_profile(
    scope: &mut PrincipalScope<'_>,
    body: &Value,
    now: u64,
) -> Result<Value, IdentityDispatchError> {
    let row = RowKey::new(PROFILE_ROW)?;
    let current = scope
        .get(Table::Cache, &row)
        .map(|stored| serde_json::from_slice::<StoredProfile>(stored.bytes()))
        .transpose()
        .map_err(|_| IdentityDispatchError::Corrupt)?;
    let display_name = body
        .get("display_name")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| current.as_ref().map(|value| value.display_name.clone()))
        .ok_or(IdentityDispatchError::InvalidInput)?;
    if display_name.is_empty()
        || display_name.len() > 256
        || display_name.chars().any(char::is_control)
    {
        return Err(IdentityDispatchError::InvalidInput);
    }
    let avatar_url = match body.get("avatar_url") {
        None => current.and_then(|value| value.avatar_url),
        Some(Value::Null) => None,
        Some(Value::String(value)) if value.len() <= 2_048 && value.starts_with("https://") => {
            Some(value.clone())
        }
        _ => return Err(IdentityDispatchError::InvalidInput),
    };
    let profile = StoredProfile {
        display_name,
        avatar_url,
    };
    scope.put(
        Table::Cache,
        row,
        now,
        serde_json::to_vec(&profile).map_err(|_| IdentityDispatchError::Corrupt)?,
    )?;
    Ok(profile_json(&profile))
}

pub(crate) fn step_up_challenge(value: &StepUpChallenge) -> Value {
    json!({"challenge_id": value.challenge_id, "confirms": digest(value.confirms),
        "ceremony": value.ceremony, "expires_at": value.expires_at})
}

pub(crate) fn step_up_evidence(value: &StepUpEvidence) -> Value {
    json!({"challenge_id": value.challenge_id(), "confirms": digest(value.confirms()),
        "passkey_id": value.passkey_id(), "completed_at": value.completed_at(),
        "expires_at": value.expires_at()})
}

pub(crate) fn onboarding_status(value: &OnboardingStatus) -> Value {
    let stages = value.stages().iter().map(|stage| json!({
        "stage": match stage.stage() { OnboardingStage::ApplicationIdentity => "application-identity",
            OnboardingStage::CustodyKey => "custody-key", OnboardingStage::DidRegistration => "protocol-identity",
            OnboardingStage::RecoveryRegistration => "recovery" },
        "state": stage_state(stage.state()),
        "evidence": stage.evidence().iter().map(|item| json!({"evidence_id": item.row().as_str(),
            "class": match item.class() { EvidenceClass::LocalJourneyState => "local-journey-state",
                EvidenceClass::CustodyKey => "custody-key", EvidenceClass::LayerxReceipt => "layerx-receipt",
                EvidenceClass::SubmissionRecord => "submission-record" },
            "verification": match item.verification() { EvidenceVerification::Unverified => "unverified",
                EvidenceVerification::ReceiptVerified => "receipt-verified" }})).collect::<Vec<_>>()
    })).collect::<Vec<_>>();
    json!({"kind":"onboarding", "state": match value.state() { OnboardingState::GettingReady => "getting-ready",
        OnboardingState::Queued => "queued", OnboardingState::Sending => "sending",
        OnboardingState::Processing => "processing", OnboardingState::StillChecking => "still-checking",
        OnboardingState::ActiveRecoveryPending => "active-recovery-pending", OnboardingState::Complete => "complete",
        OnboardingState::Refused => "refused" }, "account_active": value.account_active(), "stages": stages})
}

fn digest(value: OperationDigest) -> String {
    format!(
        "opd_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.bytes())
    )
}

fn profile_json(value: &StoredProfile) -> Value {
    json!({"display_name": value.display_name, "avatar_url": value.avatar_url})
}

fn stage_state(value: StageState) -> &'static str {
    match value {
        StageState::GettingReady => "getting-ready",
        StageState::LocalComplete => "local-complete",
        StageState::Queued { .. } => "queued",
        StageState::Sending => "sending",
        StageState::Processing => "processing",
        StageState::StillChecking => "still-checking",
        StageState::ReceiptVerified => "receipt-verified",
        StageState::Refused { .. } => "refused",
    }
}

fn provider_text(bytes: &[u8]) -> Result<String, IdentityDispatchError> {
    if bytes.is_empty() || bytes.len() > 4_096 {
        return Err(IdentityDispatchError::ProviderEvidence);
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| IdentityDispatchError::ProviderEvidence)
}

fn decode_provider_response(bytes: &[u8]) -> Result<Vec<Vec<u8>>, IdentityDispatchError> {
    if bytes.len() < 10 || &bytes[..4] != IDENTITY_MAGIC || bytes[4] != 1 {
        return Err(IdentityDispatchError::ProviderEvidence);
    }
    if bytes[5] != 0 {
        return Err(IdentityDispatchError::ProviderRefused);
    }
    let count = u32::from_be_bytes(
        bytes[6..10]
            .try_into()
            .map_err(|_| IdentityDispatchError::ProviderEvidence)?,
    ) as usize;
    let mut cursor = 10usize;
    let mut fields = Vec::with_capacity(count);
    for _ in 0..count {
        let end = cursor
            .checked_add(4)
            .ok_or(IdentityDispatchError::ProviderEvidence)?;
        let length = u32::from_be_bytes(
            bytes
                .get(cursor..end)
                .ok_or(IdentityDispatchError::ProviderEvidence)?
                .try_into()
                .map_err(|_| IdentityDispatchError::ProviderEvidence)?,
        ) as usize;
        cursor = end;
        let end = cursor
            .checked_add(length)
            .ok_or(IdentityDispatchError::ProviderEvidence)?;
        fields.push(
            bytes
                .get(cursor..end)
                .ok_or(IdentityDispatchError::ProviderEvidence)?
                .to_vec(),
        );
        cursor = end;
    }
    if cursor != bytes.len() {
        return Err(IdentityDispatchError::ProviderEvidence);
    }
    Ok(fields)
}

#[derive(Debug)]
pub(crate) enum IdentityDispatchError {
    InvalidConfiguration,
    InvalidInput,
    NotFound,
    Corrupt,
    ProviderUnavailable,
    ProviderAuthentication,
    ProviderEvidence,
    ProviderRefused,
    Store(StoreError),
}
impl From<StoreError> for IdentityDispatchError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}
