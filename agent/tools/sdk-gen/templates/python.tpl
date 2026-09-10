# Generated from the LayerX Agent API schema. Do not hand-edit.

from collections.abc import Mapping
from dataclasses import dataclass
from enum import IntEnum
from types import MappingProxyType
from typing import Generic, Literal, Protocol, TypeAlias, TypeVar, cast

_PACKAGE_METADATA: Mapping[str, str | int] = MappingProxyType({
    "name": "layerx-sdk",
    "version": "0.1.0",
    "contract_major": 1,
})

def layerx_sdk_py_package() -> Mapping[str, str | int]:
    return _PACKAGE_METADATA

{{SCALARS}}

class VerificationLevel(IntEnum):
{{LEVELS}}

ErrorClass = Literal[{{ERRORS}}]
Operation = Literal[{{OPERATIONS}}]

{{APPROVAL}}

_T = TypeVar("_T")
_R = TypeVar("_R")

@dataclass(frozen=True)
class VerifiedRead(Generic[_R]):
    value: _R
    achieved_verification_level: VerificationLevel
    chain_head: int
    latest_batch: str
    latest_checkpoint: str
    value_sequence: int

def require_verified(requested: VerificationLevel, read: VerifiedRead[_R]) -> VerifiedRead[_R]:
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
class IdempotentMutation(Generic[_T]):
    request_id: int
    key: bytes
    body_digest: bytes
    operation: _T

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
