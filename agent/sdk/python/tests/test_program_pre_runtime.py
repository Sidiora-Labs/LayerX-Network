import importlib.util
import json
import subprocess
import tempfile
import unittest
from copy import deepcopy
from dataclasses import replace
from hashlib import sha256
from pathlib import Path

from layerx_sdk.program_wire import (
    _PRE_RUNTIME, _receipt_usage, bind_retained_program_call,
    decode_and_verify_program_terminal,
)
from layerx_sdk.programs import ProgramTrustContext, _submission, verify_program_receipt
from layerx_sdk.verifier import (
    AuthorizedReceiptBatch, ReceiptVerificationError, _RECEIPT_DOMAIN,
    _decode_protocol_receipt, verify_receipt_outcome,
)


class ProgramPreRuntimeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(__file__).resolve().parents[4]
        fixture = root / "tests/fixtures/programs/pre-runtime-refusal"
        cls.execution = json.loads((fixture / "execution.json").read_text())
        cls.pin = bytes.fromhex((fixture / "sequencer-public.hex").read_text())
        cls.signed = bytes.fromhex((fixture / "signed-activity.hex").read_text())
        cls.terminal = bytes.fromhex((fixture / "terminal.hex").read_text())
        cls.graph = bytes.fromhex((fixture / "call-graph.hex").read_text())
        cls.authority = AuthorizedReceiptBatch(**{
            field: bytes.fromhex(cls.execution["authority"][field]) for field in
            ("batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key")
        })
        spec = importlib.util.spec_from_file_location(
            "pre_runtime_signatures", root / "platform/integrations/fastapi/layerx_fastapi/signatures.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cls.signatures = module.LayerXSignatureVerifier()
        cls.trust = ProgramTrustContext(cls.pin, protocol_version=3)
        cls.protocol = verify_receipt_outcome(bytes.fromhex(cls.execution["receipt"]), cls.authority,
                                               cls.signatures, protocol_version=3).receipt
        cls.payload_hash, cls.abi, cls.key = bind_retained_program_call(
            cls.signed, cls.execution["activity_id"], cls.execution["program_id"], 3)

    def test_original_native_refusal_and_submission_recovery(self):
        verified = verify_program_receipt(self.execution, self.authority, self.signatures, self.trust)
        self.assertEqual(verified.verification.receipt.result_code, -3)
        self.assertEqual(verified.transfer_verification, "reconstructed")
        self.assertEqual(self.abi, 2)
        self.assertEqual(self.execution["outcome"], {"kind": "refused", "failure": {"kind": "guest_refused", "code": -3}})
        recovered = _submission(self.execution, self.signatures, self.trust,
                                activity_id=self.execution["activity_id"], idempotency_key=self.key)
        self.assertEqual(recovered["state"], "refused")
        self.assertEqual(recovered["retained_signed_activity"], self.signed.hex())
        explicit = deepcopy(self.execution)
        del explicit["retained_signed_activity"]
        verify_program_receipt(explicit, self.authority, self.signatures, self.trust,
                               expected_signed_activity=self.signed)
        with self.assertRaisesRegex(ValueError, "original call binding"):
            verify_program_receipt(explicit, self.authority, self.signatures, self.trust)
        unknown = {"state": "unknown", "activity_id": self.execution["activity_id"], "idempotency_key": self.key}
        self.assertEqual(_submission(unknown, self.signatures, self.trust)["state"], "unknown")

    def test_call_identity_abi_idempotency_and_signature_bindings(self):
        cases = []
        for field in ("activity_id", "program_id", "idempotency_key"):
            changed = deepcopy(self.execution)
            changed[field] = "ab" * 32
            cases.append(changed)
        changed = deepcopy(self.execution)
        changed["guest_abi_version"] = 1
        cases.append(changed)
        changed = deepcopy(self.execution)
        changed["retained_signed_activity"] = self.signed[:-1].hex()
        cases.append(changed)
        changed = deepcopy(self.execution)
        changed["outcome"] = {"kind": "completed", "code": 0, "response": ""}
        cases.append(changed)
        for index, changed in enumerate(cases):
            with self.subTest(index=index), self.assertRaises(ValueError):
                verify_program_receipt(changed, self.authority, self.signatures, self.trust)
        with self.assertRaisesRegex(ValueError, "retained call mismatch"):
            verify_program_receipt(self.execution, self.authority, self.signatures, self.trust,
                                   expected_signed_activity=self.signed + b"\0")
        changed = deepcopy(self.execution)
        signed = bytes.fromhex(changed["receipt"])
        changed["receipt"] = (signed[:-1] + bytes([signed[-1] ^ 1])).hex()
        with self.assertRaises(ReceiptVerificationError):
            verify_program_receipt(changed, self.authority, self.signatures, self.trust)

    def check_terminal(self, terminal=None, graph=None, protocol=None, payload_hash=None):
        protocol = self.protocol if protocol is None else protocol
        terminal = self.terminal if terminal is None else terminal
        graph = self.graph if graph is None else graph
        outcome = replace(protocol.program_outcome,
                          terminal_payload_root=sha256(terminal).digest(), call_graph_root=sha256(graph).digest())
        protocol = replace(protocol, program_outcome=outcome)
        return decode_and_verify_program_terminal(
            terminal, graph, self.execution["program_id"], outcome, protocol.protocol_version,
            protocol=protocol, expected_payload_hash=self.payload_hash if payload_hash is None else payload_hash)

    def test_every_pre_runtime_receipt_invariant_is_enforced(self):
        self.check_terminal()
        for field, value in (("protocol_version", 2), ("activity_id", bytes(32)), ("result_code", -4),
                             ("module_id", 1), ("operation", 1), ("module_version", 3), ("parameter_version", 2)):
            with self.subTest(protocol=field), self.assertRaises(ValueError):
                self.check_terminal(protocol=replace(self.protocol, **{field: value}))
        for field, value in (("encoding_version", 3), ("terminal_kind", 1), ("runtime_version", 2),
                             ("result_code", -4), ("memory_bytes", 1), ("storage_read_bytes", 1),
                             ("output_values", 1), ("output_bytes", 1), ("applied_legs_digest", bytes(32)),
                             ("transfer_root", b"a" * 32), ("occupancy_asset_id", b"a" * 32),
                             ("occupancy_evidence_digest", b"a" * 32), ("occupancy_transfer_root", b"a" * 32),
                             ("occupancy_byte_batches", 1), ("occupancy_fee_units", 1)):
            with self.subTest(outcome=field), self.assertRaises(ValueError):
                self.check_terminal(protocol=replace(self.protocol,
                    program_outcome=replace(self.protocol.program_outcome, **{field: value})))
        with self.assertRaises(ValueError):
            self.check_terminal(payload_hash=bytes(32))
        with self.assertRaises(ValueError):
            self.check_terminal(graph=self.graph + b"\0")
        metered = replace(self.protocol.program_outcome, cpu_fuel=1, storage_write_bytes=2, fee_units=3)
        self.assertEqual(self.check_terminal(protocol=replace(self.protocol, program_outcome=metered)).usage,
                         _receipt_usage(metered))

    def test_original_layout_bounds_and_wrapper_refusal(self):
        for length in range(len(self.terminal)):
            with self.subTest(length=length), self.assertRaises(ValueError):
                self.check_terminal(terminal=self.terminal[:length])
        with self.assertRaises(ValueError):
            self.check_terminal(terminal=self.terminal + b"\0")
        for domain in (b"LXP/programs/terminal-applied-legs/v1\0", b"LXP/program-execution-with-occupancy/v1\0"):
            wrapped = domain + len(self.terminal).to_bytes(4, "big") + self.terminal + bytes(4)
            with self.subTest(wrapper=domain), self.assertRaises(ValueError):
                self.check_terminal(terminal=wrapped)

    def test_native_protocol_two_codec_preserves_historical_failure_layout(self):
        canonical = bytearray(self.signed)
        canonical[:2] = (2).to_bytes(2, "big")
        canonical[6:8] = (2).to_bytes(2, "big")
        activity = sha256(b"LXP/v1/activity-id\0" + canonical).digest()
        payload_hash, abi, key = bind_retained_program_call(
            bytes(canonical), activity.hex(), self.execution["program_id"], 2)
        self.assertEqual((payload_hash, abi, key), (self.payload_hash, 2, self.key))
        terminal = bytearray(self.terminal[:-33])
        terminal[len(_PRE_RUNTIME):len(_PRE_RUNTIME) + 32] = activity
        terminal[len(_PRE_RUNTIME) + 68:len(_PRE_RUNTIME) + 72] = (3).to_bytes(4, "big")
        outcome = replace(self.protocol.program_outcome, encoding_version=3, applied_legs_digest=bytes(32))
        protocol = replace(self.protocol, protocol_version=2, activity_id=activity,
                           module_version=3, program_outcome=outcome)
        decoded = self.check_terminal(terminal=bytes(terminal), protocol=protocol)
        self.assertEqual(decoded.outcome, self.execution["outcome"])
        with self.assertRaises(ValueError):
            bind_retained_program_call(bytes(canonical), activity.hex(), self.execution["program_id"], 3)

    def test_resigned_terminal_fields_cannot_rebind_refusal(self):
        base = len(_PRE_RUNTIME)
        with tempfile.TemporaryDirectory(prefix="layerx-program-refusal-signature-") as directory:
            root = Path(directory)
            key = root / "key.der"
            key.write_bytes(bytes.fromhex("302e020100300506032b657004220420") + bytes([3]) * 32)
            pin = subprocess.run(["openssl", "pkey", "-inform", "DER", "-in", str(key),
                                  "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout[-32:]
            for offset in (None, 0, base, base + 32, base + 67, base + 71, base + 75, base + 76, base + 77):
                terminal = bytearray(self.terminal)
                if offset is not None:
                    terminal[offset] ^= 1
                old, new = sha256(self.terminal).digest(), sha256(terminal).digest()
                encoded = bytes.fromhex(self.execution["receipt"])
                self.assertEqual(encoded.count(old), 1)
                encoded = encoded.replace(old, new)
                _, unsigned = _decode_protocol_receipt(encoded)
                digest = sha256(_RECEIPT_DOMAIN + unsigned).digest()
                (root / "digest").write_bytes(digest)
                subprocess.run(["openssl", "pkeyutl", "-sign", "-rawin", "-inkey", str(key),
                                "-keyform", "DER", "-in", str(root / "digest"), "-out", str(root / "signature")],
                               check=True, capture_output=True)
                signature = (root / "signature").read_bytes()
                self.assertEqual(len(signature), 64)
                encoded = encoded[:-64] + signature
                authority = replace(self.authority, sequencer_public_key=pin)
                verify_receipt_outcome(encoded, authority, self.signatures, protocol_version=3)
                execution = deepcopy(self.execution)
                execution.update(receipt=encoded.hex(), receipt_digest=digest.hex(), terminal_payload=terminal.hex())
                execution["authority"]["sequencer_public_key"] = pin.hex()
                with self.subTest(offset=offset):
                    if offset is None:
                        verify_program_receipt(execution, authority, self.signatures, ProgramTrustContext(pin, protocol_version=3))
                    else:
                        with self.assertRaises(ValueError):
                            verify_program_receipt(execution, authority, self.signatures, ProgramTrustContext(pin, protocol_version=3))
