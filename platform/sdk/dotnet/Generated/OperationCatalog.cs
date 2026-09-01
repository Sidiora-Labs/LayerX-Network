// Generated from agent-api and human-api. Do not hand-edit.
#nullable enable

namespace LayerX.Sdk;

public enum PlatformOperation
{
    AgentAgentRegister,
    AgentApprovalApprove,
    AgentApprovalGet,
    AgentApprovalList,
    AgentApprovalReject,
    AgentAvailabilityFetch,
    AgentBudgetCreate,
    AgentBudgetFund,
    AgentBudgetList,
    AgentBudgetReconciliation,
    AgentBudgetRevoke,
    AgentCapabilityAttenuate,
    AgentCapabilityCreate,
    AgentCapabilityList,
    AgentCapabilityRevoke,
    AgentExportOffline,
    AgentPrepare,
    AgentProgramActivity,
    AgentProgramCall,
    AgentProgramDiscover,
    AgentProgramInterface,
    AgentProgramReceipt,
    AgentProgramSimulate,
    AgentProject,
    AgentReadAccount,
    AgentReadBalance,
    AgentReadBatch,
    AgentReadCheckpoint,
    AgentReadHistory,
    AgentReadModuleState,
    AgentReadProofBundle,
    AgentSessionClose,
    AgentSessionList,
    AgentSessionOpen,
    AgentSessionRefresh,
    AgentSign,
    AgentSubmit,
    AgentSubscriptionAcknowledge,
    AgentSubscriptionCreate,
    AgentSubscriptionDelete,
    AgentSubscriptionHealth,
    AgentSubscriptionList,
    AgentSubscriptionPause,
    AgentSubscriptionResume,
    AgentTrack,
    AgentWait,
    HumanAccountBalance,
    HumanAccountCreate,
    HumanActivityEntry,
    HumanActivityExportEvidence,
    HumanActivityExportStatement,
    HumanActivityQuery,
    HumanAgentArchive,
    HumanAgentCreate,
    HumanAgentGet,
    HumanAgentLimit,
    HumanAgentList,
    HumanAgentPause,
    HumanAgentReclaim,
    HumanAgentRecover,
    HumanAgentResume,
    HumanAgentRotate,
    HumanApprovalApprove,
    HumanApprovalGet,
    HumanApprovalList,
    HumanApprovalReject,
    HumanAuthenticatorBackupRotate,
    HumanAuthenticatorDisable,
    HumanAuthenticatorSetupBegin,
    HumanAuthenticatorSetupFinish,
    HumanAuthenticatorStatus,
    HumanBindingRebind,
    HumanBindingRebindAction,
    HumanBindingStatement,
    HumanBindingStatus,
    HumanBindingSubmit,
    HumanDepositConfirm,
    HumanDepositStart,
    HumanEvidenceGet,
    HumanExitEligibility,
    HumanExitStart,
    HumanHomeSummary,
    HumanJourneyGet,
    HumanJourneyList,
    HumanMoveCommit,
    HumanMoveQuote,
    HumanNotificationList,
    HumanNotificationPreferencesGet,
    HumanNotificationPreferencesSet,
    HumanNotificationRead,
    HumanOnboardingResume,
    HumanOnboardingStatus,
    HumanPasskeyAssertBegin,
    HumanPasskeyAssertFinish,
    HumanPasskeyRegisterBegin,
    HumanPasskeyRegisterFinish,
    HumanProfileGet,
    HumanProfileUpdate,
    HumanSecurityAction,
    HumanSecurityPasskeyList,
    HumanSecurityPasskeyRegisterBegin,
    HumanSecurityPasskeyRegisterFinish,
    HumanSecurityPasskeyRevoke,
    HumanSecurityRecoveryReveal,
    HumanSecuritySessionRevoke,
    HumanSecuritySessionRevokeAll,
    HumanSessionList,
    HumanSessionOpen,
    HumanSessionRefresh,
    HumanSessionRevoke,
    HumanSessionRevokeAll,
    HumanStepupBegin,
    HumanStepupFinish,
    HumanStreamNext,
    HumanStreamOpen,
    HumanSupportCreate,
    HumanSupportFeedback,
    HumanSupportList,
    HumanSupportRead,
    HumanSupportReply,
    HumanSupportStatus,
    HumanVersion,
    HumanWithdrawClaim,
    HumanWithdrawStart
}

public static class GeneratedOperationCatalog
{
    public static OperationDescriptor Descriptor(this PlatformOperation operation) => operation switch
    {
            PlatformOperation.AgentAgentRegister => new(PlatformPlane.Agent, "agent.register", SdkHttpMethod.Post, "", "AgentRegistration", "AuthorityResponse<AgentRecord>", true, false),
            PlatformOperation.AgentApprovalApprove => new(PlatformPlane.Agent, "approval.approve", SdkHttpMethod.Post, "", "ApprovalApproveRequest", "ApprovalDecision", true, false),
            PlatformOperation.AgentApprovalGet => new(PlatformPlane.Agent, "approval.get", SdkHttpMethod.Post, "", "ApprovalGetRequest", "ApprovalRecord", false, false),
            PlatformOperation.AgentApprovalList => new(PlatformPlane.Agent, "approval.list", SdkHttpMethod.Post, "", "ApprovalListRequest", "ApprovalPage", false, false),
            PlatformOperation.AgentApprovalReject => new(PlatformPlane.Agent, "approval.reject", SdkHttpMethod.Post, "", "ApprovalRejectRequest", "ApprovalDecision", true, false),
            PlatformOperation.AgentAvailabilityFetch => new(PlatformPlane.Agent, "availability.fetch", SdkHttpMethod.Post, "", "object", "VerifiedRead<AvailabilityReport>", false, false),
            PlatformOperation.AgentBudgetCreate => new(PlatformPlane.Agent, "budget.create", SdkHttpMethod.Post, "", "object", "AuthorityResponse<BudgetRecord>", true, false),
            PlatformOperation.AgentBudgetFund => new(PlatformPlane.Agent, "budget.fund", SdkHttpMethod.Post, "", "object", "AuthorityResponse<BudgetRecord>", true, false),
            PlatformOperation.AgentBudgetList => new(PlatformPlane.Agent, "budget.list", SdkHttpMethod.Post, "", "object", "AuthorityResponse<BudgetRecords>", false, false),
            PlatformOperation.AgentBudgetReconciliation => new(PlatformPlane.Agent, "budget.reconciliation", SdkHttpMethod.Post, "", "object", "AuthorityResponse<BudgetReconciliation>", false, false),
            PlatformOperation.AgentBudgetRevoke => new(PlatformPlane.Agent, "budget.revoke", SdkHttpMethod.Post, "", "object", "AuthorityResponse<BudgetRecord>", true, false),
            PlatformOperation.AgentCapabilityAttenuate => new(PlatformPlane.Agent, "capability.attenuate", SdkHttpMethod.Post, "", "object", "AuthorityResponse<CapabilityRecord>", true, false),
            PlatformOperation.AgentCapabilityCreate => new(PlatformPlane.Agent, "capability.create", SdkHttpMethod.Post, "", "object", "AuthorityResponse<CapabilityRecord>", true, false),
            PlatformOperation.AgentCapabilityList => new(PlatformPlane.Agent, "capability.list", SdkHttpMethod.Post, "", "object", "AuthorityResponse<CapabilityRecords>", false, false),
            PlatformOperation.AgentCapabilityRevoke => new(PlatformPlane.Agent, "capability.revoke", SdkHttpMethod.Post, "", "object", "AuthorityResponse<CapabilityRecord>", true, false),
            PlatformOperation.AgentExportOffline => new(PlatformPlane.Agent, "export.offline", SdkHttpMethod.Post, "", "object", "VerifiedRead<OfflineExport>", false, false),
            PlatformOperation.AgentPrepare => new(PlatformPlane.Agent, "prepare", SdkHttpMethod.Post, "", "PrepareRequest", "Prepared", true, false),
            PlatformOperation.AgentProgramActivity => new(PlatformPlane.Agent, "program.activity", SdkHttpMethod.Get, "/v1/programs/activities/{activity_id}", "ProgramActivitySelector", "ProgramSubmission", false, false),
            PlatformOperation.AgentProgramCall => new(PlatformPlane.Agent, "program.call", SdkHttpMethod.Post, "/v1/programs/call", "ProgramCallRequest", "ProgramSubmission", true, false),
            PlatformOperation.AgentProgramDiscover => new(PlatformPlane.Agent, "program.discover", SdkHttpMethod.Get, "/v1/programs/registry/{program_id}", "ProgramSelector", "VerifiedProgramDiscovery", false, false),
            PlatformOperation.AgentProgramInterface => new(PlatformPlane.Agent, "program.interface", SdkHttpMethod.Get, "/v1/programs/registry/{program_id}/interface", "ProgramSelector", "VerifiedProgramInterface", false, false),
            PlatformOperation.AgentProgramReceipt => new(PlatformPlane.Agent, "program.receipt", SdkHttpMethod.Get, "/v1/programs/receipts/by-idempotency/{idempotency_key}", "ProgramReceiptSelector", "ProgramSubmission", false, false),
            PlatformOperation.AgentProgramSimulate => new(PlatformPlane.Agent, "program.simulate", SdkHttpMethod.Post, "/v1/programs/simulate", "ProgramCallRequest", "ProgramSimulation", false, false),
            PlatformOperation.AgentProject => new(PlatformPlane.Agent, "project", SdkHttpMethod.Post, "", "object", "ProjectionResult", false, false),
            PlatformOperation.AgentReadAccount => new(PlatformPlane.Agent, "read.account", SdkHttpMethod.Post, "", "object", "VerifiedRead<AccountValue>", false, false),
            PlatformOperation.AgentReadBalance => new(PlatformPlane.Agent, "read.balance", SdkHttpMethod.Post, "", "object", "VerifiedRead<BalanceValue>", false, false),
            PlatformOperation.AgentReadBatch => new(PlatformPlane.Agent, "read.batch", SdkHttpMethod.Post, "", "object", "VerifiedRead<BatchValue>", false, false),
            PlatformOperation.AgentReadCheckpoint => new(PlatformPlane.Agent, "read.checkpoint", SdkHttpMethod.Post, "", "object", "VerifiedRead<CheckpointValue>", false, false),
            PlatformOperation.AgentReadHistory => new(PlatformPlane.Agent, "read.history", SdkHttpMethod.Post, "", "object", "VerifiedRead<HistoryValue>", false, false),
            PlatformOperation.AgentReadModuleState => new(PlatformPlane.Agent, "read.module_state", SdkHttpMethod.Post, "", "object", "VerifiedRead<ModuleStateValue>", false, false),
            PlatformOperation.AgentReadProofBundle => new(PlatformPlane.Agent, "read.proof_bundle", SdkHttpMethod.Post, "", "object", "VerifiedRead<ProofBundle>", false, false),
            PlatformOperation.AgentSessionClose => new(PlatformPlane.Agent, "session.close", SdkHttpMethod.Post, "", "SessionClose", "AuthorityResponse<SessionRecord>", true, false),
            PlatformOperation.AgentSessionList => new(PlatformPlane.Agent, "session.list", SdkHttpMethod.Post, "", "SessionList", "AuthorityResponse<SessionRecords>", false, false),
            PlatformOperation.AgentSessionOpen => new(PlatformPlane.Agent, "session.open", SdkHttpMethod.Post, "", "SessionOpen", "AuthorityResponse<SessionRecord>", true, false),
            PlatformOperation.AgentSessionRefresh => new(PlatformPlane.Agent, "session.refresh", SdkHttpMethod.Post, "", "SessionRefresh", "AuthorityResponse<SessionRecord>", true, false),
            PlatformOperation.AgentSign => new(PlatformPlane.Agent, "sign", SdkHttpMethod.Post, "", "SignRequest", "Signed", true, false),
            PlatformOperation.AgentSubmit => new(PlatformPlane.Agent, "submit", SdkHttpMethod.Post, "", "SubmitRequest", "TrackedSubmission", true, false),
            PlatformOperation.AgentSubscriptionAcknowledge => new(PlatformPlane.Agent, "subscription.acknowledge", SdkHttpMethod.Post, "", "object", "object", true, false),
            PlatformOperation.AgentSubscriptionCreate => new(PlatformPlane.Agent, "subscription.create", SdkHttpMethod.Post, "", "object", "object", true, false),
            PlatformOperation.AgentSubscriptionDelete => new(PlatformPlane.Agent, "subscription.delete", SdkHttpMethod.Post, "", "object", "object", true, false),
            PlatformOperation.AgentSubscriptionHealth => new(PlatformPlane.Agent, "subscription.health", SdkHttpMethod.Post, "", "object", "object", false, false),
            PlatformOperation.AgentSubscriptionList => new(PlatformPlane.Agent, "subscription.list", SdkHttpMethod.Post, "", "object", "object", false, false),
            PlatformOperation.AgentSubscriptionPause => new(PlatformPlane.Agent, "subscription.pause", SdkHttpMethod.Post, "", "object", "object", true, false),
            PlatformOperation.AgentSubscriptionResume => new(PlatformPlane.Agent, "subscription.resume", SdkHttpMethod.Post, "", "object", "object", true, false),
            PlatformOperation.AgentTrack => new(PlatformPlane.Agent, "track", SdkHttpMethod.Post, "", "TrackRequest", "TrackedSubmission", false, false),
            PlatformOperation.AgentWait => new(PlatformPlane.Agent, "wait", SdkHttpMethod.Post, "", "WaitRequest", "WaitResult", false, false),
            PlatformOperation.HumanAccountBalance => new(PlatformPlane.Human, "account.balance", SdkHttpMethod.Get, "/v1/account/balance", "Empty", "AccountBalance", false, true),
            PlatformOperation.HumanAccountCreate => new(PlatformPlane.Human, "account.create", SdkHttpMethod.Post, "/v1/accounts", "AccountCreateRequest", "AccountCreation", true, false),
            PlatformOperation.HumanActivityEntry => new(PlatformPlane.Human, "activity.entry", SdkHttpMethod.Get, "/v1/activity/{entry_id}", "Empty", "ActivityEntryDetail", false, true),
            PlatformOperation.HumanActivityExportEvidence => new(PlatformPlane.Human, "activity.export.evidence", SdkHttpMethod.Post, "/v1/activity/exports/evidence", "ExportEvidenceRequest", "ExportArtefact", true, false),
            PlatformOperation.HumanActivityExportStatement => new(PlatformPlane.Human, "activity.export.statement", SdkHttpMethod.Post, "/v1/activity/exports/statement", "ExportStatementRequest", "ExportArtefact", true, false),
            PlatformOperation.HumanActivityQuery => new(PlatformPlane.Human, "activity.query", SdkHttpMethod.Post, "/v1/activity/query", "ActivityQueryRequest", "ActivityPage", false, false),
            PlatformOperation.HumanAgentArchive => new(PlatformPlane.Human, "agent.archive", SdkHttpMethod.Post, "/v1/agents/{agent_id}/archive", "AgentArchiveRequest", "Journey", true, false),
            PlatformOperation.HumanAgentCreate => new(PlatformPlane.Human, "agent.create", SdkHttpMethod.Post, "/v1/agents", "AgentCreateRequest", "Journey", true, false),
            PlatformOperation.HumanAgentGet => new(PlatformPlane.Human, "agent.get", SdkHttpMethod.Get, "/v1/agents/{agent_id}", "Empty", "Agent", false, true),
            PlatformOperation.HumanAgentLimit => new(PlatformPlane.Human, "agent.limit", SdkHttpMethod.Post, "/v1/agents/{agent_id}/limit", "AgentLimitRequest", "Agent", true, false),
            PlatformOperation.HumanAgentList => new(PlatformPlane.Human, "agent.list", SdkHttpMethod.Get, "/v1/agents", "Empty", "AgentPage", false, true),
            PlatformOperation.HumanAgentPause => new(PlatformPlane.Human, "agent.pause", SdkHttpMethod.Post, "/v1/agents/{agent_id}/pause", "Empty", "Agent", true, true),
            PlatformOperation.HumanAgentReclaim => new(PlatformPlane.Human, "agent.reclaim", SdkHttpMethod.Post, "/v1/agents/{agent_id}/reclaim", "AgentReclaimRequest", "Journey", true, false),
            PlatformOperation.HumanAgentRecover => new(PlatformPlane.Human, "agent.recover", SdkHttpMethod.Post, "/v1/agents/{agent_id}/recover", "Empty", "KeyChallenge", true, true),
            PlatformOperation.HumanAgentResume => new(PlatformPlane.Human, "agent.resume", SdkHttpMethod.Post, "/v1/agents/{agent_id}/resume", "Empty", "Agent", true, true),
            PlatformOperation.HumanAgentRotate => new(PlatformPlane.Human, "agent.rotate", SdkHttpMethod.Post, "/v1/agents/{agent_id}/rotate", "Empty", "KeyChallenge", true, true),
            PlatformOperation.HumanApprovalApprove => new(PlatformPlane.Human, "approval.approve", SdkHttpMethod.Post, "/v1/approvals/{approval_id}/approve", "ApprovalApproveRequest", "ApprovalDecision", true, false),
            PlatformOperation.HumanApprovalGet => new(PlatformPlane.Human, "approval.get", SdkHttpMethod.Get, "/v1/approvals/{approval_id}", "Empty", "ApprovalDetail", false, true),
            PlatformOperation.HumanApprovalList => new(PlatformPlane.Human, "approval.list", SdkHttpMethod.Get, "/v1/approvals", "Empty", "ApprovalPage", false, true),
            PlatformOperation.HumanApprovalReject => new(PlatformPlane.Human, "approval.reject", SdkHttpMethod.Post, "/v1/approvals/{approval_id}/reject", "Empty", "ApprovalDecision", true, true),
            PlatformOperation.HumanAuthenticatorBackupRotate => new(PlatformPlane.Human, "authenticator.backup.rotate", SdkHttpMethod.Post, "/v1/security/authenticators/backup-codes", "BackupCodeRotation", "BackupCodeSet", false, false),
            PlatformOperation.HumanAuthenticatorDisable => new(PlatformPlane.Human, "authenticator.disable", SdkHttpMethod.Post, "/v1/security/authenticators/{authenticator_id}/disable", "AuthenticatorDisable", "AuthenticatorStatus", false, false),
            PlatformOperation.HumanAuthenticatorSetupBegin => new(PlatformPlane.Human, "authenticator.setup.begin", SdkHttpMethod.Post, "/v1/security/authenticators/setups", "AuthenticatorSetupBegin", "AuthenticatorSetupChallenge", false, false),
            PlatformOperation.HumanAuthenticatorSetupFinish => new(PlatformPlane.Human, "authenticator.setup.finish", SdkHttpMethod.Post, "/v1/security/authenticators/setups/{setup_id}", "AuthenticatorSetupFinish", "AuthenticatorSetupResult", false, false),
            PlatformOperation.HumanAuthenticatorStatus => new(PlatformPlane.Human, "authenticator.status", SdkHttpMethod.Get, "/v1/security/authenticators", "Empty", "AuthenticatorStatus", false, true),
            PlatformOperation.HumanBindingRebind => new(PlatformPlane.Human, "binding.rebind", SdkHttpMethod.Post, "/v1/wallet-binding/rebind", "RebindingSubmission", "Journey", true, false),
            PlatformOperation.HumanBindingRebindAction => new(PlatformPlane.Human, "binding.rebind.action", SdkHttpMethod.Post, "/v1/wallet-binding/rebind/action", "BindingStatementRequest", "BindingRebindAction", false, false),
            PlatformOperation.HumanBindingStatement => new(PlatformPlane.Human, "binding.statement", SdkHttpMethod.Post, "/v1/wallet-binding/statement", "BindingStatementRequest", "BindingStatement", false, false),
            PlatformOperation.HumanBindingStatus => new(PlatformPlane.Human, "binding.status", SdkHttpMethod.Get, "/v1/wallet-binding", "Empty", "WalletBinding", false, true),
            PlatformOperation.HumanBindingSubmit => new(PlatformPlane.Human, "binding.submit", SdkHttpMethod.Post, "/v1/wallet-binding", "BindingSubmission", "Journey", true, false),
            PlatformOperation.HumanDepositConfirm => new(PlatformPlane.Human, "deposit.confirm", SdkHttpMethod.Post, "/v1/deposits/{journey_id}/confirm", "DepositConfirmRequest", "Journey", false, false),
            PlatformOperation.HumanDepositStart => new(PlatformPlane.Human, "deposit.start", SdkHttpMethod.Post, "/v1/deposits", "DepositStartRequest", "Journey", true, false),
            PlatformOperation.HumanEvidenceGet => new(PlatformPlane.Human, "evidence.get", SdkHttpMethod.Get, "/v1/evidence/{evidence_id}", "Empty", "EvidenceMaterial", false, true),
            PlatformOperation.HumanExitEligibility => new(PlatformPlane.Human, "exit.eligibility", SdkHttpMethod.Get, "/v1/exit/eligibility", "Empty", "ExitEligibility", false, true),
            PlatformOperation.HumanExitStart => new(PlatformPlane.Human, "exit.start", SdkHttpMethod.Post, "/v1/exit", "ExitStartRequest", "Journey", true, false),
            PlatformOperation.HumanHomeSummary => new(PlatformPlane.Human, "home.summary", SdkHttpMethod.Get, "/v1/home", "Empty", "HomeSummary", false, true),
            PlatformOperation.HumanJourneyGet => new(PlatformPlane.Human, "journey.get", SdkHttpMethod.Get, "/v1/journeys/{journey_id}", "Empty", "Journey", false, true),
            PlatformOperation.HumanJourneyList => new(PlatformPlane.Human, "journey.list", SdkHttpMethod.Get, "/v1/journeys", "Empty", "JourneyPage", false, true),
            PlatformOperation.HumanMoveCommit => new(PlatformPlane.Human, "move.commit", SdkHttpMethod.Post, "/v1/moves", "MoveCommitRequest", "Journey", true, false),
            PlatformOperation.HumanMoveQuote => new(PlatformPlane.Human, "move.quote", SdkHttpMethod.Post, "/v1/moves/quote", "MoveQuoteRequest", "MoveQuote", false, false),
            PlatformOperation.HumanNotificationList => new(PlatformPlane.Human, "notification.list", SdkHttpMethod.Get, "/v1/notifications", "Empty", "NotificationPage", false, true),
            PlatformOperation.HumanNotificationPreferencesGet => new(PlatformPlane.Human, "notification.preferences.get", SdkHttpMethod.Get, "/v1/notifications/preferences", "Empty", "NotificationPreferences", false, true),
            PlatformOperation.HumanNotificationPreferencesSet => new(PlatformPlane.Human, "notification.preferences.set", SdkHttpMethod.Post, "/v1/notifications/preferences", "NotificationPreferences", "NotificationPreferences", false, false),
            PlatformOperation.HumanNotificationRead => new(PlatformPlane.Human, "notification.read", SdkHttpMethod.Post, "/v1/notifications/{notification_id}/read", "Empty", "NotificationSummary", false, true),
            PlatformOperation.HumanOnboardingResume => new(PlatformPlane.Human, "onboarding.resume", SdkHttpMethod.Post, "/v1/onboarding/resume", "Empty", "Journey", false, true),
            PlatformOperation.HumanOnboardingStatus => new(PlatformPlane.Human, "onboarding.status", SdkHttpMethod.Get, "/v1/onboarding", "Empty", "Journey", false, true),
            PlatformOperation.HumanPasskeyAssertBegin => new(PlatformPlane.Human, "passkey.assert.begin", SdkHttpMethod.Post, "/v1/passkeys/assertions", "PasskeyAssertionBegin", "PasskeyAssertionChallenge", false, false),
            PlatformOperation.HumanPasskeyAssertFinish => new(PlatformPlane.Human, "passkey.assert.finish", SdkHttpMethod.Post, "/v1/passkeys/assertions/{assertion_id}", "PasskeyAssertionFinish", "PasskeyAssertion", false, false),
            PlatformOperation.HumanPasskeyRegisterBegin => new(PlatformPlane.Human, "passkey.register.begin", SdkHttpMethod.Post, "/v1/passkeys/registrations", "PasskeyRegistrationBegin", "PasskeyRegistrationChallenge", false, false),
            PlatformOperation.HumanPasskeyRegisterFinish => new(PlatformPlane.Human, "passkey.register.finish", SdkHttpMethod.Post, "/v1/passkeys/registrations/{registration_id}", "PasskeyRegistrationFinish", "Passkey", false, false),
            PlatformOperation.HumanProfileGet => new(PlatformPlane.Human, "profile.get", SdkHttpMethod.Get, "/v1/profile", "Empty", "Profile", false, true),
            PlatformOperation.HumanProfileUpdate => new(PlatformPlane.Human, "profile.update", SdkHttpMethod.Patch, "/v1/profile", "ProfileUpdate", "Profile", false, false),
            PlatformOperation.HumanSecurityAction => new(PlatformPlane.Human, "security.action", SdkHttpMethod.Post, "/v1/security/actions", "SecurityActionRequest", "SecurityAction", false, false),
            PlatformOperation.HumanSecurityPasskeyList => new(PlatformPlane.Human, "security.passkey.list", SdkHttpMethod.Get, "/v1/security/passkeys", "Empty", "PasskeyList", false, true),
            PlatformOperation.HumanSecurityPasskeyRegisterBegin => new(PlatformPlane.Human, "security.passkey.register.begin", SdkHttpMethod.Post, "/v1/security/passkeys/registrations", "SecurityPasskeyRegistrationBegin", "PasskeyRegistrationChallenge", false, false),
            PlatformOperation.HumanSecurityPasskeyRegisterFinish => new(PlatformPlane.Human, "security.passkey.register.finish", SdkHttpMethod.Post, "/v1/security/passkeys/registrations/{registration_id}", "SecurityPasskeyRegistrationFinish", "Passkey", false, false),
            PlatformOperation.HumanSecurityPasskeyRevoke => new(PlatformPlane.Human, "security.passkey.revoke", SdkHttpMethod.Post, "/v1/security/passkeys/{passkey_id}/revoke", "SecurityPasskeyRevocation", "PasskeyList", false, false),
            PlatformOperation.HumanSecurityRecoveryReveal => new(PlatformPlane.Human, "security.recovery.reveal", SdkHttpMethod.Post, "/v1/security/recovery/evidence", "SecurityRecoveryReveal", "TimedSecret", false, false),
            PlatformOperation.HumanSecuritySessionRevoke => new(PlatformPlane.Human, "security.session.revoke", SdkHttpMethod.Post, "/v1/security/sessions/{session_id}/revoke", "SecuritySessionRevocation", "SessionRevocation", true, false),
            PlatformOperation.HumanSecuritySessionRevokeAll => new(PlatformPlane.Human, "security.session.revoke-all", SdkHttpMethod.Post, "/v1/security/sessions/revoke-all", "SecuritySessionRevocation", "SessionRevocation", true, false),
            PlatformOperation.HumanSessionList => new(PlatformPlane.Human, "session.list", SdkHttpMethod.Get, "/v1/sessions", "Empty", "SessionList", false, true),
            PlatformOperation.HumanSessionOpen => new(PlatformPlane.Human, "session.open", SdkHttpMethod.Post, "/v1/sessions", "SessionOpenRequest", "Session", true, false),
            PlatformOperation.HumanSessionRefresh => new(PlatformPlane.Human, "session.refresh", SdkHttpMethod.Post, "/v1/sessions/refresh", "Empty", "Session", false, true),
            PlatformOperation.HumanSessionRevoke => new(PlatformPlane.Human, "session.revoke", SdkHttpMethod.Delete, "/v1/sessions/{session_id}", "Empty", "SessionRevocation", true, true),
            PlatformOperation.HumanSessionRevokeAll => new(PlatformPlane.Human, "session.revoke-all", SdkHttpMethod.Post, "/v1/sessions/revoke-all", "Empty", "SessionRevocation", true, true),
            PlatformOperation.HumanStepupBegin => new(PlatformPlane.Human, "stepup.begin", SdkHttpMethod.Post, "/v1/step-up", "StepUpRequest", "StepUpChallenge", false, false),
            PlatformOperation.HumanStepupFinish => new(PlatformPlane.Human, "stepup.finish", SdkHttpMethod.Post, "/v1/step-up/{challenge_id}", "StepUpFinish", "StepUpEvidence", false, false),
            PlatformOperation.HumanStreamNext => new(PlatformPlane.Human, "stream.next", SdkHttpMethod.Get, "/v1/stream/{cursor}", "Empty", "StreamPage", false, true),
            PlatformOperation.HumanStreamOpen => new(PlatformPlane.Human, "stream.open", SdkHttpMethod.Post, "/v1/stream", "Empty", "StreamPosition", false, true),
            PlatformOperation.HumanSupportCreate => new(PlatformPlane.Human, "support.create", SdkHttpMethod.Post, "/v1/support/conversations", "SupportCreateRequest", "SupportConversation", true, false),
            PlatformOperation.HumanSupportFeedback => new(PlatformPlane.Human, "support.feedback", SdkHttpMethod.Post, "/v1/support/conversations/{conversation_id}/feedback", "SupportFeedbackRequest", "SupportConversation", false, false),
            PlatformOperation.HumanSupportList => new(PlatformPlane.Human, "support.list", SdkHttpMethod.Get, "/v1/support/conversations", "Empty", "SupportConversationPage", false, true),
            PlatformOperation.HumanSupportRead => new(PlatformPlane.Human, "support.read", SdkHttpMethod.Post, "/v1/support/conversations/{conversation_id}/read", "SupportReadRequest", "SupportConversationStatus", false, false),
            PlatformOperation.HumanSupportReply => new(PlatformPlane.Human, "support.reply", SdkHttpMethod.Post, "/v1/support/conversations/{conversation_id}/replies", "SupportReplyRequest", "SupportConversation", true, false),
            PlatformOperation.HumanSupportStatus => new(PlatformPlane.Human, "support.status", SdkHttpMethod.Get, "/v1/support/conversations/{conversation_id}/status", "Empty", "SupportConversationStatus", false, true),
            PlatformOperation.HumanVersion => new(PlatformPlane.Human, "version", SdkHttpMethod.Get, "/v1/version", "Empty", "VersionInfo", false, true),
            PlatformOperation.HumanWithdrawClaim => new(PlatformPlane.Human, "withdraw.claim", SdkHttpMethod.Post, "/v1/withdrawals/{journey_id}/claim", "WithdrawClaimRequest", "Journey", false, false),
            PlatformOperation.HumanWithdrawStart => new(PlatformPlane.Human, "withdraw.start", SdkHttpMethod.Post, "/v1/withdrawals", "WithdrawStartRequest", "Journey", true, false),
        _ => throw new ArgumentOutOfRangeException(nameof(operation)),
    };

    public static SdkMetadata platform_sdk_dotnet()
    {
        return new("LayerX.Sdk", "0.1.0", 46, 78);
    }
}

public static class GeneratedPlatformClientExtensions
{
    public static Task<JsonValue> AgentAgentRegisterAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentAgentRegister, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentApprovalApproveAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentApprovalApprove, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentApprovalGetAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentApprovalGet, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentApprovalListAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentApprovalList, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentApprovalRejectAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentApprovalReject, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentAvailabilityFetchAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentAvailabilityFetch, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentBudgetCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentBudgetCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentBudgetFundAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentBudgetFund, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentBudgetListAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentBudgetList, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentBudgetReconciliationAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentBudgetReconciliation, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentBudgetRevokeAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentBudgetRevoke, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentCapabilityAttenuateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentCapabilityAttenuate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentCapabilityCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentCapabilityCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentCapabilityListAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentCapabilityList, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentCapabilityRevokeAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentCapabilityRevoke, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentExportOfflineAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentExportOffline, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentPrepareAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentPrepare, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramActivityAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProgramActivity, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramCallAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentProgramCall, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramDiscoverAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProgramDiscover, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramInterfaceAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProgramInterface, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramReceiptAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProgramReceipt, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProgramSimulateAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProgramSimulate, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentProjectAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentProject, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadAccountAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadAccount, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadBalanceAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadBalance, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadBatchAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadBatch, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadCheckpointAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadCheckpoint, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadHistoryAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadHistory, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadModuleStateAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadModuleState, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentReadProofBundleAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentReadProofBundle, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSessionCloseAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSessionClose, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSessionListAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentSessionList, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSessionOpenAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSessionOpen, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSessionRefreshAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSessionRefresh, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSignAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSign, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubmitAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubmit, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionAcknowledgeAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubscriptionAcknowledge, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubscriptionCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionDeleteAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubscriptionDelete, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionHealthAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentSubscriptionHealth, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionListAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentSubscriptionList, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionPauseAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubscriptionPause, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentSubscriptionResumeAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.AgentSubscriptionResume, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentTrackAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentTrack, request, pathParameters, cancellationToken);
    public static Task<JsonValue> AgentWaitAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.AgentWait, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAccountBalanceAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAccountBalance, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAccountCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAccountCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanActivityEntryAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanActivityEntry, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanActivityExportEvidenceAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanActivityExportEvidence, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanActivityExportStatementAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanActivityExportStatement, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanActivityQueryAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanActivityQuery, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentArchiveAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentArchive, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAgentGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentLimitAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentLimit, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAgentList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentPauseAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentPause, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentReclaimAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentReclaim, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentRecoverAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentRecover, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentResumeAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentResume, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAgentRotateAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanAgentRotate, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanApprovalApproveAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanApprovalApprove, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanApprovalGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanApprovalGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanApprovalListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanApprovalList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanApprovalRejectAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanApprovalReject, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAuthenticatorBackupRotateAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAuthenticatorBackupRotate, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAuthenticatorDisableAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAuthenticatorDisable, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAuthenticatorSetupBeginAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAuthenticatorSetupBegin, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAuthenticatorSetupFinishAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAuthenticatorSetupFinish, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanAuthenticatorStatusAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanAuthenticatorStatus, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanBindingRebindAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanBindingRebind, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanBindingRebindActionAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanBindingRebindAction, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanBindingStatementAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanBindingStatement, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanBindingStatusAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanBindingStatus, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanBindingSubmitAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanBindingSubmit, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanDepositConfirmAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanDepositConfirm, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanDepositStartAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanDepositStart, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanEvidenceGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanEvidenceGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanExitEligibilityAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanExitEligibility, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanExitStartAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanExitStart, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanHomeSummaryAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanHomeSummary, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanJourneyGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanJourneyGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanJourneyListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanJourneyList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanMoveCommitAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanMoveCommit, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanMoveQuoteAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanMoveQuote, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanNotificationListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanNotificationList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanNotificationPreferencesGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanNotificationPreferencesGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanNotificationPreferencesSetAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanNotificationPreferencesSet, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanNotificationReadAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanNotificationRead, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanOnboardingResumeAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanOnboardingResume, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanOnboardingStatusAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanOnboardingStatus, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanPasskeyAssertBeginAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanPasskeyAssertBegin, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanPasskeyAssertFinishAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanPasskeyAssertFinish, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanPasskeyRegisterBeginAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanPasskeyRegisterBegin, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanPasskeyRegisterFinishAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanPasskeyRegisterFinish, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanProfileGetAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanProfileGet, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanProfileUpdateAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanProfileUpdate, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityActionAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityAction, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityPasskeyListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityPasskeyList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityPasskeyRegisterBeginAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityPasskeyRegisterBegin, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityPasskeyRegisterFinishAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityPasskeyRegisterFinish, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityPasskeyRevokeAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityPasskeyRevoke, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecurityRecoveryRevealAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSecurityRecoveryReveal, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecuritySessionRevokeAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSecuritySessionRevoke, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSecuritySessionRevokeAllAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSecuritySessionRevokeAll, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSessionListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSessionList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSessionOpenAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSessionOpen, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSessionRefreshAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSessionRefresh, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSessionRevokeAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSessionRevoke, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSessionRevokeAllAsync(this PlatformClient client, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSessionRevokeAll, JsonValue.EmptyObject, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanStepupBeginAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanStepupBegin, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanStepupFinishAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanStepupFinish, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanStreamNextAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanStreamNext, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanStreamOpenAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanStreamOpen, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportCreateAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSupportCreate, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportFeedbackAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSupportFeedback, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportListAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSupportList, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportReadAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSupportRead, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportReplyAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanSupportReply, request, idempotencyKey, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanSupportStatusAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanSupportStatus, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanVersionAsync(this PlatformClient client, JsonValue? request = null, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanVersion, request ?? JsonValue.EmptyObject, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanWithdrawClaimAsync(this PlatformClient client, JsonValue request, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.ReadAsync(PlatformOperation.HumanWithdrawClaim, request, pathParameters, cancellationToken);
    public static Task<JsonValue> HumanWithdrawStartAsync(this PlatformClient client, JsonValue request, IdempotencyKey idempotencyKey, IReadOnlyDictionary<string, string>? pathParameters = null, CancellationToken cancellationToken = default) =>
        client.MutateAsync(PlatformOperation.HumanWithdrawStart, request, idempotencyKey, pathParameters, cancellationToken);
}
