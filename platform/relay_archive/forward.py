from __future__ import annotations

import http.client
import json
import socket
import ssl
from dataclasses import dataclass
from typing import Any, Mapping, Sequence

try:
    from .protocol import (
        Endpoint,
        ProtocolError,
        RelayConfig,
        canonical_json_bytes,
        parse_endpoint,
        resolve_safe_addresses,
    )
except ImportError:
    from protocol import (  # type: ignore
        Endpoint,
        ProtocolError,
        RelayConfig,
        canonical_json_bytes,
        parse_endpoint,
        resolve_safe_addresses,
    )


class TransportError(ProtocolError):
    pass


@dataclass(frozen=True)
class HTTPResult:
    status: int
    headers: Mapping[str, str]
    body: bytes


@dataclass(frozen=True)
class ForwardResult:
    status: int
    content_type: str
    body: bytes
    state: str
    upstream: str | None


class _PinnedConnection(http.client.HTTPConnection):
    def __init__(
        self,
        endpoint: Endpoint,
        address: str,
        timeout: float,
        context: ssl.SSLContext | None,
    ) -> None:
        super().__init__(endpoint.host, endpoint.port, timeout=timeout)
        self._endpoint = endpoint
        self._address = address
        self._context = context

    def connect(self) -> None:
        connection = socket.create_connection((self._address, self.port), self.timeout)
        if self._endpoint.scheme == "https":
            assert self._context is not None
            try:
                connection = self._context.wrap_socket(
                    connection,
                    server_hostname=self._endpoint.host,
                )
            except BaseException:
                connection.close()
                raise
        self.sock = connection


class SafeHTTPClient:
    def __init__(self, config: RelayConfig):
        self.config = config
        self._tls = ssl.create_default_context(
            cafile=None if config.ca_file is None else str(config.ca_file)
        )
        self._tls.minimum_version = ssl.TLSVersion.TLSv1_2

    def endpoint(self, value: str, *, submission: bool = False) -> Endpoint:
        return parse_endpoint(
            value,
            "discovered upstream",
            allow_paths=("", "/v1/activities", "/rpc") if submission else ("",),
            allow_loopback_http=self.config.allow_loopback_dev,
        )

    def request(
        self,
        endpoint: Endpoint,
        method: str,
        path: str,
        *,
        headers: Mapping[str, str] | None = None,
        body: bytes | None = None,
        maximum: int | None = None,
    ) -> HTTPResult:
        if not path.startswith("/") or "\r" in path or "\n" in path:
            raise ProtocolError("HTTP request path is invalid")
        limit = self.config.max_response_bytes if maximum is None else maximum
        addresses = resolve_safe_addresses(
            endpoint.host,
            endpoint.port,
            self.config.allow_loopback_dev and endpoint.loopback,
        )
        last_error: BaseException | None = None
        for address in addresses[:4]:
            connection = _PinnedConnection(
                endpoint,
                address,
                self.config.request_timeout_seconds,
                self._tls if endpoint.scheme == "https" else None,
            )
            try:
                request_headers = {
                    "Accept-Encoding": "identity",
                    "Connection": "close",
                    "User-Agent": "layerx-relay-archive/1",
                }
                if headers is not None:
                    request_headers.update(headers)
                connection.request(method, path, body=body, headers=request_headers)
                response = connection.getresponse()
                encoding = response.getheader("Content-Encoding")
                if encoding not in (None, "identity"):
                    raise TransportError("encoded upstream responses are refused")
                length = response.getheader("Content-Length")
                if length is not None:
                    try:
                        declared = int(length)
                    except ValueError as error:
                        raise TransportError("upstream Content-Length is invalid") from error
                    if declared < 0 or declared > limit:
                        raise TransportError("upstream response exceeds the configured bound")
                response_body = response.read(limit + 1)
                if len(response_body) > limit:
                    raise TransportError("upstream response exceeds the configured bound")
                response_headers: dict[str, str] = {}
                for name, value in response.getheaders():
                    lowered = name.lower()
                    if lowered in {"content-type", "etag", "x-content-sha256", "x-layerx-batch"}:
                        response_headers[lowered] = value
                return HTTPResult(response.status, response_headers, response_body)
            except (OSError, ssl.SSLError, http.client.HTTPException, TransportError) as error:
                last_error = error
            finally:
                connection.close()
        raise TransportError("configured upstream request failed") from last_error


def _credential_headers(headers: Mapping[str, str], idempotency_key: str) -> dict[str, str]:
    result = {
        "Content-Type": "application/octet-stream",
        "Accept": "application/json",
        "Idempotency-Key": idempotency_key,
    }
    for source, target in (
        ("authorization", "Authorization"),
        ("layerx-key", "LayerX-Key"),
        ("x-layerx-key", "X-LayerX-Key"),
    ):
        value = headers.get(source)
        if value is not None:
            if "\r" in value or "\n" in value or len(value) > 8192:
                raise ProtocolError("submission credential header is invalid")
            result[target] = value
    return result


def _json_value(body: bytes) -> dict[str, Any] | None:
    try:
        value = json.loads(body)
    except (UnicodeDecodeError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def _logical_state(status: int, body: bytes) -> str:
    value = _json_value(body)
    state = value.get("state") if value is not None else None
    if not isinstance(state, str) and value is not None and isinstance(value.get("result"), dict):
        state = value["result"].get("state")
    if status == 202 or state in {"pending", "unknown"}:
        return "pending" if state == "pending" else "unknown"
    if state == "acknowledged":
        return "acknowledged"
    return "definitive"


def _retryable(status: int) -> bool:
    return status in {408, 425, 429, 500, 502, 503, 504}


def _rpc_to_rest(result: HTTPResult, upstream: str) -> ForwardResult:
    value = _json_value(result.body)
    if result.status != 200 or value is None:
        return ForwardResult(
            result.status,
            result.headers.get("content-type", "application/json"),
            result.body,
            _logical_state(result.status, result.body),
            upstream,
        )
    if isinstance(value.get("result"), dict):
        body = canonical_json_bytes(value["result"])
        return ForwardResult(200, "application/json", body, _logical_state(200, body), upstream)
    error = value.get("error")
    if not isinstance(error, dict):
        return ForwardResult(502, "application/json", canonical_json_bytes({
            "state": "unknown", "error": {"code": "invalid_upstream_response"}
        }), "unknown", upstream)
    code = error.get("code")
    data = error.get("data") if isinstance(error.get("data"), dict) else {}
    if data.get("state") in {"pending", "unknown"}:
        status = 202
    elif code == -32602:
        status = 400
    elif code == -32002:
        status = 401
    elif code == -32005:
        status = 429
    else:
        status = 503
    body = canonical_json_bytes(data if data else {"error": error})
    return ForwardResult(status, "application/json", body, _logical_state(status, body), upstream)


class SubmissionForwarder:
    def __init__(self, config: RelayConfig, client: SafeHTTPClient):
        self.config = config
        self.client = client

    def forward(
        self,
        canonical_activity: bytes,
        idempotency_key: str,
        inbound_headers: Mapping[str, str],
        commitment: str = "executed",
    ) -> ForwardResult:
        if commitment not in {"executed", "batched", "finalised"}:
            raise ProtocolError("unsupported submission commitment")
        headers = _credential_headers(inbound_headers, idempotency_key)
        saw_retryable = False
        for endpoint in self.config.submission_upstreams:
            path = endpoint.path or "/v1/activities"
            try:
                if path == "/rpc":
                    rpc_body = canonical_json_bytes(
                        {
                            "jsonrpc": "2.0",
                            "id": "relay-archive",
                            "method": "lx_sendActivity",
                            "params": [canonical_activity.hex(), commitment],
                        }
                    )
                    rpc_headers = dict(headers)
                    rpc_headers["Content-Type"] = "application/json"
                    answer = self.client.request(
                        endpoint,
                        "POST",
                        "/rpc",
                        headers=rpc_headers,
                        body=rpc_body,
                    )
                    translated = _rpc_to_rest(answer, endpoint.url)
                else:
                    answer = self.client.request(
                        endpoint,
                        "POST",
                        "/v1/activities",
                        headers=headers,
                        body=canonical_activity,
                    )
                    translated = ForwardResult(
                        answer.status,
                        answer.headers.get("content-type", "application/json"),
                        answer.body,
                        _logical_state(answer.status, answer.body),
                        endpoint.url,
                    )
            except TransportError:
                saw_retryable = True
                continue
            if _retryable(translated.status):
                saw_retryable = True
                continue
            return translated
        body = canonical_json_bytes(
            {
                "state": "unknown",
                "error": {
                    "code": "submission_unavailable",
                    "retryable": bool(saw_retryable or self.config.submission_upstreams),
                },
            }
        )
        return ForwardResult(503, "application/json", body, "unknown", None)
