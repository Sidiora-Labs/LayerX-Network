import unittest
from pathlib import Path
from layerx_sdk.x402_receive import decode_receive
from layerx_sdk.x402_grant import validate_grant_draw


class GrantTests(unittest.TestCase):
    def test_subscription_native_payload_and_refusals(self):
        wire = bytes.fromhex(
            Path(__file__).with_name("receive.hex").read_text().splitlines()[0]
        )
        r = decode_receive(wire)
        offer = dict(
            scheme="subscription",
            asset=r["asset"],
            amount=r["amount"],
            payTo=r["to"],
            extra={
                "layerx": {
                    "commitment": "executed",
                    "purposeHash": r["payer_grant"]["purpose_hash"],
                    "payer": r["from"],
                    "windowSeconds": "3600",
                }
            },
        )
        self.assertEqual(
            validate_grant_draw(wire, offer, r["idempotency_key"], 7, 0), r
        )
        for change in (
            {"amount": "1"},
            {"asset": "ab" * 32},
            {"payTo": "ab" * 32},
            {"scheme": "metered"},
            {
                "extra": {
                    "layerx": offer["extra"]["layerx"] | {"payer": "ab" * 32}
                }
            },
        ):
            with self.assertRaises(ValueError):
                validate_grant_draw(wire, offer | change, r["idempotency_key"], 7, 0)
        for key, network, now in (
            ("00" * 32, 7, 0),
            (r["idempotency_key"], 8, 0),
            (r["idempotency_key"], 7, int(r["payer_grant"]["expiration"])),
        ):
            with self.assertRaises(ValueError):
                validate_grant_draw(wire, offer, key, network, now)

    def test_buyer_grant_header(self):
        from layerx_sdk.x402_http import grant_payment_header, decode_header

        wire = bytes.fromhex(
            Path(__file__).with_name("receive.hex").read_text().splitlines()[0]
        )
        r = decode_receive(wire)
        offer = dict(
            scheme="subscription",
            network="layerx:testnet",
            maxTimeoutSeconds=30,
            asset=r["asset"],
            amount=r["amount"],
            payTo=r["to"],
            extra={
                "layerx": {
                    "commitment": "executed",
                    "purposeHash": r["payer_grant"]["purpose_hash"],
                    "payer": r["from"],
                    "windowSeconds": "3600",
                }
            },
        )
        required = dict(
            x402Version=2, resource={"url": "https://example.com/paid"}, accepts=[offer]
        )
        payload = decode_header(grant_payment_header(required, offer, wire.hex()))
        self.assertEqual(payload["accepted"], offer)
        self.assertEqual(payload["payload"]["idempotencyKey"], r["idempotency_key"])
        with self.assertRaises(ValueError):
            grant_payment_header(required, offer | {"amount": "1"}, wire.hex())
