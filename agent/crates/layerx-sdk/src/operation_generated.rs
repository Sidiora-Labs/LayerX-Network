//! Code generated from the LayerX Agent API schema. DO NOT EDIT.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    AgentRegister,
    ApprovalApprove,
    ApprovalGet,
    ApprovalList,
    ApprovalReject,
    AvailabilityFetch,
    BudgetCreate,
    BudgetFund,
    BudgetList,
    BudgetReconciliation,
    BudgetRevoke,
    CapabilityAttenuate,
    CapabilityCreate,
    CapabilityList,
    CapabilityRevoke,
    ExportOffline,
    Prepare,
    ProgramActivity,
    ProgramCall,
    ProgramDiscover,
    ProgramInterface,
    ProgramReceipt,
    ProgramSimulate,
    Project,
    ReadAccount,
    ReadBalance,
    ReadBatch,
    ReadCheckpoint,
    ReadHistory,
    ReadModuleState,
    ReadProofBundle,
    SessionClose,
    SessionList,
    SessionOpen,
    SessionRefresh,
    Sign,
    Submit,
    SubscriptionAcknowledge,
    SubscriptionCreate,
    SubscriptionDelete,
    SubscriptionHealth,
    SubscriptionList,
    SubscriptionPause,
    SubscriptionResume,
    Track,
    Wait,
}

impl Operation {
    pub const ALL: &'static [Self] = &[
        Self::AgentRegister,
        Self::ApprovalApprove,
        Self::ApprovalGet,
        Self::ApprovalList,
        Self::ApprovalReject,
        Self::AvailabilityFetch,
        Self::BudgetCreate,
        Self::BudgetFund,
        Self::BudgetList,
        Self::BudgetReconciliation,
        Self::BudgetRevoke,
        Self::CapabilityAttenuate,
        Self::CapabilityCreate,
        Self::CapabilityList,
        Self::CapabilityRevoke,
        Self::ExportOffline,
        Self::Prepare,
        Self::ProgramActivity,
        Self::ProgramCall,
        Self::ProgramDiscover,
        Self::ProgramInterface,
        Self::ProgramReceipt,
        Self::ProgramSimulate,
        Self::Project,
        Self::ReadAccount,
        Self::ReadBalance,
        Self::ReadBatch,
        Self::ReadCheckpoint,
        Self::ReadHistory,
        Self::ReadModuleState,
        Self::ReadProofBundle,
        Self::SessionClose,
        Self::SessionList,
        Self::SessionOpen,
        Self::SessionRefresh,
        Self::Sign,
        Self::Submit,
        Self::SubscriptionAcknowledge,
        Self::SubscriptionCreate,
        Self::SubscriptionDelete,
        Self::SubscriptionHealth,
        Self::SubscriptionList,
        Self::SubscriptionPause,
        Self::SubscriptionResume,
        Self::Track,
        Self::Wait,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AgentRegister => "agent.register",
            Self::ApprovalApprove => "approval.approve",
            Self::ApprovalGet => "approval.get",
            Self::ApprovalList => "approval.list",
            Self::ApprovalReject => "approval.reject",
            Self::AvailabilityFetch => "availability.fetch",
            Self::BudgetCreate => "budget.create",
            Self::BudgetFund => "budget.fund",
            Self::BudgetList => "budget.list",
            Self::BudgetReconciliation => "budget.reconciliation",
            Self::BudgetRevoke => "budget.revoke",
            Self::CapabilityAttenuate => "capability.attenuate",
            Self::CapabilityCreate => "capability.create",
            Self::CapabilityList => "capability.list",
            Self::CapabilityRevoke => "capability.revoke",
            Self::ExportOffline => "export.offline",
            Self::Prepare => "prepare",
            Self::ProgramActivity => "program.activity",
            Self::ProgramCall => "program.call",
            Self::ProgramDiscover => "program.discover",
            Self::ProgramInterface => "program.interface",
            Self::ProgramReceipt => "program.receipt",
            Self::ProgramSimulate => "program.simulate",
            Self::Project => "project",
            Self::ReadAccount => "read.account",
            Self::ReadBalance => "read.balance",
            Self::ReadBatch => "read.batch",
            Self::ReadCheckpoint => "read.checkpoint",
            Self::ReadHistory => "read.history",
            Self::ReadModuleState => "read.module_state",
            Self::ReadProofBundle => "read.proof_bundle",
            Self::SessionClose => "session.close",
            Self::SessionList => "session.list",
            Self::SessionOpen => "session.open",
            Self::SessionRefresh => "session.refresh",
            Self::Sign => "sign",
            Self::Submit => "submit",
            Self::SubscriptionAcknowledge => "subscription.acknowledge",
            Self::SubscriptionCreate => "subscription.create",
            Self::SubscriptionDelete => "subscription.delete",
            Self::SubscriptionHealth => "subscription.health",
            Self::SubscriptionList => "subscription.list",
            Self::SubscriptionPause => "subscription.pause",
            Self::SubscriptionResume => "subscription.resume",
            Self::Track => "track",
            Self::Wait => "wait",
        }
    }

    #[must_use]
    pub const fn mutating(self) -> bool {
        matches!(
            self,
            Self::AgentRegister
            | Self::ApprovalApprove
            | Self::ApprovalReject
            | Self::BudgetCreate
            | Self::BudgetFund
            | Self::BudgetRevoke
            | Self::CapabilityAttenuate
            | Self::CapabilityCreate
            | Self::CapabilityRevoke
            | Self::Prepare
            | Self::ProgramCall
            | Self::SessionClose
            | Self::SessionOpen
            | Self::SessionRefresh
            | Self::Sign
            | Self::Submit
            | Self::SubscriptionAcknowledge
            | Self::SubscriptionCreate
            | Self::SubscriptionDelete
            | Self::SubscriptionPause
            | Self::SubscriptionResume
        )
    }
}
