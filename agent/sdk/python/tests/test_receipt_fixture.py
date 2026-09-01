from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest

from layerx_sdk import (
    AuthorizedReceiptBatch,
    PlatformSdkError,
    ReceiptVerificationError,
    verify_receipt,
)
from layerx_sdk.verifier import decode_program_receipt_outcome

_PROGRAM_OUTCOME_V3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000"

_REPO_ROOT = Path(__file__).resolve().parents[4]
_SIGNATURES_PATH = (
    _REPO_ROOT / "platform" / "integrations" / "fastapi" / "layerx_fastapi" / "signatures.py"
)
_FIXTURE_PATH = (
    _REPO_ROOT / "platform" / "sdk" / "conformance" / "fixtures" / "receipt-positive-v2.json"
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


def _load_shared_fixture(name: str) -> dict:
    return json.loads((_FIXTURE_PATH.parent / name).read_text(encoding="utf-8"))


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
    def test_program_outcome_v3_vector(self) -> None:
        outcome = decode_program_receipt_outcome(bytes.fromhex(_PROGRAM_OUTCOME_V3), 1)
        self.assertEqual(outcome.encoding_version, 3)
        self.assertEqual(outcome.abi_version, 1)
        self.assertEqual(outcome.fee_units, 16)
        self.assertEqual(outcome.call_graph_root, bytes.fromhex("11" * 32))
        self.assertEqual(outcome.terminal_payload_root, bytes.fromhex("22" * 32))

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

    def test_core_programs_fixture_preserves_optional_outcome(self) -> None:
        fixture = _load_shared_fixture("receipt-programs-positive-v2.json")
        verified = verify_receipt(
            bytes.fromhex(fixture["canonical_receipt_hex"]),
            _authorized(fixture),
            LayerXSignatureVerifier(),
        )
        outcome = verified.receipt.program_outcome
        self.assertIsNotNone(outcome)
        assert outcome is not None
        self.assertEqual(outcome.encoding_version, 3)
        self.assertEqual(outcome.runtime_version, 1)
        self.assertEqual(outcome.abi_version, 1)
        self.assertEqual(outcome.occupancy_byte_batches, 2)
        self.assertEqual(outcome.occupancy_fee_units, 7)
        self.assertEqual(
            outcome.occupancy_asset_id,
            bytes.fromhex(fixture["authorized_batch"]["asset_hex"]),
        )
        self.assertNotEqual(outcome.occupancy_evidence_digest, bytes(32))
        self.assertNotEqual(outcome.occupancy_transfer_root, bytes(32))
        self.assertEqual(outcome.fee_units, 16)

    def test_core_refusal_vectors_expose_shared_taxonomy(self) -> None:
        fixture = _load_shared_fixture("receipt-refusals-v2.json")
        authorized = _authorized(fixture)
        for vector in fixture["vectors"]:
            with self.subTest(vector=vector["name"]):
                with self.assertRaises(ReceiptVerificationError) as refused:
                    verify_receipt(
                        bytes.fromhex(vector["canonical_receipt_hex"]),
                        authorized,
                        LayerXSignatureVerifier(),
                    )
                self.assertEqual(refused.exception.check.value, vector["expected_check"])


if __name__ == "__main__":
    unittest.main()
