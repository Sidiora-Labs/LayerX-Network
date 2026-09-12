#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[4]
NODE_DIR = ROOT / 'platform/hosted/node'
BIN = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))
sys.path.insert(0, str(ROOT / 'tests/support'))
from lxgb_metadata import metadata

ASSET = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')
PKCS8_PREFIX = bytes.fromhex('302e020100300506032b657004220420')
SPKI_PREFIX = bytes.fromhex('302a300506032b6570032100')
SEED_LINE = b'LAYERX_NODE_SEQUENCER_PRIVATE_KEY='


def memory_file(value):
    descriptor = os.memfd_create('sequencer-seed-test', 0)
    os.write(descriptor, value)
    return descriptor


def public_key_of(seed):
    descriptor = memory_file(PKCS8_PREFIX + seed)
    try:
        encoded = subprocess.run(
            ['openssl', 'pkey', '-inform', 'DER', '-in', f'/proc/self/fd/{descriptor}',
             '-pubout', '-outform', 'DER'],
            stdout=subprocess.PIPE, pass_fds=(descriptor,), check=True).stdout
    finally:
        os.close(descriptor)
    assert encoded.startswith(SPKI_PREFIX) and len(encoded) == 44
    return encoded[12:]


def write_seed(path, seed):
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'wb') as handle:
        handle.write(seed)


def environment_lines(path):
    return dict(line.split('=', 1) for line in path.read_text().splitlines())


class SequencerSeedTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='lx-sequencer-seed-')
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        os.chmod(self.work, 0o755)
        self.sequencer_seed = os.urandom(32)
        self.treasury_seed = os.urandom(32)
        self.sequencer_path = self.work / 'sequencer.key'
        self.treasury_path = self.work / 'treasury.key'
        write_seed(self.sequencer_path, self.sequencer_seed)
        write_seed(self.treasury_path, self.treasury_seed)
        (self.work / 'metadata').write_bytes(
            metadata(ASSET, public_key_of(self.treasury_seed), os.urandom(32)))
        self.data = self.work / 'data'
        self.run_dir = self.work / 'run'

    def settlement_document(self):
        path = ROOT / 'contracts/config/checkpoint-settlement.json'
        threshold = json.loads(path.read_text())['finality_policy']['certificate_threshold']
        self.assertGreaterEqual(threshold, 1)
        return path

    def clean_environment(self):
        return {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_')}

    def bootstrap_arguments(self, sequencer_key):
        return [
            '--network-id', '77',
            '--sequencer-key', str(sequencer_key),
            '--treasury-key', str(self.treasury_path),
            '--genesis-metadata', str(self.work / 'metadata'),
            '--settlement-document', str(self.settlement_document()),
            '--settlement-env', str(self.work / 'settlement.env'),
            '--lni-uid', str(os.getuid() + 1),
            '--genesis-build', str(BIN / 'layerx-genesis-build'),
        ]

    def bootstrap(self, sequencer_key=None, extra=()):
        if sequencer_key is None:
            sequencer_key = self.sequencer_path
        command = [
            'bash', str(NODE_DIR / 'bootstrap.sh'),
            '--data-dir', str(self.data), '--run-dir', str(self.run_dir),
            '--layerxd', str(BIN / 'layerxd'),
        ] + self.bootstrap_arguments(sequencer_key) + list(extra)
        return subprocess.run(command, cwd=str(ROOT), env=self.clean_environment(),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def supervisor_command(self):
        self.assertIsNotNone(shutil.which('socat'), 'socat is required by the supervisor')
        return [
            'bash', str(NODE_DIR / 'supervisor.sh'), '--role', 'sequencer',
            '--data-dir', str(self.data), '--run-dir', str(self.run_dir),
            '--layerxd', str(BIN / 'layerxd'), '--',
        ] + self.bootstrap_arguments(self.sequencer_path)

    def supervise(self):
        return subprocess.run(self.supervisor_command(), cwd=str(ROOT), env=self.clean_environment(),
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=120)

    def supervise_process(self):
        return subprocess.Popen(self.supervisor_command(), cwd=str(ROOT), env=self.clean_environment(),
                                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def assert_seed_only_in_its_file(self):
        seed_hex = self.sequencer_seed.hex().encode()
        for entry in self.work.rglob('*'):
            if entry.is_file() and entry != self.sequencer_path:
                content = entry.read_bytes()
                self.assertNotIn(self.sequencer_seed, content, str(entry))
                self.assertNotIn(seed_hex, content, str(entry))
                self.assertNotIn(seed_hex.upper(), content, str(entry))

    def test_bootstrap_records_the_key_file_and_keeps_no_seed_copy(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        sequencer_env = self.data / 'sequencer.env'
        exports = environment_lines(sequencer_env)
        self.assertNotIn('LAYERX_NODE_SEQUENCER_PRIVATE_KEY', exports)
        self.assertEqual(exports['LAYERX_NODE_SEQUENCER_KEY_FILE'], str(self.sequencer_path))
        self.assertEqual(exports['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'],
                         public_key_of(self.sequencer_seed).hex())
        self.assertEqual(sequencer_env.stat().st_mode & 0o777, 0o600)
        self.assertNotIn(SEED_LINE, sequencer_env.read_bytes())
        self.assertNotIn('LAYERX_NODE_SEQUENCER_KEY_FILE', environment_lines(self.data / 'node.env'))
        self.assertNotIn('LAYERX_NODE_SEQUENCER_KEY_FILE', environment_lines(self.data / 'replica.env'))
        self.assertFalse((self.data / 'work').exists())
        self.assert_seed_only_in_its_file()

    def test_bootstrap_resolves_a_relative_key_path_against_the_working_directory(self):
        relative = Path(os.path.relpath(self.sequencer_path, ROOT))
        result = self.bootstrap(sequencer_key=relative)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        exports = environment_lines(self.data / 'sequencer.env')
        self.assertEqual(exports['LAYERX_NODE_SEQUENCER_KEY_FILE'], str(ROOT / relative))

    def test_bootstrap_refuses_a_key_file_inside_the_data_directory(self):
        self.data.mkdir(mode=0o700)
        inside = self.data / 'sequencer.key'
        write_seed(inside, self.sequencer_seed)
        result = self.bootstrap(sequencer_key=inside, extra=['--force'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'the sequencer key file must be outside the data directory', result.stderr)
        self.assertTrue(inside.exists(), 'the refusal must happen before --force discards the directory')
        self.assertFalse((self.data / 'sequencer.env').exists())

    def test_bootstrap_refuses_a_key_file_linked_into_the_data_directory(self):
        self.data.mkdir(mode=0o700)
        write_seed(self.data / 'linked.key', self.sequencer_seed)
        link = self.work / 'sequencer-link.key'
        link.symlink_to(self.data / 'linked.key')
        result = self.bootstrap(sequencer_key=link, extra=['--force'])
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'the sequencer key file must be outside the data directory', result.stderr)
        self.assertTrue((self.data / 'linked.key').exists())

    def test_bootstrap_refuses_an_unreadable_key_file(self):
        result = self.bootstrap(sequencer_key=self.work / 'absent.key')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'--sequencer-key must name a readable regular file', result.stderr)
        self.assertFalse(self.data.exists())

    def test_supervisor_refuses_a_seed_line_in_sequencer_env(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        sequencer_env = self.data / 'sequencer.env'
        with sequencer_env.open('ab') as handle:
            handle.write(SEED_LINE + self.sequencer_seed.hex().encode() + b'\n')
        supervised = self.supervise()
        self.assertNotEqual(supervised.returncode, 0)
        self.assertIn(b'must not carry LAYERX_NODE_SEQUENCER_PRIVATE_KEY', supervised.stderr)
        self.assertFalse((self.run_dir / 'generation').exists())
        self.assertFalse((self.run_dir / 'supervisor.sock').exists())

    def test_supervisor_refuses_a_key_file_that_does_not_match_the_bound_public_key(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.sequencer_path.unlink()
        write_seed(self.sequencer_path, os.urandom(32))
        supervised = self.supervise()
        self.assertNotEqual(supervised.returncode, 0)
        self.assertIn(b'does not match the bound sequencer public key', supervised.stderr)
        self.assertFalse((self.run_dir / 'generation').exists())

    def test_supervisor_refuses_a_missing_key_file(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.sequencer_path.unlink()
        supervised = self.supervise()
        self.assertNotEqual(supervised.returncode, 0)
        self.assertIn(b'sequencer key file is not a regular file', supervised.stderr)
        self.assertFalse((self.run_dir / 'generation').exists())

    def test_supervisor_refuses_a_malformed_key_file(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.sequencer_path.unlink()
        write_seed(self.sequencer_path, b'not a seed\n')
        supervised = self.supervise()
        self.assertNotEqual(supervised.returncode, 0)
        self.assertIn(b'must hold 32 raw bytes or 64 hex characters', supervised.stderr)
        self.assertFalse((self.run_dir / 'generation').exists())

    def test_supervisor_binds_the_hex_form_of_the_same_seed_before_publishing(self):
        result = self.bootstrap()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.sequencer_path.unlink()
        write_seed(self.sequencer_path, self.sequencer_seed.hex().upper().encode() + b'\n')
        bound = b'sequencer seed bound from ' + str(self.sequencer_path).encode()
        process = self.supervise_process()
        seen = []
        try:
            for line in process.stderr:
                seen.append(line)
                if line.rstrip(b'\n').endswith(bound):
                    break
            self.assertTrue(seen and seen[-1].rstrip(b'\n').endswith(bound), b''.join(seen).decode())
            self.assertNotIn(b'does not match the bound sequencer public key', b''.join(seen))
            deadline = time.monotonic() + 60
            while not (self.run_dir / 'generation').exists():
                self.assertIsNone(process.poll(), 'the supervisor exited after binding the seed')
                self.assertLess(time.monotonic(), deadline, 'the generation was not published')
                time.sleep(0.05)
            self.assertEqual((self.run_dir / 'generation').read_text(), '1')
        finally:
            process.terminate()
            process.wait(timeout=60)


if __name__ == '__main__':
    unittest.main()
