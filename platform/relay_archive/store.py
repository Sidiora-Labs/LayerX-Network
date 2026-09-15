from __future__ import annotations

import hashlib
import hmac
import json
import os
import sqlite3
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Mapping

try:
    from .protocol import (
        IntegrityError,
        ProtocolError,
        RelayConfig,
        U64_MAX,
        canonical_json_bytes,
        decode_hex,
        require_decimal,
        sha256_hex,
    )
except ImportError:
    from protocol import (  # type: ignore
        IntegrityError,
        ProtocolError,
        RelayConfig,
        U64_MAX,
        canonical_json_bytes,
        decode_hex,
        require_decimal,
        sha256_hex,
    )


SCHEMA = """
CREATE TABLE IF NOT EXISTS archive_meta (
    key TEXT PRIMARY KEY,
    value BLOB NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS bootstrap_artifacts (
    kind TEXT PRIMARY KEY CHECK(kind IN ('genesis', 'snapshot')),
    sha256 TEXT NOT NULL,
    body BLOB NOT NULL
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS batches (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_number TEXT NOT NULL UNIQUE,
    batch_id TEXT NOT NULL UNIQUE,
    first_sequence TEXT NOT NULL,
    last_sequence TEXT NOT NULL,
    previous_state_root TEXT NOT NULL,
    resulting_state_root TEXT NOT NULL,
    protocol_version INTEGER NOT NULL,
    epoch TEXT NOT NULL,
    timestamp_ms TEXT NOT NULL,
    header BLOB NOT NULL,
    signature BLOB NOT NULL,
    raw_sha256 TEXT NOT NULL,
    raw BLOB NOT NULL,
    metadata_json BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS activities (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id TEXT NOT NULL UNIQUE,
    batch_number TEXT NOT NULL REFERENCES batches(batch_number),
    batch_index INTEGER NOT NULL,
    sequence TEXT NOT NULL UNIQUE,
    actor TEXT NOT NULL,
    module INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    result_code INTEGER NOT NULL,
    canonical_sha256 TEXT NOT NULL,
    canonical BLOB NOT NULL,
    receipt_sha256 TEXT NOT NULL,
    receipt BLOB NOT NULL,
    metadata_json BLOB NOT NULL,
    UNIQUE(batch_number, batch_index)
);
CREATE INDEX IF NOT EXISTS activities_actor ON activities(actor, cursor);
CREATE INDEX IF NOT EXISTS activities_module ON activities(module, cursor);
CREATE INDEX IF NOT EXISTS activities_batch ON activities(batch_number, cursor);
CREATE TABLE IF NOT EXISTS activity_accounts (
    activity_id TEXT NOT NULL REFERENCES activities(activity_id) ON DELETE CASCADE,
    account TEXT NOT NULL,
    PRIMARY KEY(activity_id, account)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS activity_accounts_account ON activity_accounts(account, activity_id);
CREATE TABLE IF NOT EXISTS receipts (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    activity_id TEXT NOT NULL UNIQUE REFERENCES activities(activity_id) ON DELETE CASCADE,
    batch_number TEXT NOT NULL,
    sequence TEXT NOT NULL UNIQUE,
    result_code INTEGER NOT NULL,
    canonical_sha256 TEXT NOT NULL,
    canonical BLOB NOT NULL,
    metadata_json BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS receipts_batch ON receipts(batch_number, cursor);
CREATE TABLE IF NOT EXISTS maintenance (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_number TEXT NOT NULL REFERENCES batches(batch_number),
    sequence TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    canonical_sha256 TEXT NOT NULL,
    canonical BLOB NOT NULL,
    metadata_json BLOB NOT NULL,
    UNIQUE(batch_number, sequence)
);
CREATE INDEX IF NOT EXISTS maintenance_batch ON maintenance(batch_number, cursor);
CREATE TABLE IF NOT EXISTS submissions (
    actor TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    credential_digest TEXT NOT NULL,
    activity_id TEXT NOT NULL,
    body_sha256 TEXT NOT NULL,
    body BLOB NOT NULL,
    state TEXT NOT NULL,
    status INTEGER,
    content_type TEXT,
    response BLOB,
    upstream TEXT,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(actor, idempotency_key, credential_digest)
) WITHOUT ROWID;
"""


@dataclass(frozen=True)
class SubmissionSlot:
    actor: str
    idempotency_key: str
    credential_digest: str
    activity_id: str
    body: bytes
    state: str
    status: int | None
    content_type: str | None
    response: bytes | None
    upstream: str | None


class ArchiveStore:
    def __init__(self, config: RelayConfig):
        self.config = config
        config.data_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        try:
            os.chmod(config.data_dir, 0o700)
        except OSError as error:
            raise IntegrityError(f"cannot secure archive data directory: {error}") from error
        self.path = config.data_dir / "archive.sqlite3"
        self._schema_lock = threading.Lock()
        self._initialize()

    def _connect(self) -> sqlite3.Connection:
        connection = sqlite3.connect(
            self.path,
            timeout=5.0,
            isolation_level=None,
            check_same_thread=False,
        )
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA foreign_keys=ON")
        connection.execute("PRAGMA busy_timeout=5000")
        connection.execute("PRAGMA synchronous=FULL")
        return connection

    def _initialize(self) -> None:
        with self._schema_lock:
            connection = self._connect()
            try:
                mode = connection.execute("PRAGMA journal_mode=WAL").fetchone()[0]
                if str(mode).lower() != "wal":
                    raise IntegrityError("archive database did not enter WAL mode")
                version = int(connection.execute("PRAGMA user_version").fetchone()[0])
                if version not in (0, 1):
                    raise IntegrityError("archive database schema version is unsupported")
                connection.executescript(SCHEMA)
                connection.execute("PRAGMA user_version=1")
                connection.execute("PRAGMA wal_checkpoint(PASSIVE)")
            finally:
                connection.close()
            try:
                os.chmod(self.path, 0o600)
            except OSError as error:
                raise IntegrityError(f"cannot secure archive database: {error}") from error

    @staticmethod
    def _meta_get(connection: sqlite3.Connection, key: str) -> bytes | None:
        row = connection.execute("SELECT value FROM archive_meta WHERE key=?", (key,)).fetchone()
        return None if row is None else bytes(row[0])

    @staticmethod
    def _meta_text(connection: sqlite3.Connection, key: str) -> str | None:
        raw = ArchiveStore._meta_get(connection, key)
        if raw is None:
            return None
        try:
            return raw.decode("ascii")
        except UnicodeDecodeError as error:
            raise IntegrityError(f"archive metadata {key} is corrupt") from error

    @staticmethod
    def _meta_put(connection: sqlite3.Connection, key: str, value: str | bytes) -> None:
        raw = value.encode("ascii") if isinstance(value, str) else value
        connection.execute(
            "INSERT INTO archive_meta(key,value) VALUES(?,?) "
            "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            (key, raw),
        )

    @staticmethod
    def _meta_pin(connection: sqlite3.Connection, key: str, value: str) -> None:
        current = ArchiveStore._meta_text(connection, key)
        if current is not None and current != value:
            raise IntegrityError(f"archive {key} conflicts with configured identity")
        if current is None:
            ArchiveStore._meta_put(connection, key, value)

    def has_bootstrap(self) -> bool:
        connection = self._connect()
        try:
            return self._meta_text(connection, "bootstrap_complete") == "1"
        finally:
            connection.close()

    def initialize_bootstrap(
        self,
        manifest: bytes,
        snapshot: bytes,
        genesis_metadata: Mapping[str, Any],
    ) -> None:
        manifest_sha = sha256_hex(manifest)
        snapshot_sha = sha256_hex(snapshot)
        if manifest_sha != self.config.genesis_sha256:
            raise IntegrityError("genesis manifest digest does not match the configured pin")
        connection = self._connect()
        try:
            connection.execute("BEGIN IMMEDIATE")
            self._meta_pin(connection, "network_id", str(self.config.network_id))
            self._meta_pin(connection, "genesis_sha256", manifest_sha)
            self._meta_pin(connection, "snapshot_sha256", snapshot_sha)
            self._meta_pin(connection, "sequencer_id", self.config.sequencer_id)
            self._meta_pin(
                connection, "sequencer_public_key", self.config.sequencer_public_key
            )
            self._meta_pin(
                connection, "sequencer_first_batch", str(self.config.sequencer_first_batch)
            )
            self._meta_pin(
                connection, "sequencer_last_batch", str(self.config.sequencer_last_batch)
            )
            self._meta_pin(connection, "genesis_state_root", str(genesis_metadata["state_root"]))
            self._meta_pin(
                connection,
                "genesis_receipt_state_root",
                str(genesis_metadata["receipt_state_root"]),
            )
            self._meta_pin(
                connection, "genesis_global_sequence", str(genesis_metadata["global_sequence"])
            )
            self._meta_pin(
                connection, "genesis_metadata", canonical_json_bytes(dict(genesis_metadata)).decode("utf-8")
            )
            for kind, digest, body in (
                ("genesis", manifest_sha, manifest),
                ("snapshot", snapshot_sha, snapshot),
            ):
                existing = connection.execute(
                    "SELECT sha256, body FROM bootstrap_artifacts WHERE kind=?", (kind,)
                ).fetchone()
                if existing is not None and (
                    existing["sha256"] != digest or not hmac.compare_digest(bytes(existing["body"]), body)
                ):
                    raise IntegrityError(f"stored {kind} conflicts with validated bootstrap")
                if existing is None:
                    connection.execute(
                        "INSERT INTO bootstrap_artifacts(kind,sha256,body) VALUES(?,?,?)",
                        (kind, digest, body),
                    )
            if self._meta_text(connection, "next_batch") is None:
                self._meta_put(
                    connection, "next_batch", str(self.config.sequencer_first_batch)
                )
            if self._meta_text(connection, "next_sequence") is None:
                genesis_sequence = require_decimal(
                    str(genesis_metadata["global_sequence"]), "genesis.global_sequence"
                )
                if genesis_sequence == U64_MAX:
                    raise IntegrityError("genesis global sequence exhausts uint64")
                self._meta_put(
                    connection, "next_sequence", str(genesis_sequence + 1)
                )
            self._meta_put(connection, "bootstrap_complete", "1")
            connection.execute("COMMIT")
        except Exception:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise
        finally:
            connection.close()

    def load_bootstrap(self) -> tuple[bytes, bytes]:
        connection = self._connect()
        try:
            rows = connection.execute(
                "SELECT kind,sha256,body FROM bootstrap_artifacts ORDER BY kind"
            ).fetchall()
        finally:
            connection.close()
        values = {str(row["kind"]): (str(row["sha256"]), bytes(row["body"])) for row in rows}
        if set(values) != {"genesis", "snapshot"}:
            raise IntegrityError("archive bootstrap artifacts are incomplete")
        for kind, (digest, body) in values.items():
            if not hmac.compare_digest(sha256_hex(body), digest):
                raise IntegrityError(f"stored {kind} digest is corrupt")
        if values["genesis"][0] != self.config.genesis_sha256:
            raise IntegrityError("stored genesis does not match the configured pin")
        return values["genesis"][1], values["snapshot"][1]

    def artifact(self, kind: str) -> tuple[bytes, str]:
        if kind not in {"genesis", "snapshot"}:
            raise ProtocolError("unknown bootstrap artifact")
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT sha256,body FROM bootstrap_artifacts WHERE kind=?", (kind,)
            ).fetchone()
        finally:
            connection.close()
        if row is None:
            raise IntegrityError("archive bootstrap is incomplete")
        body = bytes(row["body"])
        digest = str(row["sha256"])
        if not hmac.compare_digest(sha256_hex(body), digest):
            raise IntegrityError(f"stored {kind} digest is corrupt")
        return body, digest

    def network_document(self) -> dict[str, Any]:
        connection = self._connect()
        try:
            required = {
                key: self._meta_text(connection, key)
                for key in (
                    "network_id",
                    "genesis_sha256",
                    "snapshot_sha256",
                    "sequencer_id",
                    "sequencer_public_key",
                    "sequencer_first_batch",
                    "sequencer_last_batch",
                )
            }
        finally:
            connection.close()
        if any(value is None for value in required.values()):
            raise IntegrityError("archive network identity is incomplete")
        return {
            "version": 1,
            "network_id": int(required["network_id"]),
            "genesis_sha256": required["genesis_sha256"],
            "snapshot_sha256": required["snapshot_sha256"],
            "sequencer_id": required["sequencer_id"],
            "sequencer_public_key": required["sequencer_public_key"],
            "first_batch": required["sequencer_first_batch"],
            "last_batch": required["sequencer_last_batch"],
        }

    def next_batch(self) -> int:
        connection = self._connect()
        try:
            value = self._meta_text(connection, "next_batch")
        finally:
            connection.close()
        if value is None:
            raise IntegrityError("archive bootstrap is incomplete")
        return require_decimal(value, "archive.next_batch")

    def head_document(self) -> dict[str, Any]:
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT batch_number,batch_id,raw_sha256 FROM batches ORDER BY cursor DESC LIMIT 1"
            ).fetchone()
            next_batch = self._meta_text(connection, "next_batch")
            genesis_sha = self._meta_text(connection, "genesis_sha256")
            network = self._meta_text(connection, "network_id")
        finally:
            connection.close()
        if next_batch is None or genesis_sha is None or network is None:
            raise IntegrityError("archive bootstrap is incomplete")
        return {
            "version": 1,
            "network_id": int(network),
            "genesis_sha256": genesis_sha,
            "head_batch": None if row is None else str(row["batch_number"]),
            "head_batch_id": None if row is None else str(row["batch_id"]),
            "head_raw_sha256": None if row is None else str(row["raw_sha256"]),
            "next_batch": next_batch,
        }

    def ingest_batch(self, raw: bytes, metadata: Mapping[str, Any]) -> bool:
        batch_number = str(metadata["batch_number"])
        batch_value = require_decimal(batch_number, "batch.batch_number")
        first_sequence = str(metadata["first_sequence"])
        last_sequence = str(metadata["last_sequence"])
        first_value = require_decimal(first_sequence, "batch.first_sequence")
        last_value = require_decimal(last_sequence, "batch.last_sequence")
        raw_digest = sha256_hex(raw)
        header = decode_hex(metadata["header_hex"], "batch.header_hex", self.config.max_batch_bytes)
        signature = decode_hex(metadata["signature_hex"], "batch.signature_hex", 4096)
        activities = metadata["activities"]
        maintenance = metadata["maintenance"]
        batch_meta = dict(metadata)
        del batch_meta["activities"]
        del batch_meta["maintenance"]
        del batch_meta["header_hex"]
        del batch_meta["signature_hex"]
        connection = self._connect()
        try:
            connection.execute("BEGIN IMMEDIATE")
            existing = connection.execute(
                "SELECT batch_id,raw_sha256,raw FROM batches WHERE batch_number=?",
                (batch_number,),
            ).fetchone()
            if existing is not None:
                if (
                    existing["batch_id"] == metadata["batch_id"]
                    and existing["raw_sha256"] == raw_digest
                    and hmac.compare_digest(bytes(existing["raw"]), raw)
                ):
                    connection.execute("COMMIT")
                    return False
                raise IntegrityError("immutable batch conflicts with stored history")
            expected_batch = self._meta_text(connection, "next_batch")
            expected_sequence = self._meta_text(connection, "next_sequence")
            expected_root = self._meta_text(connection, "last_state_root")
            if expected_root is None:
                expected_root = self._meta_text(connection, "genesis_receipt_state_root")
            if expected_batch is None or expected_sequence is None or expected_root is None:
                raise IntegrityError("archive synchronization position is incomplete")
            if batch_number != expected_batch or batch_value != int(expected_batch):
                raise IntegrityError("batch gap or reordering detected")
            if first_sequence != expected_sequence or first_value != int(expected_sequence):
                raise IntegrityError("global sequence gap detected")
            if metadata["previous_state_root"] != expected_root:
                raise IntegrityError("batch previous state root is discontinuous")
            connection.execute(
                "INSERT INTO batches(batch_number,batch_id,first_sequence,last_sequence,"
                "previous_state_root,resulting_state_root,protocol_version,epoch,timestamp_ms,"
                "header,signature,raw_sha256,raw,metadata_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                (
                    batch_number,
                    metadata["batch_id"],
                    first_sequence,
                    last_sequence,
                    metadata["previous_state_root"],
                    metadata["resulting_state_root"],
                    metadata["protocol_version"],
                    metadata["epoch"],
                    metadata["timestamp_ms"],
                    header,
                    signature,
                    raw_digest,
                    raw,
                    canonical_json_bytes(batch_meta),
                ),
            )
            for index, activity in enumerate(activities):
                canonical = decode_hex(
                    activity["canonical_hex"],
                    "activity.canonical_hex",
                    self.config.max_activity_bytes,
                )
                receipt = decode_hex(
                    activity["receipt_hex"],
                    "activity.receipt_hex",
                    self.config.max_batch_bytes,
                )
                activity_meta = dict(activity)
                accounts = list(activity_meta.pop("accounts"))
                activity_meta.pop("canonical_hex")
                activity_meta.pop("receipt_hex")
                connection.execute(
                    "INSERT INTO activities(activity_id,batch_number,batch_index,sequence,actor,"
                    "module,ordinal,result_code,canonical_sha256,canonical,receipt_sha256,receipt,"
                    "metadata_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                    (
                        activity["activity_id"],
                        batch_number,
                        index,
                        activity["sequence"],
                        activity["actor"],
                        activity["module"],
                        activity["ordinal"],
                        activity["result_code"],
                        sha256_hex(canonical),
                        canonical,
                        sha256_hex(receipt),
                        receipt,
                        canonical_json_bytes(activity_meta),
                    ),
                )
                for account in accounts:
                    connection.execute(
                        "INSERT INTO activity_accounts(activity_id,account) VALUES(?,?)",
                        (activity["activity_id"], account),
                    )
                receipt_meta = {
                    "activity_id": activity["activity_id"],
                    "batch_number": batch_number,
                    "sequence": activity["sequence"],
                    "result_code": activity["result_code"],
                }
                connection.execute(
                    "INSERT INTO receipts(activity_id,batch_number,sequence,result_code,"
                    "canonical_sha256,canonical,metadata_json) VALUES(?,?,?,?,?,?,?)",
                    (
                        activity["activity_id"],
                        batch_number,
                        activity["sequence"],
                        activity["result_code"],
                        sha256_hex(receipt),
                        receipt,
                        canonical_json_bytes(receipt_meta),
                    ),
                )
            for record in maintenance:
                canonical = decode_hex(
                    record["receipt_hex"],
                    "maintenance.receipt_hex",
                    self.config.max_batch_bytes,
                )
                record_meta = dict(record)
                record_meta.pop("receipt_hex")
                connection.execute(
                    "INSERT INTO maintenance(batch_number,sequence,kind,canonical_sha256,canonical,"
                    "metadata_json) VALUES(?,?,?,?,?,?)",
                    (
                        batch_number,
                        record["sequence"],
                        record["kind"],
                        sha256_hex(canonical),
                        canonical,
                        canonical_json_bytes(record_meta),
                    ),
                )
            if batch_value == U64_MAX or last_value == U64_MAX:
                raise IntegrityError("archive synchronization position exhausted uint64")
            self._meta_put(connection, "next_batch", str(batch_value + 1))
            self._meta_put(connection, "next_sequence", str(last_value + 1))
            self._meta_put(connection, "last_state_root", str(metadata["resulting_state_root"]))
            connection.execute("COMMIT")
            return True
        except sqlite3.IntegrityError as error:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise IntegrityError(f"batch conflicts with archived indexes: {error}") from error
        except Exception:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise
        finally:
            connection.close()

    def raw_batch(self, batch_number: str) -> tuple[bytes, str]:
        require_decimal(batch_number, "batch_number")
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT raw,raw_sha256 FROM batches WHERE batch_number=?", (batch_number,)
            ).fetchone()
        finally:
            connection.close()
        if row is None:
            raise KeyError(batch_number)
        body = bytes(row["raw"])
        digest = str(row["raw_sha256"])
        if not hmac.compare_digest(sha256_hex(body), digest):
            raise IntegrityError("stored canonical batch digest is corrupt")
        return body, digest

    @staticmethod
    def _metadata(row: sqlite3.Row) -> dict[str, Any]:
        try:
            value = json.loads(bytes(row["metadata_json"]))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise IntegrityError("stored index metadata is corrupt") from error
        if not isinstance(value, dict):
            raise IntegrityError("stored index metadata is not an object")
        return value

    @staticmethod
    def _accounts(connection: sqlite3.Connection, activity_id: str) -> list[str]:
        return [
            str(row[0])
            for row in connection.execute(
                "SELECT account FROM activity_accounts WHERE activity_id=? ORDER BY account",
                (activity_id,),
            )
        ]

    @staticmethod
    def _verification() -> dict[str, Any]:
        return {
            "cryptographic_inclusion": "sequencer_verified",
            "execution_replayed": False,
            "settlement_finality": "not_assessed",
        }

    def _activity_document(
        self, connection: sqlite3.Connection, row: sqlite3.Row, include_raw: bool
    ) -> dict[str, Any]:
        value = self._metadata(row)
        value.update(
            {
                "cursor": str(row["cursor"]),
                "activity_id": str(row["activity_id"]),
                "batch_number": str(row["batch_number"]),
                "sequence": str(row["sequence"]),
                "actor": str(row["actor"]),
                "module": int(row["module"]),
                "ordinal": int(row["ordinal"]),
                "result_code": int(row["result_code"]),
                "accounts": self._accounts(connection, str(row["activity_id"])),
                "canonical_sha256": str(row["canonical_sha256"]),
                "receipt_sha256": str(row["receipt_sha256"]),
                "verification": self._verification(),
            }
        )
        if include_raw:
            canonical = bytes(row["canonical"])
            receipt = bytes(row["receipt"])
            if sha256_hex(canonical) != row["canonical_sha256"] or sha256_hex(receipt) != row["receipt_sha256"]:
                raise IntegrityError("stored activity or receipt digest is corrupt")
            value["canonical_hex"] = canonical.hex()
            value["receipt_hex"] = receipt.hex()
        return value

    def list_activities(
        self,
        cursor: int,
        limit: int,
        *,
        actor: str | None = None,
        module: int | None = None,
        batch: str | None = None,
        account: str | None = None,
    ) -> tuple[list[dict[str, Any]], str | None]:
        clauses = ["a.cursor > ?"]
        parameters: list[Any] = [cursor]
        if actor is not None:
            clauses.append("a.actor = ?")
            parameters.append(actor)
        if module is not None:
            clauses.append("a.module = ?")
            parameters.append(module)
        if batch is not None:
            clauses.append("a.batch_number = ?")
            parameters.append(batch)
        if account is not None:
            clauses.append(
                "EXISTS(SELECT 1 FROM activity_accounts aa WHERE aa.activity_id=a.activity_id AND aa.account=?)"
            )
            parameters.append(account)
        parameters.append(limit + 1)
        connection = self._connect()
        try:
            rows = connection.execute(
                "SELECT a.* FROM activities a WHERE "
                + " AND ".join(clauses)
                + " ORDER BY a.cursor LIMIT ?",
                parameters,
            ).fetchall()
            more = len(rows) > limit
            rows = rows[:limit]
            items = [self._activity_document(connection, row, False) for row in rows]
        finally:
            connection.close()
        next_cursor = str(rows[-1]["cursor"]) if more and rows else None
        return items, next_cursor

    def activity(self, activity_id: str) -> dict[str, Any] | None:
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT * FROM activities WHERE activity_id=?", (activity_id,)
            ).fetchone()
            return None if row is None else self._activity_document(connection, row, True)
        finally:
            connection.close()

    def _batch_document(self, row: sqlite3.Row) -> dict[str, Any]:
        value = self._metadata(row)
        value.update(
            {
                "cursor": str(row["cursor"]),
                "batch_number": str(row["batch_number"]),
                "batch_id": str(row["batch_id"]),
                "first_sequence": str(row["first_sequence"]),
                "last_sequence": str(row["last_sequence"]),
                "previous_state_root": str(row["previous_state_root"]),
                "resulting_state_root": str(row["resulting_state_root"]),
                "protocol_version": int(row["protocol_version"]),
                "epoch": str(row["epoch"]),
                "timestamp_ms": str(row["timestamp_ms"]),
                "header_hex": bytes(row["header"]).hex(),
                "signature_hex": bytes(row["signature"]).hex(),
                "raw_sha256": str(row["raw_sha256"]),
                "verification": self._verification(),
            }
        )
        return value

    def list_batches(self, cursor: int, limit: int) -> tuple[list[dict[str, Any]], str | None]:
        connection = self._connect()
        try:
            rows = connection.execute(
                "SELECT * FROM batches WHERE cursor>? ORDER BY cursor LIMIT ?", (cursor, limit + 1)
            ).fetchall()
            more = len(rows) > limit
            rows = rows[:limit]
            items = [self._batch_document(row) for row in rows]
        finally:
            connection.close()
        return items, str(rows[-1]["cursor"]) if more and rows else None

    def batch(self, batch_number: str, include_records: bool = True) -> dict[str, Any] | None:
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT * FROM batches WHERE batch_number=?", (batch_number,)
            ).fetchone()
            if row is None:
                return None
            value = self._batch_document(row)
            if include_records:
                activities = connection.execute(
                    "SELECT * FROM activities WHERE batch_number=? ORDER BY batch_index",
                    (batch_number,),
                ).fetchall()
                maintenance = connection.execute(
                    "SELECT * FROM maintenance WHERE batch_number=? ORDER BY cursor",
                    (batch_number,),
                ).fetchall()
                value["activities"] = [
                    self._activity_document(connection, activity, True) for activity in activities
                ]
                value["maintenance"] = [
                    self._maintenance_document(record, True) for record in maintenance
                ]
            return value
        finally:
            connection.close()

    def _receipt_document(self, row: sqlite3.Row, include_raw: bool) -> dict[str, Any]:
        value = self._metadata(row)
        value.update(
            {
                "cursor": str(row["cursor"]),
                "activity_id": str(row["activity_id"]),
                "batch_number": str(row["batch_number"]),
                "sequence": str(row["sequence"]),
                "result_code": int(row["result_code"]),
                "canonical_sha256": str(row["canonical_sha256"]),
                "verification": self._verification(),
            }
        )
        if include_raw:
            canonical = bytes(row["canonical"])
            if sha256_hex(canonical) != row["canonical_sha256"]:
                raise IntegrityError("stored receipt digest is corrupt")
            value["canonical_hex"] = canonical.hex()
        return value

    def list_receipts(
        self, cursor: int, limit: int, batch: str | None = None
    ) -> tuple[list[dict[str, Any]], str | None]:
        clause = "cursor>?"
        parameters: list[Any] = [cursor]
        if batch is not None:
            clause += " AND batch_number=?"
            parameters.append(batch)
        parameters.append(limit + 1)
        connection = self._connect()
        try:
            rows = connection.execute(
                f"SELECT * FROM receipts WHERE {clause} ORDER BY cursor LIMIT ?", parameters
            ).fetchall()
            more = len(rows) > limit
            rows = rows[:limit]
            items = [self._receipt_document(row, False) for row in rows]
        finally:
            connection.close()
        return items, str(rows[-1]["cursor"]) if more and rows else None

    def receipt(self, activity_id: str) -> dict[str, Any] | None:
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT * FROM receipts WHERE activity_id=?", (activity_id,)
            ).fetchone()
            return None if row is None else self._receipt_document(row, True)
        finally:
            connection.close()

    def _maintenance_document(self, row: sqlite3.Row, include_raw: bool) -> dict[str, Any]:
        value = self._metadata(row)
        value.update(
            {
                "cursor": str(row["cursor"]),
                "batch_number": str(row["batch_number"]),
                "sequence": str(row["sequence"]),
                "kind": str(row["kind"]),
                "canonical_sha256": str(row["canonical_sha256"]),
                "verification": self._verification(),
            }
        )
        if include_raw:
            canonical = bytes(row["canonical"])
            if sha256_hex(canonical) != row["canonical_sha256"]:
                raise IntegrityError("stored maintenance receipt digest is corrupt")
            value["receipt_hex"] = canonical.hex()
        return value

    def list_maintenance(
        self, cursor: int, limit: int, batch: str | None = None
    ) -> tuple[list[dict[str, Any]], str | None]:
        clause = "cursor>?"
        parameters: list[Any] = [cursor]
        if batch is not None:
            clause += " AND batch_number=?"
            parameters.append(batch)
        parameters.append(limit + 1)
        connection = self._connect()
        try:
            rows = connection.execute(
                f"SELECT * FROM maintenance WHERE {clause} ORDER BY cursor LIMIT ?", parameters
            ).fetchall()
            more = len(rows) > limit
            rows = rows[:limit]
            items = [self._maintenance_document(row, False) for row in rows]
        finally:
            connection.close()
        return items, str(rows[-1]["cursor"]) if more and rows else None

    def maintenance(self, cursor: int) -> dict[str, Any] | None:
        connection = self._connect()
        try:
            row = connection.execute(
                "SELECT * FROM maintenance WHERE cursor=?", (cursor,)
            ).fetchone()
            return None if row is None else self._maintenance_document(row, True)
        finally:
            connection.close()

    def credential_digest(self, material: bytes) -> str:
        connection = self._connect()
        try:
            connection.execute("BEGIN IMMEDIATE")
            secret = self._meta_get(connection, "credential_hmac_key")
            if secret is None:
                secret = os.urandom(32)
                self._meta_put(connection, "credential_hmac_key", secret)
            connection.execute("COMMIT")
        except Exception:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise
        finally:
            connection.close()
        return hmac.new(secret, material, hashlib.sha256).hexdigest()

    @staticmethod
    def _slot(row: sqlite3.Row) -> SubmissionSlot:
        return SubmissionSlot(
            actor=str(row["actor"]),
            idempotency_key=str(row["idempotency_key"]),
            credential_digest=str(row["credential_digest"]),
            activity_id=str(row["activity_id"]),
            body=bytes(row["body"]),
            state=str(row["state"]),
            status=None if row["status"] is None else int(row["status"]),
            content_type=None if row["content_type"] is None else str(row["content_type"]),
            response=None if row["response"] is None else bytes(row["response"]),
            upstream=None if row["upstream"] is None else str(row["upstream"]),
        )

    def reserve_submission(
        self,
        actor: str,
        idempotency_key: str,
        credential_digest: str,
        activity_id: str,
        body: bytes,
    ) -> tuple[SubmissionSlot, bool]:
        digest = sha256_hex(body)
        connection = self._connect()
        try:
            connection.execute("BEGIN IMMEDIATE")
            row = connection.execute(
                "SELECT * FROM submissions WHERE actor=? AND idempotency_key=? AND credential_digest=?",
                (actor, idempotency_key, credential_digest),
            ).fetchone()
            if row is not None:
                if (
                    row["activity_id"] != activity_id
                    or row["body_sha256"] != digest
                    or not hmac.compare_digest(bytes(row["body"]), body)
                ):
                    raise IntegrityError("idempotency key is already bound to different signed bytes")
                connection.execute("COMMIT")
                return self._slot(row), False
            now = int(time.time() * 1000)
            connection.execute(
                "INSERT INTO submissions(actor,idempotency_key,credential_digest,activity_id,"
                "body_sha256,body,state,updated_at_ms) VALUES(?,?,?,?,?,?,?,?)",
                (
                    actor,
                    idempotency_key,
                    credential_digest,
                    activity_id,
                    digest,
                    body,
                    "new",
                    now,
                ),
            )
            row = connection.execute(
                "SELECT * FROM submissions WHERE actor=? AND idempotency_key=? AND credential_digest=?",
                (actor, idempotency_key, credential_digest),
            ).fetchone()
            connection.execute("COMMIT")
            assert row is not None
            return self._slot(row), True
        except Exception:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise
        finally:
            connection.close()

    def record_submission(
        self,
        slot: SubmissionSlot,
        state: str,
        status: int,
        content_type: str,
        response: bytes,
        upstream: str | None,
    ) -> SubmissionSlot:
        if state not in {"acknowledged", "definitive", "pending", "unknown"}:
            raise ProtocolError("invalid submission journal state")
        if not 100 <= status <= 599 or len(response) > self.config.max_response_bytes:
            raise ProtocolError("invalid submission response")
        connection = self._connect()
        try:
            connection.execute("BEGIN IMMEDIATE")
            current = connection.execute(
                "SELECT * FROM submissions WHERE actor=? AND idempotency_key=? AND credential_digest=?",
                (slot.actor, slot.idempotency_key, slot.credential_digest),
            ).fetchone()
            if current is None or not hmac.compare_digest(bytes(current["body"]), slot.body):
                raise IntegrityError("submission journal binding changed")
            connection.execute(
                "UPDATE submissions SET state=?,status=?,content_type=?,response=?,upstream=?,"
                "updated_at_ms=? WHERE actor=? AND idempotency_key=? AND credential_digest=?",
                (
                    state,
                    status,
                    content_type,
                    response,
                    upstream,
                    int(time.time() * 1000),
                    slot.actor,
                    slot.idempotency_key,
                    slot.credential_digest,
                ),
            )
            row = connection.execute(
                "SELECT * FROM submissions WHERE actor=? AND idempotency_key=? AND credential_digest=?",
                (slot.actor, slot.idempotency_key, slot.credential_digest),
            ).fetchone()
            connection.execute("COMMIT")
            assert row is not None
            return self._slot(row)
        except Exception:
            if connection.in_transaction:
                connection.execute("ROLLBACK")
            raise
        finally:
            connection.close()
