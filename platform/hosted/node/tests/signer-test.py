#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[4]
SIGNER_DIR = ROOT / 'platform/hosted/node/signer'
BIN = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))
sys.path.insert(0, str(ROOT / 'tests/support'))
sys.path.insert(0, str(SIGNER_DIR))
from client import SignerClient, SignerError
from lxgb_metadata import metadata

ASSET = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')
PKCS8_PREFIX = bytes.fromhex('302e020100300506032b657004220420')
SPKI_PREFIX = bytes.fromhex('302a300506032b6570032100')

PROVIDER_PROGRAM = '''#!/usr/bin/env python3
import os
import subprocess
import sys

PKCS8 = bytes.fromhex('302e020100300506032b657004220420')
SEED = bytes.fromhex(open(sys.argv[1]).read().strip())
MODE = sys.argv[2]
COUNTER = sys.argv[3]


def key(seed):
    handle = os.memfd_create('provider-key', 0)
    os.write(handle, PKCS8 + seed)
    return handle


def run(arguments, descriptors):
    result = subprocess.run(arguments, stdout=subprocess.PIPE, pass_fds=descriptors, check=True)
    return result.stdout


def rotate():
    count = 0
    if os.path.exists(COUNTER):
        count = int(open(COUNTER).read())
    open(COUNTER, 'w').write(str(count + 1))
    return count


seed = SEED
if MODE == 'rotating' and rotate() > 0:
    seed = bytes(32 - len(b'rotated')) + b'rotated'
handle = key(seed)
if sys.argv[4] == 'public-key':
    encoded = run(['openssl', 'pkey', '-inform', 'DER', '-in', f'/proc/self/fd/{handle}',
                   '-pubout', '-outform', 'DER'], (handle,))
    sys.stdout.write(encoded[-32:].hex())
    sys.exit(0)
if sys.argv[4] != 'sign':
    sys.exit(2)
digest = bytes.fromhex(sys.argv[5])
if MODE == 'forging':
    digest = bytes(a ^ 1 for a in digest)
message = os.memfd_create('provider-digest', 0)
os.write(message, digest)
signature = run(['openssl', 'pkeyutl', '-sign', '-rawin', '-keyform', 'DER',
                 '-inkey', f'/proc/self/fd/{handle}', '-in', f'/proc/self/fd/{message}'],
                (handle, message))
sys.stdout.write(signature.hex())
'''


def memory_file(value):
    descriptor = os.memfd_create('signer-test', 0)
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


def verify(public_key, digest, signature):
    key = memory_file(SPKI_PREFIX + public_key)
    message = memory_file(digest)
    proof = memory_file(signature)
    try:
        result = subprocess.run(
            ['openssl', 'pkeyutl', '-verify', '-rawin', '-pubin', '-keyform', 'DER',
             '-inkey', f'/proc/self/fd/{key}', '-in', f'/proc/self/fd/{message}',
             '-sigfile', f'/proc/self/fd/{proof}'],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            pass_fds=(key, message, proof), check=False)
    finally:
        for descriptor in (key, message, proof):
            os.close(descriptor)
    return result.returncode == 0


def raw_request(path, payload):
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    connection.settimeout(10)
    try:
        connection.connect(str(path))
        connection.sendall(payload)
        buffered = b''
        while b'\n' not in buffered and len(buffered) < 65536:
            chunk = connection.recv(4096)
            if not chunk:
                break
            buffered += chunk
    finally:
        connection.close()
    return json.loads(buffered.split(b'\n', 1)[0].decode())


class SignerCase(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='lx-signer-')
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        os.chmod(self.work, 0o755)
        self.processes = []
        self.addCleanup(self.stop_all)

    def stop_all(self):
        for process in self.processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)

    def seed_file(self, name='treasury.key', mode=0o600, seed=None):
        seed = seed if seed is not None else os.urandom(32)
        path = self.work / name
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
        with os.fdopen(descriptor, 'wb') as handle:
            handle.write(seed)
        os.chmod(path, mode)
        return seed, path

    def start_signer(self, arguments, expect_ready=True, name='treasury-signer.sock'):
        target = self.work / name
        log = (self.work / (name + '.log')).open('wb')
        self.addCleanup(log.close)
        process = subprocess.Popen(
            [sys.executable, str(SIGNER_DIR / 'signer.py'), '--socket', str(target)] + arguments,
            stdout=subprocess.PIPE, stderr=log, cwd=str(ROOT))
        self.processes.append(process)
        if not expect_ready:
            self.assertNotEqual(process.wait(timeout=30), 0)
            return target, process
        deadline = time.monotonic() + 30
        while not target.is_socket():
            self.assertIsNone(process.poll(),
                              (self.work / (name + '.log')).read_text())
            self.assertLess(time.monotonic(), deadline, 'the signer socket did not appear')
            time.sleep(0.05)
        return target, process

    def file_signer(self, mode=0o600, extra=()):
        seed, path = self.seed_file(mode=mode)
        target, process = self.start_signer(
            ['--allowed-uid', str(os.getuid()), '--provider', 'file', '--key-file', str(path)]
            + list(extra))
        return seed, path, target, process

    def provider_program(self, seed, mode):
        program = self.work / f'provider-{mode}.py'
        program.write_text(PROVIDER_PROGRAM)
        os.chmod(program, 0o700)
        material = self.work / f'provider-{mode}.hex'
        material.write_text(seed.hex())
        os.chmod(material, 0o600)
        counter = self.work / f'provider-{mode}.count'
        return f'{sys.executable} {program} {material} {mode} {counter}'

    def test_socket_answers_signatures_for_the_treasury_identity(self):
        seed, path, target, _ = self.file_signer()
        self.assertEqual(target.stat().st_mode & 0o777, 0o600)
        client = SignerClient(str(target))
        public_key = public_key_of(seed)
        self.assertEqual(client.public_key(), public_key)
        self.assertEqual(client.did(), 'did:layerx:' + public_key.hex())
        digest = os.urandom(32)
        signature = client.sign(digest)
        self.assertEqual(len(signature), 64)
        self.assertTrue(verify(public_key, digest, signature))
        self.assertEqual(client.sign(digest), signature)
        other = os.urandom(32)
        self.assertNotEqual(client.sign(other), signature)
        self.assertTrue(verify(public_key, other, client.sign(other)))
        self.assertFalse(verify(public_key, other, signature))
        self.assertEqual(path.read_bytes(), seed)
        self.assertEqual(
            [entry.name for entry in self.work.iterdir() if entry.suffix == '.hex'], [])

    def test_hexadecimal_material_is_accepted(self):
        seed = os.urandom(32)
        path = self.work / 'treasury.hex'
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'w') as handle:
            handle.write(seed.hex() + '\n')
        target, _ = self.start_signer(
            ['--allowed-uid', str(os.getuid()), '--provider', 'file', '--key-file', str(path)])
        self.assertEqual(SignerClient(str(target)).public_key(), public_key_of(seed))

    def test_unsafe_material_is_refused(self):
        _, readable = self.seed_file(name='readable.key', mode=0o644)
        _, group_writable = self.seed_file(name='group-writable.key', mode=0o620)
        short = self.work / 'short.key'
        descriptor = os.open(short, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as handle:
            handle.write(b'\0' * 31)
        _, target_seed = self.seed_file(name='linked.key')
        link = self.work / 'link.key'
        link.symlink_to(target_seed)
        directory = self.work / 'material'
        directory.mkdir(mode=0o700)
        for index, candidate in enumerate([readable, group_writable, short, link, directory,
                                           self.work / 'missing.key']):
            with self.subTest(material=candidate.name):
                self.start_signer(
                    ['--allowed-uid', str(os.getuid()), '--provider', 'file',
                     '--key-file', str(candidate)],
                    expect_ready=False, name=f'refused-{index}.sock')
                self.assertFalse((self.work / f'refused-{index}.sock').exists())

    def test_provider_selection_is_exclusive(self):
        _, path = self.seed_file()
        self.start_signer(['--allowed-uid', str(os.getuid()), '--provider', 'command',
                           '--key-file', str(path)], expect_ready=False, name='mixed.sock')
        self.start_signer(['--allowed-uid', str(os.getuid()), '--provider', 'file'],
                          expect_ready=False, name='empty.sock')

    def test_requests_outside_the_contract_are_refused(self):
        _, _, target, _ = self.file_signer()
        self.assertEqual(raw_request(target, b'status\n')['error']['code'], 'unknown_request')
        self.assertEqual(raw_request(target, b'sign \n')['error']['code'], 'unknown_request')
        self.assertEqual(raw_request(target, b'sign ' + b'0' * 63 + b'\n')['error']['code'],
                         'digest_refused')
        self.assertEqual(raw_request(target, b'sign ' + b'A' * 64 + b'\n')['error']['code'],
                         'digest_refused')
        self.assertEqual(raw_request(target, b'sign ' + b'0' * 4096 + b'\n')['error']['code'],
                         'unknown_request')
        with self.assertRaises(SignerError):
            SignerClient(str(target)).sign(os.urandom(31))

    def test_command_provider_signs_and_is_verified(self):
        seed = os.urandom(32)
        target, _ = self.start_signer(
            ['--allowed-uid', str(os.getuid()), '--provider', 'command',
             '--provider-command', self.provider_program(seed, 'honest')],
            name='command.sock')
        client = SignerClient(str(target))
        public_key = public_key_of(seed)
        self.assertEqual(client.public_key(), public_key)
        self.assertEqual(raw_request(target, b'public-key\n')['provider'], 'command')
        digest = os.urandom(32)
        signature = client.sign(digest)
        self.assertTrue(verify(public_key, digest, signature))

    def test_command_provider_forgery_is_refused(self):
        seed = os.urandom(32)
        target, _ = self.start_signer(
            ['--allowed-uid', str(os.getuid()), '--provider', 'command',
             '--provider-command', self.provider_program(seed, 'forging')],
            name='forging.sock')
        self.assertEqual(SignerClient(str(target)).public_key(), public_key_of(seed))
        self.assertEqual(raw_request(target, b'sign ' + os.urandom(32).hex().encode() + b'\n')
                         ['error']['code'], 'signature_refused')

    def test_command_provider_key_rotation_is_refused(self):
        seed = os.urandom(32)
        target, _ = self.start_signer(
            ['--allowed-uid', str(os.getuid()), '--provider', 'command',
             '--provider-command', self.provider_program(seed, 'rotating')],
            name='rotating.sock')
        self.assertEqual(raw_request(target, b'sign ' + os.urandom(32).hex().encode() + b'\n')
                         ['error']['code'], 'signature_refused')

    def test_peer_credentials_are_enforced(self):
        if os.getuid() != 0:
            self.skipTest('peer credential admission needs a second uid')
        other = 65534
        seed, path, target, _ = self.file_signer(extra=['--socket-group', str(other)])
        self.assertEqual(target.stat().st_mode & 0o777, 0o660)
        self.assertEqual(target.stat().st_gid, other)
        self.assertEqual(SignerClient(str(target)).public_key(), public_key_of(seed))
        client_program = self.work / 'client.py'
        shutil.copyfile(SIGNER_DIR / 'client.py', client_program)
        os.chmod(client_program, 0o755)
        refused = subprocess.run(
            ['setpriv', '--reuid', str(other), '--regid', str(other), '--clear-groups',
             sys.executable, str(client_program), '--socket', str(target), 'public-key'],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=str(self.work))
        self.assertNotEqual(refused.returncode, 0)
        self.assertIn(b'peer_refused', refused.stderr)
        admitted_seed, admitted_path = self.seed_file(name='admitted.key')
        admitted_socket, _ = self.start_signer(
            ['--allowed-uid', f'{os.getuid()},{other}', '--provider', 'file',
             '--key-file', str(admitted_path), '--socket-group', str(other)],
            name='admitted.sock')
        accepted = subprocess.run(
            ['setpriv', '--reuid', str(other), '--regid', str(other), '--clear-groups',
             sys.executable, str(client_program), '--socket', str(admitted_socket),
             'public-key'],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=str(self.work))
        self.assertEqual(accepted.returncode, 0, accepted.stderr)
        self.assertEqual(accepted.stdout.decode().strip(), public_key_of(admitted_seed).hex())

    def test_concurrent_signatures(self):
        seed, _, target, _ = self.file_signer()
        public_key = public_key_of(seed)
        digests = [os.urandom(32) for _ in range(16)]
        signatures = [None] * len(digests)

        def sign(index):
            signatures[index] = SignerClient(str(target)).sign(digests[index])

        workers = [threading.Thread(target=sign, args=(index,)) for index in range(len(digests))]
        for worker in workers:
            worker.start()
        for worker in workers:
            worker.join(60)
        for index, signature in enumerate(signatures):
            self.assertIsNotNone(signature)
            self.assertTrue(verify(public_key, digests[index], signature))

    def test_public_key_file_is_published(self):
        published = self.work / 'treasury-public-key'
        seed, _, _, _ = self.file_signer(extra=['--public-key-file', str(published)])
        self.assertEqual(published.read_text().strip(), public_key_of(seed).hex())
        self.assertEqual(published.stat().st_mode & 0o777, 0o644)


class BootstrapTreasuryCase(unittest.TestCase):
    def setUp(self):
        for binary in ('layerxd', 'layerx-genesis-build'):
            if not (BIN / binary).is_file():
                self.skipTest(f'{BIN / binary} is not built')
        self.directory = tempfile.TemporaryDirectory(prefix='lx-signer-bootstrap-')
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        os.chmod(self.work, 0o755)
        self.signer = None
        self.addCleanup(self.stop_signer)

    def stop_signer(self):
        if self.signer is not None and self.signer.poll() is None:
            self.signer.terminate()
            try:
                self.signer.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.signer.kill()
                self.signer.wait(timeout=10)

    def seeds(self):
        seeds = {}
        for name in ('sequencer', 'treasury'):
            path = self.work / (name + '.key')
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            seed = os.urandom(32)
            with os.fdopen(descriptor, 'wb') as handle:
                handle.write(seed)
            seeds[name] = (seed, path)
        return seeds

    def settlement_document(self):
        path = ROOT / 'contracts/config/checkpoint-settlement.json'
        threshold = json.loads(path.read_text())['finality_policy']['certificate_threshold']
        self.assertGreater(threshold, 1, 'the shipped settlement document must carry a '
                                         'multi-guarantor certificate threshold')
        return path

    def bootstrap(self, extra):
        treasury_public = public_key_of(self.treasury_seed)
        (self.work / 'metadata').write_bytes(metadata(ASSET, treasury_public, os.urandom(32)))
        holders, ports = [], []
        for _ in range(3):
            holder = socket.socket()
            holder.bind(('127.0.0.1', 0))
            holders.append(holder)
            ports.append(holder.getsockname()[1])
        self.assertNotIn(18545, ports)
        self.assertNotIn(6379, ports)
        environment = dict(
            {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_')},
            LAYERX_NODE_PAXEER_CHAIN_ID='31337',
            LAYERX_NODE_SETTLEMENT_CONTRACT='0x' + '1' * 40,
            LAYERX_NODE_CHECKPOINT_REGISTRY='0x' + '2' * 40,
            LAYERX_NODE_PAXEER_RPC_ADDRESS='127.0.0.1',
            LAYERX_NODE_PAXEER_RPC_PORT=str(ports[2]))
        command = [
            'bash', str(ROOT / 'platform/hosted/node/bootstrap.sh'),
            '--data-dir', str(self.work / 'data'), '--run-dir', str(self.work / 'run'),
            '--network-id', '77',
            '--sequencer-key', str(self.sequencer_path),
            '--genesis-metadata', str(self.work / 'metadata'),
            '--settlement-document', str(self.settlement_document()),
            '--lni-uid', '4021', '--lni-gid', '4021',
            '--program-port', str(ports[0]), '--replica-port', str(ports[1]),
            '--layerxd', str(BIN / 'layerxd'),
            '--genesis-build', str(BIN / 'layerx-genesis-build'),
        ] + extra
        try:
            return subprocess.run(command, cwd=str(ROOT), env=environment,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        finally:
            for holder in holders:
                holder.close()

    def prepare(self):
        seeds = self.seeds()
        self.sequencer_seed, self.sequencer_path = seeds['sequencer']
        self.treasury_seed, self.treasury_path = seeds['treasury']

    def exports(self):
        return dict(line.split('=', 1)
                    for line in (self.work / 'data/node.env').read_text().splitlines())

    def assert_no_seed_copy(self):
        data = self.work / 'data'
        self.assertFalse((data / 'secrets/treasury-key.hex').exists())
        for entry in data.rglob('*'):
            if entry.is_file():
                self.assertNotIn(self.treasury_seed, entry.read_bytes(), str(entry))
                self.assertNotIn(self.treasury_seed.hex().encode(), entry.read_bytes(), str(entry))

    def test_bootstrap_binds_the_treasury_through_the_signer(self):
        self.prepare()
        socket_path = self.work / 'treasury-signer.sock'
        self.signer = subprocess.Popen(
            [sys.executable, str(SIGNER_DIR / 'signer.py'), '--socket', str(socket_path),
             '--allowed-uid', str(os.getuid()), '--provider', 'file',
             '--key-file', str(self.treasury_path)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, cwd=str(ROOT))
        deadline = time.monotonic() + 30
        while not socket_path.is_socket():
            self.assertIsNone(self.signer.poll(), 'the treasury signer exited')
            self.assertLess(time.monotonic(), deadline, 'the signer socket did not appear')
            time.sleep(0.05)
        result = self.bootstrap(['--treasury-signer-socket', str(socket_path)])
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        exports = self.exports()
        treasury_public = public_key_of(self.treasury_seed)
        self.assertEqual(exports['LAYERX_NODE_TREASURY_PUBLIC_KEY'], treasury_public.hex())
        self.assertEqual(exports['LAYERX_NODE_TREASURY_DID'],
                         'did:layerx:' + treasury_public.hex())
        self.assertEqual(exports['LAYERX_NODE_TREASURY_SIGNER_SOCKET'], str(socket_path))
        self.assertNotIn('LAYERX_NODE_TREASURY_KEY_FILE', exports)
        self.assertEqual((self.work / 'data/node.env').stat().st_mode & 0o777, 0o600)
        identities = (self.work / 'data/identities.txt').read_text().splitlines()
        did = ('did:layerx:' + treasury_public.hex()).encode().hex()
        self.assertEqual(identities[0], f'{did}:{treasury_public.hex()}:0')
        core = dict(line.split('=', 1)
                    for line in (self.work / 'run/core.env').read_text().splitlines())
        self.assertEqual(core['LAYERX_CORE_TREASURY_SIGNER_SOCKET'], str(socket_path))
        self.assert_no_seed_copy()

    def test_bootstrap_with_a_local_seed_keeps_no_copy(self):
        self.prepare()
        result = self.bootstrap(['--treasury-key', str(self.treasury_path)])
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        exports = self.exports()
        self.assertEqual(exports['LAYERX_NODE_TREASURY_PUBLIC_KEY'],
                         public_key_of(self.treasury_seed).hex())
        self.assertNotIn('LAYERX_NODE_TREASURY_KEY_FILE', exports)
        self.assertNotIn('LAYERX_NODE_TREASURY_SIGNER_SOCKET', exports)
        self.assertEqual((self.work / 'data/node.env').stat().st_mode & 0o777, 0o600)
        core = (self.work / 'run/core.env').read_text()
        self.assertNotIn('LAYERX_CORE_TREASURY_SIGNER_SOCKET', core)
        self.assert_no_seed_copy()

    def test_bootstrap_refuses_conflicting_and_missing_treasury_sources(self):
        self.prepare()
        both = self.bootstrap(['--treasury-key', str(self.treasury_path),
                               '--treasury-signer-socket', '/tmp/absent-treasury.sock'])
        self.assertNotEqual(both.returncode, 0)
        self.assertIn(b'exclusive', both.stderr)
        neither = self.bootstrap([])
        self.assertNotEqual(neither.returncode, 0)
        self.assertIn(b'--treasury-key or --treasury-signer-socket is required', neither.stderr)
        absent = self.bootstrap(['--treasury-signer-socket', str(self.work / 'absent.sock')])
        self.assertNotEqual(absent.returncode, 0)
        self.assertIn(b'treasury signer socket is not available', absent.stderr)
        relative = self.bootstrap(['--treasury-signer-socket', 'treasury.sock'])
        self.assertNotEqual(relative.returncode, 0)
        self.assertIn(b'must be an absolute path', relative.stderr)


if __name__ == '__main__':
    unittest.main()
