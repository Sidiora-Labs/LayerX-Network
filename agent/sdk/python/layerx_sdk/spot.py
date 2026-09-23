"""Typed builders for every LayerX spot module activity (module 10).

Each encoder emits the exact bytes the kernel codec (``src/modules/spot``)
accepts and each decoder refuses what the kernel refuses.
"""

from __future__ import annotations

from dataclasses import dataclass

from .perps import TradingPayloadError, _Layout, decode_trading_activity

SPOT_MODULE_ID = 10
LIMIT = 1
MARKET = 2
GOOD_TIL_CANCELLED = 1
IMMEDIATE_OR_CANCEL = 2


class _Spot(_Layout):
    MODULE = SPOT_MODULE_ID


@dataclass(frozen=True)
class SpotMarketCreate(_Spot):
    market_id: str
    base_asset: str
    quote_asset: str
    tick_size: int
    lot_size: int
    administrator: str
    ORDINAL = 1
    LAYOUT = ("id", "id", "id", "u128", "u128", "id")

    def check(self) -> None:
        if self.tick_size == 0 or self.lot_size == 0 or self.base_asset == self.quote_asset:
            raise TradingPayloadError("non_canonical", "market")


@dataclass(frozen=True)
class SpotOrderPlace(_Spot):
    market_id: str
    order_id: str
    base_account_id: str
    quote_account_id: str
    side: int
    kind: int
    time_in_force: int
    price: int
    quantity: int
    ORDINAL = 2
    LAYOUT = ("id", "id", "id", "id", "choice", "choice", "choice", "u128", "u128")

    def check(self) -> None:
        priced = self.price != 0 if self.kind == LIMIT else self.price == 0 and self.time_in_force == IMMEDIATE_OR_CANCEL
        if not priced or self.quantity == 0 or self.base_account_id == self.quote_account_id:
            raise TradingPayloadError("non_canonical", "order")


@dataclass(frozen=True)
class SpotOrderCancel(_Spot):
    market_id: str
    order_id: str
    ORDINAL = 3
    LAYOUT = ("id", "id")


@dataclass(frozen=True)
class SpotMarketHalt(_Spot):
    market_id: str
    ORDINAL = 4
    LAYOUT = ("id",)


@dataclass(frozen=True)
class SpotMarketResume(_Spot):
    market_id: str
    ORDINAL = 5
    LAYOUT = ("id",)


SpotActivity = SpotMarketCreate | SpotOrderPlace | SpotOrderCancel | SpotMarketHalt | SpotMarketResume

SPOT_ACTIVITIES: dict[int, type[_Layout]] = {
    kind.ORDINAL: kind for kind in (SpotMarketCreate, SpotOrderPlace, SpotOrderCancel, SpotMarketHalt, SpotMarketResume)
}


def build_spot_activity(activity: SpotActivity) -> tuple[int, bytes]:
    """The packed activity type and exact payload bytes a spot activity submits."""
    return activity.activity_type, activity.encode()


def decode_spot_activity(activity_type: int, payload: bytes) -> SpotActivity:
    """Decodes and validates kernel payload bytes of a spot activity."""
    return decode_trading_activity(SPOT_ACTIVITIES, SPOT_MODULE_ID, activity_type, payload)  # type: ignore[return-value]
