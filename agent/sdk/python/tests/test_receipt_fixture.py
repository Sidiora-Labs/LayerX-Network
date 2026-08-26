from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest

from layerx_sdk import AuthorizedReceiptBatch, PlatformSdkError, verify_receipt

_REPO_ROOT = Path(__file__).resolve().parents[4]
_SIGNATURES_PATH = (
    _REPO_ROOT / "platform" / "integrations" / "fastapi" / "layerx_fastapi" / "signatures.py"
)
_FIXTURE_PATH = (
    _REPO_ROOT / "platform" / "sdk" / "conformance" / "fixtures" / "receipt-positive-v1.json"
)


def _signature_verifier_class() -> type:
    spec = importlib.util.spec_from_file_location(
        "layerx_fastapi_signatures", _SIGNATURES_PATH
    )
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load {_SIGNATURES_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LayerXSignatureVerifier


LayerXSignatureVerifier = _signature_verifier_class()


def _load_fixture() -> dict:
    return json.loads(_FIXTURE_PATH.read_text(encoding="utf-8"))


def _authorized(fixture: dict) -> AuthorizedReceiptBatch:
    batch = fixture["authorized_batch"]
    return AuthorizedReceiptBatch(
        batch_id=bytes.fromhex(batch["batch_id_hex"]),
        asset=bytes.fromhex(batch["asset_hex"]),
        previous_state_root=bytes.fromhex(batch["previous_state_root_hex"]),
        resulting_state_root=bytes.fromhex(batch["resulting_state_root_hex"]),
        sequencer_public_key=bytes.fromhex(batch["sequencer_public_key_hex"]),
    )


class ReceiptFixtureTest(unittest.TestCase):
    def test_core_fixture_receipt_verifies_positively(self) -> None:
        fixture = _load_fixture()
        expected = fixture["expected"]
        canonical = bytes.fromhex(fixture["canonical_receipt_hex"])
        verified = verify_receipt(
            canonical, _authorized(fixture), LayerXSignatureVerifier()
        )
        self.assertEqual(verified.level, expected["level"])
        self.assertEqual(verified.canonical_bytes, canonical)
        self.assertEqual(
            verified.receipt_digest, bytes.fromhex(expected["receipt_digest_hex"])
        )
        receipt = verified.receipt
        self.assertEqual(receipt.result_code, expected["result_code"])
        self.assertEqual(receipt.protocol_version, expected["protocol_version"])
        self.assertEqual(receipt.operation, expected["operation"])
        self.assertEqual(receipt.module_id, expected["module_id"])
        self.assertEqual(receipt.global_sequence, expected["global_sequence"])
        self.assertEqual(receipt.timestamp, expected["timestamp_ms"])
        self.assertEqual(receipt.amount, int(expected["amount"]))
        self.assertEqual(receipt.fee_charged, int(expected["fee_charged"]))
        self.assertEqual(
            receipt.from_balance_before, int(expected["from_balance_before"])
        )
        self.assertEqual(
            receipt.from_balance_after, int(expected["from_balance_after"])
        )
        self.assertEqual(receipt.to_balance_before, int(expected["to_balance_before"]))
        self.assertEqual(receipt.to_balance_after, int(expected["to_balance_after"]))
        self.assertEqual(receipt.activity_id, bytes.fromhex(expected["activity_id_hex"]))
        self.assertEqual(receipt.from_account, bytes.fromhex(expected["from_hex"]))
        self.assertEqual(receipt.to_account, bytes.fromhex(expected["to_hex"]))
        self.assertEqual(
            receipt.batch_id, bytes.fromhex(fixture["authorized_batch"]["batch_id_hex"])
        )
        self.assertEqual(
            receipt.asset, bytes.fromhex(fixture["authorized_batch"]["asset_hex"])
        )

    def test_core_fixture_receipt_byte_flip_fails(self) -> None:
        fixture = _load_fixture()
        mutated = bytearray(bytes.fromhex(fixture["canonical_receipt_hex"]))
        mutated[-1] ^= 0x01
        with self.assertRaises(PlatformSdkError):
            verify_receipt(
                bytes(mutated), _authorized(fixture), LayerXSignatureVerifier()
            )


if __name__ == "__main__":
    unittest.main()
