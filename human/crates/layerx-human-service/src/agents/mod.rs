//! Managed-agent lifecycle orchestration.

mod archive;
mod controls;
mod create;
mod reclaim;
mod recovery;
mod spend;

pub use archive::{
    AgentBalance, ArchiveAgentContract, ArchiveBoundary, ArchiveError, ArchiveJourney,
    ArchiveRequest, ArchiveStage, ArchiveStatus, ArchivedHistoryEntry, FundsDispositionEvidence,
    SessionArchiveAdapter, ARCHIVE_ACTION_LABEL, ARCHIVE_CONFIRMATION_TONE,
    ARCHIVE_IRREVERSIBILITY_NOTICE,
};

pub use controls::{
    AgentControlAgent, AgentControlContract, AgentControlError, AgentControlProfile,
    AgentControlState, AgentControlView, AgentControls, AppLimitEvidence, AppLimitRequest,
    LimitBacking, LimitChangeRequest, LimitEnforcement, PresentedLimit, SessionControlAdapter,
    APP_LIMIT_COPY_KEY, APP_LIMIT_EXPLANATION, PAUSE_CONSEQUENCE, PAUSE_CONSEQUENCE_COPY_KEY,
    PROTOCOL_LIMIT_COPY_KEY,
};
pub use create::{
    AgentCreationContract, AgentCreationError, AgentEvidence, AgentFailure, CapabilityProvision,
    CreateAgentRequest, CreationContext, CreationJourney, CreationProjection, CreationStage,
    CreationState, CreationStatus, ProtocolAction, ProtocolEvidence, PurposePresetCatalog,
    ScopedAgentCreationContract, SessionProvision, StageState,
};
pub use reclaim::{
    Reclaim, ReclaimAgentBoundary, ReclaimAgentContext, ReclaimError, ReclaimMechanism,
    ReclaimRequest, ReclaimResult, ReclaimStage, ReclaimStatus,
};
pub use recovery::{
    AgentKeyChallenge, AgentKeyChangeKind, AgentKeyChangeRequest, AgentKeyChangeStage,
    AgentRecovery, AgentRecoveryBoundary, AgentRecoveryBoundaryError, AgentRecoveryError,
    ChallengeDelay, CompetingRotation, ProtocolKeyChangeEvidence, ProtocolKeyChangeObservation,
    ProtocolKeyChangeState, RECOVERY_DELAY_COPY_KEY, ROTATION_COMPETITION_COPY_KEY,
    ROTATION_DELAY_COPY_KEY,
};
pub use spend::{
    AgentShell, AgentSpendSurfaces, AgentSpendView, ReconciliationDirection, ShellAgentSpend,
    SpendError, SpendProfile, SpendReconciliation, SpendReconciliationStatus,
    RECONCILIATION_COPY_KEY, RECONCILIATION_EXPLANATION,
};
