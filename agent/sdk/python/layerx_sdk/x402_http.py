from __future__ import annotations

import base64
import hashlib
import json
import re
import sqlite3
from dataclasses import dataclass
from typing import Callable
from urllib.parse import urlsplit

from .x402 import payment_commitment, verify_payment_receipt
from .x402_rpc import rpc_hex, verify_rpc_payment


def _object(value):
    if not isinstance(value, dict):
        raise ValueError("invalid-payment-object")
    return value


def _keys(value, required, optional=()):
    _object(value)
    if not set(required) <= value.keys() or value.keys() - set(required) - set(
        optional
    ):
        raise ValueError("invalid-payment-fields")


def _text(value, limit):
    if not isinstance(value, str) or not 0 < len(value) <= limit or "\0" in value:
        raise ValueError("invalid-payment-text")
    return value


def _url(value):
    _text(value, 2048)
    url = urlsplit(value)
    if (
        url.scheme not in ("http", "https")
        or not url.netloc
        or any(c in value for c in "\r\n\0")
    ):
        raise ValueError("invalid-payment-url")


def _canonical(value):
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )


def _resource(value):
    _keys(
        value, ("url",), ("description", "mimeType", "serviceName", "tags", "iconUrl")
    )
    _url(value["url"])
    for key, limit in (("description", 512), ("mimeType", 32), ("serviceName", 32)):
        if key in value:
            _text(value[key], limit)
    if "iconUrl" in value:
        _url(value["iconUrl"])
    if "tags" in value:
        if not isinstance(value["tags"], list) or len(value["tags"]) > 5:
            raise ValueError("invalid-payment-tags")
        for tag in value["tags"]:
            if re.fullmatch(r"[\x20-\x7e]{1,32}", _text(tag, 32)) is None:
                raise ValueError("invalid-payment-tags")


def validate_requirements(value):
    _keys(
        value,
        ("scheme", "network", "amount", "asset", "payTo", "maxTimeoutSeconds"),
        ("extra",),
    )
    if value["scheme"] not in ("exact", "metered", "subscription"):
        raise ValueError("unsupported-payment")
    if (
        re.fullmatch(r"layerx:[A-Za-z0-9._-]{1,64}", _text(value["network"], 71))
        is None
    ):
        raise ValueError("unsupported-network")
    amount = value["amount"]
    if (
        not isinstance(amount, str)
        or re.fullmatch(r"[1-9][0-9]{0,38}", amount) is None
        or int(amount) >= 1 << 128
    ):
        raise ValueError("invalid-payment-amount")
    rpc_hex(value["asset"], 32)
    rpc_hex(value["payTo"], 32)
    timeout = value["maxTimeoutSeconds"]
    if type(timeout) is not int or not 0 < timeout < 1 << 32:
        raise ValueError("invalid-payment-timeout")
    payment_commitment(value.get("extra"))
    if value["scheme"] != "exact":
        terms = _object(_object(value.get("extra")).get("layerx"))
        rpc_hex(terms.get("purposeHash"), 32)
        if terms["purposeHash"] == "00" * 32:
            raise ValueError("invalid-payment-purpose")
        window = terms.get("windowSeconds")
        if value["scheme"] == "subscription":
            if (
                not isinstance(window, str)
                or re.fullmatch(r"[1-9][0-9]{0,19}", window) is None
                or int(window) >= 1 << 64
            ):
                raise ValueError("invalid-payment-window")
        elif "windowSeconds" in terms:
            raise ValueError("invalid-payment-window")
    return value


def _extensions(value):
    _object(value)
    if len(value) > 32:
        raise ValueError("invalid-payment-extensions")
    for name, extension in value.items():
        if re.fullmatch(r"[A-Za-z0-9._-]{1,32}", name) is None:
            raise ValueError("invalid-payment-extension")
        _keys(extension, ("info", "schema"))


def validate_required(value):
    _keys(value, ("x402Version", "resource", "accepts"), ("error", "extensions"))
    if type(value["x402Version"]) is not int or value["x402Version"] != 2:
        raise ValueError("invalid-payment-version")
    _resource(value["resource"])
    if not isinstance(value["accepts"], list) or not 1 <= len(value["accepts"]) <= 32:
        raise ValueError("invalid-payment-offers")
    for offer in value["accepts"]:
        validate_requirements(offer)
    if "error" in value:
        _text(value["error"], 512)
    if "extensions" in value:
        _extensions(value["extensions"])
    return value


def validate_payload(value):
    _keys(value, ("x402Version", "payload", "accepted"), ("resource", "extensions"))
    if type(value["x402Version"]) is not int or value["x402Version"] != 2:
        raise ValueError("invalid-payment-version")
    _object(value["payload"])
    validate_requirements(value["accepted"])
    if "resource" in value:
        _resource(value["resource"])
    if "extensions" in value:
        _extensions(value["extensions"])
    return value


def encode_header(value):
    raw = json.dumps(
        value, separators=(",", ":"), ensure_ascii=False, allow_nan=False
    ).encode()
    if len(raw) > 65536:
        raise ValueError("payment-header-too-large")
    return base64.b64encode(raw).decode()


def decode_header(value):
    if not isinstance(value, str) or not 0 < len(value) <= 131072:
        raise ValueError("invalid-payment-header")
    raw = base64.b64decode(value, validate=True)
    if len(raw) > 65536 or base64.b64encode(raw).decode() != value:
        raise ValueError("invalid-payment-header")

    def pairs(items):
        result = {}
        for key, item in items:
            if key in result:
                raise ValueError("duplicate-payment-field")
            result[key] = item
        return result

    return json.loads(
        raw.decode("utf-8"),
        object_pairs_hook=pairs,
        parse_constant=lambda _: (_ for _ in ()).throw(
            ValueError("invalid-json-number")
        ),
    )


@dataclass(frozen=True)
class PaymentEvidence:
    canonical_receipt: bytes
    authorized_batch: object
    commitment_evidence: object = None


class FulfillmentStore:
    def __init__(self, path: str):
        self.path = path
        with sqlite3.connect(path) as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute(
                "CREATE TABLE IF NOT EXISTS payment_fulfillments (request_key TEXT PRIMARY KEY, request_digest TEXT NOT NULL, receipt_digest TEXT NOT NULL UNIQUE, resource BLOB NOT NULL)"
            )

    def fulfill(
        self, key, request_digest, receipt_digest, release: Callable[[], bytes]
    ):
        with sqlite3.connect(self.path, timeout=30) as db:
            db.execute("BEGIN IMMEDIATE")
            previous = db.execute(
                "SELECT request_digest, receipt_digest, resource FROM payment_fulfillments WHERE request_key = ?",
                (key,),
            ).fetchone()
            if previous:
                if previous[:2] != (request_digest, receipt_digest):
                    raise ValueError("fulfillment-conflict")
                return previous[2]
            if db.execute(
                "SELECT 1 FROM payment_fulfillments WHERE receipt_digest = ?",
                (receipt_digest,),
            ).fetchone():
                raise ValueError("receipt-replay")
            resource = release()
            if type(resource) is not bytes:
                raise ValueError("resource-must-be-bytes")
            db.execute(
                "INSERT INTO payment_fulfillments VALUES (?, ?, ?, ?)",
                (key, request_digest, receipt_digest, resource),
            )
            return resource


class SellerMiddleware:
    def __init__(
        self,
        required,
        signatures,
        resolve_receipt,
        fulfillments: FulfillmentStore,
        draw=None,
    ):
        self.required = validate_required(json.loads(_canonical(required)))
        self.signatures = signatures
        self.resolve_receipt = resolve_receipt
        self.fulfillments = fulfillments
        self.draw = draw

    def handle(self, principal, payment_header, release):
        if payment_header is None:
            return (
                402,
                {"PAYMENT-REQUIRED": encode_header(self.required)},
                json.dumps(self.required).encode(),
            )
        _text(principal, 512)
        payload = validate_payload(decode_header(payment_header))
        offer = payload["accepted"]
        if (
            offer not in self.required["accepts"]
            or payload.get("resource") != self.required["resource"]
        ):
            raise ValueError("requirements-mismatch")
        for name, value in self.required.get("extensions", {}).items():
            if payload.get("extensions", {}).get(name) != value:
                raise ValueError("extensions-mismatch")
        digest = hashlib.sha256(_canonical(payload).encode()).hexdigest()
        key = hashlib.sha256(
            b"LayerX/middleware/x402/idempotency\0"
            + principal.encode()
            + bytes.fromhex(digest)
        ).hexdigest()
        body = payload["payload"]
        if offer["scheme"] == "exact":
            _keys(
                body,
                ("receipt", "receiptDigest", "verificationLevel"),
                ("idempotencyKey",),
            )
            if body["verificationLevel"] != "sequencer-signed":
                raise ValueError("invalid-verification-level")
            receipt = base64.b64decode(body["receipt"], validate=True)
            if (
                hashlib.sha256(b"LXP/v1/merkle-leaf\0" + receipt).hexdigest()
                != body["receiptDigest"]
            ):
                raise ValueError("receipt-digest-mismatch")
            evidence = self.resolve_receipt(receipt, offer)
            if evidence.canonical_receipt != receipt:
                raise ValueError("receipt-mismatch")
        else:
            _keys(body, ("receive", "idempotencyKey"))
            rpc_hex(body["receive"], 733)
            rpc_hex(body["idempotencyKey"], 32)
            if self.draw is None:
                raise ValueError("grant-draw-unavailable")
            evidence = self.draw(principal, digest, body, offer)
            if evidence is None:
                return 202, {}, b""
        verified = verify_payment_receipt(
            evidence.canonical_receipt,
            evidence.authorized_batch,
            self.signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=payment_commitment(offer.get("extra")),
            evidence=evidence.commitment_evidence,
        )
        receipt_digest = hashlib.sha256(
            b"LXP/v1/merkle-leaf\0" + evidence.canonical_receipt
        ).hexdigest()
        resource = self.fulfillments.fulfill(key, digest, receipt_digest, release)
        settlement = {
            "success": True,
            "transaction": "lxp:" + receipt_digest,
            "network": offer["network"],
            "amount": offer["amount"],
            "payer": verified.receipt.from_account.hex(),
            "extensions": {
                "layerx": {
                    "receipt": base64.b64encode(evidence.canonical_receipt).decode(),
                    "receiptDigest": receipt_digest,
                    "verificationLevel": "sequencer-signed",
                }
            },
        }
        return 200, {"PAYMENT-RESPONSE": encode_header(settlement)}, resource


class BuyerMiddleware:
    def __init__(self, rpc, signatures, resolve_receipt, supported):
        self.rpc = rpc
        self.signatures = signatures
        self.resolve_receipt = resolve_receipt
        self.supported = frozenset(supported)

    def parse_offer(self, header):
        required = validate_required(decode_header(header))
        for offer in required["accepts"]:
            if (offer["scheme"], offer["network"]) in self.supported:
                return required, offer
        raise ValueError("unsupported-payment")

    def prepare(self, header, canonical_hex, activity_id, payer):
        required, offer = self.parse_offer(header)
        if offer["scheme"] != "exact":
            raise ValueError("grant-requires-receive-payload")
        result = self.rpc.send(canonical_hex, payment_commitment(offer.get("extra")))
        if result.get("state") == "pending":
            if result.get("activity_id") != activity_id:
                raise ValueError("activity-mismatch")
            return None
        receipt = rpc_hex(result.get("receipt"))
        evidence = self.resolve_receipt(receipt, offer)
        verified = verify_rpc_payment(
            result,
            activity_id,
            payer,
            evidence.authorized_batch,
            self.signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=payment_commitment(offer.get("extra")),
            evidence=evidence.commitment_evidence,
        )
        if verified is None or evidence.canonical_receipt != receipt:
            raise ValueError("receipt-mismatch")
        return encode_header(
            {
                "x402Version": 2,
                "resource": required["resource"],
                "accepted": offer,
                "extensions": required.get("extensions", {}),
                "payload": {
                    "receipt": base64.b64encode(receipt).decode(),
                    "receiptDigest": hashlib.sha256(
                        b"LXP/v1/merkle-leaf\0" + receipt
                    ).hexdigest(),
                    "verificationLevel": "sequencer-signed",
                },
            }
        )

    def grant_header(self, header, receive_hex):
        required, offer = self.parse_offer(header)
        return grant_payment_header(required, offer, receive_hex)

    def capture_grant_settlement(self, header, payment_header, expected_activity):
        from .x402_receive import decode_receive

        rpc_hex(expected_activity, 32)
        payment = validate_payload(decode_header(payment_header))
        receive = decode_receive(rpc_hex(payment["payload"]["receive"], 733))
        offer = payment["accepted"]
        settlement = _object(decode_header(header))
        body = _object(_object(settlement.get("extensions")).get("layerx"))
        if (
            settlement.get("success") is not True
            or settlement.get("network") != offer["network"]
            or settlement.get("amount") != offer["amount"]
            or body.get("verificationLevel") != "sequencer-signed"
        ):
            raise ValueError("settlement-mismatch")
        receipt = base64.b64decode(body["receipt"], validate=True)
        digest = hashlib.sha256(b"LXP/v1/merkle-leaf\0" + receipt).hexdigest()
        if (
            body.get("receiptDigest") != digest
            or settlement.get("transaction") != "lxp:" + digest
        ):
            raise ValueError("settlement-receipt-mismatch")
        evidence = self.resolve_receipt(receipt, offer)
        if evidence.canonical_receipt != receipt:
            raise ValueError("receipt-mismatch")
        return verify_rpc_payment(
            {"activity_id": expected_activity, "receipt": receipt.hex()},
            expected_activity,
            receive["from"],
            evidence.authorized_batch,
            self.signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=payment_commitment(offer.get("extra")),
            evidence=evidence.commitment_evidence,
        )

    def capture_settlement(self, header, payment_header):
        settlement = _object(decode_header(header))
        payment = validate_payload(decode_header(payment_header))
        offer = payment["accepted"]
        evidence_body = _object(_object(settlement.get("extensions")).get("layerx"))
        if (
            settlement.get("success") is not True
            or settlement.get("network") != offer["network"]
            or settlement.get("amount") != offer["amount"]
        ):
            raise ValueError("settlement-mismatch")
        if (
            evidence_body
            != {
                k: payment["payload"][k]
                for k in ("receipt", "receiptDigest", "verificationLevel")
            }
            or settlement.get("transaction") != "lxp:" + evidence_body["receiptDigest"]
        ):
            raise ValueError("settlement-receipt-mismatch")
        receipt = base64.b64decode(evidence_body["receipt"], validate=True)
        if (
            hashlib.sha256(b"LXP/v1/merkle-leaf\0" + receipt).hexdigest()
            != evidence_body["receiptDigest"]
        ):
            raise ValueError("receipt-digest-mismatch")
        evidence = self.resolve_receipt(receipt, offer)
        if evidence.canonical_receipt != receipt:
            raise ValueError("receipt-mismatch")
        return verify_payment_receipt(
            receipt,
            evidence.authorized_batch,
            self.signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=payment_commitment(offer.get("extra")),
            evidence=evidence.commitment_evidence,
        )


class ConfiguredReceiptAuthority:
    def __init__(self, authorized_batch, commitment_evidence=None):
        self.authorized_batch = authorized_batch
        self.commitment_evidence = commitment_evidence

    def __call__(self, receipt, offer):
        validate_requirements(offer)
        return PaymentEvidence(receipt, self.authorized_batch, self.commitment_evidence)


def grant_payment_header(required, accepted, receive_hex):
    from .x402_receive import decode_receive

    validate_required(required)
    validate_requirements(accepted)
    receive = decode_receive(rpc_hex(receive_hex, 733))
    if (
        accepted["scheme"] not in ("metered", "subscription")
        or accepted not in required["accepts"]
        or receive["to"] != accepted["payTo"]
        or receive["asset"] != accepted["asset"]
        or receive["amount"] != accepted["amount"]
    ):
        raise ValueError("requirements-mismatch")
    return encode_header(
        {
            "x402Version": 2,
            "resource": required["resource"],
            "accepted": accepted,
            "extensions": required.get("extensions", {}),
            "payload": {
                "receive": receive_hex,
                "idempotencyKey": receive["idempotency_key"],
            },
        }
    )
