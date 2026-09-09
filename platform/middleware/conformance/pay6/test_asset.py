import unittest
from layerx_sdk.x402_receive import (
    encode_account_open,
    encode_grant_revoke,
    encode_asset_supply,
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
