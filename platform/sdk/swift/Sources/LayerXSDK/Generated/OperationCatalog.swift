// Generated from agent-api and human-api. Do not hand-edit.

public enum PlatformOperation: String, CaseIterable, Sendable {
    case agentAgentRegister = "agent:agent.register"
    case agentApprovalApprove = "agent:approval.approve"
    case agentApprovalGet = "agent:approval.get"
    case agentApprovalList = "agent:approval.list"
    case agentApprovalReject = "agent:approval.reject"
    case agentAvailabilityFetch = "agent:availability.fetch"
    case agentBudgetCreate = "agent:budget.create"
    case agentBudgetFund = "agent:budget.fund"
    case agentBudgetList = "agent:budget.list"
    case agentBudgetReconciliation = "agent:budget.reconciliation"
    case agentBudgetRevoke = "agent:budget.revoke"
    case agentCapabilityAttenuate = "agent:capability.attenuate"
    case agentCapabilityCreate = "agent:capability.create"
    case agentCapabilityList = "agent:capability.list"
    case agentCapabilityRevoke = "agent:capability.revoke"
    case agentExportOffline = "agent:export.offline"
    case agentPrepare = "agent:prepare"
    case agentProgramActivity = "agent:program.activity"
    case agentProgramCall = "agent:program.call"
    case agentProgramDiscover = "agent:program.discover"
    case agentProgramInterface = "agent:program.interface"
    case agentProgramReceipt = "agent:program.receipt"
    case agentProgramSimulate = "agent:program.simulate"
    case agentProject = "agent:project"
    case agentReadAccount = "agent:read.account"
    case agentReadBalance = "agent:read.balance"
    case agentReadBatch = "agent:read.batch"
    case agentReadCheckpoint = "agent:read.checkpoint"
    case agentReadHistory = "agent:read.history"
    case agentReadModuleState = "agent:read.module_state"
    case agentReadProofBundle = "agent:read.proof_bundle"
    case agentSessionClose = "agent:session.close"
    case agentSessionList = "agent:session.list"
    case agentSessionOpen = "agent:session.open"
    case agentSessionRefresh = "agent:session.refresh"
    case agentSign = "agent:sign"
    case agentSubmit = "agent:submit"
    case agentSubscriptionAcknowledge = "agent:subscription.acknowledge"
    case agentSubscriptionCreate = "agent:subscription.create"
    case agentSubscriptionDelete = "agent:subscription.delete"
    case agentSubscriptionHealth = "agent:subscription.health"
    case agentSubscriptionList = "agent:subscription.list"
    case agentSubscriptionPause = "agent:subscription.pause"
    case agentSubscriptionResume = "agent:subscription.resume"
    case agentTrack = "agent:track"
    case agentWait = "agent:wait"
    case humanAccountBalance = "human:account.balance"
    case humanAccountCreate = "human:account.create"
    case humanActivityEntry = "human:activity.entry"
    case humanActivityExportEvidence = "human:activity.export.evidence"
    case humanActivityExportStatement = "human:activity.export.statement"
    case humanActivityQuery = "human:activity.query"
    case humanAgentArchive = "human:agent.archive"
    case humanAgentCreate = "human:agent.create"
    case humanAgentGet = "human:agent.get"
    case humanAgentLimit = "human:agent.limit"
    case humanAgentList = "human:agent.list"
    case humanAgentPause = "human:agent.pause"
    case humanAgentReclaim = "human:agent.reclaim"
    case humanAgentRecover = "human:agent.recover"
    case humanAgentResume = "human:agent.resume"
    case humanAgentRotate = "human:agent.rotate"
    case humanApprovalApprove = "human:approval.approve"
    case humanApprovalGet = "human:approval.get"
    case humanApprovalList = "human:approval.list"
    case humanApprovalReject = "human:approval.reject"
    case humanAuthenticatorBackupRotate = "human:authenticator.backup.rotate"
    case humanAuthenticatorDisable = "human:authenticator.disable"
    case humanAuthenticatorSetupBegin = "human:authenticator.setup.begin"
    case humanAuthenticatorSetupFinish = "human:authenticator.setup.finish"
    case humanAuthenticatorStatus = "human:authenticator.status"
    case humanBindingRebind = "human:binding.rebind"
    case humanBindingRebindAction = "human:binding.rebind.action"
    case humanBindingStatement = "human:binding.statement"
    case humanBindingStatus = "human:binding.status"
    case humanBindingSubmit = "human:binding.submit"
    case humanDepositConfirm = "human:deposit.confirm"
    case humanDepositStart = "human:deposit.start"
    case humanEvidenceGet = "human:evidence.get"
    case humanExitEligibility = "human:exit.eligibility"
    case humanExitStart = "human:exit.start"
    case humanHomeSummary = "human:home.summary"
    case humanJourneyGet = "human:journey.get"
    case humanJourneyList = "human:journey.list"
    case humanMoveCommit = "human:move.commit"
    case humanMoveQuote = "human:move.quote"
    case humanNotificationList = "human:notification.list"
    case humanNotificationPreferencesGet = "human:notification.preferences.get"
    case humanNotificationPreferencesSet = "human:notification.preferences.set"
    case humanNotificationRead = "human:notification.read"
    case humanOnboardingResume = "human:onboarding.resume"
    case humanOnboardingStatus = "human:onboarding.status"
    case humanPasskeyAssertBegin = "human:passkey.assert.begin"
    case humanPasskeyAssertFinish = "human:passkey.assert.finish"
    case humanPasskeyRegisterBegin = "human:passkey.register.begin"
    case humanPasskeyRegisterFinish = "human:passkey.register.finish"
    case humanProfileGet = "human:profile.get"
    case humanProfileUpdate = "human:profile.update"
    case humanSecurityAction = "human:security.action"
    case humanSecurityPasskeyList = "human:security.passkey.list"
    case humanSecurityPasskeyRegisterBegin = "human:security.passkey.register.begin"
    case humanSecurityPasskeyRegisterFinish = "human:security.passkey.register.finish"
    case humanSecurityPasskeyRevoke = "human:security.passkey.revoke"
    case humanSecurityRecoveryReveal = "human:security.recovery.reveal"
    case humanSecuritySessionRevoke = "human:security.session.revoke"
    case humanSecuritySessionRevokeAll = "human:security.session.revoke-all"
    case humanSessionList = "human:session.list"
    case humanSessionOpen = "human:session.open"
    case humanSessionRefresh = "human:session.refresh"
    case humanSessionRevoke = "human:session.revoke"
    case humanSessionRevokeAll = "human:session.revoke-all"
    case humanStepupBegin = "human:stepup.begin"
    case humanStepupFinish = "human:stepup.finish"
    case humanStreamNext = "human:stream.next"
    case humanStreamOpen = "human:stream.open"
    case humanSupportCreate = "human:support.create"
    case humanSupportFeedback = "human:support.feedback"
    case humanSupportList = "human:support.list"
    case humanSupportRead = "human:support.read"
    case humanSupportReply = "human:support.reply"
    case humanSupportStatus = "human:support.status"
    case humanVersion = "human:version"
    case humanWithdrawClaim = "human:withdraw.claim"
    case humanWithdrawStart = "human:withdraw.start"

    public var descriptor: OperationDescriptor {
        switch self {
        case .agentAgentRegister: return OperationDescriptor(plane: .agent, name: "agent.register", method: .post, path: "", requestType: "AgentRegistration", responseType: "AuthorityResponse<AgentRecord>", requiresIdempotency: true, bodyless: false)
        case .agentApprovalApprove: return OperationDescriptor(plane: .agent, name: "approval.approve", method: .post, path: "", requestType: "ApprovalApproveRequest", responseType: "ApprovalDecision", requiresIdempotency: true, bodyless: false)
        case .agentApprovalGet: return OperationDescriptor(plane: .agent, name: "approval.get", method: .post, path: "", requestType: "ApprovalGetRequest", responseType: "ApprovalRecord", requiresIdempotency: false, bodyless: false)
        case .agentApprovalList: return OperationDescriptor(plane: .agent, name: "approval.list", method: .post, path: "", requestType: "ApprovalListRequest", responseType: "ApprovalPage", requiresIdempotency: false, bodyless: false)
        case .agentApprovalReject: return OperationDescriptor(plane: .agent, name: "approval.reject", method: .post, path: "", requestType: "ApprovalRejectRequest", responseType: "ApprovalDecision", requiresIdempotency: true, bodyless: false)
        case .agentAvailabilityFetch: return OperationDescriptor(plane: .agent, name: "availability.fetch", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<AvailabilityReport>", requiresIdempotency: false, bodyless: false)
        case .agentBudgetCreate: return OperationDescriptor(plane: .agent, name: "budget.create", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<BudgetRecord>", requiresIdempotency: true, bodyless: false)
        case .agentBudgetFund: return OperationDescriptor(plane: .agent, name: "budget.fund", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<BudgetRecord>", requiresIdempotency: true, bodyless: false)
        case .agentBudgetList: return OperationDescriptor(plane: .agent, name: "budget.list", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<BudgetRecords>", requiresIdempotency: false, bodyless: false)
        case .agentBudgetReconciliation: return OperationDescriptor(plane: .agent, name: "budget.reconciliation", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<BudgetReconciliation>", requiresIdempotency: false, bodyless: false)
        case .agentBudgetRevoke: return OperationDescriptor(plane: .agent, name: "budget.revoke", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<BudgetRecord>", requiresIdempotency: true, bodyless: false)
        case .agentCapabilityAttenuate: return OperationDescriptor(plane: .agent, name: "capability.attenuate", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<CapabilityRecord>", requiresIdempotency: true, bodyless: false)
        case .agentCapabilityCreate: return OperationDescriptor(plane: .agent, name: "capability.create", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<CapabilityRecord>", requiresIdempotency: true, bodyless: false)
        case .agentCapabilityList: return OperationDescriptor(plane: .agent, name: "capability.list", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<CapabilityRecords>", requiresIdempotency: false, bodyless: false)
        case .agentCapabilityRevoke: return OperationDescriptor(plane: .agent, name: "capability.revoke", method: .post, path: "", requestType: "object", responseType: "AuthorityResponse<CapabilityRecord>", requiresIdempotency: true, bodyless: false)
        case .agentExportOffline: return OperationDescriptor(plane: .agent, name: "export.offline", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<OfflineExport>", requiresIdempotency: false, bodyless: false)
        case .agentPrepare: return OperationDescriptor(plane: .agent, name: "prepare", method: .post, path: "", requestType: "PrepareRequest", responseType: "Prepared", requiresIdempotency: true, bodyless: false)
        case .agentProgramActivity: return OperationDescriptor(plane: .agent, name: "program.activity", method: .get, path: "/v1/programs/activities/{activity_id}", requestType: "ProgramActivitySelector", responseType: "ProgramSubmission", requiresIdempotency: false, bodyless: false)
        case .agentProgramCall: return OperationDescriptor(plane: .agent, name: "program.call", method: .post, path: "/v1/programs/call", requestType: "ProgramCallRequest", responseType: "ProgramSubmission", requiresIdempotency: true, bodyless: false)
        case .agentProgramDiscover: return OperationDescriptor(plane: .agent, name: "program.discover", method: .get, path: "/v1/programs/registry/{program_id}", requestType: "ProgramSelector", responseType: "VerifiedProgramDiscovery", requiresIdempotency: false, bodyless: false)
        case .agentProgramInterface: return OperationDescriptor(plane: .agent, name: "program.interface", method: .get, path: "/v1/programs/registry/{program_id}/interface", requestType: "ProgramSelector", responseType: "VerifiedProgramInterface", requiresIdempotency: false, bodyless: false)
        case .agentProgramReceipt: return OperationDescriptor(plane: .agent, name: "program.receipt", method: .get, path: "/v1/programs/receipts/by-idempotency/{idempotency_key}", requestType: "ProgramReceiptSelector", responseType: "ProgramSubmission", requiresIdempotency: false, bodyless: false)
        case .agentProgramSimulate: return OperationDescriptor(plane: .agent, name: "program.simulate", method: .post, path: "/v1/programs/simulate", requestType: "ProgramCallRequest", responseType: "ProgramSimulation", requiresIdempotency: false, bodyless: false)
        case .agentProject: return OperationDescriptor(plane: .agent, name: "project", method: .post, path: "", requestType: "object", responseType: "ProjectionResult", requiresIdempotency: false, bodyless: false)
        case .agentReadAccount: return OperationDescriptor(plane: .agent, name: "read.account", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<AccountValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadBalance: return OperationDescriptor(plane: .agent, name: "read.balance", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<BalanceValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadBatch: return OperationDescriptor(plane: .agent, name: "read.batch", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<BatchValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadCheckpoint: return OperationDescriptor(plane: .agent, name: "read.checkpoint", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<CheckpointValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadHistory: return OperationDescriptor(plane: .agent, name: "read.history", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<HistoryValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadModuleState: return OperationDescriptor(plane: .agent, name: "read.module_state", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<ModuleStateValue>", requiresIdempotency: false, bodyless: false)
        case .agentReadProofBundle: return OperationDescriptor(plane: .agent, name: "read.proof_bundle", method: .post, path: "", requestType: "object", responseType: "VerifiedRead<ProofBundle>", requiresIdempotency: false, bodyless: false)
        case .agentSessionClose: return OperationDescriptor(plane: .agent, name: "session.close", method: .post, path: "", requestType: "SessionClose", responseType: "AuthorityResponse<SessionRecord>", requiresIdempotency: true, bodyless: false)
        case .agentSessionList: return OperationDescriptor(plane: .agent, name: "session.list", method: .post, path: "", requestType: "SessionList", responseType: "AuthorityResponse<SessionRecords>", requiresIdempotency: false, bodyless: false)
        case .agentSessionOpen: return OperationDescriptor(plane: .agent, name: "session.open", method: .post, path: "", requestType: "SessionOpen", responseType: "AuthorityResponse<SessionRecord>", requiresIdempotency: true, bodyless: false)
        case .agentSessionRefresh: return OperationDescriptor(plane: .agent, name: "session.refresh", method: .post, path: "", requestType: "SessionRefresh", responseType: "AuthorityResponse<SessionRecord>", requiresIdempotency: true, bodyless: false)
        case .agentSign: return OperationDescriptor(plane: .agent, name: "sign", method: .post, path: "", requestType: "SignRequest", responseType: "Signed", requiresIdempotency: true, bodyless: false)
        case .agentSubmit: return OperationDescriptor(plane: .agent, name: "submit", method: .post, path: "", requestType: "SubmitRequest", responseType: "TrackedSubmission", requiresIdempotency: true, bodyless: false)
        case .agentSubscriptionAcknowledge: return OperationDescriptor(plane: .agent, name: "subscription.acknowledge", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: true, bodyless: false)
        case .agentSubscriptionCreate: return OperationDescriptor(plane: .agent, name: "subscription.create", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: true, bodyless: false)
        case .agentSubscriptionDelete: return OperationDescriptor(plane: .agent, name: "subscription.delete", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: true, bodyless: false)
        case .agentSubscriptionHealth: return OperationDescriptor(plane: .agent, name: "subscription.health", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: false, bodyless: false)
        case .agentSubscriptionList: return OperationDescriptor(plane: .agent, name: "subscription.list", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: false, bodyless: false)
        case .agentSubscriptionPause: return OperationDescriptor(plane: .agent, name: "subscription.pause", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: true, bodyless: false)
        case .agentSubscriptionResume: return OperationDescriptor(plane: .agent, name: "subscription.resume", method: .post, path: "", requestType: "object", responseType: "object", requiresIdempotency: true, bodyless: false)
        case .agentTrack: return OperationDescriptor(plane: .agent, name: "track", method: .post, path: "", requestType: "TrackRequest", responseType: "TrackedSubmission", requiresIdempotency: false, bodyless: false)
        case .agentWait: return OperationDescriptor(plane: .agent, name: "wait", method: .post, path: "", requestType: "WaitRequest", responseType: "WaitResult", requiresIdempotency: false, bodyless: false)
        case .humanAccountBalance: return OperationDescriptor(plane: .human, name: "account.balance", method: .get, path: "/v1/account/balance", requestType: "Empty", responseType: "AccountBalance", requiresIdempotency: false, bodyless: true)
        case .humanAccountCreate: return OperationDescriptor(plane: .human, name: "account.create", method: .post, path: "/v1/accounts", requestType: "AccountCreateRequest", responseType: "AccountCreation", requiresIdempotency: true, bodyless: false)
        case .humanActivityEntry: return OperationDescriptor(plane: .human, name: "activity.entry", method: .get, path: "/v1/activity/{entry_id}", requestType: "Empty", responseType: "ActivityEntryDetail", requiresIdempotency: false, bodyless: true)
        case .humanActivityExportEvidence: return OperationDescriptor(plane: .human, name: "activity.export.evidence", method: .post, path: "/v1/activity/exports/evidence", requestType: "ExportEvidenceRequest", responseType: "ExportArtefact", requiresIdempotency: true, bodyless: false)
        case .humanActivityExportStatement: return OperationDescriptor(plane: .human, name: "activity.export.statement", method: .post, path: "/v1/activity/exports/statement", requestType: "ExportStatementRequest", responseType: "ExportArtefact", requiresIdempotency: true, bodyless: false)
        case .humanActivityQuery: return OperationDescriptor(plane: .human, name: "activity.query", method: .post, path: "/v1/activity/query", requestType: "ActivityQueryRequest", responseType: "ActivityPage", requiresIdempotency: false, bodyless: false)
        case .humanAgentArchive: return OperationDescriptor(plane: .human, name: "agent.archive", method: .post, path: "/v1/agents/{agent_id}/archive", requestType: "AgentArchiveRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanAgentCreate: return OperationDescriptor(plane: .human, name: "agent.create", method: .post, path: "/v1/agents", requestType: "AgentCreateRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanAgentGet: return OperationDescriptor(plane: .human, name: "agent.get", method: .get, path: "/v1/agents/{agent_id}", requestType: "Empty", responseType: "Agent", requiresIdempotency: false, bodyless: true)
        case .humanAgentLimit: return OperationDescriptor(plane: .human, name: "agent.limit", method: .post, path: "/v1/agents/{agent_id}/limit", requestType: "AgentLimitRequest", responseType: "Agent", requiresIdempotency: true, bodyless: false)
        case .humanAgentList: return OperationDescriptor(plane: .human, name: "agent.list", method: .get, path: "/v1/agents", requestType: "Empty", responseType: "AgentPage", requiresIdempotency: false, bodyless: true)
        case .humanAgentPause: return OperationDescriptor(plane: .human, name: "agent.pause", method: .post, path: "/v1/agents/{agent_id}/pause", requestType: "Empty", responseType: "Agent", requiresIdempotency: true, bodyless: true)
        case .humanAgentReclaim: return OperationDescriptor(plane: .human, name: "agent.reclaim", method: .post, path: "/v1/agents/{agent_id}/reclaim", requestType: "AgentReclaimRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanAgentRecover: return OperationDescriptor(plane: .human, name: "agent.recover", method: .post, path: "/v1/agents/{agent_id}/recover", requestType: "Empty", responseType: "KeyChallenge", requiresIdempotency: true, bodyless: true)
        case .humanAgentResume: return OperationDescriptor(plane: .human, name: "agent.resume", method: .post, path: "/v1/agents/{agent_id}/resume", requestType: "Empty", responseType: "Agent", requiresIdempotency: true, bodyless: true)
        case .humanAgentRotate: return OperationDescriptor(plane: .human, name: "agent.rotate", method: .post, path: "/v1/agents/{agent_id}/rotate", requestType: "Empty", responseType: "KeyChallenge", requiresIdempotency: true, bodyless: true)
        case .humanApprovalApprove: return OperationDescriptor(plane: .human, name: "approval.approve", method: .post, path: "/v1/approvals/{approval_id}/approve", requestType: "ApprovalApproveRequest", responseType: "ApprovalDecision", requiresIdempotency: true, bodyless: false)
        case .humanApprovalGet: return OperationDescriptor(plane: .human, name: "approval.get", method: .get, path: "/v1/approvals/{approval_id}", requestType: "Empty", responseType: "ApprovalDetail", requiresIdempotency: false, bodyless: true)
        case .humanApprovalList: return OperationDescriptor(plane: .human, name: "approval.list", method: .get, path: "/v1/approvals", requestType: "Empty", responseType: "ApprovalPage", requiresIdempotency: false, bodyless: true)
        case .humanApprovalReject: return OperationDescriptor(plane: .human, name: "approval.reject", method: .post, path: "/v1/approvals/{approval_id}/reject", requestType: "Empty", responseType: "ApprovalDecision", requiresIdempotency: true, bodyless: true)
        case .humanAuthenticatorBackupRotate: return OperationDescriptor(plane: .human, name: "authenticator.backup.rotate", method: .post, path: "/v1/security/authenticators/backup-codes", requestType: "BackupCodeRotation", responseType: "BackupCodeSet", requiresIdempotency: false, bodyless: false)
        case .humanAuthenticatorDisable: return OperationDescriptor(plane: .human, name: "authenticator.disable", method: .post, path: "/v1/security/authenticators/{authenticator_id}/disable", requestType: "AuthenticatorDisable", responseType: "AuthenticatorStatus", requiresIdempotency: false, bodyless: false)
        case .humanAuthenticatorSetupBegin: return OperationDescriptor(plane: .human, name: "authenticator.setup.begin", method: .post, path: "/v1/security/authenticators/setups", requestType: "AuthenticatorSetupBegin", responseType: "AuthenticatorSetupChallenge", requiresIdempotency: false, bodyless: false)
        case .humanAuthenticatorSetupFinish: return OperationDescriptor(plane: .human, name: "authenticator.setup.finish", method: .post, path: "/v1/security/authenticators/setups/{setup_id}", requestType: "AuthenticatorSetupFinish", responseType: "AuthenticatorSetupResult", requiresIdempotency: false, bodyless: false)
        case .humanAuthenticatorStatus: return OperationDescriptor(plane: .human, name: "authenticator.status", method: .get, path: "/v1/security/authenticators", requestType: "Empty", responseType: "AuthenticatorStatus", requiresIdempotency: false, bodyless: true)
        case .humanBindingRebind: return OperationDescriptor(plane: .human, name: "binding.rebind", method: .post, path: "/v1/wallet-binding/rebind", requestType: "RebindingSubmission", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanBindingRebindAction: return OperationDescriptor(plane: .human, name: "binding.rebind.action", method: .post, path: "/v1/wallet-binding/rebind/action", requestType: "BindingStatementRequest", responseType: "BindingRebindAction", requiresIdempotency: false, bodyless: false)
        case .humanBindingStatement: return OperationDescriptor(plane: .human, name: "binding.statement", method: .post, path: "/v1/wallet-binding/statement", requestType: "BindingStatementRequest", responseType: "BindingStatement", requiresIdempotency: false, bodyless: false)
        case .humanBindingStatus: return OperationDescriptor(plane: .human, name: "binding.status", method: .get, path: "/v1/wallet-binding", requestType: "Empty", responseType: "WalletBinding", requiresIdempotency: false, bodyless: true)
        case .humanBindingSubmit: return OperationDescriptor(plane: .human, name: "binding.submit", method: .post, path: "/v1/wallet-binding", requestType: "BindingSubmission", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanDepositConfirm: return OperationDescriptor(plane: .human, name: "deposit.confirm", method: .post, path: "/v1/deposits/{journey_id}/confirm", requestType: "DepositConfirmRequest", responseType: "Journey", requiresIdempotency: false, bodyless: false)
        case .humanDepositStart: return OperationDescriptor(plane: .human, name: "deposit.start", method: .post, path: "/v1/deposits", requestType: "DepositStartRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanEvidenceGet: return OperationDescriptor(plane: .human, name: "evidence.get", method: .get, path: "/v1/evidence/{evidence_id}", requestType: "Empty", responseType: "EvidenceMaterial", requiresIdempotency: false, bodyless: true)
        case .humanExitEligibility: return OperationDescriptor(plane: .human, name: "exit.eligibility", method: .get, path: "/v1/exit/eligibility", requestType: "Empty", responseType: "ExitEligibility", requiresIdempotency: false, bodyless: true)
        case .humanExitStart: return OperationDescriptor(plane: .human, name: "exit.start", method: .post, path: "/v1/exit", requestType: "ExitStartRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanHomeSummary: return OperationDescriptor(plane: .human, name: "home.summary", method: .get, path: "/v1/home", requestType: "Empty", responseType: "HomeSummary", requiresIdempotency: false, bodyless: true)
        case .humanJourneyGet: return OperationDescriptor(plane: .human, name: "journey.get", method: .get, path: "/v1/journeys/{journey_id}", requestType: "Empty", responseType: "Journey", requiresIdempotency: false, bodyless: true)
        case .humanJourneyList: return OperationDescriptor(plane: .human, name: "journey.list", method: .get, path: "/v1/journeys", requestType: "Empty", responseType: "JourneyPage", requiresIdempotency: false, bodyless: true)
        case .humanMoveCommit: return OperationDescriptor(plane: .human, name: "move.commit", method: .post, path: "/v1/moves", requestType: "MoveCommitRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        case .humanMoveQuote: return OperationDescriptor(plane: .human, name: "move.quote", method: .post, path: "/v1/moves/quote", requestType: "MoveQuoteRequest", responseType: "MoveQuote", requiresIdempotency: false, bodyless: false)
        case .humanNotificationList: return OperationDescriptor(plane: .human, name: "notification.list", method: .get, path: "/v1/notifications", requestType: "Empty", responseType: "NotificationPage", requiresIdempotency: false, bodyless: true)
        case .humanNotificationPreferencesGet: return OperationDescriptor(plane: .human, name: "notification.preferences.get", method: .get, path: "/v1/notifications/preferences", requestType: "Empty", responseType: "NotificationPreferences", requiresIdempotency: false, bodyless: true)
        case .humanNotificationPreferencesSet: return OperationDescriptor(plane: .human, name: "notification.preferences.set", method: .post, path: "/v1/notifications/preferences", requestType: "NotificationPreferences", responseType: "NotificationPreferences", requiresIdempotency: false, bodyless: false)
        case .humanNotificationRead: return OperationDescriptor(plane: .human, name: "notification.read", method: .post, path: "/v1/notifications/{notification_id}/read", requestType: "Empty", responseType: "NotificationSummary", requiresIdempotency: false, bodyless: true)
        case .humanOnboardingResume: return OperationDescriptor(plane: .human, name: "onboarding.resume", method: .post, path: "/v1/onboarding/resume", requestType: "Empty", responseType: "Journey", requiresIdempotency: false, bodyless: true)
        case .humanOnboardingStatus: return OperationDescriptor(plane: .human, name: "onboarding.status", method: .get, path: "/v1/onboarding", requestType: "Empty", responseType: "Journey", requiresIdempotency: false, bodyless: true)
        case .humanPasskeyAssertBegin: return OperationDescriptor(plane: .human, name: "passkey.assert.begin", method: .post, path: "/v1/passkeys/assertions", requestType: "PasskeyAssertionBegin", responseType: "PasskeyAssertionChallenge", requiresIdempotency: false, bodyless: false)
        case .humanPasskeyAssertFinish: return OperationDescriptor(plane: .human, name: "passkey.assert.finish", method: .post, path: "/v1/passkeys/assertions/{assertion_id}", requestType: "PasskeyAssertionFinish", responseType: "PasskeyAssertion", requiresIdempotency: false, bodyless: false)
        case .humanPasskeyRegisterBegin: return OperationDescriptor(plane: .human, name: "passkey.register.begin", method: .post, path: "/v1/passkeys/registrations", requestType: "PasskeyRegistrationBegin", responseType: "PasskeyRegistrationChallenge", requiresIdempotency: false, bodyless: false)
        case .humanPasskeyRegisterFinish: return OperationDescriptor(plane: .human, name: "passkey.register.finish", method: .post, path: "/v1/passkeys/registrations/{registration_id}", requestType: "PasskeyRegistrationFinish", responseType: "Passkey", requiresIdempotency: false, bodyless: false)
        case .humanProfileGet: return OperationDescriptor(plane: .human, name: "profile.get", method: .get, path: "/v1/profile", requestType: "Empty", responseType: "Profile", requiresIdempotency: false, bodyless: true)
        case .humanProfileUpdate: return OperationDescriptor(plane: .human, name: "profile.update", method: .patch, path: "/v1/profile", requestType: "ProfileUpdate", responseType: "Profile", requiresIdempotency: false, bodyless: false)
        case .humanSecurityAction: return OperationDescriptor(plane: .human, name: "security.action", method: .post, path: "/v1/security/actions", requestType: "SecurityActionRequest", responseType: "SecurityAction", requiresIdempotency: false, bodyless: false)
        case .humanSecurityPasskeyList: return OperationDescriptor(plane: .human, name: "security.passkey.list", method: .get, path: "/v1/security/passkeys", requestType: "Empty", responseType: "PasskeyList", requiresIdempotency: false, bodyless: true)
        case .humanSecurityPasskeyRegisterBegin: return OperationDescriptor(plane: .human, name: "security.passkey.register.begin", method: .post, path: "/v1/security/passkeys/registrations", requestType: "SecurityPasskeyRegistrationBegin", responseType: "PasskeyRegistrationChallenge", requiresIdempotency: false, bodyless: false)
        case .humanSecurityPasskeyRegisterFinish: return OperationDescriptor(plane: .human, name: "security.passkey.register.finish", method: .post, path: "/v1/security/passkeys/registrations/{registration_id}", requestType: "SecurityPasskeyRegistrationFinish", responseType: "Passkey", requiresIdempotency: false, bodyless: false)
        case .humanSecurityPasskeyRevoke: return OperationDescriptor(plane: .human, name: "security.passkey.revoke", method: .post, path: "/v1/security/passkeys/{passkey_id}/revoke", requestType: "SecurityPasskeyRevocation", responseType: "PasskeyList", requiresIdempotency: false, bodyless: false)
        case .humanSecurityRecoveryReveal: return OperationDescriptor(plane: .human, name: "security.recovery.reveal", method: .post, path: "/v1/security/recovery/evidence", requestType: "SecurityRecoveryReveal", responseType: "TimedSecret", requiresIdempotency: false, bodyless: false)
        case .humanSecuritySessionRevoke: return OperationDescriptor(plane: .human, name: "security.session.revoke", method: .post, path: "/v1/security/sessions/{session_id}/revoke", requestType: "SecuritySessionRevocation", responseType: "SessionRevocation", requiresIdempotency: true, bodyless: false)
        case .humanSecuritySessionRevokeAll: return OperationDescriptor(plane: .human, name: "security.session.revoke-all", method: .post, path: "/v1/security/sessions/revoke-all", requestType: "SecuritySessionRevocation", responseType: "SessionRevocation", requiresIdempotency: true, bodyless: false)
        case .humanSessionList: return OperationDescriptor(plane: .human, name: "session.list", method: .get, path: "/v1/sessions", requestType: "Empty", responseType: "SessionList", requiresIdempotency: false, bodyless: true)
        case .humanSessionOpen: return OperationDescriptor(plane: .human, name: "session.open", method: .post, path: "/v1/sessions", requestType: "SessionOpenRequest", responseType: "Session", requiresIdempotency: true, bodyless: false)
        case .humanSessionRefresh: return OperationDescriptor(plane: .human, name: "session.refresh", method: .post, path: "/v1/sessions/refresh", requestType: "Empty", responseType: "Session", requiresIdempotency: false, bodyless: true)
        case .humanSessionRevoke: return OperationDescriptor(plane: .human, name: "session.revoke", method: .delete, path: "/v1/sessions/{session_id}", requestType: "Empty", responseType: "SessionRevocation", requiresIdempotency: true, bodyless: true)
        case .humanSessionRevokeAll: return OperationDescriptor(plane: .human, name: "session.revoke-all", method: .post, path: "/v1/sessions/revoke-all", requestType: "Empty", responseType: "SessionRevocation", requiresIdempotency: true, bodyless: true)
        case .humanStepupBegin: return OperationDescriptor(plane: .human, name: "stepup.begin", method: .post, path: "/v1/step-up", requestType: "StepUpRequest", responseType: "StepUpChallenge", requiresIdempotency: false, bodyless: false)
        case .humanStepupFinish: return OperationDescriptor(plane: .human, name: "stepup.finish", method: .post, path: "/v1/step-up/{challenge_id}", requestType: "StepUpFinish", responseType: "StepUpEvidence", requiresIdempotency: false, bodyless: false)
        case .humanStreamNext: return OperationDescriptor(plane: .human, name: "stream.next", method: .get, path: "/v1/stream/{cursor}", requestType: "Empty", responseType: "StreamPage", requiresIdempotency: false, bodyless: true)
        case .humanStreamOpen: return OperationDescriptor(plane: .human, name: "stream.open", method: .post, path: "/v1/stream", requestType: "Empty", responseType: "StreamPosition", requiresIdempotency: false, bodyless: true)
        case .humanSupportCreate: return OperationDescriptor(plane: .human, name: "support.create", method: .post, path: "/v1/support/conversations", requestType: "SupportCreateRequest", responseType: "SupportConversation", requiresIdempotency: true, bodyless: false)
        case .humanSupportFeedback: return OperationDescriptor(plane: .human, name: "support.feedback", method: .post, path: "/v1/support/conversations/{conversation_id}/feedback", requestType: "SupportFeedbackRequest", responseType: "SupportConversation", requiresIdempotency: false, bodyless: false)
        case .humanSupportList: return OperationDescriptor(plane: .human, name: "support.list", method: .get, path: "/v1/support/conversations", requestType: "Empty", responseType: "SupportConversationPage", requiresIdempotency: false, bodyless: true)
        case .humanSupportRead: return OperationDescriptor(plane: .human, name: "support.read", method: .post, path: "/v1/support/conversations/{conversation_id}/read", requestType: "SupportReadRequest", responseType: "SupportConversationStatus", requiresIdempotency: false, bodyless: false)
        case .humanSupportReply: return OperationDescriptor(plane: .human, name: "support.reply", method: .post, path: "/v1/support/conversations/{conversation_id}/replies", requestType: "SupportReplyRequest", responseType: "SupportConversation", requiresIdempotency: true, bodyless: false)
        case .humanSupportStatus: return OperationDescriptor(plane: .human, name: "support.status", method: .get, path: "/v1/support/conversations/{conversation_id}/status", requestType: "Empty", responseType: "SupportConversationStatus", requiresIdempotency: false, bodyless: true)
        case .humanVersion: return OperationDescriptor(plane: .human, name: "version", method: .get, path: "/v1/version", requestType: "Empty", responseType: "VersionInfo", requiresIdempotency: false, bodyless: true)
        case .humanWithdrawClaim: return OperationDescriptor(plane: .human, name: "withdraw.claim", method: .post, path: "/v1/withdrawals/{journey_id}/claim", requestType: "WithdrawClaimRequest", responseType: "Journey", requiresIdempotency: false, bodyless: false)
        case .humanWithdrawStart: return OperationDescriptor(plane: .human, name: "withdraw.start", method: .post, path: "/v1/withdrawals", requestType: "WithdrawStartRequest", responseType: "Journey", requiresIdempotency: true, bodyless: false)
        }
    }
}

public extension PlatformClient {
    func agentAgentRegister(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentAgentRegister, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentApprovalApprove(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentApprovalApprove, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentApprovalGet(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentApprovalGet, request: request, pathParameters: pathParameters)
    }
    func agentApprovalList(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentApprovalList, request: request, pathParameters: pathParameters)
    }
    func agentApprovalReject(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentApprovalReject, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentAvailabilityFetch(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentAvailabilityFetch, request: request, pathParameters: pathParameters)
    }
    func agentBudgetCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentBudgetCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentBudgetFund(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentBudgetFund, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentBudgetList(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentBudgetList, request: request, pathParameters: pathParameters)
    }
    func agentBudgetReconciliation(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentBudgetReconciliation, request: request, pathParameters: pathParameters)
    }
    func agentBudgetRevoke(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentBudgetRevoke, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentCapabilityAttenuate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentCapabilityAttenuate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentCapabilityCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentCapabilityCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentCapabilityList(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentCapabilityList, request: request, pathParameters: pathParameters)
    }
    func agentCapabilityRevoke(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentCapabilityRevoke, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentExportOffline(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentExportOffline, request: request, pathParameters: pathParameters)
    }
    func agentPrepare(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentPrepare, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentProgramActivity(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProgramActivity, request: request, pathParameters: pathParameters)
    }
    func agentProgramCall(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentProgramCall, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentProgramDiscover(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProgramDiscover, request: request, pathParameters: pathParameters)
    }
    func agentProgramInterface(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProgramInterface, request: request, pathParameters: pathParameters)
    }
    func agentProgramReceipt(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProgramReceipt, request: request, pathParameters: pathParameters)
    }
    func agentProgramSimulate(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProgramSimulate, request: request, pathParameters: pathParameters)
    }
    func agentProject(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentProject, request: request, pathParameters: pathParameters)
    }
    func agentReadAccount(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadAccount, request: request, pathParameters: pathParameters)
    }
    func agentReadBalance(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadBalance, request: request, pathParameters: pathParameters)
    }
    func agentReadBatch(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadBatch, request: request, pathParameters: pathParameters)
    }
    func agentReadCheckpoint(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadCheckpoint, request: request, pathParameters: pathParameters)
    }
    func agentReadHistory(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadHistory, request: request, pathParameters: pathParameters)
    }
    func agentReadModuleState(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadModuleState, request: request, pathParameters: pathParameters)
    }
    func agentReadProofBundle(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentReadProofBundle, request: request, pathParameters: pathParameters)
    }
    func agentSessionClose(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSessionClose, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSessionList(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentSessionList, request: request, pathParameters: pathParameters)
    }
    func agentSessionOpen(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSessionOpen, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSessionRefresh(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSessionRefresh, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSign(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSign, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubmit(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubmit, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubscriptionAcknowledge(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubscriptionAcknowledge, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubscriptionCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubscriptionCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubscriptionDelete(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubscriptionDelete, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubscriptionHealth(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentSubscriptionHealth, request: request, pathParameters: pathParameters)
    }
    func agentSubscriptionList(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentSubscriptionList, request: request, pathParameters: pathParameters)
    }
    func agentSubscriptionPause(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubscriptionPause, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentSubscriptionResume(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.agentSubscriptionResume, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func agentTrack(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentTrack, request: request, pathParameters: pathParameters)
    }
    func agentWait(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.agentWait, request: request, pathParameters: pathParameters)
    }
    func humanAccountBalance(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAccountBalance, request: request, pathParameters: pathParameters)
    }
    func humanAccountCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAccountCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanActivityEntry(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanActivityEntry, request: request, pathParameters: pathParameters)
    }
    func humanActivityExportEvidence(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanActivityExportEvidence, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanActivityExportStatement(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanActivityExportStatement, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanActivityQuery(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanActivityQuery, request: request, pathParameters: pathParameters)
    }
    func humanAgentArchive(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentArchive, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAgentGet, request: request, pathParameters: pathParameters)
    }
    func humanAgentLimit(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentLimit, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAgentList, request: request, pathParameters: pathParameters)
    }
    func humanAgentPause(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentPause, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentReclaim(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentReclaim, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentRecover(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentRecover, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentResume(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentResume, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAgentRotate(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanAgentRotate, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanApprovalApprove(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanApprovalApprove, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanApprovalGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanApprovalGet, request: request, pathParameters: pathParameters)
    }
    func humanApprovalList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanApprovalList, request: request, pathParameters: pathParameters)
    }
    func humanApprovalReject(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanApprovalReject, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanAuthenticatorBackupRotate(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAuthenticatorBackupRotate, request: request, pathParameters: pathParameters)
    }
    func humanAuthenticatorDisable(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAuthenticatorDisable, request: request, pathParameters: pathParameters)
    }
    func humanAuthenticatorSetupBegin(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAuthenticatorSetupBegin, request: request, pathParameters: pathParameters)
    }
    func humanAuthenticatorSetupFinish(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAuthenticatorSetupFinish, request: request, pathParameters: pathParameters)
    }
    func humanAuthenticatorStatus(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanAuthenticatorStatus, request: request, pathParameters: pathParameters)
    }
    func humanBindingRebind(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanBindingRebind, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanBindingRebindAction(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanBindingRebindAction, request: request, pathParameters: pathParameters)
    }
    func humanBindingStatement(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanBindingStatement, request: request, pathParameters: pathParameters)
    }
    func humanBindingStatus(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanBindingStatus, request: request, pathParameters: pathParameters)
    }
    func humanBindingSubmit(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanBindingSubmit, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanDepositConfirm(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanDepositConfirm, request: request, pathParameters: pathParameters)
    }
    func humanDepositStart(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanDepositStart, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanEvidenceGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanEvidenceGet, request: request, pathParameters: pathParameters)
    }
    func humanExitEligibility(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanExitEligibility, request: request, pathParameters: pathParameters)
    }
    func humanExitStart(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanExitStart, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanHomeSummary(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanHomeSummary, request: request, pathParameters: pathParameters)
    }
    func humanJourneyGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanJourneyGet, request: request, pathParameters: pathParameters)
    }
    func humanJourneyList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanJourneyList, request: request, pathParameters: pathParameters)
    }
    func humanMoveCommit(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanMoveCommit, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanMoveQuote(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanMoveQuote, request: request, pathParameters: pathParameters)
    }
    func humanNotificationList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanNotificationList, request: request, pathParameters: pathParameters)
    }
    func humanNotificationPreferencesGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanNotificationPreferencesGet, request: request, pathParameters: pathParameters)
    }
    func humanNotificationPreferencesSet(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanNotificationPreferencesSet, request: request, pathParameters: pathParameters)
    }
    func humanNotificationRead(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanNotificationRead, request: request, pathParameters: pathParameters)
    }
    func humanOnboardingResume(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanOnboardingResume, request: request, pathParameters: pathParameters)
    }
    func humanOnboardingStatus(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanOnboardingStatus, request: request, pathParameters: pathParameters)
    }
    func humanPasskeyAssertBegin(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanPasskeyAssertBegin, request: request, pathParameters: pathParameters)
    }
    func humanPasskeyAssertFinish(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanPasskeyAssertFinish, request: request, pathParameters: pathParameters)
    }
    func humanPasskeyRegisterBegin(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanPasskeyRegisterBegin, request: request, pathParameters: pathParameters)
    }
    func humanPasskeyRegisterFinish(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanPasskeyRegisterFinish, request: request, pathParameters: pathParameters)
    }
    func humanProfileGet(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanProfileGet, request: request, pathParameters: pathParameters)
    }
    func humanProfileUpdate(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanProfileUpdate, request: request, pathParameters: pathParameters)
    }
    func humanSecurityAction(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityAction, request: request, pathParameters: pathParameters)
    }
    func humanSecurityPasskeyList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityPasskeyList, request: request, pathParameters: pathParameters)
    }
    func humanSecurityPasskeyRegisterBegin(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityPasskeyRegisterBegin, request: request, pathParameters: pathParameters)
    }
    func humanSecurityPasskeyRegisterFinish(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityPasskeyRegisterFinish, request: request, pathParameters: pathParameters)
    }
    func humanSecurityPasskeyRevoke(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityPasskeyRevoke, request: request, pathParameters: pathParameters)
    }
    func humanSecurityRecoveryReveal(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSecurityRecoveryReveal, request: request, pathParameters: pathParameters)
    }
    func humanSecuritySessionRevoke(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSecuritySessionRevoke, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSecuritySessionRevokeAll(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSecuritySessionRevokeAll, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSessionList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSessionList, request: request, pathParameters: pathParameters)
    }
    func humanSessionOpen(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSessionOpen, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSessionRefresh(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSessionRefresh, request: request, pathParameters: pathParameters)
    }
    func humanSessionRevoke(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSessionRevoke, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSessionRevokeAll(idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSessionRevokeAll, request: .emptyObject, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanStepupBegin(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanStepupBegin, request: request, pathParameters: pathParameters)
    }
    func humanStepupFinish(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanStepupFinish, request: request, pathParameters: pathParameters)
    }
    func humanStreamNext(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanStreamNext, request: request, pathParameters: pathParameters)
    }
    func humanStreamOpen(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanStreamOpen, request: request, pathParameters: pathParameters)
    }
    func humanSupportCreate(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSupportCreate, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSupportFeedback(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSupportFeedback, request: request, pathParameters: pathParameters)
    }
    func humanSupportList(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSupportList, request: request, pathParameters: pathParameters)
    }
    func humanSupportRead(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSupportRead, request: request, pathParameters: pathParameters)
    }
    func humanSupportReply(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanSupportReply, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
    func humanSupportStatus(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanSupportStatus, request: request, pathParameters: pathParameters)
    }
    func humanVersion(_ request: JSONValue = .object([:]), pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanVersion, request: request, pathParameters: pathParameters)
    }
    func humanWithdrawClaim(_ request: JSONValue, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await read(.humanWithdrawClaim, request: request, pathParameters: pathParameters)
    }
    func humanWithdrawStart(_ request: JSONValue, idempotencyKey: IdempotencyKey, pathParameters: [String: String] = [:]) async throws -> JSONValue {
        try await mutate(.humanWithdrawStart, request: request, idempotencyKey: idempotencyKey, pathParameters: pathParameters)
    }
}

private let sdkMetadata = SDKMetadata(name: "LayerXSDK", version: "0.1.0", agentOperations: 46, humanOperations: 78)

public func platform_sdk_swift() -> SDKMetadata { sdkMetadata }
