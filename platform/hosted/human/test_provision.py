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
        provision.write_json(self.input / 'recovery-policy.json', json.loads(policy.read_text()))
        env = dict(os.environ,
                   LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT=str(self.root / 'state'),
                   LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE=str(policy))
        request = json.dumps({'email': 'owner@example.com', 'display_name': 'Owner',
                              'idempotency_key': 'registration-contract', 'now': 1})
        result = subprocess.run([str(binary), 'provision-owner'], input=request,
                                text=True, capture_output=True, env=env, check=True)
        owner = json.loads(result.stdout)
        result_path = self.root / 'owner-result.json'
        provision.write_json(result_path, owner)
        self.assertEqual(provision.owner_result(self.root, result_path), owner)
        changed = dict(owner, recovery_threshold=2)
        result_path.unlink()
        provision.write_json(result_path, changed)
        with self.assertRaises(provision.Refused):
            provision.owner_result(self.root, result_path)
        self.assertEqual(owner['recovery_root'], json.loads(policy.read_text())['root'])
        self.assertEqual(owner['recovery_threshold'], 1)
        self.assertEqual(owner['recovery_delay_seconds'], 86400)
        replay = subprocess.run([str(binary), 'provision-owner'], input=request,
                                text=True, capture_output=True, env=env, check=True)
        self.assertEqual(json.loads(replay.stdout), owner)
        self.write(result.stdout)
        self.refused()
        self.assertNotIn(owner['did'], self.run_cli().stderr)

    def test_binding_refuses_actual_request_without_tenant(self):
        request = self.input / 'request.json'
        response = self.input / 'response.json'
        output = self.input / 'binding.json'
        provision.write_json(request, {'sub': 'owner', 'allowed_signer_public_keys': []})
        provision.write_json(response, {'sub': 'owner'})
        with self.assertRaises(provision.Refused) as caught:
            provision.preserve_binding(request, response, output)
        self.assertIn(str(request), str(caught.exception))
        self.assertFalse(output.exists())

    def test_binding_requires_matching_response_tenant_and_principal(self):
        request = self.input / 'request.json'
        response = self.input / 'response.json'
        output = self.input / 'binding.json'
        provision.write_json(request, {'tenant': 'beta', 'sub': 'owner'})
        for value in ({'sub': 'owner'}, {'tenant': 'other', 'sub': 'owner'},
                      {'tenant': 'beta', 'sub': 'other'}):
            provision.write_json(response, value)
            with self.assertRaises(provision.Refused):
                provision.preserve_binding(request, response, output)
            self.assertFalse(output.exists())
            response.unlink()
        provision.write_json(response, {'tenant': 'beta', 'sub': 'owner'})
        provision.preserve_binding(request, response, output)
        self.assertEqual(provision.protected_json(output), {'tenant': 'beta', 'principal': 'owner'})

    def test_catalog_refuses_missing_registry_before_output(self):
        template = Path(provision.__file__).with_name('beta-purpose-catalog.json')
        with self.assertRaises(provision.Refused) as caught:
            provision.purpose_catalog(template, self.input / 'module-registry.json',
                                      self.input / 'treasury.json', self.input / 'faucet.json', '')
        self.assertIn(str(self.input / 'module-registry.json'), str(caught.exception))
        self.assertFalse((self.root / 'human-evidence').exists())

    def test_protected_writer_never_overwrites_existing_files(self):
        provision.write_json(self.path, {'a': 1})
        self.assertEqual(self.path.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            provision.write_json(self.path, {'a': 2})
        self.assertEqual(provision.protected_json(self.path), {'a': 1})

    def test_job_mount_matches_runtime_identity_state(self):
        import yaml
        here = Path(provision.__file__).parent
        job = yaml.safe_load((here / 'provision-owner-job.yaml').read_text())
        node = next(d for d in yaml.safe_load_all((here.parent / 'node/deployment.yaml').read_text())
                    if d['kind'] == 'StatefulSet')
        runtime = next(c for c in node['spec']['template']['spec']['containers']
                       if c['name'] == 'human-identity')
        container = job['spec']['template']['spec']['containers'][0]
        state_var = 'LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT'
        self.assertEqual(next(e['value'] for e in runtime['env'] if e['name'] == state_var),
                         next(e['value'] for e in container['env'] if e['name'] == state_var))
        runtime_mount = next(m for m in runtime['volumeMounts'] if m['name'] == 'human-state')
        job_mount = next(m for m in container['volumeMounts'] if m['name'] == 'state')
        for key in ('mountPath', 'subPath'):
            self.assertEqual(runtime_mount[key], job_mount[key])
        self.assertEqual(job['spec']['backoffLimit'], 0)
        self.assertFalse(job['spec']['template']['spec']['automountServiceAccountToken'])

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
