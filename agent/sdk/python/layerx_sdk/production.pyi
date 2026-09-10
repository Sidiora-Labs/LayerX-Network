from collections.abc import Callable, Mapping
from enum import StrEnum
from typing import Literal, Protocol, Self, TypeAlias, TypeVar

from .generated.client import Operation

AGENT_OPERATIONS: tuple[Operation, ...]
HUMAN_OPERATIONS: tuple[str, ...]
HumanOperation: TypeAlias = Literal[
    "account.create", "activity.entry", "activity.export.evidence", "activity.export.statement",
    "activity.query", "agent.archive", "agent.create", "agent.get", "agent.limit", "agent.list",
    "agent.pause", "agent.reclaim", "agent.recover", "agent.resume", "agent.rotate",
    "approval.approve", "approval.get", "approval.list", "approval.reject", "binding.rebind",
    "binding.statement", "binding.status", "binding.submit", "deposit.confirm", "deposit.start",
    "evidence.get", "exit.eligibility", "exit.start", "journey.get", "journey.list", "move.commit",
    "move.quote", "notification.list", "notification.preferences.get", "notification.preferences.set",
    "notification.read", "onboarding.resume", "onboarding.status", "passkey.assert.begin",
    "passkey.assert.finish", "passkey.register.begin", "passkey.register.finish", "profile.get",
    "profile.update", "session.list", "session.open", "session.refresh", "session.revoke",
    "session.revoke-all", "stepup.begin", "stepup.finish", "stream.next", "stream.open", "version",
    "withdraw.claim", "withdraw.start",
]
PlatformPlane: TypeAlias = Literal["agent", "human"]
RetryClass: TypeAlias = Literal["never", "safe", "after", "unknown-outcome"]

class SdkErrorCode(StrEnum):
    INVALID_ARGUMENT: SdkErrorCode
    IDEMPOTENCY_REQUIRED: SdkErrorCode
    TRANSPORT_FAILURE: SdkErrorCode
    DEADLINE: SdkErrorCode
    PROTOCOL_INCOMPATIBILITY: SdkErrorCode
    UNAVAILABLE_CAPABILITY: SdkErrorCode
    CORE_REJECTION: SdkErrorCode
    VERIFICATION_FAILURE: SdkErrorCode
    POLICY_REFUSAL: SdkErrorCode
    CAPABILITY_REFUSAL: SdkErrorCode
    BUDGET_REFUSAL: SdkErrorCode
    RATE_LIMIT: SdkErrorCode
    IDEMPOTENCY_CONFLICT: SdkErrorCode
    DECODE_FAILURE: SdkErrorCode
    UNKNOWN_OUTCOME: SdkErrorCode
    INTERNAL_FAULT: SdkErrorCode

class PlatformSdkError(Exception):
    code: SdkErrorCode
    retry: RetryClass
    request_id: str | None
    protocol_result_code: int | None
    retry_after_ms: int | None
    def __init__(self, code: SdkErrorCode, retry: RetryClass, *, request_id: str | None = ..., protocol_result_code: int | None = ..., retry_after_ms: int | None = ...) -> None: ...
    def to_dict(self) -> dict[str, str | int]: ...

class IdempotencyKey(str):
    def __new__(cls, value: str) -> Self: ...

class ProtocolAmount(int):
    def __new__(cls, value: int | str) -> Self: ...

_T = TypeVar("_T")
class SecretBytes:
    def __init__(self, value: bytes | bytearray) -> None: ...
    def use(self, consumer: Callable[[memoryview], _T]) -> _T: ...
    def destroy(self) -> None: ...

class ProductionTransport(Protocol):
    def call(self, plane: PlatformPlane, operation: Operation | HumanOperation, request: object, idempotency_key: IdempotencyKey | None) -> object: ...

class SdkTelemetry(Protocol): ...

_TRequest = TypeVar("_TRequest")
_TResponse = TypeVar("_TResponse")
class ProductionClient:
    def __init__(self, transport: ProductionTransport, telemetry: SdkTelemetry | None = ...) -> None: ...
    def agent(self, operation: Operation, request: _TRequest, *, idempotency_key: IdempotencyKey | None = ...) -> _TResponse: ...
    def human(self, operation: HumanOperation, request: _TRequest, *, idempotency_key: IdempotencyKey | None = ...) -> _TResponse: ...

def platform_sdk_python() -> Mapping[str, str | int]: ...
