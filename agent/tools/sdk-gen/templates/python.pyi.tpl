# Generated from the LayerX Agent API schema. Do not hand-edit.

from collections.abc import Mapping
from enum import IntEnum
from typing import Generic, Literal, Protocol, TypeAlias, TypeVar

def layerx_sdk_py_package() -> Mapping[str, str | int]: ...

{{SCALARS}}

class VerificationLevel(IntEnum):
{{LEVELS}}

ErrorClass: TypeAlias = Literal[{{ERRORS}}]
Operation: TypeAlias = Literal[{{OPERATIONS}}]

{{APPROVAL}}

_T = TypeVar("_T")
_R = TypeVar("_R")

class VerifiedRead(Generic[_R]):
    value: _R
    achieved_verification_level: VerificationLevel
    chain_head: int
    latest_batch: str
    latest_checkpoint: str
    value_sequence: int
    def __init__(
        self,
        value: _R,
        achieved_verification_level: VerificationLevel,
        chain_head: int,
        latest_batch: str,
        latest_checkpoint: str,
        value_sequence: int,
    ) -> None: ...

def require_verified(requested: VerificationLevel, read: VerifiedRead[_R]) -> VerifiedRead[_R]: ...

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

class IdempotentMutation(Generic[_T]):
    request_id: int
    key: bytes
    body_digest: bytes
    operation: _T
    def __init__(self, request_id: int, key: bytes, body_digest: bytes, operation: _T) -> None: ...

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
