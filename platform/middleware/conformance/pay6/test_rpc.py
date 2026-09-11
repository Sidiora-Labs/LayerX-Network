import json
import unittest
from pathlib import Path
from test_commitment import CommitmentTests
from layerx_sdk.x402_rpc import (
    verify_rpc_payment,
    rpc_batch_evidence,
    PaymentRpc,
)
from layerx_sdk.verifier import SequencerAuthorization


class RpcTests(unittest.TestCase):
    def test_receipt_binding_and_pending(self):
        fixture = CommitmentTests()
        fixture.setUp()
        verified = fixture.verify()
        activity = verified.receipt.activity_id.hex()
        payer = verified.receipt.from_account.hex()
        result = {"activity_id": activity, "receipt": fixture.wire.hex()}

        def verify(body, expected_payer=payer):
            return verify_rpc_payment(
                body,
                activity,
                expected_payer,
                fixture.authorized,
                fixture.signatures,
                **fixture.offer,
            )

        self.assertEqual(verify(result), verified)
        self.assertIsNone(verify({"activity_id": activity, "state": "pending"}))
        for change in (
            {"activity_id": "ab" * 32},
            {"state": "acknowledged"},
            {"commitment": "batched"},
        ):
            with self.assertRaises(ValueError):
                verify(result | change)
        with self.assertRaises(ValueError):
            verify(result, "ab" * 32)
        with self.assertRaises(ValueError):
            verify({"activity_id": activity})

    def test_endpoints(self):
        for endpoint in (
            "http://example.com/rpc",
            "https://user:password@example.com/rpc",
            "https://example.com/rpc?token=1",
            "https://example.com/",
        ):
            with self.assertRaises(ValueError):
                PaymentRpc(endpoint)
        with self.assertRaises(ValueError):
            PaymentRpc("http://127.0.0.1:1/rpc").send("00", "finalized")

    def test_batched_result_uses_configured_sequencer_authority(self):
        fixture = CommitmentTests()
        fixture.setUp()
        verified = fixture.verify()
        activity = verified.receipt.activity_id.hex()
        payer = verified.receipt.from_account.hex()
        batch = json.loads(Path(__file__).with_name("batch.json").read_text())
        result = {
            "activity_id": activity,
            "receipt": fixture.wire.hex(),
            "commitment": "batched",
            "batch_evidence": {
                "kind": "receipt",
                "activity_id": activity,
                "canonical_value": fixture.wire.hex(),
                "proof": {"leaf_index": 0, "leaf_count": 1, "siblings": []},
                "signed_header": {
                    "canonical_header": batch["header"],
                    "signature": batch["signature"],
                    "sequencer_id": batch["sequencer_id"],
                    "public_key": fixture.authorized.sequencer_public_key.hex(),
                },
            },
        }
        authorization = SequencerAuthorization(
            bytes.fromhex(batch["sequencer_id"]),
            fixture.authorized.sequencer_public_key,
            1,
            1,
        )
        evidence = rpc_batch_evidence(result, activity, fixture.wire, 7, authorization)
        self.assertEqual(
            verify_rpc_payment(
                result,
                activity,
                payer,
                fixture.authorized,
                fixture.signatures,
                **fixture.offer,
                commitment="batched",
                evidence=evidence,
            ),
            verified,
        )
        forged = json.loads(json.dumps(result))
        forged["batch_evidence"]["signed_header"]["public_key"] = "00" * 32
        with self.assertRaises(ValueError):
            rpc_batch_evidence(forged, activity, fixture.wire, 7, authorization)
