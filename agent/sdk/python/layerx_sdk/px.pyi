from collections.abc import Mapping
from typing import Literal

from .agent_http import LayerXKeyCredential

PxMethod = Literal[
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
    "px_getHistory", "px_getUnifiedHistory",
]

HISTORY_DEFAULT_LIMIT: int
HISTORY_MAXIMUM_LIMIT: int
HISTORY_MAXIMUM_ACCOUNTS: int

PX_MAXIMUM_JOINED_ASSETS: int
PX_METHODS: tuple[PxMethod, ...]

class PxResolvedIdentities:
    evm_address: str | None
    pax_address: str | None
    layerx_did: str | None
    layerx_account: str | None
    bound: bool
    def __init__(self, evm_address: str | None, pax_address: str | None, layerx_did: str | None, layerx_account: str | None, bound: bool) -> None: ...

class PxPaxeerAccount:
    address: str
    balance: int
    nonce: int
    def __init__(self, address: str, balance: int, nonce: int) -> None: ...

class PxAccountJoin:
    account: PxResolvedIdentities
    paxeer: PxPaxeerAccount | None
    layerx: Mapping[str, object] | None
    def __init__(self, account: PxResolvedIdentities, paxeer: PxPaxeerAccount | None, layerx: Mapping[str, object] | None) -> None: ...

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
    def __init__(self, asset_id: str, denom: str, pointer: str, enabled: bool, paused: bool, minimum_deposit: int, custody_cap: int, custodied: int, released: int, pending: int) -> None: ...

class PxPaxeerAssetBalance:
    denom: str
    amount: int
    def __init__(self, denom: str, amount: int) -> None: ...

class PxAssetBalance:
    asset_id: str
    denom: str | None
    custody: PxCustodyAsset | None
    paxeer: PxPaxeerAssetBalance | None
    layerx: Mapping[str, object] | None
    def __init__(self, asset_id: str, denom: str | None, custody: PxCustodyAsset | None, paxeer: PxPaxeerAssetBalance | None, layerx: Mapping[str, object] | None) -> None: ...

class PxAccountBalances:
    account: PxResolvedIdentities
    balances: tuple[PxAssetBalance, ...]
    joined_limit: int
    def __init__(self, account: PxResolvedIdentities, balances: tuple[PxAssetBalance, ...], joined_limit: int) -> None: ...

class PxAssetEntry:
    asset_id: str
    layerx: Mapping[str, object] | None
    paxeer: PxCustodyAsset | None
    def __init__(self, asset_id: str, layerx: Mapping[str, object] | None, paxeer: PxCustodyAsset | None) -> None: ...

class PxAssetTable:
    assets: tuple[PxAssetEntry, ...]
    joined_limit: int
    def __init__(self, assets: tuple[PxAssetEntry, ...], joined_limit: int) -> None: ...

class PxAnchorHead:
    latest_finalized_batch: int | None
    status: int | None
    status_name: str | None
    status_ladder: Mapping[str, str] | None
    def __init__(self, latest_finalized_batch: int | None, status: int | None, status_name: str | None, status_ladder: Mapping[str, str] | None) -> None: ...

class PxPaxeerHead:
    chain_id: int
    latest_block: int
    def __init__(self, chain_id: int, latest_block: int) -> None: ...

class PxNetworkHead:
    network_id: str
    paxeer: PxPaxeerHead
    layerx: Mapping[str, object] | None
    anchor: PxAnchorHead | None
    def __init__(self, network_id: str, paxeer: PxPaxeerHead, layerx: Mapping[str, object] | None, anchor: PxAnchorHead | None) -> None: ...

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
    def __init__(self, asset: str, chain: str, kind: str, address: str | None, denom: str | None, symbol: str | None, decimals: int | None, native_id: str | None, pointer: str | None, metadata: object) -> None: ...

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
    def __init__(self, id: int, height_or_seq: int, chain: str, kind: str, direction: str, account: str, counterparty: str | None, asset: str, amount: int, tx_id: str, ordinal: int, final: bool, decoded: object, asset_metadata: HistoryAssetMetadata | None, side: str | None) -> None: ...

class HistoryPage:
    account: str
    items: tuple[HistoryRow, ...]
    next_cursor: str | None
    def __init__(self, account: str, items: tuple[HistoryRow, ...], next_cursor: str | None) -> None: ...

class HistorySide:
    side: str
    account: str
    def __init__(self, side: str, account: str) -> None: ...

class UnifiedHistoryPage:
    account: Mapping[str, object]
    accounts: tuple[HistorySide, ...]
    items: tuple[HistoryRow, ...]
    next_cursor: str | None
    def __init__(self, account: Mapping[str, object], accounts: tuple[HistorySide, ...], items: tuple[HistoryRow, ...], next_cursor: str | None) -> None: ...

class PxRpcError(Exception):
    code: int
    message: str
    data: object
    def __init__(self, code: int, message: str, data: object = ...) -> None: ...

class PxClient:
    endpoint: str
    def __init__(self, endpoint: str, *, credential: LayerXKeyCredential | None = ..., timeout: float = ..., maximum_response_bytes: int = ...) -> None: ...
    def resolve_account(self, account: str) -> PxResolvedIdentities: ...
    def get_account(self, account: str) -> PxAccountJoin: ...
    def get_balances(self, account: str) -> PxAccountBalances: ...
    def list_assets(self) -> PxAssetTable: ...
    def get_network(self) -> PxNetworkHead: ...
    def get_history(self, account: str, *, cursor: str | None = ..., limit: int | None = ..., kind: str | None = ...) -> HistoryPage: ...
    def get_unified_history(self, account: str, *, cursor: str | None = ..., limit: int | None = ..., kind: str | None = ...) -> UnifiedHistoryPage: ...
    def get_layerx_history(self, account: str, *, cursor: str | None = ..., limit: int | None = ..., kind: str | None = ...) -> HistoryPage: ...
    def call(self, method: str, params: list[object]) -> Mapping[str, object]: ...

def px_account_key(account: str) -> str: ...
def px_quantity(value: object) -> int: ...
def px_optional_quantity(value: object) -> int | None: ...
def decode_px_resolved_identities(value: object) -> PxResolvedIdentities: ...
def decode_px_account_join(value: object) -> PxAccountJoin: ...
def decode_px_account_balances(value: object) -> PxAccountBalances: ...
def decode_px_asset_balance(value: object) -> PxAssetBalance: ...
def decode_px_custody_asset(value: object) -> PxCustodyAsset | None: ...
def decode_px_asset_table(value: object) -> PxAssetTable: ...
def decode_px_network_head(value: object) -> PxNetworkHead: ...
def decode_px_anchor_head(value: object) -> PxAnchorHead: ...
def history_params(account: str, *, cursor: str | None = ..., limit: int | None = ..., kind: str | None = ...) -> list[object]: ...
def layerx_history_account(account: str) -> str: ...
def paxeer_history_account(account: str) -> str: ...
def decode_layerx_history_page(value: object, limit: int = ...) -> HistoryPage: ...
def decode_paxeer_history_page(value: object, limit: int = ...) -> HistoryPage: ...
def decode_unified_history_page(value: object, limit: int = ...) -> UnifiedHistoryPage: ...
def decode_history_row(value: object, unified: bool = ...) -> HistoryRow: ...
def decode_history_asset_metadata(value: object, asset: str | None = ...) -> HistoryAssetMetadata | None: ...
