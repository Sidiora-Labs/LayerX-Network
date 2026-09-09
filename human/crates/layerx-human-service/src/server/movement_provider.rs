//! Privilege-separated movement planning and external execution boundary.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use layerx_paxeer_client::{
    CheckpointProof, DebitExpectation, DepositFailure, DepositProof, FinalityReport,
    TransactionHash, WithdrawalBoundary,
};
use rustix::net::sockopt::socket_peercred;

use crate::audit::AuditChain;
use crate::journeys::{
    DepositBoundaryError, DepositPlan, DepositRuntime, ExitBoundaryError, ExitJourney,
    ExitJourneyError, ExitPlan, ExitStatus, ExitWallet, ExitWalletOutcome, ExitWalletRequest,
    IrreversibleExitConfirmation, MovePlan, PaxeerAction, PaxeerActionOutcome,
    WalletCustodyOutcome, WalletCustodyRequest, WithdrawalBoundaryError, WithdrawalJourney,
    WithdrawalJourneyError, WithdrawalPlan, WithdrawalRuntime, WithdrawalStatus,
    WithdrawalTransactionRequest,
};
use crate::store::{AgentTenantId, PrincipalId, PrincipalScope, RowKey, Table};
use crate::trace::TraceId;
use layerx_paxeer_client::EmergencyExit;

pub const MOVEMENT_PROTOCOL_VERSION: u16 = 2;
const PROTOCOL_VERSION: u16 = MOVEMENT_PROTOCOL_VERSION;

/// Mandatory local transport policy for the movement provider.
#[derive(Clone, Debug)]
pub struct MovementProviderConfig {
    pub socket: PathBuf,
    pub peer_uid: u32,
    pub peer_gid: u32,
    pub maximum_frame_bytes: usize,
    pub deadline: Duration,
}

impl MovementProviderConfig {
    /// Reads a complete configuration. Missing, relative, zero, or overly broad
    /// values refuse startup rather than selecting development defaults.
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn from_environment() -> Result<Self, MovementProviderError> {
        let socket = required("LAYERX_HUMAN_MOVEMENT_SOCKET").map(PathBuf::from)?;
        if !socket.is_absolute() {
            return Err(MovementProviderError::Configuration);
        }
        let peer_uid = number("LAYERX_HUMAN_MOVEMENT_PEER_UID")?;
        let group_id = number("LAYERX_HUMAN_MOVEMENT_PEER_GID")?;
        let maximum_frame_bytes = number("LAYERX_HUMAN_MOVEMENT_MAX_FRAME_BYTES")?;
        let deadline_seconds: u64 = number("LAYERX_HUMAN_MOVEMENT_DEADLINE_SECONDS")?;
        if maximum_frame_bytes == 0 || deadline_seconds == 0 {
            return Err(MovementProviderError::Configuration);
        }
        Ok(Self {
            socket,
            peer_uid,
            peer_gid: group_id,
            maximum_frame_bytes,
            deadline: Duration::from_secs(deadline_seconds),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningContext {
    pub request_anchor: [u8; 32],
    pub account: layerx_types::account::AccountId,
    pub reserve: layerx_types::account::AccountId,
    pub withdrawals_account: layerx_types::account::AccountId,
    pub route: Option<crate::journeys::RouteRequest>,
    pub amount: layerx_types::amount::Amount,
    pub asset: layerx_types::ids::AssetId,
    pub currency: String,
    pub actor: layerx_agent_api::identity::AgentDid,
    pub authority: layerx_agent_api::identity::AuthorityRef,
    pub custody_key: crate::custody::KeyId,
    pub custody_provider_reference: Vec<u8>,
    pub custody_binding_digest: [u8; 32],
    pub wallet: layerx_types::intent::EvmAddress,
    pub network: layerx_types::intent::NetworkId,
    pub protocol_version: u16,
    pub paxeer_chain_id: u64,
    pub account_sequence: u64,
    pub budget_grant: Option<PlanningBudgetGrant>,
    pub fee_limit: u128,
    pub evm_gas_limit: u64,
    pub evm_max_fee_per_gas: u64,
    pub evm_max_priority_fee_per_gas: u64,
    pub not_before: u64,
    pub not_after: u64,
    pub binding_receipt_digest: [u8; 32],
    pub identity_authority_evidence: Vec<u8>,
    pub balance_evidence: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningBudgetGrant {
    pub budget: layerx_types::intent::BudgetId,
    pub grant: layerx_types::intent::AuthorityGrantId,
}

/// Exact caller authority and request bytes used when constructing a plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningRequest {
    pub principal: PrincipalId,
    pub tenant: AgentTenantId,
    pub context: PlanningContext,
    pub operation: String,
    pub idempotency_key: [u8; 32],
    pub canonical_body: Vec<u8>,
    pub trace: TraceId,
    pub now: u64,
}

impl PlanningRequest {
    /// Reconstructs canonical provider fields while enforcing boundary bounds.
    /// # Errors
    /// Refuses absent identity evidence, invalid execution bounds, and malformed request data.
    pub fn from_wire_parts(value: Self) -> Result<Self, MovementProviderError> {
        let Self {
            principal,
            tenant,
            context,
            operation,
            idempotency_key,
            canonical_body,
            trace,
            now,
        } = value;
        if context.paxeer_chain_id == 0
            || context.amount.value() == 0
            || context.asset.bytes() == [0; 32]
            || context.custody_provider_reference.is_empty()
            || context.custody_binding_digest == [0; 32]
            || context.evm_gas_limit == 0
            || context.evm_max_fee_per_gas == 0
            || context.evm_max_priority_fee_per_gas > context.evm_max_fee_per_gas
            || context.reserve.namespace()
                != layerx_types::account::AccountNamespace::SystemPaxeerReserve
            || context.withdrawals_account.namespace()
                != layerx_types::account::AccountNamespace::SystemPaxeerWithdrawals
            || context
                .route
                .as_ref()
                .is_some_and(|route| route.asset != context.asset || route.amount != context.amount)
            || context.wallet.bytes() == [0; 20]
            || context.binding_receipt_digest == [0; 32]
            || context.identity_authority_evidence.is_empty()
            || context.balance_evidence.is_empty()
            || context.not_before > now
            || context.not_after <= now
            || !matches!(context.protocol_version, 2 | 3)
            || (operation == "withdraw.start"
                && (context.request_anchor == [0; 32] || context.fee_limit > u128::from(u64::MAX)))
            || operation.is_empty()
            || operation.len() > 128
            || operation.chars().any(char::is_control)
            || idempotency_key == [0; 32]
            || canonical_body.is_empty()
            || canonical_body.len() > 1_048_576
            || now == 0
        {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(Self {
            principal,
            tenant,
            context,
            operation,
            idempotency_key,
            canonical_body,
            trace,
            now,
        })
    }
}

/// Provider-owned review record. `quote_id` names the durable provider row;
/// the complete plan is returned only with the exact expiry committed there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedMovePlan {
    pub quote_id: String,
    pub expires_at: u64,
    pub arrival_at: u64,
    pub plan: MovePlan,
}

impl AuthorizedMovePlan {
    /// Encodes the quote envelope and its canonical owner plan.
    #[must_use]
    pub fn canonical_encode(&self) -> Vec<u8> {
        let plan = self.plan.canonical_encode();
        let mut out = vec![1];
        out.extend(
            u16::try_from(self.quote_id.len())
                .unwrap_or_else(|_| unreachable!("validated quote length"))
                .to_be_bytes(),
        );
        out.extend(self.quote_id.as_bytes());
        out.extend(self.expires_at.to_be_bytes());
        out.extend(self.arrival_at.to_be_bytes());
        out.extend(
            u32::try_from(plan.len())
                .unwrap_or_else(|_| unreachable!("validated plan length"))
                .to_be_bytes(),
        );
        out.extend(plan);
        out
    }

    /// Decodes a quote envelope, rejects trailing bytes, and revalidates its plan.
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn canonical_decode(bytes: &[u8]) -> Result<Self, MovementProviderError> {
        if bytes.is_empty() || bytes.len() > 1_048_576 {
            return Err(MovementProviderError::ContractViolation);
        }
        let mut at = 0usize;
        let mut take = |n: usize| {
            let end = at
                .checked_add(n)
                .ok_or(MovementProviderError::ContractViolation)?;
            let value = bytes
                .get(at..end)
                .ok_or(MovementProviderError::ContractViolation)?;
            at = end;
            Ok::<_, MovementProviderError>(value)
        };
        if take(1)?[0] != 1 {
            return Err(MovementProviderError::ContractViolation);
        }
        let quote_len = u16::from_be_bytes(
            take(2)?
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        ) as usize;
        if !(16..=128).contains(&quote_len) {
            return Err(MovementProviderError::ContractViolation);
        }
        let quote_id = std::str::from_utf8(take(quote_len)?)
            .map_err(|_| MovementProviderError::ContractViolation)?
            .to_owned();
        let expires_at = u64::from_be_bytes(
            take(8)?
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        );
        let arrival_at = u64::from_be_bytes(
            take(8)?
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        );
        let plan_len = u32::from_be_bytes(
            take(4)?
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        ) as usize;
        if plan_len == 0 || plan_len > 1_048_576 {
            return Err(MovementProviderError::ContractViolation);
        }
        let plan = MovePlan::canonical_decode(take(plan_len)?)
            .map_err(|_| MovementProviderError::ContractViolation)?;
        if at != bytes.len() {
            return Err(MovementProviderError::ContractViolation);
        }
        Self::from_wire_parts(quote_id, expires_at, arrival_at, plan)
    }

    /// Constructs one provider-owned quote with a non-empty review window.
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn from_wire_parts(
        quote_id: String,
        expires_at: u64,
        arrival_at: u64,
        plan: MovePlan,
    ) -> Result<Self, MovementProviderError> {
        quote_row(&quote_id)?;
        if expires_at == 0 || arrival_at == 0 || arrival_at < expires_at {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(Self {
            quote_id,
            expires_at,
            arrival_at,
            plan,
        })
    }
}

/// Typed provider calls. Economic requests remain native domain objects; they
/// are never converted to an unclassified JSON command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MovementProviderRequest {
    PlanMove(PlanningRequest),
    PlanDeposit(PlanningRequest),
    PlanWithdrawal(PlanningRequest),
    PlanExit(PlanningRequest),
    VerifyExternalDeposit {
        request: WalletCustodyRequest,
        transaction: TransactionHash,
    },
    SubmitDepositCustody(WalletCustodyRequest),
    PollDepositFinality(TransactionHash),
    ObtainDepositProof(TransactionHash),
    VerifyClaimSignature {
        request: WithdrawalTransactionRequest,
        signature: Vec<u8>,
    },
    CheckpointProof(DebitExpectation),
    SubmitWithdrawal(WithdrawalTransactionRequest),
    LookupWithdrawal([u8; 32]),
    SubmitExit(ExitWalletRequest),
    PrepareEvmTransaction {
        identity: crate::journeys::MovementExecutionIdentity,
        action_key: [u8; 32],
        target: layerx_types::intent::EvmAddress,
        calldata: Vec<u8>,
    },
    Readiness,
}

/// Exhaustive typed results. A mismatched response discriminant is a contract
/// violation and is never interpreted as success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MovementProviderResponse {
    MovePlan(AuthorizedMovePlan),
    DepositPlan(DepositPlan),
    WithdrawalPlan(WithdrawalPlan),
    ExitPlan(ExitPlan),
    VerifiedDeposit(TransactionHash),
    DepositCustody(WalletCustodyOutcome),
    DepositFinality(FinalityReport),
    DepositProof(Result<DepositProof, DepositFailure>),
    ClaimTransaction(Vec<u8>),
    CheckpointProof(Option<CheckpointProof>),
    Withdrawal(PaxeerActionOutcome),
    WithdrawalLookup(Option<TransactionHash>),
    Exit(ExitWalletOutcome),
    PreparedEvmTransaction(crate::custody::EvmTransaction),
    Ready,
    Unavailable,
    ContractViolation,
}

/// Authoritative native codec shared by the provider daemon and its client.
/// Implementations must encode every field and validate canonicality, bounds,
/// protocol version, and native proof construction while decoding.
pub trait MovementProviderCodec: Send + Sync {
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    fn encode_request(
        &self,
        request: &MovementProviderRequest,
    ) -> Result<Vec<u8>, MovementProviderError>;
    /// # Errors
    /// Refuses malformed, noncanonical, or unsupported movement frames.
    fn decode_request(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderRequest, MovementProviderError>;
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    fn encode_response(
        &self,
        response: &MovementProviderResponse,
    ) -> Result<Vec<u8>, MovementProviderError>;
    /// # Errors
    /// Refuses malformed, noncanonical, or unsupported movement frames.
    fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderResponse, MovementProviderError>;
}

/// Canonical native codec. Every nested economic object delegates to its
/// owner module's validated wire representation.
#[derive(Clone, Copy, Debug)]
pub struct NativeMovementCodec {
    protocol_version: u16,
}

impl Default for NativeMovementCodec {
    fn default() -> Self {
        Self::new()
    }
}
impl NativeMovementCodec {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            protocol_version: layerx_wire::limits::PROTOCOL_VERSION,
        }
    }
    /// Selects the exact `LayerX` protocol for nested checkpoint attestations.
    /// # Errors
    /// Refuses versions other than legacy 2 and composite-state 3.
    pub fn for_protocol(protocol_version: u16) -> Result<Self, MovementProviderError> {
        if !matches!(protocol_version, 2 | 3) {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(Self { protocol_version })
    }
}

impl MovementProviderCodec for NativeMovementCodec {
    fn encode_request(
        &self,
        request: &MovementProviderRequest,
    ) -> Result<Vec<u8>, MovementProviderError> {
        let mut w = MpWriter::new();
        match request {
            MovementProviderRequest::PlanMove(v) => {
                w.tag(1);
                w.planning(v)?;
            }
            MovementProviderRequest::PlanDeposit(v) => {
                w.tag(2);
                w.planning(v)?;
            }
            MovementProviderRequest::PlanWithdrawal(v) => {
                w.tag(3);
                w.planning(v)?;
            }
            MovementProviderRequest::PlanExit(v) => {
                w.tag(4);
                w.planning(v)?;
            }
            MovementProviderRequest::VerifyExternalDeposit {
                request,
                transaction,
            } => {
                w.tag(5);
                w.wallet_custody(request)?;
                w.fixed(&transaction.bytes());
            }
            MovementProviderRequest::SubmitDepositCustody(v) => {
                w.tag(6);
                w.wallet_custody(v)?;
            }
            MovementProviderRequest::PollDepositFinality(v) => {
                w.tag(7);
                w.fixed(&v.bytes());
            }
            MovementProviderRequest::ObtainDepositProof(v) => {
                w.tag(8);
                w.fixed(&v.bytes());
            }
            MovementProviderRequest::VerifyClaimSignature { request, signature } => {
                w.tag(9);
                w.withdrawal_request(request)?;
                w.blob(signature, 262_144)?;
            }
            MovementProviderRequest::CheckpointProof(v) => {
                w.tag(10);
                w.blob(
                    &layerx_paxeer_client::wire::encode_debit_expectation(v, 4096)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    4096,
                )?;
            }
            MovementProviderRequest::SubmitWithdrawal(v) => {
                w.tag(11);
                w.withdrawal_request(v)?;
            }
            MovementProviderRequest::LookupWithdrawal(v) => {
                w.tag(12);
                w.fixed(v);
            }
            MovementProviderRequest::SubmitExit(v) => {
                w.tag(13);
                w.exit_request(v)?;
            }
            MovementProviderRequest::Readiness => w.tag(14),
            MovementProviderRequest::PrepareEvmTransaction {
                identity,
                action_key,
                target,
                calldata,
            } => {
                w.tag(15);
                w.execution_identity(identity)?;
                w.fixed(action_key);
                w.fixed(&target.bytes());
                w.blob(calldata, 262_144)?;
            }
        }
        native_frame(w.finish())
    }
    fn decode_request(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderRequest, MovementProviderError> {
        let mut r = MpReader::new(bytes)?;
        let value = match r.u8()? {
            1 => MovementProviderRequest::PlanMove(r.planning()?),
            2 => MovementProviderRequest::PlanDeposit(r.planning()?),
            3 => MovementProviderRequest::PlanWithdrawal(r.planning()?),
            4 => MovementProviderRequest::PlanExit(r.planning()?),
            5 => MovementProviderRequest::VerifyExternalDeposit {
                request: r.wallet_custody()?,
                transaction: TransactionHash::new(r.fixed()?),
            },
            6 => MovementProviderRequest::SubmitDepositCustody(r.wallet_custody()?),
            7 => MovementProviderRequest::PollDepositFinality(TransactionHash::new(r.fixed()?)),
            8 => MovementProviderRequest::ObtainDepositProof(TransactionHash::new(r.fixed()?)),
            9 => MovementProviderRequest::VerifyClaimSignature {
                request: r.withdrawal_request()?,
                signature: r.blob(262_144)?.to_vec(),
            },
            10 => MovementProviderRequest::CheckpointProof(
                layerx_paxeer_client::wire::decode_debit_expectation(r.blob(4096)?, 4096)
                    .map_err(|_| MovementProviderError::ContractViolation)?,
            ),
            11 => MovementProviderRequest::SubmitWithdrawal(r.withdrawal_request()?),
            12 => MovementProviderRequest::LookupWithdrawal(r.fixed()?),
            13 => MovementProviderRequest::SubmitExit(r.exit_request()?),
            14 => MovementProviderRequest::Readiness,
            15 => MovementProviderRequest::PrepareEvmTransaction {
                identity: r.execution_identity()?,
                action_key: r.fixed()?,
                target: layerx_types::intent::EvmAddress::new(r.fixed()?),
                calldata: r.blob(262_144)?.to_vec(),
            },
            _ => return Err(MovementProviderError::ContractViolation),
        };
        r.finish()?;
        if self.encode_request(&value)? != bytes {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(value)
    }
    fn encode_response(
        &self,
        response: &MovementProviderResponse,
    ) -> Result<Vec<u8>, MovementProviderError> {
        let mut w = MpWriter::new();
        match response {
            MovementProviderResponse::MovePlan(_)
            | MovementProviderResponse::DepositPlan(_)
            | MovementProviderResponse::WithdrawalPlan(_)
            | MovementProviderResponse::ExitPlan(_) => {
                w.plan_response(response)?;
            }
            MovementProviderResponse::VerifiedDeposit(v) => {
                w.tag(5);
                w.fixed(&v.bytes());
            }
            MovementProviderResponse::DepositCustody(v) => {
                w.tag(6);
                match v {
                    WalletCustodyOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes());
                    }
                    WalletCustodyOutcome::Rejected => w.u8(2),
                    WalletCustodyOutcome::Failed => w.u8(3),
                }
            }
            MovementProviderResponse::DepositFinality(v) => {
                w.tag(7);
                w.blob(
                    &layerx_paxeer_client::wire::encode_finality_report(v, 262_144)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    262_144,
                )?;
            }
            MovementProviderResponse::DepositProof(Ok(v)) => {
                w.tag(8);
                w.u8(1);
                w.blob(
                    &layerx_paxeer_client::wire::encode_deposit_proof(
                        v,
                        layerx_paxeer_client::wire::MAX_DEPOSIT_PROOF_BYTES,
                    )
                    .map_err(|_| MovementProviderError::ContractViolation)?,
                    layerx_paxeer_client::wire::MAX_DEPOSIT_PROOF_BYTES,
                )?;
            }
            MovementProviderResponse::DepositProof(Err(v)) => {
                w.tag(8);
                w.u8(2);
                w.blob(
                    &layerx_paxeer_client::wire::encode_deposit_failure(v, 262_144)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    262_144,
                )?;
            }
            MovementProviderResponse::ClaimTransaction(v) => {
                w.tag(9);
                w.blob(v, 262_144)?;
            }
            MovementProviderResponse::CheckpointProof(value) => {
                w.checkpoint_response(value.as_ref(), self.protocol_version)?;
            }
            MovementProviderResponse::Withdrawal(v) => {
                w.tag(11);
                match v {
                    PaxeerActionOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes());
                    }
                    PaxeerActionOutcome::Unknown => w.u8(2),
                }
            }
            MovementProviderResponse::WithdrawalLookup(v) => {
                w.tag(12);
                match v {
                    None => w.u8(0),
                    Some(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes());
                    }
                }
            }
            MovementProviderResponse::Exit(v) => {
                w.tag(13);
                match v {
                    ExitWalletOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes());
                    }
                    ExitWalletOutcome::Rejected => w.u8(2),
                }
            }
            MovementProviderResponse::PreparedEvmTransaction(transaction) => {
                w.evm_transaction(transaction)?;
            }
            MovementProviderResponse::Ready => w.tag(14),
            MovementProviderResponse::Unavailable => w.tag(15),
            MovementProviderResponse::ContractViolation => w.tag(16),
        }
        native_frame(w.finish())
    }
    fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderResponse, MovementProviderError> {
        let mut r = MpReader::new(bytes)?;
        let value = match r.u8()? {
            1 => MovementProviderResponse::MovePlan(AuthorizedMovePlan::canonical_decode(
                r.blob(1_048_576)?,
            )?),
            2 => MovementProviderResponse::DepositPlan(
                crate::journeys::decode_deposit_plan(r.blob(1_048_576)?)
                    .map_err(|_| MovementProviderError::ContractViolation)?,
            ),
            3 => MovementProviderResponse::WithdrawalPlan(
                crate::journeys::decode_withdrawal_plan(r.blob(1_048_576)?)
                    .map_err(|_| MovementProviderError::ContractViolation)?,
            ),
            4 => MovementProviderResponse::ExitPlan(
                crate::journeys::decode_exit_plan(r.blob(1_048_576)?)
                    .map_err(|_| MovementProviderError::ContractViolation)?,
            ),
            5 => MovementProviderResponse::VerifiedDeposit(TransactionHash::new(r.fixed()?)),
            6 => MovementProviderResponse::DepositCustody(match r.u8()? {
                1 => WalletCustodyOutcome::Submitted(TransactionHash::new(r.fixed()?)),
                2 => WalletCustodyOutcome::Rejected,
                3 => WalletCustodyOutcome::Failed,
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            7 => MovementProviderResponse::DepositFinality(
                layerx_paxeer_client::wire::decode_finality_report(r.blob(262_144)?, 262_144)
                    .map_err(|_| MovementProviderError::ContractViolation)?,
            ),
            8 => MovementProviderResponse::DepositProof(match r.u8()? {
                1 => Ok(layerx_paxeer_client::wire::decode_deposit_proof(
                    r.blob(layerx_paxeer_client::wire::MAX_DEPOSIT_PROOF_BYTES)?,
                    layerx_paxeer_client::wire::MAX_DEPOSIT_PROOF_BYTES,
                )
                .map_err(|_| MovementProviderError::ContractViolation)?),
                2 => Err(layerx_paxeer_client::wire::decode_deposit_failure(
                    r.blob(262_144)?,
                    262_144,
                )
                .map_err(|_| MovementProviderError::ContractViolation)?),
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            9 => MovementProviderResponse::ClaimTransaction(r.blob(262_144)?.to_vec()),
            10 => MovementProviderResponse::CheckpointProof(match r.u8()? {
                0 => None,
                1 => Some(
                    layerx_paxeer_client::wire::decode_checkpoint_proof_for_protocol(
                        r.blob(1_048_576)?,
                        1_048_576,
                        self.protocol_version,
                    )
                    .map_err(|_| MovementProviderError::ContractViolation)?,
                ),
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            11 => MovementProviderResponse::Withdrawal(match r.u8()? {
                1 => PaxeerActionOutcome::Submitted(TransactionHash::new(r.fixed()?)),
                2 => PaxeerActionOutcome::Unknown,
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            12 => MovementProviderResponse::WithdrawalLookup(match r.u8()? {
                0 => None,
                1 => Some(TransactionHash::new(r.fixed()?)),
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            13 => MovementProviderResponse::Exit(match r.u8()? {
                1 => ExitWalletOutcome::Submitted(TransactionHash::new(r.fixed()?)),
                2 => ExitWalletOutcome::Rejected,
                _ => return Err(MovementProviderError::ContractViolation),
            }),
            17 => MovementProviderResponse::PreparedEvmTransaction(r.evm_transaction()?),
            14 => MovementProviderResponse::Ready,
            15 => MovementProviderResponse::Unavailable,
            16 => MovementProviderResponse::ContractViolation,
            _ => return Err(MovementProviderError::ContractViolation),
        };
        r.finish()?;
        if self.encode_response(&value)? != bytes {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(value)
    }
}

struct MpWriter {
    out: Vec<u8>,
}
fn native_frame(bytes: Vec<u8>) -> Result<Vec<u8>, MovementProviderError> {
    if bytes.len() > 1_048_576 {
        Err(MovementProviderError::ContractViolation)
    } else {
        Ok(bytes)
    }
}
impl MpWriter {
    fn new() -> Self {
        Self { out: vec![2] }
    }
    fn tag(&mut self, v: u8) {
        self.u8(v);
    }
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.out.extend(v.to_be_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.out.extend(v.to_be_bytes());
    }
    fn fixed(&mut self, v: &[u8]) {
        self.out.extend(v);
    }
    fn text(&mut self, v: &str, max: usize) -> Result<(), MovementProviderError> {
        if v.is_empty()
            || v.len() > max
            || v.len() > u16::MAX as usize
            || v.chars().any(char::is_control)
        {
            return Err(MovementProviderError::ContractViolation);
        }
        self.out.extend(
            u16::try_from(v.len())
                .map_err(|_| MovementProviderError::ContractViolation)?
                .to_be_bytes(),
        );
        self.out.extend(v.as_bytes());
        Ok(())
    }
    fn blob(&mut self, v: &[u8], max: usize) -> Result<(), MovementProviderError> {
        if v.is_empty() || v.len() > max || v.len() > u32::MAX as usize {
            return Err(MovementProviderError::ContractViolation);
        }
        self.u32(u32::try_from(v.len()).map_err(|_| MovementProviderError::ContractViolation)?);
        self.out.extend(v);
        Ok(())
    }
    fn planning(&mut self, v: &PlanningRequest) -> Result<(), MovementProviderError> {
        PlanningRequest::from_wire_parts(v.clone())?;
        self.text(v.principal.as_str(), 128)?;
        self.text(v.tenant.as_str(), 255)?;
        self.planning_context(&v.context)?;
        self.text(&v.operation, 128)?;
        self.fixed(&v.idempotency_key);
        self.blob(&v.canonical_body, 1_048_576)?;
        self.text(v.trace.as_str(), 64)?;
        self.u64(v.now);
        Ok(())
    }
    fn planning_context(&mut self, v: &PlanningContext) -> Result<(), MovementProviderError> {
        self.u8(2);
        self.fixed(&v.request_anchor);
        self.text(v.account.canonical(), 512)?;
        self.text(v.reserve.canonical(), 512)?;
        self.text(v.withdrawals_account.canonical(), 512)?;
        match &v.route {
            Some(route) => {
                self.u8(1);
                self.blob(&route.canonical_encode(), 262_144)?;
            }
            None => self.u8(0),
        }
        self.out.extend(v.amount.to_be_bytes());
        self.fixed(&v.asset.bytes());
        self.text(&v.currency, 128)?;
        self.text(v.actor.as_str(), 255)?;
        self.text(v.authority.as_str(), 255)?;
        self.text(v.custody_key.as_str(), 128)?;
        self.blob(&v.custody_provider_reference, 4096)?;
        self.fixed(&v.custody_binding_digest);
        self.fixed(&v.wallet.bytes());
        self.u32(v.network.value());
        self.out.extend(v.protocol_version.to_be_bytes());
        self.u64(v.paxeer_chain_id);
        self.u64(v.account_sequence);
        match &v.budget_grant {
            Some(grant) => {
                self.u8(1);
                self.fixed(&grant.budget.bytes());
                self.fixed(&grant.grant.bytes());
            }
            None => self.u8(0),
        }
        self.out.extend(v.fee_limit.to_be_bytes());
        self.u64(v.evm_gas_limit);
        self.u64(v.evm_max_fee_per_gas);
        self.u64(v.evm_max_priority_fee_per_gas);
        self.u64(v.not_before);
        self.u64(v.not_after);
        self.fixed(&v.binding_receipt_digest);
        self.blob(&v.identity_authority_evidence, 262_144)?;
        self.blob(&v.balance_evidence, 262_144)
    }
    fn execution_identity(
        &mut self,
        identity: &crate::journeys::MovementExecutionIdentity,
    ) -> Result<(), MovementProviderError> {
        if identity.account == [0; 32]
            || identity.plan_id == [0; 32]
            || identity.wallet.bytes() == [0; 20]
        {
            return Err(MovementProviderError::ContractViolation);
        }
        self.text(identity.principal.as_str(), 128)?;
        self.text(identity.tenant.as_str(), 255)?;
        self.fixed(&identity.account);
        self.fixed(&identity.wallet.bytes());
        self.fixed(&identity.plan_id);
        Ok(())
    }
    fn wallet_custody(&mut self, v: &WalletCustodyRequest) -> Result<(), MovementProviderError> {
        self.execution_identity(&v.identity)?;
        if v.action_key == [0; 32] || v.chain_id == 0 || v.amount.value() == 0 {
            return Err(MovementProviderError::ContractViolation);
        }
        self.fixed(&v.action_key);
        self.fixed(&v.wallet.bytes());
        self.u64(v.chain_id);
        self.fixed(&v.vault.bytes());
        self.fixed(&v.asset.bytes());
        self.fixed(&v.beneficiary);
        self.out.extend(v.amount.to_be_bytes());
        Ok(())
    }
    fn withdrawal_request(
        &mut self,
        v: &WithdrawalTransactionRequest,
    ) -> Result<(), MovementProviderError> {
        self.execution_identity(&v.identity)?;
        if v.action_key == [0; 32] || v.calldata.is_empty() {
            return Err(MovementProviderError::ContractViolation);
        }
        self.fixed(&v.action_key);
        self.u8(match v.action {
            PaxeerAction::QueueClaim => 1,
            PaxeerAction::FinalisePayout => 2,
            PaxeerAction::CancelChallengedPayout => 3,
        });
        self.fixed(&v.target.bytes());
        self.blob(&v.calldata, 262_144)?;
        match &v.signed_transaction {
            Some(bytes) => {
                self.u8(1);
                self.blob(bytes, 262_144)?;
            }
            None => self.u8(0),
        }
        Ok(())
    }
    fn exit_request(&mut self, v: &ExitWalletRequest) -> Result<(), MovementProviderError> {
        self.execution_identity(&v.identity)?;
        if v.action_key == [0; 32]
            || v.calldata.is_empty()
            || v.checkpoint == [0; 32]
            || v.withdrawal_id == [0; 32]
            || v.nullifier == [0; 32]
            || v.finalised_balance == 0
        {
            return Err(MovementProviderError::ContractViolation);
        }
        self.fixed(&v.action_key);
        self.fixed(&v.contract.bytes());
        self.blob(&v.calldata, 262_144)?;
        self.fixed(&v.checkpoint);
        self.fixed(&v.withdrawal_id);
        self.fixed(&v.nullifier);
        self.fixed(&v.recipient.bytes());
        self.out.extend(v.finalised_balance.to_be_bytes());
        Ok(())
    }
    fn checkpoint_response(
        &mut self,
        proof: Option<&CheckpointProof>,
        protocol: u16,
    ) -> Result<(), MovementProviderError> {
        self.tag(10);
        match proof {
            None => self.u8(0),
            Some(proof) => {
                self.u8(1);
                let bytes = layerx_paxeer_client::wire::encode_checkpoint_proof_for_protocol(
                    proof, 1_048_576, protocol,
                )
                .map_err(|_| MovementProviderError::ContractViolation)?;
                self.blob(&bytes, 1_048_576)?;
            }
        }
        Ok(())
    }
    fn evm_transaction(
        &mut self,
        transaction: &crate::custody::EvmTransaction,
    ) -> Result<(), MovementProviderError> {
        self.tag(17);
        self.u64(transaction.chain_id);
        self.u64(transaction.nonce);
        self.u64(transaction.max_priority_fee_per_gas);
        self.u64(transaction.max_fee_per_gas);
        self.u64(transaction.gas_limit);
        self.fixed(&transaction.to);
        self.fixed(&transaction.value);
        self.blob(&transaction.calldata, 262_144)?;
        Ok(())
    }
    fn plan_response(
        &mut self,
        response: &MovementProviderResponse,
    ) -> Result<(), MovementProviderError> {
        let w = self;
        match response {
            MovementProviderResponse::MovePlan(v) => {
                w.tag(1);
                w.blob(&v.canonical_encode(), 1_048_576)?;
            }
            MovementProviderResponse::DepositPlan(v) => {
                w.tag(2);
                w.blob(
                    &crate::journeys::encode_deposit_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?;
            }
            MovementProviderResponse::WithdrawalPlan(v) => {
                w.tag(3);
                w.blob(
                    &crate::journeys::encode_withdrawal_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?;
            }
            MovementProviderResponse::ExitPlan(v) => {
                w.tag(4);
                w.blob(
                    &crate::journeys::encode_exit_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?;
            }
            _ => return Err(MovementProviderError::ContractViolation),
        }
        Ok(())
    }
    fn finish(self) -> Vec<u8> {
        self.out
    }
}
struct MpReader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> MpReader<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self, MovementProviderError> {
        if bytes.len() < 2 || bytes.len() > 1_048_576 || bytes[0] != 2 {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(Self { bytes, at: 1 })
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], MovementProviderError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(MovementProviderError::ContractViolation)?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(MovementProviderError::ContractViolation)?;
        self.at = end;
        Ok(v)
    }
    fn fixed<const N: usize>(&mut self) -> Result<[u8; N], MovementProviderError> {
        self.take(N)?
            .try_into()
            .map_err(|_| MovementProviderError::ContractViolation)
    }
    fn u8(&mut self) -> Result<u8, MovementProviderError> {
        Ok(self.fixed::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, MovementProviderError> {
        Ok(u16::from_be_bytes(self.fixed()?))
    }
    fn u32(&mut self) -> Result<u32, MovementProviderError> {
        Ok(u32::from_be_bytes(self.fixed()?))
    }
    fn u64(&mut self) -> Result<u64, MovementProviderError> {
        Ok(u64::from_be_bytes(self.fixed()?))
    }
    fn u128(&mut self) -> Result<u128, MovementProviderError> {
        Ok(u128::from_be_bytes(self.fixed()?))
    }
    fn text(&mut self, max: usize) -> Result<String, MovementProviderError> {
        let n = self.u16()? as usize;
        if n == 0 || n > max {
            return Err(MovementProviderError::ContractViolation);
        }
        let v = std::str::from_utf8(self.take(n)?)
            .map_err(|_| MovementProviderError::ContractViolation)?;
        if v.chars().any(char::is_control) {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(v.to_owned())
    }
    fn blob(&mut self, max: usize) -> Result<&'a [u8], MovementProviderError> {
        let n = self.u32()? as usize;
        if n == 0 || n > max {
            return Err(MovementProviderError::ContractViolation);
        }
        self.take(n)
    }
    fn planning(&mut self) -> Result<PlanningRequest, MovementProviderError> {
        PlanningRequest::from_wire_parts(PlanningRequest {
            principal: PrincipalId::new(self.text(128)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            tenant: AgentTenantId::new(self.text(255)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            context: self.planning_context()?,
            operation: self.text(128)?,
            idempotency_key: self.fixed()?,
            canonical_body: self.blob(1_048_576)?.to_vec(),
            trace: TraceId::parse(&self.text(64)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            now: self.u64()?,
        })
    }

    fn planning_context(&mut self) -> Result<PlanningContext, MovementProviderError> {
        if self.u8()? != 2 {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(PlanningContext {
            request_anchor: self.fixed()?,
            account: layerx_types::account::AccountId::parse(&self.text(512)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            reserve: layerx_types::account::AccountId::parse(&self.text(512)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            withdrawals_account: layerx_types::account::AccountId::parse(&self.text(512)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            route: match self.u8()? {
                0 => None,
                1 => Some(
                    crate::journeys::RouteRequest::canonical_decode(self.blob(262_144)?)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                ),
                _ => return Err(MovementProviderError::ContractViolation),
            },
            amount: layerx_types::amount::Amount::from_u128(self.u128()?),
            asset: layerx_types::ids::AssetId::new(self.fixed()?),
            currency: self.text(128)?,
            actor: layerx_agent_api::identity::AgentDid::new(self.text(255)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            authority: layerx_agent_api::identity::AuthorityRef::new(self.text(255)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            custody_key: crate::custody::KeyId::new(self.text(128)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            custody_provider_reference: self.blob(4096)?.to_vec(),
            custody_binding_digest: self.fixed()?,
            wallet: layerx_types::intent::EvmAddress::new(self.fixed()?),
            network: layerx_types::intent::NetworkId::new(self.u32()?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            protocol_version: self.u16()?,
            paxeer_chain_id: self.u64()?,
            account_sequence: self.u64()?,
            budget_grant: match self.u8()? {
                0 => None,
                1 => Some(PlanningBudgetGrant {
                    budget: layerx_types::intent::BudgetId::new(self.fixed()?),
                    grant: layerx_types::intent::AuthorityGrantId::new(self.fixed()?),
                }),
                _ => return Err(MovementProviderError::ContractViolation),
            },
            fee_limit: self.u128()?,
            evm_gas_limit: self.u64()?,
            evm_max_fee_per_gas: self.u64()?,
            evm_max_priority_fee_per_gas: self.u64()?,
            not_before: self.u64()?,
            not_after: self.u64()?,
            binding_receipt_digest: self.fixed()?,
            identity_authority_evidence: self.blob(262_144)?.to_vec(),
            balance_evidence: self.blob(262_144)?.to_vec(),
        })
    }
    fn evm_transaction(&mut self) -> Result<crate::custody::EvmTransaction, MovementProviderError> {
        Ok(crate::custody::EvmTransaction {
            chain_id: self.u64()?,
            nonce: self.u64()?,
            max_priority_fee_per_gas: self.u64()?,
            max_fee_per_gas: self.u64()?,
            gas_limit: self.u64()?,
            to: self.fixed()?,
            value: self.fixed()?,
            calldata: self.blob(262_144)?.to_vec(),
        })
    }
    fn execution_identity(
        &mut self,
    ) -> Result<crate::journeys::MovementExecutionIdentity, MovementProviderError> {
        Ok(crate::journeys::MovementExecutionIdentity {
            principal: PrincipalId::new(self.text(128)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            tenant: AgentTenantId::new(self.text(255)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            account: self.fixed()?,
            wallet: layerx_types::intent::EvmAddress::new(self.fixed()?),
            plan_id: self.fixed()?,
        })
    }
    fn wallet_custody(&mut self) -> Result<WalletCustodyRequest, MovementProviderError> {
        let value = WalletCustodyRequest {
            identity: self.execution_identity()?,
            action_key: self.fixed()?,
            wallet: layerx_types::intent::EvmAddress::new(self.fixed()?),
            chain_id: self.u64()?,
            vault: layerx_types::intent::EvmAddress::new(self.fixed()?),
            asset: layerx_types::ids::AssetId::new(self.fixed()?),
            beneficiary: self.fixed()?,
            amount: layerx_types::amount::Amount::from_u128(self.u128()?),
        };
        if value.action_key == [0; 32] || value.chain_id == 0 || value.amount.value() == 0 {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(value)
    }
    fn withdrawal_request(
        &mut self,
    ) -> Result<WithdrawalTransactionRequest, MovementProviderError> {
        let identity = self.execution_identity()?;
        let action_key = self.fixed()?;
        let action = match self.u8()? {
            1 => PaxeerAction::QueueClaim,
            2 => PaxeerAction::FinalisePayout,
            3 => PaxeerAction::CancelChallengedPayout,
            _ => return Err(MovementProviderError::ContractViolation),
        };
        let target = layerx_types::intent::EvmAddress::new(self.fixed()?);
        let calldata = self.blob(262_144)?.to_vec();
        let signed_transaction = match self.u8()? {
            0 => None,
            1 => Some(self.blob(262_144)?.to_vec()),
            _ => return Err(MovementProviderError::ContractViolation),
        };
        if action_key == [0; 32] {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(WithdrawalTransactionRequest {
            signed_transaction,
            identity,
            action_key,
            action,
            target,
            calldata,
        })
    }
    fn exit_request(&mut self) -> Result<ExitWalletRequest, MovementProviderError> {
        let value = ExitWalletRequest {
            identity: self.execution_identity()?,
            action_key: self.fixed()?,
            contract: layerx_types::intent::EvmAddress::new(self.fixed()?),
            calldata: self.blob(262_144)?.to_vec(),
            checkpoint: self.fixed()?,
            withdrawal_id: self.fixed()?,
            nullifier: self.fixed()?,
            recipient: layerx_types::intent::EvmAddress::new(self.fixed()?),
            finalised_balance: self.u128()?,
        };
        if value.action_key == [0; 32]
            || value.checkpoint == [0; 32]
            || value.withdrawal_id == [0; 32]
            || value.nullifier == [0; 32]
            || value.finalised_balance == 0
        {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(value)
    }
    fn finish(self) -> Result<(), MovementProviderError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(MovementProviderError::ContractViolation)
        }
    }
}

/// Server-side contract. The daemon owns the real routing, wallet, Paxeer,
/// proof, and settlement implementations behind this exhaustive dispatch.
pub trait MovementProviderService: Send {
    fn dispatch(&mut self, request: MovementProviderRequest) -> MovementProviderResponse;
}

/// Serves one already-accepted provider connection after authenticating its
/// kernel identity. Listener ownership and concurrency admission remain with
/// the provider daemon so it can apply one process-wide bound.
/// # Errors
/// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
pub fn serve_connection(
    mut stream: UnixStream,
    client_uid: u32,
    group_id: u32,
    maximum_frame_bytes: usize,
    deadline: Duration,
    codec: &dyn MovementProviderCodec,
    service: &mut dyn MovementProviderService,
) -> Result<(), MovementProviderError> {
    if maximum_frame_bytes == 0 || deadline.is_zero() {
        return Err(MovementProviderError::Configuration);
    }
    stream
        .set_read_timeout(Some(deadline))
        .map_err(|_| MovementProviderError::Unavailable)?;
    stream
        .set_write_timeout(Some(deadline))
        .map_err(|_| MovementProviderError::Unavailable)?;
    let credentials = socket_peercred(&stream).map_err(|_| MovementProviderError::Unavailable)?;
    if credentials.uid.as_raw() != client_uid || credentials.gid.as_raw() != group_id {
        return Err(MovementProviderError::ContractViolation);
    }
    let mut header = [0_u8; 10];
    stream
        .read_exact(&mut header)
        .map_err(|_| MovementProviderError::Unavailable)?;
    if u16::from_be_bytes([header[0], header[1]]) != PROTOCOL_VERSION {
        return Err(MovementProviderError::ContractViolation);
    }
    let length = usize::try_from(u64::from_be_bytes(
        header[2..10]
            .try_into()
            .map_err(|_| MovementProviderError::ContractViolation)?,
    ))
    .map_err(|_| MovementProviderError::ContractViolation)?;
    if length == 0 || length > maximum_frame_bytes {
        return Err(MovementProviderError::ContractViolation);
    }
    let mut payload = vec![0; length];
    stream
        .read_exact(&mut payload)
        .map_err(|_| MovementProviderError::Unavailable)?;
    let request = codec.decode_request(&payload)?;
    let response = codec.encode_response(&service.dispatch(request))?;
    if response.is_empty() || response.len() > maximum_frame_bytes {
        return Err(MovementProviderError::ContractViolation);
    }
    stream
        .write_all(&PROTOCOL_VERSION.to_be_bytes())
        .map_err(|_| MovementProviderError::Unavailable)?;
    stream
        .write_all(&(response.len() as u64).to_be_bytes())
        .map_err(|_| MovementProviderError::Unavailable)?;
    stream
        .write_all(&response)
        .map_err(|_| MovementProviderError::Unavailable)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovementProviderError {
    Configuration,
    Unavailable,
    ContractViolation,
}

/// Production UDS client and the runtime adapters consumed by durable journeys.
pub struct UnixMovementProvider {
    config: MovementProviderConfig,
    codec: Arc<dyn MovementProviderCodec>,
    withdrawal_boundary: WithdrawalBoundary,
    execution_authority: Option<Arc<crate::custody::CustodySigner>>,
    planning_authorities: std::collections::BTreeMap<[u8; 32], PlanningRequest>,
}

impl UnixMovementProvider {
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn new(
        config: MovementProviderConfig,
        codec: Arc<dyn MovementProviderCodec>,
        withdrawal_boundary: WithdrawalBoundary,
    ) -> Result<Self, MovementProviderError> {
        validate_socket(&config)?;
        Ok(Self {
            config,
            codec,
            withdrawal_boundary,
            execution_authority: None,
            planning_authorities: std::collections::BTreeMap::new(),
        })
    }

    pub fn attach_execution_authority(&mut self, authority: Arc<crate::custody::CustodySigner>) {
        self.execution_authority = Some(authority);
    }

    fn load_execution_authorities(
        &mut self,
        scope: &PrincipalScope<'_>,
    ) -> Result<(), MovementProviderError> {
        self.planning_authorities.clear();
        for key in scope.keys(Table::Journeys) {
            if !key.as_str().starts_with("movement-authority-") {
                continue;
            }
            let row = scope
                .get(Table::Journeys, &key)
                .ok_or(MovementProviderError::ContractViolation)?;
            let MovementProviderRequest::PlanMove(planning) =
                self.codec.decode_request(row.bytes())?
            else {
                return Err(MovementProviderError::ContractViolation);
            };
            if planning.principal != *scope.principal() || planning.tenant != *scope.tenant() {
                return Err(MovementProviderError::ContractViolation);
            }
            self.planning_authorities
                .insert(planning.idempotency_key, planning);
        }
        Ok(())
    }

    fn authorize_transaction(
        &self,
        identity: &crate::journeys::MovementExecutionIdentity,
        action_key: [u8; 32],
        target: layerx_types::intent::EvmAddress,
        calldata: &[u8],
    ) -> Result<(), MovementProviderError> {
        let planning = self
            .planning_authorities
            .get(&identity.plan_id)
            .ok_or(MovementProviderError::ContractViolation)?;
        let context = &planning.context;
        if planning.principal != identity.principal
            || planning.tenant != identity.tenant
            || context.wallet != identity.wallet
            || layerx_paxeer_client::account_address_for_protocol(
                &context.account,
                context.protocol_version,
            )
            .map_err(|_| MovementProviderError::ContractViolation)?
                != identity.account
        {
            return Err(MovementProviderError::ContractViolation);
        }
        let signer = self
            .execution_authority
            .as_ref()
            .ok_or(MovementProviderError::Unavailable)?;
        let MovementProviderResponse::PreparedEvmTransaction(transaction) =
            self.call(&MovementProviderRequest::PrepareEvmTransaction {
                identity: identity.clone(),
                action_key,
                target,
                calldata: calldata.to_vec(),
            })?
        else {
            return Err(MovementProviderError::ContractViolation);
        };
        if transaction.chain_id != context.paxeer_chain_id
            || transaction.to != target.bytes()
            || transaction.calldata != calldata
            || transaction.value != [0; 32]
            || transaction.gas_limit == 0
            || transaction.max_fee_per_gas == 0
            || transaction.max_priority_fee_per_gas > transaction.max_fee_per_gas
            || transaction.gas_limit != context.evm_gas_limit
            || transaction.max_fee_per_gas != context.evm_max_fee_per_gas
            || transaction.max_priority_fee_per_gas != context.evm_max_priority_fee_per_gas
        {
            return Err(MovementProviderError::ContractViolation);
        }
        let authorization = crate::custody::EvmPlanAuthorization {
            plan_id: identity.plan_id,
            action_key,
            principal: identity.principal.as_str().to_owned(),
            tenant: identity.tenant.as_str().to_owned(),
            binding_digest: context.custody_binding_digest,
            wallet: identity.wallet.bytes(),
            not_before: context.not_before,
            not_after: context.not_after,
            transaction,
        };
        signer
            .authorize_evm_plan(&identity.principal, &context.custody_key, &authorization)
            .map_err(|_| MovementProviderError::ContractViolation)?;
        Ok(())
    }

    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn move_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<AuthorizedMovePlan, MovementProviderError> {
        let context = request.context.clone();
        match self.call(&MovementProviderRequest::PlanMove(request))? {
            MovementProviderResponse::MovePlan(value) => {
                if context.route.as_ref().is_none_or(|route| {
                    crate::journeys::RouteResolver::resolve(route).ok().as_ref()
                        != Some(value.plan.route())
                }) {
                    return Err(MovementProviderError::ContractViolation);
                }
                Ok(value)
            }
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn quote_move(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        request: PlanningRequest,
    ) -> Result<AuthorizedMovePlan, MovementProviderError> {
        let quote = self.move_plan(request)?;
        let key = quote_row(&quote.quote_id)?;
        let bytes = self
            .codec
            .encode_response(&MovementProviderResponse::MovePlan(quote.clone()))?;
        if bytes.len() > self.config.maximum_frame_bytes {
            return Err(MovementProviderError::ContractViolation);
        }
        scope
            .put(Table::Journeys, key, quote.expires_at, bytes)
            .map_err(|_| MovementProviderError::Unavailable)?;
        Ok(quote)
    }
    /// # Errors
    /// Refuses missing, expired, or invalid principal-scoped quotes.
    pub fn load_move_quote(
        &self,
        scope: &PrincipalScope<'_>,
        quote_id: &str,
        commit_key: [u8; 32],
        now: u64,
    ) -> Result<MovePlan, MovementProviderError> {
        let row = scope
            .get(Table::Journeys, &quote_row(quote_id)?)
            .ok_or(MovementProviderError::ContractViolation)?;
        match self.codec.decode_response(row.bytes())? {
            MovementProviderResponse::MovePlan(value)
                if value.quote_id == quote_id && value.expires_at >= now =>
            {
                value
                    .plan
                    .with_idempotency_key(commit_key)
                    .map_err(|_| MovementProviderError::ContractViolation)
            }
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn deposit_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<DepositPlan, MovementProviderError> {
        let context = request.context.clone();
        let plan_id = request.idempotency_key;
        match self.call(&MovementProviderRequest::PlanDeposit(request))? {
            MovementProviderResponse::DepositPlan(value)
                if value.idempotency_key == plan_id
                    && value.recipient == context.account
                    && value.reserve == context.reserve
                    && value.wallet == context.wallet
                    && value.asset == context.asset
                    && value.amount == context.amount
                    && value.network == context.network
                    && value.layerx_network == context.network
                    && value.layerx_protocol_version == context.protocol_version
                    && value.paxeer_chain_id == context.paxeer_chain_id
                    && value.currency == context.currency
                    && value.agent.custody_key == context.custody_key
                    && value.agent.actor == context.actor
                    && value.agent.authority == context.authority
                    && value.agent.account_sequence == context.account_sequence
                    && value.agent.not_before == context.not_before
                    && value.agent.not_after == context.not_after
                    && value.agent.fee_limit == context.fee_limit =>
            {
                Ok(value)
            }
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn withdrawal_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<WithdrawalPlan, MovementProviderError> {
        let context = request.context.clone();
        let plan_id = request.idempotency_key;
        match self.call(&MovementProviderRequest::PlanWithdrawal(request))? {
            MovementProviderResponse::WithdrawalPlan(value)
                if value.idempotency_key == plan_id
                    && value.owner == context.account
                    && value.request_anchor.bytes() == context.request_anchor
                    && value.withdrawals_account == context.withdrawals_account
                    && value.payout_address == context.wallet
                    && value.asset == context.asset
                    && value.amount == context.amount
                    && value.currency == context.currency
                    && value.network == context.network
                    && value.layerx_protocol_version == context.protocol_version
                    && value.agent.custody_key == context.custody_key
                    && value.agent.actor == context.actor
                    && value.agent.authority == context.authority
                    && value.agent.account_sequence == context.account_sequence
                    && value.agent.not_before == context.not_before
                    && value.agent.not_after == context.not_after
                    && value.agent.fee_limit == context.fee_limit =>
            {
                Ok(value)
            }
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn exit_plan(&self, request: PlanningRequest) -> Result<ExitPlan, MovementProviderError> {
        let context = request.context.clone();
        let plan_id = request.idempotency_key;
        match self.call(&MovementProviderRequest::PlanExit(request))? {
            MovementProviderResponse::ExitPlan(value)
                if value.idempotency_key == plan_id
                    && value.evidence.account
                        == layerx_paxeer_client::account_address_for_protocol(
                            &context.account,
                            context.protocol_version,
                        )
                        .map_err(|_| MovementProviderError::ContractViolation)?
                    && value.evidence.recipient == context.wallet
                    && value.evidence.asset_id == context.asset.bytes()
                    && value.evidence.finalised_balance == context.amount.value() =>
            {
                Ok(value)
            }
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    #[must_use]
    pub fn ready(&self) -> bool {
        matches!(
            self.call(&MovementProviderRequest::Readiness),
            Ok(MovementProviderResponse::Ready)
        )
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn claim_withdrawal(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        journey: &mut WithdrawalJourney,
        signature: &[u8],
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        self.load_execution_authorities(scope).map_err(|_| {
            WithdrawalJourneyError::Boundary(WithdrawalBoundaryError::ContractViolation)
        })?;
        let boundary = self.withdrawal_boundary.clone();
        journey.claim_external_signature(scope, self, &boundary, signature, now)
    }
    #[allow(clippy::too_many_arguments)]
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn advance_deposit<A: crate::journeys::DepositAgentBoundary>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        journey: &mut crate::journeys::DepositJourney,
        agent_contract: &layerx_sdk::Client,
        agent: &mut A,
        custody: &crate::custody::CustodySigner,
        registry: &layerx_types::payload::ModuleRegistry,
        trace: &TraceId,
        now: u64,
    ) -> Result<crate::journeys::DepositStatus, crate::journeys::DepositJourneyError> {
        self.load_execution_authorities(scope).map_err(|_| {
            crate::journeys::DepositJourneyError::Boundary(DepositBoundaryError::ContractViolation)
        })?;
        crate::server::poll_once_ready(journey.advance(
            scope,
            self,
            agent_contract,
            agent,
            custody,
            registry,
            trace,
            now,
        ))
        .map_err(|_| {
            crate::journeys::DepositJourneyError::Boundary(DepositBoundaryError::Unavailable)
        })?
    }
    #[allow(clippy::too_many_arguments)]
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn advance_withdrawal<A: crate::journeys::AgentBoundary>(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        journey: &mut WithdrawalJourney,
        agent_contract: &layerx_sdk::Client,
        agent: &mut A,
        custody: &crate::custody::CustodySigner,
        registry: &layerx_types::payload::ModuleRegistry,
        trace: &TraceId,
        step_up: Option<&crate::custody::StepUpEvidence>,
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        self.load_execution_authorities(scope).map_err(|_| {
            WithdrawalJourneyError::Boundary(WithdrawalBoundaryError::ContractViolation)
        })?;
        let boundary = self.withdrawal_boundary.clone();
        crate::server::poll_once_ready(journey.advance(
            scope,
            self,
            &boundary,
            agent_contract,
            agent,
            custody,
            registry,
            trace,
            step_up,
            now,
        ))
        .map_err(|_| WithdrawalJourneyError::Boundary(WithdrawalBoundaryError::Unavailable))?
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn advance_exit(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        trace: &TraceId,
        exit: &EmergencyExit,
        journey: &mut ExitJourney,
        now: u64,
    ) -> Result<ExitStatus, ExitJourneyError> {
        self.load_execution_authorities(scope)
            .map_err(|_| ExitJourneyError::Boundary(ExitBoundaryError::ContractViolation))?;
        let mut audit = AuditChain::open(scope)?;
        journey.advance(scope, &mut audit, trace, exit, self, now)
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn start_withdrawal(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        plan: &WithdrawalPlan,
        now: u64,
    ) -> Result<WithdrawalJourney, WithdrawalJourneyError> {
        self.load_execution_authorities(scope).map_err(|_| {
            WithdrawalJourneyError::Boundary(WithdrawalBoundaryError::ContractViolation)
        })?;
        WithdrawalJourney::start(scope, plan, now)
    }
    /// # Errors
    /// Refuses invalid or unavailable movement authority, transport, or canonical evidence.
    pub fn start_exit(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        trace: &TraceId,
        exit: &EmergencyExit,
        plan: &ExitPlan,
        confirmation: IrreversibleExitConfirmation,
        now: u64,
    ) -> Result<ExitStatus, ExitJourneyError> {
        self.load_execution_authorities(scope)
            .map_err(|_| ExitJourneyError::Boundary(ExitBoundaryError::ContractViolation))?;
        let mut audit = AuditChain::open(scope)?;
        let mut journey = ExitJourney::start(scope, &mut audit, trace, plan, confirmation, now)?;
        journey.advance(scope, &mut audit, trace, exit, self, now)
    }

    fn call(
        &self,
        request: &MovementProviderRequest,
    ) -> Result<MovementProviderResponse, MovementProviderError> {
        validate_socket(&self.config)?;
        let mut stream = UnixStream::connect(&self.config.socket)
            .map_err(|_| MovementProviderError::Unavailable)?;
        stream
            .set_read_timeout(Some(self.config.deadline))
            .map_err(|_| MovementProviderError::Unavailable)?;
        stream
            .set_write_timeout(Some(self.config.deadline))
            .map_err(|_| MovementProviderError::Unavailable)?;
        let credentials =
            socket_peercred(&stream).map_err(|_| MovementProviderError::Unavailable)?;
        if credentials.uid.as_raw() != self.config.peer_uid
            || credentials.gid.as_raw() != self.config.peer_gid
        {
            return Err(MovementProviderError::ContractViolation);
        }
        let payload = self.codec.encode_request(request)?;
        if payload.is_empty() || payload.len() > self.config.maximum_frame_bytes {
            return Err(MovementProviderError::ContractViolation);
        }
        stream
            .write_all(&PROTOCOL_VERSION.to_be_bytes())
            .map_err(|_| MovementProviderError::Unavailable)?;
        stream
            .write_all(&(payload.len() as u64).to_be_bytes())
            .map_err(|_| MovementProviderError::Unavailable)?;
        stream
            .write_all(&payload)
            .map_err(|_| MovementProviderError::Unavailable)?;
        let mut header = [0_u8; 10];
        stream
            .read_exact(&mut header)
            .map_err(|_| MovementProviderError::Unavailable)?;
        if u16::from_be_bytes([header[0], header[1]]) != PROTOCOL_VERSION {
            return Err(MovementProviderError::ContractViolation);
        }
        let length = usize::try_from(u64::from_be_bytes(
            header[2..10]
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        ))
        .map_err(|_| MovementProviderError::ContractViolation)?;
        if length == 0 || length > self.config.maximum_frame_bytes {
            return Err(MovementProviderError::ContractViolation);
        }
        let mut response = vec![0; length];
        stream
            .read_exact(&mut response)
            .map_err(|_| MovementProviderError::Unavailable)?;
        self.codec.decode_response(&response)
    }
}

impl DepositRuntime for UnixMovementProvider {
    fn verify_external_deposit(
        &mut self,
        request: &WalletCustodyRequest,
        transaction: TransactionHash,
    ) -> Result<TransactionHash, DepositBoundaryError> {
        match self
            .call(&MovementProviderRequest::VerifyExternalDeposit {
                request: request.clone(),
                transaction,
            })
            .map_err(deposit_error)?
        {
            MovementProviderResponse::VerifiedDeposit(value) => Ok(value),
            _ => Err(DepositBoundaryError::ContractViolation),
        }
    }
    fn submit_custody(
        &mut self,
        request: &WalletCustodyRequest,
    ) -> Result<WalletCustodyOutcome, DepositBoundaryError> {
        use sha3::Digest as _;
        let selector = sha3::Keccak256::digest(b"deposit(bytes32,uint256,bytes32)");
        let mut calldata = selector[..4].to_vec();
        calldata.extend(request.asset.bytes());
        calldata.extend([0; 16]);
        calldata.extend(request.amount.to_be_bytes());
        calldata.extend(request.beneficiary);
        self.authorize_transaction(
            &request.identity,
            request.action_key,
            request.vault,
            &calldata,
        )
        .map_err(deposit_error)?;
        match self
            .call(&MovementProviderRequest::SubmitDepositCustody(
                request.clone(),
            ))
            .map_err(deposit_error)?
        {
            MovementProviderResponse::DepositCustody(value) => Ok(value),
            _ => Err(DepositBoundaryError::ContractViolation),
        }
    }
    fn poll_finality(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<FinalityReport, DepositBoundaryError> {
        match self
            .call(&MovementProviderRequest::PollDepositFinality(transaction))
            .map_err(deposit_error)?
        {
            MovementProviderResponse::DepositFinality(value) => Ok(value),
            _ => Err(DepositBoundaryError::ContractViolation),
        }
    }
    fn obtain_proof(
        &mut self,
        transaction: TransactionHash,
    ) -> Result<DepositProof, DepositFailure> {
        match self.call(&MovementProviderRequest::ObtainDepositProof(transaction)) {
            Ok(MovementProviderResponse::DepositProof(value)) => value,
            Err(MovementProviderError::Unavailable) => Err(DepositFailure::ProofUnavailable(
                layerx_paxeer_client::ProofFault::ProducerUnavailable,
            )),
            _ => Err(DepositFailure::ProofUnavailable(
                layerx_paxeer_client::ProofFault::EvidenceSourceMismatch,
            )),
        }
    }
}

impl WithdrawalRuntime for UnixMovementProvider {
    fn verify_claim_signature(
        &mut self,
        request: &WithdrawalTransactionRequest,
        signature: &[u8],
    ) -> Result<Vec<u8>, WithdrawalBoundaryError> {
        self.authorize_transaction(
            &request.identity,
            request.action_key,
            request.target,
            &request.calldata,
        )
        .map_err(withdrawal_error)?;
        match self
            .call(&MovementProviderRequest::VerifyClaimSignature {
                request: request.clone(),
                signature: signature.to_vec(),
            })
            .map_err(withdrawal_error)?
        {
            MovementProviderResponse::ClaimTransaction(value) => Ok(value),
            _ => Err(WithdrawalBoundaryError::ContractViolation),
        }
    }
    fn checkpoint_proof(
        &mut self,
        debit: &DebitExpectation,
    ) -> Result<Option<CheckpointProof>, WithdrawalBoundaryError> {
        match self
            .call(&MovementProviderRequest::CheckpointProof(*debit))
            .map_err(withdrawal_error)?
        {
            MovementProviderResponse::CheckpointProof(value) => Ok(value),
            _ => Err(WithdrawalBoundaryError::ContractViolation),
        }
    }
    fn submit_or_resolve(
        &mut self,
        request: &WithdrawalTransactionRequest,
    ) -> Result<PaxeerActionOutcome, WithdrawalBoundaryError> {
        self.authorize_transaction(
            &request.identity,
            request.action_key,
            request.target,
            &request.calldata,
        )
        .map_err(withdrawal_error)?;
        match self
            .call(&MovementProviderRequest::SubmitWithdrawal(request.clone()))
            .map_err(withdrawal_error)?
        {
            MovementProviderResponse::Withdrawal(value) => Ok(value),
            _ => Err(WithdrawalBoundaryError::ContractViolation),
        }
    }
    fn lookup(
        &mut self,
        key: [u8; 32],
    ) -> Result<Option<TransactionHash>, WithdrawalBoundaryError> {
        match self
            .call(&MovementProviderRequest::LookupWithdrawal(key))
            .map_err(withdrawal_error)?
        {
            MovementProviderResponse::WithdrawalLookup(value) => Ok(value),
            _ => Err(WithdrawalBoundaryError::ContractViolation),
        }
    }
}

impl ExitWallet for UnixMovementProvider {
    fn submit_or_resolve(
        &mut self,
        request: &ExitWalletRequest,
    ) -> Result<ExitWalletOutcome, ExitBoundaryError> {
        self.authorize_transaction(
            &request.identity,
            request.action_key,
            request.contract,
            &request.calldata,
        )
        .map_err(exit_error)?;
        match self
            .call(&MovementProviderRequest::SubmitExit(request.clone()))
            .map_err(exit_error)?
        {
            MovementProviderResponse::Exit(value) => Ok(value),
            _ => Err(ExitBoundaryError::ContractViolation),
        }
    }
}

fn validate_socket(config: &MovementProviderConfig) -> Result<(), MovementProviderError> {
    let metadata =
        fs::symlink_metadata(&config.socket).map_err(|_| MovementProviderError::Unavailable)?;
    if !metadata.file_type().is_socket()
        || metadata.file_type().is_symlink()
        || metadata.uid() != config.peer_uid
        || metadata.gid() != config.peer_gid
        || metadata.mode() & 0o007 != 0
    {
        return Err(MovementProviderError::ContractViolation);
    }
    let parent = config
        .socket
        .parent()
        .ok_or(MovementProviderError::Configuration)?;
    validate_parent(parent, config.peer_uid, config.peer_gid)
}
fn validate_parent(path: &Path, uid: u32, gid: u32) -> Result<(), MovementProviderError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| MovementProviderError::Unavailable)?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != uid
        || metadata.gid() != gid
        || metadata.mode() & 0o007 != 0
    {
        Err(MovementProviderError::ContractViolation)
    } else {
        Ok(())
    }
}
fn quote_row(value: &str) -> Result<RowKey, MovementProviderError> {
    if !(16..=128).contains(&value.len())
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(MovementProviderError::ContractViolation);
    }
    RowKey::new(format!("movement-quote-{value}"))
        .map_err(|_| MovementProviderError::ContractViolation)
}
fn required(name: &str) -> Result<String, MovementProviderError> {
    env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or(MovementProviderError::Configuration)
}
fn number<T: std::str::FromStr>(name: &str) -> Result<T, MovementProviderError> {
    required(name)?
        .parse()
        .map_err(|_| MovementProviderError::Configuration)
}
fn deposit_error(error: MovementProviderError) -> DepositBoundaryError {
    match error {
        MovementProviderError::Unavailable => DepositBoundaryError::Unavailable,
        _ => DepositBoundaryError::ContractViolation,
    }
}
fn withdrawal_error(error: MovementProviderError) -> WithdrawalBoundaryError {
    match error {
        MovementProviderError::Unavailable => WithdrawalBoundaryError::Unavailable,
        _ => WithdrawalBoundaryError::ContractViolation,
    }
}
fn exit_error(error: MovementProviderError) -> ExitBoundaryError {
    match error {
        MovementProviderError::Unavailable => ExitBoundaryError::Unavailable,
        _ => ExitBoundaryError::ContractViolation,
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use k256::ecdsa::SigningKey;
    use layerx_paxeer_client::WithdrawalAttestation;
    use layerx_types::intent::EvmAddress;
    use sha2::{Digest, Sha256};
    use sha3::Keccak256;

    #[test]
    fn prior_movement_shape_is_refused() -> Result<(), String> {
        let codec = NativeMovementCodec::new();
        assert_eq!(
            checked(codec.encode_request(&MovementProviderRequest::Readiness))?,
            vec![2, 14]
        );
        assert!(codec.decode_request(&[1, 14]).is_err());
        assert!(codec.decode_response(&[1, 14]).is_err());
        Ok(())
    }

    fn signed_proof(protocol_version: u16) -> Result<CheckpointProof, String> {
        let key = checked(SigningKey::from_slice(&[7; 32]))?;
        let public = key.verifying_key().to_encoded_point(false);
        let public_hash = Keccak256::digest(&public.as_bytes()[1..]);
        let signer = EvmAddress::new(checked(public_hash[12..].try_into())?);
        let mut attestation = WithdrawalAttestation {
            protocol_version,
            network_id: 42,
            paxeer_chain_id: 31337,
            settlement_contract: EvmAddress::new([8; 20]),
            epoch: 1,
            checkpoint_id: [1; 32],
            checkpoint_hash: [1; 32],
            guarantor_id: [2; 32],
            batch_number: 1,
            data_availability_root: [3; 32],
            replayed: true,
            data_available: true,
            availability_class_mask: 31,
            attested_at: 1,
            signer,
            signature_r: [0; 32],
            signature_s: [0; 32],
            signature_v: 27,
        };
        let mut message = Vec::new();
        message.extend(protocol_version.to_be_bytes());
        message.extend(attestation.network_id.to_be_bytes());
        message.extend(attestation.paxeer_chain_id.to_be_bytes());
        message.extend(attestation.settlement_contract.bytes());
        message.extend(attestation.epoch.to_be_bytes());
        message.extend(attestation.checkpoint_id);
        message.extend(attestation.checkpoint_hash);
        message.extend(attestation.guarantor_id);
        message.extend(attestation.batch_number.to_be_bytes());
        message.extend(attestation.data_availability_root);
        message.extend([1, 1, 31]);
        message.extend(attestation.attested_at.to_be_bytes());
        let mut hash = Sha256::new();
        hash.update(b"LXP/v2/guarantor-attestation\0");
        hash.update(message);
        let (signature, recovery) = checked(key.sign_prehash_recoverable(&hash.finalize()))?;
        attestation
            .signature_r
            .copy_from_slice(&signature.to_bytes()[..32]);
        attestation
            .signature_s
            .copy_from_slice(&signature.to_bytes()[32..]);
        attestation.signature_v = recovery.to_byte() + 27;
        checked(CheckpointProof::validated_for_protocol(
            protocol_version,
            [1; 32],
            [4; 32],
            1,
            1,
            [3; 32],
            0,
            Vec::new(),
            vec![attestation],
        ))
    }

    #[test]
    fn selected_checkpoint_codec_roundtrips_signed_proofs_and_refuses_cross_version(
    ) -> Result<(), String> {
        for protocol in [2, 3] {
            let codec = checked(NativeMovementCodec::for_protocol(protocol))?;
            let other = checked(NativeMovementCodec::for_protocol(if protocol == 2 {
                3
            } else {
                2
            }))?;
            let response = MovementProviderResponse::CheckpointProof(Some(signed_proof(protocol)?));
            let encoded = checked(codec.encode_response(&response))?;
            assert_eq!(checked(codec.decode_response(&encoded))?, response);
            assert!(other.encode_response(&response).is_err());
            assert!(other.decode_response(&encoded).is_err());
            if protocol == 2 {
                assert_eq!(
                    checked(NativeMovementCodec::default().encode_response(&response))?,
                    encoded
                );
            }
        }
        for unsupported in [0, 1, 4, u16::MAX] {
            assert!(NativeMovementCodec::for_protocol(unsupported).is_err());
        }
        Ok(())
    }

    fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
        result.map_err(|error| format!("{error:?}"))
    }
}
