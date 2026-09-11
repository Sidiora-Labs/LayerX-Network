import json
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError, URLError
from layerx_sdk.x402_activity import bind_receive_activity
from layerx_sdk.x402_draw import PreparedGrantDraws
from layerx_sdk.x402_rpc import PaymentRpc, PaymentRpcError
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
                        "payer": r["from"],
                        "purposeHash": r["payer_grant"]["purpose_hash"],
                        "windowSeconds": "3600",
                    }
                },
            )
            body = {"receive": receive.hex(), "idempotencyKey": key}
            with self.assertRaises(URLError):
                store("payer", "ab" * 32, body, offer)
            with self.assertRaises(URLError):
                store("payer", "ab" * 32, body, offer)
            with self.assertRaises(ValueError):
                store.register(
                    "other", "ab" * 32, canonical, receive, key, "subscription:period:1"
                )
            with self.assertRaises(ValueError):
                store.register(
                    "payer", "cd" * 32, canonical, receive, key, "subscription:period:1"
                )

    def test_only_typed_protocol_pending_is_retryable(self):
        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers["Content-Length"])
                request = json.loads(self.rfile.read(length))
                if self.server.mode == "http":
                    self.send_response(503)
                    self.end_headers()
                    return
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                if self.server.mode == "parse":
                    self.wfile.write(b"{")
                    return
                error = (
                    {
                        "code": -32001,
                        "message": "Requested commitment unavailable",
                        "data": {"state": "pending"},
                    }
                    if self.server.mode == "pending"
                    else {
                        "code": -32001,
                        "message": "Requested commitment unavailable",
                        "data": {"state": "completed"},
                    }
                    if self.server.mode == "invalid-pending"
                    else {
                        "code": -32603,
                        "message": "Internal error",
                        "data": {"state": "pending"},
                    }
                )
                self.wfile.write(
                    json.dumps(
                        {"jsonrpc": "2.0", "id": request["id"], "error": error}
                    ).encode()
                )

            def log_message(self, format, *args):
                return

        fixture = Path(__file__).with_name("draw.hex").read_text().splitlines()
        canonical = bytes.fromhex(fixture[0])
        receive = bytes.fromhex(
            Path(__file__).with_name("receive.hex").read_text().splitlines()[0]
        )
        key = "04" + "00" * 31
        trust = test_commitment.CommitmentTests()
        trust.setUp()
        authority = ConfiguredReceiptAuthority(trust.authorized)
        decoded = decode_receive(receive)
        offer = {
            "scheme": "subscription",
            "asset": decoded["asset"],
            "amount": decoded["amount"],
            "payTo": decoded["to"],
            "extra": {
                "layerx": {
                    "commitment": "executed",
                    "payer": decoded["from"],
                    "purposeHash": decoded["payer_grant"]["purpose_hash"],
                    "windowSeconds": "3600",
                }
            },
        }
        body = {"receive": receive.hex(), "idempotencyKey": key}
        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for mode in ("pending", "invalid-pending", "rpc", "http", "parse"):
                server.mode = mode
                with tempfile.TemporaryDirectory() as directory:
                    store = PreparedGrantDraws(
                        str(Path(directory) / "draws.sqlite"),
                        "did:lxp:pay6-receiver",
                        7,
                        PaymentRpc(f"http://127.0.0.1:{server.server_port}/rpc"),
                        authority,
                        trust.signatures,
                    )
                    store.register(
                        "payer",
                        "ab" * 32,
                        canonical,
                        receive,
                        key,
                        "subscription:period:1",
                    )
                    if mode == "pending":
                        self.assertIsNone(store("payer", "ab" * 32, body, offer))
                    elif mode in ("rpc", "invalid-pending"):
                        with self.assertRaises(PaymentRpcError) as raised:
                            store("payer", "ab" * 32, body, offer)
                        self.assertEqual(
                            raised.exception.code,
                            -32603 if mode == "rpc" else -32001,
                        )
                    elif mode == "http":
                        with self.assertRaises(HTTPError):
                            store("payer", "ab" * 32, body, offer)
                    else:
                        with self.assertRaises(json.JSONDecodeError):
                            store("payer", "ab" * 32, body, offer)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()
