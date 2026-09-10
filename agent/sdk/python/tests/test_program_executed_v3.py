import importlib.util
import json
import unittest
from pathlib import Path

from layerx_sdk.production import PlatformSdkError
from layerx_sdk.program_wire import decode_and_verify_program_terminal
from layerx_sdk.programs import ProgramTrustContext, _execution, verify_program_receipt
from layerx_sdk.verifier import AuthorizedReceiptBatch, verify_receipt_outcome


class ProgramExecutedV3Test(unittest.TestCase):
    def test_actual_kernel_runtime_receipt_and_module_binding(self):
        root = Path(__file__).resolve().parents[4]
        fixture = json.loads((root / "platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json").read_text())
        spec = importlib.util.spec_from_file_location("executed_v3_signatures", root / "platform/integrations/fastapi/layerx_fastapi/signatures.py")
        self.assertIsNotNone(spec)
        self.assertIsNotNone(spec.loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        signatures = module.LayerXSignatureVerifier()
        source = fixture["authorized_batch"]
        authority = AuthorizedReceiptBatch(**{field: bytes.fromhex(source[field + "_hex"]) for field in ("batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key")})
        canonical = bytes.fromhex(fixture["canonical_receipt_hex"])
        receipt = verify_receipt_outcome(canonical, authority, signatures, protocol_version=3).receipt
        self.assertEqual((receipt.protocol_version, receipt.module_version, receipt.operation), (3, 4, 3))
        self.assertIsNotNone(receipt.program_outcome)
        terminal = decode_and_verify_program_terminal(bytes.fromhex(fixture["terminal_payload_hex"]), bytes.fromhex(fixture["call_graph_hex"]), fixture["program_id_hex"], receipt.program_outcome, 3)
        document = fixture["execution_document"]
        self.assertEqual(document["usage"], terminal.usage)
        self.assertEqual(document["outcome"], terminal.outcome)
        parsed = _execution(document, "executed")
        self.assertEqual(parsed["module_version"], 4)
        trust = ProgramTrustContext(authority.sequencer_public_key, protocol_version=3)
        verified = verify_program_receipt(parsed, authority, signatures, trust)
        self.assertEqual(verified.verification.receipt_digest.hex(), fixture["receipt_digest_hex"])
        with self.assertRaises((ValueError, PlatformSdkError)):
            verify_program_receipt(_execution({**document, "module_version": 3}, "executed"), authority, signatures, trust)
        with self.assertRaises((ValueError, PlatformSdkError)):
            verify_program_receipt(parsed, authority, signatures, ProgramTrustContext(authority.sequencer_public_key, protocol_version=2))
        for field in ("module_version", "guest_abi_version", "result_code"):
            with self.assertRaises((ValueError, PlatformSdkError)):
                verify_program_receipt({**document, field: True}, authority, signatures, trust)
        for value in (0, 5, True):
            with self.assertRaises(ValueError): _execution({**document, "module_version": value}, "executed")
        corrupted = canonical[:-1] + bytes([canonical[-1] ^ 1])
        with self.assertRaises((ValueError, PlatformSdkError)):
            verify_program_receipt(_execution({**document, "receipt": corrupted.hex()}, "executed"), authority, signatures, trust)
