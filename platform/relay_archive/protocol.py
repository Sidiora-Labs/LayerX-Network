from __future__ import annotations

import hashlib
import ipaddress
import json
import os
import re
import socket
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Mapping, Sequence
from urllib.parse import SplitResult, urlsplit, urlunsplit


U64_MAX = (1 << 64) - 1
MAX_ACTIVITY_BYTES = 1_048_576
MAX_BATCH_BYTES = 16_777_216
HEX_32 = re.compile(r"^[0-9a-f]{64}$")
DECIMAL = re.compile(r"^(0|[1-9][0-9]*)$")


class RelayArchiveError(Exception):
    pass


class ConfigError(RelayArchiveError):
    pass


class ProtocolError(RelayArchiveError):
    pass


class IntegrityError(RelayArchiveError):
    pass


class CodecError(RelayArchiveError):
    pass


class MissingBatch(CodecError):
    pass


@dataclass(frozen=True)
class Endpoint:
    url: str
    scheme: str
    host: str
    port: int
    path: str
    loopback: bool

    @property
    def origin(self) -> str:
        host = self.host
        if ":" in host and not host.startswith("["):
            host = f"[{host}]"
        default = 443 if self.scheme == "https" else 80
        authority = host if self.port == default else f"{host}:{self.port}"
        return f"{self.scheme}://{authority}"


@dataclass(frozen=True)
class RelayConfig:
    config_path: Path
    network_id: int
    genesis_sha256: str
    sequencer_id: str
    sequencer_public_key: str
    sequencer_first_batch: int
    sequencer_last_batch: int
    genesis_manifest: Path | None
    genesis_snapshot: Path | None
    upstreams: tuple[Endpoint, ...]
    submission_upstreams: tuple[Endpoint, ...]
    source_log: Path | None
    source_lni_socket: Path | None
    source_submission_token_file: Path | None
    data_dir: Path
    listen_host: str
    listen_port: int
    public_url: str
    tls_cert: Path | None
    tls_key: Path | None
    ca_file: Path | None
    codec: Path
    poll_interval_seconds: float
    request_timeout_seconds: float
    codec_timeout_seconds: float
    max_activity_bytes: int
    max_batch_bytes: int
    max_bootstrap_bytes: int
    max_response_bytes: int
    history_page_limit: int
    max_history_page_limit: int
    max_concurrency: int
    allow_loopback_dev: bool
    peer_discovery: Mapping[str, Any]

    @property
    def listen_is_loopback(self) -> bool:
        return host_is_loopback(self.listen_host)

    @property
    def identity(self) -> dict[str, Any]:
        return {
            "network_id": self.network_id,
            "genesis_sha256": self.genesis_sha256,
            "sequencer_id": self.sequencer_id,
            "sequencer_public_key": self.sequencer_public_key,
            "public_url": self.public_url,
        }


def canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode(
        "utf-8"
    )


def sha256_hex(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def require_hex32(value: Any, name: str) -> str:
    if not isinstance(value, str) or HEX_32.fullmatch(value) is None:
        raise ProtocolError(f"{name} must be 32-byte lowercase hexadecimal")
    return value


def require_decimal(value: Any, name: str) -> int:
    if not isinstance(value, str) or DECIMAL.fullmatch(value) is None:
        raise ProtocolError(f"{name} must be a canonical decimal string")
    number = int(value)
    if number > U64_MAX:
        raise ProtocolError(f"{name} exceeds uint64")
    return number


def decimal_string(value: int) -> str:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= U64_MAX:
        raise ProtocolError("value is not uint64")
    return str(value)


def decode_hex(value: Any, name: str, maximum_bytes: int) -> bytes:
    if (
        not isinstance(value, str)
        or len(value) % 2 != 0
        or len(value) // 2 > maximum_bytes
        or any(character not in "0123456789abcdef" for character in value)
    ):
        raise ProtocolError(f"{name} is not bounded lowercase hexadecimal")
    try:
        return bytes.fromhex(value)
    except ValueError as error:
        raise ProtocolError(f"{name} is invalid hexadecimal") from error


def read_file_bounded(path: Path, maximum: int) -> bytes:
    try:
        with path.open("rb") as handle:
            value = handle.read(maximum + 1)
    except OSError as error:
        raise ProtocolError(f"cannot read {path}: {error.strerror or error}") from error
    if len(value) > maximum:
        raise ProtocolError(f"{path} exceeds configured size limit")
    return value


def host_is_loopback(host: str) -> bool:
    if host.lower().rstrip(".") == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def resolve_safe_addresses(host: str, port: int, allow_loopback: bool) -> tuple[str, ...]:
    try:
        answers = socket.getaddrinfo(host, port, type=socket.SOCK_STREAM)
    except OSError as error:
        raise ProtocolError(f"cannot resolve configured endpoint {host}: {error}") from error
    addresses: list[str] = []
    for answer in answers:
        address = answer[4][0]
        try:
            parsed = ipaddress.ip_address(address)
        except ValueError as error:
            raise ProtocolError("resolver returned a non-IP address") from error
        allowed = parsed.is_global or (allow_loopback and parsed.is_loopback)
        if not allowed:
            raise ProtocolError(f"endpoint {host} resolves to a non-public address")
        normalized = str(parsed)
        if normalized not in addresses:
            addresses.append(normalized)
    if not addresses:
        raise ProtocolError(f"configured endpoint {host} has no usable address")
    return tuple(addresses)


def parse_endpoint(
    value: Any,
    name: str,
    *,
    allow_paths: Sequence[str],
    allow_loopback_http: bool,
) -> Endpoint:
    if not isinstance(value, str) or len(value) > 2048:
        raise ConfigError(f"{name} must be a bounded absolute URL")
    parsed: SplitResult = urlsplit(value)
    if (
        parsed.scheme not in {"https", "http"}
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
    ):
        raise ConfigError(f"{name} must be an HTTPS URL without credentials, query, or fragment")
    path = parsed.path.rstrip("/")
    if path not in allow_paths:
        raise ConfigError(f"{name} has an unsupported path")
    try:
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
    except ValueError as error:
        raise ConfigError(f"{name} has an invalid port") from error
    if not 1 <= port <= 65535:
        raise ConfigError(f"{name} has an invalid port")
    host = parsed.hostname
    loopback = host_is_loopback(host)
    if parsed.scheme != "https" and not (allow_loopback_http and loopback):
        raise ConfigError(f"{name} must use HTTPS except for explicit loopback development")
    normalized_host = host
    if ":" in normalized_host and not normalized_host.startswith("["):
        normalized_host = f"[{normalized_host}]"
    default = 443 if parsed.scheme == "https" else 80
    authority = normalized_host if port == default else f"{normalized_host}:{port}"
    normalized = urlunsplit((parsed.scheme, authority, path, "", ""))
    return Endpoint(normalized, parsed.scheme, host, port, path, loopback)


def _strict_int(value: Any, name: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise ConfigError(f"{name} must be an integer from {minimum} through {maximum}")
    return value


def _strict_float(value: Any, name: str, minimum: float, maximum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ConfigError(f"{name} must be numeric")
    result = float(value)
    if not minimum <= result <= maximum:
        raise ConfigError(f"{name} must be from {minimum} through {maximum}")
    return result


def _u64_config(value: Any, name: str) -> int:
    if isinstance(value, bool):
        raise ConfigError(f"{name} must be uint64")
    if isinstance(value, int):
        result = value
    elif isinstance(value, str) and DECIMAL.fullmatch(value):
        result = int(value)
    else:
        raise ConfigError(f"{name} must be uint64")
    if not 0 <= result <= U64_MAX:
        raise ConfigError(f"{name} must be uint64")
    return result


def _path(base: Path, value: Any, name: str, required: bool = False) -> Path | None:
    if value is None:
        if required:
            raise ConfigError(f"{name} is required")
        return None
    if not isinstance(value, str) or not value or "\x00" in value:
        raise ConfigError(f"{name} must be a path")
    candidate = Path(value)
    if not candidate.is_absolute():
        candidate = base / candidate
    return candidate.resolve(strict=False)


def _listen(value: Any) -> tuple[str, int]:
    if not isinstance(value, str) or not value:
        raise ConfigError("listen must be host:port")
    parsed = urlsplit(f"//{value}")
    if not parsed.hostname or parsed.path not in {"", "/"} or parsed.username is not None:
        raise ConfigError("listen must be host:port")
    try:
        port = parsed.port
    except ValueError as error:
        raise ConfigError("listen has an invalid port") from error
    if port is None or not 1 <= port <= 65535:
        raise ConfigError("listen has an invalid port")
    return parsed.hostname, port


def load_config(path: str | os.PathLike[str]) -> RelayConfig:
    config_path = Path(path).resolve(strict=True)
    raw = read_file_bounded(config_path, 1_048_576)
    try:
        document = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ConfigError("configuration must be valid UTF-8 JSON") from error
    if not isinstance(document, dict):
        raise ConfigError("configuration root must be an object")
    allowed = {
        "network_id",
        "genesis_sha256",
        "sequencer_id",
        "sequencer_public_key",
        "sequencer_first_batch",
        "sequencer_last_batch",
        "genesis_manifest",
        "genesis_snapshot",
        "upstreams",
        "submission_upstreams",
        "source_log",
        "source_lni_socket",
        "source_submission_token_file",
        "data_dir",
        "listen",
        "public_url",
        "tls_cert",
        "tls_key",
        "ca_file",
        "codec",
        "poll_interval_seconds",
        "request_timeout_seconds",
        "codec_timeout_seconds",
        "max_activity_bytes",
        "max_batch_bytes",
        "max_bootstrap_bytes",
        "max_response_bytes",
        "history_page_limit",
        "max_history_page_limit",
        "max_concurrency",
        "allow_loopback_dev",
        "peer_discovery",
    }
    unknown = sorted(set(document) - allowed)
    if unknown:
        raise ConfigError(f"unknown configuration field: {unknown[0]}")
    base = config_path.parent
    network_id = _strict_int(document.get("network_id"), "network_id", 1, (1 << 32) - 1)
    try:
        genesis_sha256 = require_hex32(document.get("genesis_sha256"), "genesis_sha256")
        sequencer_id = require_hex32(document.get("sequencer_id"), "sequencer_id")
        sequencer_public_key = require_hex32(
            document.get("sequencer_public_key"), "sequencer_public_key"
        )
    except ProtocolError as error:
        raise ConfigError(str(error)) from error
    first_batch = _u64_config(document.get("sequencer_first_batch", 1), "sequencer_first_batch")
    last_batch = _u64_config(
        document.get("sequencer_last_batch", str(U64_MAX)), "sequencer_last_batch"
    )
    if first_batch > last_batch:
        raise ConfigError("sequencer batch authorization range is empty")
    manifest = _path(base, document.get("genesis_manifest"), "genesis_manifest")
    snapshot = _path(base, document.get("genesis_snapshot"), "genesis_snapshot")
    if (manifest is None) != (snapshot is None):
        raise ConfigError("genesis_manifest and genesis_snapshot must be configured together")
    allow_loopback = document.get("allow_loopback_dev", True)
    if not isinstance(allow_loopback, bool):
        raise ConfigError("allow_loopback_dev must be boolean")
    upstream_values = document.get("upstreams", [])
    submission_values = document.get("submission_upstreams", [])
    if not isinstance(upstream_values, list) or not isinstance(submission_values, list):
        raise ConfigError("upstreams and submission_upstreams must be arrays")
    if len(upstream_values) > 64 or len(submission_values) > 16:
        raise ConfigError("too many configured upstreams")
    upstreams = tuple(
        parse_endpoint(
            value,
            f"upstreams[{index}]",
            allow_paths=("",),
            allow_loopback_http=allow_loopback,
        )
        for index, value in enumerate(upstream_values)
    )
    submissions = tuple(
        parse_endpoint(
            value,
            f"submission_upstreams[{index}]",
            allow_paths=("", "/v1/activities", "/rpc"),
            allow_loopback_http=allow_loopback,
        )
        for index, value in enumerate(submission_values)
    )
    data_dir = _path(base, document.get("data_dir"), "data_dir", required=True)
    codec = _path(base, document.get("codec"), "codec", required=True)
    assert data_dir is not None and codec is not None
    listen_host, listen_port = _listen(document.get("listen"))
    public = parse_endpoint(
        document.get("public_url"),
        "public_url",
        allow_paths=("",),
        allow_loopback_http=allow_loopback,
    )
    tls_cert = _path(base, document.get("tls_cert"), "tls_cert")
    tls_key = _path(base, document.get("tls_key"), "tls_key")
    if (tls_cert is None) != (tls_key is None):
        raise ConfigError("tls_cert and tls_key must be configured together")
    if not host_is_loopback(listen_host) and tls_cert is None:
        raise ConfigError("TLS certificate and key are mandatory for a non-loopback listener")
    source_lni = _path(base, document.get("source_lni_socket"), "source_lni_socket")
    source_token = _path(
        base,
        document.get("source_submission_token_file"),
        "source_submission_token_file",
    )
    if (source_lni is None) != (source_token is None):
        raise ConfigError(
            "source_lni_socket and source_submission_token_file must be configured together"
        )
    peer_discovery = document.get("peer_discovery", {})
    if not isinstance(peer_discovery, dict):
        raise ConfigError("peer_discovery must be an object")
    maximum_page = _strict_int(
        document.get("max_history_page_limit", 500),
        "max_history_page_limit",
        1,
        5000,
    )
    default_page = _strict_int(
        document.get("history_page_limit", 100), "history_page_limit", 1, maximum_page
    )
    return RelayConfig(
        config_path=config_path,
        network_id=network_id,
        genesis_sha256=genesis_sha256,
        sequencer_id=sequencer_id,
        sequencer_public_key=sequencer_public_key,
        sequencer_first_batch=first_batch,
        sequencer_last_batch=last_batch,
        genesis_manifest=manifest,
        genesis_snapshot=snapshot,
        upstreams=upstreams,
        submission_upstreams=submissions,
        source_log=_path(base, document.get("source_log"), "source_log"),
        source_lni_socket=source_lni,
        source_submission_token_file=source_token,
        data_dir=data_dir,
        listen_host=listen_host,
        listen_port=listen_port,
        public_url=public.origin,
        tls_cert=tls_cert,
        tls_key=tls_key,
        ca_file=_path(base, document.get("ca_file"), "ca_file"),
        codec=codec,
        poll_interval_seconds=_strict_float(
            document.get("poll_interval_seconds", 2.0),
            "poll_interval_seconds",
            0.1,
            300.0,
        ),
        request_timeout_seconds=_strict_float(
            document.get("request_timeout_seconds", 10.0),
            "request_timeout_seconds",
            0.1,
            300.0,
        ),
        codec_timeout_seconds=_strict_float(
            document.get("codec_timeout_seconds", 30.0),
            "codec_timeout_seconds",
            0.1,
            600.0,
        ),
        max_activity_bytes=_strict_int(
            document.get("max_activity_bytes", MAX_ACTIVITY_BYTES),
            "max_activity_bytes",
            1,
            MAX_ACTIVITY_BYTES,
        ),
        max_batch_bytes=_strict_int(
            document.get("max_batch_bytes", MAX_BATCH_BYTES),
            "max_batch_bytes",
            1,
            MAX_BATCH_BYTES,
        ),
        max_bootstrap_bytes=_strict_int(
            document.get("max_bootstrap_bytes", 268_435_456),
            "max_bootstrap_bytes",
            1,
            1_073_741_824,
        ),
        max_response_bytes=_strict_int(
            document.get("max_response_bytes", MAX_BATCH_BYTES + 1_048_576),
            "max_response_bytes",
            1,
            268_435_456,
        ),
        history_page_limit=default_page,
        max_history_page_limit=maximum_page,
        max_concurrency=_strict_int(
            document.get("max_concurrency", 16), "max_concurrency", 1, 128
        ),
        allow_loopback_dev=allow_loopback,
        peer_discovery=peer_discovery,
    )


class NativeCodec:
    def __init__(self, config: RelayConfig):
        self.config = config
        if not config.codec.is_file() or not os.access(config.codec, os.X_OK):
            raise ConfigError("codec must name an executable regular file")

    def _run(
        self,
        arguments: Sequence[str],
        body: bytes | None,
        maximum: int,
        *,
        missing_batch_exit: bool = False,
    ) -> bytes:
        command = [str(self.config.codec), *arguments]
        try:
            process = subprocess.Popen(
                command,
                stdin=subprocess.PIPE if body is not None else subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                close_fds=True,
            )
            stdout, stderr = process.communicate(body, timeout=self.config.codec_timeout_seconds)
        except subprocess.TimeoutExpired as error:
            process.kill()
            process.communicate()
            raise CodecError("native codec timed out") from error
        except OSError as error:
            raise CodecError(f"cannot execute native codec: {error.strerror or error}") from error
        if missing_batch_exit and process.returncode == 3:
            raise MissingBatch("canonical batch is not yet available")
        if process.returncode != 0:
            diagnostic = stderr[:2048].decode("utf-8", "replace").strip()
            raise CodecError(diagnostic or f"native codec exited {process.returncode}")
        if stderr:
            raise CodecError("native codec wrote diagnostics on successful execution")
        if not stdout or len(stdout) > maximum:
            raise CodecError("native codec returned an empty or oversized result")
        return stdout

    def _json(
        self,
        arguments: Sequence[str],
        body: bytes | None = None,
        maximum: int | None = None,
    ) -> dict[str, Any]:
        raw = self._run(
            arguments,
            body,
            self.config.max_response_bytes if maximum is None else maximum,
        )
        try:
            value = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise CodecError("native codec returned invalid JSON") from error
        if not isinstance(value, dict):
            raise CodecError("native codec returned a non-object result")
        return value

    def genesis(self, manifest: Path, snapshot: Path) -> dict[str, Any]:
        value = self._json(
            [
                "genesis",
                str(self.config.network_id),
                self.config.sequencer_public_key,
                str(manifest),
                str(snapshot),
            ]
        )
        validate_genesis_metadata(value, self.config)
        return value

    def export(self, log: Path, batch_number: int) -> bytes:
        return self._run(
            ["export", str(log), decimal_string(batch_number)],
            None,
            self.config.max_batch_bytes,
            missing_batch_exit=True,
        )

    def verify(self, canonical_batch: bytes) -> dict[str, Any]:
        if not canonical_batch or len(canonical_batch) > self.config.max_batch_bytes:
            raise ProtocolError("canonical batch is empty or oversized")
        value = self._json(
            [
                "verify",
                str(self.config.network_id),
                self.config.sequencer_id,
                self.config.sequencer_public_key,
                str(self.config.sequencer_first_batch),
                str(self.config.sequencer_last_batch),
            ],
            canonical_batch,
            min(68_157_440, self.config.max_batch_bytes * 4 + 1_048_576),
        )
        validate_batch_metadata(value, self.config)
        return value

    def activity(self, canonical_activity: bytes) -> dict[str, Any]:
        if not canonical_activity or len(canonical_activity) > self.config.max_activity_bytes:
            raise ProtocolError("canonical activity is empty or oversized")
        value = self._json(
            ["activity"],
            canonical_activity,
            self.config.max_activity_bytes * 2 + 65_536,
        )
        validate_activity_metadata(value, self.config, canonical_activity)
        return value

    def submit(self, socket_path: Path, canonical_activity: bytes) -> dict[str, Any]:
        value = self._json(["submit", str(socket_path)], canonical_activity)
        if value.get("state") not in {"acknowledged", "refused"}:
            raise CodecError("native submit returned an invalid admission outcome")
        require_hex32(value.get("activity_id"), "submit.activity_id")
        require_hex32(value.get("idempotency_key"), "submit.idempotency_key")
        if value["state"] == "refused":
            error = value.get("error")
            if not isinstance(error, dict) or error.get("code") != "native_refusal":
                raise CodecError("native submit returned a malformed refusal")
            result = error.get("result_code")
            if type(result) is not int or result == 0 or not -(1 << 31) <= result < (1 << 31):
                raise CodecError("native submit returned an invalid refusal code")
        return value


def validate_genesis_metadata(value: Mapping[str, Any], config: RelayConfig) -> None:
    if value.get("network_id") != config.network_id:
        raise IntegrityError("genesis network does not match the configured pin")
    if value.get("signer_public_key") != config.sequencer_public_key:
        raise IntegrityError("genesis signer does not match the configured pin")
    for field in (
        "state_root",
        "receipt_state_root",
        "snapshot_digest",
        "signer_public_key",
        "manifest_commitment",
    ):
        require_hex32(value.get(field), f"genesis.{field}")
    for field in ("genesis_timestamp_ms", "global_sequence"):
        require_decimal(value.get(field), f"genesis.{field}")
    protocol_version = value.get("protocol_version")
    if isinstance(protocol_version, bool) or not isinstance(protocol_version, int):
        raise ProtocolError("genesis.protocol_version must be an integer")
    if not 1 <= protocol_version <= 0xFFFF:
        raise ProtocolError("genesis.protocol_version is out of range")


def validate_activity_metadata(
    value: Mapping[str, Any], config: RelayConfig, canonical_activity: bytes
) -> None:
    for field in ("activity_id", "idempotency_key", "actor_account"):
        require_hex32(value.get(field), f"activity.{field}")
    if value.get("network_id") != config.network_id:
        raise IntegrityError("activity network does not match the configured pin")
    actor = value.get("actor")
    if not isinstance(actor, str) or not actor or len(actor.encode("utf-8")) > 512:
        raise ProtocolError("activity.actor is invalid")
    canonical = decode_hex(
        value.get("canonical_hex"), "activity.canonical_hex", config.max_activity_bytes
    )
    if canonical != canonical_activity:
        raise IntegrityError("native activity decoder did not preserve exact bytes")
    for field, maximum in (
        ("protocol_version", 0xFFFF),
        ("activity_type", 0xFFFFFFFF),
        ("module", 0xFFFF),
        ("ordinal", 0xFFFF),
    ):
        number = value.get(field)
        if isinstance(number, bool) or not isinstance(number, int) or not 0 <= number <= maximum:
            raise ProtocolError(f"activity.{field} is out of range")
    require_decimal(value.get("account_sequence"), "activity.account_sequence")


def validate_batch_metadata(value: Mapping[str, Any], config: RelayConfig) -> None:
    batch_number = require_decimal(value.get("batch_number"), "batch.batch_number")
    if not config.sequencer_first_batch <= batch_number <= config.sequencer_last_batch:
        raise IntegrityError("batch falls outside the pinned sequencer authorization")
    first = require_decimal(value.get("first_sequence"), "batch.first_sequence")
    last = require_decimal(value.get("last_sequence"), "batch.last_sequence")
    if first > last:
        raise ProtocolError("batch sequence range is empty")
    if value.get("network_id") != config.network_id:
        raise IntegrityError("batch network does not match the configured pin")
    if value.get("sequencer_id") != config.sequencer_id:
        raise IntegrityError("batch sequencer does not match the configured pin")
    for field in ("batch_id", "previous_state_root", "resulting_state_root", "sequencer_id"):
        require_hex32(value.get(field), f"batch.{field}")
    for field in ("epoch", "timestamp_ms"):
        require_decimal(value.get(field), f"batch.{field}")
    for field, maximum in (("network_id", 0xFFFFFFFF), ("protocol_version", 0xFFFF)):
        number = value.get(field)
        if isinstance(number, bool) or not isinstance(number, int) or not 0 <= number <= maximum:
            raise ProtocolError(f"batch.{field} is out of range")
    decode_hex(value.get("header_hex"), "batch.header_hex", config.max_batch_bytes)
    decode_hex(value.get("signature_hex"), "batch.signature_hex", 4096)
    activities = value.get("activities")
    maintenance = value.get("maintenance")
    if not isinstance(activities, list) or not isinstance(maintenance, list):
        raise ProtocolError("batch sections must be arrays")
    if len(activities) + len(maintenance) > 262_144:
        raise ProtocolError("batch contains too many indexed records")
    observed: list[int] = []
    activity_ids: set[str] = set()
    for index, activity in enumerate(activities):
        if not isinstance(activity, dict):
            raise ProtocolError("batch activity must be an object")
        identifier = require_hex32(activity.get("activity_id"), f"activities[{index}].activity_id")
        if identifier in activity_ids:
            raise IntegrityError("batch contains a duplicate activity")
        activity_ids.add(identifier)
        sequence = require_decimal(activity.get("sequence"), f"activities[{index}].sequence")
        observed.append(sequence)
        actor = activity.get("actor")
        if not isinstance(actor, str) or not actor or len(actor.encode("utf-8")) > 512:
            raise ProtocolError("batch activity actor is invalid")
        for field in ("module", "ordinal"):
            number = activity.get(field)
            if isinstance(number, bool) or not isinstance(number, int) or not 0 <= number <= 0xFFFF:
                raise ProtocolError(f"batch activity {field} is out of range")
        result = activity.get("result_code")
        if isinstance(result, bool) or not isinstance(result, int) or not -(1 << 31) <= result < (1 << 31):
            raise ProtocolError("batch activity result_code is out of range")
        decode_hex(
            activity.get("canonical_hex"),
            f"activities[{index}].canonical_hex",
            config.max_activity_bytes,
        )
        decode_hex(
            activity.get("receipt_hex"),
            f"activities[{index}].receipt_hex",
            config.max_batch_bytes,
        )
        accounts = activity.get("accounts")
        if not isinstance(accounts, list) or len(accounts) > 64:
            raise ProtocolError("batch activity accounts are invalid")
        seen_accounts: set[str] = set()
        for account in accounts:
            normalized = require_hex32(account, "activity account")
            if normalized in seen_accounts:
                raise IntegrityError("batch activity accounts are not unique")
            seen_accounts.add(normalized)
    for index, record in enumerate(maintenance):
        if not isinstance(record, dict):
            raise ProtocolError("batch maintenance record must be an object")
        observed.append(
            require_decimal(record.get("sequence"), f"maintenance[{index}].sequence")
        )
        if record.get("kind") not in {
            "batch_maintenance",
            "occupancy_maintenance",
            "unknown_receipt",
        }:
            raise ProtocolError("batch maintenance kind is invalid")
        if record.get("result_code") is not None:
            raise ProtocolError("batch maintenance result_code must be null")
        decode_hex(
            record.get("receipt_hex"),
            f"maintenance[{index}].receipt_hex",
            config.max_batch_bytes,
        )
    if len(observed) != last - first + 1:
        raise IntegrityError("batch indexed records do not cover its sequence range")
    for offset, sequence in enumerate(sorted(observed)):
        if sequence != first + offset:
            raise IntegrityError("batch indexed records contain a sequence gap or duplicate")
