from __future__ import annotations

from dataclasses import dataclass
import json
from http.client import HTTPException
from typing import Mapping, cast
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlparse, urlunparse
from urllib.request import HTTPRedirectHandler, Request, build_opener
from .program_wire import bind_signed_program_lifecycle

from .production import (
    IdempotencyKey,
    PlatformPlane,
    PlatformSdkError,
    ProductionTransport,
    SdkErrorCode,
    SecretBytes,
)

_MAX_RESPONSE_BYTES = 8 * 1024 * 1024
_MAX_REQUEST_BYTES = 4 * 1024 * 1024
_HEX = frozenset("0123456789abcdef")
_ERROR_CLASS: Mapping[str, SdkErrorCode] = {
    "TransportFailure": SdkErrorCode.TRANSPORT_FAILURE,
    "Deadline": SdkErrorCode.DEADLINE,
    "ProtocolIncompatibility": SdkErrorCode.PROTOCOL_INCOMPATIBILITY,
    "UnavailableCapability": SdkErrorCode.UNAVAILABLE_CAPABILITY,
    "CoreRejection": SdkErrorCode.CORE_REJECTION,
    "VerificationFailure": SdkErrorCode.VERIFICATION_FAILURE,
    "PolicyRefusal": SdkErrorCode.POLICY_REFUSAL,
    "CapabilityRefusal": SdkErrorCode.CAPABILITY_REFUSAL,
    "BudgetRefusal": SdkErrorCode.BUDGET_REFUSAL,
    "RateLimit": SdkErrorCode.RATE_LIMIT,
    "IdempotencyConflict": SdkErrorCode.IDEMPOTENCY_CONFLICT,
    "InternalFault": SdkErrorCode.INTERNAL_FAULT,
}


@dataclass(frozen=True)
class _Route:
    method: str
    path: str
    path_field: str | None = None


_ROUTES: Mapping[str, _Route] = {
    "program.deploy": _Route("POST", "/v1/programs/deploy"),
    "program.upgrade": _Route("POST", "/v1/programs/upgrade"),
    "program.wind-down": _Route("POST", "/v1/programs/wind-down"),
    "program.discover": _Route("GET", "/v1/programs/registry/{program_id}", "program_id"),
    "program.interface": _Route("GET", "/v1/programs/registry/{program_id}/interface", "program_id"),
    "program.simulate": _Route("POST", "/v1/programs/simulate"),
    "program.call": _Route("POST", "/v1/programs/call"),
    "program.receipt": _Route("GET", "/v1/programs/receipts/by-idempotency/{idempotency_key}", "idempotency_key"),
    "program.activity": _Route("GET", "/v1/programs/activities/{activity_id}", "activity_id"),
}


class LayerXKeyCredential:
    __slots__ = ("_key_id", "_secret")

    def __init__(self, key_id: str, secret: SecretBytes) -> None:
        if not 0 < len(key_id) <= 64 or not key_id.isascii() or any(
            not (character.isalnum() or character in "-_") for character in key_id
        ):
            raise _invalid_argument()
        self._key_id = key_id
        self._secret = secret

    def use(self) -> str:
        def authorization(value: memoryview) -> str:
            try:
                secret = bytes(value).decode("ascii")
            except UnicodeDecodeError:
                raise _invalid_argument() from None
            suffix = secret.removeprefix("lxp_live_")
            if len(suffix) != 64 or any(character not in _HEX for character in suffix):
                raise _invalid_argument()
            return f"LayerX-Key {self._key_id}:{secret}"

        return self._secret.use(authorization)

    def __repr__(self) -> str:
        return "LayerXKeyCredential([REDACTED])"

    def __str__(self) -> str:
        return "[REDACTED]"


class AgentHttpTransport(ProductionTransport):
    __slots__ = ("_endpoint", "_credential", "_timeout", "_maximum_response_bytes", "_opener")

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
        if not isinstance(maximum_response_bytes, int) or isinstance(maximum_response_bytes, bool) or maximum_response_bytes <= 0 or maximum_response_bytes > _MAX_RESPONSE_BYTES:
            raise _invalid_argument()
        self._credential = credential
        self._timeout = float(timeout)
        self._maximum_response_bytes = maximum_response_bytes
        self._opener = build_opener(_NoRedirect())

    def call(
        self,
        plane: PlatformPlane,
        operation: object,
        request: object,
        idempotency_key: IdempotencyKey | None,
    ) -> object:
        if plane != "agent" or not isinstance(operation, str) or operation not in _ROUTES:
            raise _unavailable_capability()
        if not isinstance(request, Mapping) or any(not isinstance(key, str) for key in request):
            raise _invalid_argument()
        route = _ROUTES[operation]
        path = route.path
        if route.path_field is not None:
            value = request.get(route.path_field)
            if not isinstance(value, str) or not _hex32(value):
                raise _invalid_argument()
            path = path.replace("{" + route.path_field + "}", quote(value, safe=""))
        if operation in {"program.call", "program.deploy", "program.upgrade", "program.wind-down"}:
            if idempotency_key is None or not _hex32(str(idempotency_key)):
                raise _invalid_argument()
        elif idempotency_key is not None:
            raise _invalid_argument()
        if operation in {"program.discover", "program.interface", "program.receipt", "program.activity"} and request.get("requested_verification_level") != "sequencer-signed":
            raise _invalid_argument()
        _require_exact_request(operation, request)
        try:
            if route.method == "POST":
                body = _encode_program_mutation_body(request.get("signed_activity"))
            else:
                body = json.dumps(request, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("utf-8")
        except (TypeError, ValueError, OverflowError):
            raise _invalid_argument() from None
        if len(body) > _MAX_REQUEST_BYTES:
            raise _invalid_argument()
        if operation in {"program.deploy", "program.upgrade", "program.wind-down"}:
            ordinal = {"program.deploy": 1, "program.upgrade": 2, "program.wind-down": 7}[operation]
            try:
                bind_signed_program_lifecycle(body, None, ordinal, str(idempotency_key))
            except (TypeError, ValueError, OverflowError):
                raise _invalid_argument() from None
        headers = {
            "Accept": "application/json",
            "Content-Type": "application/octet-stream" if route.method == "POST" else "application/json",
            "Content-Length": str(len(body)),
            "User-Agent": "layerx-python/0.1.0",
        }
        if idempotency_key is not None:
            headers["Idempotency-Key"] = str(idempotency_key)
        if self._credential is not None:
            headers["Authorization"] = self._credential.use()
        outbound = Request(_route_endpoint(self._endpoint, path), data=body, headers=headers, method=route.method)
        try:
            with self._opener.open(outbound, timeout=self._timeout) as response:
                encoded = _bounded_read(response, self._maximum_response_bytes)
                if response.headers.get("Content-Type") != "application/json":
                    raise _decode_failure()
                return _decode_envelope(response.status, encoded, operation)
        except HTTPError as error:
            try:
                encoded = _bounded_read(error, self._maximum_response_bytes)
                if error.headers.get("Content-Type") != "application/json":
                    raise _decode_failure()
                return _decode_envelope(error.code, encoded, operation)
            finally:
                error.close()
        except PlatformSdkError:
            raise
        except (TimeoutError, URLError, OSError, HTTPException):
            raise _transport_failure(operation) from None


def _encode_program_mutation_body(signed: object) -> bytes:
    if not isinstance(signed, str) or not 0 < len(signed) <= 2_097_152 or len(signed) % 2 or any(character not in _HEX for character in signed):
        raise _invalid_argument()
    return bytes.fromhex(signed)


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


def _require_exact_request(operation: str, request: Mapping[str, object]) -> None:
    fields: Mapping[str, frozenset[str]] = {
        "program.deploy": frozenset(("signed_activity",)),
        "program.upgrade": frozenset(("signed_activity",)),
        "program.wind-down": frozenset(("signed_activity",)),
        "program.discover": frozenset(("program_id", "requested_verification_level")),
        "program.interface": frozenset(("program_id", "requested_verification_level")),
        "program.simulate": frozenset(("program_id", "calldata", "budget", "capabilities", "signed_activity")),
        "program.call": frozenset(("program_id", "calldata", "budget", "capabilities", "signed_activity")),
        "program.receipt": frozenset(("idempotency_key", "expected_activity_id", "requested_verification_level")),
        "program.activity": frozenset(("activity_id", "requested_verification_level")),
    }
    expected = fields[operation]
    if operation in {"program.call", "program.simulate"} and request.get("payload_encoding") == "native-v1":
        expected = frozenset(("payload_encoding", "program_id", "calldata", "budget", "signed_activity", "native_call"))
    if frozenset(request) != expected:
        raise _invalid_argument()


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


def _decode_envelope(status: int, encoded: bytes, operation: str) -> object:
    try:
        envelope = json.loads(encoded.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise _decode_failure() from None
    if not isinstance(envelope, dict) or any(not isinstance(key, str) for key in envelope):
        raise _decode_failure()
    if "class" in envelope:
        _exact(envelope, ("class", "protocol_result_code", "retriability", "reason", "request_id"))
        raise _service_error(status, envelope)
    if operation in {"program.deploy", "program.upgrade", "program.wind-down"} or operation == "program.receipt" and ("result" in envelope or "error" in envelope):
        if "error" in envelope:
            _exact(envelope, ("error",))
            raise _decode_program_boundary_error(status, envelope["error"])
        if status == 202 and envelope.get("state") == "unknown":
            return envelope
        _exact(envelope, ("result",))
        if not 200 <= status < 300:
            raise _decode_failure()
        return envelope["result"]
    _exact(envelope, ("request_id", "value", "verification_status"))
    request_id = envelope.get("request_id")
    if not 200 <= status < 300 or not _valid_request_id(request_id) or "value" not in envelope:
        raise _decode_failure(request_id if isinstance(request_id, str) else None)
    if not _accepted_program_verification(operation, envelope.get("value"), envelope.get("verification_status")):
        raise PlatformSdkError(SdkErrorCode.VERIFICATION_FAILURE, "never", request_id=request_id)
    return envelope["value"]


class ProgramBoundaryError(PlatformSdkError):
    def __init__(self, status: int, boundary_code: str, retry_after_ms: int | None = None) -> None:
        super().__init__(SdkErrorCode.IDEMPOTENCY_CONFLICT if status == 409 else SdkErrorCode.RATE_LIMIT if status == 429 else SdkErrorCode.CORE_REJECTION,
                         "never" if retry_after_ms is None else "safe", retry_after_ms=retry_after_ms)
        self.status = status
        self.boundary_code = boundary_code


def _decode_program_boundary_error(status: int, value: object) -> ProgramBoundaryError:
    if type(status) is not int or not 400 <= status < 600 or not isinstance(value, dict):
        raise _decode_failure()
    code = value.get("code")
    if not isinstance(code, str) or not 0 < len(code) <= 128 or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789_" for character in code):
        raise _decode_failure()
    if value.get("retry") == "never":
        _exact(value, ("code", "retry"))
        return ProgramBoundaryError(status, code)
    _exact(value, ("code", "retry", "retry_after_seconds"))
    seconds = value.get("retry_after_seconds")
    if value.get("retry") != "after" or type(seconds) is not int or not 0 < seconds < 1 << 64:
        raise _decode_failure()
    return ProgramBoundaryError(status, code, seconds * 1000)


def _accepted_program_verification(operation: str, result: object, value: object) -> bool:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        return False
    result_state = result.get("state") if isinstance(result, dict) else None
    if operation in {"program.discover", "program.interface"}:
        return _exact_unverified(value, "server_side_receipt_verification_only")
    if operation in {"program.call", "program.receipt", "program.activity"} and result_state in {"unknown", "pending"}:
        return _exact_unverified(value, "receipt_pending")
    return set(value) == {"state", "level"} and value.get("state") == "Achieved" and value.get("level") == "SequencerSigned"


def _exact_unverified(value: Mapping[str, object], reason: str) -> bool:
    return set(value) == {"state", "requested", "achieved", "reason"} and value.get("state") == "Unverified" and value.get("requested") == "SequencerSigned" and value.get("achieved") == "Unverified" and value.get("reason") == reason


def _service_error(status: int, error: Mapping[str, object]) -> PlatformSdkError:
    request_id = error.get("request_id")
    exact_class = error.get("class")
    retriability = error.get("retriability")
    reason = error.get("reason")
    protocol = error.get("protocol_result_code")
    code = _ERROR_CLASS.get(exact_class) if isinstance(exact_class, str) else None
    if 200 <= status < 300 or not _valid_request_id(request_id) or code is None or retriability not in {"Terminal", "Retriable"} or not isinstance(reason, str) or not reason or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789_." for character in reason) or (protocol is not None and (not isinstance(protocol, int) or isinstance(protocol, bool))):
        raise _decode_failure(request_id if isinstance(request_id, str) else None)
    return PlatformSdkError(
        code,
        "safe" if retriability == "Retriable" else "never",
        request_id=request_id,
        protocol_result_code=cast(int | None, protocol),
    )


def _hex32(value: str) -> bool:
    return len(value) == 64 and all(character in _HEX for character in value)


def _exact(value: Mapping[str, object], required: tuple[str, ...]) -> None:
    if set(value) != set(required):
        raise _decode_failure()


def _valid_request_id(value: object) -> bool:
    return isinstance(value, str) and 0 < len(value) <= 128 and value.isascii() and all(0x21 <= ord(character) <= 0x7E for character in value)


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, request: Request, file_pointer: object, code: int, message: str, headers: object, new_url: str) -> None:
        del request, file_pointer, code, message, headers, new_url
        return None


def _transport_failure(operation: str) -> PlatformSdkError:
    if operation in {"program.call", "program.deploy", "program.upgrade", "program.wind-down"}:
        return PlatformSdkError(SdkErrorCode.UNKNOWN_OUTCOME, "unknown-outcome")
    return PlatformSdkError(SdkErrorCode.TRANSPORT_FAILURE, "safe")


def _invalid_argument() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")


def _unavailable_capability() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.UNAVAILABLE_CAPABILITY, "never")


def _decode_failure(request_id: str | None = None) -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never", request_id=request_id)
