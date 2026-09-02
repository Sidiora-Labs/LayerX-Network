//! Stable contract shared by agent-facing servers and SDKs.

#[path = "error.rs"]
mod error_contract;
pub mod generated;
#[path = "identity.rs"]
mod identity_contract;
mod operation_generated;
#[path = "read.rs"]
mod read_contract;
#[path = "stream.rs"]
mod stream_contract;
#[path = "write.rs"]
mod write_contract;

/// Identity and session contract namespace.
pub mod identity {
    pub use crate::identity_contract::*;
}

/// Capability contract namespace.
pub mod capability {
    pub use crate::identity_contract::{
        AmountCeiling, CapabilityAttenuate, CapabilityCreate, CapabilityDimensions, CapabilityId,
        CapabilityList, CapabilityRevoke, ExplicitSet, RateCeiling,
    };
}

/// Budget contract namespace.
pub mod budget {
    pub use crate::identity_contract::{
        BudgetCreate, BudgetEnforcement, BudgetFund, BudgetId, BudgetList, BudgetTarget,
    };
}

/// Preparation contract namespace.
pub mod prepare {
    pub use crate::write_contract::{
        CanonicalBytes, DisclosedAmount, Disclosure, IdempotencyRef, PayloadBytes, PreparationRef,
        PrepareRequest, Prepared, SigningPreimage, TimestampBound,
    };
}

/// Submission contract namespace.
pub mod submit {
    pub use crate::write_contract::{PreparationRef, SignRequest, SignatureBytes, SubmitRequest};
}

/// Tracking contract namespace.
pub mod track {
    pub use crate::write_contract::{
        EvidenceRef, ReceiptRef, SubmissionRef, SubmissionState, TrackRequest, TrackedSubmission,
        Transition, WaitRequest, WaitResult,
    };
}

pub use operation_generated::Operation;
pub use write_contract::SubmissionState;

/// Verified read contract namespace.
pub mod read {
    pub use crate::read_contract::{
        AccountRef, AccountValue, BalanceSelector, BalanceValue, BatchRef, BatchValue,
        CheckpointRef, CheckpointValue, CoreProduced, Freshness, HistoryCursor, HistorySelector,
        HistoryValue, ModuleRef, ModuleStateSelector, ModuleStateValue, ProjectionResult,
        ReadRequest, RelativeTo, VerifiedRead,
    };
}

/// Proof retrieval contract namespace.
pub mod proof {
    pub use crate::read_contract::ProofBundle;
}

/// Availability retrieval contract namespace.
pub mod availability {
    pub use crate::read_contract::{
        AvailabilityClass, AvailabilityCompletion, AvailabilityReport, AvailabilityRequest,
        ClassReport, ProviderRef, ProviderReport,
    };
}

/// Offline verification export namespace.
pub mod export {
    pub use crate::read_contract::{FactRef, OfflineExport};
}

/// Durable subscription and delivery contract namespace.
pub mod subscription {
    pub use crate::stream_contract::{
        Cursor, CursorAcknowledgement, DeduplicationId, Delivery, DeliveryTarget, EventDelivery,
        EventIdentity, GapNotice, ReceiptReference, SubscriptionCreate, SubscriptionFilter,
        SubscriptionHealth, SubscriptionId, SubscriptionList, SubscriptionRecord,
        SubscriptionScope, SubscriptionTarget, TenantObject, TruncationNotice,
    };
}

pub use stream_contract::{Delivery, GapNotice};

/// Stable wire error contract namespace.
pub mod error {
    pub use crate::error_contract::{ApiError, ErrorClass, ReasonCode, RequestId, Retriability};
}

/// Idempotent mutation contract namespace.
pub mod idempotency {
    pub use crate::error_contract::{
        classify_repeat, BodyDigest, IdempotencyOutcome, IdempotentMutation, Key,
    };
}

/// Contract-level verification lattice and success envelope.
pub mod verify {
    pub use crate::error_contract::{ApiSuccess, Level, VerificationStatus};
}

pub use generated::{
    agent_api_compat_gate, agent_api_schema_v1, Amount, BudgetLimit, ContractSchema,
    ContractVersion, Sequence, TimestampSeconds, VersionRequest, VersionResponse,
    AGENT_API_V1_SOURCE,
};
