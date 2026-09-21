"""Unified-network read helpers over one network-gateway JSON-RPC endpoint."""

from __future__ import annotations

import json
import re
import urllib.parse
import urllib.request
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from types import MappingProxyType
from typing import Literal

from .agent_http import LayerXKeyCredential

_MAX_RESPONSE_BYTES = 9 * 1024 * 1024
_MAX_REQUEST_BYTES = 1024 * 1024
_MAX_U128 = 340282366920938463463374607431768211455
_MAX_U64 = 18446744073709551615
_EVM_ADDRESS = re.compile(r"^0x[0-9a-f]{40}$")
_HEX32 = re.compile(r"^[0-9a-f]{64}$")
_DID = re.compile(r"^did:layerx:[0-9a-f]{64}$")
_DECIMAL = re.compile(r"^(?:0|[1-9][0-9]*)$")
_HEX_QUANTITY = re.compile(r"^0x[0-9a-f]{1,32}$")

PX_MAXIMUM_JOINED_ASSETS = 1024

PxMethod = Literal[
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
]
PX_METHODS: tuple[PxMethod, ...] = (
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
)


@dataclass(frozen=True)
class PxResolvedIdentities:
    evm_address: str | None
    pax_address: str | None
    layerx_did: str | None
    layerx_account: str | None
    bound: bool


@dataclass(frozen=True)
class PxPaxeerAccount:
    address: str
    balance: int
    nonce: int


@dataclass(frozen=True)
class PxAccountJoin:
    account: PxResolvedIdentities
    paxeer: PxPaxeerAccount | None
    layerx: Mapping[str, object] | None


@dataclass(frozen=True)
class PxCustodyAsset:
    asset_id: str
    denom: str
    pointer: str
    enabled: bool
    paused: bool
    minimum_deposit: int
    custody_cap: int
    custodied: int
    released: int
    pending: int


@dataclass(frozen=True)
class PxPaxeerAssetBalance:
    denom: str
    amount: int


@dataclass(frozen=True)
class PxAssetBalance:
    asset_id: str
    denom: str | None
    custody: PxCustodyAsset | None
    paxeer: PxPaxeerAssetBalance | None
    layerx: Mapping[str, object] | None


@dataclass(frozen=True)
class PxAccountBalances:
    account: PxResolvedIdentities
    balances: tuple[PxAssetBalance, ...]
    joined_limit: int


@dataclass(frozen=True)
class PxAssetEntry:
    asset_id: str
    layerx: Mapping[str, object] | None
    paxeer: PxCustodyAsset | None


@dataclass(frozen=True)
class PxAssetTable:
    assets: tuple[PxAssetEntry, ...]
    joined_limit: int


@dataclass(frozen=True)
class PxAnchorHead:
    latest_finalized_batch: int | None
    status: int | None
    status_name: str | None
    status_ladder: Mapping[str, str] | None


@dataclass(frozen=True)
class PxPaxeerHead:
    chain_id: int
    latest_block: int


@dataclass(frozen=True)
class PxNetworkHead:
    network_id: str
    paxeer: PxPaxeerHead
    layerx: Mapping[str, object] | None
    anchor: PxAnchorHead | None


class PxRpcError(Exception):
    __slots__ = ("code", "data", "message")

    def __init__(self, code: int, message: str, data: object = None) -> None:
        super().__init__(f"JSON-RPC error {code}")
        self.code = code
        self.message = message
        self.data = data


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("px-redirect")


class PxClient:
    def __init__(
        self,
        endpoint: str,
        *,
        credential: LayerXKeyCredential | None = None,
        timeout: float = 30.0,
        maximum_response_bytes: int = _MAX_RESPONSE_BYTES,
    ) -> None:
        url = urllib.parse.urlsplit(endpoint)
        if (
            url.username
            or url.password
            or url.fragment
            or url.query
            or not url.hostname
            or url.scheme != "https"
            and not (
                url.scheme == "http"
                and (
                    url.hostname in ("localhost", "::1")
                    or re.fullmatch(r"127(?:\.[0-9]{1,3}){3}", url.hostname) is not None
                )
            )
        ):
            raise ValueError("invalid-px-endpoint")
        if not isinstance(timeout, (int, float)) or isinstance(timeout, bool) or timeout <= 0:
            raise ValueError("invalid-px-timeout")
        if type(maximum_response_bytes) is not int or not 0 < maximum_response_bytes <= _MAX_RESPONSE_BYTES:
            raise ValueError("invalid-px-response-bound")
        self.endpoint = endpoint
        self._credential = credential
        self._timeout = float(timeout)
        self._maximum_response_bytes = maximum_response_bytes
        self._id = 0
        self._opener = urllib.request.build_opener(_NoRedirect())

    def resolve_account(self, account: str) -> PxResolvedIdentities:
        return decode_px_resolved_identities(self.call("px_resolveAccount", [px_account_key(account)]))

    def get_account(self, account: str) -> PxAccountJoin:
        return decode_px_account_join(self.call("px_getAccount", [px_account_key(account)]))

    def get_balances(self, account: str) -> PxAccountBalances:
        return decode_px_account_balances(self.call("px_getBalances", [px_account_key(account)]))

    def list_assets(self) -> PxAssetTable:
        return decode_px_asset_table(self.call("px_listAssets", []))

    def get_network(self) -> PxNetworkHead:
        return decode_px_network_head(self.call("px_getNetwork", []))

    def call(self, method: str, params: list[object]) -> Mapping[str, object]:
        if method not in PX_METHODS or not isinstance(params, list):
            raise ValueError("invalid-px-method")
        self._id += 1
        request_id = self._id
        body = json.dumps(
            {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params},
            ensure_ascii=True,
            allow_nan=False,
            separators=(",", ":"),
        ).encode("utf-8")
        if len(body) > _MAX_REQUEST_BYTES:
            raise ValueError("px-request-too-large")
        headers = {
            "Accept": "application/json",
            "Content-Type": "application/json",
            "Content-Length": str(len(body)),
            "User-Agent": "layerx-python/0.1.0",
        }
        if self._credential is not None:
            headers["Authorization"] = self._credential.use()
        request = urllib.request.Request(self.endpoint, method="POST", headers=headers, data=body)
        with self._opener.open(request, timeout=self._timeout) as response:
            if response.status != 200 or response.headers.get_content_type() != "application/json":
                raise ValueError("invalid-px-http-answer")
            raw = response.read(self._maximum_response_bytes + 1)
        if len(raw) > self._maximum_response_bytes:
            raise ValueError("px-body-too-large")
        answer = json.loads(raw.decode("utf-8"))
        if (
            not isinstance(answer, dict)
            or answer.get("jsonrpc") != "2.0"
            or type(answer.get("id")) is not int
            or answer["id"] != request_id
            or ("error" in answer) == ("result" in answer)
        ):
            raise ValueError("invalid-px-envelope")
        if "error" in answer:
            error = answer["error"]
            if (
                not isinstance(error, dict)
                or type(error.get("code")) is not int
                or not isinstance(error.get("message"), str)
            ):
                raise ValueError("invalid-px-error")
            raise PxRpcError(error["code"], error["message"], error.get("data"))
        result = answer["result"]
        if not isinstance(result, dict) or any(not isinstance(key, str) for key in result):
            raise ValueError("invalid-px-result")
        return result


def px_account_key(account: str) -> str:
    if not isinstance(account, str):
        raise ValueError("invalid-px-account")
    lowered = account.strip().lower()
    if (
        _EVM_ADDRESS.fullmatch(lowered) is None
        and _DID.fullmatch(lowered) is None
        and _HEX32.fullmatch(lowered) is None
    ):
        raise ValueError("invalid-px-account")
    return lowered


def px_quantity(value: object) -> int:
    if type(value) is int:
        if value < 0 or value > _MAX_U128:
            raise ValueError("invalid-px-quantity")
        return value
    if not isinstance(value, str):
        raise ValueError("invalid-px-quantity")
    text = value.strip().lower()
    if _HEX_QUANTITY.fullmatch(text) is not None:
        parsed = int(text, 16)
    elif _DECIMAL.fullmatch(text) is not None:
        parsed = int(text, 10)
    else:
        raise ValueError("invalid-px-quantity")
    if parsed > _MAX_U128:
        raise ValueError("invalid-px-quantity")
    return parsed


def px_optional_quantity(value: object) -> int | None:
    return None if value is None else px_quantity(value)


def _px_counted(value: object) -> int:
    parsed = px_quantity(value)
    if parsed > _MAX_U64:
        raise ValueError("invalid-px-count")
    return parsed


def _px_optional_counted(value: object) -> int | None:
    return None if value is None else _px_counted(value)


def decode_px_resolved_identities(value: object) -> PxResolvedIdentities:
    document = _document(value)
    _present(document, ("evm_address", "pax_address", "layerx_did", "layerx_account", "bound"))
    bound = document["bound"]
    if type(bound) is not bool:
        raise ValueError("invalid-px-identities")
    return PxResolvedIdentities(
        _optional_pattern(document["evm_address"], _EVM_ADDRESS),
        _optional_text(document["pax_address"], 128),
        _optional_pattern(document["layerx_did"], _DID),
        _optional_pattern(document["layerx_account"], _HEX32),
        bound,
    )


def decode_px_account_join(value: object) -> PxAccountJoin:
    document = _document(value)
    _present(document, ("account", "paxeer", "layerx"))
    paxeer = document["paxeer"]
    return PxAccountJoin(
        decode_px_resolved_identities(document["account"]),
        None if paxeer is None else _decode_paxeer_account(paxeer),
        _document_or_none(document["layerx"]),
    )


def decode_px_account_balances(value: object) -> PxAccountBalances:
    document = _document(value)
    _present(document, ("account", "balances", "joined_limit"))
    rows = document["balances"]
    if not isinstance(rows, list) or len(rows) > PX_MAXIMUM_JOINED_ASSETS:
        raise ValueError("invalid-px-balances")
    return PxAccountBalances(
        decode_px_resolved_identities(document["account"]),
        tuple(decode_px_asset_balance(row) for row in rows),
        _px_counted(document["joined_limit"]),
    )


def decode_px_asset_balance(value: object) -> PxAssetBalance:
    row = _document(value)
    _present(row, ("asset_id", "denom", "custody", "paxeer", "layerx"))
    paxeer = row["paxeer"]
    balance: PxPaxeerAssetBalance | None = None
    if paxeer is not None:
        half = _document(paxeer)
        _present(half, ("denom", "amount"))
        balance = PxPaxeerAssetBalance(_text(half["denom"], 128), px_quantity(half["amount"]))
    return PxAssetBalance(
        _pattern(row["asset_id"], _HEX32),
        _optional_text(row["denom"], 128),
        decode_px_custody_asset(row["custody"]),
        balance,
        _document_or_none(row["layerx"]),
    )


def decode_px_custody_asset(value: object) -> PxCustodyAsset | None:
    if value is None:
        return None
    custody = _document(value)
    _present(custody, (
        "asset_id", "denom", "pointer", "enabled", "paused",
        "minimum_deposit", "custody_cap", "custodied", "released", "pending",
    ))
    enabled = custody["enabled"]
    paused = custody["paused"]
    if type(enabled) is not bool or type(paused) is not bool:
        raise ValueError("invalid-px-custody-asset")
    return PxCustodyAsset(
        _pattern(custody["asset_id"], _HEX32),
        _text(custody["denom"], 128),
        _pattern(custody["pointer"], _EVM_ADDRESS),
        enabled,
        paused,
        px_quantity(custody["minimum_deposit"]),
        px_quantity(custody["custody_cap"]),
        px_quantity(custody["custodied"]),
        px_quantity(custody["released"]),
        px_quantity(custody["pending"]),
    )


def decode_px_asset_table(value: object) -> PxAssetTable:
    document = _document(value)
    _present(document, ("assets", "joined_limit"))
    rows = document["assets"]
    if not isinstance(rows, list) or len(rows) > PX_MAXIMUM_JOINED_ASSETS:
        raise ValueError("invalid-px-asset-map")
    assets: list[PxAssetEntry] = []
    for row in rows:
        entry = _document(row)
        _present(entry, ("asset_id", "layerx", "paxeer"))
        assets.append(PxAssetEntry(
            _pattern(entry["asset_id"], _HEX32),
            _document_or_none(entry["layerx"]),
            decode_px_custody_asset(entry["paxeer"]),
        ))
    return PxAssetTable(tuple(assets), _px_counted(document["joined_limit"]))


def decode_px_network_head(value: object) -> PxNetworkHead:
    document = _document(value)
    _present(document, ("network_id", "paxeer", "layerx", "anchor"))
    paxeer = _document(document["paxeer"])
    _present(paxeer, ("chain_id", "latest_block"))
    anchor = document["anchor"]
    return PxNetworkHead(
        _text(document["network_id"], 128),
        PxPaxeerHead(_px_counted(paxeer["chain_id"]), _px_counted(paxeer["latest_block"])),
        _document_or_none(document["layerx"]),
        None if anchor is None else decode_px_anchor_head(anchor),
    )


def decode_px_anchor_head(value: object) -> PxAnchorHead:
    anchor = _document(value)
    _present(anchor, ("latest_finalized_batch", "status", "status_name", "status_ladder"))
    ladder = anchor["status_ladder"]
    status_ladder: Mapping[str, str] | None = None
    if ladder is not None:
        rungs = _document(ladder)
        for rung in rungs.values():
            if not isinstance(rung, str) or not 0 < len(rung) <= 64:
                raise ValueError("invalid-px-anchor-ladder")
        status_ladder = MappingProxyType({key: str(rung) for key, rung in rungs.items()})
    return PxAnchorHead(
        _px_optional_counted(anchor["latest_finalized_batch"]),
        _px_optional_counted(anchor["status"]),
        _optional_text(anchor["status_name"], 64),
        status_ladder,
    )


def _decode_paxeer_account(value: object) -> PxPaxeerAccount:
    document = _document(value)
    _present(document, ("address", "balance", "nonce"))
    return PxPaxeerAccount(
        _pattern(document["address"], _EVM_ADDRESS),
        px_quantity(document["balance"]),
        _px_counted(document["nonce"]),
    )


def _document(value: object) -> Mapping[str, object]:
    if not isinstance(value, Mapping) or isinstance(value, (str, bytes)) or any(not isinstance(key, str) for key in value):
        raise ValueError("invalid-px-document")
    return value


def _document_or_none(value: object) -> Mapping[str, object] | None:
    return None if value is None else MappingProxyType(dict(_document(value)))


def _present(value: Mapping[str, object], required: Sequence[str]) -> None:
    if any(key not in value for key in required):
        raise ValueError("incomplete-px-document")


def _pattern(value: object, expected: re.Pattern[str]) -> str:
    if not isinstance(value, str) or expected.fullmatch(value.lower()) is None:
        raise ValueError("invalid-px-field")
    return value.lower()


def _optional_pattern(value: object, expected: re.Pattern[str]) -> str | None:
    return None if value is None or value == "" else _pattern(value, expected)


def _text(value: object, maximum: int) -> str:
    if not isinstance(value, str) or not 0 < len(value) <= maximum:
        raise ValueError("invalid-px-text")
    return value


def _optional_text(value: object, maximum: int) -> str | None:
    return None if value is None else _text(value, maximum)
