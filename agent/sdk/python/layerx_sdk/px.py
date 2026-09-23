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

_MAX_U256 = (1 << 256) - 1
_ROW_ID = re.compile(r"^[1-9][0-9]*$")
_CURSOR = re.compile(r"^[0-9A-Za-z]{1,32}$")
_KIND = re.compile(r"^[a-z0-9_]{1,64}$")

PX_MAXIMUM_JOINED_ASSETS = 1024
HISTORY_DEFAULT_LIMIT = 50
HISTORY_MAXIMUM_LIMIT = 100
HISTORY_MAXIMUM_ACCOUNTS = 16

PxMethod = Literal[
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
    "px_getHistory", "px_getUnifiedHistory",
]
PX_METHODS: tuple[PxMethod, ...] = (
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
    "px_getHistory", "px_getUnifiedHistory",
)
_GATEWAY_METHODS = (*PX_METHODS, "lx_getHistory")


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


@dataclass(frozen=True)
class HistoryAssetMetadata:
    asset: str
    chain: str
    kind: str
    address: str | None
    denom: str | None
    symbol: str | None
    decimals: int | None
    native_id: str | None
    pointer: str | None
    metadata: object


@dataclass(frozen=True)
class HistoryRow:
    id: int
    height_or_seq: int
    chain: str
    kind: str
    direction: str
    account: str
    counterparty: str | None
    asset: str
    amount: int
    tx_id: str
    ordinal: int
    final: bool
    decoded: object
    asset_metadata: HistoryAssetMetadata | None
    side: str | None


@dataclass(frozen=True)
class HistoryPage:
    account: str
    items: tuple[HistoryRow, ...]
    next_cursor: str | None


@dataclass(frozen=True)
class HistorySide:
    side: str
    account: str


@dataclass(frozen=True)
class UnifiedHistoryPage:
    account: Mapping[str, object]
    accounts: tuple[HistorySide, ...]
    items: tuple[HistoryRow, ...]
    next_cursor: str | None


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

    def get_history(
        self, account: str, *, cursor: str | None = None, limit: int | None = None, kind: str | None = None,
    ) -> HistoryPage:
        params = history_params(paxeer_history_account(account), cursor=cursor, limit=limit, kind=kind)
        return decode_paxeer_history_page(self.call("px_getHistory", params), limit or HISTORY_DEFAULT_LIMIT)

    def get_unified_history(
        self, account: str, *, cursor: str | None = None, limit: int | None = None, kind: str | None = None,
    ) -> UnifiedHistoryPage:
        params = history_params(px_account_key(account), cursor=cursor, limit=limit, kind=kind)
        return decode_unified_history_page(self.call("px_getUnifiedHistory", params), limit or HISTORY_DEFAULT_LIMIT)

    def get_layerx_history(
        self, account: str, *, cursor: str | None = None, limit: int | None = None, kind: str | None = None,
    ) -> HistoryPage:
        params = history_params(layerx_history_account(account), cursor=cursor, limit=limit, kind=kind)
        return decode_layerx_history_page(self.call("lx_getHistory", params), limit or HISTORY_DEFAULT_LIMIT)

    def call(self, method: str, params: list[object]) -> Mapping[str, object]:
        if method not in _GATEWAY_METHODS or not isinstance(params, list):
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


def history_params(
    account: str, *, cursor: str | None = None, limit: int | None = None, kind: str | None = None,
) -> list[object]:
    if cursor is not None and (not isinstance(cursor, str) or _CURSOR.fullmatch(cursor) is None):
        raise ValueError("invalid-history-cursor")
    if limit is not None and (type(limit) is not int or not 1 <= limit <= HISTORY_MAXIMUM_LIMIT):
        raise ValueError("invalid-history-limit")
    if kind is not None and (not isinstance(kind, str) or _KIND.fullmatch(kind) is None):
        raise ValueError("invalid-history-kind")
    return [account, cursor, limit, kind]


def layerx_history_account(account: str) -> str:
    lowered = account.strip().lower() if isinstance(account, str) else ""
    if _HEX32.fullmatch(lowered) is None or lowered == "00" * 32:
        raise ValueError("invalid-layerx-account")
    return lowered


def paxeer_history_account(account: str) -> str:
    lowered = account.strip().lower() if isinstance(account, str) else ""
    if _EVM_ADDRESS.fullmatch(lowered) is None:
        raise ValueError("invalid-paxeer-address")
    return lowered


def decode_layerx_history_page(value: object, limit: int = HISTORY_MAXIMUM_LIMIT) -> HistoryPage:
    return _decode_history_page(value, _HEX32, limit)


def decode_paxeer_history_page(value: object, limit: int = HISTORY_MAXIMUM_LIMIT) -> HistoryPage:
    return _decode_history_page(value, _EVM_ADDRESS, limit)


def decode_unified_history_page(value: object, limit: int = HISTORY_MAXIMUM_LIMIT) -> UnifiedHistoryPage:
    document = _document(value)
    _present(document, ("account", "accounts", "items", "next_cursor"))
    sides = document["accounts"]
    if not isinstance(sides, list) or len(sides) > HISTORY_MAXIMUM_ACCOUNTS:
        raise ValueError("invalid-history-accounts")
    accounts: list[HistorySide] = []
    for entry in sides:
        side = _document(entry)
        _present(side, ("side", "account"))
        name = _history_chain(side["side"])
        account = side["account"]
        expected = _HEX32 if name == "layerx" else _EVM_ADDRESS
        if not isinstance(account, str) or expected.fullmatch(account) is None:
            raise ValueError("invalid-history-account")
        accounts.append(HistorySide(name, account))
    items = _history_rows(document["items"], True, limit)
    for newer, older in zip(items, items[1:]):
        if newer.id <= older.id:
            raise ValueError("unordered-unified-history")
    return UnifiedHistoryPage(
        MappingProxyType(dict(_document(document["account"]))),
        tuple(accounts),
        items,
        _history_cursor(document["next_cursor"], items),
    )


def decode_history_row(value: object, unified: bool = False) -> HistoryRow:
    row = _document(value)
    _present(row, (
        "id", "height_or_seq", "chain", "kind", "direction", "account", "counterparty",
        "asset", "amount", "tx_id", "ordinal", "final", "decoded", "asset_metadata",
    ))
    direction = row["direction"]
    if direction not in ("in", "out"):
        raise ValueError("invalid-history-direction")
    final = row["final"]
    if type(final) is not bool:
        raise ValueError("invalid-history-finality")
    kind = row["kind"]
    if not isinstance(kind, str) or _KIND.fullmatch(kind) is None:
        raise ValueError("invalid-history-kind")
    if not isinstance(row["id"], str) or _ROW_ID.fullmatch(row["id"]) is None:
        raise ValueError("invalid-history-row-id")
    if unified:
        _present(row, ("side",))
    asset = _text(row["asset"], 256)
    counterparty = row["counterparty"]
    return HistoryRow(
        _history_decimal(row["id"], _MAX_U64),
        _history_decimal(row["height_or_seq"], _MAX_U64),
        _history_chain(row["chain"]),
        kind,
        str(direction),
        _text(row["account"], 256),
        None if counterparty is None else _text(counterparty, 256),
        asset,
        _history_decimal(row["amount"], _MAX_U256),
        _text(row["tx_id"], 256),
        _history_decimal(row["ordinal"], _MAX_U64),
        final,
        row["decoded"],
        decode_history_asset_metadata(row["asset_metadata"], asset),
        _history_chain(row["side"]) if unified else None,
    )


def decode_history_asset_metadata(value: object, asset: str | None = None) -> HistoryAssetMetadata | None:
    if value is None:
        return None
    document = _document(value)
    _present(document, (
        "asset", "chain", "kind", "address", "denom", "symbol", "decimals", "native_id", "pointer", "metadata",
    ))
    label = _text(document["asset"], 256)
    if asset is not None and label != asset:
        raise ValueError("mismatched-history-asset")
    decimals = document["decimals"]
    if decimals is not None and (type(decimals) is not int or not 0 <= decimals <= 255):
        raise ValueError("invalid-history-decimals")
    return HistoryAssetMetadata(
        label,
        _history_chain(document["chain"]),
        _text(document["kind"], 64),
        _exact_optional(document["address"], _EVM_ADDRESS),
        _optional_text(document["denom"], 128),
        _optional_text(document["symbol"], 64),
        decimals,
        _exact_optional(document["native_id"], _HEX32),
        _exact_optional(document["pointer"], _EVM_ADDRESS),
        document["metadata"],
    )


def _decode_history_page(value: object, expected: re.Pattern[str], limit: int) -> HistoryPage:
    document = _document(value)
    _present(document, ("account", "items", "next_cursor"))
    account = document["account"]
    if not isinstance(account, str) or expected.fullmatch(account) is None:
        raise ValueError("invalid-history-account")
    items = _history_rows(document["items"], False, limit)
    return HistoryPage(account, items, _history_cursor(document["next_cursor"], items))


def _history_rows(value: object, unified: bool, limit: int) -> tuple[HistoryRow, ...]:
    if not isinstance(value, list) or len(value) > limit:
        raise ValueError("invalid-history-page")
    return tuple(decode_history_row(row, unified) for row in value)


def _history_cursor(value: object, items: tuple[HistoryRow, ...]) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or _CURSOR.fullmatch(value) is None or not items:
        raise ValueError("invalid-history-cursor")
    return value


def _history_chain(value: object) -> str:
    if value not in ("layerx", "paxeer"):
        raise ValueError("invalid-history-chain")
    return str(value)


def _history_decimal(value: object, maximum: int) -> int:
    if not isinstance(value, str) or _DECIMAL.fullmatch(value) is None:
        raise ValueError("invalid-history-decimal")
    parsed = int(value, 10)
    if parsed > maximum:
        raise ValueError("invalid-history-decimal")
    return parsed


def _exact_optional(value: object, expected: re.Pattern[str]) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or expected.fullmatch(value) is None:
        raise ValueError("invalid-history-field")
    return value


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
