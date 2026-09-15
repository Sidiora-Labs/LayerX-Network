"""Bounded, trust-pin-preserving discovery for LayerX relay/archive nodes."""

from __future__ import annotations

import concurrent.futures
import http.client
import ipaddress
import json
import re
import socket
import ssl
import threading
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Any


_HEX32 = re.compile(r"[0-9a-f]{64}\Z")
_DNS_LABEL = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\Z")
_DOCUMENT_KEYS = {
    "version",
    "network_id",
    "genesis_sha256",
    "sequencer_id",
    "sequencer_public_key",
    "generated_at",
    "expires_at",
    "peers",
}
_PEER_KEYS = {"url", "expires_at"}


def _integer(value: Any, name: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{name} must be an integer")
    if value < minimum or value > maximum:
        raise ValueError(f"{name} is out of range")
    return value


def _number(value: Any, name: str, minimum: float, maximum: float) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be numeric")
    result = float(value)
    if result < minimum or result > maximum:
        raise ValueError(f"{name} is out of range")
    return result


def _hex32(value: Any, name: str) -> str:
    if not isinstance(value, str) or _HEX32.fullmatch(value) is None:
        raise ValueError(f"{name} must be 64 lowercase hexadecimal characters")
    if value == "0" * 64:
        raise ValueError(f"{name} must not be zero")
    return value


def _dns_name(host: str) -> bool:
    if len(host) > 253 or host.endswith("."):
        return False
    labels = host.split(".")
    return len(labels) >= 2 and all(_DNS_LABEL.fullmatch(label) for label in labels)


@dataclass(frozen=True)
class _Origin:
    text: str
    scheme: str
    host: str
    port: int
    explicit_loopback: bool


def _parse_origin(value: Any, *, allow_explicit_loopback: bool, discovered: bool) -> _Origin:
    if not isinstance(value, str) or not value or len(value) > 2048:
        raise ValueError("peer origin must be a bounded string")
    if any(ord(character) < 0x21 or ord(character) > 0x7E for character in value):
        raise ValueError("peer origin must contain visible ASCII only")
    try:
        parsed = urllib.parse.urlsplit(value)
        port = parsed.port
    except ValueError as error:
        raise ValueError("peer origin is malformed") from error
    if (
        parsed.scheme not in {"https", "http"}
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.path not in {"", "/"}
        or parsed.hostname is None
    ):
        raise ValueError("peer origin must be an absolute origin without credentials or suffixes")
    host = parsed.hostname.lower()
    try:
        host.encode("ascii")
    except UnicodeEncodeError as error:
        raise ValueError("peer origin host must be ASCII") from error
    literal = None
    try:
        literal = ipaddress.ip_address(host)
    except ValueError:
        if not _dns_name(host):
            raise ValueError("peer origin host is not canonical")
    explicit_loopback = bool(literal is not None and literal.is_loopback)
    if literal is not None and not literal.is_global:
        if not (allow_explicit_loopback and not discovered and explicit_loopback):
            raise ValueError("peer origin address is not globally routable")
    if parsed.scheme == "http":
        if discovered or not allow_explicit_loopback or not explicit_loopback:
            raise ValueError("HTTP is limited to an explicitly enabled loopback development origin")
    if discovered and parsed.scheme != "https":
        raise ValueError("discovered peer origins must use HTTPS")
    effective_port = port if port is not None else (443 if parsed.scheme == "https" else 80)
    if effective_port < 1 or effective_port > 65535:
        raise ValueError("peer origin port is out of range")
    rendered_host = f"[{host}]" if literal is not None and literal.version == 6 else host
    default_port = 443 if parsed.scheme == "https" else 80
    suffix = "" if effective_port == default_port else f":{effective_port}"
    return _Origin(
        text=f"{parsed.scheme}://{rendered_host}{suffix}",
        scheme=parsed.scheme,
        host=host,
        port=effective_port,
        explicit_loopback=explicit_loopback,
    )


def _resolved_addresses(origin: _Origin) -> tuple[str, ...]:
    try:
        literal = ipaddress.ip_address(origin.host)
        addresses = (str(literal),)
    except ValueError:
        records = socket.getaddrinfo(
            origin.host,
            origin.port,
            type=socket.SOCK_STREAM,
            proto=socket.IPPROTO_TCP,
        )
        addresses = tuple(sorted({record[4][0] for record in records}))
    if not addresses or len(addresses) > 16:
        raise ValueError("peer origin did not resolve to a bounded address set")
    for address in addresses:
        parsed = ipaddress.ip_address(address)
        if origin.explicit_loopback and parsed.is_loopback:
            continue
        if not parsed.is_global:
            raise ValueError("peer origin resolved to a non-global address")
    return addresses


class _PinnedConnection(http.client.HTTPConnection):
    def __init__(
        self,
        origin: _Origin,
        address: str,
        timeout: float,
        context: ssl.SSLContext | None,
    ) -> None:
        super().__init__(origin.host, origin.port, timeout=timeout)
        self._origin = origin
        self._address = address
        self._context = context

    def connect(self) -> None:
        connection = socket.create_connection((self._address, self.port), self.timeout)
        if self._origin.scheme == "https":
            assert self._context is not None
            try:
                connection = self._context.wrap_socket(
                    connection,
                    server_hostname=self._origin.host,
                )
            except Exception:
                connection.close()
                raise
        self.sock = connection


@dataclass(frozen=True)
class _Identity:
    network_id: int
    genesis_sha256: str
    sequencer_id: str
    sequencer_public_key: str
    public_url: str

    @classmethod
    def load(cls, config: dict[str, Any], value: dict[str, Any]) -> "_Identity":
        if not isinstance(value, dict):
            raise ValueError("peer discovery identity must be an object")
        return cls(
            network_id=_integer(value.get("network_id"), "network_id", 1, 0xFFFFFFFF),
            genesis_sha256=_hex32(value.get("genesis_sha256"), "genesis_sha256"),
            sequencer_id=_hex32(value.get("sequencer_id"), "sequencer_id"),
            sequencer_public_key=_hex32(
                value.get("sequencer_public_key"), "sequencer_public_key"
            ),
            public_url=str(value.get("public_url", config.get("public_url", ""))),
        )


class DisabledDiscovery:
    def __init__(self, identity: _Identity) -> None:
        self._identity = identity

    def start(self) -> None:
        return None

    def stop(self) -> None:
        return None

    def sync_origins(self) -> tuple[str, ...]:
        return ()

    def validate_sync_origin(self, origin: str) -> bool:
        del origin
        return False

    def public_document(self) -> dict[str, Any]:
        now = int(time.time())
        return _document(self._identity, now, now + 300, [])


@dataclass(frozen=True)
class _Candidate:
    origin: _Origin
    expires_at: int | None
    explicit: bool


def _document(
    identity: _Identity,
    generated_at: int,
    expires_at: int,
    peers: list[dict[str, Any]],
) -> dict[str, Any]:
    return {
        "version": 1,
        "network_id": identity.network_id,
        "genesis_sha256": identity.genesis_sha256,
        "sequencer_id": identity.sequencer_id,
        "sequencer_public_key": identity.sequencer_public_key,
        "generated_at": generated_at,
        "expires_at": expires_at,
        "peers": peers,
    }


class PeerDiscovery:
    def __init__(self, config: dict[str, Any], identity: _Identity, options: dict[str, Any]) -> None:
        allowed = {
            "enabled",
            "seeds",
            "advertise_ttl_seconds",
            "refresh_interval_seconds",
            "max_peers",
            "max_advertised_peers",
            "allow_loopback_dev",
        }
        if set(options) - allowed:
            raise ValueError("peer_discovery contains unknown fields")
        allow_loopback = options.get("allow_loopback_dev", False)
        if not isinstance(allow_loopback, bool):
            raise ValueError("peer_discovery.allow_loopback_dev must be a boolean")
        self._ttl = _integer(
            options.get("advertise_ttl_seconds", 300),
            "peer_discovery.advertise_ttl_seconds",
            30,
            3600,
        )
        self._refresh_interval = _integer(
            options.get("refresh_interval_seconds", 60),
            "peer_discovery.refresh_interval_seconds",
            5,
            900,
        )
        if self._refresh_interval >= self._ttl:
            raise ValueError("peer discovery refresh interval must be shorter than its TTL")
        self._max_peers = _integer(
            options.get("max_peers", 64), "peer_discovery.max_peers", 1, 256
        )
        self._max_advertised = _integer(
            options.get("max_advertised_peers", min(32, self._max_peers)),
            "peer_discovery.max_advertised_peers",
            1,
            self._max_peers,
        )
        self._timeout = _number(
            config.get("request_timeout_seconds", 5), "request_timeout_seconds", 0.1, 30.0
        )
        configured_response_bytes = _integer(
            config.get("max_response_bytes", 8 * 1024 * 1024),
            "max_response_bytes",
            1024,
            64 * 1024 * 1024,
        )
        self._max_document_bytes = min(configured_response_bytes, 1024 * 1024)
        self._max_concurrency = _integer(
            config.get("max_concurrency", 8), "max_concurrency", 1, 64
        )
        self._identity = identity
        self._public_origin = _parse_origin(
            identity.public_url,
            allow_explicit_loopback=allow_loopback,
            discovered=False,
        )
        ca_file = config.get("ca_file")
        if ca_file is not None and (not isinstance(ca_file, str) or not ca_file):
            raise ValueError("ca_file must be a non-empty path")
        if ca_file is not None:
            ca_path = Path(ca_file)
            if not ca_path.is_file():
                raise ValueError("ca_file must name a regular file")
        self._tls_context = ssl.create_default_context(cafile=ca_file)
        raw_seeds = options.get("seeds", [])
        if not isinstance(raw_seeds, list) or len(raw_seeds) > self._max_peers:
            raise ValueError("peer_discovery.seeds must be a bounded array")
        seeds: dict[str, _Candidate] = {}
        for value in raw_seeds:
            origin = _parse_origin(
                value,
                allow_explicit_loopback=allow_loopback,
                discovered=False,
            )
            if origin.text == self._public_origin.text or origin.text in seeds:
                continue
            seeds[origin.text] = _Candidate(origin, None, True)
        self._seeds = seeds
        self._candidates = dict(seeds)
        self._verified: dict[str, tuple[_Origin, int, bool]] = {}
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        with self._lock:
            if self._thread is not None:
                return
            self._stop.clear()
            self._thread = threading.Thread(
                target=self._run,
                name="layerx-relay-peer-discovery",
                daemon=True,
            )
            self._thread.start()

    def stop(self) -> None:
        with self._lock:
            thread = self._thread
            self._thread = None
        if thread is None:
            return
        self._stop.set()
        thread.join(timeout=self._timeout + 2.0)

    def _run(self) -> None:
        while not self._stop.is_set():
            try:
                self._refresh()
            except Exception:
                pass
            self._stop.wait(self._refresh_interval)

    def _sources(self, now: int) -> list[_Candidate]:
        with self._lock:
            self._verified = {
                key: value for key, value in self._verified.items() if value[1] > now
            }
            self._candidates = {
                key: value
                for key, value in self._candidates.items()
                if value.expires_at is None or value.expires_at > now
            }
            values = list(self._seeds.values())
            for origin, expires_at, explicit in self._verified.values():
                if origin.text not in self._seeds:
                    values.append(_Candidate(origin, expires_at, explicit))
            for candidate in self._candidates.values():
                if all(candidate.origin.text != item.origin.text for item in values):
                    values.append(candidate)
            return values[: self._max_peers]

    def _refresh(self) -> None:
        now = int(time.time())
        sources = self._sources(now)
        if not sources:
            return
        workers = min(self._max_concurrency, len(sources))
        with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as executor:
            futures = {executor.submit(self._fetch_document, source.origin): source for source in sources}
            for future in concurrent.futures.as_completed(futures):
                source = futures[future]
                try:
                    document = future.result()
                    expiry, advertised = self._validate_document(document, int(time.time()))
                except (OSError, ValueError, TypeError, json.JSONDecodeError, ssl.SSLError):
                    continue
                effective_expiry = min(expiry, int(time.time()) + self._ttl)
                with self._lock:
                    self._verified[source.origin.text] = (
                        source.origin,
                        effective_expiry,
                        source.explicit,
                    )
                    self._candidates.pop(source.origin.text, None)
                    room = self._max_peers - len(self._candidates) - len(self._verified)
                    for candidate in advertised:
                        if room <= 0:
                            break
                        if (
                            candidate.origin.text == self._public_origin.text
                            or candidate.origin.text in self._verified
                            or candidate.origin.text in self._candidates
                        ):
                            continue
                        self._candidates[candidate.origin.text] = candidate
                        room -= 1

    def _fetch_document(self, origin: _Origin) -> dict[str, Any]:
        last_error: BaseException | None = None
        for address in _resolved_addresses(origin)[:4]:
            connection = _PinnedConnection(
                origin,
                address,
                self._timeout,
                self._tls_context if origin.scheme == "https" else None,
            )
            try:
                connection.request(
                    "GET",
                    "/v1/peers",
                    headers={
                        "Accept": "application/json",
                        "Connection": "close",
                        "User-Agent": "layerx-relay-archive/1",
                    },
                )
                response = connection.getresponse()
                if response.status != 200:
                    raise ValueError("peer discovery response was not successful")
                if response.getheader("Content-Encoding") not in {None, "identity"}:
                    raise ValueError("peer discovery response must not be encoded")
                media_type = response.getheader("Content-Type", "").split(";", 1)[0].strip().lower()
                if media_type != "application/json":
                    raise ValueError("peer discovery response is not JSON")
                length = response.getheader("Content-Length")
                if length is not None:
                    declared = int(length)
                    if declared < 0 or declared > self._max_document_bytes:
                        raise ValueError("peer discovery response is too large")
                body = response.read(self._max_document_bytes + 1)
                if len(body) > self._max_document_bytes:
                    raise ValueError("peer discovery response is too large")
                value = json.loads(body.decode("utf-8", errors="strict"))
                if not isinstance(value, dict):
                    raise ValueError("peer discovery response must be an object")
                return value
            except Exception as error:
                last_error = error
            finally:
                connection.close()
        if last_error is None:
            raise ValueError("peer discovery origin has no usable address")
        raise ValueError("peer discovery request failed") from last_error

    def _validate_document(
        self, value: dict[str, Any], now: int
    ) -> tuple[int, list[_Candidate]]:
        if set(value) != _DOCUMENT_KEYS or value.get("version") != 1:
            raise ValueError("peer discovery document shape is invalid")
        if _integer(value.get("network_id"), "peer network_id", 1, 0xFFFFFFFF) != self._identity.network_id:
            raise ValueError("peer network identity differs")
        for name in ("genesis_sha256", "sequencer_id", "sequencer_public_key"):
            if _hex32(value.get(name), f"peer {name}") != getattr(self._identity, name):
                raise ValueError("peer trust identity differs")
        generated_at = _integer(value.get("generated_at"), "peer generated_at", 0, 0x7FFFFFFFFFFFFFFF)
        expires_at = _integer(value.get("expires_at"), "peer expires_at", 1, 0x7FFFFFFFFFFFFFFF)
        if generated_at > now + 60 or expires_at <= now or expires_at <= generated_at:
            raise ValueError("peer discovery document is stale or future-dated")
        raw_peers = value.get("peers")
        if not isinstance(raw_peers, list) or len(raw_peers) > self._max_advertised:
            raise ValueError("peer advertisement count exceeds its bound")
        advertised: list[_Candidate] = []
        seen: set[str] = set()
        for raw_peer in raw_peers:
            if not isinstance(raw_peer, dict) or set(raw_peer) != _PEER_KEYS:
                raise ValueError("peer advertisement shape is invalid")
            peer_expiry = _integer(
                raw_peer.get("expires_at"), "advertised peer expires_at", 1, 0x7FFFFFFFFFFFFFFF
            )
            if peer_expiry <= now or peer_expiry > expires_at:
                raise ValueError("advertised peer expiration is invalid")
            origin = _parse_origin(
                raw_peer.get("url"),
                allow_explicit_loopback=False,
                discovered=True,
            )
            if origin.text in seen:
                raise ValueError("peer advertisement is duplicated")
            seen.add(origin.text)
            advertised.append(
                _Candidate(origin, min(peer_expiry, now + self._ttl), False)
            )
        return expires_at, advertised

    def sync_origins(self) -> tuple[str, ...]:
        now = int(time.time())
        with self._lock:
            candidates = [
                (origin, explicit)
                for origin, expiry, explicit in self._verified.values()
                if expiry > now
            ]
        valid = []
        for origin, explicit in candidates:
            try:
                if not explicit and origin.explicit_loopback:
                    continue
                _resolved_addresses(origin)
            except (OSError, ValueError):
                continue
            valid.append(origin.text)
        return tuple(sorted(set(valid)))

    def validate_sync_origin(self, origin: str) -> bool:
        now = int(time.time())
        with self._lock:
            record = self._verified.get(origin)
        if record is None or record[1] <= now:
            return False
        try:
            _resolved_addresses(record[0])
        except (OSError, ValueError):
            return False
        return True

    def public_document(self) -> dict[str, Any]:
        now = int(time.time())
        expiry = now + self._ttl
        peers = []
        if not self._public_origin.explicit_loopback:
            peers.append({"url": self._public_origin.text, "expires_at": expiry})
        with self._lock:
            active = sorted(
                (origin.text, min(peer_expiry, expiry))
                for origin, peer_expiry, _explicit in self._verified.values()
                if peer_expiry > now
                and not origin.explicit_loopback
                and origin.text != self._public_origin.text
            )
        for origin, peer_expiry in active[: self._max_advertised - len(peers)]:
            peers.append({"url": origin, "expires_at": peer_expiry})
        return _document(self._identity, now, expiry, peers)


def create_discovery(config: dict[str, Any], identity: dict[str, Any]) -> DisabledDiscovery | PeerDiscovery:
    if not isinstance(config, dict):
        raise ValueError("relay/archive configuration must be an object")
    validated_identity = _Identity.load(config, identity)
    options = config.get("peer_discovery")
    if options is None or options == {}:
        return DisabledDiscovery(validated_identity)
    if not isinstance(options, dict):
        raise ValueError("peer_discovery must be an object")
    enabled = options.get("enabled", True)
    if not isinstance(enabled, bool):
        raise ValueError("peer_discovery.enabled must be a boolean")
    if not enabled:
        return DisabledDiscovery(validated_identity)
    return PeerDiscovery(config, validated_identity, options)
