#!/usr/bin/env python3
"""Recorded JSON-RPC server for the explorer's local compose stack.

The backend needs a node to talk to before it will boot, and the Paxeer X
capability probe needs one to answer `eth_getCode` at the LayerX precompiles.
This server stands in for that node from responses recorded off the Paxeer X
Network public JSON-RPC surface, so the stack can be brought up and proved
without reaching a node at all.

It uses nothing outside the Python standard library and reads every answer it
gives from the fixture directory; it never invents one.

Fixtures
--------

One file per method, `<method>.json`, in the directory named by
`RPC_FIXTURE_DIR`:

    {
      "method": "eth_getCode",
      "recorded_from": "<which surface the answers were read from>",
      "responses": [
        {"match": ["0x...1013", "*"], "result": "0x"}
      ]
    }

`match` is compared element by element against the leading parameters of the
request:

  * `"*"` matches one parameter of any value;
  * an object matches a parameter object that carries every key it names with
    an equal value, and may ignore keys it does not name;
  * anything else matches on equality.

Parameters beyond the length of `match` are not compared, so `[]` matches every
call of the method. The first entry that matches wins, and each entry carries
exactly one of `result` or `error`.

A method with no fixture file, or a call no entry matches, is answered with
JSON-RPC error -32601 naming the method and its parameters, and is written to
the log. Nothing is answered by default.

HTTP
----

  * `POST` on any path: one JSON-RPC request object, or a batch array of them.
  * `GET /__health`: `{"status": "ok", "methods": [...]}` once the fixtures are
    loaded, for the compose health check.
  * `GET /__journal`: every call this server has answered, in arrival order, as
    `{"calls": [{"method": ..., "params": [...], "matched": true|false}]}`, so a
    test can assert which requests the backend actually made.

Environment: `RPC_FIXTURE_DIR` (required), `RPC_FIXTURE_HOST` (default
`0.0.0.0`), `RPC_FIXTURE_PORT` (default `8545`).
"""

import json
import os
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

WILDCARD = "*"
METHOD_NOT_RECORDED = -32601
PARSE_ERROR = -32700
INVALID_REQUEST = -32600


class FixtureError(Exception):
    """A fixture directory that cannot be served as written."""


class Fixtures:
    """Every recorded response, indexed by method."""

    def __init__(self, by_method):
        self._by_method = by_method

    @classmethod
    def load(cls, directory):
        if not os.path.isdir(directory):
            raise FixtureError("fixture directory %s does not exist" % directory)

        by_method = {}

        for name in sorted(os.listdir(directory)):
            if not name.endswith(".json"):
                continue

            path = os.path.join(directory, name)

            with open(path, encoding="utf-8") as handle:
                try:
                    document = json.load(handle)
                except ValueError as error:
                    raise FixtureError("%s is not valid JSON: %s" % (path, error))

            method = document.get("method")

            if not isinstance(method, str) or not method:
                raise FixtureError("%s has no method" % path)

            if method in by_method:
                raise FixtureError("method %s is recorded twice" % method)

            responses = document.get("responses")

            if not isinstance(responses, list) or not responses:
                raise FixtureError("%s records no responses" % path)

            for response in responses:
                if not isinstance(response, dict):
                    raise FixtureError("%s has a response that is not an object" % path)

                if not isinstance(response.get("match"), list):
                    raise FixtureError("%s has a response without a match list" % path)

                if ("result" in response) == ("error" in response):
                    raise FixtureError(
                        "%s has a response that is not exactly one of result or error" % path
                    )

            by_method[method] = responses

        if not by_method:
            raise FixtureError("no fixture files in %s" % directory)

        return cls(by_method)

    @property
    def methods(self):
        return sorted(self._by_method)

    def answer(self, method, params):
        """The recorded answer for one call, or None when none was recorded."""
        for response in self._by_method.get(method, []):
            if self._matches(response["match"], params):
                if "result" in response:
                    return {"result": response["result"]}

                return {"error": response["error"]}

        return None

    @classmethod
    def _matches(cls, match, params):
        if len(match) > len(params):
            return False

        return all(cls._matches_one(expected, actual) for expected, actual in zip(match, params))

    @classmethod
    def _matches_one(cls, expected, actual):
        if expected == WILDCARD:
            return True

        if isinstance(expected, dict):
            if not isinstance(actual, dict):
                return False

            return all(key in actual and actual[key] == value for key, value in expected.items())

        return expected == actual


class Journal:
    """Every call the server answered, in arrival order."""

    def __init__(self):
        self._calls = []
        self._lock = threading.Lock()

    def record(self, method, params, matched):
        with self._lock:
            self._calls.append({"method": method, "params": params, "matched": matched})

    def calls(self):
        with self._lock:
            return list(self._calls)


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    server_version = "paxeer-x-rpc-fixture"

    fixtures = None
    journal = None

    def do_GET(self):  # noqa: N802 - the name is BaseHTTPRequestHandler's
        path = self.path.split("?", 1)[0]

        if path == "/__health":
            self._send_json(200, {"status": "ok", "methods": self.fixtures.methods})
        elif path == "/__journal":
            self._send_json(200, {"calls": self.journal.calls()})
        else:
            self._send_json(404, {"error": "no such path: %s" % path})

    def do_POST(self):  # noqa: N802 - the name is BaseHTTPRequestHandler's
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length)

        try:
            payload = json.loads(body.decode("utf-8"))
        except (ValueError, UnicodeDecodeError) as error:
            self._send_json(200, self._failure(None, PARSE_ERROR, "invalid JSON: %s" % error))
            return

        if isinstance(payload, list):
            if not payload:
                self._send_json(
                    200, self._failure(None, INVALID_REQUEST, "an empty batch is not a request")
                )
                return

            self._send_json(200, [self._answer(request) for request in payload])
        else:
            self._send_json(200, self._answer(payload))

    def log_message(self, format, *args):  # noqa: A002 - the name is the base class's
        sys.stderr.write("[rpc-fixture] %s\n" % (format % args))
        sys.stderr.flush()

    def _answer(self, request):
        if not isinstance(request, dict):
            return self._failure(None, INVALID_REQUEST, "a request must be an object")

        identifier = request.get("id")
        method = request.get("method")
        params = request.get("params")

        if not isinstance(method, str):
            return self._failure(identifier, INVALID_REQUEST, "a request must name a method")

        if params is None:
            params = []

        if not isinstance(params, list):
            return self._failure(identifier, INVALID_REQUEST, "params must be a list")

        answer = self.fixtures.answer(method, params)
        self.journal.record(method, params, answer is not None)

        if answer is None:
            self.log_message(
                "no recorded response for %s %s", method, json.dumps(params, sort_keys=True)
            )

            return self._failure(
                identifier,
                METHOD_NOT_RECORDED,
                "no recorded response for %s with params %s"
                % (method, json.dumps(params, sort_keys=True)),
            )

        response = {"jsonrpc": "2.0", "id": identifier}
        response.update(answer)

        return response

    @staticmethod
    def _failure(identifier, code, message):
        return {
            "jsonrpc": "2.0",
            "id": identifier,
            "error": {"code": code, "message": message},
        }

    def _send_json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")

        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def main():
    directory = os.environ.get("RPC_FIXTURE_DIR")

    if not directory:
        sys.stderr.write("[rpc-fixture] RPC_FIXTURE_DIR is not set\n")
        return 2

    try:
        fixtures = Fixtures.load(directory)
    except FixtureError as error:
        sys.stderr.write("[rpc-fixture] %s\n" % error)
        return 2

    host = os.environ.get("RPC_FIXTURE_HOST", "0.0.0.0")
    port = int(os.environ.get("RPC_FIXTURE_PORT", "8545"))

    Handler.fixtures = fixtures
    Handler.journal = Journal()

    server = ThreadingHTTPServer((host, port), Handler)
    server.daemon_threads = True

    sys.stderr.write(
        "[rpc-fixture] serving %d recorded methods on %s:%d\n" % (len(fixtures.methods), host, port)
    )
    sys.stderr.flush()

    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
