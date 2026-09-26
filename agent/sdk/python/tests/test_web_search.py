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
from typing import Any

from layerx_sdk import (
    AuthorizedReceiptBatch,
    ReceiptFailureCode,
    ReceiptVerificationError,
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
from layerx_sdk.x402 import receipt_protocol_version

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
SEARCH_PAID = EXCHANGE[1]


def _fetch_index(currency: str) -> int:
    return 3 + 2 * CURRENCIES.index(currency)


def _fetch_url(currency: str) -> str:
    return VECTORS[CURRENCIES.index(currency)]["payload"]


USDC_PAID = _fetch_index("USDC")


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
                if "bodyBase64" in step:
                    headers["content-type"] = "application/octet-stream"
                    body = base64.b64decode(step["bodyBase64"])
                else:
                    body = json.dumps(step["body"]).encode("utf-8")
                self._send(step["status"], headers, body)

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


def _exact_in(currency: str) -> _Payer:
    payload = EXCHANGE[_fetch_index(currency)]["request"]["headers"]["PAYMENT-SIGNATURE"]["payload"]
    return _Payer(preferences=(WebSearchPreference(currency, "exact"),),
                  receipt=lambda _: WebSearchExactPayment(base64.b64decode(payload["receipt"])))


def _exact_usdc() -> _Payer:
    return _exact_in("USDC")


def _terms() -> dict[str, WebSearchAssetTerms]:
    return {currency: WebSearchAssetTerms(ASSETS[currency]["asset_id"], int(ASSETS[currency]["price"])) for currency in CURRENCIES}


def _client(endpoint: str, payer: _Payer, key: str = TRUSTED_KEY, assets: dict[str, WebSearchAssetTerms] | None = None) -> WebSearchClient:
    return WebSearchClient(endpoint, "layerx:1", ASSETS["PAX"]["asset_id"], assets if assets is not None else _terms(), payer,
                           lambda receipt, _: _batch_facts(receipt, key), LayerXSignatureVerifier(),
                           protocol_version=3, now=lambda: 1_000_000_000)


def _unbounded_client(endpoint: str, payer: _Payer) -> WebSearchClient:
    return WebSearchClient(endpoint, "layerx:1", ASSETS["PAX"]["asset_id"], _terms(), payer,
                           lambda receipt, _: _batch_facts(receipt, TRUSTED_KEY), LayerXSignatureVerifier(),
                           now=lambda: 1_000_000_000)


def _bounded_client(endpoint: str, payer: _Payer, protocol_version: Any) -> WebSearchClient:
    return WebSearchClient(endpoint, "layerx:1", ASSETS["PAX"]["asset_id"], _terms(), payer,
                           lambda receipt, _: _batch_facts(receipt, TRUSTED_KEY), LayerXSignatureVerifier(),
                           protocol_version=protocol_version, now=lambda: 1_000_000_000)


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

    def test_exact_fetch_settles_in_every_asset(self) -> None:
        for position, currency in enumerate(CURRENCIES):
            paid = _fetch_index(currency)
            vector = VECTORS[position]
            with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
                fetched = _client(sidecar.endpoint, _exact_in(currency)).fetch(vector["payload"])
                self.assertEqual(sidecar.served, [paid - 1, paid])
                self.assertEqual(sidecar.unrecorded, [])
            settlement = EXCHANGE[paid]["response"]["headers"]["PAYMENT-RESPONSE"]
            self.assertEqual((fetched.url, fetched.text, fetched.media_type, fetched.digest),
                             (vector["payload"], vector["text"], vector["media_type"], vector["digest"]))
            assert fetched.settlement is not None
            self.assertEqual((fetched.settlement.scheme, fetched.settlement.currency, fetched.settlement.network), ("exact", currency, "layerx:1"))
            self.assertEqual(fetched.settlement.payer, settlement["payer"])
            self.assertEqual(fetched.settlement.transaction, settlement["transaction"])
            self.assertEqual(fetched.settlement.receipt_digest, settlement["extensions"]["layerx"]["receiptDigest"])
            self.assertEqual((fetched.settlement.asset, fetched.settlement.amount), (ASSETS[currency]["asset_id"], ASSETS[currency]["price"]))

    def test_metered_sid_settlement_repeats_the_purpose(self) -> None:
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            found = _client(sidecar.endpoint, _metered_sid()).search("paxeer")
            self.assertEqual(sidecar.served, [0, 1])
            self.assertEqual(sidecar.unrecorded, [])
        settlement = SEARCH_PAID["response"]["headers"]["PAYMENT-RESPONSE"]
        offer = next(offer for offer in EXCHANGE[0]["response"]["headers"]["PAYMENT-REQUIRED"]["accepts"]
                     if offer["scheme"] == "metered" and offer["extra"]["layerx"]["currency"] == "SID")
        self.assertEqual(settlement["extensions"]["layerx"]["purposeHash"], offer["extra"]["layerx"]["purposeHash"])
        self.assertEqual([(result.url, result.title, result.snippet) for result in found.results],
                         [(result["url"], result["title"], result["snippet"]) for result in SEARCH_PAID["response"]["body"]["results"]])
        assert found.settlement is not None
        self.assertEqual((found.settlement.scheme, found.settlement.currency, found.settlement.payer, found.settlement.transaction),
                         ("metered", "SID", settlement["payer"], settlement["transaction"]))
        for purpose in (None, "44" * 32):
            steps = copy.deepcopy(EXCHANGE)
            layerx = steps[1]["response"]["headers"]["PAYMENT-RESPONSE"]["extensions"]["layerx"]
            if purpose is None:
                del layerx["purposeHash"]
            else:
                layerx["purposeHash"] = purpose
            with _Replay(steps) as sidecar:
                self._refused(lambda: _client(sidecar.endpoint, _metered_sid()).search("paxeer"), "settlement-purpose-mismatch")
                self.assertEqual(sidecar.served, [0, 1])

    def test_refused_settlements(self) -> None:
        other = BUYER["exact"]["SID"]

        def swap(settlement: dict) -> None:
            settlement["extensions"] = {"layerx": {"receipt": other["receipt"], "receiptDigest": other["receiptDigest"], "verificationLevel": "sequencer-signed"}}
            settlement["transaction"] = f"lxp:{other['receiptDigest']}"

        sid_payer = EXCHANGE[_fetch_index("SID")]["response"]["headers"]["PAYMENT-RESPONSE"]["payer"]
        cases: list[tuple[Callable[[dict], None], str]] = [
            (lambda settlement: settlement.update(payer=sid_payer), "settlement-payer-mismatch"),
            (lambda settlement: settlement.update(success=False), "settlement-mismatch"),
            (lambda settlement: settlement.update(amount="1"), "settlement-mismatch"),
            (lambda settlement: settlement.update(transaction=f"lxp:{'11' * 32}"), "settlement-receipt-mismatch"),
            (swap, "settlement-receipt-mismatch"),
        ]
        for mutate, code in cases:
            steps = copy.deepcopy(EXCHANGE)
            mutate(steps[USDC_PAID]["response"]["headers"]["PAYMENT-RESPONSE"])
            with _Replay(steps) as sidecar:
                self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch(_fetch_url("USDC")), code)
                self.assertEqual(sidecar.served, [USDC_PAID - 1, USDC_PAID])
        steps = copy.deepcopy(EXCHANGE)
        del steps[USDC_PAID]["response"]["headers"]["PAYMENT-RESPONSE"]
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch(_fetch_url("USDC")), "missing-payment-response")

    def test_recorded_receipts_carry_protocol_version_three(self) -> None:
        receipts = [base64.b64decode(EXCHANGE[_fetch_index(currency)]["request"]["headers"]["PAYMENT-SIGNATURE"]["payload"]["receipt"])
                    for currency in CURRENCIES]
        receipts.append(base64.b64decode(SEARCH_PAID["response"]["headers"]["PAYMENT-RESPONSE"]["extensions"]["layerx"]["receipt"]))
        for receipt in receipts:
            self.assertEqual((receipt[0:2], receipt[4:6]), (b"\x00\x03", b"\x00\x03"))

    def test_unconfigured_client_verifies_by_the_carried_version(self) -> None:
        for position, currency in enumerate(CURRENCIES):
            paid = _fetch_index(currency)
            with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
                fetched = _unbounded_client(sidecar.endpoint, _exact_in(currency)).fetch(VECTORS[position]["payload"])
                self.assertEqual(sidecar.served, [paid - 1, paid])
            assert fetched.settlement is not None
            self.assertEqual(fetched.digest, VECTORS[position]["digest"])
            self.assertEqual(fetched.settlement.transaction, EXCHANGE[paid]["response"]["headers"]["PAYMENT-RESPONSE"]["transaction"])
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            found = _unbounded_client(sidecar.endpoint, _metered_sid()).search("paxeer")
            self.assertEqual(sidecar.served, [0, 1])
        assert found.settlement is not None
        self.assertEqual((found.settlement.scheme, found.settlement.transaction),
                         ("metered", SEARCH_PAID["response"]["headers"]["PAYMENT-RESPONSE"]["transaction"]))
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: WebSearchClient(sidecar.endpoint, "layerx:1", ASSETS["PAX"]["asset_id"], _terms(), _exact_usdc(),
                                                  lambda receipt, _: _batch_facts(receipt, UNTRUSTED_KEY), LayerXSignatureVerifier(),
                                                  now=lambda: 1_000_000_000).fetch(_fetch_url("USDC")), "receipt-unverified")
            self.assertEqual(sidecar.served, [USDC_PAID - 1])

    def test_configured_version_two_refuses_version_three_receipts(self) -> None:
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _bounded_client(sidecar.endpoint, _exact_usdc(), 2).fetch(_fetch_url("USDC")), "receipt-protocol-version")
            self.assertEqual(sidecar.served, [USDC_PAID - 1])
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _bounded_client(sidecar.endpoint, _metered_sid(), 2).search("paxeer"), "receipt-protocol-version")
            self.assertEqual(sidecar.served, [0, 1])
        recorded = base64.b64decode(EXCHANGE[USDC_PAID]["response"]["headers"]["PAYMENT-RESPONSE"]["extensions"]["layerx"]["receipt"])
        self.assertEqual(receipt_protocol_version(recorded), 3)
        with self.assertRaises(ReceiptVerificationError) as raised:
            receipt_protocol_version(recorded[:5])
        self.assertIs(raised.exception.check, ReceiptFailureCode.DECODE)
        with self.assertRaises(ReceiptVerificationError) as raised:
            receipt_protocol_version(b"\x00\x04" + recorded[2:4] + b"\x00\x04" + recorded[6:])
        self.assertIs(raised.exception.check, ReceiptFailureCode.PROTOCOL_VERSION)
        for version in (1, 4, True, "3"):
            self._refused(lambda: _bounded_client("http://127.0.0.1:1", _exact_usdc(), version), "invalid-protocol-version")

    def test_untrusted_sequencer_receipt_is_never_sent(self) -> None:
        with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc(), UNTRUSTED_KEY).fetch(_fetch_url("USDC")), "receipt-unverified")
            self.assertEqual(sidecar.served, [USDC_PAID - 1])

    def test_digest_mismatches(self) -> None:
        steps = copy.deepcopy(EXCHANGE)
        steps[USDC_PAID]["response"]["body"]["text"] += " altered"
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).fetch(_fetch_url("USDC")), "content-digest-mismatch")
            self.assertEqual(sidecar.served, [USDC_PAID - 1, USDC_PAID])
        first, second = VECTORS[0], VECTORS[1]
        recorded = base64.b64decode(EXCHANGE[10]["response"]["bodyBase64"])
        self.assertEqual(content_digest(recorded), first["digest"])
        with _Replay(copy.deepcopy(EXCHANGE), {f"/content/{second['digest']}": recorded}) as sidecar:
            content = _client(sidecar.endpoint, _exact_usdc()).content(first["digest"])
            self.assertEqual(sidecar.served, [10])
            self.assertEqual((content.digest, content.content.text, content.content.payload.decode(), content.settlement),
                             (first["digest"], first["text"], first["payload"], None))
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).content(second["digest"]), "content-digest-mismatch")
            self._refused(lambda: _client(sidecar.endpoint, _exact_usdc()).content("zz"), "invalid-digest")

    def test_offers_are_checked_per_asset(self) -> None:
        for currency in CURRENCIES:
            payer = _Payer(
                preferences=(WebSearchPreference(currency, "metered"),),
                did=PAYER_DID,
                grant=lambda _, c=currency: WebSearchMeteredPayment(bytes.fromhex(BUYER["grants"][c]), "22" * 32),
            )
            with _Replay(copy.deepcopy(EXCHANGE)) as sidecar:
                client = _client(sidecar.endpoint, payer)
                self._refused(lambda: client.search("paxeer"), "payment-refused:unrecorded_request")
                self.assertEqual(len(payer.offers), 1)
                offer = payer.offers[0]
                self.assertEqual((offer.asset, offer.amount), (ASSETS[currency]["asset_id"], ASSETS[currency]["price"]))
                suffix = ":main" if currency == "PAX" else f":asset:{ASSETS[currency]['asset_id']}"
                self.assertTrue(offer.account.endswith(suffix), offer.account)
                sent = json.loads(base64.b64decode(sidecar.unrecorded[0]["payment-signature"]))
                self.assertEqual(sent["accepted"]["payTo"], offer.pay_to)
        steps = copy.deepcopy(EXCHANGE)
        offer = next(offer for offer in steps[_fetch_index("PAX") - 1]["response"]["headers"]["PAYMENT-REQUIRED"]["accepts"]
                     if offer["scheme"] == "exact" and offer["extra"]["layerx"]["currency"] == "PAX")
        offer["extra"]["layerx"]["account"] = offer["extra"]["layerx"]["account"].removesuffix(":main") + f":asset:{ASSETS['PAX']['asset_id']}"
        with _Replay(steps) as sidecar:
            self._refused(lambda: _client(sidecar.endpoint, _exact_in("PAX")).fetch(_fetch_url("PAX")), "offer-account-mismatch")
            self.assertEqual(sidecar.unrecorded, [])

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
            self._refused(lambda: _client(sidecar.endpoint, _Payer(preferences=(WebSearchPreference("SID", "metered"),))).fetch(_fetch_url("SID")), "no-acceptable-offer")
            self.assertEqual(sidecar.served, [0, 2])
        self._refused(lambda: _client("http://example.com", _exact_usdc()), "invalid-endpoint")


if __name__ == "__main__":
    unittest.main()
