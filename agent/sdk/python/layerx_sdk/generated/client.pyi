# Generated from the LayerX Agent API schema. Do not hand-edit.

from enum import IntEnum
from typing import Generic, Literal, Mapping, Protocol, TypeAlias, TypeVar

def layerx_sdk_py_package() -> Mapping[str, str | int]: ...

Amount: TypeAlias = int
def parse_amount(value: str) -> Amount: ...

BudgetLimit: TypeAlias = int
def parse_budget_limit(value: str) -> BudgetLimit: ...

Sequence: TypeAlias = int
def parse_sequence(value: str) -> Sequence: ...

TimestampSeconds: TypeAlias = int
def parse_timestamp_seconds(value: str) -> TimestampSeconds: ...

class VerificationLevel(IntEnum):
    UNVERIFIED = 0
    SEQUENCER_SIGNED = 1
    BATCH_INCLUDED = 2
    STATE_PROVEN = 3
    CHECKPOINT_FINALISED = 4
    SETTLEMENT_ANCHORED = 5

ErrorClass: TypeAlias = Literal["TransportFailure", "Deadline", "ProtocolIncompatibility", "UnavailableCapability", "CoreRejection", "VerificationFailure", "PolicyRefusal", "CapabilityRefusal", "BudgetRefusal", "RateLimit", "IdempotencyConflict", "InternalFault"]
Operation: TypeAlias = Literal["agent.register", "approval.approve", "approval.get", "approval.list", "approval.reject", "availability.fetch", "budget.create", "budget.fund", "budget.list", "budget.reconciliation", "budget.revoke", "capability.attenuate", "capability.create", "capability.list", "capability.revoke", "export.offline", "prepare", "program.activity", "program.call", "program.discover", "program.interface", "program.receipt", "program.simulate", "project", "read.account", "read.balance", "read.batch", "read.checkpoint", "read.history", "read.module_state", "read.proof_bundle", "session.close", "session.list", "session.open", "session.refresh", "sign", "submit", "subscription.acknowledge", "subscription.create", "subscription.delete", "subscription.health", "subscription.list", "subscription.pause", "subscription.resume", "track", "wait"]

ApprovalState: TypeAlias = Literal["Held", "Granted", "Rejected", "Expired", "Defective"]
ApprovalDecisionOutcome: TypeAlias = Literal["Granted", "Rejected", "Expired", "Defective", "AlreadyDecided", "Conflict"]
ApprovalEventKind: TypeAlias = Literal["Created", "Granted", "Rejected", "Expired", "Defective"]

APPROVAL_CONTRACT_INTRODUCED: Literal["1.1"]
APPROVAL_ENFORCEMENT_NOTICE: str
APPROVAL_STATES: tuple[ApprovalState, ...]
APPROVAL_DECISION_OUTCOMES: tuple[ApprovalDecisionOutcome, ...]
APPROVAL_EVENT_KINDS: tuple[ApprovalEventKind, ...]

class StructuredActivityDisclosure:
    canonical_digest: str
    activity_type: str
    actor: str
    authority: str
    counterparties: tuple[str, ...]
    amounts: tuple[Amount, ...]
    asset: str
    fee_limit: Amount
    expiry: TimestampSeconds
    idempotency_key: str
    def __init__(self, canonical_digest: str, activity_type: str, actor: str, authority: str, counterparties: tuple[str, ...], amounts: tuple[Amount, ...], asset: str, fee_limit: Amount, expiry: TimestampSeconds, idempotency_key: str) -> None: ...

class HoldReason:
    code: str
    message: str
    def __init__(self, code: str, message: str) -> None: ...

class ApprovalRecord:
    approval_id: str
    tenant: str
    held_activity: StructuredActivityDisclosure
    canonical_bytes_digest: str
    hold_reason: HoldReason
    created_at: TimestampSeconds
    expires_at: TimestampSeconds
    state: ApprovalState
    enforcement: Literal["daemon_enforced"]
    authority_notice: str

class ApprovalPage:
    approvals: tuple[ApprovalRecord, ...]
    next_cursor: str | None

class ApprovalListRequest:
    tenant: str
    cursor: str | None
    page_limit: int
    def __init__(self, tenant: str, cursor: str | None, page_limit: int) -> None: ...

class ApprovalGetRequest:
    tenant: str
    approval_id: str
    def __init__(self, tenant: str, approval_id: str) -> None: ...

class ApprovalApproveRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    def __init__(self, tenant: str, approval_id: str, idempotency_key: str) -> None: ...

class ApprovalRejectRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    reason: str
    def __init__(self, tenant: str, approval_id: str, idempotency_key: str, reason: str) -> None: ...

class ApprovalDecision:
    outcome: ApprovalDecisionOutcome
    submission_ref: str | None
    winning_outcome: ApprovalDecisionOutcome | None
    enforcement: Literal["daemon_enforced"]
    authority_notice: str

class ApprovalLifecycleEvent:
    event_id: str
    tenant: str
    approval_id: str
    kind: ApprovalEventKind
    at: TimestampSeconds
    record_digest: str

T = TypeVar("T")
R = TypeVar("R")

class VerifiedRead(Generic[R]):
    value: R
    achieved_verification_level: VerificationLevel
    chain_head: int
    latest_batch: str
    latest_checkpoint: str
    value_sequence: int
    def __init__(
        self,
        value: R,
        achieved_verification_level: VerificationLevel,
        chain_head: int,
        latest_batch: str,
        latest_checkpoint: str,
        value_sequence: int,
    ) -> None: ...

def require_verified(requested: VerificationLevel, read: VerifiedRead[R]) -> VerifiedRead[R]: ...

class SubmissionUnknown:
    kind: Literal["Unknown"]
    def __init__(self, kind: Literal["Unknown"] = "Unknown") -> None: ...

class SubmissionExecuted:
    receipt_ref: str
    kind: Literal["Executed"]
    def __init__(self, receipt_ref: str, kind: Literal["Executed"] = "Executed") -> None: ...

class SubmissionFailed:
    protocol_result_code: int
    kind: Literal["Failed"]
    def __init__(self, protocol_result_code: int, kind: Literal["Failed"] = "Failed") -> None: ...

class SubmissionPending:
    stage: str
    kind: Literal["Pending"]
    def __init__(self, stage: str, kind: Literal["Pending"] = "Pending") -> None: ...

SubmissionState: TypeAlias = SubmissionUnknown | SubmissionExecuted | SubmissionFailed | SubmissionPending

class IdempotentMutation(Generic[T]):
    request_id: int
    key: bytes
    body_digest: bytes
    operation: T
    def __init__(self, request_id: int, key: bytes, body_digest: bytes, operation: T) -> None: ...

class ApiError(Exception):
    error_class: ErrorClass
    protocol_result_code: int | None
    retriable: bool
    request_id: int
    reason: str
    def __init__(
        self,
        error_class: ErrorClass,
        protocol_result_code: int | None,
        retriable: bool,
        request_id: int,
        reason: str,
    ) -> None: ...

class Transport(Protocol):
    def call(self, operation: Operation, request: object) -> object: ...

class Client:
    def __init__(self, transport: Transport) -> None: ...
    def call(self, operation: Operation, request: object) -> object: ...
    def approval_list(self, request: ApprovalListRequest) -> ApprovalPage: ...
    def approval_get(self, request: ApprovalGetRequest) -> ApprovalRecord: ...
    def approval_approve(self, request: ApprovalApproveRequest) -> ApprovalDecision: ...
    def approval_reject(self, request: ApprovalRejectRequest) -> ApprovalDecision: ...
