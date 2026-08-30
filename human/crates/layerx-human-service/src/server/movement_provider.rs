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

const PROTOCOL_VERSION: u16 = 1;

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
    pub fn from_environment() -> Result<Self, MovementProviderError> {
        let socket = required("LAYERX_HUMAN_MOVEMENT_SOCKET").map(PathBuf::from)?;
        if !socket.is_absolute() {
            return Err(MovementProviderError::Configuration);
        }
        let peer_uid = number("LAYERX_HUMAN_MOVEMENT_PEER_UID")?;
        let peer_gid = number("LAYERX_HUMAN_MOVEMENT_PEER_GID")?;
        let maximum_frame_bytes = number("LAYERX_HUMAN_MOVEMENT_MAX_FRAME_BYTES")?;
        let deadline_seconds: u64 = number("LAYERX_HUMAN_MOVEMENT_DEADLINE_SECONDS")?;
        if maximum_frame_bytes == 0 || deadline_seconds == 0 {
            return Err(MovementProviderError::Configuration);
        }
        Ok(Self {
            socket,
            peer_uid,
            peer_gid,
            maximum_frame_bytes,
            deadline: Duration::from_secs(deadline_seconds),
        })
    }
}

/// Exact caller authority and request bytes used when constructing a plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanningRequest {
    pub principal: PrincipalId,
    pub tenant: AgentTenantId,
    pub operation: String,
    pub idempotency_key: [u8; 32],
    pub canonical_body: Vec<u8>,
    pub trace: TraceId,
    pub now: u64,
}

impl PlanningRequest {
    /// Reconstructs canonical provider fields while enforcing boundary bounds.
    pub fn from_wire_parts(
        principal: PrincipalId,
        tenant: AgentTenantId,
        operation: String,
        idempotency_key: [u8; 32],
        canonical_body: Vec<u8>,
        trace: TraceId,
        now: u64,
    ) -> Result<Self, MovementProviderError> {
        if operation.is_empty()
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
        out.extend((self.quote_id.len() as u16).to_be_bytes());
        out.extend(self.quote_id.as_bytes());
        out.extend(self.expires_at.to_be_bytes());
        out.extend(self.arrival_at.to_be_bytes());
        out.extend((plan.len() as u32).to_be_bytes());
        out.extend(plan);
        out
    }

    /// Decodes a quote envelope, rejects trailing bytes, and revalidates its plan.
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
        if quote_len < 16 || quote_len > 128 {
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
        drop(take);
        if at != bytes.len() {
            return Err(MovementProviderError::ContractViolation);
        }
        Self::from_wire_parts(quote_id, expires_at, arrival_at, plan)
    }

    /// Constructs one provider-owned quote with a non-empty review window.
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
    Ready,
    Unavailable,
    ContractViolation,
}

/// Authoritative native codec shared by the provider daemon and its client.
/// Implementations must encode every field and validate canonicality, bounds,
/// protocol version, and native proof construction while decoding.
pub trait MovementProviderCodec: Send + Sync {
    fn encode_request(
        &self,
        request: &MovementProviderRequest,
    ) -> Result<Vec<u8>, MovementProviderError>;
    fn decode_request(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderRequest, MovementProviderError>;
    fn encode_response(
        &self,
        response: &MovementProviderResponse,
    ) -> Result<Vec<u8>, MovementProviderError>;
    fn decode_response(
        &self,
        bytes: &[u8],
    ) -> Result<MovementProviderResponse, MovementProviderError>;
}

/// Canonical native codec. Every nested economic object delegates to its
/// owner module's validated wire representation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMovementCodec;
impl NativeMovementCodec {
    #[must_use]
    pub const fn new() -> Self {
        Self
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
                w.planning(v)?
            }
            MovementProviderRequest::PlanDeposit(v) => {
                w.tag(2);
                w.planning(v)?
            }
            MovementProviderRequest::PlanWithdrawal(v) => {
                w.tag(3);
                w.planning(v)?
            }
            MovementProviderRequest::PlanExit(v) => {
                w.tag(4);
                w.planning(v)?
            }
            MovementProviderRequest::VerifyExternalDeposit {
                request,
                transaction,
            } => {
                w.tag(5);
                w.wallet_custody(request)?;
                w.fixed(&transaction.bytes())
            }
            MovementProviderRequest::SubmitDepositCustody(v) => {
                w.tag(6);
                w.wallet_custody(v)?
            }
            MovementProviderRequest::PollDepositFinality(v) => {
                w.tag(7);
                w.fixed(&v.bytes())
            }
            MovementProviderRequest::ObtainDepositProof(v) => {
                w.tag(8);
                w.fixed(&v.bytes())
            }
            MovementProviderRequest::VerifyClaimSignature { request, signature } => {
                w.tag(9);
                w.withdrawal_request(request)?;
                w.blob(signature, 262_144)?
            }
            MovementProviderRequest::CheckpointProof(v) => {
                w.tag(10);
                w.blob(
                    &layerx_paxeer_client::wire::encode_debit_expectation(v, 4096)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    4096,
                )?
            }
            MovementProviderRequest::SubmitWithdrawal(v) => {
                w.tag(11);
                w.withdrawal_request(v)?
            }
            MovementProviderRequest::LookupWithdrawal(v) => {
                w.tag(12);
                w.fixed(v)
            }
            MovementProviderRequest::SubmitExit(v) => {
                w.tag(13);
                w.exit_request(v)?
            }
            MovementProviderRequest::Readiness => w.tag(14),
        };
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
            MovementProviderResponse::MovePlan(v) => {
                w.tag(1);
                w.blob(&v.canonical_encode(), 1_048_576)?
            }
            MovementProviderResponse::DepositPlan(v) => {
                w.tag(2);
                w.blob(
                    &crate::journeys::encode_deposit_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?
            }
            MovementProviderResponse::WithdrawalPlan(v) => {
                w.tag(3);
                w.blob(
                    &crate::journeys::encode_withdrawal_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?
            }
            MovementProviderResponse::ExitPlan(v) => {
                w.tag(4);
                w.blob(
                    &crate::journeys::encode_exit_plan(v)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    1_048_576,
                )?
            }
            MovementProviderResponse::VerifiedDeposit(v) => {
                w.tag(5);
                w.fixed(&v.bytes())
            }
            MovementProviderResponse::DepositCustody(v) => {
                w.tag(6);
                match v {
                    WalletCustodyOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes())
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
                )?
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
                )?
            }
            MovementProviderResponse::DepositProof(Err(v)) => {
                w.tag(8);
                w.u8(2);
                w.blob(
                    &layerx_paxeer_client::wire::encode_deposit_failure(v, 262_144)
                        .map_err(|_| MovementProviderError::ContractViolation)?,
                    262_144,
                )?
            }
            MovementProviderResponse::ClaimTransaction(v) => {
                w.tag(9);
                w.blob(v, 262_144)?
            }
            MovementProviderResponse::CheckpointProof(v) => {
                w.tag(10);
                match v {
                    None => w.u8(0),
                    Some(p) => {
                        w.u8(1);
                        w.blob(
                            &layerx_paxeer_client::wire::encode_checkpoint_proof(p, 1_048_576)
                                .map_err(|_| MovementProviderError::ContractViolation)?,
                            1_048_576,
                        )?
                    }
                }
            }
            MovementProviderResponse::Withdrawal(v) => {
                w.tag(11);
                match v {
                    PaxeerActionOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes())
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
                        w.fixed(&h.bytes())
                    }
                }
            }
            MovementProviderResponse::Exit(v) => {
                w.tag(13);
                match v {
                    ExitWalletOutcome::Submitted(h) => {
                        w.u8(1);
                        w.fixed(&h.bytes())
                    }
                    ExitWalletOutcome::Rejected => w.u8(2),
                }
            }
            MovementProviderResponse::Ready => w.tag(14),
            MovementProviderResponse::Unavailable => w.tag(15),
            MovementProviderResponse::ContractViolation => w.tag(16),
        };
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
                    layerx_paxeer_client::wire::decode_checkpoint_proof(
                        r.blob(1_048_576)?,
                        1_048_576,
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
        Self { out: vec![1] }
    }
    fn tag(&mut self, v: u8) {
        self.u8(v)
    }
    fn u8(&mut self, v: u8) {
        self.out.push(v)
    }
    fn u32(&mut self, v: u32) {
        self.out.extend(v.to_be_bytes())
    }
    fn u64(&mut self, v: u64) {
        self.out.extend(v.to_be_bytes())
    }
    fn fixed(&mut self, v: &[u8]) {
        self.out.extend(v)
    }
    fn text(&mut self, v: &str, max: usize) -> Result<(), MovementProviderError> {
        if v.is_empty()
            || v.len() > max
            || v.len() > u16::MAX as usize
            || v.chars().any(char::is_control)
        {
            return Err(MovementProviderError::ContractViolation);
        }
        self.out.extend((v.len() as u16).to_be_bytes());
        self.out.extend(v.as_bytes());
        Ok(())
    }
    fn blob(&mut self, v: &[u8], max: usize) -> Result<(), MovementProviderError> {
        if v.is_empty() || v.len() > max || v.len() > u32::MAX as usize {
            return Err(MovementProviderError::ContractViolation);
        }
        self.u32(v.len() as u32);
        self.out.extend(v);
        Ok(())
    }
    fn planning(&mut self, v: &PlanningRequest) -> Result<(), MovementProviderError> {
        self.text(v.principal.as_str(), 128)?;
        self.text(v.tenant.as_str(), 255)?;
        self.text(&v.operation, 128)?;
        self.fixed(&v.idempotency_key);
        self.blob(&v.canonical_body, 1_048_576)?;
        self.text(v.trace.as_str(), 64)?;
        self.u64(v.now);
        Ok(())
    }
    fn wallet_custody(&mut self, v: &WalletCustodyRequest) -> Result<(), MovementProviderError> {
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
        self.blob(&v.calldata, 262_144)
    }
    fn exit_request(&mut self, v: &ExitWalletRequest) -> Result<(), MovementProviderError> {
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
        if bytes.len() < 2 || bytes.len() > 1_048_576 || bytes[0] != 1 {
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
        PlanningRequest::from_wire_parts(
            PrincipalId::new(self.text(128)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            AgentTenantId::new(self.text(255)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            self.text(128)?,
            self.fixed()?,
            self.blob(1_048_576)?.to_vec(),
            TraceId::parse(&self.text(64)?)
                .map_err(|_| MovementProviderError::ContractViolation)?,
            self.u64()?,
        )
    }
    fn wallet_custody(&mut self) -> Result<WalletCustodyRequest, MovementProviderError> {
        let value = WalletCustodyRequest {
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
        let action_key = self.fixed()?;
        let action = match self.u8()? {
            1 => PaxeerAction::QueueClaim,
            2 => PaxeerAction::FinalisePayout,
            3 => PaxeerAction::CancelChallengedPayout,
            _ => return Err(MovementProviderError::ContractViolation),
        };
        let target = layerx_types::intent::EvmAddress::new(self.fixed()?);
        let calldata = self.blob(262_144)?.to_vec();
        if action_key == [0; 32] {
            return Err(MovementProviderError::ContractViolation);
        }
        Ok(WithdrawalTransactionRequest {
            action_key,
            action,
            target,
            calldata,
        })
    }
    fn exit_request(&mut self) -> Result<ExitWalletRequest, MovementProviderError> {
        let value = ExitWalletRequest {
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
pub fn serve_connection(
    mut stream: UnixStream,
    client_uid: u32,
    client_gid: u32,
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
    if credentials.uid.as_raw() != client_uid || credentials.gid.as_raw() != client_gid {
        return Err(MovementProviderError::ContractViolation);
    }
    let mut header = [0_u8; 10];
    stream
        .read_exact(&mut header)
        .map_err(|_| MovementProviderError::Unavailable)?;
    if u16::from_be_bytes([header[0], header[1]]) != PROTOCOL_VERSION {
        return Err(MovementProviderError::ContractViolation);
    }
    let length = u64::from_be_bytes(
        header[2..10]
            .try_into()
            .map_err(|_| MovementProviderError::ContractViolation)?,
    ) as usize;
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
}

impl UnixMovementProvider {
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
        })
    }

    pub fn move_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<AuthorizedMovePlan, MovementProviderError> {
        match self.call(MovementProviderRequest::PlanMove(request))? {
            MovementProviderResponse::MovePlan(value) => Ok(value),
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
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
    pub fn deposit_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<DepositPlan, MovementProviderError> {
        match self.call(MovementProviderRequest::PlanDeposit(request))? {
            MovementProviderResponse::DepositPlan(value) => Ok(value),
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    pub fn withdrawal_plan(
        &self,
        request: PlanningRequest,
    ) -> Result<WithdrawalPlan, MovementProviderError> {
        match self.call(MovementProviderRequest::PlanWithdrawal(request))? {
            MovementProviderResponse::WithdrawalPlan(value) => Ok(value),
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    pub fn exit_plan(&self, request: PlanningRequest) -> Result<ExitPlan, MovementProviderError> {
        match self.call(MovementProviderRequest::PlanExit(request))? {
            MovementProviderResponse::ExitPlan(value) => Ok(value),
            _ => Err(MovementProviderError::ContractViolation),
        }
    }
    pub fn ready(&self) -> bool {
        matches!(
            self.call(MovementProviderRequest::Readiness),
            Ok(MovementProviderResponse::Ready)
        )
    }
    pub fn claim_withdrawal(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        journey: &mut WithdrawalJourney,
        signature: &[u8],
        now: u64,
    ) -> Result<WithdrawalStatus, WithdrawalJourneyError> {
        let boundary = self.withdrawal_boundary.clone();
        journey.claim_external_signature(scope, self, &boundary, signature, now)
    }
    #[allow(clippy::too_many_arguments)]
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
    pub fn advance_exit(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        trace: &TraceId,
        exit: &EmergencyExit,
        journey: &mut ExitJourney,
        now: u64,
    ) -> Result<ExitStatus, ExitJourneyError> {
        let mut audit = AuditChain::open(scope)?;
        journey.advance(scope, &mut audit, trace, exit, self, now)
    }
    pub fn start_withdrawal(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        plan: &WithdrawalPlan,
        now: u64,
    ) -> Result<WithdrawalJourney, WithdrawalJourneyError> {
        WithdrawalJourney::start(scope, plan, now)
    }
    pub fn start_exit(
        &mut self,
        scope: &mut PrincipalScope<'_>,
        trace: &TraceId,
        exit: &EmergencyExit,
        plan: &ExitPlan,
        confirmation: IrreversibleExitConfirmation,
        now: u64,
    ) -> Result<ExitStatus, ExitJourneyError> {
        let mut audit = AuditChain::open(scope)?;
        let mut journey = ExitJourney::start(scope, &mut audit, trace, plan, confirmation, now)?;
        journey.advance(scope, &mut audit, trace, exit, self, now)
    }

    fn call(
        &self,
        request: MovementProviderRequest,
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
        let payload = self.codec.encode_request(&request)?;
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
        let length = u64::from_be_bytes(
            header[2..10]
                .try_into()
                .map_err(|_| MovementProviderError::ContractViolation)?,
        ) as usize;
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
            .call(MovementProviderRequest::VerifyExternalDeposit {
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
        match self
            .call(MovementProviderRequest::SubmitDepositCustody(
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
            .call(MovementProviderRequest::PollDepositFinality(transaction))
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
        match self.call(MovementProviderRequest::ObtainDepositProof(transaction)) {
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
        match self
            .call(MovementProviderRequest::VerifyClaimSignature {
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
            .call(MovementProviderRequest::CheckpointProof(*debit))
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
        match self
            .call(MovementProviderRequest::SubmitWithdrawal(request.clone()))
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
            .call(MovementProviderRequest::LookupWithdrawal(key))
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
        match self
            .call(MovementProviderRequest::SubmitExit(request.clone()))
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
    if value.len() < 16
        || value.len() > 128
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
