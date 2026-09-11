import importlib.util
import json
from dataclasses import replace
from pathlib import Path
import unittest

from layerx_sdk.x402 import (
    PaymentCommitmentEvidence,
    payment_commitment,
    verify_payment_commitment_evidence,
    verify_payment_receipt,
)
from layerx_sdk.verifier import (
    AuthorizedReceiptBatch,
    MerkleProof,
    SequencerAuthorization,
)
from layerx_sdk.production import PlatformSdkError

ROOT = Path(__file__).resolve().parents[4]
FIXTURE = json.loads(
    (ROOT / "platform/sdk/conformance/fixtures/receipt-positive-v2.json").read_text()
)
SPEC = importlib.util.spec_from_file_location(
    "signatures", ROOT / "platform/integrations/fastapi/layerx_fastapi/signatures.py"
)
SIGNATURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SIGNATURES)


class CommitmentTests(unittest.TestCase):
    def setUp(self):
        self.signatures = SIGNATURES.LayerXSignatureVerifier()
        self.wire = bytes.fromhex(FIXTURE["canonical_receipt_hex"])
        batch = FIXTURE["authorized_batch"]
        self.authorized = AuthorizedReceiptBatch(
            **{k.removesuffix("_hex"): bytes.fromhex(v) for k, v in batch.items()}
        )
        self.offer = dict(
            amount=FIXTURE["expected"]["amount"],
            asset=batch["asset_hex"],
            pay_to=FIXTURE["expected"]["to_hex"],
        )

    def verify(self, **changes):
        return verify_payment_receipt(
            self.wire, self.authorized, self.signatures, **(self.offer | changes)
        )

    def test_executed_payment_binding(self):
        payer = self.verify().receipt.from_account.hex()
        self.verify(payer=payer)
        for change in (
            {"amount": "25001"},
            {"asset": "01" * 32},
            {"pay_to": "01" * 32},
        ):
            with self.assertRaises(PlatformSdkError):
                self.verify(**change)
        for invalid_payer in ("01" * 32, "00" * 32):
            with self.assertRaises(PlatformSdkError):
                self.verify(payer=invalid_payer)
        with self.assertRaises(PlatformSdkError):
            verify_payment_receipt(
                self.wire[:-1] + bytes([self.wire[-1] ^ 1]),
                self.authorized,
                self.signatures,
                **self.offer,
            )

    def test_no_commitment_downgrade(self):
        for commitment in (
            "batched",
            "finalised",
            "finalized",
            "acknowledged",
            None,
            1,
        ):
            with self.assertRaises(PlatformSdkError):
                self.verify(commitment=commitment)
        self.assertEqual(payment_commitment(), "executed")
        for value in (None, [], {}, "executed"):
            with self.assertRaises(PlatformSdkError):
                payment_commitment({"layerx": value})

    def test_real_signed_batch(self):
        fixture = json.loads(Path(__file__).with_name("batch.json").read_text())
        proof = PaymentCommitmentEvidence(
            7,
            bytes.fromhex(fixture["header"]),
            bytes.fromhex(fixture["signature"]),
            SequencerAuthorization(
                bytes.fromhex(fixture["sequencer_id"]),
                self.authorized.sequencer_public_key,
                1,
                1,
            ),
            MerkleProof(0, 1, ()),
        )
        verified = self.verify(commitment="batched", evidence=proof)
        with self.assertRaises(PlatformSdkError):
            self.verify(commitment="finalised", evidence=proof)
        for invalid in (
            replace(proof, network_id=8),
            replace(proof, header_signature=bytes(64)),
            replace(proof, proof=MerkleProof(1, 1, ())),
        ):
            with self.assertRaises(PlatformSdkError):
                self.verify(commitment="batched", evidence=invalid)
        with self.assertRaises(PlatformSdkError):
            verify_payment_commitment_evidence(
                verified, bytes(32), "batched", proof, self.signatures
            )
        with self.assertRaises(PlatformSdkError):
            verify_payment_commitment_evidence(
                replace(verified, receipt=replace(verified.receipt, global_sequence=2)),
                self.authorized.sequencer_public_key,
                "batched",
                proof,
                self.signatures,
            )


if __name__ == "__main__":
    unittest.main()
