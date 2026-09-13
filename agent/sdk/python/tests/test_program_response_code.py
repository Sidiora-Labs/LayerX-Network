import importlib.util
import json
import unittest
from copy import deepcopy
from dataclasses import replace
from pathlib import Path

from layerx_sdk.program_wire import decode_and_verify_program_terminal
from layerx_sdk.programs import ProgramTrustContext, verify_program_receipt
from layerx_sdk.verifier import (
    AuthorizedReceiptBatch, _decode_protocol_receipt, verify_receipt_outcome,
)


class ProgramResponseCodeTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(__file__).resolve().parents[4]
        fixture = root / "tests/fixtures/programs/emulator-response-code"
        read = lambda name: bytes.fromhex((fixture / name).read_text())
        cls.expected = json.loads((fixture / "expected.json").read_text())
        cls.encoded = read("receipt.hex")
        cls.terminal = read("terminal.hex")
        cls.graph = read("call-graph.hex")
        cls.signed = read("signed-activity.hex")
        cls.pin = read("sequencer-public.hex")
        receipt, _ = _decode_protocol_receipt(cls.encoded)
        cls.authority = AuthorizedReceiptBatch(receipt.batch_id, receipt.asset,
            receipt.previous_state_root, receipt.resulting_state_root, cls.pin)
        spec = importlib.util.spec_from_file_location(
            "response_code_signatures", root / "platform/integrations/fastapi/layerx_fastapi/signatures.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cls.signatures = module.LayerXSignatureVerifier()
        cls.trust = ProgramTrustContext(cls.pin, protocol_version=receipt.protocol_version)
        cls.verified = verify_receipt_outcome(cls.encoded, cls.authority, cls.signatures,
                                             protocol_version=receipt.protocol_version)
        cls.protocol = cls.verified.receipt

    def execution(self):
        fixture = Path(__file__).resolve().parents[4] / "tests/fixtures/programs/emulator-response-code"
        return json.loads((fixture / "execution.json").read_text())

    def test_original_executed_guest_response_seven_has_protocol_success_zero(self):
        execution = self.execution()
        verified = verify_program_receipt(execution, self.authority, self.signatures, self.trust,
                                          expected_signed_activity=self.signed)
        self.assertEqual(verified.verification.receipt.result_code, 0)
        self.assertEqual(verified.verification.receipt.program_outcome.result_code, 0)
        self.assertEqual(execution["outcome"]["kind"], "completed")
        self.assertEqual(execution["outcome"]["code"], 7)
        self.assertEqual(self.protocol.previous_state_root.hex(), self.expected["previous_state_root"])

    def test_guest_response_and_protocol_result_remain_independently_authenticated(self):
        execution = self.execution()
        changed = deepcopy(execution)
        changed["outcome"]["code"] = 8
        with self.assertRaisesRegex(ValueError, "terminal document binding"):
            verify_program_receipt(changed, self.authority, self.signatures, self.trust)
        changed = deepcopy(execution)
        changed["terminal_payload"] = (self.terminal[:-1] + bytes([self.terminal[-1] ^ 1])).hex()
        with self.assertRaises(ValueError):
            verify_program_receipt(changed, self.authority, self.signatures, self.trust)
        with self.assertRaisesRegex(ValueError, "candidate response result code"):
            decode_and_verify_program_terminal(self.terminal, self.graph, self.expected["program_id"],
                replace(self.protocol.program_outcome, result_code=1), self.protocol.protocol_version)
