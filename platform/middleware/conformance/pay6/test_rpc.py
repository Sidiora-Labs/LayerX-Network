import unittest
from test_commitment import CommitmentTests
from layerx_sdk.x402_rpc import verify_rpc_payment, PaymentRpc


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
                amount=fixture.offer["amount"],
                asset=fixture.offer["asset"],
                pay_to=fixture.offer["pay_to"],
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
