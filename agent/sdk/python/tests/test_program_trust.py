from __future__ import annotations

from hashlib import sha256
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import threading
import unittest

from layerx_sdk import AgentHttpTransport, ProductionClient
from layerx_sdk.program_wire import (
    assert_fresh_simulation_observation,
    decode_signed_program_call,
)
from layerx_sdk.programs import ProgramCall, ProgramOperations, ProgramTrustContext


_REPO_ROOT = Path(__file__).resolve().parents[4]
_SIGNATURES_PATH = (
    _REPO_ROOT / "platform" / "integrations" / "fastapi" / "layerx_fastapi" / "signatures.py"
)


def _signature_verifier_class() -> type:
    spec = importlib.util.spec_from_file_location(
        "layerx_fastapi_program_trust_signatures", _SIGNATURES_PATH
    )
    if spec is None or spec.loader is None:
        raise AssertionError(f"cannot load {_SIGNATURES_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.LayerXSignatureVerifier


LayerXSignatureVerifier = _signature_verifier_class()


PROGRAM_ID = "11" * 32
IDEMPOTENCY_KEY = "33" * 32
SEQUENCER_KEY = bytes([0x44]) * 32


def sized(value: bytes) -> bytes:
    return len(value).to_bytes(4, "big") + value


def canonical_program_call(not_before: int = 10, not_after: int = 20) -> bytes:
    payload = b"".join((
        b"LayerX/programs/call/v1\0",
        bytes.fromhex(PROGRAM_ID),
        (1).to_bytes(8, "big"),
        (0).to_bytes(16, "big"),
        (0).to_bytes(2, "big"),
        sized(b"\xaa"),
    ))
    payload_hash = sha256(b"LXP/v1/payload-hash\0" + payload).digest()
    return b"".join((
        (1).to_bytes(2, "big"),
        (0x1001).to_bytes(2, "big"),
        b"\x0c",
        b"\x01",
        (1).to_bytes(2, "big"),
        b"\x02",
        (1).to_bytes(4, "big"),
        b"\x03",
        (0x0009_0003).to_bytes(4, "big"),
        b"\x04",
        sized(b"did:lxp:test"),
        b"\x05",
        sized(b"\x01"),
        b"\x06",
        (0).to_bytes(8, "big"),
        b"\x07",
        not_before.to_bytes(8, "big"),
        not_after.to_bytes(8, "big"),
        b"\x08",
        sized(bytes.fromhex(IDEMPOTENCY_KEY)),
        b"\x09",
        (0).to_bytes(16, "big"),
        b"\x0a",
        sized(payload_hash),
        b"\x0b",
        sized(payload),
        b"\x0c",
        sized(b"\x02"),
    ))


def call(not_before: int = 10, not_after: int = 20) -> ProgramCall:
    return ProgramCall(PROGRAM_ID, b"\xaa", 1, 0, (), canonical_program_call(not_before, not_after))


def discovery(sequence: int, observed_at: int, valid_through: int, state_root: str) -> dict[str, object]:
    return {
        "program_id": PROGRAM_ID,
        "lifecycle": "active",
        "version": 1,
        "code_hash": "22" * 32,
        "abi_version": 2,
        "receipt_digest": "33" * 32,
        "state_root": state_root,
        "observed_sequence": str(sequence),
        "observed_at": str(observed_at),
        "valid_through": str(valid_through),
        "verification": "registry-receipt-and-current-head-verified",
    }


def envelope(value: object, *, achieved: bool) -> bytes:
    verification: dict[str, object]
    if achieved:
        verification = {"state": "Achieved", "level": "SequencerSigned"}
    else:
        verification = {
            "state": "Unverified",
            "requested": "SequencerSigned",
            "achieved": "Unverified",
            "reason": "server_side_receipt_verification_only",
        }
    return json.dumps({
        "request_id": "1",
        "value": value,
        "verification_status": verification,
    }, separators=(",", ":")).encode()


def reply(handler: BaseHTTPRequestHandler, value: object, *, achieved: bool) -> None:
    encoded = envelope(value, achieved=achieved)
    handler.send_response(200)
    handler.send_header("Content-Type", "application/json")
    handler.send_header("Content-Length", str(len(encoded)))
    handler.end_headers()
    handler.wfile.write(encoded)


class ProgramTrustTests(unittest.TestCase):
    def test_signed_validity_and_maximum_simulation_age_are_enforced(self) -> None:
        binding = decode_signed_program_call(call())
        self.assertEqual((binding.not_before, binding.not_after), (10, 20))
        assert_fresh_simulation_observation(15, binding, 15, 5)
        for observed_at, now, maximum_age in (
            (9, 15, 10),
            (21, 21, 10),
            (15, 14, 10),
            (15, 21, 5),
        ):
            with self.subTest(observed_at=observed_at, now=now, maximum_age=maximum_age):
                with self.assertRaises(ValueError):
                    assert_fresh_simulation_observation(observed_at, binding, now, maximum_age)
        self.assertEqual(
            ProgramTrustContext(SEQUENCER_KEY).maximum_simulation_age_milliseconds,
            300_000,
        )
        for invalid_age in (0, -1, (1 << 64), True):
            with self.subTest(invalid_age=invalid_age), self.assertRaises(ValueError):
                ProgramTrustContext(SEQUENCER_KEY, maximum_simulation_age_milliseconds=invalid_age)
        with self.assertRaises(ValueError):
            ProgramTrustContext(bytes(32))

    def test_discovery_cache_rejects_rollback_and_same_sequence_conflict(self) -> None:
        responses = iter((
            discovery(9, 1000, 2000, "44" * 32),
            discovery(8, 1100, 2100, "44" * 32),
            discovery(9, 1200, 2100, "55" * 32),
            discovery(10, 1300, 2200, "55" * 32),
        ))

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                reply(self, next(responses), achieved=False)

            def log_message(self, _format: str, *args: object) -> None:
                del args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            programs = ProgramOperations(
                ProductionClient(AgentHttpTransport(f"http://127.0.0.1:{server.server_port}")),
                LayerXSignatureVerifier(),
                ProgramTrustContext(SEQUENCER_KEY, lambda: 1500),
            )
            self.assertEqual(programs.discover(PROGRAM_ID).observed_sequence, 9)
            with self.assertRaisesRegex(ValueError, "rollback or conflict"):
                programs.discover(PROGRAM_ID)
            with self.assertRaisesRegex(ValueError, "rollback or conflict"):
                programs.discover(PROGRAM_ID)
            self.assertEqual(programs.discover(PROGRAM_ID).observed_sequence, 10)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_simulation_rechecks_head_freshness_after_http(self) -> None:
        now = [15]

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                reply(self, discovery(1, 10, 20, "44" * 32), achieved=False)

            def do_POST(self) -> None:
                now[0] = 21
                reply(self, {}, achieved=True)

            def log_message(self, _format: str, *args: object) -> None:
                del args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            programs = ProgramOperations(
                ProductionClient(AgentHttpTransport(f"http://127.0.0.1:{server.server_port}")),
                LayerXSignatureVerifier(),
                ProgramTrustContext(SEQUENCER_KEY, lambda: now[0], 5),
            )
            programs.discover(PROGRAM_ID)
            with self.assertRaisesRegex(ValueError, "stale program head"):
                programs.simulate(call(10, 20))
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_concurrent_head_change_invalidates_inflight_simulation(self) -> None:
        simulation_entered = threading.Event()
        release_simulation = threading.Event()
        discovery_count = [0]

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                discovery_count[0] += 1
                sequence = discovery_count[0]
                reply(self, discovery(sequence, 10 + sequence, 30, f"{sequence + 0x43:02x}" * 32), achieved=False)

            def do_POST(self) -> None:
                simulation_entered.set()
                if not release_simulation.wait(5):
                    raise AssertionError("simulation release timed out")
                reply(self, {}, achieved=True)

            def log_message(self, _format: str, *args: object) -> None:
                del args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        server_thread = threading.Thread(target=server.serve_forever, daemon=True)
        server_thread.start()
        try:
            programs = ProgramOperations(
                ProductionClient(AgentHttpTransport(f"http://127.0.0.1:{server.server_port}")),
                LayerXSignatureVerifier(),
                ProgramTrustContext(SEQUENCER_KEY, lambda: 15, 5),
            )
            programs.discover(PROGRAM_ID)
            failure: list[BaseException] = []

            def simulate() -> None:
                try:
                    programs.simulate(call(10, 20))
                except BaseException as error:
                    failure.append(error)

            simulation_thread = threading.Thread(target=simulate)
            simulation_thread.start()
            self.assertTrue(simulation_entered.wait(5), "simulation did not reach the HTTP boundary")
            programs.discover(PROGRAM_ID)
            release_simulation.set()
            simulation_thread.join(5)
            self.assertFalse(simulation_thread.is_alive(), "simulation thread did not finish")
            self.assertEqual(len(failure), 1)
            self.assertRegex(str(failure[0]), "head changed during simulation")
        finally:
            release_simulation.set()
            server.shutdown()
            server.server_close()
            server_thread.join()


if __name__ == "__main__":
    unittest.main()
