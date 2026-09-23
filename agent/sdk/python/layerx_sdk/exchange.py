"""Calldata builders, a wallet send helper and event decoders for the
LayerXExchange precompile, plus the EVM ABI machinery the bridge and
launchpad precompile modules share."""

from __future__ import annotations

import re
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Literal, Union

from .account_derivation import keccak256

LAYERX_EXCHANGE_PRECOMPILE = "0x0000000000000000000000000000000000001015"

AbiType = Literal["address", "bool", "bytes32", "bytes[]", "string", "uint8", "uint64", "uint256"]
AbiValue = Union[int, bool, str, Sequence[str]]
EventValue = Union[int, bool, str]
WalletRequest = Callable[[str, list[object]], object]

_ADDRESS = re.compile(r"^0x[0-9a-fA-F]{40}$")
_BYTES32 = re.compile(r"^0x[0-9a-fA-F]{64}$")
_WORD = 32


class PrecompileAbiError(ValueError):
    def __init__(self, code: str, detail: str = "") -> None:
        super().__init__(f"{code}: {detail}" if detail else code)
        self.code = code


@dataclass(frozen=True)
class PrecompileCall:
    to: str
    data: str
    value: int = 0


@dataclass(frozen=True)
class EventInput:
    name: str
    type: AbiType
    indexed: bool = False


@dataclass(frozen=True)
class EventSpec:
    name: str
    precompile: str
    inputs: tuple[EventInput, ...]

    @property
    def signature(self) -> str:
        return abi_signature(self.name, [item.type for item in self.inputs])

    @property
    def topic0(self) -> str:
        return "0x" + keccak256(self.signature.encode()).hex()


@dataclass(frozen=True)
class DecodedEvent:
    event: str
    precompile: str
    topic0: str
    fields: dict[str, EventValue]


def _unhex(value: str, label: str) -> bytes:
    if not isinstance(value, str) or not value.startswith("0x") or len(value) % 2:
        raise PrecompileAbiError("invalid_value", label)
    try:
        return bytes.fromhex(value[2:])
    except ValueError as error:
        raise PrecompileAbiError("invalid_value", label) from error


def _bits(kind: AbiType) -> int:
    return {"uint8": 8, "uint64": 64}.get(kind, 256)


def _uint(value: int, bits: int, label: str) -> bytes:
    if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value < 1 << bits:
        raise PrecompileAbiError("invalid_value", label)
    return value.to_bytes(_WORD, "big")


def _padded(raw: bytes) -> bytes:
    return raw + bytes(-len(raw) % _WORD)


def _dynamic_bytes(raw: bytes) -> bytes:
    return _uint(len(raw), 256, "length") + _padded(raw)


def _static(kind: AbiType, value: AbiValue, label: str) -> bytes:
    if kind == "address":
        if not isinstance(value, str) or not _ADDRESS.match(value):
            raise PrecompileAbiError("invalid_value", label)
        return bytes(12) + _unhex(value, label)
    if kind == "bytes32":
        if not isinstance(value, str) or not _BYTES32.match(value):
            raise PrecompileAbiError("invalid_value", label)
        return _unhex(value, label)
    if kind == "bool":
        if not isinstance(value, bool):
            raise PrecompileAbiError("invalid_value", label)
        return _uint(int(value), 8, label)
    return _uint(value, _bits(kind), label)  # type: ignore[arg-type]


def _dynamic(kind: AbiType, value: AbiValue, label: str) -> bytes:
    if kind == "string":
        if not isinstance(value, str):
            raise PrecompileAbiError("invalid_value", label)
        return _dynamic_bytes(value.encode())
    if isinstance(value, (str, int)):
        raise PrecompileAbiError("invalid_value", label)
    items = [_dynamic_bytes(_unhex(item, label)) for item in value]
    offsets = []
    offset = len(items) * _WORD
    for item in items:
        offsets.append(_uint(offset, 256, label))
        offset += len(item)
    return _uint(len(items), 256, label) + b"".join(offsets) + b"".join(items)


def abi_signature(name: str, types: Sequence[AbiType]) -> str:
    return f"{name}({','.join(types)})"


def abi_selector(signature: str) -> str:
    return "0x" + keccak256(signature.encode())[:4].hex()


def encode_abi_call(name: str, types: Sequence[AbiType], values: Sequence[AbiValue]) -> str:
    """ABI-encodes a call to ``name(types...)`` with ``values``."""
    if len(types) != len(values):
        raise PrecompileAbiError("invalid_value", "argument count")
    heads: list[bytes] = []
    tails: list[bytes] = []
    tail_offset = len(types) * _WORD
    for index, (kind, value) in enumerate(zip(types, values)):
        label = f"{name} argument {index}"
        if kind in ("string", "bytes[]"):
            tail = _dynamic(kind, value, label)
            heads.append(_uint(tail_offset, 256, label))
            tails.append(tail)
            tail_offset += len(tail)
        else:
            heads.append(_static(kind, value, label))
    return abi_selector(abi_signature(name, types)) + (b"".join(heads) + b"".join(tails)).hex()


def precompile_transaction_request(sender: str, call: PrecompileCall) -> dict[str, str]:
    """The transaction fields a signer or wallet sends for a precompile write."""
    if not _ADDRESS.match(sender):
        raise PrecompileAbiError("invalid_value", "from")
    return {"from": sender, "to": call.to, "data": call.data, "value": hex(call.value)}


def send_precompile_call(request: WalletRequest, sender: str, call: PrecompileCall) -> str:
    """Asks an EIP-1193 ``request(method, params)`` to send a precompile write; returns its hash."""
    answer = request("eth_sendTransaction", [precompile_transaction_request(sender, call)])
    if not isinstance(answer, str) or not _BYTES32.match(answer):
        raise PrecompileAbiError("malformed_wallet_answer", "wallet returned no transaction hash")
    return answer


def _word(data: bytes, offset: int) -> bytes:
    if offset + _WORD > len(data):
        raise PrecompileAbiError("data_length", str(len(data)))
    return data[offset : offset + _WORD]


def _decode_word(kind: AbiType, raw: bytes) -> EventValue:
    if kind == "address":
        if any(raw[:12]):
            raise PrecompileAbiError("non_canonical_word", kind)
        return "0x" + raw[12:].hex()
    if kind == "bytes32":
        return "0x" + raw.hex()
    value = int.from_bytes(raw, "big")
    if kind == "bool":
        if value > 1:
            raise PrecompileAbiError("non_canonical_word", kind)
        return value == 1
    if value >= 1 << _bits(kind):
        raise PrecompileAbiError("non_canonical_word", kind)
    return value


def _decode_string(data: bytes, head: int) -> str:
    offset = int(_decode_word("uint64", _word(data, head)))
    length = int(_decode_word("uint64", _word(data, offset)))
    start = offset + _WORD
    if start + length > len(data):
        raise PrecompileAbiError("data_length", str(len(data)))
    try:
        return data[start : start + length].decode("utf-8")
    except UnicodeDecodeError as error:
        raise PrecompileAbiError("invalid_string") from error


def decode_event_with_spec(spec: EventSpec, address: str, topics: Sequence[str], data: str) -> DecodedEvent:
    """Decodes one log against one event spec."""
    if address.lower() != spec.precompile or not topics or topics[0].lower() != spec.topic0:
        raise PrecompileAbiError("unknown_event", spec.name)
    indexed = [item for item in spec.inputs if item.indexed]
    if len(topics) != len(indexed) + 1:
        raise PrecompileAbiError("topic_count", str(len(topics)))
    body = _unhex(data, "data")
    fields: dict[str, EventValue] = {}
    topic = 1
    head = 0
    for item in spec.inputs:
        if item.indexed:
            value = topics[topic]
            if not _BYTES32.match(value):
                raise PrecompileAbiError("non_canonical_word", item.name)
            fields[item.name] = _decode_word(item.type, _unhex(value, item.name))
            topic += 1
        else:
            fields[item.name] = _decode_string(body, head) if item.type == "string" else _decode_word(item.type, _word(body, head))
            head += _WORD
    if not any(not item.indexed and item.type == "string" for item in spec.inputs) and len(body) != head:
        raise PrecompileAbiError("data_length", str(len(body)))
    return DecodedEvent(spec.name, spec.precompile, spec.topic0, fields)


def decode_event_from(specs: Sequence[EventSpec], address: str, topics: Sequence[str], data: str) -> DecodedEvent:
    """Finds the spec a log belongs to and decodes it."""
    topic0 = topics[0].lower() if topics else ""
    for spec in specs:
        if spec.precompile == address.lower() and spec.topic0 == topic0:
            return decode_event_with_spec(spec, address, topics, data)
    raise PrecompileAbiError("unknown_event", topic0 or "no topic")


def _event(name: str, *inputs: tuple[str, AbiType, bool]) -> EventSpec:
    return EventSpec(name, LAYERX_EXCHANGE_PRECOMPILE, tuple(EventInput(*item) for item in inputs))


EXCHANGE_EVENTS: tuple[EventSpec, ...] = (
    _event(
        "MarginDeposited",
        ("intentId", "bytes32", True),
        ("account", "bytes32", True),
        ("owner", "address", True),
        ("assetId", "bytes32", False),
        ("amount", "uint256", False),
        ("depositId", "bytes32", False),
        ("nonce", "uint64", False),
    ),
    _event(
        "MarginWithdrawalRequested",
        ("intentId", "bytes32", True),
        ("account", "bytes32", True),
        ("owner", "address", True),
        ("assetId", "bytes32", False),
        ("amount", "uint256", False),
        ("nonce", "uint64", False),
    ),
    _event(
        "OrderCancelRequested",
        ("intentId", "bytes32", True),
        ("orderId", "bytes32", True),
        ("owner", "address", True),
        ("nonce", "uint64", False),
    ),
    _event(
        "OrderPlaced",
        ("intentId", "bytes32", True),
        ("marketId", "bytes32", True),
        ("owner", "address", True),
        ("side", "uint8", False),
        ("price", "uint256", False),
        ("quantity", "uint256", False),
        ("timeInForce", "uint8", False),
        ("nonce", "uint64", False),
    ),
    _event(
        "SettlementRequested",
        ("intentId", "bytes32", True),
        ("positionId", "bytes32", True),
        ("owner", "address", True),
        ("nonce", "uint64", False),
    ),
)


def decode_exchange_event(address: str, topics: Sequence[str], data: str) -> DecodedEvent:
    return decode_event_from(EXCHANGE_EVENTS, address, topics, data)


def _call(name: str, types: Sequence[AbiType], values: Sequence[AbiValue], value: int = 0) -> PrecompileCall:
    return PrecompileCall(LAYERX_EXCHANGE_PRECOMPILE, encode_abi_call(name, types, values), value)


def exchange_place_order_call(market_id: str, side: int, price: int, quantity: int, time_in_force: int) -> PrecompileCall:
    """``placeOrder(bytes32,uint8,uint256,uint256,uint8)``."""
    return _call("placeOrder", ["bytes32", "uint8", "uint256", "uint256", "uint8"], [market_id, side, price, quantity, time_in_force])


def exchange_cancel_order_call(order_id: str) -> PrecompileCall:
    """``cancelOrder(bytes32)``."""
    return _call("cancelOrder", ["bytes32"], [order_id])


def exchange_request_settlement_call(position_id: str) -> PrecompileCall:
    """``requestSettlement(bytes32)``."""
    return _call("requestSettlement", ["bytes32"], [position_id])


def exchange_deposit_margin_call(account: str, amount_wei: int) -> PrecompileCall:
    """``depositMargin(bytes32)``, payable with the native amount in wei."""
    if amount_wei <= 0:
        raise PrecompileAbiError("invalid_value", "deposit amount")
    return _call("depositMargin", ["bytes32"], [account], amount_wei)


def exchange_deposit_margin_token_call(pointer: str, amount: int, account: str) -> PrecompileCall:
    """``depositMarginToken(address,uint256,bytes32)``."""
    return _call("depositMarginToken", ["address", "uint256", "bytes32"], [pointer, amount, account])


def exchange_withdraw_margin_call(account: str, asset_id: str, amount: int) -> PrecompileCall:
    """``withdrawMargin(bytes32,bytes32,uint256)``."""
    return _call("withdrawMargin", ["bytes32", "bytes32", "uint256"], [account, asset_id, amount])


def send_exchange_place_order(
    request: WalletRequest, sender: str, market_id: str, side: int, price: int, quantity: int, time_in_force: int
) -> str:
    return send_precompile_call(request, sender, exchange_place_order_call(market_id, side, price, quantity, time_in_force))


def send_exchange_cancel_order(request: WalletRequest, sender: str, order_id: str) -> str:
    return send_precompile_call(request, sender, exchange_cancel_order_call(order_id))


def send_exchange_request_settlement(request: WalletRequest, sender: str, position_id: str) -> str:
    return send_precompile_call(request, sender, exchange_request_settlement_call(position_id))


def send_exchange_deposit_margin(request: WalletRequest, sender: str, account: str, amount_wei: int) -> str:
    return send_precompile_call(request, sender, exchange_deposit_margin_call(account, amount_wei))


def send_exchange_deposit_margin_token(request: WalletRequest, sender: str, pointer: str, amount: int, account: str) -> str:
    return send_precompile_call(request, sender, exchange_deposit_margin_token_call(pointer, amount, account))


def send_exchange_withdraw_margin(request: WalletRequest, sender: str, account: str, asset_id: str, amount: int) -> str:
    return send_precompile_call(request, sender, exchange_withdraw_margin_call(account, asset_id, amount))
