from __future__ import annotations

import json
import unittest
from hashlib import sha256
from pathlib import Path

from layerx_sdk import AuthorizedReceiptBatch, PlatformSdkError, verify_receipt
from layerx_sdk.verifier import _decode_protocol_receipt
from test_receipt_fixture import LayerXSignatureVerifier

ROOT = Path(__file__).resolve().parents[4]
NATIVE = ROOT / "tests/fixtures/asset/daemon-send-supply"


def authority(canonical, public_key):
    receipt, _ = _decode_protocol_receipt(canonical)
    return AuthorizedReceiptBatch(receipt.batch_id, receipt.asset, receipt.previous_state_root,
                                  receipt.resulting_state_root, public_key)


class SupplyReceiptTests(unittest.TestCase):
    def setUp(self):
        self.signatures = LayerXSignatureVerifier()

    def test_actual_native_send_verifies_supply_fee_and_debit(self):
        canonical = (NATIVE / "receipt").read_bytes()
        expected = json.loads((NATIVE / "expected.json").read_text())
        bound = authority(canonical, (NATIVE / "sequencer.public").read_bytes())
        verified = verify_receipt(canonical, bound, self.signatures, protocol_version=3)
        receipt = verified.receipt
        self.assertEqual(receipt.activity_id.hex(), expected["activity_id"])
        self.assertEqual((receipt.module_id, receipt.operation, receipt.result_code), (1, 5, 0))
        self.assertEqual(receipt.from_account.hex(), expected["source_account"])
        self.assertEqual(receipt.to_account.hex(), expected["destination_account"])
        self.assertEqual(receipt.asset.hex(), expected["asset"])
        self.assertEqual(receipt.amount, 1)
        self.assertEqual(receipt.from_sequence, int(expected["source_sequence_before"]))
        self.assertEqual(receipt.fee_charged, 4)
        self.assertEqual(receipt.from_balance_before + receipt.fee_charged,
                         int(expected["source_balance_before"]))
        self.assertEqual(receipt.to_balance_before, int(expected["destination_balance_before"]))
        self.assertEqual(receipt.total_units, (0, 0))
        self.assertEqual(verified.receipt_digest,
                         sha256(b"LXP/v1/receipt\0" + canonical[:-69] + b"\0").digest())
        with self.assertRaises(PlatformSdkError):
            verify_receipt(canonical, bound, self.signatures)

    def test_original_native_supply_fixture_still_verifies(self):
        canonical = (ROOT / "tests/fixtures/receipt-supply-v2.bin").read_bytes()
        fixture = json.loads((ROOT / "platform/sdk/conformance/fixtures/receipt-positive-v2.json").read_text())
        bound = authority(canonical, bytes.fromhex(fixture["authorized_batch"]["sequencer_public_key_hex"]))
        verified = verify_receipt(canonical, bound, self.signatures)
        self.assertEqual(verified.receipt.total_units, (1_000_000, 1_000_000))
        self.assertEqual(verified.receipt.amount, 25_000)

    def test_canonical_native_pause_and_unpause_supply_fields(self):
        fixture = json.loads((ROOT / "platform/sdk/conformance/fixtures/receipt-positive-v2.json").read_text())
        public_key = bytes.fromhex(fixture["authorized_batch"]["sequencer_public_key_hex"])
        for name, operation in (("pause", 2), ("unpause", 3)):
            canonical = (ROOT / "tests/fixtures/asset" / f"supply-{name}.receipt").read_bytes()
            verified = verify_receipt(canonical, authority(canonical, public_key), self.signatures)
            self.assertEqual(verified.receipt.operation, operation)
            self.assertEqual(verified.receipt.amount, 0)
            self.assertEqual(verified.receipt.total_units, (1_000_000, 1_000_000))

    def test_supply_totals_cannot_be_changed_or_downgraded(self):
        canonical = (NATIVE / "receipt").read_bytes()
        bound = authority(canonical, (NATIVE / "sequencer.public").read_bytes())
        supply_offset = len(canonical) - 69 - 32
        changed = bytearray(canonical)
        changed[supply_offset + 15] ^= 1
        with self.assertRaises(PlatformSdkError):
            _decode_protocol_receipt(bytes(changed))
        changed[supply_offset + 31] ^= 1
        decoded, _ = _decode_protocol_receipt(bytes(changed))
        self.assertEqual(decoded.total_units[0], decoded.total_units[1])
        with self.assertRaises(PlatformSdkError):
            verify_receipt(bytes(changed), bound, self.signatures, protocol_version=3)
        downgraded = bytearray(canonical)
        downgraded[3] = 1
        del downgraded[supply_offset:supply_offset + 32]
        self.assertIsNone(_decode_protocol_receipt(bytes(downgraded))[0].total_units)
        with self.assertRaises(PlatformSdkError):
            verify_receipt(bytes(downgraded), bound, self.signatures, protocol_version=3)
        for size in range(len(canonical)):
            with self.subTest(size=size), self.assertRaises(PlatformSdkError):
                verify_receipt(canonical[:size], bound, self.signatures, protocol_version=3)
        for offset in (0, 1, 2, 3, 4, 5, len(canonical) - 1):
            changed = bytearray(canonical)
            changed[offset] ^= 1
            with self.subTest(offset=offset), self.assertRaises(PlatformSdkError):
                verify_receipt(bytes(changed), bound, self.signatures, protocol_version=3)
        with self.assertRaises(PlatformSdkError):
            verify_receipt(canonical + b"\0", bound, self.signatures, protocol_version=3)
