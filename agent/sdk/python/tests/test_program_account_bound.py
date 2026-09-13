import importlib.util
import json
import unittest
from copy import deepcopy
from dataclasses import replace
from hashlib import sha256
from pathlib import Path

from layerx_sdk.program_wire import (
    _ACCOUNT_BOUND_SET, _AUTHORITY, _Reader, _verify_authorization_root,
    decode_and_verify_program_terminal,
)
from layerx_sdk.programs import ProgramTrustContext, verify_program_receipt
from layerx_sdk.verifier import AuthorizedReceiptBatch, ReceiptVerificationError, verify_receipt_outcome


class ProgramAccountBoundTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = Path(__file__).resolve().parents[4]
        fixture = root / "tests/fixtures/programs/account-bound-call"
        cls.execution = json.loads((fixture / "execution.json").read_text())
        cls.pin = bytes.fromhex((fixture / "sequencer-key.hex").read_text())
        cls.signed = bytes.fromhex((fixture / "signed-activity.hex").read_text())
        source = cls.execution["authority"]
        cls.authority = AuthorizedReceiptBatch(**{
            field: bytes.fromhex(source[field]) for field in
            ("batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key")
        })
        spec = importlib.util.spec_from_file_location(
            "account_bound_signatures", root / "platform/integrations/fastapi/layerx_fastapi/signatures.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cls.signatures = module.LayerXSignatureVerifier()
        cls.trust = ProgramTrustContext(cls.pin, protocol_version=3)

    def test_real_native_call_verifies_with_principal_account_binding(self):
        self.assertEqual(self.authority.sequencer_public_key, self.pin)
        self.assertEqual(sha256(b"LXP/v1/activity-id\0" + self.signed).hexdigest(), self.execution["activity_id"])
        verified = verify_program_receipt(self.execution, self.authority, self.signatures, self.trust)
        receipt = verified.verification.receipt
        self.assertEqual((receipt.protocol_version, receipt.module_id, receipt.operation, receipt.result_code), (3, 9, 3, 0))
        self.assertEqual(verified.transfer_verification, "reconstructed")
        self.assertEqual(self.execution["outcome"]["code"], 0)

    def test_account_bound_authority_rejects_changed_names_and_shape(self):
        terminal = bytes.fromhex(self.execution["terminal_payload"])
        domain = b"LXP/programs/terminal-applied-legs/v1\0"
        wrapper = _Reader(terminal)
        self.assertEqual(wrapper.fixed(len(domain)), domain)
        detail = wrapper.sized_u32(1_048_576)
        wrapper.sized_u32(256 * 115)
        wrapper.end()
        self.assertTrue(detail.startswith(_AUTHORITY))
        wrapper = _Reader(detail[len(_AUTHORITY):])
        wrapper.sized_u32(1_048_576)
        authorization = wrapper.sized_u32(1_048_576)
        root = wrapper.fixed(32)
        wrapper.end()
        self.assertTrue(authorization.startswith(_ACCOUNT_BOUND_SET))
        wrapped = _Reader(authorization[len(_ACCOUNT_BOUND_SET):])
        original = wrapped.sized_u32(1_048_576)
        name = wrapped.fixed(wrapped.u16())
        wrapped.end()
        self.assertTrue(name.startswith(b"agent:did:layerx:"))
        _verify_authorization_root(authorization, root, require_v2=True)
        prefix = _ACCOUNT_BOUND_SET + len(original).to_bytes(4, "big") + original
        changed_owner = name.replace(b"did:layerx:", b"did:layerz:", 1)
        cases = [
            authorization + b"\0", authorization[:-1], original,
            _ACCOUNT_BOUND_SET + len(authorization).to_bytes(4, "big") + authorization,
            prefix + b"\0\0",
            prefix + len(name).to_bytes(2, "big") + name.upper(),
            prefix + len(changed_owner).to_bytes(2, "big") + changed_owner,
            authorization + len(name).to_bytes(2, "big") + name,
            _ACCOUNT_BOUND_SET + len(original).to_bytes(4, "big") + original.replace(b"transfer-set/v2\0", b"transfer-set/v1\0", 1) + len(name).to_bytes(2, "big") + name,
        ]
        for index, changed in enumerate(cases):
            with self.subTest(index=index), self.assertRaises(ValueError):
                _verify_authorization_root(changed, root, require_v2=True)
        with self.assertRaises(ValueError):
            _verify_authorization_root(authorization, bytes(32), require_v2=True)

    def test_response_result_and_signed_commitments_remain_bound(self):
        receipt = verify_receipt_outcome(bytes.fromhex(self.execution["receipt"]), self.authority,
                                         self.signatures, protocol_version=3).receipt
        with self.assertRaisesRegex(ValueError, "candidate response result code"):
            decode_and_verify_program_terminal(
                bytes.fromhex(self.execution["terminal_payload"]), bytes.fromhex(self.execution["call_graph"]),
                self.execution["program_id"], replace(receipt.program_outcome, result_code=1), 3)
        for field in ("receipt", "terminal_payload", "call_graph"):
            changed = deepcopy(self.execution)
            value = bytes.fromhex(changed[field])
            changed[field] = (value[:-1] + bytes((value[-1] ^ 1,))).hex()
            with self.subTest(field=field), self.assertRaises(ReceiptVerificationError if field == "receipt" else ValueError):
                verify_program_receipt(changed, self.authority, self.signatures, self.trust)
