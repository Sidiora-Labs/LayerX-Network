"""Typed builders for every LayerX perps module activity.

Each encoder emits the exact bytes the kernel codec (``src/modules/perps``)
accepts and each decoder refuses what the kernel refuses.
"""

from __future__ import annotations

from dataclasses import dataclass, fields
from typing import ClassVar, Literal

PERPS_MODULE_ID = 6
MAX_ORACLE_KEYS = 8
ADL_CAPACITY = 128
BASIS_POINTS_ONE = 10000

TradingPayloadErrorCode = Literal["unknown_activity", "length", "non_canonical", "parameter_bounds", "unsorted_sequence"]


class TradingPayloadError(ValueError):
    def __init__(self, code: TradingPayloadErrorCode, detail: str = "") -> None:
        super().__init__(f"{code}: {detail}" if detail else code)
        self.code = code


_WIDTHS = {"u8": 1, "u32": 4, "u64": 8, "u128": 16}
_ZERO_ID = "00" * 32


def _check_id(value: str, nonzero: bool = True) -> bytes:
    if not isinstance(value, str) or len(value) != 64 or value != value.lower():
        raise TradingPayloadError("non_canonical", "identifier")
    try:
        raw = bytes.fromhex(value)
    except ValueError as error:
        raise TradingPayloadError("non_canonical", "identifier") from error
    if nonzero and raw == bytes(32):
        raise TradingPayloadError("non_canonical", "zero identifier")
    return raw


class Writer:
    def __init__(self) -> None:
        self.out = bytearray()

    def id(self, value: str, nonzero: bool = True) -> Writer:
        self.out += _check_id(value, nonzero)
        return self

    def uint(self, value: int, width: int) -> Writer:
        if not isinstance(value, int) or isinstance(value, bool) or value < 0 or value >= 1 << (8 * width):
            raise TradingPayloadError("non_canonical", "integer width")
        self.out += value.to_bytes(width, "big")
        return self

    def flag(self, value: bool) -> Writer:
        self.out.append(1 if value else 0)
        return self


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.offset = 0

    def take(self, count: int) -> bytes:
        if self.offset + count > len(self.data):
            raise TradingPayloadError("length")
        out = self.data[self.offset : self.offset + count]
        self.offset += count
        return out

    def id(self) -> str:
        return self.take(32).hex()

    def uint(self, width: int) -> int:
        return int.from_bytes(self.take(width), "big")

    def flag(self) -> bool:
        value = self.uint(1)
        if value > 1:
            raise TradingPayloadError("non_canonical", "flag")
        return value == 1


def _one_or_two(value: int, label: str) -> int:
    if value not in (1, 2):
        raise TradingPayloadError("non_canonical", label)
    return value


def _nonzero(*values: int) -> None:
    if any(value == 0 for value in values):
        raise TradingPayloadError("non_canonical", "zero value")


class _Layout:
    """A fixed-width activity whose fields encode in declaration order."""

    ORDINAL: ClassVar[int]
    MODULE: ClassVar[int]
    LAYOUT: ClassVar[tuple[str, ...]]

    @property
    def activity_type(self) -> int:
        return (self.MODULE << 16) | self.ORDINAL

    def check(self) -> None:
        return None

    def encode(self) -> bytes:
        self.check()
        writer = Writer()
        for spec, item in zip(self.LAYOUT, fields(self)):  # type: ignore[arg-type]
            value = getattr(self, item.name)
            if spec == "id":
                writer.id(value)
            elif spec == "flag":
                writer.flag(value)
            elif spec == "choice":
                writer.uint(_one_or_two(value, item.name), 1)
            else:
                writer.uint(value, _WIDTHS[spec])
        return bytes(writer.out)

    @classmethod
    def length(cls) -> int:
        return sum(32 if spec == "id" else 1 if spec in ("flag", "choice") else _WIDTHS[spec] for spec in cls.LAYOUT)

    @classmethod
    def read(cls, reader: Reader) -> _Layout:
        values = []
        for spec, item in zip(cls.LAYOUT, fields(cls)):  # type: ignore[arg-type]
            if spec == "id":
                values.append(reader.id())
            elif spec == "flag":
                values.append(reader.flag())
            elif spec == "choice":
                values.append(_one_or_two(reader.uint(1), item.name))
            else:
                values.append(reader.uint(_WIDTHS[spec]))
        return cls(*values)


class _Perps(_Layout):
    MODULE = PERPS_MODULE_ID


@dataclass(frozen=True)
class PerpsMarketCreate(_Perps):
    market_id: str
    quote_asset: str
    administrator: str
    liquidity_account_id: str
    long_funding_account_id: str
    short_funding_account_id: str
    insurance_account_id: str
    contract_size: int
    tick_size: int
    lot_size: int
    price_scale: int
    initial_margin_ratio_bps: int
    maintenance_margin_ratio_bps: int
    liquidation_fee_bps: int
    liquidator_share_bps: int
    maximum_funding_rate_bps: int
    maximum_deviation_basis_points: int
    funding_interval_ms: int
    maximum_oracle_staleness_ms: int
    minimum_price: int
    maximum_price: int
    permitted_oracle_keys: tuple[str, ...]
    parameter_version: int
    halted: bool

    ORDINAL = 1
    LAYOUT = (
        ("id",) * 7
        + ("u128",) * 4
        + ("u32",) * 6
        + ("u64",) * 2
        + ("u128",) * 2
    )

    def check(self) -> None:
        accounts = [self.liquidity_account_id, self.long_funding_account_id, self.short_funding_account_id, self.insurance_account_id]
        keys = list(self.permitted_oracle_keys)
        ok = (
            self.market_id != _ZERO_ID
            and self.quote_asset != _ZERO_ID
            and self.administrator != _ZERO_ID
            and all(account != _ZERO_ID and account not in accounts[:index] for index, account in enumerate(accounts))
            and self.contract_size != 0
            and self.tick_size != 0
            and self.lot_size != 0
            and self.price_scale != 0
            and self.maintenance_margin_ratio_bps != 0
            and self.maintenance_margin_ratio_bps < self.initial_margin_ratio_bps <= BASIS_POINTS_ONE
            and self.liquidation_fee_bps <= BASIS_POINTS_ONE
            and self.liquidator_share_bps <= BASIS_POINTS_ONE
            and 0 < self.maximum_funding_rate_bps <= BASIS_POINTS_ONE
            and 0 < self.maximum_deviation_basis_points <= BASIS_POINTS_ONE
            and self.funding_interval_ms != 0
            and self.maximum_oracle_staleness_ms != 0
            and 0 < self.minimum_price < self.maximum_price
            and self.parameter_version != 0
            and 0 < len(keys) <= MAX_ORACLE_KEYS
            and all(key != _ZERO_ID for key in keys)
            and all(left < right for left, right in zip(keys, keys[1:]))
        )
        if not ok:
            raise TradingPayloadError("parameter_bounds")

    def encode(self) -> bytes:
        self.check()
        writer = Writer()
        head = [getattr(self, item.name) for item in fields(self)][: len(self.LAYOUT)]
        for spec, value in zip(self.LAYOUT, head):
            if spec == "id":
                writer.id(value)
            else:
                writer.uint(value, _WIDTHS[spec])
        writer.uint(len(self.permitted_oracle_keys), 1)
        for key in self.permitted_oracle_keys:
            writer.id(key)
        writer.out += bytes(32 * (MAX_ORACLE_KEYS - len(self.permitted_oracle_keys)))
        writer.uint(self.parameter_version, 4).flag(self.halted)
        return bytes(writer.out)

    @classmethod
    def length(cls) -> int:
        return 622

    @classmethod
    def read(cls, reader: Reader) -> PerpsMarketCreate:
        head: list[object] = [reader.id() if spec == "id" else reader.uint(_WIDTHS[spec]) for spec in cls.LAYOUT]
        count = reader.uint(1)
        if count > MAX_ORACLE_KEYS:
            raise TradingPayloadError("non_canonical", "oracle key count")
        keys = []
        for index in range(MAX_ORACLE_KEYS):
            key = reader.id()
            if index < count:
                keys.append(key)
            elif key != _ZERO_ID:
                raise TradingPayloadError("non_canonical", "oracle key padding")
        market = cls(*head, tuple(keys), reader.uint(4), reader.flag())  # type: ignore[arg-type]
        market.check()
        return market


@dataclass(frozen=True)
class PerpsMarketHalt(_Perps):
    market_id: str
    halted: bool
    ORDINAL = 2
    LAYOUT = ("id", "flag")


@dataclass(frozen=True)
class PerpsOraclePush(_Perps):
    market_id: str
    observation_sequence: int
    price: int
    observed_at: int
    source_identifier: int
    ORDINAL = 3
    LAYOUT = ("id", "u64", "u128", "u64", "u64")

    def check(self) -> None:
        _nonzero(self.observation_sequence, self.price, self.observed_at, self.source_identifier)


@dataclass(frozen=True)
class PerpsOrderPlace(_Perps):
    market_id: str
    order_id: str
    owner_account_id: str
    side: int
    price: int
    quantity: int
    ORDINAL = 4
    LAYOUT = ("id", "id", "id", "choice", "u128", "u128")

    def check(self) -> None:
        _nonzero(self.price, self.quantity)


@dataclass(frozen=True)
class PerpsOrderCancel(_Perps):
    market_id: str
    order_id: str
    ORDINAL = 5
    LAYOUT = ("id", "id")


@dataclass(frozen=True)
class PerpsPositionOpen(_Perps):
    market_id: str
    position_id: str
    margin_account_id: str
    side: int
    size: int
    entry_notional: int
    margin_amount: int
    ORDINAL = 6
    LAYOUT = ("id", "id", "id", "choice", "u128", "u128", "u128")

    def check(self) -> None:
        _nonzero(self.size, self.margin_amount)
        if self.entry_notional != 0:
            raise TradingPayloadError("non_canonical", "entry notional must be zero")


@dataclass(frozen=True)
class PerpsPositionIncrease(_Perps):
    market_id: str
    position_id: str
    size_delta: int
    notional_delta: int
    margin_amount: int
    ORDINAL = 7
    LAYOUT = ("id", "id", "u128", "u128", "u128")

    def check(self) -> None:
        _nonzero(self.size_delta, self.margin_amount)
        if self.notional_delta != 0:
            raise TradingPayloadError("non_canonical", "notional delta must be zero")


@dataclass(frozen=True)
class PerpsPositionClose(_Perps):
    market_id: str
    position_id: str
    ORDINAL = 8
    LAYOUT = ("id", "id")


@dataclass(frozen=True)
class PerpsFundingTick(_Perps):
    market_id: str
    ORDINAL = 9
    LAYOUT = ("id",)


@dataclass(frozen=True)
class PerpsLiquidate(_Perps):
    market_id: str
    position_id: str
    liquidator_account_id: str
    ORDINAL = 10
    LAYOUT = ("id", "id", "id")


@dataclass(frozen=True)
class PerpsAdl(_Perps):
    market_id: str
    position_ids: tuple[str, ...]
    ORDINAL = 11
    LAYOUT = ()

    def encode(self) -> bytes:
        ids = list(self.position_ids)
        if not 0 < len(ids) <= ADL_CAPACITY:
            raise TradingPayloadError("non_canonical", "position count")
        for position in ids:
            _check_id(position)
        if any(left >= right for left, right in zip(ids, ids[1:])):
            raise TradingPayloadError("unsorted_sequence")
        writer = Writer().id(self.market_id).uint(len(ids), 1)
        for position in ids:
            writer.id(position)
        return bytes(writer.out)


PerpsActivity = (
    PerpsMarketCreate
    | PerpsMarketHalt
    | PerpsOraclePush
    | PerpsOrderPlace
    | PerpsOrderCancel
    | PerpsPositionOpen
    | PerpsPositionIncrease
    | PerpsPositionClose
    | PerpsFundingTick
    | PerpsLiquidate
    | PerpsAdl
)

PERPS_ACTIVITIES: dict[int, type[_Layout]] = {
    kind.ORDINAL: kind
    for kind in (
        PerpsMarketCreate,
        PerpsMarketHalt,
        PerpsOraclePush,
        PerpsOrderPlace,
        PerpsOrderCancel,
        PerpsPositionOpen,
        PerpsPositionIncrease,
        PerpsPositionClose,
        PerpsFundingTick,
        PerpsLiquidate,
        PerpsAdl,
    )
}


def build_perps_activity(activity: PerpsActivity) -> tuple[int, bytes]:
    """The packed activity type and exact payload bytes a perps activity submits."""
    return activity.activity_type, activity.encode()


def decode_trading_activity(
    activities: dict[int, type[_Layout]], module: int, activity_type: int, payload: bytes
) -> _Layout:
    kind = activities.get(activity_type & 0xFFFF) if activity_type >> 16 == module else None
    if kind is None:
        raise TradingPayloadError("unknown_activity", hex(activity_type))
    if kind is PerpsAdl:
        if not 65 <= len(payload) <= 33 + 32 * ADL_CAPACITY:
            raise TradingPayloadError("length")
        reader = Reader(payload)
        market_id = reader.id()
        count = reader.uint(1)
        if count == 0 or len(payload) != 33 + 32 * count or market_id == _ZERO_ID:
            raise TradingPayloadError("non_canonical", "adl shape")
        decoded: _Layout = PerpsAdl(market_id, tuple(reader.id() for _ in range(count)))
        decoded.encode()
        return decoded
    if len(payload) != kind.length():
        raise TradingPayloadError("length")
    decoded = kind.read(Reader(bytes(payload)))
    if decoded.encode() != bytes(payload):
        raise TradingPayloadError("non_canonical")
    return decoded


def decode_perps_activity(activity_type: int, payload: bytes) -> PerpsActivity:
    """Decodes and validates kernel payload bytes of a perps activity."""
    return decode_trading_activity(PERPS_ACTIVITIES, PERPS_MODULE_ID, activity_type, payload)  # type: ignore[return-value]
