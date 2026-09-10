from __future__ import annotations

import hashlib
from typing import Mapping

_CORE = (
    ("from", "hex", 32),
    ("to", "hex", 32),
    ("asset", "hex", 32),
    ("amount", "integer", 16),
    ("grant_id", "hex", 32),
    ("receiver_sequence", "integer", 8),
    ("idempotency_key", "hex", 32),
    ("context_hash", "hex", 32),
)
_AUTH = (
    ("kind", "number", 1),
    ("controller", "hex", 32),
    ("public_key", "hex", 32),
    ("signature", "hex", 64),
    ("signed_context_hash", "hex", 32),
    ("network_id", "number", 4),
    ("protocol_version", "number", 2),
)
_GRANT = (
    ("grant_id", "hex", 32),
    ("from", "hex", 32),
    ("recipient", "hex", 32),
    ("asset", "hex", 32),
    ("per_draw_maximum", "integer", 16),
    ("allowance", "integer", 16),
    ("recurring", "boolean", 1),
    ("window_length", "integer", 8),
    ("expiration", "integer", 8),
    ("purpose_hash", "hex", 32),
    ("has_reference", "boolean", 1),
    ("reference_hash", "hex", 32),
    ("revocation_sequence", "integer", 8),
    ("public_key", "hex", 32),
    ("signature", "hex", 64),
)


def _record(value: object) -> Mapping[str, object]:
    if not isinstance(value, Mapping) or any(not isinstance(key, str) for key in value):
        raise ValueError("invalid-receive")
    return value


def _exact(value: object, keys: tuple[str, ...]) -> Mapping[str, object]:
    result = _record(value)
    if set(result) != set(keys):
        raise ValueError("invalid-receive")
    return result


def _encode(value: object, fields: tuple[tuple[str, str, int], ...]) -> bytes:
    source = _record(value)
    output = bytearray()
    for key, kind, size in fields:
        field = source.get(key)
        if kind == "hex":
            if (
                not isinstance(field, str)
                or len(field) != size * 2
                or any(c not in "0123456789abcdef" for c in field)
            ):
                raise ValueError("invalid-receive")
            output.extend(bytes.fromhex(field))
            continue
        if kind == "integer":
            if (
                not isinstance(field, str)
                or not 0 < len(field) <= 39
                or any(c not in "0123456789" for c in field)
                or len(field) > 1
                and field[0] == "0"
            ):
                raise ValueError("invalid-receive")
            number = int(field)
        elif kind == "number":
            if type(field) is not int:
                raise ValueError("invalid-receive")
            number = field
        else:
            if type(field) is not bool:
                raise ValueError("invalid-receive")
            number = int(field)
        if not 0 <= number < 1 << (size * 8):
            raise ValueError("invalid-receive")
        output.extend(number.to_bytes(size, "big"))
    return bytes(output)


def encode_grant(grant: Mapping[str, object]) -> bytes:
    _exact(grant, tuple(key for key, _, _ in _GRANT))
    return _encode(grant, _GRANT)


def grant_authorization_message(grant: Mapping[str, object]) -> bytes:
    encode_grant(grant)
    return b"LXP:GRANT:v1" + _encode(
        grant,
        tuple(field for field in _GRANT if field[0] not in {"grant_id", "signature"}),
    )


def encode_receive(receive: Mapping[str, object]) -> bytes:
    _exact(
        receive,
        tuple(key for key, _, _ in _CORE) + ("receiver_authorization", "payer_grant"),
    )
    auth = _exact(receive["receiver_authorization"], tuple(key for key, _, _ in _AUTH))
    grant = _record(receive["payer_grant"])
    return (
        b"\x52\x01\x00\x0a"
        + _encode(receive, _CORE)
        + _encode(auth, _AUTH)
        + encode_grant(grant)
    )


def receive_authorization_message(receive: Mapping[str, object]) -> bytes:
    encode_receive(receive)
    return (
        b"LXP:RECEIVE:v1"
        + _encode(receive, _CORE)
        + _encode(
            receive["receiver_authorization"],
            tuple(
                field for field in _AUTH if field[0] not in {"public_key", "signature"}
            ),
        )
    )


def decode_receive(value: bytes) -> dict[str, object]:
    if (
        type(value) is not bytes
        or len(value) != 733
        or value[:4] != b"\x52\x01\x00\x0a"
    ):
        raise ValueError("invalid-receive")
    offset = 4

    def decode(fields: tuple[tuple[str, str, int], ...]) -> dict[str, object]:
        nonlocal offset
        result: dict[str, object] = {}
        for key, kind, size in fields:
            field = value[offset : offset + size]
            offset += size
            if kind == "hex":
                result[key] = field.hex()
                continue
            number = int.from_bytes(field, "big")
            if kind == "boolean" and number > 1:
                raise ValueError("invalid-receive")
            result[key] = (
                bool(number)
                if kind == "boolean"
                else number
                if kind == "number"
                else str(number)
            )
        return result

    result = decode(_CORE)
    result["receiver_authorization"] = decode(_AUTH)
    result["payer_grant"] = decode(_GRANT)
    if offset != len(value):
        raise ValueError("invalid-receive")
    return result


def derive_native_asset_id(issuer_did_id32: str, salt: str) -> str:
    try:
        issuer = _encode({"issuer": issuer_did_id32}, (("issuer", "hex", 32),))
        salt_bytes = _encode({"salt": salt}, (("salt", "hex", 32),))
    except ValueError as error:
        raise ValueError("invalid-asset-register") from error
    return hashlib.sha256(b"LX:ASSET:v1" + issuer + salt_bytes).hexdigest()


def encode_asset_register(
    issuer_did_id32: str, registration: Mapping[str, object]
) -> bytes:
    source = _record(registration)
    required = {
        "salt",
        "symbol",
        "name",
        "decimals",
        "supply_cap",
        "issuer_kind",
        "custody_ref",
    }
    if not required <= source.keys() or source.keys() - required - {"asset_id"}:
        raise ValueError("invalid-asset-register")
    try:
        salt = _encode(source, (("salt", "hex", 32),))
        symbol = source["symbol"].encode("ascii")
        name = source["name"].encode("utf-8")
    except (AttributeError, UnicodeError, ValueError) as error:
        raise ValueError("invalid-asset-register") from error
    decimals = source["decimals"]
    issuer_kind = source["issuer_kind"]
    custody_ref = source["custody_ref"]
    if (
        not 1 <= len(symbol) <= 16
        or not 1 <= len(name) <= 32
        or type(decimals) is not int
        or not 0 <= decimals <= 38
        or type(issuer_kind) is not int
        or issuer_kind not in (1, 2)
        or type(custody_ref) is not bytes
        or len(custody_ref) > 128
        or (issuer_kind == 1 and custody_ref)
    ):
        raise ValueError("invalid-asset-register")
    derived = derive_native_asset_id(issuer_did_id32, source["salt"])
    asset_id = source.get("asset_id", derived)
    if (issuer_kind == 1 and asset_id != derived) or (
        issuer_kind == 2 and "asset_id" not in source
    ):
        raise ValueError("invalid-asset-register")
    try:
        asset = _encode({"asset": asset_id}, (("asset", "hex", 32),))
        tail = _encode(
            source,
            (
                ("decimals", "number", 1),
                ("supply_cap", "integer", 16),
                ("issuer_kind", "number", 1),
            ),
        )
    except ValueError as error:
        raise ValueError("invalid-asset-register") from error
    return (
        b"\x00\x01"
        + asset
        + salt
        + bytes((len(symbol),))
        + symbol
        + bytes((len(name),))
        + name
        + tail
        + bytes((len(custody_ref),))
        + custody_ref
    )


def encode_account_open(asset: str) -> bytes:
    return b"\x00\x01" + _encode({"asset": asset}, (("asset", "hex", 32),))


def encode_grant_revoke(grant_id: str, revocation_sequence: str) -> bytes:
    return b"\x00\x01" + _encode(
        {"grant_id": grant_id, "revocation_sequence": revocation_sequence},
        (("grant_id", "hex", 32), ("revocation_sequence", "integer", 8)),
    )


def encode_asset_supply(asset: str, account: str, amount: str) -> bytes:
    body = _encode(
        {"asset": asset, "account": account, "amount": amount},
        (("asset", "hex", 32), ("account", "hex", 32), ("amount", "integer", 16)),
    )
    if int(amount) == 0:
        raise ValueError("invalid-asset-amount")
    return b"\x00\x01" + body
