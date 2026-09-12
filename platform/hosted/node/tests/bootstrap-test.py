#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[4]
BIN = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))
sys.path.insert(0, str(ROOT / 'tests/support'))
from lxgb_metadata import metadata  # noqa: E402

ASSET = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')
PKCS8_PREFIX = bytes.fromhex('302e020100300506032b657004220420')


def public_key_of(seed):
    encoded = subprocess.run(
        ['openssl', 'pkey', '-inform', 'DER', '-pubout', '-outform', 'DER'],
        input=PKCS8_PREFIX + seed, stdout=subprocess.PIPE, check=True).stdout
    if len(encoded) != 44:
        raise ValueError('unexpected ed25519 SubjectPublicKeyInfo length %d' % len(encoded))
    return encoded[12:]


class BootstrapTest(unittest.TestCase):
    def bootstrap(self, work, threshold):
        document = json.loads((ROOT / 'contracts/config/checkpoint-settlement.json').read_text())
        document['finality_policy']['certificate_threshold'] = threshold
        policy = work / 'settlement.json'
        policy.write_text(json.dumps(document))
        seeds = {}
        for name in ('sequencer', 'treasury'):
            key = work / (name + '.key')
            seeds[name] = os.urandom(32)
            key.write_bytes(seeds[name])
            key.chmod(0o600)
        genesis_metadata = work / 'metadata'
        genesis_metadata.write_bytes(metadata(ASSET, public_key_of(seeds['treasury']), os.urandom(32)))
        env = {k: v for k, v in os.environ.items() if not k.startswith('LAYERX_')}
        process = subprocess.Popen([
            'bash', str(ROOT / 'platform/hosted/node/bootstrap.sh'),
            '--data-dir', str(work / 'data'), '--run-dir', str(work / 'run'),
            '--network-id', '4242', '--asset', ASSET.hex(),
            '--genesis-metadata', str(genesis_metadata),
            '--sequencer-key', str(work / 'sequencer.key'),
            '--treasury-key', str(work / 'treasury.key'),
            '--lni-uid', str(os.getuid() + 1),
            '--settlement-env', str(work / 'settlement.env'),
            '--settlement-document', str(policy),
            '--layerxd', str(BIN / 'layerxd'),
            '--genesis-build', str(BIN / 'layerx-genesis-build'),
        ], cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        request_file = None
        while process.poll() is None:
            if request_file is None:
                try:
                    request_file = (work / 'data/work/genesis-request.lxgb').open('rb')
                except FileNotFoundError:
                    pass
            time.sleep(0.001)
        stdout, stderr = process.communicate()
        self.request = None
        if request_file is not None:
            with request_file:
                self.request = request_file.read()
        return subprocess.CompletedProcess(process.args, process.returncode, stdout, stderr)

    def test_real_genesis_sets(self):
        for count in (1, 2, 3, 32):
            with self.subTest(count=count), tempfile.TemporaryDirectory(prefix='lxgb-') as directory:
                work = Path(directory)
                result = self.bootstrap(work, count)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                data = work / 'data'
                request = self.request
                self.assertIsNotNone(request)
                self.assertEqual(len(request), 314 + 81 * count)
                self.assertEqual(request[:5], b'LXGB\x01')
                self.assertEqual(struct.unpack('>H', request[87:89])[0], count)
                exports = dict(line.split('=', 1) for line in (data / 'node.env').read_text().splitlines())
                self.assertEqual(int(exports['LAYERX_NODE_GENESIS_GUARANTOR_COUNT']), count)
                ids, publics = [], []
                for index in range(count):
                    entry = request[89 + index * 81:89 + (index + 1) * 81]
                    identifier, public = entry[:32].hex(), entry[32:65].hex()
                    self.assertEqual(entry[65:], bytes(16))
                    self.assertEqual(identifier, hashlib.sha256(('layerx-beta-guarantor:' + public).encode()).hexdigest())
                    self.assertEqual(exports[f'LAYERX_NODE_GENESIS_GUARANTOR_ID_{index}'], identifier)
                    self.assertEqual(exports[f'LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY_{index}'], public)
                    ids.append(identifier)
                    publics.append(public)
                self.assertEqual(ids, sorted(set(ids)))
                key_publics = []
                keys = list((data / 'secrets').glob('guarantor-key*.pem'))
                self.assertEqual(len(keys), count)
                legacy_key = Path(exports['LAYERX_NODE_GENESIS_GUARANTOR_KEY_FILE'])
                self.assertIn(legacy_key, keys)
                for key in keys:
                    self.assertEqual(key.stat().st_mode & 0o777, 0o600)
                    der = subprocess.check_output(['openssl', 'ec', '-in', str(key), '-pubout', '-conv_form', 'compressed', '-outform', 'DER'], stderr=subprocess.DEVNULL)
                    key_publics.append(der[-33:].hex())
                    if key == legacy_key:
                        self.assertEqual(exports['LAYERX_NODE_GENESIS_GUARANTOR_PUBLIC_KEY'], der[-33:].hex())
                        self.assertEqual(exports['LAYERX_NODE_GENESIS_GUARANTOR_KEY_FILE'], str(key))
                self.assertEqual(sorted(key_publics), sorted(publics))
                self.assertIn(exports['LAYERX_NODE_GENESIS_GUARANTOR_ID'], ids)
                for artifact in ('genesis.manifest', '00000000000000000000.lxs', 'paxeer-registration-request.lxrr', 'paxeer-deployment-descriptor.lxgd'):
                    self.assertGreater((data / 'genesis' / artifact).stat().st_size, 0)

    def test_invalid_thresholds_refused_before_generation(self):
        for threshold in (0, -1, 33, 1.5, '2', None, True):
            with self.subTest(threshold=threshold), tempfile.TemporaryDirectory(prefix='lxgb-') as directory:
                work = Path(directory)
                result = self.bootstrap(work, threshold)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'certificate threshold must be an integer in 1..32', result.stderr)
                self.assertFalse((work / 'data').exists())


if __name__ == '__main__':
    unittest.main()
