"""Calldata builders, wallet send helpers and event decoders for the LayerXBridge precompile."""

from __future__ import annotations

from collections.abc import Sequence

from .exchange import (
    DecodedEvent,
    EventInput,
    EventSpec,
    PrecompileCall,
    WalletRequest,
    decode_event_from,
    encode_abi_call,
    send_precompile_call,
)

LAYERX_BRIDGE_PRECOMPILE = "0x0000000000000000000000000000000000001016"

BRIDGE_EVENTS: tuple[EventSpec, ...] = (
    EventSpec(
        "BridgeIn",
        LAYERX_BRIDGE_PRECOMPILE,
        (
            EventInput("chain", "uint64", True),
            EventInput("txHash", "bytes32", True),
            EventInput("recipient", "address", True),
            EventInput("logIndex", "uint64"),
            EventInput("asset", "address"),
            EventInput("amount", "uint256"),
            EventInput("denom", "string"),
        ),
    ),
    EventSpec(
        "BridgeOut",
        LAYERX_BRIDGE_PRECOMPILE,
        (
            EventInput("chain", "uint64", True),
            EventInput("asset", "address", True),
            EventInput("amount", "uint256"),
            EventInput("recipient", "address"),
            EventInput("nonce", "uint64", True),
        ),
    ),
)


def decode_bridge_event(address: str, topics: Sequence[str], data: str) -> DecodedEvent:
    return decode_event_from(BRIDGE_EVENTS, address, topics, data)


def bridge_in_call(
    chain: int,
    vault: str,
    tx_hash: str,
    log_index: int,
    recipient: str,
    asset: str,
    amount: int,
    signatures: Sequence[str],
) -> PrecompileCall:
    """``bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])``."""
    return PrecompileCall(
        LAYERX_BRIDGE_PRECOMPILE,
        encode_abi_call(
            "bridgeIn",
            ["uint64", "address", "bytes32", "uint64", "bytes32", "address", "uint256", "bytes[]"],
            [chain, vault, tx_hash, log_index, recipient, asset, amount, list(signatures)],
        ),
    )


def bridge_out_call(chain: int, asset: str, amount: int, recipient: str) -> PrecompileCall:
    """``bridgeOut(uint64,address,uint256,address)``."""
    return PrecompileCall(
        LAYERX_BRIDGE_PRECOMPILE,
        encode_abi_call("bridgeOut", ["uint64", "address", "uint256", "address"], [chain, asset, amount, recipient]),
    )


def send_bridge_in(
    request: WalletRequest,
    sender: str,
    chain: int,
    vault: str,
    tx_hash: str,
    log_index: int,
    recipient: str,
    asset: str,
    amount: int,
    signatures: Sequence[str],
) -> str:
    return send_precompile_call(
        request, sender, bridge_in_call(chain, vault, tx_hash, log_index, recipient, asset, amount, signatures)
    )


def send_bridge_out(request: WalletRequest, sender: str, chain: int, asset: str, amount: int, recipient: str) -> str:
    return send_precompile_call(request, sender, bridge_out_call(chain, asset, amount, recipient))
