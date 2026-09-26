from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

from layerx_sdk import (
    AuthorizedReceiptBatch,
    PlatformSdkError,
    ReceiptFailureCode,
    ReceiptVerificationError,
    verify_payment_receipt,
)
from layerx_sdk.x402 import PAYMENT_RECEIPT_PROTOCOL_VERSIONS, receipt_protocol_version

_REPO_ROOT = Path(__file__).resolve().parents[4]
_SIGNATURES_PATH = _REPO_ROOT / "platform" / "integrations" / "fastapi" / "layerx_fastapi" / "signatures.py"
_FIXTURES = _REPO_ROOT / "platform" / "sdk" / "conformance" / "fixtures"


def _signature_verifier_class() -> type:
    spec = importlib.util.spec_from_file_location("layerx_fastapi_signatures", _SIGNATURES_PATH)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load {_SIGNATURES_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LayerXSignatureVerifier


LayerXSignatureVerifier = _signature_verifier_class()


def _fixture(version: int) -> dict:
    return json.loads((_FIXTURES / f"receipt-positive-v{version}.json").read_text(encoding="utf-8"))


def _authorized(fixture: dict) -> AuthorizedReceiptBatch:
    batch = fixture["authorized_batch"]
    return AuthorizedReceiptBatch(
        batch_id=bytes.fromhex(batch["batch_id_hex"]),
        asset=bytes.fromhex(batch["asset_hex"]),
        previous_state_root=bytes.fromhex(batch["previous_state_root_hex"]),
        resulting_state_root=bytes.fromhex(batch["resulting_state_root_hex"]),
        sequencer_public_key=bytes.fromhex(batch["sequencer_public_key_hex"]),
    )


def _verify(fixture: dict, canonical: bytes | None = None, *, payer: str | None = None, amount: str | None = None):
    expected = fixture["expected"]
    return verify_payment_receipt(
        bytes.fromhex(fixture["canonical_receipt_hex"]) if canonical is None else canonical,
        _authorized(fixture),
        LayerXSignatureVerifier(),
        amount=expected["amount"] if amount is None else amount,
        asset=fixture["authorized_batch"]["asset_hex"],
        pay_to=expected["to_hex"],
        payer=expected["from_hex"] if payer is None else payer,
    )


class VerifyPaymentReceiptTest(unittest.TestCase):
    def test_supported_versions(self) -> None:
        self.assertEqual(PAYMENT_RECEIPT_PROTOCOL_VERSIONS, (2, 3))

    def test_version_three_receipt_verifies(self) -> None:
        fixture = _fixture(3)
        verified = _verify(fixture)
        self.assertEqual(verified.receipt.protocol_version, 3)
        self.assertEqual(verified.receipt_digest.hex(), fixture["expected"]["receipt_digest_hex"])
        self.assertEqual(receipt_protocol_version(bytes.fromhex(fixture["canonical_receipt_hex"])), 3)

    def test_version_two_receipt_verifies(self) -> None:
        fixture = _fixture(2)
        verified = _verify(fixture)
        self.assertEqual(verified.receipt.protocol_version, 2)
        self.assertEqual(verified.receipt_digest.hex(), fixture["expected"]["receipt_digest_hex"])
        self.assertEqual(receipt_protocol_version(bytes.fromhex(fixture["canonical_receipt_hex"])), 2)

    def test_unsupported_version_is_refused_by_name(self) -> None:
        fixture = _fixture(1)
        canonical = bytes.fromhex(fixture["canonical_receipt_hex"])
        self.assertEqual((canonical[0:2], canonical[4:6]), (b"\x00\x01", b"\x00\x01"))
        with self.assertRaises(ReceiptVerificationError) as raised:
            _verify(fixture)
        self.assertIs(raised.exception.check, ReceiptFailureCode.PROTOCOL_VERSION)
        with self.assertRaises(ReceiptVerificationError) as raised:
            receipt_protocol_version(canonical)
        self.assertIs(raised.exception.check, ReceiptFailureCode.PROTOCOL_VERSION)
        recorded = bytes.fromhex(_fixture(3)["canonical_receipt_hex"])
        for changed in (b"\x00\x04" + recorded[2:4] + b"\x00\x04" + recorded[6:], b"\x00\x03" + recorded[2:4] + b"\x00\x02" + recorded[6:]):
            with self.assertRaises(ReceiptVerificationError) as raised:
                _verify(_fixture(3), changed)
            self.assertIs(raised.exception.check, ReceiptFailureCode.PROTOCOL_VERSION)
        with self.assertRaises(ReceiptVerificationError) as raised:
            receipt_protocol_version(recorded[:5])
        self.assertIs(raised.exception.check, ReceiptFailureCode.DECODE)

    def test_carried_version_does_not_bypass_the_signature(self) -> None:
        fixture = _fixture(2)
        canonical = bytes.fromhex(fixture["canonical_receipt_hex"])
        downgraded = b"\x00\x02" + canonical[2:4] + b"\x00\x02" + bytes.fromhex(_fixture(3)["canonical_receipt_hex"])[6:]
        with self.assertRaises(ReceiptVerificationError):
            _verify(_fixture(3), downgraded)
        corrupted = canonical[:-1] + bytes([canonical[-1] ^ 1])
        with self.assertRaises(ReceiptVerificationError):
            _verify(fixture, corrupted)

    def test_payment_terms_still_bind(self) -> None:
        for version in (2, 3):
            fixture = _fixture(version)
            with self.assertRaises(PlatformSdkError):
                _verify(fixture, payer="ab" * 32)
            with self.assertRaises(PlatformSdkError):
                _verify(fixture, amount=str(int(fixture["expected"]["amount"]) + 1))


if __name__ == "__main__":
    unittest.main()
