from __future__ import annotations

import ast
from hashlib import sha256
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
from types import MappingProxyType
from typing import cast, get_args
import unittest
import zipfile

from layerx_sdk import (
    APPROVAL_CONTRACT_INTRODUCED,
    APPROVAL_DECISION_OUTCOMES,
    APPROVAL_ENFORCEMENT_NOTICE,
    APPROVAL_EVENT_KINDS,
    APPROVAL_STATES,
    AgentHttpTransport,
    ApiError,
    ApprovalApproveRequest,
    ApprovalGetRequest,
    ApprovalListRequest,
    ApprovalRejectRequest,
    Client,
    CheckpointAttestation,
    ErrorClass,
    IdempotentMutation,
    IdempotencyKey,
    LayerXKeyCredential,
    PlatformSdkError,
    ProgramCall,
    ProgramOperations,
    ProgramTrustContext,
    SubmissionFailed,
    SubmissionUnknown,
    ProductionClient,
    SecretBytes,
    SdkErrorCode,
    VerificationLevel,
    VerifiedRead,
    layerx_sdk_py_package,
    parse_amount,
    parse_sequence,
    require_verified,
)
from layerx_sdk.program_wire import decode_and_verify_program_terminal
from layerx_sdk.verifier import ProgramReceiptOutcome


ROOT = Path(__file__).resolve().parents[1]


def canonical_program_call(program_id: str, idempotency: str) -> bytes:
    payload = b"".join((
        b"LayerX/programs/call/v1\0",
        bytes.fromhex(program_id),
        (1).to_bytes(8, "big"),
        (0).to_bytes(16, "big"),
        (0).to_bytes(2, "big"),
        sized(b"\xaa"),
    ))
    payload_hash = sha256(b"LXP/v1/payload-hash\0" + payload).digest()
    return b"".join((
        (1).to_bytes(2, "big"), (0x1001).to_bytes(2, "big"), b"\x0c",
        b"\x01", (1).to_bytes(2, "big"),
        b"\x02", (1).to_bytes(4, "big"),
        b"\x03", (0x0009_0003).to_bytes(4, "big"),
        b"\x04", sized(b"did:lxp:test"),
        b"\x05", sized(b"\x01"),
        b"\x06", (0).to_bytes(8, "big"),
        b"\x07", (0).to_bytes(8, "big"), (1).to_bytes(8, "big"),
        b"\x08", sized(bytes.fromhex(idempotency)),
        b"\x09", (0).to_bytes(16, "big"),
        b"\x0a", sized(payload_hash),
        b"\x0b", sized(payload),
        b"\x0c", sized(b"\x02"),
    ))


def sized(value: bytes) -> bytes:
    return len(value).to_bytes(4, "big") + value


def sized64(value: bytes) -> bytes:
    return len(value).to_bytes(8, "big") + value


def load_build_backend():
    spec = importlib.util.spec_from_file_location("layerx_sdk_build_backend", ROOT / "build_backend.py")
    if spec is None or spec.loader is None:
        raise RuntimeError("build backend unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PythonSdkIntegration(unittest.TestCase):
    def test_agent_http_transport_preserves_program_contract_and_authentication(self) -> None:
        program_id = "11" * 32
        observed: dict[str, object] = {}

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                length = int(self.headers["Content-Length"])
                observed["path"] = self.path
                observed["authorization"] = self.headers["Authorization"]
                observed["body"] = self.rfile.read(length)
                encoded = json.dumps({
                    "request_id": "request-1",
                    "value": {"program_id": program_id},
                    "verification_status": {"state": "Unverified", "requested": "SequencerSigned", "achieved": "Unverified", "reason": "server_side_receipt_verification_only"},
                }, separators=(",", ":")).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            def log_message(self, _format: str, *args: object) -> None:
                del args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            credential = LayerXKeyCredential("key_1", SecretBytes(("lxp_live_" + "22" * 32).encode()))
            client = ProductionClient(AgentHttpTransport(
                f"http://127.0.0.1:{server.server_port}", credential=credential
            ))
            result = client.agent(
                "program.discover",
                {"program_id": program_id, "requested_verification_level": "sequencer-signed"},
            )
            self.assertEqual(result, {"program_id": program_id})
            self.assertEqual(observed["path"], f"/v1/programs/registry/{program_id}")
            self.assertEqual(observed["authorization"], "LayerX-Key key_1:lxp_live_" + "22" * 32)
            self.assertEqual(json.loads(cast(bytes, observed["body"])), {
                "program_id": program_id,
                "requested_verification_level": "sequencer-signed",
            })
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_program_call_ambiguity_retains_canonical_signed_evidence(self) -> None:
        program_id = "11" * 32
        idempotency = "33" * 32
        signed = canonical_program_call(program_id, idempotency)

        class AmbiguousTransport:
            def call(self, plane: object, operation: object, request: object, idempotency_key: object) -> object:
                del plane, operation, request, idempotency_key
                raise PlatformSdkError(SdkErrorCode.DECODE_FAILURE, "never")

        class Signatures:
            def verify_ed25519(self, public_key: bytes, signature: bytes, digest: bytes) -> bool:
                del public_key, signature, digest
                return False

            def verify_recoverable_secp256k1(self, public_key: bytes, signature: bytes, signature_v: int, signer: bytes, digest: bytes) -> bool:
                del public_key, signature, signature_v, signer, digest
                return False

        programs = ProgramOperations(
            ProductionClient(AmbiguousTransport()),  # type: ignore[arg-type]
            Signatures(),  # type: ignore[arg-type]
            ProgramTrustContext(bytes([0x44]) * 32, lambda: 1),
        )
        unknown = programs.submit(ProgramCall(program_id, b"\xaa", 1, 0, (), signed), IdempotencyKey(idempotency))
        self.assertEqual(unknown["state"], "unknown")
        self.assertEqual(unknown["activity_id"], sha256(b"LXP/v1/activity-id\0" + signed).hexdigest())
        self.assertEqual(unknown["retained_signed_activity"], signed.hex())

    def test_agent_http_transport_never_follows_redirects(self) -> None:
        followed = False

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                nonlocal followed
                if self.path == "/target":
                    followed = True
                    self.send_response(500)
                    self.end_headers()
                    return
                encoded = b"{}"
                self.send_response(307)
                self.send_header("Location", "/target")
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            def log_message(self, _format: str, *args: object) -> None:
                del args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            client = ProductionClient(AgentHttpTransport(f"http://127.0.0.1:{server.server_port}"))
            with self.assertRaises(PlatformSdkError):
                client.agent("program.discover", {"program_id": "11" * 32, "requested_verification_level": "sequencer-signed"})
            self.assertFalse(followed)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_candidate_terminal_response_and_usage_are_receipt_bound(self) -> None:
        program_id = "11" * 32
        graph = b"LayerX/programs/call-graph/v1\0"
        terminal = b"".join((
            b"LXP/program-execution/v4\0",
            (1).to_bytes(2, "big"), (1).to_bytes(4, "big"), (1).to_bytes(4, "big"), (0).to_bytes(8, "big"),
            (1).to_bytes(8, "big"), (2).to_bytes(8, "big"), (3).to_bytes(8, "big"), (4).to_bytes(8, "big"),
            (0).to_bytes(4, "big"), (0).to_bytes(8, "big"), (10).to_bytes(16, "big"), b"\0",
            bytes.fromhex(program_id), (2).to_bytes(2, "big"), b"\0", (0).to_bytes(4, "big"),
            sized64(b"\xaa\xbb"), sized64(graph),
        ))
        receipt = ProgramReceiptOutcome(
            3, 1, 0, 1, 2, 1, 1, 1, 2, 3, 4, 0, 0, 0, 0,
            (0, 0, 0, 0, 0, 0, 0), bytes(32), bytes(32), bytes(32), 10,
            sha256(graph).digest(), sha256(terminal).digest(), bytes(32),
        )
        decoded = decode_and_verify_program_terminal(terminal, graph, program_id, receipt, 1)
        self.assertEqual(decoded.outcome, {"kind": "completed", "code": 0, "response": "aabb"})
        self.assertEqual(decoded.usage["fee_units"], "10")

    def test_approval_contract_operations_events_and_outcomes_are_exact(self) -> None:
        self.assertEqual(APPROVAL_CONTRACT_INTRODUCED, "1.1")
        self.assertIn("confers no protocol authority", APPROVAL_ENFORCEMENT_NOTICE)
        self.assertEqual(APPROVAL_STATES, ("Held", "Granted", "Rejected", "Expired", "Defective"))
        self.assertEqual(
            APPROVAL_DECISION_OUTCOMES,
            ("Granted", "Rejected", "Expired", "Defective", "AlreadyDecided", "Conflict"),
        )
        self.assertEqual(
            APPROVAL_EVENT_KINDS,
            ("Created", "Granted", "Rejected", "Expired", "Defective"),
        )

        class RecordingTransport:
            def __init__(self) -> None:
                self.operations: list[str] = []

            def call(self, operation: str, request: object) -> object:
                self.operations.append(operation)
                return request

        transport = RecordingTransport()
        client = Client(transport)  # type: ignore[arg-type]
        client.approval_list(ApprovalListRequest("tenant-a", None, 50))
        client.approval_get(ApprovalGetRequest("tenant-a", "approval-7"))
        client.approval_approve(ApprovalApproveRequest("tenant-a", "approval-7", "approve-7"))
        client.approval_reject(
            ApprovalRejectRequest("tenant-a", "approval-7", "reject-7", "not expected")
        )
        self.assertEqual(
            transport.operations,
            ["approval.list", "approval.get", "approval.approve", "approval.reject"],
        )

    def test_package_metadata_is_immutable_and_contract_bound(self) -> None:
        metadata = layerx_sdk_py_package()
        self.assertIsInstance(metadata, MappingProxyType)
        self.assertEqual(metadata["name"], "layerx-sdk")
        self.assertEqual(metadata["version"], "0.1.0")
        self.assertEqual(metadata["contract_major"], 1)
        with self.assertRaises(TypeError):
            metadata["contract_major"] = 2  # type: ignore[index]

    def test_consensus_integers_are_exact_and_bounded(self) -> None:
        self.assertEqual(parse_amount("9007199254740993"), 9_007_199_254_740_993)
        self.assertEqual(parse_sequence("18446744073709551615"), 18_446_744_073_709_551_615)
        for invalid in ("", "01", "-1", "1.5", "１"):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                parse_amount(invalid)
        with self.assertRaises(OverflowError):
            parse_sequence("18446744073709551616")

    def test_checkpoint_attestation_exposes_canonical_paxeer_binding(self) -> None:
        attestation = CheckpointAttestation(
            protocol_version=1,
            network_id=17,
            paxeer_chain_id=777,
            settlement_contract=bytes([1]) * 20,
            epoch=9,
            checkpoint_id=bytes(32),
            checkpoint_hash=bytes(32),
            guarantor_id=bytes(32),
            batch_number=12,
            data_availability_root=bytes(32),
            replayed=True,
            data_possessed=True,
            availability_class_mask=0x1F,
            attested_at_ms=1,
            signer=bytes([2]) * 20,
            signature=bytes(64),
            signature_v=27,
        )
        self.assertEqual(attestation.paxeer_chain_id, 777)
        self.assertEqual(attestation.settlement_contract, bytes([1]) * 20)

    def test_reads_require_the_declared_verification_level(self) -> None:
        read = VerifiedRead(
            value=10,
            achieved_verification_level=VerificationLevel.STATE_PROVEN,
            chain_head=20,
            latest_batch="batch-19",
            latest_checkpoint="checkpoint-18",
            value_sequence=17,
        )
        self.assertIs(require_verified(VerificationLevel.STATE_PROVEN, read), read)
        with self.assertRaisesRegex(ValueError, "verification_below_requested"):
            require_verified(VerificationLevel.CHECKPOINT_FINALISED, read)
        unverified = VerifiedRead(
            value=10,
            achieved_verification_level=VerificationLevel.UNVERIFIED,
            chain_head=20,
            latest_batch="batch-19",
            latest_checkpoint="checkpoint-18",
            value_sequence=17,
        )
        with self.assertRaisesRegex(ValueError, "unverified_read"):
            require_verified(VerificationLevel.UNVERIFIED, unverified)

    def test_unknown_idempotency_and_future_result_codes_remain_lossless(self) -> None:
        unknown = SubmissionUnknown()
        future_failure = SubmissionFailed(protocol_result_code=-77_777)
        mutation = IdempotentMutation(
            request_id=18_446_744_073_709_551_615,
            key=bytes(range(32)),
            body_digest=bytes(reversed(range(32))),
            operation={"amount": 9_007_199_254_740_993},
        )
        self.assertEqual(unknown.kind, "Unknown")
        self.assertEqual(future_failure.protocol_result_code, -77_777)
        self.assertEqual(mutation.operation["amount"], 9_007_199_254_740_993)
        self.assertEqual(len(mutation.key), 32)

    def test_typed_error_taxonomy_is_complete_and_disjoint(self) -> None:
        classes = set(get_args(ErrorClass))
        self.assertEqual(len(classes), 12)
        self.assertIn("TransportFailure", classes)
        self.assertIn("VerificationFailure", classes)
        self.assertNotEqual("TransportFailure", "VerificationFailure")
        error = ApiError("CoreRejection", -77_777, False, 7, "future_code")
        self.assertEqual(error.protocol_result_code, -77_777)
        self.assertEqual(str(error), "CoreRejection:future_code")

    def test_type_stubs_and_examples_are_parseable(self) -> None:
        for path in sorted((ROOT / "layerx_sdk").rglob("*.pyi")):
            ast.parse(path.read_text(), filename=str(path))
        examples = sorted((ROOT / "examples").glob("*.py"))
        self.assertEqual(len(examples), 5)
        for path in examples:
            ast.parse(path.read_text(), filename=str(path))

    def test_wheel_build_is_byte_reproducible_and_complete(self) -> None:
        backend = load_build_backend()
        with tempfile.TemporaryDirectory() as first, tempfile.TemporaryDirectory() as second:
            first_name = backend.build_wheel(first)
            second_name = backend.build_wheel(second)
            first_bytes = (Path(first) / first_name).read_bytes()
            second_bytes = (Path(second) / second_name).read_bytes()
            self.assertEqual(first_bytes, second_bytes)
            with zipfile.ZipFile(Path(first) / first_name) as archive:
                names = set(archive.namelist())
                self.assertIn("layerx_sdk/generated/client.py", names)
                self.assertIn("layerx_sdk/generated/client.pyi", names)
                self.assertIn("layerx_sdk/py.typed", names)
                self.assertEqual(
                    len([name for name in names if "/share/layerx-sdk/examples/" in name]),
                    5,
                )
                metadata = archive.read("layerx_sdk-0.1.0.dist-info/METADATA").decode()
                self.assertIn("Requires-Python: >=3.11", metadata)


if __name__ == "__main__":
    unittest.main()
