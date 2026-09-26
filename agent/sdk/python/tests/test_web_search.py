from __future__ import annotations

import base64
import copy
import importlib.util
import json
import struct
import threading
import unittest
from collections.abc import Callable
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from layerx_sdk import (
    AuthorizedReceiptBatch,
    WebSearchAssetTerms,
    WebSearchClient,
    WebSearchError,
    WebSearchExactPayment,
    WebSearchMeteredPayment,
    WebSearchOffer,
    WebSearchPreference,
    content_digest,
    decode_payer_grant,
    decode_web_content,
    encode_grant,
    web_content_bytes,
)

_REPO_ROOT = Path(__file__).resolve().parents[4]
_FIXTURES = _REPO_ROOT / "interop" / "crates" / "x-websearch" / "tests" / "fixtures"
_SIGNATURES_PATH = _REPO_ROOT / "platform" / "integrations" / "fastapi" / "layerx_fastapi" / "signatures.py"


def _signature_verifier_class() -> type:
    spec = importlib.util.spec_from_file_location("layerx_fastapi_signatures", _SIGNATURES_PATH)
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load {_SIGNATURES_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LayerXSignatureVerifier


LayerXSignatureVerifier = _signature_verifier_class()


def _fixture(path: str) -> dict:
    return json.loads((_FIXTURES / path).read_text(encoding="utf-8"))


EXCHANGE = _fixture("client-exchange.json")["exchange"]
VECTORS = _fixture("content-vectors.json")["vectors"]
BUYER = _fixture("gateway/buyer.json")
CONFIG = _fixture("config/valid.json")
ASSETS = CONFIG["assets"]
TRUSTED_KEY = BUYER["sequencerPublicKey"]
UNTRUSTED_KEY = CONFIG["gateway"]["sequencer_public_key"]
PAYER_DID = BUYER["payerDid"]
CURRENCIES = ("SID", "PAX", "USDC", "USDL")
SEARCH_CHALLENGE, SEARCH_PAID, FETCH_CHALLENGE, FETCH_PAID = EXCHANGE


def _batch_facts(receipt: bytes, sequencer_public_key: str) -> AuthorizedReceiptBatch:
    offset = 6

    def bounded() -> bytes:
        nonlocal offset
        (length,) = struct.unpack_from(">I", receipt, offset)
        offset += 4
        value = receipt[offset:offset + length]
        offset += length
        return value

    bounded()
    offset += 8
    previous = bounded()
    resulting = bounded()
    bounded()
    offset += 4
    (effects,) = struct.unpack_from(">I", receipt, offset)
    offset += 4
    for _ in range(effects):
        offset += 8
        bounded()
        bounded()
    offset += 16
    batch_id = bounded()
    offset += 11
    asset = bounded()
    return AuthorizedReceiptBatch(batch_id=batch_id, asset=asset, previous_state_root=previous,
                                  resulting_state_root=resulting, sequencer_public_key=bytes.fromhex(sequencer_public_key))


def _header_value(value: object) -> str:
    return value if isinstance(value, str) else base64.b64encode(json.dumps(value).encode("utf-8")).decode("ascii")


class _Replay:
    def __init__(self, steps: list[dict], content: dict[str, bytes] | None = None) -> None:
        self.served: list[int] = []
        self.unrecorded: list[dict[str, str]] = []
        replay = self
        stored = content or {}

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, format: str, *args: object) -> None:
                return

            def _matches(self, step: dict) -> bool:
                if step["request"]["method"] != "GET" or step["request"]["target"] != self.path:
                    return False
                for name in ("LAYERX-PAYER-DID", "PAYMENT-SIGNATURE"):
                    sent = self.headers.get(name)
                    recorded = step["request"]["headers"].get(name)
                    if (sent is None) != (recorded is None):
                        return False
                    if recorded is None:
                        continue
                    if name == "PAYMENT-SIGNATURE":
                        if json.loads(base64.b64decode(sent)) != recorded:
                            return False
                    elif sent != recorded:
                        return False
                return True

            def do_GET(self) -> None:
                index = next((position for position, step in enumerate(steps)
                              if position not in replay.served and self._matches(step)), None)
                if index is None:
                    if self.path in stored:
                        self._send(200, {"content-type": "application/octet-stream"}, stored[self.path])
                        return
                    replay.unrecorded.append({name.lower(): value for name, value in self.headers.items()})
                    self._send(400, {"content-type": "application/json"}, json.dumps({"error": "unrecorded_request"}).encode())
                    return
                replay.served.append(index)
                step = steps[index]["response"]
                headers = {"content-type": "application/json"}
                headers.update({name: _header_value(value) for name, value in step["headers"].items()})
                self._send(step["status"], headers, json.dumps(step["body"]).encode("utf-8"))

            def _send(self, status: int, headers: dict[str, str], body: bytes) -> None:
                self.send_response(status)
                for name, value in headers.items():
                    self.send_header(name, value)
                self.send_header("content-length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self._server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()
        self.endpoint = f"http://127.0.0.1:{self._server.server_address[1]}"

    def __enter__(self) -> _Replay:
        return self

    def __exit__(self, *_: object) -> None:
        self._server.shutdown()
        self._server.server_close()
        self._thread.join()


@dataclass
class _Payer:
    preferences: tuple[WebSearchPreference, ...]
    did: str | None = None
    grant: Callable[[WebSearchOffer], WebSearchMeteredPayment] | None = None
    receipt: Callable[[WebSearchOffer], WebSearchExactPayment] | None = None
    offers: list[WebSearchOffer] = field(default_factory=list)

    def metered(self, offer: WebSearchOffer, target: str) -> WebSearchMeteredPayment:
        self.offers.append(offer)
        if self.grant is None:
            raise AssertionError("metered payment not expected")
        return self.grant(offer)

    def exact(self, offer: WebSearchOffer, target: str) -> WebSearchExactPayment:
        self.offers.append(offer)
        if self.receipt is None:
            raise AssertionError("exact payment not expected")
        return self.receipt(offer)


def _metered_sid() -> _Payer:
    payload = SEARCH_PAID["request"]["headers"]["PAYMENT-SIGNATURE"]["payload"]
    return _Payer(preferences=(WebSearchPreference("SID", "metered"),), did=PAYER_DID,
                  grant=lambda _: WebSearchMeteredPayment(bytes.fromhex(payload["grant"]), payload["idempotencyKey"]))


def _exact_usdc() -> _Payer:
    payload = FETCH_PAID["request"]["headers"]["PAYMENT-SIGNATURE"]["payload"]
    return _Payer(preferences=(WebSearchPreference("USDC", "exact"),),
                  receipt=lambda _: WebSearchExactPayment(base64.b64decode(payload["receipt"])))


def _client(endpoint: str, payer: _Payer, key: str = TRUSTED_KEY, assets: dict[str, WebSearchAssetTerms] | None = None) -> WebSearchClient:
    terms = assets if assets is not None else {currency: WebSearchAssetTerms(ASSETS[currency]["asset_id"], int(ASSETS[currency]["price"])) for currency in CURRENCIES}
    return WebSearchClient(endpoint, "layerx:1", ASSETS["PAX"]["asset_id"], terms, payer,
                           lambda receipt, _: _batch_facts(receipt, key), LayerXSignatureVerifier(),
                           protocol_version=3, now=lambda: 1_000_000_000)


def _fetch_body(url: str, vector: dict, text: str | None = None) -> dict:
    digest = content_digest(web_content_bytes(1, url, vector["media_type"], vector["text"]))
    body = vector["text"] if text is None else text
    return {"url": url, "final_url": url, "media_type": vector["media_type"], "digest": digest, "length": len(body.encode("utf-8")), "text": body}


def _with_fetch_body(steps: list[dict], body: dict) -> list[dict]:
    steps[3]["response"]["body"] = body
    return steps


class WebSearchClientTest(unittest.TestCase):
    def _refused(self, action: Callable[[], object], code: str) -> None:
        with self.assertRaises(WebSearchError) as raised:
            action()
        self.assertEqual(raised.exception.code, code)

    def test_content_vectors(self) -> None:
        for vector in VECTORS:
            canonical = web_content_bytes(1, vector["payload"], vector["media_type"], vector["text"])
            self.assertEqual(content_digest(canonical), vector["digest"])
            decoded = decode_web_content(canonical)
            self.assertEqual((decoded.kind, decoded.payload.decode(), decoded.media_type, decoded.text),
                             (1, vector["payload"], vector["media_type"], vector["text"]))
            self._refused(lambda: decode_web_content(canonical[:-1]), "invalid-content")
        self._refused(lambda: web_content_bytes(1, "x", "Text/HTML; charset=utf-8", ""), "invalid-media-type")

    def test_payer_grants_round_trip(self) -> None:
        for currency in CURRENCIES:
            canonical = bytes.fromhex(BUYER["grants"][currency])
            self.assertEqual(encode_grant(decode_payer_grant(canonical)), canonical)

    def test_exact_usdc_fetch_settles(self) -> None:
        steps = _with_fetch_body(copy.deepcopy(EXCHANGE), _fetch_body("paxeer", VECTORS[0]))
        with _Replay(steps) as sidecar:
            fetched = _client(sidecar.endpoint, _exact_usdc()).fetch("paxeer")
            self.assertEqual(sidecar.served, [2, 3])
        settlement = FETCH_PAID["response"]["headers"]["PAYMENT-RESPONSE"]
        self.assertEqual(fetched.text, VECTORS[0]["text"])
        self.assertEqual(fetched.digest, steps[3]["response"]["body"]["digest"])
        assert fetched.settlement is not None
        self.assertEqual(fetched.settlement.payer, settlement["payer"])
        self.assertEqual(fetched.settlement.transaction, settlement["transaction"])
        self.assertEqual(fetched.settlement.receipt_digest, settlement["extensions"]["layerx"]["receiptDigest"])
        self.assertEqual((fetched.settlement.asset, fetched.settlement.amount), (ASSETS["USDC"]["asset_id"], ASSETS["USDC"]["price"]))

    def test_metered_sid_settlement_without_purpose_is_refused(self) -> None:
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _metered_sid()).search("paxeer"), "settlement-purpose-mismatch")
            self.assertEqual(sidecar.served, [0, 1])
            self.assertEqual(sidecar.unrecorded, [])

    def test_refused_settlements(self) -> None:
        other = BUYER["exact"]["SID"]

        def swap(settlement: dict) -> None:
            settlement["extensions"] = {"layerx": {"receipt": other["receipt"], "receiptDigest": other["receiptDigest"], "verificationLevel": "sequencer-signed"}}
            settlement["transaction"] = f"lxp:{other['receiptDigest']}"

        cases: list[tuple[Callable[[dict], None], str]] = [
            (lambda settlement: settlement.update(payer=SEARCH_PAID["response"]["headers"]["PAYMENT-RESPONSE"]["payer"]), "settlement-payer-mismatch"),
            (lambda settlement: settlement.update(success=False), "settlement-mismatch"),
            (lambda settlement: settlement.update(amount="1"), "settlement-mismatch"),
            (lambda settlement: settlement.update(transaction=f"lxp:{'11' * 32}"), "settlement-receipt-mismatch"),
            (swap, "settlement-receipt-mismatch"),
        ]
        for mutate, code in cases:
            steps = _with_fetch_body(copy.deepcopy(EXCHANGE), _fetch_body("paxeer", VECTORS[0]))
            mutate(steps[3]["response"]["headers"]["PAYMENT-RESPONSE"])
            with _Replay(steps) as sidecar:
                self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch("paxeer"), code)
                self.assertEqual(sidecar.served, [2, 3])
        steps = copy.deepcopy(EXCHANGE)
        del steps[3]["response"]["headers"]["PAYMENT-RESPONSE"]
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch("paxeer"), "missing-payment-response")

    def test_untrusted_sequencer_receipt_is_never_sent(self) -> None:
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc(), UNTRUSTED_KEY).fetch("paxeer"), "receipt-unverified")
            self.assertEqual(sidecar.served, [2])

    def test_digest_mismatches(self) -> None:
        steps = _with_fetch_body(copy.deepcopy(EXCHANGE), _fetch_body("paxeer", VECTORS[0], VECTORS[0]["text"] + " altered"))
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch("paxeer"), "content-digest-mismatch")
            self.assertEqual(sidecar.served, [2, 3])
        stored = {f"/content/{vector['digest']}": web_content_bytes(1, vector["payload"], vector["media_type"], vector["text"]) for vector in VECTORS}
        first, second = VECTORS[0], VECTORS[1]
        stored[f"/content/{second['digest']}"] = stored[f"/content/{first['digest']}"]
        with _Replay([], stored) as sidecar:
            content = _client(sidecar.endpoint, _exact_usdc()).content(first["digest"])
            self.assertEqual((content.digest, content.content.text, content.settlement), (first["digest"], first["text"], None))
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).content(second["digest"]), "content-digest-mismatch")
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).content("zz"), "invalid-digest")

    def test_offers_are_checked_per_asset(self) -> None:
        for currency in CURRENCIES:
            for scheme in ("metered", "exact"):
                if currency == "USDC" and scheme == "exact":
                    continue
                payer = _Payer(
                    preferences=(WebSearchPreference(currency, scheme),),
                    did=PAYER_DID if scheme == "metered" else None,
                    grant=lambda _, c=currency: WebSearchMeteredPayment(bytes.fromhex(BUYER["grants"][c]), "22" * 32),
                    receipt=lambda _, c=currency: WebSearchExactPayment(base64.b64decode(BUYER["exact"][c]["receipt"])),
                )
                with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
                    client = _client(sidecar.endpoint, payer)
                    action = (lambda: client.search("paxeer")) if scheme == "metered" else (lambda: client.fetch("paxeer"))
                    if currency == "PAX":
                        self._refused(action, "offer-account-mismatch")
                        self.assertEqual(payer.offers, [])
                        continue
                    self._refused(action, "payment-refused:unrecorded_request")
                    self.assertEqual(len(payer.offers), 1)
                    self.assertEqual((payer.offers[0].asset, payer.offers[0].amount), (ASSETS[currency]["asset_id"], ASSETS[currency]["price"]))
                    sent = json.loads(base64.b64decode(sidecar.unrecorded[0]["payment-signature"]))
                    self.assertEqual(sent["accepted"]["payTo"], payer.offers[0].pay_to)

    def test_unpaid_refusals(self) -> None:
        payer = _metered_sid()
        payer.grant = lambda _: WebSearchMeteredPayment(bytes.fromhex(BUYER["refusedGrants"]["otherPurpose"]), "33" * 32)
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, payer).search("paxeer"), "grant-offer-mismatch")
            self.assertEqual(sidecar.unrecorded, [])
        steps = copy.deepcopy(EXCHANGE)
        offer = next(offer for offer in steps[0]["response"]["headers"]["PAYMENT-REQUIRED"]["accepts"]
                     if offer["scheme"] == "metered" and offer["extra"]["layerx"]["currency"] == "SID")
        offer["extra"]["layerx"]["payer"] = "44" * 32
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _metered_sid()).search("paxeer"), "offer-payer-mismatch")
            self.assertEqual(sidecar.unrecorded, [])
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            cheaper = {"SID": WebSearchAssetTerms(ASSETS["SID"]["asset_id"], int(ASSETS["SID"]["price"]) - 1)}
            self._refused(lambda: _client(sidecar.endpoint, _metered_sid(), assets=cheaper).search("paxeer"), "offer-price-exceeded")
            self._refused(lambda: _client(sidecar.endpoint, _Payer(preferences=(WebSearchPreference("SID", "metered"),))).fetch("paxeer"), "no-acceptable-offer")
            self.assertEqual(sidecar.served, [0, 2])
        self._refused(lambda: _client("http://example.com", _exact_usdc()), "invalid-endpoint")


if __name__ == "__main__":
    unittest.main()
