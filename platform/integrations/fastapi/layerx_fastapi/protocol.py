from __future__ import annotations

import json
from base64 import b64decode, b64encode
from binascii import Error as BinasciiError
from dataclasses import dataclass, field
from enum import StrEnum
from hashlib import sha256
from re import fullmatch
from typing import Mapping, Sequence

from layerx_sdk import (
    AuthorizedReceiptBatch,
    LocalSignatureVerifier,
    PlatformSdkError,
    ReceiptVerification,
    grant_payment_terms,
    payment_payer,
    verify_receipt,
)

X402_VERSION = 2
PAYMENT_REQUIRED_HEADER = "PAYMENT-REQUIRED"
PAYMENT_SIGNATURE_HEADER = "PAYMENT-SIGNATURE"
PAYMENT_RESPONSE_HEADER = "PAYMENT-RESPONSE"

MERKLE_LEAF_DOMAIN = b"LXP/v1/merkle-leaf\0"
PAYMENT_KEY_DOMAIN = b"LayerX/middleware/x402/idempotency\0"
MAX_HEADER_BYTES = 64 * 1024
MAX_U128 = 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF

JsonValue = None | bool | int | float | str | list["JsonValue"] | Mapping[str, "JsonValue"]


class MiddlewareErrorCode(StrEnum):
    INVALID_PAYMENT_REQUIRED = "invalid-payment-required"
    INVALID_PAYMENT_PAYLOAD = "invalid-payment-payload"
    REQUIREMENTS_MISMATCH = "requirements-mismatch"
    UNSUPPORTED_PAYMENT = "unsupported-payment"
    PAYMENT_PENDING = "payment-pending"
    PAYMENT_REFUSED = "payment-refused"
    VERIFICATION_FAILURE = "verification-failure"
    FULFILLMENT_CONFLICT = "fulfillment-conflict"
    INVALID_WEBHOOK = "invalid-webhook"
    WEBHOOK_REPLAY = "webhook-replay"


class MiddlewareError(Exception):
    __slots__ = ("code",)

    def __init__(self, code: MiddlewareErrorCode) -> None:
        super().__init__(code.value)
        self.code = code


@dataclass(frozen=True)
class ResourceInfo:
    url: str
    description: str | None = None
    mime_type: str | None = None
    service_name: str | None = None
    tags: tuple[str, ...] | None = None
    icon_url: str | None = None

    def to_wire(self) -> dict[str, JsonValue]:
        wire: dict[str, JsonValue] = {"url": self.url}
        if self.description is not None:
            wire["description"] = self.description
        if self.mime_type is not None:
            wire["mimeType"] = self.mime_type
        if self.service_name is not None:
            wire["serviceName"] = self.service_name
        if self.tags is not None:
            wire["tags"] = list(self.tags)
        if self.icon_url is not None:
            wire["iconUrl"] = self.icon_url
        return wire


@dataclass(frozen=True)
class X402Extension:
    info: JsonValue
    schema: JsonValue

    def to_wire(self) -> dict[str, JsonValue]:
        return {"info": self.info, "schema": self.schema}


@dataclass(frozen=True)
class PaymentRequirements:
    scheme: str
    network: str
    amount: str
    asset: str
    pay_to: str
    max_timeout_seconds: int
    extra: JsonValue = None
    has_extra: bool = False

    def to_wire(self) -> dict[str, JsonValue]:
        wire: dict[str, JsonValue] = {
            "scheme": self.scheme,
            "network": self.network,
            "amount": self.amount,
            "asset": self.asset,
            "payTo": self.pay_to,
            "maxTimeoutSeconds": self.max_timeout_seconds,
        }
        if self.has_extra:
            wire["extra"] = self.extra
        return wire


@dataclass(frozen=True)
class PaymentRequired:
    resource: ResourceInfo
    accepts: tuple[PaymentRequirements, ...]
    error: str | None = None
    extensions: Mapping[str, X402Extension] | None = None

    def to_wire(self) -> dict[str, JsonValue]:
        wire: dict[str, JsonValue] = {
            "x402Version": X402_VERSION,
            "resource": self.resource.to_wire(),
            "accepts": [item.to_wire() for item in self.accepts],
        }
        if self.error is not None:
            wire["error"] = self.error
        if self.extensions is not None:
            wire["extensions"] = {name: item.to_wire() for name, item in self.extensions.items()}
        return wire


@dataclass(frozen=True)
class PaymentPayload:
    payload: Mapping[str, JsonValue]
    accepted: PaymentRequirements
    resource: ResourceInfo | None = None
    extensions: Mapping[str, X402Extension] | None = None

    def to_wire(self) -> dict[str, JsonValue]:
        wire: dict[str, JsonValue] = {
            "x402Version": X402_VERSION,
            "payload": dict(self.payload),
            "accepted": self.accepted.to_wire(),
        }
        if self.resource is not None:
            wire["resource"] = self.resource.to_wire()
        if self.extensions is not None:
            wire["extensions"] = {name: item.to_wire() for name, item in self.extensions.items()}
        return wire


@dataclass(frozen=True)
class LayerXReceiptEvidence:
    receipt: str
    receipt_digest: str
    verification_level: str = "sequencer-signed"


@dataclass(frozen=True)
class SettlementResponse:
    success: bool
    transaction: str
    network: str
    error_reason: str | None = None
    payer: str | None = None
    amount: str | None = None
    extensions: Mapping[str, JsonValue] = field(default_factory=dict)
    has_extensions: bool = False

    def to_wire(self) -> dict[str, JsonValue]:
        wire: dict[str, JsonValue] = {
            "success": self.success,
            "transaction": self.transaction,
            "network": self.network,
        }
        if self.error_reason is not None:
            wire["errorReason"] = self.error_reason
        if self.payer is not None:
            wire["payer"] = self.payer
        if self.amount is not None:
            wire["amount"] = self.amount
        if self.has_extensions:
            wire["extensions"] = dict(self.extensions)
        return wire


def canonical_json(value: JsonValue) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        if value != value or value in (float("inf"), float("-inf")):
            raise MiddlewareError(MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD)
        return json.dumps(value)
    if isinstance(value, (list, tuple)):
        return "[" + ",".join(canonical_json(item) for item in value) + "]"
    if isinstance(value, Mapping):
        parts = [
            f"{json.dumps(str(key), ensure_ascii=False)}:{canonical_json(item)}"
            for key, item in sorted(value.items(), key=lambda entry: str(entry[0]))
        ]
        return "{" + ",".join(parts) + "}"
    raise MiddlewareError(MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD)


def sha256_digest(*values: bytes) -> bytes:
    digest = sha256()
    for value in values:
        digest.update(value)
    return digest.digest()


def merkle_leaf_digest(canonical_receipt: bytes) -> bytes:
    return sha256_digest(MERKLE_LEAF_DOMAIN, canonical_receipt)


def payment_idempotency_key(principal: str, request_digest: bytes) -> str:
    return sha256_digest(PAYMENT_KEY_DOMAIN, principal.encode("utf-8"), request_digest).hex()


def encode_payment_required_header(value: PaymentRequired) -> str:
    return _encode_header(parse_payment_required(value.to_wire()).to_wire())


def decode_payment_required_header(value: str) -> PaymentRequired:
    return parse_payment_required(_decode_header(value, MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED))


def encode_payment_payload_header(value: PaymentPayload) -> str:
    return _encode_header(parse_payment_payload(value.to_wire()).to_wire())


def decode_payment_payload_header(value: str) -> PaymentPayload:
    return parse_payment_payload(_decode_header(value, MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD))


def encode_settlement_header(value: SettlementResponse) -> str:
    return _encode_header(parse_settlement(value.to_wire()).to_wire())


def decode_settlement_header(value: str) -> SettlementResponse:
    return parse_settlement(_decode_header(value, MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD))


def parse_payment_required(value: JsonValue) -> PaymentRequired:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    body = _as_object(value, code)
    _exact_keys(body, ("x402Version", "resource", "accepts"), ("error", "extensions"), code)
    if body.get("x402Version") != X402_VERSION:
        raise MiddlewareError(code)
    accepts = _as_array(body.get("accepts"), code)
    if not 0 < len(accepts) <= 32:
        raise MiddlewareError(code)
    error = body.get("error")
    extensions = body.get("extensions")
    return PaymentRequired(
        resource=parse_resource(body.get("resource")),
        accepts=tuple(parse_requirements(item) for item in accepts),
        error=None if error is None else _bounded_string(error, 512, code),
        extensions=None if extensions is None else parse_extensions(extensions),
    )


def parse_payment_payload(value: JsonValue) -> PaymentPayload:
    code = MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD
    body = _as_object(value, code)
    _exact_keys(body, ("x402Version", "payload", "accepted"), ("resource", "extensions"), code)
    if body.get("x402Version") != X402_VERSION:
        raise MiddlewareError(code)
    resource = body.get("resource")
    extensions = body.get("extensions")
    return PaymentPayload(
        payload=dict(_as_object(body.get("payload"), code)),
        accepted=parse_requirements(body.get("accepted")),
        resource=None if resource is None else parse_resource(resource),
        extensions=None if extensions is None else parse_extensions(extensions),
    )


def parse_settlement(value: JsonValue) -> SettlementResponse:
    code = MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD
    body = _as_object(value, code)
    _exact_keys(
        body,
        ("success", "transaction", "network"),
        ("errorReason", "payer", "amount", "extensions"),
        code,
    )
    success = body.get("success")
    if not isinstance(success, bool):
        raise MiddlewareError(code)
    transaction = body.get("transaction")
    if not isinstance(transaction, str):
        raise MiddlewareError(code)
    raw_reason = body.get("errorReason")
    error_reason = None if raw_reason is None else _bounded_string(raw_reason, 512, code)
    if success and (len(transaction) == 0 or error_reason is not None):
        raise MiddlewareError(code)
    if not success and error_reason is None:
        raise MiddlewareError(code)
    if not success and error_reason != "settlement_pending" and len(transaction) != 0:
        raise MiddlewareError(code)
    if not success and error_reason == "settlement_pending" and len(transaction) == 0:
        raise MiddlewareError(code)
    payer = body.get("payer")
    amount = body.get("amount")
    extensions = body.get("extensions")
    return SettlementResponse(
        success=success,
        transaction=transaction,
        network=parse_network(body.get("network")),
        error_reason=error_reason,
        payer=None if payer is None else _bounded_string(payer, 256, code),
        amount=None if amount is None else parse_amount(amount),
        extensions={} if extensions is None else dict(_as_json_record(extensions, code)),
        has_extensions=extensions is not None,
    )


def parse_resource(value: JsonValue) -> ResourceInfo:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    body = _as_object(value, code)
    _exact_keys(
        body,
        ("url",),
        ("description", "mimeType", "serviceName", "tags", "iconUrl"),
        code,
    )
    raw_tags = body.get("tags")
    tags: tuple[str, ...] | None = None
    if raw_tags is not None:
        tags = tuple(_printable_string(item, 32) for item in _as_array(raw_tags, code))
        if len(tags) > 5:
            raise MiddlewareError(code)
    description = body.get("description")
    mime_type = body.get("mimeType")
    service_name = body.get("serviceName")
    icon_url = body.get("iconUrl")
    return ResourceInfo(
        url=parse_url(body.get("url")),
        description=None if description is None else _bounded_string(description, 512, code),
        mime_type=None if mime_type is None else _bounded_string(mime_type, 32, code),
        service_name=None if service_name is None else _printable_string(service_name, 32),
        tags=tags,
        icon_url=None if icon_url is None else parse_url(icon_url),
    )


def parse_requirements(value: JsonValue) -> PaymentRequirements:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    body = _as_object(value, code)
    _exact_keys(
        body,
        ("scheme", "network", "amount", "asset", "payTo", "maxTimeoutSeconds"),
        ("extra",),
        code,
    )
    timeout = body.get("maxTimeoutSeconds")
    if isinstance(timeout, bool) or not isinstance(timeout, int) or not 0 < timeout <= 0xFFFF_FFFF:
        raise MiddlewareError(code)
    asset = _bounded_string(body.get("asset"), 256, code)
    pay_to = _bounded_string(body.get("payTo"), 256, code)
    parse_hex32(asset)
    parse_hex32(pay_to)
    has_extra = "extra" in body
    return PaymentRequirements(
        scheme=_identifier_string(body.get("scheme"), 32, code),
        network=parse_network(body.get("network")),
        amount=parse_amount(body.get("amount")),
        asset=asset,
        pay_to=pay_to,
        max_timeout_seconds=timeout,
        extra=body.get("extra") if has_extra else None,
        has_extra=has_extra,
    )


def parse_extensions(value: JsonValue) -> dict[str, X402Extension]:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    body = _as_object(value, code)
    if len(body) > 32:
        raise MiddlewareError(code)
    extensions: dict[str, X402Extension] = {}
    for name, item in body.items():
        if not _is_identifier(name, 32):
            raise MiddlewareError(code)
        entry = _as_object(item, code)
        _exact_keys(entry, ("info", "schema"), (), code)
        canonical_json(entry.get("info"))
        canonical_json(entry.get("schema"))
        extensions[name] = X402Extension(info=entry.get("info"), schema=entry.get("schema"))
    return extensions


def match_requirements(required: PaymentRequired, payload: PaymentPayload) -> PaymentRequirements:
    accepted = canonical_json(payload.accepted.to_wire())
    for candidate in required.accepts:
        if canonical_json(candidate.to_wire()) == accepted:
            break
    else:
        raise MiddlewareError(MiddlewareErrorCode.REQUIREMENTS_MISMATCH)
    for name, extension in (required.extensions or {}).items():
        actual = (payload.extensions or {}).get(name)
        if actual is None or canonical_json(actual.to_wire()) != canonical_json(extension.to_wire()):
            raise MiddlewareError(MiddlewareErrorCode.REQUIREMENTS_MISMATCH)
    return candidate


def parse_layerx_evidence(payload: Mapping[str, JsonValue]) -> LayerXReceiptEvidence:
    code = MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD
    body = _as_object(payload, code)
    _exact_keys(body, ("receipt", "receiptDigest", "verificationLevel"), ("idempotencyKey",), code)
    if body.get("verificationLevel") != "sequencer-signed":
        raise MiddlewareError(MiddlewareErrorCode.VERIFICATION_FAILURE)
    receipt_digest = body.get("receiptDigest")
    if not isinstance(receipt_digest, str):
        raise MiddlewareError(code)
    parse_hex32(receipt_digest)
    return LayerXReceiptEvidence(
        receipt=_bounded_string(body.get("receipt"), MAX_HEADER_BYTES, code),
        receipt_digest=receipt_digest,
    )


def verify_payment_receipt(
    canonical_receipt: bytes,
    authorized_batch: AuthorizedReceiptBatch,
    requirements: PaymentRequirements,
    signatures: LocalSignatureVerifier,
) -> ReceiptVerification:
    try:
        verified = verify_receipt(canonical_receipt, authorized_batch, signatures)
        payer = (
            payment_payer(requirements.extra)
            if requirements.scheme == "exact"
            else grant_payment_terms(requirements.extra)[2]
        )
    except PlatformSdkError as error:
        raise MiddlewareError(MiddlewareErrorCode.VERIFICATION_FAILURE) from error
    if (
        verified.receipt.amount != int(requirements.amount)
        or not constant_time_equal(verified.receipt.asset, parse_hex32(requirements.asset))
        or not constant_time_equal(verified.receipt.to_account, parse_hex32(requirements.pay_to))
        or not constant_time_equal(verified.receipt.from_account, parse_hex32(payer))
    ):
        raise MiddlewareError(MiddlewareErrorCode.VERIFICATION_FAILURE)
    return verified


def parse_amount(value: JsonValue) -> str:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    if not isinstance(value, str) or fullmatch(r"0|[1-9][0-9]*", value) is None:
        raise MiddlewareError(code)
    amount = int(value)
    if not 0 < amount <= MAX_U128:
        raise MiddlewareError(code)
    return value


def parse_network(value: JsonValue) -> str:
    if not isinstance(value, str):
        raise MiddlewareError(MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED)
    parts = value.split(":")
    if len(parts) != 2 or parts[0] != "layerx" or not _is_identifier(parts[1], 64):
        raise MiddlewareError(MiddlewareErrorCode.UNSUPPORTED_PAYMENT)
    return value


def parse_url(value: JsonValue) -> str:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    text = _bounded_string(value, 2048, code)
    if fullmatch(r"https?://[^\s\x00-\x1f\x7f]+", text) is None:
        raise MiddlewareError(code)
    return text


def parse_hex32(value: str) -> bytes:
    digits = value[2:] if value.startswith("0x") else value
    if fullmatch(r"[0-9a-fA-F]{64}", digits) is None:
        raise MiddlewareError(MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED)
    return bytes.fromhex(digits)


def constant_time_equal(left: bytes, right: bytes) -> bool:
    if len(left) != len(right):
        return False
    difference = 0
    for left_byte, right_byte in zip(left, right, strict=True):
        difference |= left_byte ^ right_byte
    return difference == 0


def constant_time_equal_text(left: str, right: str) -> bool:
    return constant_time_equal(left.encode("utf-8"), right.encode("utf-8"))


def encode_base64(value: bytes) -> str:
    return b64encode(value).decode("ascii")


def decode_base64(value: str, code: MiddlewareErrorCode) -> bytes:
    if len(value) == 0 or len(value) > MAX_HEADER_BYTES * 2 or fullmatch(r"[A-Za-z0-9+/]*={0,2}", value) is None:
        raise MiddlewareError(code)
    try:
        return b64decode(value, validate=True)
    except (BinasciiError, ValueError) as error:
        raise MiddlewareError(code) from error


def _encode_header(value: JsonValue) -> str:
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(encoded) > MAX_HEADER_BYTES:
        raise MiddlewareError(MiddlewareErrorCode.INVALID_PAYMENT_PAYLOAD)
    return encode_base64(encoded)


def _decode_header(value: str, code: MiddlewareErrorCode) -> JsonValue:
    decoded = decode_base64(value, code)
    if len(decoded) > MAX_HEADER_BYTES:
        raise MiddlewareError(code)
    try:
        return json.loads(decoded.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise MiddlewareError(code) from error


def _as_object(value: JsonValue, code: MiddlewareErrorCode) -> Mapping[str, JsonValue]:
    if not isinstance(value, Mapping):
        raise MiddlewareError(code)
    for key in value:
        if not isinstance(key, str):
            raise MiddlewareError(code)
    return value


def _as_array(value: JsonValue, code: MiddlewareErrorCode) -> Sequence[JsonValue]:
    if not isinstance(value, list):
        raise MiddlewareError(code)
    return value


def _as_json_record(value: JsonValue, code: MiddlewareErrorCode) -> Mapping[str, JsonValue]:
    body = _as_object(value, code)
    for item in body.values():
        canonical_json(item)
    return body


def _exact_keys(
    value: Mapping[str, JsonValue],
    required: tuple[str, ...],
    optional: tuple[str, ...],
    code: MiddlewareErrorCode,
) -> None:
    allowed = set(required) | set(optional)
    if any(value.get(key) is None for key in required) or any(key not in allowed for key in value):
        raise MiddlewareError(code)


def _bounded_string(value: JsonValue, limit: int, code: MiddlewareErrorCode) -> str:
    if not isinstance(value, str) or not _is_bounded(value, limit):
        raise MiddlewareError(code)
    return value


def _printable_string(value: JsonValue, limit: int) -> str:
    code = MiddlewareErrorCode.INVALID_PAYMENT_REQUIRED
    text = _bounded_string(value, limit, code)
    if fullmatch(r"[\x20-\x7e]+", text) is None:
        raise MiddlewareError(code)
    return text


def _identifier_string(value: JsonValue, limit: int, code: MiddlewareErrorCode) -> str:
    if not isinstance(value, str) or not _is_identifier(value, limit):
        raise MiddlewareError(code)
    return value


def _is_bounded(value: str, limit: int) -> bool:
    return 0 < len(value) <= limit and "\0" not in value


def _is_identifier(value: str, limit: int) -> bool:
    return _is_bounded(value, limit) and fullmatch(r"[A-Za-z0-9._-]+", value) is not None
