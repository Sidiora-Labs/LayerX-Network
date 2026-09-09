import tempfile
import unittest
from pathlib import Path
from layerx_sdk.x402_activity import bind_receive_activity
from layerx_sdk.x402_draw import PreparedGrantDraws
from layerx_sdk.x402_rpc import PaymentRpc
from layerx_sdk.x402_http import ConfiguredReceiptAuthority
import test_commitment
from layerx_sdk.x402_receive import decode_receive


class DrawTests(unittest.TestCase):
    def test_native_envelope_and_registration(self):
        fixture = Path(__file__).with_name("draw.hex").read_text().splitlines()
        canonical = bytes.fromhex(fixture[0])
        receive = bytes.fromhex(
            Path(__file__).with_name("receive.hex").read_text().splitlines()[0]
        )
        key = "04" + "00" * 31
        self.assertEqual(
            bind_receive_activity(canonical, receive, "did:lxp:pay6-receiver", 7, key),
            fixture[1],
        )
        for length in range(len(canonical)):
            with self.assertRaises(ValueError):
                bind_receive_activity(
                    canonical[:length], receive, "did:lxp:pay6-receiver", 7, key
                )
        trust = test_commitment.CommitmentTests()
        trust.setUp()
        with tempfile.TemporaryDirectory() as directory:
            store = PreparedGrantDraws(
                str(Path(directory) / "draws.sqlite"),
                "did:lxp:pay6-receiver",
                7,
                PaymentRpc("http://127.0.0.1:1/rpc"),
                ConfiguredReceiptAuthority(trust.authorized),
                trust.signatures,
            )
            store.register(
                "payer", "ab" * 32, canonical, receive, key, "subscription:period:1"
            )
            store.register(
                "payer", "ab" * 32, canonical, receive, key, "subscription:period:1"
            )
            r = decode_receive(receive)
            offer = dict(
                scheme="subscription",
                asset=r["asset"],
                amount=r["amount"],
                payTo=r["to"],
                extra={
                    "layerx": {
                        "commitment": "executed",
                        "purposeHash": r["payer_grant"]["purpose_hash"],
                        "windowSeconds": "3600",
                    }
                },
            )
            body = {"receive": receive.hex(), "idempotencyKey": key}
            self.assertIsNone(store("payer", "ab" * 32, body, offer))
            self.assertIsNone(store("payer", "ab" * 32, body, offer))
            with self.assertRaises(ValueError):
                store.register(
                    "other", "ab" * 32, canonical, receive, key, "subscription:period:1"
                )
            with self.assertRaises(ValueError):
                store.register(
                    "payer", "cd" * 32, canonical, receive, key, "subscription:period:1"
                )
