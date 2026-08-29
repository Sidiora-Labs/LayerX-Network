from __future__ import annotations

from dataclasses import dataclass
import json
from typing import Mapping, cast
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlparse, urlunparse
from urllib.request import Request, urlopen

from .production import (
    IdempotencyKey,
    PlatformPlane,
    PlatformSdkError,
    ProductionTransport,
    SdkErrorCode,
    SecretBytes,
)

_MAX_RESPONSE_BYTES = 8 * 1024 * 1024
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
    __slots__ = ("_endpoint", "_credential", "_timeout", "_maximum_response_bytes")

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
        if not isinstance(maximum_response_bytes, int) or isinstance(maximum_response_bytes, bool) or maximum_response_bytes <= 0:
            raise _invalid_argument()
        self._credential = credential
        self._timeout = float(timeout)
        self._maximum_response_bytes = maximum_response_bytes

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
        if operation == "program.call":
            if idempotency_key is None or not _hex32(str(idempotency_key)):
                raise _invalid_argument()
        elif idempotency_key is not None:
            raise _invalid_argument()
        if operation in {"program.discover", "program.interface", "program.receipt", "program.activity"} and request.get("requested_verification_level") != "sequencer-signed":
            raise _invalid_argument()
        try:
            body = json.dumps(request, ensure_ascii=True, allow_nan=False, separators=(",", ":")).encode("utf-8")
        except (TypeError, ValueError, OverflowError):
            raise _invalid_argument() from None
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
        outbound = Request(_route_endpoint(self._endpoint, path), data=body, headers=headers, method=route.method)
        try:
            with urlopen(outbound, timeout=self._timeout) as response:
                encoded = _bounded_read(response, self._maximum_response_bytes)
                return _decode_envelope(response.status, encoded)
        except HTTPError as error:
            try:
                encoded = _bounded_read(error, self._maximum_response_bytes)
                return _decode_envelope(error.code, encoded)
            finally:
                error.close()
        except PlatformSdkError:
            raise
        except (TimeoutError, URLError, OSError):
            raise _transport_failure(operation) from None


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
    encoded = cast(bytes, reader(maximum + 1))
    if len(encoded) > maximum:
        raise _decode_failure()
    return encoded


def _decode_envelope(status: int, encoded: bytes) -> object:
    try:
        envelope = json.loads(encoded.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        raise _decode_failure() from None
    if not isinstance(envelope, dict) or any(not isinstance(key, str) for key in envelope):
        raise _decode_failure()
    if "class" in envelope:
        raise _service_error(status, envelope)
    request_id = envelope.get("request_id")
    if not 200 <= status < 300 or not isinstance(request_id, str) or not request_id or "value" not in envelope:
        raise _decode_failure(request_id if isinstance(request_id, str) else None)
    if not _achieved_sequencer(envelope.get("verification_status")):
        raise PlatformSdkError(SdkErrorCode.VERIFICATION_FAILURE, "never", request_id=request_id)
    return envelope["value"]


def _achieved_sequencer(value: object) -> bool:
    return isinstance(value, dict) and value.get("state") == "Achieved" and value.get("level") == "SequencerSigned"


def _service_error(status: int, error: Mapping[str, object]) -> PlatformSdkError:
    request_id = error.get("request_id")
    exact_class = error.get("class")
    retriability = error.get("retriability")
    reason = error.get("reason")
    protocol = error.get("protocol_result_code")
    code = _ERROR_CLASS.get(exact_class) if isinstance(exact_class, str) else None
    if 200 <= status < 300 or not isinstance(request_id, str) or not request_id or code is None or retriability not in {"Terminal", "Retriable"} or not isinstance(reason, str) or not reason or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789_." for character in reason) or (protocol is not None and (not isinstance(protocol, int) or isinstance(protocol, bool))):
        raise _decode_failure(request_id if isinstance(request_id, str) else None)
    return PlatformSdkError(
        code,
        "safe" if retriability == "Retriable" else "never",
        request_id=request_id,
        protocol_result_code=cast(int | None, protocol),
    )


def _hex32(value: str) -> bool:
    return len(value) == 64 and all(character in _HEX for character in value)


def _transport_failure(operation: str) -> PlatformSdkError:
    if operation == "program.call":
        return PlatformSdkError(SdkErrorCode.UNKNOWN_OUTCOME, "unknown-outcome")
    return PlatformSdkError(SdkErrorCode.TRANSPORT_FAILURE, "safe")


def _invalid_argument() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.INVALID_ARGUMENT, "never")


def _unavailable_capability() -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.UNAVAILABLE_CAPABILITY, "never")


def _decode_failure(request_id: str | None = None) -> PlatformSdkError:
    return PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never", request_id=request_id)
