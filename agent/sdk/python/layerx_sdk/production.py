from __future__ import annotations

from collections.abc import Callable
from enum import StrEnum
from types import MappingProxyType
from typing import Literal, Mapping, Protocol, TypeVar, cast

from .generated.client import Operation

AGENT_OPERATIONS: tuple[Operation, ...] = (
    "agent.register", "approval.approve", "approval.get", "approval.list", "approval.reject",
    "availability.fetch", "budget.create", "budget.fund", "budget.list", "budget.reconciliation",
    "budget.revoke", "capability.attenuate", "capability.create", "capability.list",
    "capability.revoke", "export.offline", "prepare", "program.activity", "program.call",
    "program.discover", "program.interface", "program.receipt", "program.simulate", "project", "read.account", "read.balance",
    "read.batch", "read.checkpoint", "read.history", "read.module_state", "read.proof_bundle",
    "session.close", "session.list", "session.open", "session.refresh", "sign", "submit",
    "subscription.acknowledge", "subscription.create", "subscription.delete", "subscription.health",
    "subscription.list", "subscription.pause", "subscription.resume", "track", "wait",
)

HUMAN_OPERATIONS = (
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
)

HumanOperation = Literal[
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
PlatformPlane = Literal["agent", "human"]
RetryClass = Literal["never", "safe", "after", "unknown-outcome"]


class SdkErrorCode(StrEnum):
    INVALID_ARGUMENT = "invalid-argument"
    IDEMPOTENCY_REQUIRED = "idempotency-required"
    TRANSPORT_FAILURE = "transport-failure"
    DEADLINE = "deadline"
    PROTOCOL_INCOMPATIBILITY = "protocol-incompatibility"
    UNAVAILABLE_CAPABILITY = "unavailable-capability"
    CORE_REJECTION = "core-rejection"
    VERIFICATION_FAILURE = "verification-failure"
    POLICY_REFUSAL = "policy-refusal"
    CAPABILITY_REFUSAL = "capability-refusal"
    BUDGET_REFUSAL = "budget-refusal"
    RATE_LIMIT = "rate-limit"
    IDEMPOTENCY_CONFLICT = "idempotency-conflict"
    DECODE_FAILURE = "decode-failure"
    UNKNOWN_OUTCOME = "unknown-outcome"
    INTERNAL_FAULT = "internal-fault"


_SAFE_MESSAGES: Mapping[SdkErrorCode, str] = MappingProxyType({
    SdkErrorCode.INVALID_ARGUMENT: "The SDK rejected an invalid argument.",
    SdkErrorCode.IDEMPOTENCY_REQUIRED: "This operation requires an idempotency key.",
    SdkErrorCode.TRANSPORT_FAILURE: "The request could not reach the service.",
    SdkErrorCode.DEADLINE: "The request deadline elapsed.",
    SdkErrorCode.PROTOCOL_INCOMPATIBILITY: "The service protocol is not compatible with this SDK.",
    SdkErrorCode.UNAVAILABLE_CAPABILITY: "The requested operation is unavailable.",
    SdkErrorCode.CORE_REJECTION: "The protocol refused the request.",
    SdkErrorCode.VERIFICATION_FAILURE: "Local verification failed.",
    SdkErrorCode.POLICY_REFUSAL: "Policy refused the request.",
    SdkErrorCode.CAPABILITY_REFUSAL: "The caller does not have the required authority.",
    SdkErrorCode.BUDGET_REFUSAL: "The configured budget refused the request.",
    SdkErrorCode.RATE_LIMIT: "The request rate limit was reached.",
    SdkErrorCode.IDEMPOTENCY_CONFLICT: "The idempotency key belongs to a different request.",
    SdkErrorCode.DECODE_FAILURE: "The service response did not match the contract.",
    SdkErrorCode.UNKNOWN_OUTCOME: "The request outcome is unknown and must be resolved before retrying.",
    SdkErrorCode.INTERNAL_FAULT: "The service could not complete the request.",
})


class PlatformSdkError(Exception):
    __slots__ = ("code", "retry", "request_id", "protocol_result_code", "retry_after_ms")

    def __init__(
        self,
        code: SdkErrorCode,
        retry: RetryClass,
        *,
        request_id: str | None = None,
        protocol_result_code: int | None = None,
        retry_after_ms: int | None = None,
    ) -> None:
        super().__init__(_SAFE_MESSAGES[code])
        self.code = code
        self.retry = retry
        self.request_id = request_id
        self.protocol_result_code = protocol_result_code
        self.retry_after_ms = retry_after_ms

    def to_dict(self) -> dict[str, str | int]:
        result: dict[str, str | int] = {"code": self.code.value, "retry": self.retry}
        if self.request_id is not None:
            result["request_id"] = self.request_id
        if self.protocol_result_code is not None:
            result["protocol_result_code"] = self.protocol_result_code
        if self.retry_after_ms is not None:
            result["retry_after_ms"] = self.retry_after_ms
        return result


class IdempotencyKey(str):
    def __new__(cls, value: str) -> IdempotencyKey:
        if not value or len(value) > 255 or "\0" in value:
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        return cast(IdempotencyKey, str.__new__(cls, value))


class ProtocolAmount(int):
    def __new__(cls, value: int | str) -> ProtocolAmount:
        if isinstance(value, bool):
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        if isinstance(value, str):
            if not value.isascii() or not value.isdigit() or (value != "0" and value.startswith("0")):
                raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
            parsed = int(value)
        else:
            parsed = value
        if parsed < 0 or parsed > 340282366920938463463374607431768211455:
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        return cast(ProtocolAmount, int.__new__(cls, parsed))


class SecretBytes:
    __slots__ = ("_value", "_destroyed")

    def __init__(self, value: bytes | bytearray) -> None:
        if not value:
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        self._value = bytearray(value)
        self._destroyed = False

    def use(self, consumer: Callable[[memoryview], T]) -> T:
        if self._destroyed:
            raise PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")
        return consumer(memoryview(self._value))

    def destroy(self) -> None:
        for index in range(len(self._value)):
            self._value[index] = 0
        self._destroyed = True

    def __repr__(self) -> str:
        return "SecretBytes([REDACTED])"

    def __str__(self) -> str:
        return "[REDACTED]"

    def __reduce__(self) -> object:
        raise TypeError("SecretBytes cannot be serialised")

    def __del__(self) -> None:
        value = getattr(self, "_value", None)
        if value is not None:
            self.destroy()


TRequest = TypeVar("TRequest")
TResponse = TypeVar("TResponse")
T = TypeVar("T")


class ProductionTransport(Protocol):
    def call(
        self,
        plane: PlatformPlane,
        operation: Operation | HumanOperation,
        request: object,
        idempotency_key: IdempotencyKey | None,
    ) -> object: ...


class SdkTelemetry(Protocol):
    def __call__(
        self,
        plane: PlatformPlane,
        operation: Operation | HumanOperation,
        outcome: Literal["completed", "refused"],
        code: SdkErrorCode | None,
    ) -> None: ...


_AGENT_IDEMPOTENT = frozenset({
    "agent.register", "approval.approve", "approval.reject", "budget.create", "budget.fund",
    "budget.revoke", "capability.attenuate", "capability.create", "capability.revoke", "prepare", "program.call",
    "session.close", "session.open", "session.refresh", "sign", "submit",
    "subscription.acknowledge", "subscription.create", "subscription.delete", "subscription.pause",
    "subscription.resume",
})
_HUMAN_IDEMPOTENT = frozenset({
    "account.create", "activity.export.evidence", "activity.export.statement", "agent.archive",
    "agent.create", "agent.limit", "agent.pause", "agent.reclaim", "agent.recover", "agent.resume",
    "agent.rotate", "approval.approve", "approval.reject", "binding.rebind", "binding.submit",
    "deposit.start", "exit.start", "move.commit", "withdraw.start",
})


class ProductionClient:
    def __init__(self, transport: ProductionTransport, telemetry: SdkTelemetry | None = None) -> None:
        self._transport = transport
        self._telemetry = telemetry

    def agent(
        self,
        operation: Operation,
        request: TRequest,
        *,
        idempotency_key: IdempotencyKey | None = None,
    ) -> TResponse:
        return self._execute("agent", operation, request, idempotency_key)

    def human(
        self,
        operation: HumanOperation,
        request: TRequest,
        *,
        idempotency_key: IdempotencyKey | None = None,
    ) -> TResponse:
        return self._execute("human", operation, request, idempotency_key)

    def _execute(
        self,
        plane: PlatformPlane,
        operation: Operation | HumanOperation,
        request: TRequest,
        idempotency_key: IdempotencyKey | None,
    ) -> TResponse:
        required = operation in (_AGENT_IDEMPOTENT if plane == "agent" else _HUMAN_IDEMPOTENT)
        if required and idempotency_key is None:
            raise PlatformSdkError(SdkErrorCode.IDEMPOTENCY_REQUIRED, "never")
        try:
            response = self._transport.call(plane, operation, request, idempotency_key)
        except PlatformSdkError as error:
            if self._telemetry is not None:
                self._telemetry(plane, operation, "refused", error.code)
            raise
        except Exception:
            error = PlatformSdkError(SdkErrorCode.TRANSPORT_FAILURE, "safe")
            if self._telemetry is not None:
                self._telemetry(plane, operation, "refused", error.code)
            raise error from None
        if self._telemetry is not None:
            self._telemetry(plane, operation, "completed", None)
        return cast(TResponse, response)


_PACKAGE_METADATA = MappingProxyType({
    "name": "layerx-sdk",
    "version": "0.1.0",
    "agent_operations": len(AGENT_OPERATIONS),
    "human_operations": len(HUMAN_OPERATIONS),
})


def platform_sdk_python() -> Mapping[str, str | int]:
    return _PACKAGE_METADATA
