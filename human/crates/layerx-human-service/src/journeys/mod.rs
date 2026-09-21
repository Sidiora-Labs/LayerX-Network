//! Durable, receipt-gated human journeys and their deterministic routing.

mod deposit;
mod engine;
mod exit;
mod move_money;
mod observed;
mod resolver;
mod router;
mod wire;
mod withdraw;

pub use deposit::{
    DepositActivity, DepositAgentBoundary, DepositAgentPlan, DepositBoundaryError,
    DepositFailureKind, DepositJourney, DepositJourneyError, DepositNotification, DepositPlan,
    DepositProofIdentity, DepositRuntime, DepositStage, DepositStatus, FinalityDelay,
    WalletCustodyOutcome, WalletCustodyRequest,
};

pub use engine::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, JourneyEngine,
    JourneyError, JourneyKind, JourneyLeg, JourneyPhase, JourneyPlan, JourneyProgress,
    JourneyState, JourneyStatus, ReceiptLookup, ReceiptMaterial, VerifiedLegEvidence,
};

pub use move_money::{
    MoveAuthorization, MoveJourney, MoveJourneyError, MoveLegExecution, MoveLegProgress, MovePlan,
    MoveQuote, MoveReceiptReference, MoveStage, MoveStatus,
};

pub use exit::{
    ExitBoundaryError, ExitConfirmationError, ExitFailureKind, ExitFinalityEvidence, ExitJourney,
    ExitJourneyError, ExitPlan, ExitStage, ExitStatus, ExitWallet, ExitWalletOutcome,
    ExitWalletRequest, IrreversibleExitConfirmation, EXIT_CONFIRMATION_PHRASE,
    EXIT_IRREVERSIBILITY_NOTICE, EXIT_NORMAL_OPERATION_MESSAGE, EXIT_SETTINGS_SURFACE, EXIT_TITLE,
    ORDINARY_WITHDRAWAL_PATH,
};

pub use observed::{NetworkObservation, ObservationError, ObservedStateBuilder};

pub use resolver::{
    BudgetCreation, BudgetRoute, ChangeSurface, CustodyRoute, Endpoint, EndpointConstructionError,
    EndpointKind, LimitRefusal, LimitRefusalError, LimitSource, Mechanism, MovementTerm,
    PayerGrantRoute, Relationship, Route, RouteError, RouteLeg, RouteRequest, RouteResolver,
    SendRoute,
};

pub use router::{
    plan, plan_with_advisor, Advice, AdvisedPlan, AllowanceId, AllowanceKind, AllowanceScope,
    Annotation, BalanceEntry, BudgetBinding, CandidateRef, CandidateSet, Constraints,
    CustodyContext, Domain, FeeSchedule, LegBinding, LegMechanism, ObservedState, PlannedLeg,
    Refusal, RequiredAuthority, RouteAdvisor, SignedAllowance, SignedPlan, SigningRequirement,
    TopUpRecord, UnifiedIntent, UnifiedPlan,
};
pub use withdraw::{
    CancellationPolicy, PaxeerAction, PaxeerActionOutcome, SettlementConfig, SettlementExpectation,
    WithdrawalAgentPlan, WithdrawalBoundaryError, WithdrawalJourney, WithdrawalJourneyError,
    WithdrawalPlan, WithdrawalReminder, WithdrawalRuntime, WithdrawalStage, WithdrawalStatus,
    WithdrawalTransactionRequest,
};

pub(crate) use deposit::{decode_deposit_plan, encode_deposit_plan};
pub(crate) use exit::{decode_exit_plan, encode_exit_plan};
pub(crate) use withdraw::{decode_withdrawal_plan, encode_withdrawal_plan, MATERIAL_WIRE_BYTES};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MovementExecutionIdentity {
    pub principal: crate::store::PrincipalId,
    pub tenant: crate::store::AgentTenantId,
    pub account: [u8; 32],
    pub wallet: layerx_types::intent::EvmAddress,
    pub plan_id: [u8; 32],
}
