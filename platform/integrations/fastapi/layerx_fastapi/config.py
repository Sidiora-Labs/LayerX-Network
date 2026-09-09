from __future__ import annotations

import json
from base64 import b64decode
from binascii import Error as BinasciiError
from dataclasses import dataclass
from enum import StrEnum
from os import environ
from re import fullmatch, search
from typing import Mapping

from layerx_sdk import AuthorizedReceiptBatch

from .protocol import (
    JsonValue,
    PaymentRequired,
    PaymentRequirements,
    ResourceInfo,
)

DECLARED_KEYS = (
    "LAYERX_PRINCIPAL",
    "LAYERX_PROTECTED_PATH",
    "LAYERX_RESOURCE_URL",
    "LAYERX_RESOURCE_DESCRIPTION",
    "LAYERX_RESOURCE_MIME_TYPE",
    "LAYERX_RESOURCE_SERVICE_NAME",
    "LAYERX_X402_SCHEME",
    "LAYERX_X402_NETWORK",
    "LAYERX_PRICE",
    "LAYERX_ASSET",
    "LAYERX_PAY_TO",
    "LAYERX_PAYER",
    "LAYERX_COMMITMENT",
    "LAYERX_PURPOSE_HASH",
    "LAYERX_WINDOW_SECONDS",
    "LAYERX_PAYMENT_TIMEOUT_SECONDS",
    "LAYERX_AUTHORIZED_BATCH_JSON",
    "LAYERX_WEBHOOK_PATH",
    "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON",
    "LAYERX_WEBHOOK_MAX_AGE_MS",
    "LAYERX_WEBHOOK_LEASE_MS",
    "LAYERX_HUMAN_URL",
    "LAYERX_SOURCE",
    "LAYERX_TOKEN",
)

SECRET_KEYS = ("LAYERX_TOKEN",)
PUBLISHED_PREFIXES = ("NEXT_PUBLIC_", "PUBLIC_", "VITE_", "REACT_APP_", "EXPO_PUBLIC_")


class IntegrationErrorCode(StrEnum):
    MISSING_DECLARED_KEY = "missing-declared-key"
    INVALID_DECLARED_KEY = "invalid-declared-key"
    CLIENT_RUNTIME_REFUSED = "client-runtime-refused"
    DUPLICATE_HEADER = "duplicate-header"
    UNVERIFIABLE_BODY = "unverifiable-body"
    RECEIPT_NOT_BACKED = "receipt-not-backed"


class IntegrationError(Exception):
    __slots__ = ("code",)

    def __init__(self, code: IntegrationErrorCode) -> None:
        super().__init__(code.value)
        self.code = code


@dataclass(frozen=True)
class WebhookSettings:
    path: str
    public_keys: Mapping[str, bytes]
    maximum_age_ms: int
    lease_ms: int


@dataclass(frozen=True)
class BuyerSettings:
    human_url: str
    source: str
    scheme: str
    network: str


@dataclass(frozen=True)
class DeclaredConfig:
    principal: str
    protected_path: str
    payment_required: PaymentRequired
    requirements: PaymentRequirements
    authorized_batch: AuthorizedReceiptBatch
    webhook: WebhookSettings
    buyer: BuyerSettings | None = None


def assert_no_published_secrets(environment: Mapping[str, str]) -> None:
    for name, value in environment.items():
        if len(value) == 0 or not _is_published_name(name):
            continue
        for secret in SECRET_KEYS:
            declared = environment.get(secret)
            if declared is not None and len(declared) > 0 and declared == value:
                raise IntegrationError(IntegrationErrorCode.CLIENT_RUNTIME_REFUSED)
        if _looks_like_key_material(name):
            raise IntegrationError(IntegrationErrorCode.CLIENT_RUNTIME_REFUSED)


def read_declared_config(environment: Mapping[str, str] | None = None) -> DeclaredConfig:
    values = dict(environ if environment is None else environment)
    assert_no_published_secrets(values)
    scheme = _required(values, "LAYERX_X402_SCHEME")
    network = _required(values, "LAYERX_X402_NETWORK")
    commitment = _optional(values, "LAYERX_COMMITMENT") or "executed"
    if commitment not in ("executed", "batched", "finalised"):
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    layerx: dict[str, JsonValue] = {
        "commitment": commitment,
        "payer": _hex32(_required(values, "LAYERX_PAYER")).hex(),
    }
    if scheme != "exact":
        layerx["purposeHash"] = _hex32(_required(values, "LAYERX_PURPOSE_HASH")).hex()
        if scheme == "subscription":
            window = _required(values, "LAYERX_WINDOW_SECONDS")
            if fullmatch(r"[1-9][0-9]{0,19}", window) is None or int(window) >= 1 << 64:
                raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
            layerx["windowSeconds"] = window
        elif _optional(values, "LAYERX_WINDOW_SECONDS") is not None:
            raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    requirements = PaymentRequirements(
        scheme=scheme,
        network=network,
        amount=_required(values, "LAYERX_PRICE"),
        asset=_required(values, "LAYERX_ASSET"),
        pay_to=_required(values, "LAYERX_PAY_TO"),
        max_timeout_seconds=_positive_integer(_required(values, "LAYERX_PAYMENT_TIMEOUT_SECONDS")),
        extra={"layerx": layerx},
        has_extra=True,
    )
    payment_required = PaymentRequired(
        resource=ResourceInfo(
            url=_required(values, "LAYERX_RESOURCE_URL"),
            description=_optional(values, "LAYERX_RESOURCE_DESCRIPTION"),
            mime_type=_optional(values, "LAYERX_RESOURCE_MIME_TYPE"),
            service_name=_optional(values, "LAYERX_RESOURCE_SERVICE_NAME"),
        ),
        accepts=(requirements,),
    )
    human_url = _optional(values, "LAYERX_HUMAN_URL")
    source = _optional(values, "LAYERX_SOURCE")
    buyer = (
        None
        if human_url is None or source is None
        else BuyerSettings(human_url=human_url, source=source, scheme=scheme, network=network)
    )
    return DeclaredConfig(
        principal=_required(values, "LAYERX_PRINCIPAL"),
        protected_path=_mount_path(_required(values, "LAYERX_PROTECTED_PATH")),
        payment_required=payment_required,
        requirements=requirements,
        authorized_batch=parse_authorized_batch(_required(values, "LAYERX_AUTHORIZED_BATCH_JSON")),
        webhook=WebhookSettings(
            path=_mount_path(_required(values, "LAYERX_WEBHOOK_PATH")),
            public_keys=parse_webhook_keys(_required(values, "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON")),
            maximum_age_ms=_positive_integer(_optional(values, "LAYERX_WEBHOOK_MAX_AGE_MS") or "300000"),
            lease_ms=_positive_integer(_optional(values, "LAYERX_WEBHOOK_LEASE_MS") or "60000"),
        ),
        buyer=buyer,
    )


def parse_authorized_batch(value: str) -> AuthorizedReceiptBatch:
    parsed = _json_object(value)
    return AuthorizedReceiptBatch(
        batch_id=_hex32(_required_text(parsed.get("batchId"))),
        asset=_hex32(_required_text(parsed.get("asset"))),
        previous_state_root=_hex32(_required_text(parsed.get("previousStateRoot"))),
        resulting_state_root=_hex32(_required_text(parsed.get("resultingStateRoot"))),
        sequencer_public_key=_hex32(_required_text(parsed.get("sequencerPublicKey"))),
    )


def parse_webhook_keys(value: str) -> dict[str, bytes]:
    parsed = _json_object(value)
    if not 0 < len(parsed) <= 32:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    keys: dict[str, bytes] = {}
    for key_id, encoded in parsed.items():
        if fullmatch(r"[A-Za-z0-9._-]{1,64}", key_id) is None:
            raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
        key = _decode_base64(_required_text(encoded))
        if len(key) != 32:
            raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
        keys[key_id] = key
    return keys


def _required(environment: Mapping[str, str], key: str) -> str:
    value = environment.get(key)
    if value is None or len(value) == 0:
        raise IntegrationError(IntegrationErrorCode.MISSING_DECLARED_KEY)
    return value


def _optional(environment: Mapping[str, str], key: str) -> str | None:
    value = environment.get(key)
    return None if value is None or len(value) == 0 else value


def _positive_integer(value: str) -> int:
    if fullmatch(r"[1-9][0-9]*", value) is None:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    parsed = int(value)
    if parsed > 0xFFFF_FFFF_FFFF:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    return parsed


def _mount_path(value: str) -> str:
    if not value.startswith("/") or len(value) > 512 or search(r"[\s?#]", value) is not None:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    return value


def _json_object(value: str) -> Mapping[str, JsonValue]:
    try:
        parsed = json.loads(value)
    except json.JSONDecodeError as error:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY) from error
    if not isinstance(parsed, dict):
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    return parsed


def _required_text(value: JsonValue | None) -> str:
    if not isinstance(value, str) or len(value) == 0:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    return value


def _hex32(value: str) -> bytes:
    digits = value[2:] if value.startswith("0x") else value
    if fullmatch(r"[0-9a-fA-F]{64}", digits) is None:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    return bytes.fromhex(digits)


def _decode_base64(value: str) -> bytes:
    if len(value) == 0 or fullmatch(r"[A-Za-z0-9+/]*={0,2}", value) is None:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY)
    try:
        return b64decode(value, validate=True)
    except (BinasciiError, ValueError) as error:
        raise IntegrationError(IntegrationErrorCode.INVALID_DECLARED_KEY) from error


def _is_published_name(name: str) -> bool:
    return any(name.startswith(prefix) for prefix in PUBLISHED_PREFIXES)


def _looks_like_key_material(name: str) -> bool:
    return search(r"(^|_)(TOKEN|SECRET|PRIVATE|CREDENTIAL|PASSWORD|SIGNING_KEY|API_KEY)(_|$)", name) is not None
