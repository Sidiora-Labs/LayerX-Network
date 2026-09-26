from __future__ import annotations

import base64
import binascii
import hashlib
import http.client
import ipaddress
import json
import re
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Literal, Protocol
from urllib.parse import quote, urlsplit

from .account_derivation import keccak256
from .generated.receipt import ReceiptFailureCode
from .verifier import AuthorizedReceiptBatch, LocalSignatureVerifier, ReceiptVerification, ReceiptVerificationError, verify_receipt
from .x402 import PAYMENT_RECEIPT_PROTOCOL_VERSIONS, receipt_protocol_version
from .x402_receive import _GRANT, encode_grant

WEB_CONTENT_DOMAIN = b"PAXEERX_WEB_CONTENT_V1"
WEB_CONTENT_FETCH = 1
WEB_CONTENT_SEARCH = 2
WEB_SEARCH_CURRENCIES = ("SID", "PAX", "USDC", "USDL")
WEB_SEARCH_PAYER_HEADER = "LAYERX-PAYER-DID"
WEB_SEARCH_MAX_RESULTS = 10

WebSearchScheme = Literal["metered", "exact"]

_HEX32 = re.compile(r"[0-9a-f]{64}")
_ZERO32 = "00" * 32
_MEDIA_TYPE = re.compile(r"[a-z0-9!#$&^_.+-]{1,127}/[a-z0-9!#$&^_.+-]{1,127}")
_AMOUNT = re.compile(r"[1-9][0-9]{0,38}")
_ERROR = re.compile(r"[a-z0-9_]{1,64}")
_NETWORK = re.compile(r"layerx:[A-Za-z0-9._-]{1,64}")
_DID = re.compile(r"[a-z0-9._:-]{1,255}")
_BASE64 = re.compile(r"[A-Za-z0-9+/]+={0,2}")
_MAX_REPLY_BYTES = 9 * 1048576
_MAX_HEADER_CHARACTERS = 131072
_MAX_RETRY_AFTER_SECONDS = 30
_GRANT_BYTES = 346
_RECEIPT_LEAF_DOMAIN = b"LXP/v1/merkle-leaf\x00"
_TIMEOUT_SECONDS = 30.0


class WebSearchError(Exception):
    def __init__(self, code: str, status: int | None = None) -> None:
        super().__init__(code)
        self.code = code
        self.status = status


@dataclass(frozen=True)
class WebContent:
    kind: int
    payload: bytes
    media_type: str
    text: str


@dataclass(frozen=True)
class WebSearchOffer:
    scheme: WebSearchScheme
    network: str
    amount: str
    asset: str
    pay_to: str
    max_timeout_seconds: int
    currency: str
    account: str
    payer: str | None = None
    purpose_hash: str | None = None


@dataclass(frozen=True)
class WebSearchPreference:
    currency: str
    scheme: WebSearchScheme


@dataclass(frozen=True)
class WebSearchMeteredPayment:
    grant: bytes
    idempotency_key: str


@dataclass(frozen=True)
class WebSearchExactPayment:
    receipt: bytes


class WebSearchPayer(Protocol):
    @property
    def did(self) -> str | None: ...

    @property
    def preferences(self) -> Sequence[WebSearchPreference]: ...

    def metered(self, offer: WebSearchOffer, target: str) -> WebSearchMeteredPayment: ...

    def exact(self, offer: WebSearchOffer, target: str) -> WebSearchExactPayment: ...


@dataclass(frozen=True)
class WebSearchAssetTerms:
    asset_id: str
    max_amount: int


@dataclass(frozen=True)
class WebSettlement:
    scheme: WebSearchScheme
    currency: str
    network: str
    asset: str
    amount: str
    payer: str
    receipt_digest: str
    transaction: str


@dataclass(frozen=True)
class WebSearchResult:
    url: str
    title: str
    snippet: str


@dataclass(frozen=True)
class WebSearchResponse:
    results: tuple[WebSearchResult, ...]
    settlement: WebSettlement | None


@dataclass(frozen=True)
class WebFetchResponse:
    url: str
    final_url: str
    media_type: str
    digest: str
    text: str
    settlement: WebSettlement | None


@dataclass(frozen=True)
class WebContentResponse:
    digest: str
    canonical: bytes
    content: WebContent
    settlement: WebSettlement | None


@dataclass(frozen=True)
class _Reply:
    status: int
    headers: Mapping[str, str]
    body: bytes


@dataclass(frozen=True)
class _PreparedPayment:
    header: str
    receipt: bytes | None


def _hex32(value: object) -> bool:
    return isinstance(value, str) and _HEX32.fullmatch(value) is not None


def _record(value: object, code: str) -> dict[str, object]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise WebSearchError(code)
    return value


def _exact_keys(value: Mapping[str, object], keys: tuple[str, ...], code: str) -> None:
    if set(value) != set(keys) or len(value) != len(keys):
        raise WebSearchError(code)


def _utf8(value: bytes, code: str) -> str:
    try:
        return value.decode("utf-8")
    except UnicodeDecodeError:
        raise WebSearchError(code) from None


def _json(raw: bytes, code: str) -> object:
    def pairs(items: list[tuple[str, object]]) -> dict[str, object]:
        result: dict[str, object] = {}
        for key, item in items:
            if key in result:
                raise WebSearchError(code)
            result[key] = item
        return result

    def constant(_: str) -> object:
        raise WebSearchError(code)

    try:
        return json.loads(_utf8(raw, code), object_pairs_hook=pairs, parse_constant=constant)
    except ValueError:
        raise WebSearchError(code) from None


def web_content_bytes(kind: int, payload: bytes | str, media_type: str, text: str) -> bytes:
    if kind not in (WEB_CONTENT_FETCH, WEB_CONTENT_SEARCH):
        raise WebSearchError("invalid-content-kind")
    if not isinstance(media_type, str) or _MEDIA_TYPE.fullmatch(media_type) is None:
        raise WebSearchError("invalid-media-type")
    payload_bytes = payload.encode("utf-8") if isinstance(payload, str) else bytes(payload)
    media = media_type.encode("utf-8")
    body = text.encode("utf-8")
    if len(payload_bytes) >= 1 << 32 or len(body) >= 1 << 64:
        raise WebSearchError("content-too-long")
    return b"".join((
        WEB_CONTENT_DOMAIN, bytes((kind,)),
        len(payload_bytes).to_bytes(4, "big"), payload_bytes,
        len(media).to_bytes(4, "big"), media,
        len(body).to_bytes(8, "big"), body,
    ))


def content_digest(canonical: bytes) -> str:
    return keccak256(bytes(canonical)).hex()


def decode_web_content(canonical: bytes) -> WebContent:
    value = bytes(canonical)
    if len(value) < len(WEB_CONTENT_DOMAIN) + 17 or not value.startswith(WEB_CONTENT_DOMAIN):
        raise WebSearchError("invalid-content")
    offset = len(WEB_CONTENT_DOMAIN)
    kind = value[offset]
    offset += 1
    if kind not in (WEB_CONTENT_FETCH, WEB_CONTENT_SEARCH):
        raise WebSearchError("invalid-content")

    def take(size: int) -> bytes:
        nonlocal offset
        if offset + size > len(value):
            raise WebSearchError("invalid-content")
        length = int.from_bytes(value[offset:offset + size], "big")
        offset += size
        if length > len(value) - offset:
            raise WebSearchError("invalid-content")
        field = value[offset:offset + length]
        offset += length
        return field

    payload = take(4)
    media_type = _utf8(take(4), "invalid-content")
    text = _utf8(take(8), "invalid-content")
    if offset != len(value) or _MEDIA_TYPE.fullmatch(media_type) is None:
        raise WebSearchError("invalid-content")
    return WebContent(kind=kind, payload=payload, media_type=media_type, text=text)


def decode_payer_grant(canonical: bytes) -> dict[str, object]:
    value = bytes(canonical) if isinstance(canonical, (bytes, bytearray)) else b""
    if len(value) != _GRANT_BYTES:
        raise WebSearchError("invalid-grant")
    offset = 0
    grant: dict[str, object] = {}
    for key, kind, size in _GRANT:
        field = value[offset:offset + size]
        offset += size
        if kind == "hex":
            grant[key] = field.hex()
        elif kind == "integer":
            grant[key] = str(int.from_bytes(field, "big"))
        elif field[0] in (0, 1):
            grant[key] = field[0] == 1
        else:
            raise WebSearchError("invalid-grant")
    try:
        encoded = encode_grant(grant)
    except ValueError:
        raise WebSearchError("invalid-grant") from None
    if encoded != value:
        raise WebSearchError("invalid-grant")
    return grant


def _wallet_account_name(did: str, asset: str, native_asset: str) -> str:
    if (_DID.fullmatch(did) is None or did.startswith(":") or did.endswith(":") or "::" in did or ":asset:" in did
            or not _hex32(asset) or not _hex32(native_asset)):
        raise ValueError("invalid-wallet-selector")
    return f"agent:{did}:main" if asset == native_asset else f"agent:{did}:asset:{asset}"


def _wallet_account(did: str, asset: str, native_asset: str) -> str:
    name = _wallet_account_name(did, asset, native_asset).encode("utf-8")
    return hashlib.sha256(b"LX:ACCOUNT:v1" + len(name).to_bytes(4, "big") + name).hexdigest()


def _derived(did: str, asset: str, native_asset: str, code: str) -> tuple[str, str]:
    try:
        return _wallet_account_name(did, asset, native_asset), _wallet_account(did, asset, native_asset)
    except ValueError:
        raise WebSearchError(code) from None


def _account_did(account: str) -> str:
    if not account.startswith("agent:"):
        raise WebSearchError("offer-account-mismatch")
    tail = account[len("agent:"):]
    if tail.endswith(":main"):
        return tail[:-len(":main")]
    marker = tail.rfind(":asset:")
    if marker <= 0 or not _hex32(tail[marker + len(":asset:"):]):
        raise WebSearchError("offer-account-mismatch")
    return tail[:marker]


def _decode_header(value: str | None, code: str) -> dict[str, object]:
    if value is None or not 0 < len(value) <= _MAX_HEADER_CHARACTERS or _BASE64.fullmatch(value) is None:
        raise WebSearchError(code)
    try:
        raw = base64.b64decode(value, validate=True)
    except (binascii.Error, ValueError):
        raise WebSearchError(code) from None
    if base64.b64encode(raw).decode("ascii") != value:
        raise WebSearchError(code)
    return _record(_json(raw, code), code)


def _encode_header(value: object) -> str:
    return base64.b64encode(json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode("utf-8")).decode("ascii")


def _receipt_digest(receipt: bytes) -> str:
    return hashlib.sha256(_RECEIPT_LEAF_DOMAIN + bytes(receipt)).hexdigest()


def _error_of(reply: _Reply) -> str | None:
    try:
        body = json.loads(reply.body.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return None
    error = body.get("error") if isinstance(body, dict) else None
    return error if isinstance(error, str) and _ERROR.fullmatch(error) is not None else None


def _json_body(reply: _Reply) -> dict[str, object]:
    return _record(_json(reply.body, "invalid-resource"), "invalid-resource")


def _loopback(host: str) -> bool:
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


class WebSearchClient:
    def __init__(
        self,
        endpoint: str,
        network: str,
        native_asset: str,
        assets: Mapping[str, WebSearchAssetTerms],
        payer: WebSearchPayer,
        authority: Callable[[bytes, WebSearchOffer], AuthorizedReceiptBatch],
        signatures: LocalSignatureVerifier,
        *,
        protocol_version: int | None = None,
        now: Callable[[], int] | None = None,
        pending_attempts: int = 5,
    ) -> None:
        parts = urlsplit(endpoint)
        host = parts.hostname or ""
        if (parts.scheme not in ("https", "http") or not host or parts.username or parts.password or parts.query or parts.fragment
                or (parts.scheme == "http" and not _loopback(host))):
            raise WebSearchError("invalid-endpoint")
        if not isinstance(network, str) or _NETWORK.fullmatch(network) is None:
            raise WebSearchError("invalid-network")
        if not _hex32(native_asset) or native_asset == _ZERO32:
            raise WebSearchError("invalid-native-asset")
        for currency, terms in assets.items():
            if (currency not in WEB_SEARCH_CURRENCIES or not _hex32(terms.asset_id) or terms.asset_id == _ZERO32
                    or isinstance(terms.max_amount, bool) or not isinstance(terms.max_amount, int)
                    or not 0 < terms.max_amount < 1 << 128):
                raise WebSearchError("invalid-asset-terms")
        if len(payer.preferences) == 0:
            raise WebSearchError("invalid-payer")
        if isinstance(pending_attempts, bool) or not isinstance(pending_attempts, int) or not 1 <= pending_attempts <= 60:
            raise WebSearchError("invalid-pending-attempts")
        if protocol_version is not None and (type(protocol_version) is not int or protocol_version not in PAYMENT_RECEIPT_PROTOCOL_VERSIONS):
            raise WebSearchError("invalid-protocol-version")
        self._scheme = parts.scheme
        self._host = host
        self._port = parts.port
        self._base = parts.path.rstrip("/")
        self._network = network
        self._native_asset = native_asset
        self._assets = dict(assets)
        self._payer = payer
        self._authority = authority
        self._signatures = signatures
        self._protocol_version = protocol_version
        self._now = now if now is not None else (lambda: int(time.time()))
        self._pending_attempts = pending_attempts

    def search(self, query: str) -> WebSearchResponse:
        if not isinstance(query, str) or not query:
            raise WebSearchError("invalid-query")
        reply, settlement = self._exchange(f"/search?q={quote(query, safe='')}")
        results = _json_body(reply).get("results")
        if not isinstance(results, list) or len(results) > WEB_SEARCH_MAX_RESULTS:
            raise WebSearchError("invalid-resource")
        parsed: list[WebSearchResult] = []
        for item in results:
            result = _record(item, "invalid-resource")
            _exact_keys(result, ("url", "title", "snippet"), "invalid-resource")
            url, title, snippet = result["url"], result["title"], result["snippet"]
            if not isinstance(url, str) or not isinstance(title, str) or not isinstance(snippet, str):
                raise WebSearchError("invalid-resource")
            parsed.append(WebSearchResult(url=url, title=title, snippet=snippet))
        return WebSearchResponse(results=tuple(parsed), settlement=settlement)

    def fetch(self, url: str) -> WebFetchResponse:
        if not isinstance(url, str) or not url:
            raise WebSearchError("invalid-url")
        reply, settlement = self._exchange(f"/fetch?url={quote(url, safe='')}")
        body = _json_body(reply)
        final_url, media_type, digest, length, text = (body.get(key) for key in ("final_url", "media_type", "digest", "length", "text"))
        if body.get("url") != url:
            raise WebSearchError("content-url-mismatch")
        if (not isinstance(final_url, str) or not isinstance(media_type, str) or not isinstance(digest, str) or not _hex32(digest)
                or not isinstance(text, str) or isinstance(length, bool) or not isinstance(length, int)):
            raise WebSearchError("invalid-resource")
        if content_digest(web_content_bytes(WEB_CONTENT_FETCH, url, media_type, text)) != digest:
            raise WebSearchError("content-digest-mismatch")
        if len(text.encode("utf-8")) != length:
            raise WebSearchError("content-length-mismatch")
        return WebFetchResponse(url=url, final_url=final_url, media_type=media_type, digest=digest, text=text, settlement=settlement)

    def content(self, digest: str) -> WebContentResponse:
        if not _hex32(digest):
            raise WebSearchError("invalid-digest")
        reply, settlement = self._exchange(f"/content/{digest}")
        if content_digest(reply.body) != digest:
            raise WebSearchError("content-digest-mismatch")
        return WebContentResponse(digest=digest, canonical=reply.body, content=decode_web_content(reply.body), settlement=settlement)

    def _exchange(self, target: str) -> tuple[_Reply, WebSettlement | None]:
        base: dict[str, str] = {}
        if self._payer.did is not None:
            base[WEB_SEARCH_PAYER_HEADER] = self._payer.did
        challenge = self._get(target, base)
        if challenge.status == 200:
            return challenge, None
        if challenge.status != 402:
            raise WebSearchError(f"sidecar-refused:{_error_of(challenge) or challenge.status}", challenge.status)
        offer, raw = self._select(_decode_header(challenge.headers.get("payment-required"), "invalid-payment-required"))
        payment = self._prepare(offer, raw, target)
        attempt = 1
        while True:
            reply = self._get(target, {**base, "PAYMENT-SIGNATURE": payment.header})
            if reply.status == 503 and _error_of(reply) == "payment_pending":
                if attempt >= self._pending_attempts:
                    raise WebSearchError("payment-pending", 503)
                retry = reply.headers.get("retry-after")
                if retry is None or re.fullmatch(r"[0-9]{1,2}", retry) is None or int(retry) > _MAX_RETRY_AFTER_SECONDS:
                    raise WebSearchError("invalid-retry-after", 503)
                time.sleep(int(retry))
                attempt += 1
                continue
            if reply.status != 200:
                raise WebSearchError(f"payment-refused:{_error_of(reply) or reply.status}", reply.status)
            return reply, self._settle(reply.headers.get("payment-response"), offer, payment)

    def _select(self, required: Mapping[str, object]) -> tuple[WebSearchOffer, dict[str, object]]:
        if required.get("x402Version") != 2 or isinstance(required.get("x402Version"), bool):
            raise WebSearchError("invalid-payment-required")
        accepts = required.get("accepts")
        if not isinstance(accepts, list) or not 0 < len(accepts) <= 32:
            raise WebSearchError("invalid-payment-required")
        offers = [_record(offer, "invalid-payment-required") for offer in accepts]
        for preference in self._payer.preferences:
            matching = []
            for offer in offers:
                extra = offer.get("extra")
                layerx = extra.get("layerx") if isinstance(extra, dict) else None
                if offer.get("scheme") == preference.scheme and isinstance(layerx, dict) and layerx.get("currency") == preference.currency:
                    matching.append(offer)
            if len(matching) > 1:
                raise WebSearchError("ambiguous-offer")
            if matching:
                return self._offer(matching[0], preference), matching[0]
        raise WebSearchError("no-acceptable-offer")

    def _offer(self, raw: Mapping[str, object], preference: WebSearchPreference) -> WebSearchOffer:
        _exact_keys(raw, ("scheme", "network", "amount", "asset", "payTo", "maxTimeoutSeconds", "extra"), "invalid-offer")
        extra = _record(raw["extra"], "invalid-offer")
        _exact_keys(extra, ("layerx",), "invalid-offer")
        layerx = _record(extra["layerx"], "invalid-offer")
        metered = preference.scheme == "metered"
        _exact_keys(layerx, ("account", "commitment", "currency", "payer", "purposeHash") if metered else ("account", "commitment", "currency"), "invalid-offer")
        network, amount, asset, pay_to, timeout = raw["network"], raw["amount"], raw["asset"], raw["payTo"], raw["maxTimeoutSeconds"]
        account = layerx["account"]
        if (not isinstance(network, str) or not isinstance(amount, str) or _AMOUNT.fullmatch(amount) is None or int(amount) >= 1 << 128
                or not isinstance(asset, str) or not _hex32(asset) or not isinstance(pay_to, str) or not _hex32(pay_to)
                or isinstance(timeout, bool) or not isinstance(timeout, int) or timeout <= 0 or not isinstance(account, str)):
            raise WebSearchError("invalid-offer")
        if network != self._network:
            raise WebSearchError("offer-network-mismatch")
        if layerx["commitment"] != "executed":
            raise WebSearchError("unsupported-commitment")
        terms = self._assets.get(preference.currency)
        if terms is None:
            raise WebSearchError("asset-not-configured")
        if asset != terms.asset_id:
            raise WebSearchError("offer-asset-mismatch")
        if int(amount) > terms.max_amount:
            raise WebSearchError("offer-price-exceeded")
        name, identifier = _derived(_account_did(account), asset, self._native_asset, "offer-account-mismatch")
        if account != name or pay_to != identifier:
            raise WebSearchError("offer-account-mismatch")
        if not metered:
            return WebSearchOffer(scheme=preference.scheme, network=network, amount=amount, asset=asset, pay_to=pay_to,
                                  max_timeout_seconds=timeout, currency=preference.currency, account=account)
        payer, purpose_hash = layerx["payer"], layerx["purposeHash"]
        if not isinstance(purpose_hash, str) or not _hex32(purpose_hash) or purpose_hash == _ZERO32 or not isinstance(payer, str) or not _hex32(payer):
            raise WebSearchError("invalid-offer")
        did = self._payer.did
        if did is None or payer != _derived(did, asset, self._native_asset, "offer-payer-mismatch")[1]:
            raise WebSearchError("offer-payer-mismatch")
        return WebSearchOffer(scheme=preference.scheme, network=network, amount=amount, asset=asset, pay_to=pay_to,
                              max_timeout_seconds=timeout, currency=preference.currency, account=account,
                              payer=payer, purpose_hash=purpose_hash)

    def _prepare(self, offer: WebSearchOffer, raw: Mapping[str, object], target: str) -> _PreparedPayment:
        if offer.scheme == "metered":
            payment = self._payer.metered(offer, target)
            canonical = bytes(payment.grant)
            grant = decode_payer_grant(canonical)
            amount = int(offer.amount)
            if (grant["from"] != offer.payer or grant["recipient"] != offer.pay_to or grant["asset"] != offer.asset
                    or grant["purpose_hash"] != offer.purpose_hash or grant["recurring"] is not False or grant["window_length"] != "0"
                    or grant["has_reference"] is not False or grant["reference_hash"] != _ZERO32
                    or int(str(grant["per_draw_maximum"])) < amount or int(str(grant["allowance"])) < amount
                    or self._now() >= int(str(grant["expiration"]))):
                raise WebSearchError("grant-offer-mismatch")
            key = payment.idempotency_key
            if not _hex32(key) or key == _ZERO32:
                raise WebSearchError("invalid-idempotency-key")
            header = _encode_header({"x402Version": 2, "accepted": raw, "payload": {"grant": canonical.hex(), "idempotencyKey": key}})
            return _PreparedPayment(header=header, receipt=None)
        receipt = bytes(self._payer.exact(offer, target).receipt)
        self._verify(receipt, offer)
        payload = {"receipt": base64.b64encode(receipt).decode("ascii"), "receiptDigest": _receipt_digest(receipt), "verificationLevel": "sequencer-signed"}
        return _PreparedPayment(header=_encode_header({"x402Version": 2, "accepted": raw, "payload": payload}), receipt=receipt)

    def _verify(self, receipt: bytes, offer: WebSearchOffer) -> ReceiptVerification:
        try:
            version = receipt_protocol_version(receipt)
        except ReceiptVerificationError as error:
            raise WebSearchError("receipt-protocol-version" if error.check is ReceiptFailureCode.PROTOCOL_VERSION else "receipt-unverified") from None
        if self._protocol_version is not None and version != self._protocol_version:
            raise WebSearchError("receipt-protocol-version")
        try:
            authorized = self._authority(receipt, offer)
            verified = verify_receipt(receipt, authorized, self._signatures, protocol_version=version)
        except Exception:
            raise WebSearchError("receipt-unverified") from None
        body = verified.receipt
        if body.asset.hex() != offer.asset or body.amount != int(offer.amount) or body.to_account.hex() != offer.pay_to:
            raise WebSearchError("receipt-offer-mismatch")
        return verified

    def _settle(self, header: str | None, offer: WebSearchOffer, payment: _PreparedPayment) -> WebSettlement:
        if header is None:
            raise WebSearchError("missing-payment-response")
        settlement = _decode_header(header, "invalid-payment-response")
        extensions = _record(settlement.get("extensions"), "invalid-payment-response")
        layerx = _record(extensions.get("layerx"), "invalid-payment-response")
        encoded = layerx.get("receipt")
        if (settlement.get("success") is not True or settlement.get("network") != offer.network or settlement.get("amount") != offer.amount
                or layerx.get("verificationLevel") != "sequencer-signed" or not isinstance(encoded, str)):
            raise WebSearchError("settlement-mismatch")
        try:
            receipt = base64.b64decode(encoded, validate=True)
        except (binascii.Error, ValueError):
            raise WebSearchError("settlement-receipt-mismatch") from None
        if base64.b64encode(receipt).decode("ascii") != encoded:
            raise WebSearchError("settlement-receipt-mismatch")
        digest = _receipt_digest(receipt)
        if layerx.get("receiptDigest") != digest or settlement.get("transaction") != f"lxp:{digest}":
            raise WebSearchError("settlement-receipt-mismatch")
        if payment.receipt is not None and payment.receipt != receipt:
            raise WebSearchError("settlement-receipt-mismatch")
        verified = self._verify(receipt, offer)
        payer = verified.receipt.from_account.hex()
        if settlement.get("payer") != payer:
            raise WebSearchError("settlement-payer-mismatch")
        if offer.scheme == "metered":
            if verified.receipt.module_id != 1 or verified.receipt.operation != 6:
                raise WebSearchError("settlement-operation-mismatch")
            if payer != offer.payer:
                raise WebSearchError("settlement-payer-mismatch")
            if layerx.get("purposeHash") != offer.purpose_hash:
                raise WebSearchError("settlement-purpose-mismatch")
        return WebSettlement(scheme=offer.scheme, currency=offer.currency, network=offer.network, asset=offer.asset,
                             amount=offer.amount, payer=payer, receipt_digest=digest, transaction=f"lxp:{digest}")

    def _get(self, target: str, headers: Mapping[str, str]) -> _Reply:
        connection: http.client.HTTPConnection
        if self._scheme == "https":
            connection = http.client.HTTPSConnection(self._host, self._port, timeout=_TIMEOUT_SECONDS)
        else:
            connection = http.client.HTTPConnection(self._host, self._port, timeout=_TIMEOUT_SECONDS)
        try:
            connection.request("GET", f"{self._base}{target}", headers=dict(headers))
            response = connection.getresponse()
            body = response.read(_MAX_REPLY_BYTES + 1)
            if len(body) > _MAX_REPLY_BYTES:
                raise WebSearchError("reply-too-large")
            collected = {name.lower(): value for name, value in response.getheaders()}
            return _Reply(status=response.status, headers=collected, body=body)
        except OSError:
            raise WebSearchError("sidecar-unreachable") from None
        finally:
            connection.close()


__all__ = [
    "WEB_CONTENT_DOMAIN",
    "WEB_CONTENT_FETCH",
    "WEB_CONTENT_SEARCH",
    "WEB_SEARCH_CURRENCIES",
    "WEB_SEARCH_MAX_RESULTS",
    "WEB_SEARCH_PAYER_HEADER",
    "WebContent",
    "WebContentResponse",
    "WebFetchResponse",
    "WebSearchAssetTerms",
    "WebSearchClient",
    "WebSearchError",
    "WebSearchExactPayment",
    "WebSearchMeteredPayment",
    "WebSearchOffer",
    "WebSearchPayer",
    "WebSearchPreference",
    "WebSearchResponse",
    "WebSearchResult",
    "WebSettlement",
    "content_digest",
    "decode_payer_grant",
    "decode_web_content",
    "web_content_bytes",
]
