import unittest
from layerx_sdk.x402_receive import (
    derive_native_asset_id,
    encode_account_open,
    encode_asset_register,
    encode_grant_revoke,
    encode_asset_supply,
)


class AssetTests(unittest.TestCase):
    issuer = "1eff10b60dad92693680cd2a6ebf032e48aa1ef8f019e2f51a0362df67755d18"
    salt = "02" * 32
    asset = "073db0b2bcee1c62c538a3c25482055597708ec6d79708710c5a94c63b2f8aba"
    native = {
        "salt": salt,
        "symbol": "TOK",
        "name": "Token €",
        "decimals": 18,
        "supply_cap": "1000000",
        "issuer_kind": 1,
        "custody_ref": b"",
    }

    def test_encodings(self):
        self.assertEqual(encode_account_open("ab" * 32).hex(), "0001" + "ab" * 32)
        self.assertEqual(
            encode_grant_revoke("ab" * 32, str(2**64 - 1)).hex(),
            "0001" + "ab" * 32 + "ff" * 8,
        )
        self.assertEqual(
            encode_asset_supply("ab" * 32, "cd" * 32, str(2**128 - 1)).hex(),
            "0001" + "ab" * 32 + "cd" * 32 + "ff" * 16,
        )

    def test_refusals(self):
        for amount in ("0", "01", "-1", str(2**128), 1, True):
            with self.assertRaises(ValueError):
                encode_asset_supply("ab" * 32, "cd" * 32, amount)
        for asset in ("AB" * 32, "ab" * 31, "0x" + "ab" * 32):
            with self.assertRaises(ValueError):
                encode_account_open(asset)
        with self.assertRaises(ValueError):
            encode_grant_revoke("ab" * 32, str(2**64))

    def test_register_derivation_and_version_1_wire_layout(self):
        self.assertEqual(derive_native_asset_id(self.issuer, self.salt), self.asset)
        self.assertEqual(
            encode_asset_register(self.issuer, self.native).hex(),
            "0001"
            + self.asset
            + self.salt
            + "03"
            + "TOK".encode().hex()
            + "09"
            + "Token €".encode().hex()
            + "12"
            + "000000000000000000000000000f4240"
            + "0100",
        )
        custody = self.native | {
            "issuer_kind": 2,
            "asset_id": "ab" * 32,
            "custody_ref": b"\x01\x02",
        }
        self.assertTrue(
            encode_asset_register(self.issuer, custody).endswith(b"\x02\x02\x01\x02")
        )

    def test_register_refusals(self):
        invalid = (
            self.native | {"asset_id": "ab" * 32},
            self.native | {"symbol": ""},
            self.native | {"symbol": "A" * 17},
            self.native | {"symbol": "€"},
            self.native | {"name": ""},
            self.native | {"name": "€" * 11},
            self.native | {"name": "\ud800"},
            self.native | {"decimals": -1},
            self.native | {"decimals": 39},
            self.native | {"decimals": True},
            self.native | {"supply_cap": "01"},
            self.native | {"supply_cap": str(2**128)},
            self.native | {"issuer_kind": 0},
            self.native | {"issuer_kind": 3},
            self.native | {"custody_ref": b"\x01"},
            self.native | {"issuer_kind": 2},
            self.native
            | {"issuer_kind": 2, "asset_id": "ab" * 32, "custody_ref": bytes(129)},
        )
        for registration in invalid:
            with self.assertRaises(ValueError):
                encode_asset_register(self.issuer, registration)
        for identity in ("ab" * 31, "AB" * 32, "00" * 33):
            with self.assertRaises(ValueError):
                encode_asset_register(identity, self.native)
