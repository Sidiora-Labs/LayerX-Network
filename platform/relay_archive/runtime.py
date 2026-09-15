#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hmac
import json
import os
import signal
import socketserver
import sqlite3
import ssl
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any, Mapping
from urllib.parse import parse_qs, urlsplit

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    from forward import ForwardResult, SafeHTTPClient, SubmissionForwarder, TransportError
    from protocol import (
        CodecError,
        ConfigError,
        Endpoint,
        HEX_32,
        IntegrityError,
        MissingBatch,
        NativeCodec,
        ProtocolError,
        RelayArchiveError,
        RelayConfig,
        U64_MAX,
        canonical_json_bytes,
        load_config,
        parse_endpoint,
        read_file_bounded,
        require_decimal,
        require_hex32,
        sha256_hex,
    )
    from store import ArchiveStore, SubmissionSlot
else:
    from .forward import ForwardResult, SafeHTTPClient, SubmissionForwarder, TransportError
    from .protocol import (
        CodecError,
        ConfigError,
        Endpoint,
        HEX_32,
        IntegrityError,
        MissingBatch,
        NativeCodec,
        ProtocolError,
        RelayArchiveError,
        RelayConfig,
        U64_MAX,
        canonical_json_bytes,
        load_config,
        parse_endpoint,
        read_file_bounded,
        require_decimal,
        require_hex32,
        sha256_hex,
    )
    from .store import ArchiveStore, SubmissionSlot


class _BoundedServer(socketserver.ThreadingMixIn, HTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 128

    def __init__(self, address: tuple[str, int], handler: type[BaseHTTPRequestHandler], maximum: int):
        self._slots = threading.BoundedSemaphore(maximum)
        super().__init__(address, handler)

    def process_request(self, request: Any, client_address: Any) -> None:
        self._slots.acquire()
        try:
            super().process_request(request, client_address)
        except BaseException:
            self._slots.release()
            raise

    def process_request_thread(self, request: Any, client_address: Any) -> None:
        try:
            super().process_request_thread(request, client_address)
        finally:
            self._slots.release()


class RelayArchive:
    def __init__(self, config: RelayConfig):
        self.config = config
        self.codec = NativeCodec(config)
        self.store = ArchiveStore(config)
        self.http = SafeHTTPClient(config)
        self.forwarder = SubmissionForwarder(config, self.http)
        self._stop = threading.Event()
        self._sync_thread: threading.Thread | None = None
        self._server: _BoundedServer | None = None
        self._sync_lock = threading.Lock()
        self._state_lock = threading.Lock()
        self._last_sync_error: str | None = None
        self._last_sync_at_ms: int | None = None
        self._bootstrapped = False
        self.discovery = self._create_discovery()

    def _create_discovery(self) -> Any:
        settings = {
            "network_id": self.config.network_id,
            "genesis_sha256": self.config.genesis_sha256,
            "sequencer_id": self.config.sequencer_id,
            "sequencer_public_key": self.config.sequencer_public_key,
            "public_url": self.config.public_url,
            "request_timeout_seconds": max(
                1, min(30, int(self.config.request_timeout_seconds))
            ),
            "max_response_bytes": min(self.config.max_response_bytes, 64 * 1024 * 1024),
            "max_concurrency": min(self.config.max_concurrency, 64),
        }
        if self.config.peer_discovery:
            settings["peer_discovery"] = dict(self.config.peer_discovery)
        if self.config.ca_file is not None:
            settings["ca_file"] = str(self.config.ca_file)
        try:
            if __package__ in (None, ""):
                from peers import create_discovery
            else:
                from .peers import create_discovery
        except ImportError:
            raise ConfigError("required peer discovery runtime peers.py is unavailable")
        try:
            return create_discovery(settings, self.config.identity)
        except (TypeError, ValueError) as error:
            raise ConfigError(f"peer discovery configuration is invalid: {error}") from error

    def _set_sync_state(self, error: str | None) -> None:
        with self._state_lock:
            self._last_sync_error = error
            self._last_sync_at_ms = int(time.time() * 1000)

    def bootstrap(self) -> None:
        self.discovery.start()
        if self.store.has_bootstrap():
            manifest, snapshot = self.store.load_bootstrap()
            metadata = self._verify_bootstrap_bytes(manifest, snapshot)
            self.store.initialize_bootstrap(manifest, snapshot, metadata)
            self._bootstrapped = True
            return
        if self.config.genesis_manifest is not None:
            assert self.config.genesis_snapshot is not None
            manifest = read_file_bounded(
                self.config.genesis_manifest, self.config.max_bootstrap_bytes
            )
            snapshot = read_file_bounded(
                self.config.genesis_snapshot, self.config.max_bootstrap_bytes
            )
            metadata = self.codec.genesis(
                self.config.genesis_manifest, self.config.genesis_snapshot
            )
            if sha256_hex(manifest) != self.config.genesis_sha256:
                raise IntegrityError("local genesis manifest does not match the configured pin")
            self.store.initialize_bootstrap(manifest, snapshot, metadata)
            self._bootstrapped = True
            return
        origins = self._wait_for_bootstrap_origins()
        errors: list[str] = []
        for endpoint in origins:
            try:
                manifest, snapshot = self._download_bootstrap(endpoint)
                metadata = self._verify_bootstrap_bytes(manifest, snapshot)
                self.store.initialize_bootstrap(manifest, snapshot, metadata)
                self._bootstrapped = True
                return
            except (RelayArchiveError, OSError) as error:
                errors.append(str(error))
        detail = errors[-1] if errors else "no compatible synchronization origin"
        raise IntegrityError(f"cannot bootstrap pinned archive identity: {detail}")

    def _verify_bootstrap_bytes(self, manifest: bytes, snapshot: bytes) -> dict[str, Any]:
        if sha256_hex(manifest) != self.config.genesis_sha256:
            raise IntegrityError("downloaded genesis manifest does not match the configured pin")
        with tempfile.TemporaryDirectory(prefix="layerx-relay-bootstrap-") as directory:
            root = Path(directory)
            manifest_path = root / "genesis.manifest"
            snapshot_path = root / "genesis.snapshot"
            manifest_path.write_bytes(manifest)
            snapshot_path.write_bytes(snapshot)
            os.chmod(manifest_path, 0o600)
            os.chmod(snapshot_path, 0o600)
            return self.codec.genesis(manifest_path, snapshot_path)

    def _wait_for_bootstrap_origins(self) -> tuple[Endpoint, ...]:
        if self.config.upstreams:
            return self._sync_endpoints()
        deadline = time.monotonic() + max(5.0, self.config.request_timeout_seconds * 3.0)
        while time.monotonic() < deadline and not self._stop.is_set():
            origins = self._sync_endpoints()
            if origins:
                return origins
            self._stop.wait(min(0.1, self.config.poll_interval_seconds))
        return self._sync_endpoints()

    def _sync_endpoints(self) -> tuple[Endpoint, ...]:
        endpoints: list[Endpoint] = list(self.config.upstreams)
        known = {endpoint.origin for endpoint in endpoints}
        for origin in self.discovery.sync_origins():
            if origin in known or not self.discovery.validate_sync_origin(origin):
                continue
            try:
                endpoint = self.http.endpoint(origin)
            except (ConfigError, ProtocolError):
                continue
            endpoints.append(endpoint)
            known.add(endpoint.origin)
        return tuple(endpoints)

    def _download_bootstrap(self, endpoint: Endpoint) -> tuple[bytes, bytes]:
        network = self._get_json(endpoint, "/v1/sync/network", 1_048_576)
        self._validate_network_document(network)
        genesis = self.http.request(
            endpoint,
            "GET",
            "/v1/sync/genesis",
            headers={"Accept": "application/octet-stream"},
            maximum=self.config.max_bootstrap_bytes,
        )
        snapshot = self.http.request(
            endpoint,
            "GET",
            "/v1/sync/snapshot",
            headers={"Accept": "application/octet-stream"},
            maximum=self.config.max_bootstrap_bytes,
        )
        if genesis.status != 200 or snapshot.status != 200:
            raise TransportError("bootstrap origin did not serve both pinned artifacts")
        if sha256_hex(genesis.body) != self.config.genesis_sha256:
            raise IntegrityError("bootstrap origin served a foreign genesis manifest")
        expected_snapshot = require_hex32(network.get("snapshot_sha256"), "snapshot_sha256")
        if sha256_hex(snapshot.body) != expected_snapshot:
            raise IntegrityError("bootstrap origin snapshot digest does not match its validated identity")
        return genesis.body, snapshot.body

    def _validate_network_document(self, value: Mapping[str, Any]) -> None:
        if value.get("version") != 1 or value.get("network_id") != self.config.network_id:
            raise IntegrityError("synchronization origin has a foreign network identity")
        expected = {
            "genesis_sha256": self.config.genesis_sha256,
            "sequencer_id": self.config.sequencer_id,
            "sequencer_public_key": self.config.sequencer_public_key,
            "first_batch": str(self.config.sequencer_first_batch),
            "last_batch": str(self.config.sequencer_last_batch),
        }
        for field, pinned in expected.items():
            if value.get(field) != pinned:
                raise IntegrityError(f"synchronization origin changed pinned {field}")
        require_hex32(value.get("snapshot_sha256"), "snapshot_sha256")

    def _get_json(self, endpoint: Endpoint, path: str, maximum: int) -> dict[str, Any]:
        answer = self.http.request(
            endpoint,
            "GET",
            path,
            headers={"Accept": "application/json"},
            maximum=maximum,
        )
        if answer.status != 200:
            raise TransportError(f"synchronization origin returned HTTP {answer.status}")
        media = answer.headers.get("content-type", "").split(";", 1)[0].strip().lower()
        if media != "application/json":
            raise TransportError("synchronization origin returned a non-JSON document")
        try:
            value = json.loads(answer.body)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise TransportError("synchronization origin returned invalid JSON") from error
        if not isinstance(value, dict):
            raise TransportError("synchronization origin returned a non-object document")
        return value

    def synchronize(self) -> int:
        if not self._bootstrapped:
            raise IntegrityError("archive has not completed pinned bootstrap")
        with self._sync_lock:
            try:
                count = self._sync_local_source()
                count += self._sync_remote_sources()
                self._set_sync_state(None)
                return count
            except (RelayArchiveError, OSError) as error:
                self._set_sync_state(str(error))
                raise

    def _sync_local_source(self) -> int:
        if self.config.source_log is None:
            return 0
        count = 0
        while not self._stop.is_set():
            batch_number = self.store.next_batch()
            if batch_number > self.config.sequencer_last_batch:
                break
            try:
                raw = self.codec.export(self.config.source_log, batch_number)
            except MissingBatch:
                break
            metadata = self.codec.verify(raw)
            if metadata["batch_number"] != str(batch_number):
                raise IntegrityError("native source exported an unexpected batch")
            if self.store.ingest_batch(raw, metadata):
                count += 1
        return count

    def _sync_remote_sources(self) -> int:
        count = 0
        while not self._stop.is_set():
            next_batch = self.store.next_batch()
            candidates: list[tuple[Endpoint, int]] = []
            for endpoint in self._sync_endpoints():
                try:
                    network = self._get_json(endpoint, "/v1/sync/network", 1_048_576)
                    self._validate_network_document(network)
                    head = self._get_json(endpoint, "/v1/sync/head", 1_048_576)
                    if (
                        head.get("version") != 1
                        or head.get("network_id") != self.config.network_id
                        or head.get("genesis_sha256") != self.config.genesis_sha256
                    ):
                        raise IntegrityError("synchronization head changed pinned identity")
                    raw_head = head.get("head_batch")
                    if raw_head is None:
                        continue
                    head_number = require_decimal(raw_head, "head_batch")
                    if head_number >= next_batch:
                        candidates.append((endpoint, head_number))
                except (RelayArchiveError, OSError):
                    continue
            if not candidates:
                break
            accepted = False
            for endpoint, head_number in candidates:
                if head_number < next_batch:
                    continue
                try:
                    answer = self.http.request(
                        endpoint,
                        "GET",
                        f"/v1/sync/batches/{next_batch}",
                        headers={"Accept": "application/octet-stream"},
                        maximum=self.config.max_batch_bytes,
                    )
                    if answer.status != 200:
                        continue
                    digest = sha256_hex(answer.body)
                    advertised = answer.headers.get("x-content-sha256")
                    if advertised is not None and not hmac.compare_digest(advertised, digest):
                        raise IntegrityError("synchronization batch digest header is false")
                    metadata = self.codec.verify(answer.body)
                    if metadata["batch_number"] != str(next_batch):
                        raise IntegrityError("synchronization origin returned the wrong batch")
                    if self.store.ingest_batch(answer.body, metadata):
                        count += 1
                    accepted = True
                    break
                except (RelayArchiveError, OSError):
                    continue
            if not accepted:
                raise IntegrityError(f"no origin supplied a valid contiguous batch {next_batch}")
        return count

    def _sync_loop(self) -> None:
        while not self._stop.is_set():
            try:
                self.synchronize()
            except (RelayArchiveError, OSError):
                pass
            self._stop.wait(self.config.poll_interval_seconds)

    def start_sync(self) -> None:
        if self._sync_thread is not None:
            return
        self._sync_thread = threading.Thread(
            target=self._sync_loop,
            name="layerx-relay-archive-sync",
            daemon=True,
        )
        self._sync_thread.start()

    def status(self) -> dict[str, Any]:
        with self._state_lock:
            error = self._last_sync_error
            last = self._last_sync_at_ms
        return {
            "ready": self._bootstrapped and error is None,
            "network_id": self.config.network_id,
            "last_sync_at_ms": last,
            "sync_error": error,
            "role": "relay-archive",
            "executes_activities": False,
            "orders_activities": False,
        }

    def _submission_token_valid(self, authorization: str | None) -> bool:
        token_file = self.config.source_submission_token_file
        if token_file is None:
            return True
        try:
            raw = read_file_bounded(token_file, 8192)
        except ProtocolError:
            return False
        token = raw.rstrip(b"\r\n")
        if not token or len(token) > 4096 or any(byte < 0x21 or byte > 0x7E for byte in token):
            return False
        if authorization is None:
            return False
        supplied = authorization.encode("utf-8", "surrogatepass")
        expected = b"Bearer " + token
        return hmac.compare_digest(supplied, expected)

    @staticmethod
    def _credential_material(headers: Mapping[str, str]) -> bytes:
        values = []
        for name in ("authorization", "layerx-key", "x-layerx-key"):
            value = headers.get(name, "")
            values.append(name.encode("ascii") + b"=" + value.encode("utf-8", "surrogatepass"))
        return b"\x00".join(values)

    def submit_activity(
        self,
        canonical: bytes,
        headers: Mapping[str, str],
        commitment: str = "executed",
    ) -> ForwardResult:
        if commitment not in {"executed", "batched", "finalised"}:
            raise ProtocolError("unsupported submission commitment")
        metadata = self.codec.activity(canonical)
        actor = str(metadata["actor"])
        activity_id = str(metadata["activity_id"])
        idempotency = str(metadata["idempotency_key"])
        supplied_key = headers.get("idempotency-key")
        if supplied_key is not None and supplied_key != idempotency:
            raise ProtocolError("Idempotency-Key does not match the signed activity")
        credential = self.store.credential_digest(self._credential_material(headers))
        try:
            slot, _created = self.store.reserve_submission(
                actor, idempotency, credential, activity_id, canonical
            )
        except IntegrityError as error:
            raise SubmissionConflict(str(error)) from error
        if slot.state in {"acknowledged", "definitive"} and slot.response is not None:
            cached = ForwardResult(
                slot.status or 500,
                slot.content_type or "application/json",
                slot.response,
                slot.state,
                slot.upstream,
            )
            return self._prevent_unverified_completion(
                cached, activity_id, idempotency, commitment
            )
        if self.config.source_lni_socket is not None:
            if not self._submission_token_valid(headers.get("authorization")):
                result = ForwardResult(
                    401,
                    "application/json",
                    canonical_json_bytes({"error": {"code": "unauthorized"}}),
                    "definitive",
                    None,
                )
            else:
                try:
                    acknowledgement = self.codec.submit(
                        self.config.source_lni_socket, canonical
                    )
                    if (
                        acknowledgement["activity_id"] != activity_id
                        or acknowledgement["idempotency_key"] != idempotency
                    ):
                        raise IntegrityError("native submit acknowledgement changed activity identity")
                    result = ForwardResult(
                        422 if acknowledgement["state"] == "refused" else 200,
                        "application/json",
                        canonical_json_bytes(acknowledgement),
                        "definitive" if acknowledgement["state"] == "refused" else "acknowledged",
                        "native-lni",
                    )
                except CodecError:
                    result = ForwardResult(
                        503,
                        "application/json",
                        canonical_json_bytes(
                            {"state": "unknown", "error": {"code": "native_submit_unavailable"}}
                        ),
                        "unknown",
                        None,
                    )
        else:
            result = self.forwarder.forward(
                canonical, idempotency, headers, commitment=commitment
            )
            result = self._prevent_unverified_completion(
                result, activity_id, idempotency, commitment
            )
        self.store.record_submission(
            slot,
            result.state,
            result.status,
            result.content_type,
            result.body,
            result.upstream,
        )
        return result

    def _prevent_unverified_completion(
        self,
        result: ForwardResult,
        activity_id: str,
        idempotency_key: str,
        commitment: str,
    ) -> ForwardResult:
        if result.status != 200:
            return result
        try:
            value = json.loads(result.body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            return ForwardResult(
                502,
                "application/json",
                canonical_json_bytes({"state": "unknown", "error": {"code": "invalid_upstream_response"}}),
                "unknown",
                result.upstream,
            )
        if not isinstance(value, dict):
            return result
        response_activity = value.get("activity_id")
        response_idempotency = value.get("idempotency_key")
        if (
            (response_activity is not None and response_activity != activity_id)
            or (
                response_idempotency is not None
                and response_idempotency != idempotency_key
            )
        ):
            return ForwardResult(
                502,
                "application/json",
                canonical_json_bytes(
                    {
                        "state": "unknown",
                        "activity_id": activity_id,
                        "error": {"code": "upstream_identity_mismatch"},
                    }
                ),
                "unknown",
                result.upstream,
            )
        state = value.get("state")
        if state not in {"completed", "executed", "batched", "finalised", "succeeded"}:
            return result
        receipt = self.store.receipt(activity_id)
        if receipt is not None and commitment in {"executed", "batched"}:
            return ForwardResult(
                200,
                "application/json",
                canonical_json_bytes(
                    {
                        "state": commitment,
                        "activity_id": activity_id,
                        "receipt": receipt,
                        "verification": {
                            "cryptographic_inclusion": "sequencer_verified",
                            "execution_replayed": False,
                            "settlement_finality": "not_assessed",
                        },
                    }
                ),
                "definitive",
                result.upstream,
            )
        return ForwardResult(
            202,
            "application/json",
            canonical_json_bytes(
                {
                    "state": "unknown",
                    "activity_id": activity_id,
                    "reason": "verified_archive_receipt_unavailable",
                }
            ),
            "unknown",
            result.upstream,
        )

    def serve(self) -> None:
        self.start_sync()
        handler = self._handler()
        server = _BoundedServer(
            (self.config.listen_host, self.config.listen_port),
            handler,
            self.config.max_concurrency,
        )
        if self.config.tls_cert is not None:
            assert self.config.tls_key is not None
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            context.minimum_version = ssl.TLSVersion.TLSv1_2
            context.load_cert_chain(str(self.config.tls_cert), str(self.config.tls_key))
            server.socket = context.wrap_socket(
                server.socket,
                server_side=True,
                do_handshake_on_connect=False,
            )
        self._server = server
        try:
            server.serve_forever(poll_interval=0.2)
        finally:
            server.server_close()
            self._server = None

    def close(self) -> None:
        self._stop.set()
        server = self._server
        if server is not None:
            server.shutdown()
        if self._sync_thread is not None:
            self._sync_thread.join(timeout=self.config.request_timeout_seconds + 2.0)
            self._sync_thread = None
        self.discovery.stop()

    def _handler(self) -> type[BaseHTTPRequestHandler]:
        runtime = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"
            server_version = "LayerXRelayArchive/1"
            sys_version = ""

            def setup(self) -> None:
                self.request.settimeout(runtime.config.request_timeout_seconds)
                if isinstance(self.request, ssl.SSLSocket):
                    self.request.do_handshake()
                super().setup()

            def log_message(self, format: str, *args: Any) -> None:
                del format, args

            def do_GET(self) -> None:
                try:
                    status, content_type, body, extra = runtime._get(self.path)
                except RequestError as error:
                    status, content_type, body, extra = error.response()
                except (IntegrityError, sqlite3.Error) as error:
                    status, content_type, body, extra = _error_response(503, "archive_unavailable", str(error))
                except BaseException:
                    status, content_type, body, extra = _error_response(500, "internal_error", None)
                self._send(status, content_type, body, extra)

            def do_POST(self) -> None:
                try:
                    headers = self._request_headers()
                    body = self._read_body(runtime.config.max_response_bytes)
                    status, content_type, answer = runtime._post(self.path, headers, body)
                    extra: dict[str, str] = {}
                except SubmissionConflict as error:
                    status, content_type, answer, extra = _error_response(409, "idempotency_conflict", str(error))
                except RequestError as error:
                    status, content_type, answer, extra = error.response()
                except (ProtocolError, CodecError) as error:
                    status, content_type, answer, extra = _error_response(400, "invalid_request", str(error))
                except IntegrityError as error:
                    status, content_type, answer, extra = _error_response(503, "archive_unavailable", str(error))
                except BaseException:
                    status, content_type, answer, extra = _error_response(500, "internal_error", None)
                self._send(status, content_type, answer, extra)

            def _request_headers(self) -> dict[str, str]:
                result: dict[str, str] = {}
                for name in ("authorization", "layerx-key", "x-layerx-key", "idempotency-key", "content-type"):
                    values = self.headers.get_all(name, [])
                    if len(values) > 1:
                        raise RequestError(400, "duplicate_header")
                    if values:
                        value = values[0]
                        if len(value) > 8192 or "\r" in value or "\n" in value:
                            raise RequestError(400, "invalid_header")
                        result[name] = value
                return result

            def _read_body(self, maximum: int) -> bytes:
                if self.headers.get("Transfer-Encoding") is not None:
                    raise RequestError(400, "transfer_encoding_refused")
                raw_length = self.headers.get("Content-Length")
                if raw_length is None:
                    raise RequestError(411, "content_length_required")
                try:
                    length = int(raw_length)
                except ValueError as error:
                    raise RequestError(400, "invalid_content_length") from error
                if length < 0 or length > maximum:
                    raise RequestError(413, "request_too_large")
                body = self.rfile.read(length)
                if len(body) != length:
                    raise RequestError(400, "truncated_request")
                return body

            def _send(
                self,
                status: int,
                content_type: str,
                body: bytes,
                extra: Mapping[str, str],
            ) -> None:
                if len(body) > runtime.config.max_response_bytes and status != 413:
                    status, content_type, body, extra = _error_response(
                        413, "response_too_large", None
                    )
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("X-Content-Type-Options", "nosniff")
                for name, value in extra.items():
                    self.send_header(name, value)
                self.end_headers()
                if body:
                    self.wfile.write(body)

        return Handler

    def _get(self, target: str) -> tuple[int, str, bytes, dict[str, str]]:
        parsed = urlsplit(target)
        if parsed.fragment:
            raise RequestError(400, "invalid_path")
        path = parsed.path
        query = parse_qs(parsed.query, keep_blank_values=True, strict_parsing=False)
        if path == "/healthz":
            return _json_response(200, {"ok": True, "service": "layerx-relay-archive"})
        if path == "/readyz":
            status = self.status()
            return _json_response(200 if status["ready"] else 503, status)
        if path == "/v1/peers":
            return _json_response(200, self.discovery.public_document())
        if path == "/v1/sync/network":
            return _json_response(200, self.store.network_document())
        if path == "/v1/sync/head":
            return _json_response(200, self.store.head_document())
        if path in {"/v1/sync/genesis", "/v1/sync/snapshot"}:
            kind = path.rsplit("/", 1)[1]
            body, digest = self.store.artifact(kind)
            return 200, "application/octet-stream", body, {
                "ETag": f'"{digest}"',
                "X-Content-SHA256": digest,
                "Cache-Control": "public, immutable",
            }
        batch_prefix = "/v1/sync/batches/"
        if path.startswith(batch_prefix):
            number = path[len(batch_prefix) :]
            try:
                require_decimal(number, "batch_number")
                body, digest = self.store.raw_batch(number)
            except KeyError:
                raise RequestError(404, "batch_not_found")
            return 200, "application/vnd.layerx.canonical-batch", body, {
                "ETag": f'"{digest}"',
                "X-Content-SHA256": digest,
                "X-LayerX-Batch": number,
                "Cache-Control": "public, immutable",
            }
        if path == "/v1/history/activities":
            cursor, limit = self._page(query)
            allowed = {"cursor", "limit", "actor", "module", "batch", "account"}
            self._query_shape(query, allowed)
            actor = self._single(query, "actor", 512)
            module_text = self._single(query, "module", 5)
            module = None
            if module_text is not None:
                try:
                    module = int(module_text)
                except ValueError as error:
                    raise RequestError(400, "invalid_module") from error
                if not 0 <= module <= 0xFFFF:
                    raise RequestError(400, "invalid_module")
            batch = self._single(query, "batch", 20)
            if batch is not None:
                try:
                    require_decimal(batch, "batch")
                except ProtocolError as error:
                    raise RequestError(400, "invalid_batch") from error
            account = self._single(query, "account", 64)
            if account is not None and HEX_32.fullmatch(account) is None:
                raise RequestError(400, "invalid_account")
            items, next_cursor = self.store.list_activities(
                cursor, limit, actor=actor, module=module, batch=batch, account=account
            )
            return _json_response(200, {"version": 1, "items": items, "next_cursor": next_cursor})
        activity_prefix = "/v1/history/activities/"
        if path.startswith(activity_prefix):
            identifier = path[len(activity_prefix) :]
            if HEX_32.fullmatch(identifier) is None:
                raise RequestError(400, "invalid_activity_id")
            value = self.store.activity(identifier)
            if value is None:
                raise RequestError(404, "activity_not_found")
            return _json_response(200, value)
        if path == "/v1/history/batches":
            self._query_shape(query, {"cursor", "limit"})
            cursor, limit = self._page(query)
            items, next_cursor = self.store.list_batches(cursor, limit)
            return _json_response(200, {"version": 1, "items": items, "next_cursor": next_cursor})
        history_batch_prefix = "/v1/history/batches/"
        if path.startswith(history_batch_prefix):
            number = path[len(history_batch_prefix) :]
            try:
                require_decimal(number, "batch_number")
            except ProtocolError as error:
                raise RequestError(400, "invalid_batch") from error
            value = self.store.batch(number)
            if value is None:
                raise RequestError(404, "batch_not_found")
            return _json_response(200, value)
        if path == "/v1/history/receipts":
            self._query_shape(query, {"cursor", "limit", "batch"})
            cursor, limit = self._page(query)
            batch = self._single(query, "batch", 20)
            if batch is not None:
                try:
                    require_decimal(batch, "batch")
                except ProtocolError as error:
                    raise RequestError(400, "invalid_batch") from error
            items, next_cursor = self.store.list_receipts(cursor, limit, batch)
            return _json_response(200, {"version": 1, "items": items, "next_cursor": next_cursor})
        receipt_prefix = "/v1/history/receipts/"
        if path.startswith(receipt_prefix):
            identifier = path[len(receipt_prefix) :]
            if HEX_32.fullmatch(identifier) is None:
                raise RequestError(400, "invalid_activity_id")
            value = self.store.receipt(identifier)
            if value is None:
                raise RequestError(404, "receipt_not_found")
            return _json_response(200, value)
        if path == "/v1/history/maintenance":
            self._query_shape(query, {"cursor", "limit", "batch"})
            cursor, limit = self._page(query)
            batch = self._single(query, "batch", 20)
            if batch is not None:
                try:
                    require_decimal(batch, "batch")
                except ProtocolError as error:
                    raise RequestError(400, "invalid_batch") from error
            items, next_cursor = self.store.list_maintenance(cursor, limit, batch)
            return _json_response(200, {"version": 1, "items": items, "next_cursor": next_cursor})
        maintenance_prefix = "/v1/history/maintenance/"
        if path.startswith(maintenance_prefix):
            value_text = path[len(maintenance_prefix) :]
            if not value_text.isdigit() or int(value_text) <= 0:
                raise RequestError(400, "invalid_cursor")
            value = self.store.maintenance(int(value_text))
            if value is None:
                raise RequestError(404, "maintenance_not_found")
            return _json_response(200, value)
        raise RequestError(404, "not_found")

    @staticmethod
    def _query_shape(query: Mapping[str, list[str]], allowed: set[str]) -> None:
        if set(query) - allowed or any(len(values) != 1 for values in query.values()):
            raise RequestError(400, "invalid_query")

    @staticmethod
    def _single(query: Mapping[str, list[str]], name: str, maximum: int) -> str | None:
        values = query.get(name)
        if values is None:
            return None
        if len(values) != 1 or not values[0] or len(values[0].encode("utf-8")) > maximum:
            raise RequestError(400, f"invalid_{name}")
        return values[0]

    def _page(self, query: Mapping[str, list[str]]) -> tuple[int, int]:
        cursor_text = self._single(query, "cursor", 20)
        limit_text = self._single(query, "limit", 10)
        if cursor_text is None:
            cursor = 0
        elif not cursor_text.isdigit() or int(cursor_text) < 0:
            raise RequestError(400, "invalid_cursor")
        else:
            cursor = int(cursor_text)
        if limit_text is None:
            limit = self.config.history_page_limit
        elif not limit_text.isdigit():
            raise RequestError(400, "invalid_limit")
        else:
            limit = int(limit_text)
        if not 1 <= limit <= self.config.max_history_page_limit:
            raise RequestError(400, "invalid_limit")
        return cursor, limit

    def _post(
        self, target: str, headers: Mapping[str, str], body: bytes
    ) -> tuple[int, str, bytes]:
        parsed = urlsplit(target)
        if parsed.query or parsed.fragment:
            raise RequestError(400, "invalid_path")
        if parsed.path == "/v1/activities":
            media = headers.get("content-type", "").split(";", 1)[0].strip().lower()
            if media != "application/octet-stream":
                raise RequestError(415, "unsupported_media_type")
            if not body or len(body) > self.config.max_activity_bytes:
                raise RequestError(400 if not body else 413, "invalid_activity")
            if self.config.source_lni_socket is not None and not self._submission_token_valid(
                headers.get("authorization")
            ):
                return 401, "application/json", canonical_json_bytes(
                    {"error": {"code": "unauthorized"}}
                )
            result = self.submit_activity(body, headers)
            return result.status, result.content_type, result.body
        if parsed.path == "/rpc":
            media = headers.get("content-type", "").split(";", 1)[0].strip().lower()
            if media != "application/json":
                raise RequestError(415, "unsupported_media_type")
            return 200, "application/json", self._rpc(body, headers)
        raise RequestError(404, "not_found")

    def _rpc(self, body: bytes, headers: Mapping[str, str]) -> bytes:
        try:
            request = json.loads(body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            return _rpc_error(None, -32700, "Parse error")
        if not isinstance(request, dict):
            return _rpc_error(None, -32600, "Invalid Request")
        request_id = request.get("id")
        if (
            request.get("jsonrpc") != "2.0"
            or not isinstance(request.get("method"), str)
            or not (request_id is None or isinstance(request_id, (str, int, float)))
        ):
            return _rpc_error(None, -32600, "Invalid Request")
        method = request["method"]
        params = request.get("params", [])
        try:
            if method == "lx_sendActivity":
                if (
                    not isinstance(params, list)
                    or len(params) != 2
                    or not isinstance(params[0], str)
                    or not isinstance(params[1], str)
                    or len(params[0]) % 2
                    or len(params[0]) // 2 > self.config.max_activity_bytes
                ):
                    raise ProtocolError("invalid send params")
                try:
                    canonical = bytes.fromhex(params[0])
                except ValueError as error:
                    raise ProtocolError("invalid activity hexadecimal") from error
                result = self.submit_activity(canonical, headers, params[1])
                if result.status == 200:
                    value = json.loads(result.body)
                    if isinstance(value, dict) and value.get("state") == "acknowledged":
                        return _rpc_error(
                            request_id,
                            -32001,
                            "Requested commitment unavailable",
                            {
                                "state": "pending",
                                "requested_commitment": params[1],
                                "acknowledgement": value,
                            },
                        )
                    return canonical_json_bytes(
                        {"jsonrpc": "2.0", "id": request_id, "result": value}
                    )
                code = {
                    400: -32602,
                    401: -32002,
                    403: -32002,
                    409: -32003,
                    429: -32005,
                }.get(result.status, -32001)
                data = json.loads(result.body) if result.body else {}
                return _rpc_error(request_id, code, "Submission unavailable", data)
            result = self._rpc_read(method, params)
            return canonical_json_bytes({"jsonrpc": "2.0", "id": request_id, "result": result})
        except (ProtocolError, ValueError, TypeError):
            return _rpc_error(request_id, -32602, "Invalid params")
        except KeyError:
            return _rpc_error(request_id, -32004, "Not found")

    def _rpc_read(self, method: str, params: Any) -> Any:
        if not isinstance(params, list):
            raise ProtocolError("params must be an array")
        if method in {"lx_getNodeInfo", "lx_getArchiveNetwork"} and not params:
            return self.store.network_document()
        if method == "lx_getArchiveHead" and not params:
            return self.store.head_document()
        if method == "lx_getActivityStatus" and len(params) == 1 and isinstance(params[0], str):
            value = self.store.activity(require_hex32(params[0], "activity_id"))
            if value is None:
                raise KeyError(params[0])
            value["state"] = "included"
            return value
        if method == "lx_getReceipt" and len(params) == 1 and isinstance(params[0], str):
            value = self.store.receipt(require_hex32(params[0], "activity_id"))
            if value is None:
                raise KeyError(params[0])
            return value
        if method == "lx_getBatchHeader" and len(params) == 1:
            number = str(params[0])
            require_decimal(number, "batch_number")
            value = self.store.batch(number, include_records=False)
            if value is None:
                raise KeyError(number)
            return value
        if method == "lx_listActivities" and len(params) <= 1:
            options = {} if not params else params[0]
            if not isinstance(options, dict):
                raise ProtocolError("options must be an object")
            allowed = {"cursor", "limit", "actor", "module", "batch", "account"}
            if set(options) - allowed:
                raise ProtocolError("unknown list option")
            cursor = int(options.get("cursor", 0))
            limit = int(options.get("limit", self.config.history_page_limit))
            if cursor < 0 or not 1 <= limit <= self.config.max_history_page_limit:
                raise ProtocolError("invalid page")
            items, next_cursor = self.store.list_activities(
                cursor,
                limit,
                actor=options.get("actor"),
                module=options.get("module"),
                batch=options.get("batch"),
                account=options.get("account"),
            )
            return {"version": 1, "items": items, "next_cursor": next_cursor}
        if method == "lx_listBatches" and len(params) <= 1:
            options = {} if not params else params[0]
            if not isinstance(options, dict) or set(options) - {"cursor", "limit"}:
                raise ProtocolError("invalid list options")
            cursor = int(options.get("cursor", 0))
            limit = int(options.get("limit", self.config.history_page_limit))
            if cursor < 0 or not 1 <= limit <= self.config.max_history_page_limit:
                raise ProtocolError("invalid page")
            items, next_cursor = self.store.list_batches(cursor, limit)
            return {"version": 1, "items": items, "next_cursor": next_cursor}
        raise ProtocolError("method not found")


class SubmissionConflict(RelayArchiveError):
    pass


class RequestError(RelayArchiveError):
    def __init__(self, status: int, code: str):
        super().__init__(code)
        self.status = status
        self.code = code

    def response(self) -> tuple[int, str, bytes, dict[str, str]]:
        return _error_response(self.status, self.code, None)


def _json_response(
    status: int, value: Any
) -> tuple[int, str, bytes, dict[str, str]]:
    return status, "application/json", canonical_json_bytes(value), {}


def _error_response(
    status: int, code: str, detail: str | None
) -> tuple[int, str, bytes, dict[str, str]]:
    error: dict[str, Any] = {"code": code}
    if detail:
        error["message"] = detail[:512]
    return status, "application/json", canonical_json_bytes({"error": error}), {}


def _rpc_error(identifier: Any, code: int, message: str, data: Any = None) -> bytes:
    error: dict[str, Any] = {"code": code, "message": message}
    if data is not None:
        error["data"] = data
    return canonical_json_bytes({"jsonrpc": "2.0", "id": identifier, "error": error})


def _arguments(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="layerx-relay-archive")
    parser.add_argument("--config", required=True)
    parser.add_argument("--once", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    arguments = _arguments(sys.argv[1:] if argv is None else argv)
    runtime: RelayArchive | None = None
    try:
        config = load_config(arguments.config)
        runtime = RelayArchive(config)
        runtime.bootstrap()
        runtime.synchronize()
        if arguments.once:
            return 0

        def stop(_signum: int, _frame: Any) -> None:
            raise KeyboardInterrupt

        signal.signal(signal.SIGINT, stop)
        signal.signal(signal.SIGTERM, stop)
        runtime.serve()
        return 0
    except KeyboardInterrupt:
        return 0
    except (RelayArchiveError, OSError, ValueError) as error:
        print(f"layerx-relay-archive: {error}", file=sys.stderr)
        return 1
    finally:
        if runtime is not None:
            runtime.close()


if __name__ == "__main__":
    raise SystemExit(main())
