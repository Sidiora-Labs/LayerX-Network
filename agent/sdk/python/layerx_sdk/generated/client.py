# Generated from the LayerX Agent API schema. Do not hand-edit.

from dataclasses import dataclass
from enum import IntEnum
from types import MappingProxyType
from typing import Generic, Literal, Mapping, Protocol, TypeAlias, TypeVar, cast

_PACKAGE_METADATA: Mapping[str, str | int] = MappingProxyType({
    "name": "layerx-sdk",
    "version": "0.1.0",
    "contract_major": 1,
})

def layerx_sdk_py_package() -> Mapping[str, str | int]:
    return _PACKAGE_METADATA

Amount = int

def parse_amount(value: str) -> Amount:
    if not value or (value != "0" and value.startswith("0")) or not value.isascii() or not value.isdigit():
        raise ValueError("invalid Amount")
    parsed = int(value)
    if parsed > 340282366920938463463374607431768211455:
        raise OverflowError("Amount out of range")
    return parsed

BudgetLimit = int

def parse_budget_limit(value: str) -> BudgetLimit:
    if not value or (value != "0" and value.startswith("0")) or not value.isascii() or not value.isdigit():
        raise ValueError("invalid BudgetLimit")
    parsed = int(value)
    if parsed > 340282366920938463463374607431768211455:
        raise OverflowError("BudgetLimit out of range")
    return parsed

Sequence = int

def parse_sequence(value: str) -> Sequence:
    if not value or (value != "0" and value.startswith("0")) or not value.isascii() or not value.isdigit():
        raise ValueError("invalid Sequence")
    parsed = int(value)
    if parsed > 18446744073709551615:
        raise OverflowError("Sequence out of range")
    return parsed

TimestampSeconds = int

def parse_timestamp_seconds(value: str) -> TimestampSeconds:
    if not value or (value != "0" and value.startswith("0")) or not value.isascii() or not value.isdigit():
        raise ValueError("invalid TimestampSeconds")
    parsed = int(value)
    if parsed > 18446744073709551615:
        raise OverflowError("TimestampSeconds out of range")
    return parsed

class VerificationLevel(IntEnum):
    UNVERIFIED = 0
    SEQUENCER_SIGNED = 1
    BATCH_INCLUDED = 2
    STATE_PROVEN = 3
    CHECKPOINT_FINALISED = 4
    SETTLEMENT_ANCHORED = 5

ErrorClass = Literal["TransportFailure", "Deadline", "ProtocolIncompatibility", "UnavailableCapability", "CoreRejection", "VerificationFailure", "PolicyRefusal", "CapabilityRefusal", "BudgetRefusal", "RateLimit", "IdempotencyConflict", "InternalFault"]
Operation = Literal["agent.register", "approval.approve", "approval.get", "approval.list", "approval.reject", "availability.fetch", "budget.create", "budget.fund", "budget.list", "budget.reconciliation", "budget.revoke", "capability.attenuate", "capability.create", "capability.list", "capability.revoke", "export.offline", "prepare", "program.activity", "program.call", "program.discover", "program.interface", "program.receipt", "program.simulate", "project", "read.account", "read.balance", "read.batch", "read.checkpoint", "read.history", "read.module_state", "read.proof_bundle", "session.close", "session.list", "session.open", "session.refresh", "sign", "submit", "subscription.acknowledge", "subscription.create", "subscription.delete", "subscription.health", "subscription.list", "subscription.pause", "subscription.resume", "track", "wait"]

APPROVAL_CONTRACT_INTRODUCED = "1.1"
APPROVAL_ENFORCEMENT_NOTICE = "An approval hold is a daemon-enforced restriction. It confers no protocol authority, and bypassing the daemon bypasses the restriction."
APPROVAL_STATES = ("Held", "Granted", "Rejected", "Expired", "Defective",)
APPROVAL_DECISION_OUTCOMES = ("Granted", "Rejected", "Expired", "Defective", "AlreadyDecided", "Conflict",)
APPROVAL_EVENT_KINDS = ("Created", "Granted", "Rejected", "Expired", "Defective",)

ApprovalState = Literal["Held", "Granted", "Rejected", "Expired", "Defective"]
ApprovalDecisionOutcome = Literal["Granted", "Rejected", "Expired", "Defective", "AlreadyDecided", "Conflict"]
ApprovalEventKind = Literal["Created", "Granted", "Rejected", "Expired", "Defective"]

@dataclass(frozen=True)
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

@dataclass(frozen=True)
class HoldReason:
    code: str
    message: str

@dataclass(frozen=True)
class ApprovalRecord:
    approval_id: str
    tenant: str
    held_activity: StructuredActivityDisclosure
    canonical_bytes_digest: str
    hold_reason: HoldReason
    created_at: TimestampSeconds
    expires_at: TimestampSeconds
    state: ApprovalState
    enforcement: Literal["daemon_enforced"] = "daemon_enforced"
    authority_notice: str = APPROVAL_ENFORCEMENT_NOTICE

@dataclass(frozen=True)
class ApprovalPage:
    approvals: tuple[ApprovalRecord, ...]
    next_cursor: str | None

@dataclass(frozen=True)
class ApprovalListRequest:
    tenant: str
    cursor: str | None
    page_limit: int

@dataclass(frozen=True)
class ApprovalGetRequest:
    tenant: str
    approval_id: str

@dataclass(frozen=True)
class ApprovalApproveRequest:
    tenant: str
    approval_id: str
    idempotency_key: str

@dataclass(frozen=True)
class ApprovalRejectRequest:
    tenant: str
    approval_id: str
    idempotency_key: str
    reason: str

@dataclass(frozen=True)
class ApprovalDecision:
    outcome: ApprovalDecisionOutcome
    submission_ref: str | None
    winning_outcome: ApprovalDecisionOutcome | None
    enforcement: Literal["daemon_enforced"] = "daemon_enforced"
    authority_notice: str = APPROVAL_ENFORCEMENT_NOTICE

@dataclass(frozen=True)
class ApprovalLifecycleEvent:
    event_id: str
    tenant: str
    approval_id: str
    kind: ApprovalEventKind
    at: TimestampSeconds
    record_digest: str
    hold_reason: HoldReason | None = None
    expires_at: TimestampSeconds | None = None
    submission_ref: str | None = None
    reason: str | None = None
    deterministic_expiry: bool | None = None
    defect_code: str | None = None

T = TypeVar("T")
R = TypeVar("R")

@dataclass(frozen=True)
class VerifiedRead(Generic[R]):
    value: R
    achieved_verification_level: VerificationLevel
    chain_head: int
    latest_batch: str
    latest_checkpoint: str
    value_sequence: int

def require_verified(requested: VerificationLevel, read: VerifiedRead[R]) -> VerifiedRead[R]:
    if read.achieved_verification_level == VerificationLevel.UNVERIFIED:
        raise ValueError("unverified_read")
    if read.achieved_verification_level < requested:
        raise ValueError(
            f"verification_below_requested:{requested.value}:{read.achieved_verification_level.value}"
        )
    return read

@dataclass(frozen=True)
class SubmissionUnknown:
    kind: Literal["Unknown"] = "Unknown"

@dataclass(frozen=True)
class SubmissionExecuted:
    receipt_ref: str
    kind: Literal["Executed"] = "Executed"

@dataclass(frozen=True)
class SubmissionFailed:
    protocol_result_code: int
    kind: Literal["Failed"] = "Failed"

@dataclass(frozen=True)
class SubmissionPending:
    stage: str
    kind: Literal["Pending"] = "Pending"

SubmissionState: TypeAlias = (
    SubmissionUnknown | SubmissionExecuted | SubmissionFailed | SubmissionPending
)

@dataclass(frozen=True)
class IdempotentMutation(Generic[T]):
    request_id: int
    key: bytes
    body_digest: bytes
    operation: T

@dataclass(frozen=True)
class ApiError(Exception):
    error_class: ErrorClass
    protocol_result_code: int | None
    retriable: bool
    request_id: int
    reason: str

    def __str__(self) -> str:
        return f"{self.error_class}:{self.reason}"

class Transport(Protocol):
    def call(self, operation: Operation, request: object) -> object: ...

class Client:
    def __init__(self, transport: Transport) -> None:
        self._transport = transport

    def call(self, operation: Operation, request: object) -> object:
        return self._transport.call(operation, request)

    def approval_list(self, request: ApprovalListRequest) -> ApprovalPage:
        return cast(ApprovalPage, self.call("approval.list", request))

    def approval_get(self, request: ApprovalGetRequest) -> ApprovalRecord:
        return cast(ApprovalRecord, self.call("approval.get", request))

    def approval_approve(self, request: ApprovalApproveRequest) -> ApprovalDecision:
        return cast(ApprovalDecision, self.call("approval.approve", request))

    def approval_reject(self, request: ApprovalRejectRequest) -> ApprovalDecision:
        return cast(ApprovalDecision, self.call("approval.reject", request))
