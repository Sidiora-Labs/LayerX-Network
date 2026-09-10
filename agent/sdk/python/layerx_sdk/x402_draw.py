import sqlite3
import time

from .x402_activity import bind_receive_activity
from .x402_grant import validate_grant_draw
from .x402_rpc import PaymentRpcError, rpc_hex, verify_rpc_payment


class _DrawUnresolved(Exception):
    __slots__ = ()


class PreparedGrantDraws:
    def __init__(self, path, actor, network, rpc, authority, signatures):
        self.path, self.actor, self.network = path, actor, network
        self.rpc, self.authority, self.signatures = rpc, authority, signatures
        with sqlite3.connect(path) as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute(
                "CREATE TABLE IF NOT EXISTS grant_draws (idempotency_key TEXT PRIMARY KEY, principal TEXT NOT NULL, request_digest TEXT NOT NULL, canonical BLOB NOT NULL, receive BLOB NOT NULL, activity_id TEXT NOT NULL UNIQUE, period_key TEXT UNIQUE, attempted INTEGER NOT NULL DEFAULT 0, attempted_at INTEGER)"
            )

    def register(
        self, principal, request_digest, canonical, receive, key, period_key=None
    ):
        rpc_hex(request_digest, 32)
        if not principal or period_key is not None and not period_key:
            raise ValueError("invalid-draw-registration")
        activity = bind_receive_activity(
            canonical, receive, self.actor, self.network, key
        )
        with sqlite3.connect(self.path, timeout=30) as db:
            db.execute("BEGIN IMMEDIATE")
            existing = db.execute(
                "SELECT principal, request_digest, activity_id, period_key FROM grant_draws WHERE idempotency_key = ?",
                (key,),
            ).fetchone()
            if existing:
                if existing != (principal, request_digest, activity, period_key):
                    raise ValueError("draw-registration-conflict")
                return
            db.execute(
                "INSERT INTO grant_draws (idempotency_key, principal, request_digest, canonical, receive, activity_id, period_key) VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    key,
                    principal,
                    request_digest,
                    canonical,
                    receive,
                    activity,
                    period_key,
                ),
            )

    def _resolve(self, claimed, row, offer):
        try:
            if claimed:
                return self.rpc.send(
                    row["canonical"].hex(), offer["extra"]["layerx"]["commitment"]
                )
            return self.rpc.receipt(row["activity_id"])
        except PaymentRpcError:
            raise
        except Exception as unresolved:
            raise _DrawUnresolved from unresolved

    def __call__(self, principal, request_digest, body, offer):
        key = body["idempotencyKey"]
        with sqlite3.connect(self.path, timeout=30) as db:
            db.row_factory = sqlite3.Row
            row = db.execute(
                "SELECT * FROM grant_draws WHERE idempotency_key = ?", (key,)
            ).fetchone()
        if (
            row is None
            or row["principal"] != principal
            or row["request_digest"] != request_digest
            or row["receive"].hex() != body["receive"]
        ):
            raise ValueError("unregistered-grant-draw")
        receive = validate_grant_draw(
            row["receive"],
            offer,
            key,
            self.network,
            row["attempted_at"]
            if row["attempted_at"] is not None
            else int(time.time()),
        )
        if offer["scheme"] == "subscription" and row["period_key"] is None:
            raise ValueError("subscription-period-required")
        with sqlite3.connect(self.path, timeout=30) as db:
            claimed = db.execute(
                "UPDATE grant_draws SET attempted = 1, attempted_at = ? WHERE idempotency_key = ? AND attempted = 0",
                (int(time.time()), key),
            ).rowcount
        try:
            result = self._resolve(claimed, row, offer)
        except _DrawUnresolved:
            return None
        if result.get("activity_id") != row["activity_id"]:
            raise ValueError("draw-activity-mismatch")
        if result.get("state") == "pending":
            return None
        receipt = rpc_hex(result.get("receipt"))
        evidence = self.authority(receipt, offer)
        verified = verify_rpc_payment(
            result,
            row["activity_id"],
            receive["from"],
            evidence.authorized_batch,
            self.signatures,
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
            commitment=offer["extra"]["layerx"]["commitment"],
            evidence=evidence.commitment_evidence,
        )
        if verified is None:
            return None
        if (
            verified.receipt.module_id != 1
            or verified.receipt.operation != 6
            or evidence.canonical_receipt != receipt
        ):
            raise ValueError("draw-receipt-mismatch")
        return evidence
