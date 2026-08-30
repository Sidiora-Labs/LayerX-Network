// Code generated from the LayerX Agent API and Human API schemas. DO NOT EDIT.

package layerx

type Amount = Uint128
type BudgetLimit = Uint128
type Sequence = uint64
type TimestampSeconds = uint64

type AgentOperation string

const (
	AgentOperationAgentRegister           AgentOperation = "agent.register"
	AgentOperationApprovalApprove         AgentOperation = "approval.approve"
	AgentOperationApprovalGet             AgentOperation = "approval.get"
	AgentOperationApprovalList            AgentOperation = "approval.list"
	AgentOperationApprovalReject          AgentOperation = "approval.reject"
	AgentOperationAvailabilityFetch       AgentOperation = "availability.fetch"
	AgentOperationBudgetCreate            AgentOperation = "budget.create"
	AgentOperationBudgetFund              AgentOperation = "budget.fund"
	AgentOperationBudgetList              AgentOperation = "budget.list"
	AgentOperationBudgetReconciliation    AgentOperation = "budget.reconciliation"
	AgentOperationBudgetRevoke            AgentOperation = "budget.revoke"
	AgentOperationCapabilityAttenuate     AgentOperation = "capability.attenuate"
	AgentOperationCapabilityCreate        AgentOperation = "capability.create"
	AgentOperationCapabilityList          AgentOperation = "capability.list"
	AgentOperationCapabilityRevoke        AgentOperation = "capability.revoke"
	AgentOperationExportOffline           AgentOperation = "export.offline"
	AgentOperationPrepare                 AgentOperation = "prepare"
	AgentOperationProgramActivity         AgentOperation = "program.activity"
	AgentOperationProgramCall             AgentOperation = "program.call"
	AgentOperationProgramDiscover         AgentOperation = "program.discover"
	AgentOperationProgramInterface        AgentOperation = "program.interface"
	AgentOperationProgramReceipt          AgentOperation = "program.receipt"
	AgentOperationProgramSimulate         AgentOperation = "program.simulate"
	AgentOperationProject                 AgentOperation = "project"
	AgentOperationReadAccount             AgentOperation = "read.account"
	AgentOperationReadBalance             AgentOperation = "read.balance"
	AgentOperationReadBatch               AgentOperation = "read.batch"
	AgentOperationReadCheckpoint          AgentOperation = "read.checkpoint"
	AgentOperationReadHistory             AgentOperation = "read.history"
	AgentOperationReadModuleState         AgentOperation = "read.module_state"
	AgentOperationReadProofBundle         AgentOperation = "read.proof_bundle"
	AgentOperationSessionClose            AgentOperation = "session.close"
	AgentOperationSessionList             AgentOperation = "session.list"
	AgentOperationSessionOpen             AgentOperation = "session.open"
	AgentOperationSessionRefresh          AgentOperation = "session.refresh"
	AgentOperationSign                    AgentOperation = "sign"
	AgentOperationSubmit                  AgentOperation = "submit"
	AgentOperationSubscriptionAcknowledge AgentOperation = "subscription.acknowledge"
	AgentOperationSubscriptionCreate      AgentOperation = "subscription.create"
	AgentOperationSubscriptionDelete      AgentOperation = "subscription.delete"
	AgentOperationSubscriptionHealth      AgentOperation = "subscription.health"
	AgentOperationSubscriptionList        AgentOperation = "subscription.list"
	AgentOperationSubscriptionPause       AgentOperation = "subscription.pause"
	AgentOperationSubscriptionResume      AgentOperation = "subscription.resume"
	AgentOperationTrack                   AgentOperation = "track"
	AgentOperationWait                    AgentOperation = "wait"
)

func AllAgentOperations() []AgentOperation {
	return []AgentOperation{
		AgentOperationAgentRegister,
		AgentOperationApprovalApprove,
		AgentOperationApprovalGet,
		AgentOperationApprovalList,
		AgentOperationApprovalReject,
		AgentOperationAvailabilityFetch,
		AgentOperationBudgetCreate,
		AgentOperationBudgetFund,
		AgentOperationBudgetList,
		AgentOperationBudgetReconciliation,
		AgentOperationBudgetRevoke,
		AgentOperationCapabilityAttenuate,
		AgentOperationCapabilityCreate,
		AgentOperationCapabilityList,
		AgentOperationCapabilityRevoke,
		AgentOperationExportOffline,
		AgentOperationPrepare,
		AgentOperationProgramActivity,
		AgentOperationProgramCall,
		AgentOperationProgramDiscover,
		AgentOperationProgramInterface,
		AgentOperationProgramReceipt,
		AgentOperationProgramSimulate,
		AgentOperationProject,
		AgentOperationReadAccount,
		AgentOperationReadBalance,
		AgentOperationReadBatch,
		AgentOperationReadCheckpoint,
		AgentOperationReadHistory,
		AgentOperationReadModuleState,
		AgentOperationReadProofBundle,
		AgentOperationSessionClose,
		AgentOperationSessionList,
		AgentOperationSessionOpen,
		AgentOperationSessionRefresh,
		AgentOperationSign,
		AgentOperationSubmit,
		AgentOperationSubscriptionAcknowledge,
		AgentOperationSubscriptionCreate,
		AgentOperationSubscriptionDelete,
		AgentOperationSubscriptionHealth,
		AgentOperationSubscriptionList,
		AgentOperationSubscriptionPause,
		AgentOperationSubscriptionResume,
		AgentOperationTrack,
		AgentOperationWait,
	}
}

func (operation AgentOperation) Valid() bool {
	switch operation {
	case AgentOperationAgentRegister, AgentOperationApprovalApprove, AgentOperationApprovalGet, AgentOperationApprovalList, AgentOperationApprovalReject, AgentOperationAvailabilityFetch, AgentOperationBudgetCreate, AgentOperationBudgetFund, AgentOperationBudgetList, AgentOperationBudgetReconciliation, AgentOperationBudgetRevoke, AgentOperationCapabilityAttenuate, AgentOperationCapabilityCreate, AgentOperationCapabilityList, AgentOperationCapabilityRevoke, AgentOperationExportOffline, AgentOperationPrepare, AgentOperationProgramActivity, AgentOperationProgramCall, AgentOperationProgramDiscover, AgentOperationProgramInterface, AgentOperationProgramReceipt, AgentOperationProgramSimulate, AgentOperationProject, AgentOperationReadAccount, AgentOperationReadBalance, AgentOperationReadBatch, AgentOperationReadCheckpoint, AgentOperationReadHistory, AgentOperationReadModuleState, AgentOperationReadProofBundle, AgentOperationSessionClose, AgentOperationSessionList, AgentOperationSessionOpen, AgentOperationSessionRefresh, AgentOperationSign, AgentOperationSubmit, AgentOperationSubscriptionAcknowledge, AgentOperationSubscriptionCreate, AgentOperationSubscriptionDelete, AgentOperationSubscriptionHealth, AgentOperationSubscriptionList, AgentOperationSubscriptionPause, AgentOperationSubscriptionResume, AgentOperationTrack, AgentOperationWait:
		return true
	default:
		return false
	}
}

func (operation AgentOperation) RequiresIdempotency() bool {
	switch operation {
	case AgentOperationAgentRegister, AgentOperationApprovalApprove, AgentOperationApprovalReject, AgentOperationBudgetCreate, AgentOperationBudgetFund, AgentOperationBudgetRevoke, AgentOperationCapabilityAttenuate, AgentOperationCapabilityCreate, AgentOperationCapabilityRevoke, AgentOperationPrepare, AgentOperationProgramCall, AgentOperationSessionClose, AgentOperationSessionOpen, AgentOperationSessionRefresh, AgentOperationSign, AgentOperationSubmit, AgentOperationSubscriptionAcknowledge, AgentOperationSubscriptionCreate, AgentOperationSubscriptionDelete, AgentOperationSubscriptionPause, AgentOperationSubscriptionResume:
		return true
	default:
		return false
	}
}

type HumanOperation string

const (
	HumanOperationAccountBalance                HumanOperation = "account.balance"
	HumanOperationAccountCreate                 HumanOperation = "account.create"
	HumanOperationActivityEntry                 HumanOperation = "activity.entry"
	HumanOperationActivityExportEvidence        HumanOperation = "activity.export.evidence"
	HumanOperationActivityExportStatement       HumanOperation = "activity.export.statement"
	HumanOperationActivityQuery                 HumanOperation = "activity.query"
	HumanOperationAgentArchive                  HumanOperation = "agent.archive"
	HumanOperationAgentCreate                   HumanOperation = "agent.create"
	HumanOperationAgentGet                      HumanOperation = "agent.get"
	HumanOperationAgentLimit                    HumanOperation = "agent.limit"
	HumanOperationAgentList                     HumanOperation = "agent.list"
	HumanOperationAgentPause                    HumanOperation = "agent.pause"
	HumanOperationAgentReclaim                  HumanOperation = "agent.reclaim"
	HumanOperationAgentRecover                  HumanOperation = "agent.recover"
	HumanOperationAgentResume                   HumanOperation = "agent.resume"
	HumanOperationAgentRotate                   HumanOperation = "agent.rotate"
	HumanOperationApprovalApprove               HumanOperation = "approval.approve"
	HumanOperationApprovalGet                   HumanOperation = "approval.get"
	HumanOperationApprovalList                  HumanOperation = "approval.list"
	HumanOperationApprovalReject                HumanOperation = "approval.reject"
	HumanOperationAuthenticatorBackupRotate     HumanOperation = "authenticator.backup.rotate"
	HumanOperationAuthenticatorDisable          HumanOperation = "authenticator.disable"
	HumanOperationAuthenticatorSetupBegin       HumanOperation = "authenticator.setup.begin"
	HumanOperationAuthenticatorSetupFinish      HumanOperation = "authenticator.setup.finish"
	HumanOperationAuthenticatorStatus           HumanOperation = "authenticator.status"
	HumanOperationBindingRebind                 HumanOperation = "binding.rebind"
	HumanOperationBindingRebindAction           HumanOperation = "binding.rebind.action"
	HumanOperationBindingStatement              HumanOperation = "binding.statement"
	HumanOperationBindingStatus                 HumanOperation = "binding.status"
	HumanOperationBindingSubmit                 HumanOperation = "binding.submit"
	HumanOperationDepositConfirm                HumanOperation = "deposit.confirm"
	HumanOperationDepositStart                  HumanOperation = "deposit.start"
	HumanOperationEvidenceGet                   HumanOperation = "evidence.get"
	HumanOperationExitEligibility               HumanOperation = "exit.eligibility"
	HumanOperationExitStart                     HumanOperation = "exit.start"
	HumanOperationHomeSummary                   HumanOperation = "home.summary"
	HumanOperationJourneyGet                    HumanOperation = "journey.get"
	HumanOperationJourneyList                   HumanOperation = "journey.list"
	HumanOperationMoveCommit                    HumanOperation = "move.commit"
	HumanOperationMoveQuote                     HumanOperation = "move.quote"
	HumanOperationNotificationList              HumanOperation = "notification.list"
	HumanOperationNotificationPreferencesGet    HumanOperation = "notification.preferences.get"
	HumanOperationNotificationPreferencesSet    HumanOperation = "notification.preferences.set"
	HumanOperationNotificationRead              HumanOperation = "notification.read"
	HumanOperationOnboardingResume              HumanOperation = "onboarding.resume"
	HumanOperationOnboardingStatus              HumanOperation = "onboarding.status"
	HumanOperationPasskeyAssertBegin            HumanOperation = "passkey.assert.begin"
	HumanOperationPasskeyAssertFinish           HumanOperation = "passkey.assert.finish"
	HumanOperationPasskeyRegisterBegin          HumanOperation = "passkey.register.begin"
	HumanOperationPasskeyRegisterFinish         HumanOperation = "passkey.register.finish"
	HumanOperationProfileGet                    HumanOperation = "profile.get"
	HumanOperationProfileUpdate                 HumanOperation = "profile.update"
	HumanOperationSecurityAction                HumanOperation = "security.action"
	HumanOperationSecurityPasskeyList           HumanOperation = "security.passkey.list"
	HumanOperationSecurityPasskeyRegisterBegin  HumanOperation = "security.passkey.register.begin"
	HumanOperationSecurityPasskeyRegisterFinish HumanOperation = "security.passkey.register.finish"
	HumanOperationSecurityPasskeyRevoke         HumanOperation = "security.passkey.revoke"
	HumanOperationSecurityRecoveryReveal        HumanOperation = "security.recovery.reveal"
	HumanOperationSecuritySessionRevoke         HumanOperation = "security.session.revoke"
	HumanOperationSecuritySessionRevokeAll      HumanOperation = "security.session.revoke-all"
	HumanOperationSessionList                   HumanOperation = "session.list"
	HumanOperationSessionOpen                   HumanOperation = "session.open"
	HumanOperationSessionRefresh                HumanOperation = "session.refresh"
	HumanOperationSessionRevoke                 HumanOperation = "session.revoke"
	HumanOperationSessionRevokeAll              HumanOperation = "session.revoke-all"
	HumanOperationStepupBegin                   HumanOperation = "stepup.begin"
	HumanOperationStepupFinish                  HumanOperation = "stepup.finish"
	HumanOperationStreamNext                    HumanOperation = "stream.next"
	HumanOperationStreamOpen                    HumanOperation = "stream.open"
	HumanOperationSupportCreate                 HumanOperation = "support.create"
	HumanOperationSupportFeedback               HumanOperation = "support.feedback"
	HumanOperationSupportList                   HumanOperation = "support.list"
	HumanOperationSupportRead                   HumanOperation = "support.read"
	HumanOperationSupportReply                  HumanOperation = "support.reply"
	HumanOperationSupportStatus                 HumanOperation = "support.status"
	HumanOperationVersion                       HumanOperation = "version"
	HumanOperationWithdrawClaim                 HumanOperation = "withdraw.claim"
	HumanOperationWithdrawStart                 HumanOperation = "withdraw.start"
)

func AllHumanOperations() []HumanOperation {
	return []HumanOperation{
		HumanOperationAccountBalance,
		HumanOperationAccountCreate,
		HumanOperationActivityEntry,
		HumanOperationActivityExportEvidence,
		HumanOperationActivityExportStatement,
		HumanOperationActivityQuery,
		HumanOperationAgentArchive,
		HumanOperationAgentCreate,
		HumanOperationAgentGet,
		HumanOperationAgentLimit,
		HumanOperationAgentList,
		HumanOperationAgentPause,
		HumanOperationAgentReclaim,
		HumanOperationAgentRecover,
		HumanOperationAgentResume,
		HumanOperationAgentRotate,
		HumanOperationApprovalApprove,
		HumanOperationApprovalGet,
		HumanOperationApprovalList,
		HumanOperationApprovalReject,
		HumanOperationAuthenticatorBackupRotate,
		HumanOperationAuthenticatorDisable,
		HumanOperationAuthenticatorSetupBegin,
		HumanOperationAuthenticatorSetupFinish,
		HumanOperationAuthenticatorStatus,
		HumanOperationBindingRebind,
		HumanOperationBindingRebindAction,
		HumanOperationBindingStatement,
		HumanOperationBindingStatus,
		HumanOperationBindingSubmit,
		HumanOperationDepositConfirm,
		HumanOperationDepositStart,
		HumanOperationEvidenceGet,
		HumanOperationExitEligibility,
		HumanOperationExitStart,
		HumanOperationHomeSummary,
		HumanOperationJourneyGet,
		HumanOperationJourneyList,
		HumanOperationMoveCommit,
		HumanOperationMoveQuote,
		HumanOperationNotificationList,
		HumanOperationNotificationPreferencesGet,
		HumanOperationNotificationPreferencesSet,
		HumanOperationNotificationRead,
		HumanOperationOnboardingResume,
		HumanOperationOnboardingStatus,
		HumanOperationPasskeyAssertBegin,
		HumanOperationPasskeyAssertFinish,
		HumanOperationPasskeyRegisterBegin,
		HumanOperationPasskeyRegisterFinish,
		HumanOperationProfileGet,
		HumanOperationProfileUpdate,
		HumanOperationSecurityAction,
		HumanOperationSecurityPasskeyList,
		HumanOperationSecurityPasskeyRegisterBegin,
		HumanOperationSecurityPasskeyRegisterFinish,
		HumanOperationSecurityPasskeyRevoke,
		HumanOperationSecurityRecoveryReveal,
		HumanOperationSecuritySessionRevoke,
		HumanOperationSecuritySessionRevokeAll,
		HumanOperationSessionList,
		HumanOperationSessionOpen,
		HumanOperationSessionRefresh,
		HumanOperationSessionRevoke,
		HumanOperationSessionRevokeAll,
		HumanOperationStepupBegin,
		HumanOperationStepupFinish,
		HumanOperationStreamNext,
		HumanOperationStreamOpen,
		HumanOperationSupportCreate,
		HumanOperationSupportFeedback,
		HumanOperationSupportList,
		HumanOperationSupportRead,
		HumanOperationSupportReply,
		HumanOperationSupportStatus,
		HumanOperationVersion,
		HumanOperationWithdrawClaim,
		HumanOperationWithdrawStart,
	}
}

func (operation HumanOperation) Valid() bool {
	switch operation {
	case HumanOperationAccountBalance, HumanOperationAccountCreate, HumanOperationActivityEntry, HumanOperationActivityExportEvidence, HumanOperationActivityExportStatement, HumanOperationActivityQuery, HumanOperationAgentArchive, HumanOperationAgentCreate, HumanOperationAgentGet, HumanOperationAgentLimit, HumanOperationAgentList, HumanOperationAgentPause, HumanOperationAgentReclaim, HumanOperationAgentRecover, HumanOperationAgentResume, HumanOperationAgentRotate, HumanOperationApprovalApprove, HumanOperationApprovalGet, HumanOperationApprovalList, HumanOperationApprovalReject, HumanOperationAuthenticatorBackupRotate, HumanOperationAuthenticatorDisable, HumanOperationAuthenticatorSetupBegin, HumanOperationAuthenticatorSetupFinish, HumanOperationAuthenticatorStatus, HumanOperationBindingRebind, HumanOperationBindingRebindAction, HumanOperationBindingStatement, HumanOperationBindingStatus, HumanOperationBindingSubmit, HumanOperationDepositConfirm, HumanOperationDepositStart, HumanOperationEvidenceGet, HumanOperationExitEligibility, HumanOperationExitStart, HumanOperationHomeSummary, HumanOperationJourneyGet, HumanOperationJourneyList, HumanOperationMoveCommit, HumanOperationMoveQuote, HumanOperationNotificationList, HumanOperationNotificationPreferencesGet, HumanOperationNotificationPreferencesSet, HumanOperationNotificationRead, HumanOperationOnboardingResume, HumanOperationOnboardingStatus, HumanOperationPasskeyAssertBegin, HumanOperationPasskeyAssertFinish, HumanOperationPasskeyRegisterBegin, HumanOperationPasskeyRegisterFinish, HumanOperationProfileGet, HumanOperationProfileUpdate, HumanOperationSecurityAction, HumanOperationSecurityPasskeyList, HumanOperationSecurityPasskeyRegisterBegin, HumanOperationSecurityPasskeyRegisterFinish, HumanOperationSecurityPasskeyRevoke, HumanOperationSecurityRecoveryReveal, HumanOperationSecuritySessionRevoke, HumanOperationSecuritySessionRevokeAll, HumanOperationSessionList, HumanOperationSessionOpen, HumanOperationSessionRefresh, HumanOperationSessionRevoke, HumanOperationSessionRevokeAll, HumanOperationStepupBegin, HumanOperationStepupFinish, HumanOperationStreamNext, HumanOperationStreamOpen, HumanOperationSupportCreate, HumanOperationSupportFeedback, HumanOperationSupportList, HumanOperationSupportRead, HumanOperationSupportReply, HumanOperationSupportStatus, HumanOperationVersion, HumanOperationWithdrawClaim, HumanOperationWithdrawStart:
		return true
	default:
		return false
	}
}

func (operation HumanOperation) RequiresIdempotency() bool {
	switch operation {
	case HumanOperationAccountCreate, HumanOperationActivityExportEvidence, HumanOperationActivityExportStatement, HumanOperationAgentArchive, HumanOperationAgentCreate, HumanOperationAgentLimit, HumanOperationAgentPause, HumanOperationAgentReclaim, HumanOperationAgentRecover, HumanOperationAgentResume, HumanOperationAgentRotate, HumanOperationApprovalApprove, HumanOperationApprovalReject, HumanOperationBindingRebind, HumanOperationBindingSubmit, HumanOperationDepositStart, HumanOperationExitStart, HumanOperationMoveCommit, HumanOperationSupportCreate, HumanOperationSupportReply, HumanOperationWithdrawStart:
		return true
	default:
		return false
	}
}

type HumanOperationMetadata struct {
	Method   string
	Path     string
	Request  string
	Response string
}

func (operation HumanOperation) Metadata() (HumanOperationMetadata, bool) {
	switch operation {
	case HumanOperationAccountBalance:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/account/balance", Request: "Empty", Response: "AccountBalance"}, true
	case HumanOperationAccountCreate:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/accounts", Request: "AccountCreateRequest", Response: "AccountCreation"}, true
	case HumanOperationActivityEntry:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/activity/{entry_id}", Request: "Empty", Response: "ActivityEntryDetail"}, true
	case HumanOperationActivityExportEvidence:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/activity/exports/evidence", Request: "ExportEvidenceRequest", Response: "ExportArtefact"}, true
	case HumanOperationActivityExportStatement:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/activity/exports/statement", Request: "ExportStatementRequest", Response: "ExportArtefact"}, true
	case HumanOperationActivityQuery:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/activity/query", Request: "ActivityQueryRequest", Response: "ActivityPage"}, true
	case HumanOperationAgentArchive:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/archive", Request: "AgentArchiveRequest", Response: "Journey"}, true
	case HumanOperationAgentCreate:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents", Request: "AgentCreateRequest", Response: "Journey"}, true
	case HumanOperationAgentGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/agents/{agent_id}", Request: "Empty", Response: "Agent"}, true
	case HumanOperationAgentLimit:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/limit", Request: "AgentLimitRequest", Response: "Agent"}, true
	case HumanOperationAgentList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/agents", Request: "Empty", Response: "AgentPage"}, true
	case HumanOperationAgentPause:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/pause", Request: "Empty", Response: "Agent"}, true
	case HumanOperationAgentReclaim:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/reclaim", Request: "AgentReclaimRequest", Response: "Journey"}, true
	case HumanOperationAgentRecover:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/recover", Request: "Empty", Response: "KeyChallenge"}, true
	case HumanOperationAgentResume:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/resume", Request: "Empty", Response: "Agent"}, true
	case HumanOperationAgentRotate:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/agents/{agent_id}/rotate", Request: "Empty", Response: "KeyChallenge"}, true
	case HumanOperationApprovalApprove:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/approvals/{approval_id}/approve", Request: "ApprovalApproveRequest", Response: "ApprovalDecision"}, true
	case HumanOperationApprovalGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/approvals/{approval_id}", Request: "Empty", Response: "ApprovalDetail"}, true
	case HumanOperationApprovalList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/approvals", Request: "Empty", Response: "ApprovalPage"}, true
	case HumanOperationApprovalReject:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/approvals/{approval_id}/reject", Request: "Empty", Response: "ApprovalDecision"}, true
	case HumanOperationAuthenticatorBackupRotate:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/authenticators/backup-codes", Request: "BackupCodeRotation", Response: "BackupCodeSet"}, true
	case HumanOperationAuthenticatorDisable:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/authenticators/{authenticator_id}/disable", Request: "AuthenticatorDisable", Response: "AuthenticatorStatus"}, true
	case HumanOperationAuthenticatorSetupBegin:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/authenticators/setups", Request: "AuthenticatorSetupBegin", Response: "AuthenticatorSetupChallenge"}, true
	case HumanOperationAuthenticatorSetupFinish:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/authenticators/setups/{setup_id}", Request: "AuthenticatorSetupFinish", Response: "AuthenticatorSetupResult"}, true
	case HumanOperationAuthenticatorStatus:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/security/authenticators", Request: "Empty", Response: "AuthenticatorStatus"}, true
	case HumanOperationBindingRebind:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/wallet-binding/rebind", Request: "RebindingSubmission", Response: "Journey"}, true
	case HumanOperationBindingRebindAction:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/wallet-binding/rebind/action", Request: "BindingStatementRequest", Response: "BindingRebindAction"}, true
	case HumanOperationBindingStatement:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/wallet-binding/statement", Request: "BindingStatementRequest", Response: "BindingStatement"}, true
	case HumanOperationBindingStatus:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/wallet-binding", Request: "Empty", Response: "WalletBinding"}, true
	case HumanOperationBindingSubmit:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/wallet-binding", Request: "BindingSubmission", Response: "Journey"}, true
	case HumanOperationDepositConfirm:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/deposits/{journey_id}/confirm", Request: "DepositConfirmRequest", Response: "Journey"}, true
	case HumanOperationDepositStart:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/deposits", Request: "DepositStartRequest", Response: "Journey"}, true
	case HumanOperationEvidenceGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/evidence/{evidence_id}", Request: "Empty", Response: "EvidenceMaterial"}, true
	case HumanOperationExitEligibility:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/exit/eligibility", Request: "Empty", Response: "ExitEligibility"}, true
	case HumanOperationExitStart:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/exit", Request: "ExitStartRequest", Response: "Journey"}, true
	case HumanOperationHomeSummary:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/home", Request: "Empty", Response: "HomeSummary"}, true
	case HumanOperationJourneyGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/journeys/{journey_id}", Request: "Empty", Response: "Journey"}, true
	case HumanOperationJourneyList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/journeys", Request: "Empty", Response: "JourneyPage"}, true
	case HumanOperationMoveCommit:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/moves", Request: "MoveCommitRequest", Response: "Journey"}, true
	case HumanOperationMoveQuote:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/moves/quote", Request: "MoveQuoteRequest", Response: "MoveQuote"}, true
	case HumanOperationNotificationList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/notifications", Request: "Empty", Response: "NotificationPage"}, true
	case HumanOperationNotificationPreferencesGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/notifications/preferences", Request: "Empty", Response: "NotificationPreferences"}, true
	case HumanOperationNotificationPreferencesSet:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/notifications/preferences", Request: "NotificationPreferences", Response: "NotificationPreferences"}, true
	case HumanOperationNotificationRead:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/notifications/{notification_id}/read", Request: "Empty", Response: "NotificationSummary"}, true
	case HumanOperationOnboardingResume:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/onboarding/resume", Request: "Empty", Response: "Journey"}, true
	case HumanOperationOnboardingStatus:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/onboarding", Request: "Empty", Response: "Journey"}, true
	case HumanOperationPasskeyAssertBegin:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/passkeys/assertions", Request: "PasskeyAssertionBegin", Response: "PasskeyAssertionChallenge"}, true
	case HumanOperationPasskeyAssertFinish:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/passkeys/assertions/{assertion_id}", Request: "PasskeyAssertionFinish", Response: "PasskeyAssertion"}, true
	case HumanOperationPasskeyRegisterBegin:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/passkeys/registrations", Request: "PasskeyRegistrationBegin", Response: "PasskeyRegistrationChallenge"}, true
	case HumanOperationPasskeyRegisterFinish:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/passkeys/registrations/{registration_id}", Request: "PasskeyRegistrationFinish", Response: "Passkey"}, true
	case HumanOperationProfileGet:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/profile", Request: "Empty", Response: "Profile"}, true
	case HumanOperationProfileUpdate:
		return HumanOperationMetadata{Method: "PATCH", Path: "/v1/profile", Request: "ProfileUpdate", Response: "Profile"}, true
	case HumanOperationSecurityAction:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/actions", Request: "SecurityActionRequest", Response: "SecurityAction"}, true
	case HumanOperationSecurityPasskeyList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/security/passkeys", Request: "Empty", Response: "PasskeyList"}, true
	case HumanOperationSecurityPasskeyRegisterBegin:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/passkeys/registrations", Request: "SecurityPasskeyRegistrationBegin", Response: "PasskeyRegistrationChallenge"}, true
	case HumanOperationSecurityPasskeyRegisterFinish:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/passkeys/registrations/{registration_id}", Request: "SecurityPasskeyRegistrationFinish", Response: "Passkey"}, true
	case HumanOperationSecurityPasskeyRevoke:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/passkeys/{passkey_id}/revoke", Request: "SecurityPasskeyRevocation", Response: "PasskeyList"}, true
	case HumanOperationSecurityRecoveryReveal:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/recovery/evidence", Request: "SecurityRecoveryReveal", Response: "TimedSecret"}, true
	case HumanOperationSecuritySessionRevoke:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/sessions/{session_id}/revoke", Request: "SecuritySessionRevocation", Response: "SessionRevocation"}, true
	case HumanOperationSecuritySessionRevokeAll:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/security/sessions/revoke-all", Request: "SecuritySessionRevocation", Response: "SessionRevocation"}, true
	case HumanOperationSessionList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/sessions", Request: "Empty", Response: "SessionList"}, true
	case HumanOperationSessionOpen:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/sessions", Request: "SessionOpenRequest", Response: "Session"}, true
	case HumanOperationSessionRefresh:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/sessions/refresh", Request: "Empty", Response: "Session"}, true
	case HumanOperationSessionRevoke:
		return HumanOperationMetadata{Method: "DELETE", Path: "/v1/sessions/{session_id}", Request: "Empty", Response: "SessionRevocation"}, true
	case HumanOperationSessionRevokeAll:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/sessions/revoke-all", Request: "Empty", Response: "SessionRevocation"}, true
	case HumanOperationStepupBegin:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/step-up", Request: "StepUpRequest", Response: "StepUpChallenge"}, true
	case HumanOperationStepupFinish:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/step-up/{challenge_id}", Request: "StepUpFinish", Response: "StepUpEvidence"}, true
	case HumanOperationStreamNext:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/stream/{cursor}", Request: "Empty", Response: "StreamPage"}, true
	case HumanOperationStreamOpen:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/stream", Request: "Empty", Response: "StreamPosition"}, true
	case HumanOperationSupportCreate:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/support/conversations", Request: "SupportCreateRequest", Response: "SupportConversation"}, true
	case HumanOperationSupportFeedback:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/support/conversations/{conversation_id}/feedback", Request: "SupportFeedbackRequest", Response: "SupportConversation"}, true
	case HumanOperationSupportList:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/support/conversations", Request: "Empty", Response: "SupportConversationPage"}, true
	case HumanOperationSupportRead:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/support/conversations/{conversation_id}/read", Request: "SupportReadRequest", Response: "SupportConversationStatus"}, true
	case HumanOperationSupportReply:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/support/conversations/{conversation_id}/replies", Request: "SupportReplyRequest", Response: "SupportConversation"}, true
	case HumanOperationSupportStatus:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/support/conversations/{conversation_id}/status", Request: "Empty", Response: "SupportConversationStatus"}, true
	case HumanOperationVersion:
		return HumanOperationMetadata{Method: "GET", Path: "/v1/version", Request: "Empty", Response: "VersionInfo"}, true
	case HumanOperationWithdrawClaim:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/withdrawals/{journey_id}/claim", Request: "WithdrawClaimRequest", Response: "Journey"}, true
	case HumanOperationWithdrawStart:
		return HumanOperationMetadata{Method: "POST", Path: "/v1/withdrawals", Request: "WithdrawStartRequest", Response: "Journey"}, true
	default:
		return HumanOperationMetadata{}, false
	}
}

type AgentErrorClass string

const (
	AgentErrorTransportFailure        AgentErrorClass = "TransportFailure"
	AgentErrorDeadline                AgentErrorClass = "Deadline"
	AgentErrorProtocolIncompatibility AgentErrorClass = "ProtocolIncompatibility"
	AgentErrorUnavailableCapability   AgentErrorClass = "UnavailableCapability"
	AgentErrorCoreRejection           AgentErrorClass = "CoreRejection"
	AgentErrorVerificationFailure     AgentErrorClass = "VerificationFailure"
	AgentErrorPolicyRefusal           AgentErrorClass = "PolicyRefusal"
	AgentErrorCapabilityRefusal       AgentErrorClass = "CapabilityRefusal"
	AgentErrorBudgetRefusal           AgentErrorClass = "BudgetRefusal"
	AgentErrorRateLimit               AgentErrorClass = "RateLimit"
	AgentErrorIdempotencyConflict     AgentErrorClass = "IdempotencyConflict"
	AgentErrorInternalFault           AgentErrorClass = "InternalFault"
)

func (value AgentErrorClass) Valid() bool {
	switch value {
	case AgentErrorTransportFailure, AgentErrorDeadline, AgentErrorProtocolIncompatibility, AgentErrorUnavailableCapability, AgentErrorCoreRejection, AgentErrorVerificationFailure, AgentErrorPolicyRefusal, AgentErrorCapabilityRefusal, AgentErrorBudgetRefusal, AgentErrorRateLimit, AgentErrorIdempotencyConflict, AgentErrorInternalFault:
		return true
	default:
		return false
	}
}

type HumanErrorCode string

const (
	HumanErrorUnauthenticated            HumanErrorCode = "unauthenticated"
	HumanErrorSessionExpired             HumanErrorCode = "session-expired"
	HumanErrorStepUpRequired             HumanErrorCode = "step-up-required"
	HumanErrorForbidden                  HumanErrorCode = "forbidden"
	HumanErrorNotFound                   HumanErrorCode = "not-found"
	HumanErrorInvalidRequest             HumanErrorCode = "invalid-request"
	HumanErrorConflict                   HumanErrorCode = "conflict"
	HumanErrorRateLimited                HumanErrorCode = "rate-limited"
	HumanErrorCursorExpired              HumanErrorCode = "cursor-expired"
	HumanErrorUnavailable                HumanErrorCode = "unavailable"
	HumanErrorUpstreamDegraded           HumanErrorCode = "upstream-degraded"
	HumanErrorChallengeExpired           HumanErrorCode = "challenge-expired"
	HumanErrorRefusedByPolicy            HumanErrorCode = "refused-by-policy"
	HumanErrorRefusedByBudget            HumanErrorCode = "refused-by-budget"
	HumanErrorRefusedByCapability        HumanErrorCode = "refused-by-capability"
	HumanErrorRefusedByProtocol          HumanErrorCode = "refused-by-protocol"
	HumanErrorRefusedByLimit             HumanErrorCode = "refused-by-limit"
	HumanErrorQuoteExpired               HumanErrorCode = "quote-expired"
	HumanErrorWalletNotBound             HumanErrorCode = "wallet-not-bound"
	HumanErrorExitUnavailable            HumanErrorCode = "exit-unavailable"
	HumanErrorAlreadyDecided             HumanErrorCode = "already-decided"
	HumanErrorHoldExpired                HumanErrorCode = "hold-expired"
	HumanErrorHoldDefective              HumanErrorCode = "hold-defective"
	HumanErrorArchiveNeedsDisposition    HumanErrorCode = "archive-needs-disposition"
	HumanErrorConfirmationMismatch       HumanErrorCode = "confirmation-mismatch"
	HumanErrorNotSuppressible            HumanErrorCode = "not-suppressible"
	HumanErrorSupportUnavailable         HumanErrorCode = "support-unavailable"
	HumanErrorSupportConversationUnknown HumanErrorCode = "support-conversation-unknown"
	HumanErrorSupportMessageUnknown      HumanErrorCode = "support-message-unknown"
)

func (value HumanErrorCode) Valid() bool {
	switch value {
	case HumanErrorUnauthenticated, HumanErrorSessionExpired, HumanErrorStepUpRequired, HumanErrorForbidden, HumanErrorNotFound, HumanErrorInvalidRequest, HumanErrorConflict, HumanErrorRateLimited, HumanErrorCursorExpired, HumanErrorUnavailable, HumanErrorUpstreamDegraded, HumanErrorChallengeExpired, HumanErrorRefusedByPolicy, HumanErrorRefusedByBudget, HumanErrorRefusedByCapability, HumanErrorRefusedByProtocol, HumanErrorRefusedByLimit, HumanErrorQuoteExpired, HumanErrorWalletNotBound, HumanErrorExitUnavailable, HumanErrorAlreadyDecided, HumanErrorHoldExpired, HumanErrorHoldDefective, HumanErrorArchiveNeedsDisposition, HumanErrorConfirmationMismatch, HumanErrorNotSuppressible, HumanErrorSupportUnavailable, HumanErrorSupportConversationUnknown, HumanErrorSupportMessageUnknown:
		return true
	default:
		return false
	}
}

type JourneyKind string

const (
	JourneyOnboarding    JourneyKind = "onboarding"
	JourneyWalletBinding JourneyKind = "wallet-binding"
	JourneyDeposit       JourneyKind = "deposit"
	JourneyWithdraw      JourneyKind = "withdraw"
	JourneyExit          JourneyKind = "exit"
	JourneyMove          JourneyKind = "move"
	JourneyAgentCreate   JourneyKind = "agent-create"
	JourneyAgentFund     JourneyKind = "agent-fund"
	JourneyAgentPause    JourneyKind = "agent-pause"
	JourneyAgentRetire   JourneyKind = "agent-retire"
)

func (value JourneyKind) Valid() bool {
	switch value {
	case JourneyOnboarding, JourneyWalletBinding, JourneyDeposit, JourneyWithdraw, JourneyExit, JourneyMove, JourneyAgentCreate, JourneyAgentFund, JourneyAgentPause, JourneyAgentRetire:
		return true
	default:
		return false
	}
}

type JourneyState string

const (
	JourneyStateGettingReady  JourneyState = "getting-ready"
	JourneyStateSending       JourneyState = "sending"
	JourneyStateProcessing    JourneyState = "processing"
	JourneyStateDone          JourneyState = "done"
	JourneyStateDoneFinalised JourneyState = "done-finalised"
	JourneyStateStillChecking JourneyState = "still-checking"
	JourneyStateRefused       JourneyState = "refused"
	JourneyStateWaitingForYou JourneyState = "waiting-for-you"
)

func (value JourneyState) Valid() bool {
	switch value {
	case JourneyStateGettingReady, JourneyStateSending, JourneyStateProcessing, JourneyStateDone, JourneyStateDoneFinalised, JourneyStateStillChecking, JourneyStateRefused, JourneyStateWaitingForYou:
		return true
	default:
		return false
	}
}

type HumanVerificationLevel string

const (
	HumanVerificationUnverified          HumanVerificationLevel = "unverified"
	HumanVerificationReceiptVerified     HumanVerificationLevel = "receipt-verified"
	HumanVerificationCheckpointFinalised HumanVerificationLevel = "checkpoint-finalised"
	HumanVerificationPaxeerFinalised     HumanVerificationLevel = "paxeer-finalised"
)

func (value HumanVerificationLevel) Valid() bool {
	switch value {
	case HumanVerificationUnverified, HumanVerificationReceiptVerified, HumanVerificationCheckpointFinalised, HumanVerificationPaxeerFinalised:
		return true
	default:
		return false
	}
}

type HumanRetriability string

const (
	HumanRetryRetriable      HumanRetriability = "retriable"
	HumanRetryRetriableAfter HumanRetriability = "retriable-after"
	HumanRetryStructural     HumanRetriability = "structural"
	HumanRetryFinal          HumanRetriability = "final"
)

func (value HumanRetriability) Valid() bool {
	switch value {
	case HumanRetryRetriable, HumanRetryRetriableAfter, HumanRetryStructural, HumanRetryFinal:
		return true
	default:
		return false
	}
}

type HumanApprovalState string

const (
	HumanApprovalPending   HumanApprovalState = "pending"
	HumanApprovalApproved  HumanApprovalState = "approved"
	HumanApprovalRejected  HumanApprovalState = "rejected"
	HumanApprovalExpired   HumanApprovalState = "expired"
	HumanApprovalDefective HumanApprovalState = "defective"
)

func (value HumanApprovalState) Valid() bool {
	switch value {
	case HumanApprovalPending, HumanApprovalApproved, HumanApprovalRejected, HumanApprovalExpired, HumanApprovalDefective:
		return true
	default:
		return false
	}
}

type HumanStreamEventKind string

const (
	HumanStreamEventJourneyProgress  HumanStreamEventKind = "journey-progress"
	HumanStreamEventApprovalCreated  HumanStreamEventKind = "approval-created"
	HumanStreamEventApprovalApproved HumanStreamEventKind = "approval-approved"
	HumanStreamEventApprovalRejected HumanStreamEventKind = "approval-rejected"
	HumanStreamEventApprovalExpired  HumanStreamEventKind = "approval-expired"
	HumanStreamEventNotification     HumanStreamEventKind = "notification"
)

func (value HumanStreamEventKind) Valid() bool {
	switch value {
	case HumanStreamEventJourneyProgress, HumanStreamEventApprovalCreated, HumanStreamEventApprovalApproved, HumanStreamEventApprovalRejected, HumanStreamEventApprovalExpired, HumanStreamEventNotification:
		return true
	default:
		return false
	}
}

type AgentApprovalEventKind string

const (
	AgentApprovalEventCreated   AgentApprovalEventKind = "Created"
	AgentApprovalEventGranted   AgentApprovalEventKind = "Granted"
	AgentApprovalEventRejected  AgentApprovalEventKind = "Rejected"
	AgentApprovalEventExpired   AgentApprovalEventKind = "Expired"
	AgentApprovalEventDefective AgentApprovalEventKind = "Defective"
)

func (value AgentApprovalEventKind) Valid() bool {
	switch value {
	case AgentApprovalEventCreated, AgentApprovalEventGranted, AgentApprovalEventRejected, AgentApprovalEventExpired, AgentApprovalEventDefective:
		return true
	default:
		return false
	}
}

type AgentApprovalState string

const (
	AgentApprovalStateHeld      AgentApprovalState = "Held"
	AgentApprovalStateGranted   AgentApprovalState = "Granted"
	AgentApprovalStateRejected  AgentApprovalState = "Rejected"
	AgentApprovalStateExpired   AgentApprovalState = "Expired"
	AgentApprovalStateDefective AgentApprovalState = "Defective"
)

func (value AgentApprovalState) Valid() bool {
	switch value {
	case AgentApprovalStateHeld, AgentApprovalStateGranted, AgentApprovalStateRejected, AgentApprovalStateExpired, AgentApprovalStateDefective:
		return true
	default:
		return false
	}
}

type AgentApprovalDecisionOutcome string

const (
	AgentApprovalOutcomeGranted        AgentApprovalDecisionOutcome = "Granted"
	AgentApprovalOutcomeRejected       AgentApprovalDecisionOutcome = "Rejected"
	AgentApprovalOutcomeExpired        AgentApprovalDecisionOutcome = "Expired"
	AgentApprovalOutcomeDefective      AgentApprovalDecisionOutcome = "Defective"
	AgentApprovalOutcomeAlreadyDecided AgentApprovalDecisionOutcome = "AlreadyDecided"
	AgentApprovalOutcomeConflict       AgentApprovalDecisionOutcome = "Conflict"
)

func (value AgentApprovalDecisionOutcome) Valid() bool {
	switch value {
	case AgentApprovalOutcomeGranted, AgentApprovalOutcomeRejected, AgentApprovalOutcomeExpired, AgentApprovalOutcomeDefective, AgentApprovalOutcomeAlreadyDecided, AgentApprovalOutcomeConflict:
		return true
	default:
		return false
	}
}

type AgentRetriability string

const (
	AgentRetryTerminal  AgentRetriability = "Terminal"
	AgentRetryRetriable AgentRetriability = "Retriable"
)

func (value AgentRetriability) Valid() bool {
	switch value {
	case AgentRetryTerminal, AgentRetryRetriable:
		return true
	default:
		return false
	}
}

type AgentDeliveryKind string

const (
	AgentDeliveryEvent     AgentDeliveryKind = "Event"
	AgentDeliveryGap       AgentDeliveryKind = "Gap"
	AgentDeliveryTruncated AgentDeliveryKind = "Truncated"
)

func (value AgentDeliveryKind) Valid() bool {
	switch value {
	case AgentDeliveryEvent, AgentDeliveryGap, AgentDeliveryTruncated:
		return true
	default:
		return false
	}
}
