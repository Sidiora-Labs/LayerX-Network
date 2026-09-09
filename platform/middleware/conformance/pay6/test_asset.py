import hashlib
import unittest
from pathlib import Path

from layerx_sdk import (
    encode_account_open,
    encode_asset_supply,
    encode_grant_revoke,
    encode_register,
    native_asset_id,
)


class AssetTests(unittest.TestCase):
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

    def test_native_register_derives_kind1_and_matches_signer_vector(self):
        actor = b"did:layerx:alice"
        issuer = hashlib.sha256(b"LXP/v1/did-id\0" + len(actor).to_bytes(2, "big") + actor).hexdigest()
        salt = "02" * 32
        asset = native_asset_id(issuer, salt)
        self.assertEqual(
            asset,
            hashlib.sha256(b"LX:ASSET:v1" + bytes.fromhex(issuer) + bytes.fromhex(salt)).hexdigest(),
        )
        wire = encode_register(
            {
                "issuer_did_id32": issuer,
                "salt": salt,
                "symbol": "TOK",
                "name": "Token €",
                "decimals": 18,
                "supply_cap": "1000000",
                "issuer_kind": 1,
                "custody_ref": "",
            }
        )
        fixture = (
            Path(__file__).resolve().parents[4]
            / "agent/crates/layerx-crypto/tests/fixtures/payments/1-1.hex"
        ).read_text().strip()
        self.assertEqual(wire.hex(), fixture)
        self.assertEqual(
            encode_register(
                {
                    "issuer_did_id32": issuer,
                    "salt": salt,
                    "symbol": "TOK",
                    "name": "Token €",
                    "decimals": 18,
                    "supply_cap": "1000000",
                    "issuer_kind": 1,
                    "custody_ref": "",
                    "asset_id": asset,
                }
            ),
            wire,
        )

    def test_register_refusals(self):
        issuer = "ab" * 32
        salt = "cd" * 32
        valid = {
            "issuer_did_id32": issuer,
            "salt": salt,
            "symbol": "TOK",
            "name": "Token",
            "decimals": 6,
            "supply_cap": "0",
            "issuer_kind": 1,
            "custody_ref": "",
        }
        self.assertEqual(encode_register(valid)[2:34], bytes.fromhex(native_asset_id(issuer, salt)))
        for change in (
            {"symbol": ""},
            {"symbol": "X" * 17},
            {"symbol": "T€K"},
            {"name": ""},
            {"name": "n" * 33},
            {"decimals": 39},
            {"decimals": -1},
            {"issuer_kind": 3},
            {"issuer_kind": 0},
            {"custody_ref": "aa"},
            {"asset_id": "00" * 32},
            {"supply_cap": "01"},
            {"supply_cap": str(2**128)},
        ):
            with self.assertRaises(ValueError):
                encode_register(valid | change)
        custody = {
            **valid,
            "issuer_kind": 2,
            "asset_id": "11" * 32,
            "custody_ref": "aa" * 128,
        }
        encoded = encode_register(custody)
        self.assertEqual(encoded[2:34], bytes.fromhex("11" * 32))
        self.assertEqual(encoded[-128:], bytes.fromhex("aa" * 128))
        with self.assertRaises(ValueError):
            encode_register({**valid, "issuer_kind": 2})
        with self.assertRaises(ValueError):
            encode_register({**custody, "custody_ref": "aa" * 129})
