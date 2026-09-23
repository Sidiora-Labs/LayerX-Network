"""Calldata builders, wallet send helpers and event decoders for the Launchpad precompile."""

from __future__ import annotations

from collections.abc import Sequence
from typing import Literal

from .exchange import (
    AbiType,
    AbiValue,
    DecodedEvent,
    EventInput,
    EventSpec,
    PrecompileCall,
    WalletRequest,
    decode_event_from,
    encode_abi_call,
    send_precompile_call,
)

LAUNCHPAD_PRECOMPILE = "0x0000000000000000000000000000000000001017"

LaunchpadTokenWrite = Literal["claimAirdrop", "executeAirdrop", "executeBurn", "executeLpRewards", "pause", "unpause"]


def _event(name: str, *inputs: tuple[str, AbiType, bool]) -> EventSpec:
    return EventSpec(name, LAUNCHPAD_PRECOMPILE, tuple(EventInput(*item) for item in inputs))


LAUNCHPAD_EVENTS: tuple[EventSpec, ...] = (
    _event("AirdropClaimed", ("token", "address", True), ("holder", "address", True), ("amount", "uint256", False), ("epoch", "uint256", False)),
    _event("AirdropExecuted", ("token", "address", True), ("amount", "uint256", False), ("epoch", "uint256", False)),
    _event(
        "FeeRecorded",
        ("token", "address", True),
        ("feeAmount", "uint256", False),
        ("protocolCut", "uint256", False),
        ("poolCut", "uint256", False),
    ),
    _event("FeeStrategyChanged", ("token", "address", True), ("oldStrategy", "uint8", False), ("newStrategy", "uint8", False)),
    _event("FeesBurned", ("token", "address", True), ("amount", "uint256", False)),
    _event("FeesClaimed", ("token", "address", True), ("recipient", "address", True), ("amount", "uint256", False)),
    _event("LpRewardsExecuted", ("token", "address", True), ("amount", "uint256", False)),
    _event(
        "MarketCreated",
        ("token", "address", True),
        ("creator", "address", True),
        ("denom", "string", False),
        ("name", "string", False),
        ("symbol", "string", False),
        ("feeStrategy", "uint8", False),
    ),
    _event("PauseToggled", ("token", "address", True), ("paused", "bool", False)),
    _event(
        "Swap",
        ("token", "address", True),
        ("trader", "address", True),
        ("recipient", "address", True),
        ("isBuy", "bool", False),
        ("amountIn", "uint256", False),
        ("amountOut", "uint256", False),
        ("feeAmount", "uint256", False),
        ("price", "uint256", False),
    ),
)


def decode_launchpad_event(address: str, topics: Sequence[str], data: str) -> DecodedEvent:
    return decode_event_from(LAUNCHPAD_EVENTS, address, topics, data)


def _call(name: str, types: Sequence[AbiType], values: Sequence[AbiValue]) -> PrecompileCall:
    return PrecompileCall(LAUNCHPAD_PRECOMPILE, encode_abi_call(name, types, values))


_SWAP: list[AbiType] = ["address", "uint256", "uint256", "address", "uint256"]


def launchpad_buy_call(token: str, quote_in: int, min_out: int, recipient: str, deadline: int) -> PrecompileCall:
    """``buy(address,uint256,uint256,address,uint256)``."""
    return _call("buy", _SWAP, [token, quote_in, min_out, recipient, deadline])


def launchpad_sell_call(token: str, amount_in: int, min_out: int, recipient: str, deadline: int) -> PrecompileCall:
    """``sell(address,uint256,uint256,address,uint256)``."""
    return _call("sell", _SWAP, [token, amount_in, min_out, recipient, deadline])


def launchpad_create_market_call(name: str, symbol: str, fee_strategy: int) -> PrecompileCall:
    """``createMarket(string,string,uint8)``."""
    return _call("createMarket", ["string", "string", "uint8"], [name, symbol, fee_strategy])


def launchpad_set_fee_strategy_call(token: str, fee_strategy: int) -> PrecompileCall:
    """``setFeeStrategy(address,uint8)``."""
    return _call("setFeeStrategy", ["address", "uint8"], [token, fee_strategy])


def launchpad_claim_fees_call(token: str, recipient: str) -> PrecompileCall:
    """``claimFees(address,address)``."""
    return _call("claimFees", ["address", "address"], [token, recipient])


def launchpad_token_call(write: LaunchpadTokenWrite, token: str) -> PrecompileCall:
    """``claimAirdrop``, ``executeAirdrop``, ``executeBurn``, ``executeLpRewards``, ``pause`` or ``unpause``."""
    return _call(write, ["address"], [token])


def send_launchpad_buy(request: WalletRequest, sender: str, token: str, quote_in: int, min_out: int, recipient: str, deadline: int) -> str:
    return send_precompile_call(request, sender, launchpad_buy_call(token, quote_in, min_out, recipient, deadline))


def send_launchpad_sell(request: WalletRequest, sender: str, token: str, amount_in: int, min_out: int, recipient: str, deadline: int) -> str:
    return send_precompile_call(request, sender, launchpad_sell_call(token, amount_in, min_out, recipient, deadline))


def send_launchpad_create_market(request: WalletRequest, sender: str, name: str, symbol: str, fee_strategy: int) -> str:
    return send_precompile_call(request, sender, launchpad_create_market_call(name, symbol, fee_strategy))


def send_launchpad_set_fee_strategy(request: WalletRequest, sender: str, token: str, fee_strategy: int) -> str:
    return send_precompile_call(request, sender, launchpad_set_fee_strategy_call(token, fee_strategy))


def send_launchpad_claim_fees(request: WalletRequest, sender: str, token: str, recipient: str) -> str:
    return send_precompile_call(request, sender, launchpad_claim_fees_call(token, recipient))


def send_launchpad_token_write(request: WalletRequest, sender: str, write: LaunchpadTokenWrite, token: str) -> str:
    return send_precompile_call(request, sender, launchpad_token_call(write, token))
