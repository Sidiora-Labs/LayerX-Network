import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


ROOT = Path(__file__).resolve().parents[4]


def load(name, path):
    specification = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


AUTHORIZE = load('native_authorization', ROOT / 'cmd/layerx-guarantor/authorization.py')
PUBLICATION = load('native_publication', ROOT / 'cmd/layerx-guarantor/publication.py')
POLICY = ROOT / 'platform/hosted/tests/publication-policy.py'


class PublicationAuthorization(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix='layerx-publication-authority-'))
        cls.key = cls.root / 'authority.pem'
        descriptor = os.open(cls.key, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as destination:
            subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519'], stdout=destination,
                           stderr=subprocess.DEVNULL, check=True)
        public = subprocess.run(['openssl', 'pkey', '-in', str(cls.key), '-pubout', '-outform', 'DER'],
                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, check=True).stdout
        cls.public = public[12:]
        cls.path = cls.root / 'authorization.json'
        subprocess.run(['python3', str(POLICY), 'authorization', str(cls.path), '402', '125',
                        '0x' + '12' * 20, '0x' + '23' * 20, '0x' + '34' * 20,
                        cls.public.hex(), '45' * 32, '56' * 20], check=True)
        cls.policy = AUTHORIZE.configuration(cls.path)
        cls.policy['deposit_authority_key_file'] = str(cls.key)
        cls.message = (b'LX:PAXEER:DEPOSIT:ROOT:v1' + bytes.fromhex('67' * 32 + '78' * 32
                       + '89' * 32 + '00' * 12 + '34' * 20) + (402).to_bytes(4, 'big') + b'\0\3')

    def test_actual_protected_policy_and_duplicate_refusal(self):
        self.assertEqual(AUTHORIZE.configuration(self.path)['network_id'], 402)
        duplicate = self.root / 'duplicate.json'
        descriptor = os.open(duplicate, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'w') as target:
            target.write('{"version":1,"version":1}')
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            AUTHORIZE.configuration(duplicate)

    def test_policy_rejects_unprotected_links_and_unknown_fields(self):
        path = self.root / 'bad-policy.json'
        path.write_text(json.dumps(self.policy))
        path.chmod(0o640)
        with self.assertRaisesRegex(ValueError, 'protected'):
            AUTHORIZE.configuration(path)
        path.chmod(0o600)
        linked = self.root / 'policy-link.json'
        linked.symlink_to(path)
        with self.assertRaisesRegex(ValueError, 'canonical'):
            AUTHORIZE.configuration(linked)
        policy = copy.deepcopy(self.policy)
        policy['unexpected'] = True
        path.write_text(json.dumps(policy))
        with self.assertRaisesRegex(ValueError, 'fields'):
            AUTHORIZE.configuration(path)
        policy.pop('unexpected')
        policy['network_id'] = True
        path.write_text(json.dumps(policy))
        with self.assertRaisesRegex(ValueError, 'domain'):
            AUTHORIZE.configuration(path)

    def test_real_deposit_key_signs_exact_role_and_reopens_the_same_key(self):
        first = AUTHORIZE.sign_deposit(PUBLICATION, self.policy, self.public, self.message)
        second = AUTHORIZE.sign_deposit(PUBLICATION, self.policy, self.public, self.message)
        self.assertEqual(first, second)
        Ed25519PublicKey.from_public_bytes(self.public).verify(first, self.message)
        for index in (0, 23, 55, 87, len(self.message) - 6, len(self.message) - 1):
            with self.subTest(index=index):
                changed = bytearray(self.message)
                changed[index] ^= 1
                with self.assertRaises(ValueError):
                    PUBLICATION.signature(self.public, bytes(changed), first)

    def test_deposit_signing_refuses_wrong_authority_domain_and_material(self):
        wrong = bytes(value ^ 1 for value in self.public)
        with self.assertRaisesRegex(ValueError, 'vault authority'):
            AUTHORIZE.sign_deposit(PUBLICATION, self.policy, wrong, self.message)
        with self.assertRaisesRegex(ValueError, 'role or network'):
            AUTHORIZE.sign_deposit(PUBLICATION, self.policy, self.public, b'other role')
        changed = bytearray(self.message)
        changed[-3] ^= 1
        with self.assertRaisesRegex(ValueError, 'role or network'):
            AUTHORIZE.sign_deposit(PUBLICATION, self.policy, self.public, bytes(changed))
        self.key.chmod(0o640)
        try:
            with self.assertRaisesRegex(ValueError, 'protected'):
                AUTHORIZE.sign_deposit(PUBLICATION, self.policy, self.public, self.message)
        finally:
            self.key.chmod(0o600)
        policy = dict(self.policy, deposit_authority_key_file=str(self.root / 'missing.pem'))
        with self.assertRaises(FileNotFoundError):
            AUTHORIZE.sign_deposit(PUBLICATION, policy, self.public, self.message)
        self.assertFalse((self.root / 'missing.pem').exists())


if __name__ == '__main__':
    unittest.main()
