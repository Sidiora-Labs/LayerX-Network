//! Authored Rust SDK for direct-node and daemon deployments.

use std::path::{Path, PathBuf};

use layerx_agent_api::availability::AvailabilityRequest;
use layerx_agent_api::budget::{BudgetCreate, BudgetFund, BudgetList, BudgetTarget};
use layerx_agent_api::capability::{
    CapabilityAttenuate, CapabilityCreate, CapabilityList, CapabilityRevoke,
};
use layerx_agent_api::idempotency::IdempotentMutation;
use layerx_agent_api::identity::{
    AgentRegistration, SessionClose, SessionContext, SessionList, SessionOpen, SessionRefresh,
};
use layerx_agent_api::prepare::{PrepareRequest, Prepared};
use layerx_agent_api::read::{
    BalanceSelector, BatchRef, CheckpointRef, CoreProduced, HistorySelector, ModuleStateSelector,
    ReadRequest, VerifiedRead,
};
use layerx_agent_api::submit::{SignRequest, SubmitRequest};
use layerx_agent_api::subscription::{
    CursorAcknowledgement, SubscriptionCreate, SubscriptionList, SubscriptionTarget,
};
use layerx_agent_api::track::{TrackRequest, TrackedSubmission, WaitRequest};
use layerx_agent_api::verify::Level;
use layerx_agent_api::{agent_api_schema_v1, ContractVersion, SubmissionState};
use layerx_client::client::{Client as NodeClient, ClientConfig, ConnectionError};
use layerx_proof::export::{
    verify as verify_offline_export, ExportVerificationError, OfflineExport, VerificationReport,
};
use layerx_types::result::ResultCode;

pub mod approval;
pub mod mirror_generated;
pub mod production;

macro_rules! mutation_methods {
    ($( $method:ident: $request:ty => $operation:ident ),+ $(,)?) => {
        $(
            #[must_use]
            pub fn $method(
                &self,
                request: IdempotentMutation<$request>,
            ) -> Call<IdempotentMutation<$request>> {
                self.call(Operation::$operation, request)
            }
        )+
    };
}

macro_rules! query_methods {
    ($( $method:ident: $request:ty => $operation:ident ),+ $(,)?) => {
        $(
            #[must_use]
            pub fn $method(&self, request: $request) -> Call<$request> {
                self.call(Operation::$operation, request)
            }
        )+
    };
}

pub mod signer {
    use layerx_agent_api::prepare::{Disclosure, SigningPreimage};
    use layerx_agent_api::submit::SignatureBytes;

    /// Key-free material sent to an external signing boundary.
    #[derive(Clone, Copy, Debug)]
    pub struct SigningRequest<'a> {
        pub canonical_bytes: &'a [u8],
        pub signing_preimage: &'a SigningPreimage,
        pub disclosure: &'a Disclosure,
    }

    /// Explicit external signer. The interface deliberately has no key-export operation.
    pub trait ExternalSigner {
        type Error;

        fn signer_id(&self) -> &str;

        /// Signs the exact preimage after presenting its byte-bound disclosure.
        ///
        /// # Errors
        ///
        /// Returns the signer's typed refusal or device failure.
        fn sign(&mut self, request: SigningRequest<'_>) -> Result<SignatureBytes, Self::Error>;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Deployment {
    Daemon,
    DirectNode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Guarantee {
    VerificationLevelOnEveryRead,
    UnknownIsFirstClass,
    CallerSuppliedIdempotency,
    LosslessProtocolResultCodes,
    ExternalSignerOnly,
    ApprovalHoldsAreDaemonOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Guarantees {
    pub enforced: &'static [Guarantee],
}

pub const GUARANTEES: Guarantees = Guarantees {
    enforced: &[
        Guarantee::VerificationLevelOnEveryRead,
        Guarantee::UnknownIsFirstClass,
        Guarantee::CallerSuppliedIdempotency,
        Guarantee::LosslessProtocolResultCodes,
        Guarantee::ExternalSignerOnly,
        Guarantee::ApprovalHoldsAreDaemonOnly,
    ],
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operation {
    AgentRegister,
    SessionOpen,
    SessionRefresh,
    SessionClose,
    SessionList,
    CapabilityCreate,
    CapabilityAttenuate,
    CapabilityList,
    CapabilityRevoke,
    BudgetCreate,
    BudgetFund,
    BudgetList,
    BudgetReconciliation,
    BudgetRevoke,
    Project,
    Prepare,
    Sign,
    Submit,
    Track,
    Wait,
    ReadBalance,
    ReadAccount,
    ReadModuleState,
    ReadHistory,
    ReadBatch,
    ReadCheckpoint,
    ReadProofBundle,
    AvailabilityFetch,
    ExportOffline,
    SubscriptionCreate,
    SubscriptionList,
    SubscriptionHealth,
    SubscriptionAcknowledge,
    SubscriptionPause,
    SubscriptionResume,
    SubscriptionDelete,
    ApprovalList,
    ApprovalGet,
    ApprovalApprove,
    ApprovalReject,
}

impl Operation {
    pub const ALL: &'static [Self] = &[
        Self::AgentRegister,
        Self::SessionOpen,
        Self::SessionRefresh,
        Self::SessionClose,
        Self::SessionList,
        Self::CapabilityCreate,
        Self::CapabilityAttenuate,
        Self::CapabilityList,
        Self::CapabilityRevoke,
        Self::BudgetCreate,
        Self::BudgetFund,
        Self::BudgetList,
        Self::BudgetReconciliation,
        Self::BudgetRevoke,
        Self::Project,
        Self::Prepare,
        Self::Sign,
        Self::Submit,
        Self::Track,
        Self::Wait,
        Self::ReadBalance,
        Self::ReadAccount,
        Self::ReadModuleState,
        Self::ReadHistory,
        Self::ReadBatch,
        Self::ReadCheckpoint,
        Self::ReadProofBundle,
        Self::AvailabilityFetch,
        Self::ExportOffline,
        Self::SubscriptionCreate,
        Self::SubscriptionList,
        Self::SubscriptionHealth,
        Self::SubscriptionAcknowledge,
        Self::SubscriptionPause,
        Self::SubscriptionResume,
        Self::SubscriptionDelete,
        Self::ApprovalList,
        Self::ApprovalGet,
        Self::ApprovalApprove,
        Self::ApprovalReject,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AgentRegister => "agent.register",
            Self::SessionOpen => "session.open",
            Self::SessionRefresh => "session.refresh",
            Self::SessionClose => "session.close",
            Self::SessionList => "session.list",
            Self::CapabilityCreate => "capability.create",
            Self::CapabilityAttenuate => "capability.attenuate",
            Self::CapabilityList => "capability.list",
            Self::CapabilityRevoke => "capability.revoke",
            Self::BudgetCreate => "budget.create",
            Self::BudgetFund => "budget.fund",
            Self::BudgetList => "budget.list",
            Self::BudgetReconciliation => "budget.reconciliation",
            Self::BudgetRevoke => "budget.revoke",
            Self::Project => "project",
            Self::Prepare => "prepare",
            Self::Sign => "sign",
            Self::Submit => "submit",
            Self::Track => "track",
            Self::Wait => "wait",
            Self::ReadBalance => "read.balance",
            Self::ReadAccount => "read.account",
            Self::ReadModuleState => "read.module_state",
            Self::ReadHistory => "read.history",
            Self::ReadBatch => "read.batch",
            Self::ReadCheckpoint => "read.checkpoint",
            Self::ReadProofBundle => "read.proof_bundle",
            Self::AvailabilityFetch => "availability.fetch",
            Self::ExportOffline => "export.offline",
            Self::SubscriptionCreate => "subscription.create",
            Self::SubscriptionList => "subscription.list",
            Self::SubscriptionHealth => "subscription.health",
            Self::SubscriptionAcknowledge => "subscription.acknowledge",
            Self::SubscriptionPause => "subscription.pause",
            Self::SubscriptionResume => "subscription.resume",
            Self::SubscriptionDelete => "subscription.delete",
            Self::ApprovalList => "approval.list",
            Self::ApprovalGet => "approval.get",
            Self::ApprovalApprove => "approval.approve",
            Self::ApprovalReject => "approval.reject",
        }
    }

    #[must_use]
    pub const fn mutating(self) -> bool {
        matches!(
            self,
            Self::AgentRegister
                | Self::SessionOpen
                | Self::SessionRefresh
                | Self::SessionClose
                | Self::CapabilityCreate
                | Self::CapabilityAttenuate
                | Self::CapabilityRevoke
                | Self::BudgetCreate
                | Self::BudgetFund
                | Self::BudgetRevoke
                | Self::Prepare
                | Self::Sign
                | Self::Submit
                | Self::SubscriptionCreate
                | Self::SubscriptionAcknowledge
                | Self::SubscriptionPause
                | Self::SubscriptionResume
                | Self::SubscriptionDelete
                | Self::ApprovalApprove
                | Self::ApprovalReject
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Call<T> {
    deployment: Deployment,
    operation: Operation,
    request: T,
}

impl<T> Call<T> {
    #[must_use]
    pub const fn deployment(&self) -> Deployment {
        self.deployment
    }

    #[must_use]
    pub const fn operation(&self) -> Operation {
        self.operation
    }

    #[must_use]
    pub const fn request(&self) -> &T {
        &self.request
    }

    #[must_use]
    pub fn into_request(self) -> T {
        self.request
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyDryRun {
    pub context: SessionContext,
    pub canonical_intent: Vec<u8>,
}

struct DirectBackend {
    configuration: ClientConfig,
    client: Option<NodeClient>,
}

enum Backend {
    Daemon { endpoint: PathBuf },
    DirectNode(Box<DirectBackend>),
}

pub struct Client {
    backend: Backend,
    contract: ContractVersion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SdkError {
    EmptyEndpoint,
    ContractMajor { supported: u16, requested: u16 },
    NodeInterfaceMajor { supported: u16, configured: u16 },
    InvalidDirectLimits,
    NotDirectNode,
    DirectConnection(ConnectionError),
    UnverifiedRead,
    VerificationBelowRequested { requested: Level, achieved: Level },
    ExecutedWithoutEvidence,
}

impl Client {
    /// Configures the typed contract surface for a daemon endpoint.
    ///
    /// # Errors
    ///
    /// Refuses an empty endpoint or unsupported contract major.
    pub fn daemon(
        endpoint: impl Into<PathBuf>,
        contract: ContractVersion,
    ) -> Result<Self, SdkError> {
        let endpoint = endpoint.into();
        validate_contract(contract)?;
        if endpoint.as_os_str().is_empty() {
            return Err(SdkError::EmptyEndpoint);
        }
        Ok(Self {
            backend: Backend::Daemon { endpoint },
            contract,
        })
    }

    /// Configures a direct `layerx-client` connection with no daemon dependency.
    ///
    /// # Errors
    ///
    /// Refuses incompatible node-interface configuration or disabled transport limits.
    pub fn direct_node(configuration: ClientConfig) -> Result<Self, SdkError> {
        let schema = agent_api_schema_v1();
        if configuration.handshake.built_interface_version.major != schema.node_interface_major {
            return Err(SdkError::NodeInterfaceMajor {
                supported: schema.node_interface_major,
                configured: configuration.handshake.built_interface_version.major,
            });
        }
        configuration
            .limits
            .validate()
            .map_err(|_| SdkError::InvalidDirectLimits)?;
        Ok(Self {
            backend: Backend::DirectNode(Box::new(DirectBackend {
                configuration,
                client: None,
            })),
            contract: schema.version,
        })
    }

    /// Establishes the configured direct-node boundary using `layerx-client`.
    ///
    /// # Errors
    ///
    /// Refuses daemon-mode clients and returns the exact `layerx-client` connection error.
    pub fn connect_direct_node(&mut self) -> Result<(), SdkError> {
        let Backend::DirectNode(direct) = &mut self.backend else {
            return Err(SdkError::NotDirectNode);
        };
        direct.client = Some(
            NodeClient::connect(direct.configuration.clone())
                .map_err(SdkError::DirectConnection)?,
        );
        Ok(())
    }

    #[must_use]
    pub const fn deployment(&self) -> Deployment {
        match &self.backend {
            Backend::Daemon { .. } => Deployment::Daemon,
            Backend::DirectNode(_) => Deployment::DirectNode,
        }
    }

    #[must_use]
    pub const fn contract(&self) -> ContractVersion {
        self.contract
    }

    #[must_use]
    pub const fn guarantees(&self) -> Guarantees {
        match self.deployment() {
            Deployment::Daemon | Deployment::DirectNode => GUARANTEES,
        }
    }

    #[must_use]
    pub fn daemon_endpoint(&self) -> Option<&Path> {
        match &self.backend {
            Backend::Daemon { endpoint } => Some(endpoint),
            Backend::DirectNode(_) => None,
        }
    }

    #[must_use]
    pub fn direct_node_client(&mut self) -> Option<&mut NodeClient> {
        match &mut self.backend {
            Backend::Daemon { .. } => None,
            Backend::DirectNode(direct) => direct.client.as_mut(),
        }
    }

    fn call<T>(&self, operation: Operation, request: T) -> Call<T> {
        Call {
            deployment: self.deployment(),
            operation,
            request,
        }
    }

    /// Requests a signature without importing or exporting key material.
    ///
    /// # Errors
    ///
    /// Returns the external signer's own typed error.
    pub fn sign_external<S: signer::ExternalSigner>(
        signer: &mut S,
        prepared: &Prepared,
    ) -> Result<SignRequest, S::Error> {
        let signature = signer.sign(signer::SigningRequest {
            canonical_bytes: prepared.unsigned_canonical_bytes.as_bytes(),
            signing_preimage: &prepared.signing_preimage,
            disclosure: &prepared.disclosure,
        })?;
        Ok(SignRequest {
            preparation_ref: prepared.preparation_ref.clone(),
            signature,
        })
    }

    /// Admits only a proof-bearing value at or above the requested level.
    ///
    /// # Errors
    ///
    /// Refuses unverified and below-requested values instead of presenting them as results.
    pub fn accept_verified_read<T: CoreProduced>(
        requested: Level,
        read: VerifiedRead<T>,
    ) -> Result<VerifiedRead<T>, SdkError> {
        if read.achieved_verification_level == Level::Unverified {
            return Err(SdkError::UnverifiedRead);
        }
        if read.achieved_verification_level < requested {
            return Err(SdkError::VerificationBelowRequested {
                requested,
                achieved: read.achieved_verification_level,
            });
        }
        Ok(read)
    }

    /// Preserves every submission state and the exact protocol result code.
    ///
    /// # Errors
    ///
    /// Refuses an executed state without both evidence and a verified level.
    pub fn submission_outcome(
        submission: TrackedSubmission,
    ) -> Result<SubmissionOutcome, SdkError> {
        match &submission.state {
            SubmissionState::Unknown => Ok(SubmissionOutcome::Unknown(submission)),
            SubmissionState::Executed { .. }
                if submission.evidence.is_empty()
                    || submission.verification_level == Level::Unverified =>
            {
                Err(SdkError::ExecutedWithoutEvidence)
            }
            SubmissionState::Executed { .. } => Ok(SubmissionOutcome::Executed(submission)),
            SubmissionState::Failed { result } => Ok(SubmissionOutcome::Failed {
                result: *result,
                submission,
            }),
            SubmissionState::Expired => Ok(SubmissionOutcome::Expired(submission)),
            _ => Ok(SubmissionOutcome::Pending(submission)),
        }
    }

    /// Verifies an offline export with `layerx-proof` and no daemon or network.
    ///
    /// # Errors
    ///
    /// Returns the exact proof verification failure.
    pub fn verify_offline(
        export: &OfflineExport,
    ) -> Result<VerificationReport, ExportVerificationError> {
        verify_offline_export(export)
    }

    mutation_methods! {
        agent_register: AgentRegistration => AgentRegister,
        session_open: SessionOpen => SessionOpen,
        session_refresh: SessionRefresh => SessionRefresh,
        session_close: SessionClose => SessionClose,
        capability_create: CapabilityCreate => CapabilityCreate,
        capability_attenuate: CapabilityAttenuate => CapabilityAttenuate,
        capability_revoke: CapabilityRevoke => CapabilityRevoke,
        budget_create: BudgetCreate => BudgetCreate,
        budget_fund: BudgetFund => BudgetFund,
        budget_revoke: BudgetTarget => BudgetRevoke,
        prepare: PrepareRequest => Prepare,
        sign: SignRequest => Sign,
        submit: SubmitRequest => Submit,
        subscription_create: SubscriptionCreate => SubscriptionCreate,
        subscription_acknowledge: CursorAcknowledgement => SubscriptionAcknowledge,
        subscription_pause: SubscriptionTarget => SubscriptionPause,
        subscription_resume: SubscriptionTarget => SubscriptionResume,
        subscription_delete: SubscriptionTarget => SubscriptionDelete
    }

    query_methods! {
        session_list: SessionList => SessionList,
        capability_list: CapabilityList => CapabilityList,
        budget_list: BudgetList => BudgetList,
        budget_reconciliation: BudgetTarget => BudgetReconciliation,
        policy_dry_run: PolicyDryRun => Project,
        track: TrackRequest => Track,
        wait: WaitRequest => Wait,
        read_balance: ReadRequest<BalanceSelector> => ReadBalance,
        read_account: ReadRequest<layerx_agent_api::read::AccountRef> => ReadAccount,
        read_module_state: ReadRequest<ModuleStateSelector> => ReadModuleState,
        read_history: ReadRequest<HistorySelector> => ReadHistory,
        read_batch: ReadRequest<BatchRef> => ReadBatch,
        read_checkpoint: ReadRequest<CheckpointRef> => ReadCheckpoint,
        read_proof_bundle: ReadRequest<layerx_agent_api::prepare::CanonicalBytes> => ReadProofBundle,
        availability_fetch: AvailabilityRequest => AvailabilityFetch,
        export_offline: ReadRequest<Vec<layerx_agent_api::export::FactRef>> => ExportOffline,
        subscription_list: SubscriptionList => SubscriptionList,
        subscription_health: SubscriptionTarget => SubscriptionHealth
    }

    query_methods! {
        approval_list: approval::ApprovalListRequest => ApprovalList,
        approval_get: approval::ApprovalGetRequest => ApprovalGet,
        approval_approve: approval::ApprovalApproveRequest => ApprovalApprove,
        approval_reject: approval::ApprovalRejectRequest => ApprovalReject
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionOutcome {
    Pending(TrackedSubmission),
    Unknown(TrackedSubmission),
    Executed(TrackedSubmission),
    Failed {
        result: ResultCode,
        submission: TrackedSubmission,
    },
    Expired(TrackedSubmission),
}

const fn validate_contract(contract: ContractVersion) -> Result<(), SdkError> {
    let supported = agent_api_schema_v1().version.major;
    if contract.major == supported {
        Ok(())
    } else {
        Err(SdkError::ContractMajor {
            supported,
            requested: contract.major,
        })
    }
}
