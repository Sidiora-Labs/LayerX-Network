import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, ROOT / path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


FEES = load('genesis_fees', 'platform/hosted/node/genesis_fees.py')
METADATA = load('lxgb_metadata', 'tests/support/lxgb_metadata.py')
PREPARE = load('prepare_beta', 'platform/hosted/paxeer/prepare-beta.py')
ASSET = bytes.fromhex(PREPARE.BETA_ASSET_ID)


class WithdrawalGenesisTest(unittest.TestCase):
    def test_explicit_price_preserves_records_and_existing_fee_components(self):
        legacy = METADATA.check()
        expanded = FEES.withdrawal_metadata(legacy, 17)
        self.assertEqual(expanded, METADATA.metadata_withdrawal(
            METADATA.VECTOR_ASSET, METADATA.VECTOR_ISSUER_PUBLIC, METADATA.VECTOR_SALT, 17))
        self.assertEqual(expanded[-255:], (ROOT / 'tests/fixtures/fee-params-v3.bin').read_bytes())
        self.assertEqual(expanded[:-257], legacy[:-249])
        self.assertEqual(expanded[-253:-169], legacy[-245:-161])
        self.assertEqual(expanded[-168:-8], legacy[-160:])
        self.assertEqual(FEES.withdrawal_metadata(expanded, 17), expanded)
        for price in (-1, 2**64, True, '17'):
            with self.subTest(price=price), self.assertRaises(ValueError):
                FEES.withdrawal_metadata(legacy, price)
        for malformed in (b'', legacy[:-1], legacy + b'\0', b'\0\0' + legacy[2:],
                          legacy[:-247] + b'\0\x01' + legacy[-245:]):
            with self.subTest(length=len(malformed)), self.assertRaises(ValueError):
                FEES.withdrawal_metadata(malformed, 17)
        with self.assertRaises(ValueError):
            FEES.withdrawal_metadata(expanded, 0)

    def test_public_bootstrap_and_real_builder_bind_v3_and_native_authority(self):
        for price in (0, 17):
            with self.subTest(price=price), tempfile.TemporaryDirectory(prefix='lxp-paid-genesis-', dir='/tmp') as directory:
                work = Path(directory)
                for name in ('sequencer', 'treasury'):
                    path = work / name
                    path.write_bytes(os.urandom(32))
                    path.chmod(0o600)
                public = Ed25519PrivateKey.from_private_bytes((work / 'treasury').read_bytes()).public_key().public_bytes_raw()
                original = METADATA.metadata(ASSET, public, os.urandom(32))
                (work / 'metadata').write_bytes(original)
                args = ['bash', str(ROOT / 'platform/hosted/node/bootstrap.sh'),
                        '--data-dir', str(work / 'data'), '--run-dir', str(work / 'run'),
                        '--network-id', '77', '--genesis-metadata', str(work / 'metadata'),
                        '--withdrawal-fee', str(price), '--sequencer-key', str(work / 'sequencer'),
                        '--treasury-key', str(work / 'treasury'), '--lni-uid', str(os.getuid() + 1),
                        '--settlement-env', str(work / 'settlement.env'),
                        '--layerxd', str(BIN / 'layerxd'), '--genesis-build', str(BIN / 'layerx-genesis-build')]
                environment = {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_')}
                result = subprocess.run(args, cwd=ROOT, env=environment, capture_output=True, check=False)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                self.assertEqual((work / 'metadata').read_bytes(), original)
                genesis = work / 'data/genesis'
                request = genesis / 'genesis-request.lxgb'
                encoded = request.read_bytes()
                self.assertEqual(encoded[:7], b'LXGB\x02\0\x03')
                self.assertEqual(int.from_bytes(encoded[19:21], 'big'), 7)
                expected = {'parameter-version': 1, 'native-fee-authority-version': 2,
                            **{'module-enable:' + name: 1 for name in PREPARE.genesis_modules()}}
                for index, (key, value) in enumerate(sorted(expected.items())):
                    self.assertEqual(encoded[21 + index * 66:21 + (index + 1) * 66],
                                     b'\0\x07' + key.encode().ljust(32, b'\0') + value.to_bytes(32, 'big'))
                self.assertTrue(encoded.endswith(FEES.withdrawal_metadata(original, price)))
                rebuilt = work / 'rebuilt'
                subprocess.run([str(BIN / 'layerx-genesis-build'), str(request), str(work / 'sequencer'), str(rebuilt)],
                               cwd=ROOT, check=True, capture_output=True)
                for name in ('genesis.manifest', '00000000000000000000.lxs',
                             'paxeer-registration-request.lxrr', 'paxeer-deployment-descriptor.lxgd'):
                    self.assertEqual((genesis / name).read_bytes(), (rebuilt / name).read_bytes())
                roots = (genesis / 'paxeer-registration-request.lxrr').read_bytes()
                self.assertEqual(len(roots), 73)
                self.assertNotEqual(roots[9:41], roots[41:73])


if __name__ == '__main__':
    unittest.main()
