import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[4]
BIN = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))
SPEC = importlib.util.spec_from_file_location('lxgb_metadata', ROOT / 'tests/support/lxgb_metadata.py')
METADATA = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(METADATA)
ASSET = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')


class GenesisModulesTest(unittest.TestCase):
    def bootstrap(self, work, modules):
        for name in ('sequencer', 'treasury'):
            path = work / name
            path.write_bytes(os.urandom(32))
            path.chmod(0o600)
        public = Ed25519PrivateKey.from_private_bytes((work / 'treasury').read_bytes()).public_key().public_bytes_raw()
        (work / 'metadata').write_bytes(METADATA.metadata(ASSET, public, os.urandom(32)))
        args = ['bash', str(ROOT / 'platform/hosted/node/bootstrap.sh'),
                '--data-dir', str(work / 'data'), '--run-dir', str(work / 'run'),
                '--network-id', '77', '--genesis-metadata', str(work / 'metadata'),
                '--sequencer-key', str(work / 'sequencer'), '--treasury-key', str(work / 'treasury'),
                '--lni-uid', str(os.getuid() + 1), '--settlement-env', str(work / 'settlement.env'),
                '--layerxd', str(BIN / 'layerxd'), '--genesis-build', str(BIN / 'layerx-genesis-build')]
        for module in modules:
            args += ['--enable-module', module]
        env = {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_')}
        return subprocess.run(args, cwd=ROOT, env=env, capture_output=True, check=False)

    def test_enabled_rows_are_canonical_and_rebuild_identically(self):
        for modules in (('stream',), ('service', 'perps', 'stream', 'escrow', 'budget')):
            with self.subTest(modules=modules), tempfile.TemporaryDirectory(prefix='lxp-genesis-modules-', dir='/tmp') as directory:
                work = Path(directory)
                result = self.bootstrap(work, modules)
                self.assertEqual(result.returncode, 0, result.stderr.decode())
                genesis = work / 'data/genesis'
                request = genesis / 'genesis-request.lxgb'
                data = request.read_bytes()
                self.assertEqual(data[:5], b'LXGB\x02')
                count = int.from_bytes(data[19:21], 'big')
                self.assertEqual(count, 1 + len(modules))
                parameters = []
                for index in range(count):
                    entry = data[21 + index * 66:21 + (index + 1) * 66]
                    self.assertEqual(entry[:2], b'\0\x07')
                    self.assertEqual(entry[34:], bytes(31) + b'\x01')
                    parameters.append(entry[2:34])
                expected = [b'parameter-version'.ljust(32, b'\0')]
                expected += [('module-enable:' + module).encode().ljust(32, b'\0') for module in modules]
                self.assertEqual(parameters, sorted(expected))
                rebuilt = work / 'rebuilt'
                subprocess.run([str(BIN / 'layerx-genesis-build'), str(request), str(work / 'sequencer'), str(rebuilt)],
                               cwd=ROOT, check=True, capture_output=True)
                for name in ('genesis.manifest', '00000000000000000000.lxs',
                             'paxeer-registration-request.lxrr', 'paxeer-deployment-descriptor.lxgd'):
                    self.assertEqual((genesis / name).read_bytes(), (rebuilt / name).read_bytes())

    def test_unknown_and_repeated_modules_refused_before_generation(self):
        for modules, refusal in ((('asset',), b'--enable-module requires'),
                                 (('escrow', 'escrow'), b'--enable-module repeats escrow'),
                                 (('Escrow',), b'--enable-module requires'),
                                 (('',), b'--enable-module requires')):
            with self.subTest(modules=modules), tempfile.TemporaryDirectory(prefix='lxp-genesis-modules-', dir='/tmp') as directory:
                work = Path(directory)
                result = self.bootstrap(work, modules)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(refusal, result.stderr)
                self.assertFalse((work / 'data').exists())


if __name__ == '__main__':
    unittest.main()
