//! Production SDK contracts shared by daemon and human-plane integrations.

use std::collections::BTreeSet;
use std::fmt;

pub use layerx_mirror::source::{
    MirrorCanonicalPosition, MirrorLag, MirrorLocator, MirrorObservation, MirrorProvenance,
    MirrorReadPolicy, MirrorSource, MirrorSourceError, MirrorSourceFreshness, MirrorSourceId,
    MirrorSources, MirrorTargetIdentity,
};
pub use layerx_mirror::{
    MirrorVerification, MirrorVerificationFreshness, MirrorVerifier, MirrorVerifyError,
    SignedHeaderTrust,
};
use layerx_proof::checkpoint::{
    verify_certificate, Certificate, CheckpointError, GuarantorKey, SettlementDomain,
    ThresholdReport,
};
use layerx_proof::inclusion::{
    verify_activity, verify_state, InclusionError, InclusionEvidence, SequencerAuthorization,
};
use layerx_proof::merkle::Proof;
use layerx_proof::receipt::{
    verify_outcome, AuthorizedBatch, VerificationFailure, VerifiedReceipt,
};
use zeroize::Zeroize;

use crate::{Client, Deployment};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlatformSdkMetadata {
    pub package: &'static str,
    pub version: &'static str,
    pub agent_operations: usize,
    pub human_operations: usize,
}

#[must_use]
pub const fn platform_sdk_rust() -> PlatformSdkMetadata {
    PlatformSdkMetadata {
        package: "layerx-sdk",
        version: env!("CARGO_PKG_VERSION"),
        agent_operations: crate::Operation::ALL.len(),
        human_operations: HumanOperation::ALL.len(),
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum HumanOperation {
    AccountCreate,
    ActivityEntry,
    ActivityExportEvidence,
    ActivityExportStatement,
    ActivityQuery,
    AgentArchive,
    AgentCreate,
    AgentGet,
    AgentLimit,
    AgentList,
    AgentPause,
    AgentReclaim,
    AgentRecover,
    AgentResume,
    AgentRotate,
    ApprovalApprove,
    ApprovalGet,
    ApprovalList,
    ApprovalReject,
    BindingRebind,
    BindingStatement,
    BindingStatus,
    BindingSubmit,
    DepositConfirm,
    DepositStart,
    EvidenceGet,
    ExitEligibility,
    ExitStart,
    JourneyGet,
    JourneyList,
    MoveCommit,
    MoveQuote,
    NotificationList,
    NotificationPreferencesGet,
    NotificationPreferencesSet,
    NotificationRead,
    OnboardingResume,
    OnboardingStatus,
    PasskeyAssertBegin,
    PasskeyAssertFinish,
    PasskeyRegisterBegin,
    PasskeyRegisterFinish,
    ProfileGet,
    ProfileUpdate,
    SessionList,
    SessionOpen,
    SessionRefresh,
    SessionRevoke,
    SessionRevokeAll,
    StepupBegin,
    StepupFinish,
    StreamNext,
    StreamOpen,
    Version,
    WithdrawClaim,
    WithdrawStart,
}

impl HumanOperation {
    pub const ALL: &'static [Self] = &[
        Self::AccountCreate,
        Self::ActivityEntry,
        Self::ActivityExportEvidence,
        Self::ActivityExportStatement,
        Self::ActivityQuery,
        Self::AgentArchive,
        Self::AgentCreate,
        Self::AgentGet,
        Self::AgentLimit,
        Self::AgentList,
        Self::AgentPause,
        Self::AgentReclaim,
        Self::AgentRecover,
        Self::AgentResume,
        Self::AgentRotate,
        Self::ApprovalApprove,
        Self::ApprovalGet,
        Self::ApprovalList,
        Self::ApprovalReject,
        Self::BindingRebind,
        Self::BindingStatement,
        Self::BindingStatus,
        Self::BindingSubmit,
        Self::DepositConfirm,
        Self::DepositStart,
        Self::EvidenceGet,
        Self::ExitEligibility,
        Self::ExitStart,
        Self::JourneyGet,
        Self::JourneyList,
        Self::MoveCommit,
        Self::MoveQuote,
        Self::NotificationList,
        Self::NotificationPreferencesGet,
        Self::NotificationPreferencesSet,
        Self::NotificationRead,
        Self::OnboardingResume,
        Self::OnboardingStatus,
        Self::PasskeyAssertBegin,
        Self::PasskeyAssertFinish,
        Self::PasskeyRegisterBegin,
        Self::PasskeyRegisterFinish,
        Self::ProfileGet,
        Self::ProfileUpdate,
        Self::SessionList,
        Self::SessionOpen,
        Self::SessionRefresh,
        Self::SessionRevoke,
        Self::SessionRevokeAll,
        Self::StepupBegin,
        Self::StepupFinish,
        Self::StreamNext,
        Self::StreamOpen,
        Self::Version,
        Self::WithdrawClaim,
        Self::WithdrawStart,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AccountCreate => "account.create",
            Self::ActivityEntry => "activity.entry",
            Self::ActivityExportEvidence => "activity.export.evidence",
            Self::ActivityExportStatement => "activity.export.statement",
            Self::ActivityQuery => "activity.query",
            Self::AgentArchive => "agent.archive",
            Self::AgentCreate => "agent.create",
            Self::AgentGet => "agent.get",
            Self::AgentLimit => "agent.limit",
            Self::AgentList => "agent.list",
            Self::AgentPause => "agent.pause",
            Self::AgentReclaim => "agent.reclaim",
            Self::AgentRecover => "agent.recover",
            Self::AgentResume => "agent.resume",
            Self::AgentRotate => "agent.rotate",
            Self::ApprovalApprove => "approval.approve",
            Self::ApprovalGet => "approval.get",
            Self::ApprovalList => "approval.list",
            Self::ApprovalReject => "approval.reject",
            Self::BindingRebind => "binding.rebind",
            Self::BindingStatement => "binding.statement",
            Self::BindingStatus => "binding.status",
            Self::BindingSubmit => "binding.submit",
            Self::DepositConfirm => "deposit.confirm",
            Self::DepositStart => "deposit.start",
            Self::EvidenceGet => "evidence.get",
            Self::ExitEligibility => "exit.eligibility",
            Self::ExitStart => "exit.start",
            Self::JourneyGet => "journey.get",
            Self::JourneyList => "journey.list",
            Self::MoveCommit => "move.commit",
            Self::MoveQuote => "move.quote",
            Self::NotificationList => "notification.list",
            Self::NotificationPreferencesGet => "notification.preferences.get",
            Self::NotificationPreferencesSet => "notification.preferences.set",
            Self::NotificationRead => "notification.read",
            Self::OnboardingResume => "onboarding.resume",
            Self::OnboardingStatus => "onboarding.status",
            Self::PasskeyAssertBegin => "passkey.assert.begin",
            Self::PasskeyAssertFinish => "passkey.assert.finish",
            Self::PasskeyRegisterBegin => "passkey.register.begin",
            Self::PasskeyRegisterFinish => "passkey.register.finish",
            Self::ProfileGet => "profile.get",
            Self::ProfileUpdate => "profile.update",
            Self::SessionList => "session.list",
            Self::SessionOpen => "session.open",
            Self::SessionRefresh => "session.refresh",
            Self::SessionRevoke => "session.revoke",
            Self::SessionRevokeAll => "session.revoke-all",
            Self::StepupBegin => "stepup.begin",
            Self::StepupFinish => "stepup.finish",
            Self::StreamNext => "stream.next",
            Self::StreamOpen => "stream.open",
            Self::Version => "version",
            Self::WithdrawClaim => "withdraw.claim",
            Self::WithdrawStart => "withdraw.start",
        }
    }

    #[must_use]
    pub const fn requires_idempotency(self) -> bool {
        matches!(
            self,
            Self::AccountCreate
                | Self::ActivityExportEvidence
                | Self::ActivityExportStatement
                | Self::AgentArchive
                | Self::AgentCreate
                | Self::AgentLimit
                | Self::AgentPause
                | Self::AgentReclaim
                | Self::AgentRecover
                | Self::AgentResume
                | Self::AgentRotate
                | Self::ApprovalApprove
                | Self::ApprovalReject
                | Self::BindingRebind
                | Self::BindingSubmit
                | Self::DepositStart
                | Self::ExitStart
                | Self::MoveCommit
                | Self::WithdrawStart
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    /// Creates one caller-owned mutation key.
    ///
    /// # Errors
    ///
    /// Refuses empty, overlong, or NUL-containing values.
    pub fn new(value: impl Into<String>) -> Result<Self, ProductionError> {
        let value = value.into();
        if value.is_empty() || value.len() > 255 || value.as_bytes().contains(&0) {
            return Err(ProductionError::new(
                SdkErrorCode::InvalidArgument,
                RetryClass::Never,
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolAmount(u128);

impl ProtocolAmount {
    #[must_use]
    pub const fn new(value: u128) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Copies non-empty secret material into a zeroizing owner.
    ///
    /// # Errors
    ///
    /// Refuses empty inputs.
    pub fn new(value: &[u8]) -> Result<Self, ProductionError> {
        if value.is_empty() {
            return Err(ProductionError::new(
                SdkErrorCode::InvalidArgument,
                RetryClass::Never,
            ));
        }
        Ok(Self(value.to_vec()))
    }

    pub fn expose_to<T>(&self, consumer: impl FnOnce(&[u8]) -> T) -> T {
        consumer(&self.0)
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryClass {
    Never,
    Safe,
    After,
    UnknownOutcome,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdkErrorCode {
    InvalidArgument,
    IdempotencyRequired,
    TransportFailure,
    Deadline,
    ProtocolIncompatibility,
    UnavailableCapability,
    CoreRejection,
    VerificationFailure,
    PolicyRefusal,
    CapabilityRefusal,
    BudgetRefusal,
    RateLimit,
    IdempotencyConflict,
    DecodeFailure,
    UnknownOutcome,
    InternalFault,
}

impl SdkErrorCode {
    #[must_use]
    pub const fn machine_code(self) -> &'static str {
        match self {
            Self::InvalidArgument => "invalid-argument",
            Self::IdempotencyRequired => "idempotency-required",
            Self::TransportFailure => "transport-failure",
            Self::Deadline => "deadline",
            Self::ProtocolIncompatibility => "protocol-incompatibility",
            Self::UnavailableCapability => "unavailable-capability",
            Self::CoreRejection => "core-rejection",
            Self::VerificationFailure => "verification-failure",
            Self::PolicyRefusal => "policy-refusal",
            Self::CapabilityRefusal => "capability-refusal",
            Self::BudgetRefusal => "budget-refusal",
            Self::RateLimit => "rate-limit",
            Self::IdempotencyConflict => "idempotency-conflict",
            Self::DecodeFailure => "decode-failure",
            Self::UnknownOutcome => "unknown-outcome",
            Self::InternalFault => "internal-fault",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProductionError {
    pub code: SdkErrorCode,
    pub retry: RetryClass,
    pub protocol_result_code: Option<i32>,
    pub retry_after_ms: Option<u64>,
}

impl ProductionError {
    #[must_use]
    pub const fn new(code: SdkErrorCode, retry: RetryClass) -> Self {
        Self {
            code,
            retry,
            protocol_result_code: None,
            retry_after_ms: None,
        }
    }
}

impl fmt::Display for ProductionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code.machine_code())
    }
}

impl std::error::Error for ProductionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HumanCall<T> {
    deployment: Deployment,
    operation: HumanOperation,
    idempotency_key: Option<IdempotencyKey>,
    request: T,
}

impl<T> HumanCall<T> {
    #[must_use]
    pub const fn deployment(&self) -> Deployment {
        self.deployment
    }

    #[must_use]
    pub const fn operation(&self) -> HumanOperation {
        self.operation
    }

    #[must_use]
    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }

    #[must_use]
    pub const fn request(&self) -> &T {
        &self.request
    }
}

pub trait HumanApiCalls {
    /// Builds a human-plane call while enforcing mutation idempotency.
    ///
    /// # Errors
    ///
    /// Refuses a mutation without a caller-owned idempotency key.
    fn human_call<T>(
        &self,
        operation: HumanOperation,
        idempotency_key: Option<IdempotencyKey>,
        request: T,
    ) -> Result<HumanCall<T>, ProductionError>;
}

impl HumanApiCalls for Client {
    fn human_call<T>(
        &self,
        operation: HumanOperation,
        idempotency_key: Option<IdempotencyKey>,
        request: T,
    ) -> Result<HumanCall<T>, ProductionError> {
        if operation.requires_idempotency() && idempotency_key.is_none() {
            return Err(ProductionError::new(
                SdkErrorCode::IdempotencyRequired,
                RetryClass::Never,
            ));
        }
        Ok(HumanCall {
            deployment: self.deployment(),
            operation,
            idempotency_key,
            request,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamCursor(String);

impl StreamCursor {
    /// Creates one bounded opaque service cursor.
    ///
    /// # Errors
    ///
    /// Refuses empty, overlong, or NUL-containing cursors.
    pub fn new(value: impl Into<String>) -> Result<Self, ProductionError> {
        let value = value.into();
        if value.is_empty() || value.len() > 512 || value.as_bytes().contains(&0) {
            return Err(ProductionError::new(
                SdkErrorCode::InvalidArgument,
                RetryClass::Never,
            ));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamEvent<T> {
    pub event_id: String,
    pub previous_cursor: StreamCursor,
    pub cursor: StreamCursor,
    pub value: T,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamPage<T> {
    pub requested_cursor: StreamCursor,
    pub events: Vec<StreamEvent<T>>,
    pub next_cursor: StreamCursor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResumableStream {
    cursor: StreamCursor,
    seen: BTreeSet<String>,
}

impl ResumableStream {
    #[must_use]
    pub fn new(cursor: StreamCursor) -> Self {
        Self {
            cursor,
            seen: BTreeSet::new(),
        }
    }

    #[must_use]
    pub const fn cursor(&self) -> &StreamCursor {
        &self.cursor
    }

    /// Admits one exact cursor chain and advances only after every event passes.
    ///
    /// # Errors
    ///
    /// Refuses gaps, duplicates, replayed pages, and inconsistent next cursors.
    pub fn accept<T>(
        &mut self,
        page: StreamPage<T>,
    ) -> Result<Vec<StreamEvent<T>>, ProductionError> {
        if page.requested_cursor != self.cursor {
            return Err(ProductionError::new(
                SdkErrorCode::DecodeFailure,
                RetryClass::Never,
            ));
        }
        let mut expected = self.cursor.clone();
        let mut page_ids = BTreeSet::new();
        for event in &page.events {
            if event.event_id.is_empty()
                || event.previous_cursor != expected
                || self.seen.contains(&event.event_id)
                || !page_ids.insert(event.event_id.clone())
            {
                return Err(ProductionError::new(
                    SdkErrorCode::DecodeFailure,
                    RetryClass::Never,
                ));
            }
            expected = event.cursor.clone();
        }
        if page.next_cursor != expected {
            return Err(ProductionError::new(
                SdkErrorCode::DecodeFailure,
                RetryClass::Never,
            ));
        }
        self.seen.extend(page_ids);
        self.cursor = page.next_cursor;
        Ok(page.events)
    }
}

/// Verifies one full canonical receipt locally while preserving protocol refusal codes.
///
/// # Errors
///
/// Returns the exact canonical, invariant, root-chain, or signature failure.
pub fn verify_receipt(
    canonical_receipt: &[u8],
    authorised_batch: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    verify_outcome(canonical_receipt, authorised_batch)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MirrorSdkError {
    Source(MirrorSourceError),
    Verification(MirrorVerifyError),
}

impl From<MirrorSourceError> for MirrorSdkError {
    fn from(value: MirrorSourceError) -> Self {
        Self::Source(value)
    }
}

impl From<MirrorVerifyError> for MirrorSdkError {
    fn from(value: MirrorVerifyError) -> Self {
        Self::Verification(value)
    }
}

/// Resolves a pinned source policy and verifies the receipt without composing
/// chain, archive or freshness evidence across sources.
///
/// # Errors
///
/// Returns the exact source read failure or receipt verification failure.
pub fn verify_mirror_receipt_from_sources(
    sources: &MirrorSources,
    batch_number: u64,
    policy: &MirrorReadPolicy,
    canonical_receipt: &[u8],
    trust: SignedHeaderTrust,
) -> Result<MirrorVerification<VerifiedReceipt>, MirrorSdkError> {
    let archive = sources.read(batch_number, policy)?;
    Ok(MirrorVerifier::from_source(archive, trust)?.receipt(canonical_receipt)?)
}

/// Resolves a pinned source policy and verifies state inclusion without
/// composing chain, archive or freshness evidence across sources.
///
/// # Errors
///
/// Returns the exact source read failure or state verification failure.
pub fn verify_mirror_state_from_sources(
    sources: &MirrorSources,
    batch_number: u64,
    policy: &MirrorReadPolicy,
    canonical_state: &[u8],
    proof: &Proof,
    trust: SignedHeaderTrust,
) -> Result<MirrorVerification<InclusionEvidence>, MirrorSdkError> {
    let archive = sources.read(batch_number, policy)?;
    Ok(MirrorVerifier::from_source(archive, trust)?.state(canonical_state, proof)?)
}

/// Verifies canonical activity inclusion under an authorised signed batch header.
///
/// # Errors
///
/// Returns the exact header, authority, signature, or Merkle failure.
pub fn verify_batch_inclusion(
    canonical_activity: &[u8],
    proof: &Proof,
    canonical_header: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<InclusionEvidence, InclusionError> {
    verify_activity(
        canonical_activity,
        proof,
        canonical_header,
        header_signature,
        authorization,
    )
}

/// Verifies canonical state inclusion under an authorised signed batch header.
///
/// # Errors
///
/// Returns the exact header, authority, signature, root, or Merkle failure.
pub fn verify_state_inclusion(
    canonical_state: &[u8],
    proof: &Proof,
    named_state_root: &[u8; 32],
    canonical_header: &[u8],
    header_signature: &[u8; 64],
    authorization: &SequencerAuthorization,
) -> Result<InclusionEvidence, InclusionError> {
    verify_state(
        canonical_state,
        proof,
        named_state_root,
        canonical_header,
        header_signature,
        authorization,
    )
}

/// Verifies a checkpoint certificate against its bonded set and registration.
///
/// # Errors
///
/// Returns the exact identifier, membership, signature, threshold, availability, or settlement failure.
pub fn verify_checkpoint(
    certificate: &Certificate,
    bonded_set: &[GuarantorKey],
    registered_checkpoint_id: &[u8; 32],
    expected_settlement_domain: SettlementDomain,
    registered_settlement_reference: Option<&[u8]>,
) -> Result<ThresholdReport, CheckpointError> {
    verify_certificate(
        certificate,
        bonded_set,
        registered_checkpoint_id,
        expected_settlement_domain,
        registered_settlement_reference,
    )
}
