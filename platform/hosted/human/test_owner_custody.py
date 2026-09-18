import hashlib
import os
from pathlib import Path
import stat
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

import owner_custody
import provision

HERE = Path(__file__).resolve().parent
PROVIDER = HERE.parents[2] / 'human/crates/layerx-human-movement-provider/src'
TRANSACTION = '0x' + bytes(range(32)).hex()


def big(value, length):
    return value.to_bytes(length, 'big')


def attested_credit(evidence_hash, version=b'LXDC1'):
    key = Ed25519PrivateKey.generate()
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    profile = (b'LXBC1' + big(31337, 8) + bytes([8] * 20) + bytes([1] * 32) + public + bytes([2] * 32)
               + bytes([3] * 32) + big(1, 8) + bytes([4] * 32) + big(77, 4) + big(3, 2))
    assert len(profile) == 207
    unsigned = (version + hashlib.sha256(profile).digest() + big(77, 4) + big(3, 2) + bytes([5] * 32)
                + bytes([2] * 32) + bytes([6] * 32) + bytes([7] * 32) + bytes([9] * 20) + big(10 ** 18, 16)
                + big(1, 8) + big(12, 8) + bytes([11] * 32) + bytes([12] * 32) + big(13, 8) + bytes([14] * 32)
                + evidence_hash + big(0, 4))
    assert len(unsigned) == 363
    domain = b'LX:CUSTODY:CREDIT:v1' if version == b'LXDC1' else b'LX:CUSTODY:CREDIT:v2'
    return profile, unsigned + key.sign(domain + unsigned)


def evidence_input(directory):
    root = Path(directory).resolve() / 'human-evidence-input'
    root.mkdir(mode=0o700)
    return root


class OwnerCustodyCreditMaterialTests(unittest.TestCase):
    def test_per_transaction_name_matches_the_movement_provider_lookup(self):
        name = owner_custody.credit_material_name('0x' + 'AB' * 32)
        self.assertEqual(name, 'credit-' + 'ab' * 32 + '.bin')
        self.assertRegex(name, r'^credit-[0-9a-f]{64}\.bin$')
        service = (PROVIDER / 'service.rs').read_text()
        self.assertIn('.join(format!("credit-{}.bin", hex_string(&transaction.bytes())));', service)
        self.assertIn('read_private(&path, 427)', service)
        self.assertIn('const DIGITS: &[u8; 16] = b"0123456789abcdef";', (PROVIDER / 'config.rs').read_text())
        for refused in ('ab' * 32, '0x' + 'ab' * 31, '0x' + 'ab' * 33, '0x' + 'zz' * 32):
            with self.assertRaises(ValueError):
                owner_custody.credit_material_name(refused)

    def test_publish_writes_the_provider_copy_beside_the_native_producer_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            profile, credit = attested_credit(bytes.fromhex(TRANSACTION[2:]))
            owner_custody.write_new(root / 'custody-credit.bin', credit)
            published = owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertEqual(published, root / ('credit-' + TRANSACTION[2:] + '.bin'))
            info = published.lstat()
            self.assertTrue(stat.S_ISREG(info.st_mode))
            self.assertEqual((stat.S_IMODE(info.st_mode), info.st_nlink, info.st_size, info.st_uid),
                             (0o600, 1, 427, os.geteuid()))
            payload = provision.protected_bytes(published, 427)
            self.assertEqual(payload, credit)
            self.assertEqual(provision.protected_bytes(root / 'custody-credit.bin', 427), credit)
            self.assertEqual(payload[327:359], bytes.fromhex(TRANSACTION[2:]))
            Ed25519PublicKey.from_public_bytes(profile[65:97]).verify(
                payload[363:], b'LX:CUSTODY:CREDIT:v1' + payload[:363])
            self.assertEqual(sorted(path.name for path in root.iterdir()),
                             ['credit-' + TRANSACTION[2:] + '.bin', 'custody-credit.bin'])
            with self.assertRaises(FileExistsError):
                owner_custody.publish_credit_material(root, TRANSACTION)

    def test_publish_refuses_a_credit_that_does_not_bind_the_named_transaction(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            _, credit = attested_credit(bytes([1] * 32))
            owner_custody.write_new(root / 'custody-credit.bin', credit)
            with self.assertRaises(provision.Refused) as refused:
                owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertIn('custody credit transaction binding', str(refused.exception))
            self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])
            (root / 'custody-credit.bin').chmod(0o640)
            with self.assertRaises(provision.Refused):
                owner_custody.publish_credit_material(root, '0x' + '01' * 32)
            self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])

    def test_publish_refuses_a_malformed_credit_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            owner_custody.write_new(root / 'custody-credit.bin', b'LXDC9' + bytes(422))
            with self.assertRaises(provision.Refused) as refused:
                owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertIn('attested custody credit layout', str(refused.exception))
            self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])

    def test_publish_keeps_comet_state_evidence_without_a_receipt_transaction_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            _, credit = attested_credit(bytes([1] * 32), b'LXDC2')
            owner_custody.write_new(root / 'custody-credit.bin', credit)
            published = owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertEqual(published.name, 'credit-' + TRANSACTION[2:] + '.bin')
            self.assertEqual(provision.protected_bytes(published, 427), credit)


if __name__ == '__main__':
    unittest.main()
