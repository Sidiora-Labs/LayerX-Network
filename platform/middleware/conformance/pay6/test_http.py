import base64
import hashlib
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import test_commitment
from layerx_sdk.x402_http import (
    SellerMiddleware,
    BuyerMiddleware,
    FulfillmentStore,
    ConfiguredReceiptAuthority,
    encode_header,
    decode_header,
    validate_required,
)
from layerx_sdk.x402_rpc import PaymentRpc


class HttpTests(unittest.TestCase):
    def setUp(self):
        self.fixture = test_commitment.CommitmentTests()
        self.fixture.setUp()
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        f = self.fixture
        self.offer = dict(
            scheme="exact",
            network="layerx:testnet",
            amount=f.offer["amount"],
            asset=f.offer["asset"],
            payTo=f.offer["pay_to"],
            maxTimeoutSeconds=30,
        )
        self.required = dict(
            x402Version=2,
            resource={"url": "https://example.com/paid"},
            accepts=[self.offer],
        )
        self.payload = dict(
            x402Version=2,
            resource=self.required["resource"],
            accepted=self.offer,
            payload={
                "receipt": base64.b64encode(f.wire).decode(),
                "receiptDigest": hashlib.sha256(
                    b"LXP/v1/merkle-leaf\0" + f.wire
                ).hexdigest(),
                "verificationLevel": "sequencer-signed",
            },
        )
        self.authority = ConfiguredReceiptAuthority(f.authorized)
        self.seller = SellerMiddleware(
            self.required,
            f.signatures,
            self.authority,
            FulfillmentStore(str(Path(self.directory.name) / "payments.sqlite")),
        )

    def test_real_receipt_release_retry_and_buyer_capture(self):
        status, headers, _ = self.seller.handle("payer", None, lambda: b"resource")
        self.assertEqual(status, 402)
        self.assertEqual(decode_header(headers["PAYMENT-REQUIRED"]), self.required)
        header = encode_header(self.payload)
        first = self.seller.handle("payer", header, lambda: b"resource")
        retry = self.seller.handle("payer", header, lambda: b"changed-resource")
        self.assertEqual(first, retry)
        self.assertEqual(first[0], 200)
        buyer = BuyerMiddleware(
            PaymentRpc("http://127.0.0.1:1/rpc"),
            self.fixture.signatures,
            self.authority,
            [("exact", "layerx:testnet")],
        )
        verified = buyer.capture_settlement(first[1]["PAYMENT-RESPONSE"], header)
        self.assertEqual(verified.canonical_bytes, self.fixture.wire)
        with self.assertRaises(ValueError):
            self.seller.handle("different-payer", header, lambda: b"resource")

    def test_concurrent_retry_persists_one_resource(self):
        header = encode_header(self.payload)
        with ThreadPoolExecutor(max_workers=4) as pool:
            responses = list(
                pool.map(
                    lambda _: self.seller.handle("payer", header, lambda: b"resource"),
                    range(8),
                )
            )
        self.assertTrue(all(response == responses[0] for response in responses))

    def test_offer_mismatch_and_invalid_headers(self):
        for header in (
            "!",
            encode_header({}),
            base64.b64encode(b'{"a":1,"a":2}').decode(),
            base64.b64encode(b" " * 65537).decode(),
        ):
            with self.assertRaises((ValueError, KeyError)):
                self.seller.handle("payer", header, lambda: b"resource")
        for change in (
            {"amount": "1"},
            {"asset": "ab" * 32},
            {"extra": {"layerx": {"commitment": "finalised"}}},
        ):
            with self.assertRaises(ValueError):
                self.seller.handle(
                    "payer",
                    encode_header(self.payload | {"accepted": self.offer | change}),
                    lambda: b"resource",
                )
        for scheme in ("metered", "subscription", "unknown"):
            with self.assertRaises(ValueError):
                validate_required(
                    self.required | {"accepts": [self.offer | {"scheme": scheme}]}
                )
