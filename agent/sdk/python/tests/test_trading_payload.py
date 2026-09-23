from __future__ import annotations

import dataclasses
import json
import unittest
from pathlib import Path

from layerx_sdk.perps import (
    PERPS_ACTIVITIES,
    PERPS_MODULE_ID,
    TradingPayloadError,
    build_perps_activity,
    decode_perps_activity,
)
from layerx_sdk.spot import SPOT_ACTIVITIES, SPOT_MODULE_ID, build_spot_activity, decode_spot_activity

FIXTURE = json.loads(
    (Path(__file__).resolve().parents[4] / "tests/fixtures/trading-payloads/vectors.json").read_text()
)
VECTORS = {vector["name"]: vector for vector in FIXTURE["vectors"]}


def typed(vector: dict) -> object:
    module, ordinal = vector["activity_type"] >> 16, vector["activity_type"] & 0xFFFF
    kind = (PERPS_ACTIVITIES if module == PERPS_MODULE_ID else SPOT_ACTIVITIES)[ordinal]
    values = {}
    for item in dataclasses.fields(kind):
        value = vector["fields"][item.name]
        if isinstance(value, list):
            values[item.name] = tuple(value)
        elif item.type == "int":
            values[item.name] = int(value)
        else:
            values[item.name] = value
    return kind(**values)


class TradingPayloadTest(unittest.TestCase):
    def refuses(self, code: str, run) -> None:
        with self.assertRaises(TradingPayloadError) as caught:
            run()
        self.assertEqual(caught.exception.code, code)

    def test_every_activity_matches_the_kernel_codec_vectors(self) -> None:
        self.assertEqual(FIXTURE["source"], "tests/modules/dump_trading_vectors.c")
        covered = set()
        for vector in FIXTURE["vectors"]:
            activity = typed(vector)
            perps = vector["name"].startswith("perps_")
            activity_type, payload = (build_perps_activity if perps else build_spot_activity)(activity)
            self.assertEqual(activity_type, vector["activity_type"], vector["name"])
            self.assertEqual(payload.hex(), vector["bytes"], vector["name"])
            decode = decode_perps_activity if perps else decode_spot_activity
            self.assertEqual(decode(activity_type, bytes.fromhex(vector["bytes"])), activity, vector["name"])
            covered.add(activity_type)
        expected = {(PERPS_MODULE_ID << 16) | ordinal for ordinal in PERPS_ACTIVITIES}
        expected |= {(SPOT_MODULE_ID << 16) | ordinal for ordinal in SPOT_ACTIVITIES}
        self.assertEqual(covered, expected)

    def test_perps_decoder_refuses_what_the_kernel_refuses(self) -> None:
        order = VECTORS["perps_order_place"]
        raw = bytes.fromhex(order["bytes"])
        self.refuses("length", lambda: decode_perps_activity(order["activity_type"], raw[1:]))
        self.refuses("unknown_activity", lambda: decode_perps_activity(0x0006000C, raw))
        self.refuses("unknown_activity", lambda: decode_perps_activity(0x000A0002, raw))
        self.refuses("non_canonical", lambda: decode_perps_activity(order["activity_type"], raw[:96] + b"\x03" + raw[97:]))
        self.refuses("non_canonical", lambda: decode_perps_activity(order["activity_type"], raw[:113] + bytes(16)))
        market = VECTORS["perps_market_create"]
        body = bytearray.fromhex(market["bytes"])
        body[32 * 7 + 16 * 4 + 4 * 6 + 8 * 2 + 16 * 2 + 1 + 32 * 7] = 1
        self.refuses("non_canonical", lambda: decode_perps_activity(market["activity_type"], bytes(body)))
        created = typed(market)
        self.refuses(
            "parameter_bounds",
            lambda: dataclasses.replace(created, initial_margin_ratio_bps=created.maintenance_margin_ratio_bps).encode(),
        )
        adl = typed(VECTORS["perps_adl"])
        self.assertGreater(len(adl.position_ids), 1)
        self.refuses("unsorted_sequence", lambda: dataclasses.replace(adl, position_ids=adl.position_ids[::-1]).encode())
        opened = typed(VECTORS["perps_position_open"])
        self.refuses("non_canonical", lambda: dataclasses.replace(opened, entry_notional=1).encode())

    def test_spot_decoder_refuses_what_the_kernel_refuses(self) -> None:
        limit = typed(VECTORS["spot_order_place_limit"])
        self.refuses("non_canonical", lambda: dataclasses.replace(limit, price=0).encode())
        self.refuses("non_canonical", lambda: dataclasses.replace(limit, quote_account_id=limit.base_account_id).encode())
        self.refuses("non_canonical", lambda: dataclasses.replace(limit, kind=2).encode())
        raw = bytes.fromhex(VECTORS["spot_order_place_limit"]["bytes"])
        self.refuses("non_canonical", lambda: decode_spot_activity(0x000A0002, raw[:129] + b"\x03" + raw[130:]))
        self.refuses("unknown_activity", lambda: decode_spot_activity(0x000A0006, raw))
        self.refuses("non_canonical", lambda: decode_spot_activity(0x000A0004, bytes(32)))


if __name__ == "__main__":
    unittest.main()
