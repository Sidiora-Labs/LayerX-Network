import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption

import guardians
import owner_native
import provision


def native_inputs(work):
    """Every protected input the native producer reads before the custody credit."""
    secrets = work / 'secrets'
    secrets.mkdir(mode=0o700)
    for role in owner_native.GUARDIAN_ROLES:
        guardians.enroll(work, secrets, role, hashlib.sha256(role.encode()).hexdigest(), 1)
    guardians.assemble(work)
    root = (work / 'human-evidence-input').resolve()
    authority = work / 'authority'
    authority.mkdir(mode=0o700)
    custody = work / 'owner'
    custody.mkdir(mode=0o700)
    public = {}
    for name in ('owner', 'pending', 'sequencer'):
        key = Ed25519PrivateKey.generate()
        public[name] = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        if name != 'sequencer':
            owner_native.protected_write(custody / (name + '.seed'),
                                         key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
    did = 'did:layerx:act_' + hashlib.sha256(b'beta owner').hexdigest()[:32]
    account = hashlib.sha256(b'LX:ACCOUNT:v1' + owner_native.span(b'agent:' + did.encode() + b':main')).hexdigest()
    provision.write_json(root / 'owner-admission.json',
                         dict(did=did, public_key=public['owner'].hex(), owner_account=account))
    policy = json.loads((root / 'recovery-policy.json').read_text())
    provision.write_json(work / 'human-owner-result.json',
                         dict(principal='beta-owner', did=did, recovery_root=policy['root'],
                              recovery_threshold=policy['threshold'],
                              recovery_delay_seconds=policy['delay_seconds']))
    provision.write_json(root / 'owner-native.json',
                         dict(node_socket=str(work / 'layerxd.lni.sock'), network_id=1,
                              owner_seed_file=str(custody / 'owner.seed'),
                              pending_seed_file=str(custody / 'pending.seed'),
                              sequencer_public_key=public['sequencer'].hex(),
                              layerxctl='/usr/local/bin/layerxctl', fee_limit=0,
                              authority_url='https://localhost:9445',
                              authority_token_file=str(work / 'authority.token'),
                              authority_ca_file=str(work / 'ca.crt'),
                              authority_state_root=str(authority)))
    return root


def republish(path, document):
    path.unlink()
    provision.write_json(path, document)


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


class GuardianBindingConsumptionTests(unittest.TestCase):
    def test_signed_guardian_bindings_are_accepted_before_the_custody_credit(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve()
            root = native_inputs(work)
            document = json.loads((root / 'recovery-guardians.json').read_text())
            self.assertEqual(len(document['members']), len(owner_native.GUARDIAN_ROLES))
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(work)
            self.assertIn('custody-credit.bin', str(refused.exception))
            self.assertFalse((root / 'owner-registration.json').exists())

    def test_forged_guardian_binding_refuses_the_producer(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve()
            root = native_inputs(work)
            path = root / 'recovery-guardians.json'
            document = json.loads(path.read_text())
            member = document['members'][0]
            member['signature'] = '%0128x' % (int(member['signature'], 16) ^ 1)
            republish(path, document)
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(work)
            self.assertIn('recovery-guardians.json', str(refused.exception))
            self.assertFalse((root / 'owner-native-run').exists())

    def test_unsigned_guardian_set_refuses_the_producer(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve()
            root = native_inputs(work)
            path = root / 'recovery-guardians.json'
            republish(path, dict(public_keys=json.loads(path.read_text())['public_keys']))
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(work)
            self.assertIn('recovery-guardians.json', str(refused.exception))

    def test_substituted_guardian_set_refuses_the_recovery_commitment(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve()
            root = native_inputs(work)
            other = work / 'other'
            other.mkdir()
            secrets = other / 'secrets'
            secrets.mkdir(mode=0o700)
            for role in owner_native.GUARDIAN_ROLES:
                guardians.enroll(other, secrets, role, hashlib.sha256(role.encode()).hexdigest(), 1)
            replacement = guardians.assemble(other)
            republish(root / 'recovery-guardians.json', replacement)
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(work)
            self.assertIn('recovery-policy.json', str(refused.exception))

    def test_rotated_guardian_set_is_accepted_with_its_own_recovery_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory).resolve()
            root = native_inputs(work)
            secrets = work / 'secrets'
            guardians.enroll(work, secrets, 'sequencer', hashlib.sha256(b'sequencer').hexdigest(), 2)
            guardians.authorize(work, secrets, 'sequencer', 2)
            rotated = guardians.assemble(work, root / 'guardian-epoch-2')
            self.assertEqual(rotated['version'], 2)
            republish(root / 'recovery-guardians.json', rotated)
            republish(root / 'recovery-policy.json',
                      json.loads((root / 'guardian-epoch-2/recovery-policy.json').read_text()))
            owner = json.loads((work / 'human-owner-result.json').read_text())
            owner['recovery_root'] = json.loads((root / 'recovery-policy.json').read_text())['root']
            republish(work / 'human-owner-result.json', owner)
            with self.assertRaises(provision.Refused) as refused:
                owner_native.produce(work)
            self.assertIn('custody-credit.bin', str(refused.exception))
