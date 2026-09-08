#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import provision


class RegistrationInputTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.input = self.root / 'human-evidence-input'
        self.input.mkdir(mode=0o700)
        self.path = self.input / 'owner-registration.json'

    def write(self, data):
        self.path.write_text(data)
        self.path.chmod(0o600)

    def refused(self):
        with self.assertRaises(provision.Refused) as caught:
            provision.owner_registration(self.root)
        self.assertIn(str(self.path), str(caught.exception))
        self.assertFalse((self.root / 'human-evidence').exists())

    def test_missing_wrong_mode_symlink_hardlink_and_directory_refuse(self):
        self.refused()
        self.write('{}')
        self.path.chmod(0o644)
        self.refused()
        self.path.chmod(0o600)
        linked = self.input / 'linked.json'
        os.link(self.path, linked)
        self.refused()
        self.path.unlink()
        self.path.symlink_to(linked)
        self.refused()
        self.path.unlink()
        self.path.mkdir(mode=0o600)
        self.refused()

    def test_duplicate_nonfinite_unknown_and_missing_fields_refuse(self):
        for data in ('{}', '[]', '{"owner_account":1,"owner_account":2}',
                     '{"owner_account":NaN}', '{"unexpected":true}'):
            self.write(data)
            self.refused()

    def test_real_owner_output_is_not_a_protocol_registration(self):
        repo = Path(__file__).resolve().parents[3]
        binary = repo / 'human/target/debug/layerx-human-identity-provider'
        self.assertTrue(binary.is_file(), f'required real provider binary: {binary}')
        policy = self.root / 'recovery-policy.json'
        policy.write_text(json.dumps({'root': list(os.urandom(32)), 'threshold': 1,
                                      'delay_seconds': 86400}))
        policy.chmod(0o600)
        env = dict(os.environ,
                   LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT=str(self.root / 'state'),
                   LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE=str(policy))
        request = json.dumps({'email': 'owner@example.com', 'display_name': 'Owner',
                              'idempotency_key': 'registration-contract', 'now': 1})
        result = subprocess.run([str(binary), 'provision-owner'], input=request,
                                text=True, capture_output=True, env=env, check=True)
        owner = json.loads(result.stdout)
        self.assertEqual(owner['recovery_root'], json.loads(policy.read_text())['root'])
        self.assertEqual(owner['recovery_threshold'], 1)
        self.assertEqual(owner['recovery_delay_seconds'], 86400)
        replay = subprocess.run([str(binary), 'provision-owner'], input=request,
                                text=True, capture_output=True, env=env, check=True)
        self.assertEqual(json.loads(replay.stdout), owner)
        self.write(result.stdout)
        self.refused()
        self.assertNotIn(owner['did'], self.run_cli().stderr)

    def run_cli(self):
        return subprocess.run(['python3', str(Path(provision.__file__).resolve()),
                               '--validate-owner-registration', '--work-dir', str(self.root)],
                              text=True, capture_output=True, check=False)

    def test_cli_refuses_with_exact_path_without_values(self):
        result = self.run_cli()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(str(self.path), result.stderr)
        self.assertEqual(result.stdout, '')


if __name__ == '__main__':
    unittest.main()
