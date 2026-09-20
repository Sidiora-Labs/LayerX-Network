from collections.abc import Mapping
from typing import Literal

from .agent_http import LayerXKeyCredential

PxMethod = Literal[
    "px_resolveAccount", "px_getAccount", "px_getBalances", "px_listAssets", "px_getNetwork",
]

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
