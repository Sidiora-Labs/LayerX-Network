import base64
import hashlib
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

import owner_custody
import provision

HERE = Path(__file__).resolve().parent
PROVIDER = HERE.parents[2] / 'human/crates/layerx-human-movement-provider/src'
CLUSTER = HERE.parents[0] / 'tests/beta-cluster.sh'
TRANSACTION = '0x' + bytes(range(32)).hex()


def big(value, length):
    return value.to_bytes(length, 'big')


def light_credit(bundle=b'LXLB1' + bytes([15] * 600)):
    profile = (b'LXBC3' + big(125, 8) + bytes([8] * 20) + bytes([1] * 32) + bytes([10] * 32) + bytes([2] * 32)
               + bytes([3] * 32) + big(1, 8) + b'hyperpax_125-1'.ljust(32, b'\0') + big(77, 4) + big(3, 2))
    assert len(profile) == 223
    head = (b'LXDC3' + hashlib.sha256(profile).digest() + big(77, 4) + big(3, 2) + bytes([5] * 32)
            + bytes([2] * 32) + bytes([6] * 32) + bytes([7] * 32) + bytes([9] * 20) + big(10 ** 18, 16)
            + big(1, 8) + big(12, 8) + bytes([11] * 32) + bytes([12] * 32) + big(13, 8) + bytes([14] * 32)
            + hashlib.sha256(bundle).digest() + big(2, 4))
    assert len(head) == 363
    return profile, head + bundle


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
            _, credit = light_credit()
            owner_custody.write_new(root / 'custody-credit.bin', credit)
            published = owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertEqual(published, root / ('credit-' + TRANSACTION[2:] + '.bin'))
            info = published.lstat()
            self.assertTrue(stat.S_ISREG(info.st_mode))
            self.assertEqual((stat.S_IMODE(info.st_mode), info.st_nlink, info.st_size, info.st_uid),
                             (0o600, 1, len(credit), os.geteuid()))
            payload = provision.protected_bytes(published)
            self.assertEqual(payload, credit)
            self.assertEqual(provision.protected_bytes(root / 'custody-credit.bin'), credit)
            self.assertEqual(payload[327:359], hashlib.sha256(payload[363:]).digest())
            self.assertEqual(sorted(path.name for path in root.iterdir()),
                             ['credit-' + TRANSACTION[2:] + '.bin', 'custody-credit.bin'])
            with self.assertRaises(FileExistsError):
                owner_custody.publish_credit_material(root, TRANSACTION)

    def test_publish_refuses_a_credit_whose_head_does_not_bind_its_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            _, credit = light_credit()
            owner_custody.write_new(root / 'custody-credit.bin', credit[:-1] + bytes([credit[-1] ^ 1]))
            with self.assertRaises(provision.Refused) as refused:
                owner_custody.publish_credit_material(root, TRANSACTION)
            self.assertIn('light-client custody credit layout', str(refused.exception))
            self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])
            (root / 'custody-credit.bin').chmod(0o640)
            with self.assertRaises(provision.Refused):
                owner_custody.publish_credit_material(root, '0x' + '01' * 32)
            self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])

    def test_publish_refuses_a_malformed_credit_layout(self):
        with tempfile.TemporaryDirectory() as directory:
            root = evidence_input(directory)
            for retired in (b'LXDC9' + bytes(422), b'LXDC1' + bytes(422), b'LXDC2' + bytes(422), light_credit()[1][:363]):
                owner_custody.write_new(root / 'custody-credit.bin', retired)
                with self.assertRaises(provision.Refused) as refused:
                    owner_custody.publish_credit_material(root, TRANSACTION)
                self.assertIn('light-client custody credit layout', str(refused.exception))
                self.assertEqual([path.name for path in root.iterdir()], ['custody-credit.bin'])
                (root / 'custody-credit.bin').unlink()


def delivery_script():
    text = CLUSTER.read_text()
    return text.split("cat <<'HUMAN_EVIDENCE_DELIVERY'\n", 1)[1].split('\nHUMAN_EVIDENCE_DELIVERY\n', 1)[0] + '\n'


def deliver(root, credit):
    return subprocess.run(['sh', '-s', '--', str(root), TRANSACTION[2:], base64.b64encode(credit).decode(),
                           hashlib.sha256(credit).hexdigest()],
                          input=delivery_script().encode(), capture_output=True)


def private_stat(path):
    info = path.lstat()
    return stat.S_ISREG(info.st_mode), stat.S_IMODE(info.st_mode), info.st_nlink, info.st_uid


class ClusterEvidenceDeliveryTests(unittest.TestCase):
    def test_delivery_installs_the_credit_beside_the_provider_deposit_proof(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve() / 'evidence'
            root.mkdir(mode=0o700)
            _, credit = light_credit()
            published = root / ('credit-' + TRANSACTION[2:] + '.bin')
            deposit = root / ('deposit-' + TRANSACTION[2:] + '.bin')
            refused = deliver(root, credit)
            self.assertNotEqual(refused.returncode, 0)
            self.assertIn(str(deposit) + ': evidence file missing', refused.stderr.decode())
            self.assertEqual(published.read_bytes(), credit)
            owner_custody.write_new(deposit, b'movement provider deposit proof publication')
            delivered = deliver(root, credit)
            self.assertEqual((delivered.returncode, delivered.stderr), (0, b''))
            self.assertEqual(private_stat(published), (True, 0o600, 1, os.geteuid()))
            self.assertEqual(private_stat(deposit), (True, 0o600, 1, os.geteuid()))
            self.assertEqual(provision.protected_bytes(published), credit)
            self.assertEqual(sorted(path.name for path in root.iterdir()), sorted([deposit.name, published.name]))
            self.assertEqual(deliver(root, credit).returncode, 0)
            self.assertEqual(published.read_bytes(), credit)
            _, other = light_credit(b'LXLB1' + bytes([16] * 600))
            tampered = deliver(root, other)
            self.assertNotEqual(tampered.returncode, 0)
            self.assertIn('custody credit bytes differ from the produced credit', tampered.stderr.decode())
            self.assertEqual(published.read_bytes(), credit)
            deposit.chmod(0o640)
            exposed = deliver(root, credit)
            self.assertNotEqual(exposed.returncode, 0)
            self.assertIn(str(deposit) + ': evidence file is not private to the movement provider', exposed.stderr.decode())

    def test_delivery_refuses_a_credit_that_carries_no_light_client_bundle(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve() / 'evidence'
            root.mkdir(mode=0o700)
            owner_custody.write_new(root / ('deposit-' + TRANSACTION[2:] + '.bin'), b'movement provider deposit proof publication')
            short = deliver(root, light_credit()[1][:363])
            self.assertNotEqual(short.returncode, 0)
            self.assertIn('custody credit carries no light-client bundle', short.stderr.decode())
            missing = subprocess.run(['sh', '-s', '--', str(root / 'absent'), TRANSACTION[2:], '', hashlib.sha256(b'').hexdigest()],
                                     input=delivery_script().encode(), capture_output=True)
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn('movement provider evidence root missing', missing.stderr.decode())

    def test_cluster_up_publishes_the_deposit_proof_in_the_movement_container_before_delivery(self):
        text = CLUSTER.read_text()
        step = text.split('human_custody_evidence_publish() {', 1)[1].split('\n}\n', 1)[0]
        self.assertIn("latestCanonicalCheckpointHash()", text.split('custody_latest_checkpoint() {', 1)[1].split('\n}\n', 1)[0])
        self.assertIn('kube -n "$ns" exec layerx-node-0 -c human-movement -- sh -ec', step)
        self.assertIn('/usr/local/bin/layerx-runtime-clock --runtime-dir "$runtime" --', step)
        self.assertIn('/usr/local/bin/layerx-human-movement-provider --publish-deposit-proof "$@"', step)
        self.assertLess(step.index('--publish-deposit-proof'), step.index('human_evidence_delivery_script | kube'))
        self.assertIn('sh -s -- /var/lib/layerx/human/evidence "${transaction#0x}" "$(base64 -w0 "$credit")" "$(sha256sum "$credit" | cut -c1-64)"', step)
        up = text.split('beta_cluster_up() {', 1)[1].split('\n}\n', 1)[0]
        ready = up.index('wait_for_pod_ready "$TESTNET_NAMESPACE" app=layerx-node 600')
        publish = up.index('if [ "${LAYERX_BETA_RETAIN_MATERIAL:-0}" != 1 ]; then human_custody_evidence_publish; fi')
        self.assertLess(ready, publish)
        self.assertLess(publish, up.index('wait_ready ||'))


if __name__ == '__main__':
    unittest.main()
