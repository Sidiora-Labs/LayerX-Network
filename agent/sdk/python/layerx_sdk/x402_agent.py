import hashlib
import sqlite3

from .x402 import payment_commitment, verify_payment_receipt
from .x402_http import validate_requirements
from .x402_rpc import rpc_hex


class PaymentBudget:
    def __init__(self, path, tenant, asset, limit):
        rpc_hex(asset, 32)
        if not isinstance(tenant, str) or not 0 < len(tenant) <= 512 or "\0" in tenant:
            raise ValueError("invalid-budget-tenant")
        if type(limit) is not int or not 0 < limit < 2**128:
            raise ValueError("invalid-budget-limit")
        self.path, self.tenant, self.asset, self.limit = path, tenant, asset, limit
        with sqlite3.connect(path) as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute(
                "CREATE TABLE IF NOT EXISTS payment_budgets (tenant TEXT, asset TEXT, amount_limit TEXT NOT NULL, PRIMARY KEY (tenant, asset))"
            )
            db.execute(
                "CREATE TABLE IF NOT EXISTS payment_reservations (tenant TEXT, asset TEXT, request_key TEXT, request_digest TEXT NOT NULL, amount TEXT NOT NULL, receipt_digest TEXT, PRIMARY KEY (tenant, asset, request_key), UNIQUE (tenant, asset, receipt_digest))"
            )
            db.execute(
                "INSERT OR IGNORE INTO payment_budgets VALUES (?, ?, ?)",
                (tenant, asset, str(limit)),
            )
            if db.execute(
                "SELECT amount_limit FROM payment_budgets WHERE tenant = ? AND asset = ?",
                (tenant, asset),
            ).fetchone() != (str(limit),):
                raise ValueError("budget-configuration-conflict")

    def reserve(self, key, digest, offer):
        validate_requirements(offer)
        rpc_hex(key, 32)
        rpc_hex(digest, 32)
        if offer["asset"] != self.asset:
            raise ValueError("budget-asset-mismatch")
        amount = int(offer["amount"])
        with sqlite3.connect(self.path, timeout=30) as db:
            db.execute("BEGIN IMMEDIATE")
            current = db.execute(
                "SELECT request_digest, amount FROM payment_reservations WHERE tenant = ? AND asset = ? AND request_key = ?",
                (self.tenant, self.asset, key),
            ).fetchone()
            if current:
                if current != (digest, str(amount)):
                    raise ValueError("budget-conflict")
                return
            used = sum(
                int(row[0])
                for row in db.execute(
                    "SELECT amount FROM payment_reservations WHERE tenant = ? AND asset = ?",
                    (self.tenant, self.asset),
                )
            )
            if used + amount > self.limit:
                raise ValueError("budget-exhausted")
            db.execute(
                "INSERT INTO payment_reservations VALUES (?, ?, ?, ?, ?, NULL)",
                (self.tenant, self.asset, key, digest, str(amount)),
            )

    def verify_and_commit(self, key, digest, offer, evidence, signatures):
        validate_requirements(offer)
        verified = verify_payment_receipt(
            evidence.canonical_receipt,
            evidence.authorized_batch,
            signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=payment_commitment(offer.get("extra")),
            evidence=evidence.commitment_evidence,
        )
        receipt_digest = hashlib.sha256(
            b"LXP/v1/merkle-leaf\0" + evidence.canonical_receipt
        ).hexdigest()
        with sqlite3.connect(self.path, timeout=30) as db:
            db.execute("BEGIN IMMEDIATE")
            current = db.execute(
                "SELECT request_digest, amount, receipt_digest FROM payment_reservations WHERE tenant = ? AND asset = ? AND request_key = ?",
                (self.tenant, self.asset, key),
            ).fetchone()
            if (
                offer["asset"] != self.asset
                or current is None
                or current[:2] != (digest, offer["amount"])
                or current[2] not in (None, receipt_digest)
            ):
                raise ValueError("budget-conflict")
            db.execute(
                "UPDATE payment_reservations SET receipt_digest = ? WHERE tenant = ? AND asset = ? AND request_key = ?",
                (receipt_digest, self.tenant, self.asset, key),
            )
        return verified


class AgentGrantMiddleware:
    def __init__(self, budgets: PaymentBudget, draws, signatures):
        self.budgets, self.draws, self.signatures = budgets, draws, signatures

    def __call__(self, principal, request_digest, body, offer):
        validate_requirements(offer)
        if offer["scheme"] not in ("metered", "subscription"):
            raise ValueError("grant-offer-required")
        key = body["idempotencyKey"]
        self.budgets.reserve(key, request_digest, offer)
        evidence = self.draws(principal, request_digest, body, offer)
        if evidence is None:
            return None
        self.budgets.verify_and_commit(
            key, request_digest, offer, evidence, self.signatures
        )
        return evidence
