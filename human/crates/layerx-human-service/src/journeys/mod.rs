//! Durable, receipt-gated human journeys and their deterministic routing.

mod deposit;
mod engine;
mod exit;
mod move_money;
mod resolver;
mod withdraw;

pub use deposit::{
    DepositActivity, DepositAgentBoundary, DepositAgentPlan, DepositBoundaryError,
    DepositFailureKind, DepositJourney, DepositJourneyError, DepositNotification, DepositPlan,
    DepositProofIdentity, DepositRuntime, DepositStage, DepositStatus, FinalityDelay,
    WalletCustodyOutcome, WalletCustodyRequest,
};

pub use engine::{
    AgentBoundary, AgentBoundaryError, AgentObservation, AgentPreparation, JourneyEngine,
    JourneyError, JourneyLeg, JourneyPhase, JourneyPlan, JourneyProgress, JourneyState,
    JourneyStatus, ReceiptLookup, ReceiptMaterial, VerifiedLegEvidence,
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

pub use resolver::{
    BudgetCreation, BudgetRoute, ChangeSurface, CustodyRoute, Endpoint, EndpointKind, LimitRefusal,
    LimitRefusalError, LimitSource, Mechanism, MovementTerm, PayerGrantRoute, Relationship, Route,
    RouteError, RouteLeg, RouteRequest, RouteResolver, SendRoute,
};
pub use withdraw::{
    CancellationPolicy, PaxeerAction, PaxeerActionOutcome, SettlementConfig, SettlementExpectation,
    WithdrawalAgentPlan, WithdrawalBoundaryError, WithdrawalJourney, WithdrawalJourneyError,
    WithdrawalPlan, WithdrawalReminder, WithdrawalRuntime, WithdrawalStage, WithdrawalStatus,
    WithdrawalTransactionRequest,
};
