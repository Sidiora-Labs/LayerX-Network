import importlib.util
import json
from pathlib import Path
import unittest
from hashlib import sha256

from layerx_sdk.program_wire import decode_and_verify_program_terminal, _verify_applied_legs
from layerx_sdk.verifier import AuthorizedReceiptBatch, verify_receipt_outcome

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('signatures', ROOT / 'platform/integrations/fastapi/layerx_fastapi/signatures.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class TerminalV4(unittest.TestCase):
    def test_signed_shared_vectors(self):
        for name, expected in [('executed-v4', 'reconstructed'), ('principal-v4', 'reconstructed'),
                               ('mutated-leg-v4', None), ('executed-v3', 'recorded_terminal_root_not_locally_reconstructable')]:
            with self.subTest(name=name):
                fixture = json.loads((ROOT / f'platform/sdk/conformance/fixtures/receipt-programs-{name}.json').read_text())
                source = fixture['authorized_batch']
                authority = AuthorizedReceiptBatch(**{field: bytes.fromhex(source[field + '_hex']) for field in
                    ('batch_id', 'asset', 'previous_state_root', 'resulting_state_root', 'sequencer_public_key')})
                verified = verify_receipt_outcome(bytes.fromhex(fixture['canonical_receipt_hex']), authority, module.LayerXSignatureVerifier(), protocol_version=3)
                self.assertEqual(verified.receipt_digest.hex(), fixture['receipt_digest_hex'])
                self.assertEqual(sha256(b'LXP/v1/activity-id\0' + bytes.fromhex(fixture['signed_activity_hex'])).digest(), verified.receipt.activity_id)
                args = (bytes.fromhex(fixture['terminal_payload_hex']), bytes.fromhex(fixture['call_graph_hex']), fixture['program_id_hex'], verified.receipt.program_outcome, 3)
                if expected is None:
                    with self.assertRaisesRegex(ValueError, 'applied transfer root'):
                        decode_and_verify_program_terminal(*args)
                else:
                    self.assertEqual(decode_and_verify_program_terminal(*args).transfer_verification, expected)
                if name == 'executed-v4':
                    for length in range(len(args[0])):
                        with self.assertRaises(ValueError):
                            decode_and_verify_program_terminal(args[0][:length], *args[1:])
                    with self.assertRaises(ValueError):
                        decode_and_verify_program_terminal(args[0] + b'\0', *args[1:])

    def test_empty_legs_require_zero_root(self):
        _verify_applied_legs(b'', bytes(32))
        with self.assertRaises(ValueError):
            _verify_applied_legs(b'', b'\1' * 32)


if __name__ == '__main__':
    unittest.main()
