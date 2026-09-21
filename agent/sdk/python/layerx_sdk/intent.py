"""The two human-plane intent operations: planning and plan submission."""

from __future__ import annotations

import json
import re
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from http.client import HTTPException
from typing import Literal, cast
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse, urlunparse
from urllib.request import HTTPRedirectHandler, Request, build_opener

from .agent_http import LayerXKeyCredential
from .production import (
    IdempotencyKey,
    PlatformSdkError,
    ProtocolAmount,
    RetryClass,
    SdkErrorCode,
)

_MAX_RESPONSE_BYTES = 8 * 1024 * 1024
_MAX_REQUEST_BYTES = 1024 * 1024
_MAX_LEGS = 16
_MAX_U64 = (1 << 64) - 1
_HEX32 = re.compile(r"^[0-9a-f]{64}$")
_HEX = re.compile(r"^(?:[0-9a-f]{2})+$")
_ACCOUNT_NAME = re.compile(r"^[a-z0-9][a-z0-9:._-]{0,254}$")
_ACTOR = re.compile(r"^did:[a-z0-9]{1,32}:[a-z0-9:._-]{1,214}$")
_AUTHORITY = re.compile(r"^[a-z][a-z0-9-]{0,63}$")
_CURRENCY = re.compile(r"^[A-Z][A-Z0-9]{1,15}$")
_DEADLINE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$")
_COPY_KEY = re.compile(r"^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$")
_TRACE_ID = re.compile(r"^trc_[0-9a-z]{1,64}$")
_JOURNEY_ID = re.compile(r"^jrn_[0-9a-z]{1,64}$")
_LOWER_TOKEN = re.compile(r"^[a-z][a-z0-9-]{0,63}$")
_HEADER_VALUE = re.compile(r"^[\x21-\x7e]{1,255}$")
_DECIMAL = re.compile(r"^(?:0|[1-9][0-9]*)$")

IntentEndpointKind = Literal["paxeer-wallet", "human", "agent", "agent-budget"]
IntentDomain = Literal["paxeer", "layerx"]
IntentOperation = Literal["intent.plan", "intent.submit"]
HumanErrorCode = Literal[
    "unauthenticated", "session-expired", "step-up-required", "forbidden", "not-found",
    "invalid-request", "conflict", "rate-limited", "cursor-expired", "unavailable",
    "upstream-degraded", "challenge-expired", "refused-by-policy", "refused-by-budget",
    "refused-by-capability", "refused-by-protocol", "refused-by-limit", "quote-expired",
    "wallet-not-bound", "exit-unavailable", "already-decided", "hold-expired", "hold-defective",
    "archive-needs-disposition", "confirmation-mismatch", "not-suppressible", "support-unavailable",
    "support-conversation-unknown", "support-message-unknown",
]
HumanRetriability = Literal["retriable", "retriable-after", "structural", "final"]

INTENT_ENDPOINT_KINDS: tuple[IntentEndpointKind, ...] = (
    "paxeer-wallet", "human", "agent", "agent-budget",
)
INTENT_DOMAINS: tuple[IntentDomain, ...] = ("paxeer", "layerx")
HUMAN_RETRIABILITIES: tuple[HumanRetriability, ...] = (
    "retriable", "retriable-after", "structural", "final",
)
HUMAN_ERROR_CODES: tuple[HumanErrorCode, ...] = (
    "unauthenticated", "session-expired", "step-up-required", "forbidden", "not-found",
    "invalid-request", "conflict", "rate-limited", "cursor-expired", "unavailable",
    "upstream-degraded", "challenge-expired", "refused-by-policy", "refused-by-budget",
    "refused-by-capability", "refused-by-protocol", "refused-by-limit", "quote-expired",
    "wallet-not-bound", "exit-unavailable", "already-decided", "hold-expired", "hold-defective",
    "archive-needs-disposition", "confirmation-mismatch", "not-suppressible", "support-unavailable",
    "support-conversation-unknown", "support-message-unknown",
)

_HUMAN_ERROR_CLASS: Mapping[str, SdkErrorCode] = {
    "unauthenticated": SdkErrorCode.CAPABILITY_REFUSAL,
    "session-expired": SdkErrorCode.CAPABILITY_REFUSAL,
    "step-up-required": SdkErrorCode.CAPABILITY_REFUSAL,
    "forbidden": SdkErrorCode.CAPABILITY_REFUSAL,
    "not-found": SdkErrorCode.CORE_REJECTION,
    "invalid-request": SdkErrorCode.INVALID_ARGUMENT,
    "conflict": SdkErrorCode.IDEMPOTENCY_CONFLICT,
    "rate-limited": SdkErrorCode.RATE_LIMIT,
    "cursor-expired": SdkErrorCode.INVALID_ARGUMENT,
    "unavailable": SdkErrorCode.UNAVAILABLE_CAPABILITY,
    "upstream-degraded": SdkErrorCode.UNAVAILABLE_CAPABILITY,
    "challenge-expired": SdkErrorCode.DEADLINE,
    "refused-by-policy": SdkErrorCode.POLICY_REFUSAL,
    "refused-by-budget": SdkErrorCode.BUDGET_REFUSAL,
    "refused-by-capability": SdkErrorCode.CAPABILITY_REFUSAL,
    "refused-by-protocol": SdkErrorCode.CORE_REJECTION,
    "refused-by-limit": SdkErrorCode.POLICY_REFUSAL,
    "quote-expired": SdkErrorCode.DEADLINE,
    "wallet-not-bound": SdkErrorCode.CORE_REJECTION,
    "exit-unavailable": SdkErrorCode.UNAVAILABLE_CAPABILITY,
    "already-decided": SdkErrorCode.CORE_REJECTION,
    "hold-expired": SdkErrorCode.DEADLINE,
    "hold-defective": SdkErrorCode.CORE_REJECTION,
    "archive-needs-disposition": SdkErrorCode.CORE_REJECTION,
    "confirmation-mismatch": SdkErrorCode.CORE_REJECTION,
    "not-suppressible": SdkErrorCode.POLICY_REFUSAL,
    "support-unavailable": SdkErrorCode.UNAVAILABLE_CAPABILITY,
    "support-conversation-unknown": SdkErrorCode.CORE_REJECTION,
    "support-message-unknown": SdkErrorCode.CORE_REJECTION,
}

_HUMAN_RETRY: Mapping[str, RetryClass] = {
    "retriable": "safe",
    "retriable-after": "after",
    "structural": "never",
    "final": "never",
}


@dataclass(frozen=True)
class _Route:
    path: str
    idempotent: bool


_ROUTES: Mapping[str, _Route] = {
    "intent.plan": _Route("/v1/intents/plan", False),
    "intent.submit": _Route("/v1/intents/submit", True),
}


@dataclass(frozen=True)
class IntentMoney:
    amount: int
    currency: str


@dataclass(frozen=True)
class IntentEndpointRef:
    kind: IntentEndpointKind
    account: str | None


@dataclass(frozen=True)
class IntentConstraints:
    deadline: str
    max_fee: IntentMoney
    allow_top_up: bool


@dataclass(frozen=True)
class PlanIntentRequest:
    source: IntentEndpointRef
    destination: IntentEndpointRef
    asset_id: str
    money: IntentMoney
    constraints: IntentConstraints


@dataclass(frozen=True)
class IntentLeg:
    index: int
    mechanism: str
    domain: IntentDomain
    source: IntentEndpointRef
    destination: IntentEndpointRef
    money: IntentMoney
    fee: IntentMoney


@dataclass(frozen=True)
class IntentSigningRequirement:
    leg_index: int
    action_key: str
    signing_context: str
    authority: str


@dataclass(frozen=True)
class IntentPlan:
    plan_digest: str
    journey_kind: str
    total_fee: IntentMoney
    legs: tuple[IntentLeg, ...]
    signing_requirements: tuple[IntentSigningRequirement, ...]


@dataclass(frozen=True)
class IntentLegBinding:
    leg_index: int
    action_key: str
    actor: str
    authority: str
    relationship: str
    account_sequence: int
    not_before: int
    not_after: int
    fee_limit: IntentMoney


@dataclass(frozen=True)
class SubmitPlanRequest:
    plan_digest: str
    signed_digest: str
    bindings: tuple[IntentLegBinding, ...]


@dataclass(frozen=True)
class IntentSubmission:
    journey_id: str
    plan_digest: str
    state: str
    state_copy_key: str


class HumanIntentError(PlatformSdkError):
    __slots__ = ("copy_key", "field", "human_code", "retriability", "status", "trace")

    def __init__(
        self,
        status: int,
        human_code: HumanErrorCode,
        copy_key: str,
        retriability: HumanRetriability,
        trace: str,
        field: str | None = None,
        retry_after_ms: int | None = None,
    ) -> None:
        super().__init__(
            _HUMAN_ERROR_CLASS[human_code],
            _HUMAN_RETRY[retriability],
            retry_after_ms=retry_after_ms,
        )
        self.status = status
        self.human_code = human_code
        self.copy_key = copy_key
        self.retriability = retriability
        self.trace = trace
        self.field = field


class HumanIntentClient:
    __slots__ = ("_credential", "_endpoint", "_maximum_response_bytes", "_opener", "_timeout")

    def __init__(
        self,
        endpoint: str,
        *,
        credential: LayerXKeyCredential | None = None,
        timeout: float = 30.0,
        maximum_response_bytes: int = _MAX_RESPONSE_BYTES,
    ) -> None:
        self._endpoint = _validated_endpoint(endpoint)
        if not isinstance(timeout, (int, float)) or isinstance(timeout, bool) or timeout <= 0:
            raise _invalid_argument()
        if not isinstance(maximum_response_bytes, int) or isinstance(maximum_response_bytes, bool) or not 0 < maximum_response_bytes <= _MAX_RESPONSE_BYTES:
            raise _invalid_argument()
        self._credential = credential
        self._timeout = float(timeout)
        self._maximum_response_bytes = maximum_response_bytes
        self._opener = build_opener(_NoRedirect())

    def plan_intent(self, request: PlanIntentRequest) -> IntentPlan:
        document = encode_plan_intent_request(request)
        return decode_intent_plan(self._dispatch("intent.plan", document, None))

    def submit_plan(self, request: SubmitPlanRequest, idempotency_key: IdempotencyKey) -> IntentSubmission:
        document = encode_submit_plan_request(request)
        return decode_intent_submission(self._dispatch("intent.submit", document, idempotency_key))

    def _dispatch(
        self,
        operation: IntentOperation,
        document: Mapping[str, object],
        idempotency_key: IdempotencyKey | None,
    ) -> object:
        route = _ROUTES[operation]
        if route.idempotent == (idempotency_key is None):
            raise _invalid_argument()
        if idempotency_key is not None and _HEADER_VALUE.fullmatch(str(idempotency_key)) is None:
            raise _invalid_argument()
        try:
            body = json.dumps(document, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("utf-8")
        except (TypeError, ValueError, OverflowError):
            raise _invalid_argument() from None
        if len(body) > _MAX_REQUEST_BYTES:
            raise _invalid_argument()
        headers = {
            "Accept": "application/json",
            "Content-Type": "application/json",
            "Content-Length": str(len(body)),
            "User-Agent": "layerx-python/0.1.0",
        }
        if idempotency_key is not None:
            headers["Idempotency-Key"] = str(idempotency_key)
        if self._credential is not None:
            headers["Authorization"] = self._credential.use()
        outbound = Request(
            _route_endpoint(self._endpoint, route.path), data=body, headers=headers, method="POST"
        )
        try:
            with self._opener.open(outbound, timeout=self._timeout) as response:
                encoded = _bounded_read(response, self._maximum_response_bytes)
                if response.headers.get("Content-Type") != "application/json":
                    raise _decode_failure()
                return decode_human_envelope(response.status, encoded)
        except HTTPError as error:
            try:
                encoded = _bounded_read(error, self._maximum_response_bytes)
                if error.headers.get("Content-Type") != "application/json":
                    raise _decode_failure()
                return decode_human_envelope(error.code, encoded)
            finally:
                error.close()
        except PlatformSdkError:
            raise
        except (TimeoutError, URLError, OSError, HTTPException):
            raise _transport_failure(route.idempotent) from None


def encode_plan_intent_request(request: PlanIntentRequest) -> dict[str, object]:
    if not isinstance(request, PlanIntentRequest):
        raise _invalid_argument()
    return {
        "source": _encode_endpoint(request.source),
        "destination": _encode_endpoint(request.destination),
        "asset_id": _hex32(request.asset_id),
        "money": _encode_money(request.money),
        "constraints": {
            "deadline": _text(request.constraints.deadline, _DEADLINE),
            "max_fee": _encode_money(request.constraints.max_fee),
            "allow_top_up": _boolean(request.constraints.allow_top_up),
        },
    }


def encode_submit_plan_request(request: SubmitPlanRequest) -> dict[str, object]:
    if not isinstance(request, SubmitPlanRequest) or not isinstance(request.bindings, Sequence):
        raise _invalid_argument()
    if not 0 < len(request.bindings) <= _MAX_LEGS:
        raise _invalid_argument()
    seen: set[int] = set()
    bindings: list[dict[str, object]] = []
    for binding in request.bindings:
        if not isinstance(binding, IntentLegBinding):
            raise _invalid_argument()
        leg_index = _leg_position(binding.leg_index)
        if leg_index in seen:
            raise _invalid_argument()
        seen.add(leg_index)
        bindings.append({
            "leg_index": leg_index,
            "action_key": _hex32(binding.action_key),
            "actor": _text(binding.actor, _ACTOR),
            "authority": _text(binding.authority, _AUTHORITY),
            "relationship": _text(binding.relationship, _AUTHORITY),
            "account_sequence": _unsigned64(binding.account_sequence),
            "not_before": _unsigned64(binding.not_before),
            "not_after": _unsigned64(binding.not_after),
            "fee_limit": _encode_money(binding.fee_limit),
        })
    return {
        "plan_digest": _hex32(request.plan_digest),
        "signed_digest": _hex32(request.signed_digest),
        "bindings": bindings,
    }


def decode_human_envelope(status: int, encoded: bytes) -> object:
    try:
        envelope = json.loads(encoded.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise _decode_failure() from None
    if not isinstance(envelope, dict) or any(not isinstance(key, str) for key in envelope):
        raise _decode_failure()
    trace = envelope.get("trace")
    if not isinstance(trace, str) or _TRACE_ID.fullmatch(trace) is None:
        raise _decode_failure()
    if envelope.get("ok") is True:
        _exact(envelope, ("ok", "result", "trace"))
        if not 200 <= status < 300:
            raise _decode_failure()
        return envelope["result"]
    if envelope.get("ok") is not False:
        raise _decode_failure()
    _exact(envelope, ("ok", "error", "trace"))
    if not 400 <= status < 600:
        raise _decode_failure()
    raise decode_human_error(status, envelope["error"], trace)


def decode_human_error(status: int, value: object, trace: str) -> HumanIntentError:
    if not isinstance(value, dict) or not set(value) <= {"code", "copy_key", "retry", "retry_after_ms", "field"}:
        raise _decode_failure()
    code = value.get("code")
    copy_key = value.get("copy_key")
    retry = value.get("retry")
    if code not in HUMAN_ERROR_CODES or retry not in HUMAN_RETRIABILITIES:
        raise _decode_failure()
    if not isinstance(copy_key, str) or len(copy_key) > 128 or _COPY_KEY.fullmatch(copy_key) is None:
        raise _decode_failure()
    retry_after_ms = value.get("retry_after_ms")
    if retry_after_ms is not None and (type(retry_after_ms) is not int or retry_after_ms <= 0):
        raise _decode_failure()
    field = value.get("field")
    if field is not None and (not isinstance(field, str) or not 0 < len(field) <= 128):
        raise _decode_failure()
    if (retry == "retriable-after") != (retry_after_ms is not None):
        raise _decode_failure()
    return HumanIntentError(
        status,
        cast(HumanErrorCode, code),
        copy_key,
        cast(HumanRetriability, retry),
        trace,
        field,
        retry_after_ms,
    )


def decode_intent_plan(value: object) -> IntentPlan:
    plan = _mapping(value)
    _exact(plan, ("plan_digest", "journey_kind", "total_fee", "legs", "signing_requirements"))
    rows = plan["legs"]
    if not isinstance(rows, list) or not 0 < len(rows) <= _MAX_LEGS:
        raise _decode_failure()
    legs = tuple(_decode_leg(row, position) for position, row in enumerate(rows))
    requirements = plan["signing_requirements"]
    if not isinstance(requirements, list) or len(requirements) > _MAX_LEGS:
        raise _decode_failure()
    seen: set[int] = set()
    signing_requirements: list[IntentSigningRequirement] = []
    for requirement in requirements:
        decoded = _decode_signing_requirement(requirement)
        if decoded.leg_index >= len(legs) or decoded.leg_index in seen:
            raise _decode_failure()
        seen.add(decoded.leg_index)
        signing_requirements.append(decoded)
    return IntentPlan(
        _decoded_hex32(plan["plan_digest"]),
        _decoded_token(plan["journey_kind"]),
        _decode_money(plan["total_fee"]),
        legs,
        tuple(signing_requirements),
    )


def decode_intent_submission(value: object) -> IntentSubmission:
    submission = _mapping(value)
    _exact(submission, ("journey_id", "plan_digest", "state", "state_copy_key"))
    journey_id = submission["journey_id"]
    state = submission["state"]
    state_copy_key = submission["state_copy_key"]
    if not isinstance(journey_id, str) or _JOURNEY_ID.fullmatch(journey_id) is None:
        raise _decode_failure()
    if not isinstance(state, str) or _LOWER_TOKEN.fullmatch(state) is None:
        raise _decode_failure()
    if not isinstance(state_copy_key, str) or len(state_copy_key) > 128 or _COPY_KEY.fullmatch(state_copy_key) is None:
        raise _decode_failure()
    return IntentSubmission(journey_id, _decoded_hex32(submission["plan_digest"]), state, state_copy_key)


def _decode_leg(value: object, position: int) -> IntentLeg:
    leg = _mapping(value)
    _exact(leg, ("index", "mechanism", "domain", "source", "destination", "money", "fee"))
    if type(leg["index"]) is not int or leg["index"] != position:
        raise _decode_failure()
    domain = leg["domain"]
    if domain not in INTENT_DOMAINS:
        raise _decode_failure()
    return IntentLeg(
        position,
        _decoded_token(leg["mechanism"]),
        cast(IntentDomain, domain),
        _decode_endpoint(leg["source"]),
        _decode_endpoint(leg["destination"]),
        _decode_money(leg["money"]),
        _decode_money(leg["fee"]),
    )


def _decode_signing_requirement(value: object) -> IntentSigningRequirement:
    requirement = _mapping(value)
    _exact(requirement, ("leg_index", "action_key", "signing_context", "authority"))
    leg_index = requirement["leg_index"]
    context = requirement["signing_context"]
    authority = requirement["authority"]
    if type(leg_index) is not int or not 0 <= leg_index < _MAX_LEGS:
        raise _decode_failure()
    if not isinstance(context, str) or not 0 < len(context) <= 2048 or _HEX.fullmatch(context) is None:
        raise _decode_failure()
    if not isinstance(authority, str) or _AUTHORITY.fullmatch(authority) is None:
        raise _decode_failure()
    return IntentSigningRequirement(leg_index, _decoded_hex32(requirement["action_key"]), context, authority)


def _decode_endpoint(value: object) -> IntentEndpointRef:
    endpoint = _mapping(value)
    kind = endpoint.get("kind")
    if kind not in INTENT_ENDPOINT_KINDS:
        raise _decode_failure()
    if kind == "paxeer-wallet":
        _exact(endpoint, ("kind",))
        return IntentEndpointRef(cast(IntentEndpointKind, kind), None)
    _exact(endpoint, ("kind", "account"))
    account = endpoint["account"]
    if not isinstance(account, str) or _ACCOUNT_NAME.fullmatch(account) is None:
        raise _decode_failure()
    return IntentEndpointRef(cast(IntentEndpointKind, kind), account)


def _decode_money(value: object) -> IntentMoney:
    money = _mapping(value)
    _exact(money, ("amount", "currency"))
    amount = money["amount"]
    currency = money["currency"]
    if not isinstance(amount, str) or _DECIMAL.fullmatch(amount) is None:
        raise _decode_failure()
    if not isinstance(currency, str) or _CURRENCY.fullmatch(currency) is None:
        raise _decode_failure()
    try:
        parsed = ProtocolAmount(amount)
    except PlatformSdkError:
        raise _decode_failure() from None
    return IntentMoney(parsed, currency)


def _encode_endpoint(endpoint: IntentEndpointRef) -> dict[str, object]:
    if not isinstance(endpoint, IntentEndpointRef) or endpoint.kind not in INTENT_ENDPOINT_KINDS:
        raise _invalid_argument()
    if endpoint.kind == "paxeer-wallet":
        if endpoint.account is not None:
            raise _invalid_argument()
        return {"kind": endpoint.kind}
    if not isinstance(endpoint.account, str) or _ACCOUNT_NAME.fullmatch(endpoint.account) is None:
        raise _invalid_argument()
    return {"kind": endpoint.kind, "account": endpoint.account}


def _encode_money(money: IntentMoney) -> dict[str, object]:
    if not isinstance(money, IntentMoney) or not isinstance(money.currency, str) or _CURRENCY.fullmatch(money.currency) is None:
        raise _invalid_argument()
    try:
        amount = ProtocolAmount(money.amount)
    except PlatformSdkError:
        raise _invalid_argument() from None
    return {"amount": str(int(amount)), "currency": money.currency}


def _mapping(value: object) -> Mapping[str, object]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise _decode_failure()
    return value


def _exact(value: Mapping[str, object], required: tuple[str, ...]) -> None:
    if set(value) != set(required):
        raise _decode_failure()


def _decoded_hex32(value: object) -> str:
    if not isinstance(value, str) or _HEX32.fullmatch(value) is None:
        raise _decode_failure()
    return value


def _decoded_token(value: object) -> str:
    if not isinstance(value, str) or _LOWER_TOKEN.fullmatch(value) is None:
        raise _decode_failure()
    return value


def _hex32(value: str) -> str:
    if not isinstance(value, str) or _HEX32.fullmatch(value) is None:
        raise _invalid_argument()
    return value


def _text(value: str, pattern: re.Pattern[str]) -> str:
    if not isinstance(value, str) or pattern.fullmatch(value) is None:
        raise _invalid_argument()
    return value


def _boolean(value: bool) -> bool:
    if type(value) is not bool:
        raise _invalid_argument()
    return value


def _leg_position(value: int) -> int:
    if type(value) is not int or not 0 <= value < _MAX_LEGS:
        raise _invalid_argument()
    return value


def _unsigned64(value: int) -> int:
    if type(value) is not int or not 0 <= value <= _MAX_U64:
        raise _invalid_argument()
    return value


def _validated_endpoint(value: str) -> str:
    try:
        parsed = urlparse(value)
        port = parsed.port
    except ValueError:
        raise _invalid_argument() from None
    if parsed.scheme not in {"http", "https"} or not parsed.hostname or parsed.username is not None or parsed.password is not None or parsed.query or parsed.fragment:
        raise _invalid_argument()
    if port is not None and not 0 < port <= 65535:
        raise _invalid_argument()
    if parsed.scheme == "http" and not _loopback(parsed.hostname):
        raise _invalid_argument()
    return urlunparse((parsed.scheme, parsed.netloc, parsed.path.rstrip("/"), "", "", ""))


def _loopback(hostname: str) -> bool:
    lowered = hostname.lower()
    if lowered in {"localhost", "::1"}:
        return True
    octets = lowered.split(".")
    return len(octets) == 4 and octets[0] == "127" and all(octet.isdigit() and 0 <= int(octet) <= 255 for octet in octets)


def _route_endpoint(base: str, path: str) -> str:
    parsed = urlparse(base)
    return urlunparse((parsed.scheme, parsed.netloc, parsed.path.rstrip("/") + path, "", "", ""))


def _bounded_read(response: object, maximum: int) -> bytes:
    reader = getattr(response, "read", None)
    if not callable(reader):
        raise _decode_failure()
    try:
        encoded = cast(bytes, reader(maximum + 1))
    except (OSError, HTTPException):
        raise _decode_failure() from None
    if len(encoded) > maximum:
        raise _decode_failure()
    return encoded


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request: Request, file_pointer: object, code: int, message: str, headers: object, new_url: str) -> None:
        del request, file_pointer, code, message, headers, new_url


def _transport_failure(mutation: bool) -> PlatformSdkError:
    if mutation:
        return PlatformSdkError(SdkErrorCode.UNKNOWN_OUTCOME, "unknown-outcome")
    return PlatformSdkError(SdkErrorCode.TRANSPORT_FAILURE, "safe")


def _invalid_argument() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")


def _decode_failure() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never")
