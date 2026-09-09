import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import owner_native
import provision


class NativeProducerRefusalTests(unittest.TestCase):
    def test_missing_native_inputs_name_the_file_and_publish_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(root)
            self.assertIn('owner-native.json', str(refused.exception))
            self.assertFalse((root / 'human-evidence-input/owner-registration.json').exists())

    def test_cli_missing_inputs_fail_without_traceback(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run([sys.executable, str(Path(provision.__file__)),
                                     '--produce-owner-registration', '--work-dir', directory],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('owner-native.json', result.stderr)
            self.assertNotIn('Traceback', result.stderr)

    def test_native_config_rejects_duplicate_and_unknown_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            inputs = root / 'human-evidence-input'
            inputs.mkdir(mode=0o700)
            path = inputs / 'owner-native.json'
            for value in ('{"node_socket":"a","node_socket":"b"}', json.dumps({'unknown': True})):
                path.write_text(value)
                path.chmod(0o600)
                with self.assertRaises(provision.Refused):
                    owner_native.produce(root)
                self.assertFalse((inputs / 'owner-native-run').exists())
                self.assertFalse((inputs / 'owner-registration.json').exists())

    def test_reader_refuses_truncation_bounds_and_trailing_bytes(self):
        for data, maximum in ((b'\0\0\0\2a', 2), (b'\0\0\0\2ab', 1)):
            with self.assertRaises(provision.Refused):
                owner_native.Reader(data, 'native.receipt').span(maximum)
        with self.assertRaises(provision.Refused):
            owner_native.Reader(b'x', 'native.receipt').finish()

    def test_real_native_receipt_signature_and_tamper_refusals(self):
        path = Path(__file__).resolve().parents[3] / 'platform/hosted/authority/tests/fixtures/real-program-deploy-receipt.json'
        value = json.loads(path.read_text())
        raw = bytes.fromhex(value['receipt_hex'])
        public = bytes.fromhex(value['sequencer_public_key_hex'])
        result = owner_native.receipt_fields(raw, public, path)
        self.assertEqual(result['module'], 9)
        self.assertEqual(result['version'], 4)
        self.assertEqual(result['sequence'], 2)
        for changed in (raw[:-1], raw + b'\0', raw[:-1] + bytes([raw[-1] ^ 1])):
            with self.assertRaises(provision.Refused):
                owner_native.receipt_fields(changed, public, path)
