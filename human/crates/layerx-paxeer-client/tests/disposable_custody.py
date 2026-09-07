import contextlib
import hashlib
import http.server
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from types import SimpleNamespace
import urllib.request

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'tests/bridge'))
from custody_credit import Rpc, create_profile, eth_hash, unhex, write_new
from deploy_local_custody import command, disposable_rpc, signer
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


def port():
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        return reservation.getsockname()[1]


@contextlib.contextmanager
def chain(directory, extra=()):
    url = 'http://127.0.0.1:' + str(port())
    process = subprocess.Popen(['anvil', '--silent', '--mnemonic-random', '12', '--chain-id', '125',
                                '--port', url.rsplit(':', 1)[1], *extra],
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        rpc = Rpc(url)
        for _ in range(200):
            if process.poll() is not None:
                raise RuntimeError('Anvil exited')
            try:
                rpc.call('eth_chainId', [])
                break
            except (OSError, ValueError):
                time.sleep(.05)
        else:
            raise RuntimeError('Anvil readiness deadline')
        yield rpc
    finally:
        process.terminate()
        process.wait(timeout=10)


@contextlib.contextmanager
def boundary(directory, rpc):
    command('openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
            '-subj', '/CN=Custody test CA', '-keyout', str(directory / 'ca.key'),
            '-out', str(directory / 'ca.pem'))
    command('openssl', 'req', '-newkey', 'rsa:2048', '-nodes', '-subj', '/CN=localhost',
            '-keyout', str(directory / 'server.key'), '-out', str(directory / 'server.csr'))
    (directory / 'extensions').write_text('subjectAltName=DNS:localhost\nextendedKeyUsage=serverAuth\n')
    command('openssl', 'x509', '-req', '-in', str(directory / 'server.csr'), '-CA',
            str(directory / 'ca.pem'), '-CAkey', str(directory / 'ca.key'), '-CAcreateserial',
            '-days', '1', '-extfile', str(directory / 'extensions'), '-out', str(directory / 'server.pem'))
    refused = []

    class Proxy(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = self.rfile.read(int(self.headers['Content-Length']))
            calls = json.loads(body)
            for call in calls if isinstance(calls, list) else [calls]:
                method = call['method']
                if method.startswith(('anvil_', 'evm_')) or method in ('eth_sendTransaction', 'eth_accounts'):
                    refused.append(method)
                    self.send_error(403)
                    return
            request = urllib.request.Request(rpc.url, body, {'Content-Type': 'application/json'})
            with urllib.request.urlopen(request, timeout=30) as upstream:
                response = upstream.read()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(response)))
            self.end_headers()
            self.wfile.write(response)

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Proxy)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(directory / 'server.pem', directory / 'server.key')
    server.socket = context.wrap_socket(server.socket, server_side=True)
    worker = threading.Thread(target=server.serve_forever)
    worker.start()
    try:
        yield 'https://localhost:' + str(server.server_port), refused
    finally:
        server.shutdown()
        worker.join()
        server.server_close()


class DisposableCustody(unittest.TestCase):
    def test_real_signed_ca_verified_deployment_and_identity_refusals(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'qual-logs') as temporary:
            directory = Path(temporary)
            with chain(directory) as rpc, boundary(directory, rpc) as (url, refused):
                private = ec.generate_private_key(ec.SECP256K1())
                write_new(directory / 'signer.key',
                          ('0x' + private.private_numbers().private_value.to_bytes(32, 'big').hex()).encode())
                public = private.public_key().public_bytes(Encoding.X962, PublicFormat.UncompressedPoint)
                address = '0x' + eth_hash(public[1:])[-20:].hex()
                rpc.call('eth_sendTransaction', [{'from': rpc.call('eth_accounts', [])[0],
                                                  'to': address, 'value': hex(10 ** 21)}])
                genesis = rpc.call('eth_getBlockByNumber', ['0x0', False])['hash']
                with chain(directory) as other:
                    denied = other.call('eth_getBlockByNumber', ['0x0', False])['hash']
                self.assertNotEqual(genesis, denied)
                identity = {'chain_id': 125, 'genesis_hash': genesis, 'persistent_genesis_hash': denied,
                            'rpc_origins': [url], 'ca_sha256': '0x' + hashlib.sha256(
                                (directory / 'ca.pem').read_bytes()).hexdigest()}
                identity_path = directory / 'disposable.json'
                identity_path.write_text(json.dumps(identity))
                ca = str(directory / 'ca.pem')
                verified = disposable_rpc(url, ca, identity_path)
                self.assertEqual(signer(verified, directory / 'signer.key'), address)
                with self.assertRaises(ValueError):
                    disposable_rpc('https://127.0.0.1:18545', ca, identity_path)
                with self.assertRaises(ValueError):
                    disposable_rpc('https://localhost:19443', ca, identity_path)
                with self.assertRaises(OSError):
                    Rpc(url).call('eth_chainId', [])
                for field, value in [('genesis_hash', denied), ('chain_id', 31337),
                                     ('ca_sha256', '0x' + '00' * 32), ('rpc_origins', ['https://localhost:1'])]:
                    identity_path.write_text(json.dumps({**identity, field: value}))
                    with self.assertRaises(ValueError):
                        disposable_rpc(url, ca, identity_path)
                identity_path.write_text(json.dumps(identity))
                os.chmod(directory / 'signer.key', 0o644)
                with self.assertRaises(ValueError):
                    signer(verified, directory / 'signer.key')
                os.chmod(directory / 'signer.key', 0o600)
                actor = ed25519.Ed25519PrivateKey.generate()
                actor_public = actor.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
                name = ('agent:did:layerx:' + actor_public.hex() + ':main').encode()
                beneficiary = '0x' + hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hexdigest()
                output = directory / 'custody.json'
                command('python3', str(ROOT / 'tests/bridge/deploy_local_custody.py'),
                        '--allow-local-chain', '--rpc', url, '--ca-bundle', ca,
                        '--disposable-identity', str(identity_path), '--key-file', str(directory / 'signer.key'),
                        '--asset', '0x' + hashlib.sha256(b'test asset').hexdigest(),
                        '--beneficiary', beneficiary,
                        '--amount', '1000000000000000000', '--output', str(output))
                deployed = json.loads(output.read_text())
                self.assertEqual(deployed['chain_id'], 125)
                self.assertEqual(refused, [])
                code = unhex(rpc.call('eth_getCode', [deployed['vault'], 'latest']))
                self.assertEqual(deployed['runtime_sha256'], '0x' + hashlib.sha256(code).hexdigest())
                self.assertEqual(int(rpc.call('eth_getBalance', [deployed['token'], 'latest']), 16), 10 ** 18)

                rpc.call('anvil_mine', ['0x80'], allow_missing=True)
                fork_height = int(rpc.call('eth_blockNumber', []), 16)
                observer_dir = directory / 'observer'
                observer_dir.mkdir()
                with chain(directory, ['--fork-url', rpc.url, '--fork-block-number', str(fork_height)]) as observer:
                    with boundary(observer_dir, observer) as (observer_url, observer_refused):
                        bundle = directory / 'bundle.pem'
                        bundle.write_bytes((directory / 'ca.pem').read_bytes()
                                           + (observer_dir / 'ca.pem').read_bytes())
                        identity['rpc_origins'].append(observer_url)
                        identity['ca_sha256'] = '0x' + hashlib.sha256(bundle.read_bytes()).hexdigest()
                        identity_path.write_text(json.dumps(identity))
                        disposable_rpc(observer_url, str(bundle), identity_path)
                        authority = ed25519.Ed25519PrivateKey.generate()
                        write_new(directory / 'attestor.key', authority.private_bytes_raw())
                        profile = directory / 'custody.profile'
                        previous_ca = os.environ.get('SSL_CERT_FILE')
                        os.environ['SSL_CERT_FILE'] = str(bundle)
                        try:
                            create_profile(SimpleNamespace(rpc=[url, observer_url], chain_id=125, network_id=402,
                                           vault=deployed['vault'], runtime_sha256=deployed['runtime_sha256'],
                                           asset=deployed['asset'], confirmations=2,
                                           attestor_key=str(directory / 'attestor.key'), output=str(profile)))
                        finally:
                            if previous_ca is None:
                                del os.environ['SSL_CERT_FILE']
                            else:
                                os.environ['SSL_CERT_FILE'] = previous_ca
                        builder = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', '/root/lx-lanes/native-bin')) / 'layerx-genesis-build'
                        invocation = ['python3', str(ROOT / 'tests/bridge/custody_genesis.py'),
                                      '--profile', str(profile), '--builder', str(builder)]
                        with self.assertRaises(ValueError):
                            command(*invocation, '--output', str(directory / 'refused-genesis'))
                        self.assertFalse((directory / 'refused-genesis').exists())
                        command(*invocation, '--output', str(directory / 'genesis'), '--rpc', url,
                                '--ca-bundle', str(bundle), '--disposable-identity', str(identity_path))
                        self.assertTrue((directory / 'genesis/artifacts/genesis.manifest').is_file())

                        evidence_args = ['python3', str(ROOT / 'tests/bridge/local_credit_evidence.py'),
                                         '--rpc', url, '--rpc', observer_url, '--ca-bundle', str(bundle),
                                         '--disposable-identity', str(identity_path), '--custody', str(output),
                                         '--asset', deployed['asset'], '--attestor-key', str(directory / 'attestor.key'),
                                         '--attestor-public-key', '0x' + authority.public_key().public_bytes(
                                             Encoding.Raw, PublicFormat.Raw).hex(),
                                         '--beneficiary-key', '0x' + actor_public.hex(),
                                         '--network-id', '402', '--confirmations', '2']
                        command(*evidence_args, '--output', str(directory / 'evidence'))
                        produced_profile = (directory / 'evidence/custody.profile').read_bytes()
                        self.assertEqual(produced_profile, profile.read_bytes())
                        credit = (directory / 'evidence/custody.credit').read_bytes()
                        self.assertEqual(len(credit), 427)
                        self.assertEqual(credit[37:41], (402).to_bytes(4, 'big'))
                        authority.public_key().verify(credit[363:], b'LX:CUSTODY:CREDIT:v1' + credit[:363])
                        for option, value in [('--asset', '0x' + '00' * 32),
                                              ('--attestor-public-key', '0x' + '00' * 32),
                                              ('--beneficiary-key', '0x' + '00' * 32),
                                              ('--rpc', url)]:
                            invalid = evidence_args.copy()
                            selected = len(invalid) - 1 - invalid[::-1].index(option)
                            invalid[selected + 1] = value
                            rejected = directory / ('rejected-' + option[2:])
                            with self.assertRaises(ValueError):
                                command(*invalid, '--output', str(rejected))
                            self.assertFalse((rejected / 'custody.credit').exists())
                        self.assertEqual(refused, [])
                        self.assertEqual(observer_refused, [])


if __name__ == '__main__':
    unittest.main()
