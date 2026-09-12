import base64
import contextlib
import hashlib
import http.server
import json
import os
import re
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
sys.path.insert(0, str(ROOT / 'tests/daemon'))
from custody_chain import boundaries, owned_chain
from custody_credit import Rpc, create_profile, eth_hash, unhex, write_new
sys.path.insert(0, str(ROOT / 'tests/support'))
from lxgb_metadata import metadata
from deploy_local_custody import (command, disposable_rpc, signer, genesis_document,
                                  PERSISTENT_GENESIS, PERSISTENT_BLUEPRINT)
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


PERSISTENT_COMET_CHAIN_ID = "hyperpax_125-1"

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
def comet_chain(directory, blueprint_code=False):
    home = directory / "paxd"
    binary = os.environ.get("PAXD", "paxd")
    chain_id = "custody-disposable-1"
    command(binary, "init", "custody", "--chain-id", chain_id, "--home", str(home))
    command(binary, "keys", "add", "validator", "--keyring-backend", "test", "--home", str(home))
    command(binary, "add-genesis-account", "validator", "100000000000000000000uhpx",
            "--keyring-backend", "test", "--home", str(home))
    command(binary, "gentx", "validator", "7000000000000000uhpx", "--chain-id", chain_id,
            "--keyring-backend", "test", "--home", str(home), "--ip", "127.0.0.1")
    genesis_path = home / "config/genesis.json"
    genesis = json.loads(genesis_path.read_bytes())
    validator = json.loads((home / "config/priv_validator_key.json").read_bytes())["pub_key"]
    genesis["validators"] = [{"power": "7000000000", "pub_key": validator}]
    genesis["app_state"]["staking"]["params"]["max_voting_power_ratio"] = "1.000000000000000000"
    if blueprint_code:
        runtime = unhex(command("forge", "inspect", "--contracts", "platform/hosted/paxeer/contracts",
                               "platform/hosted/paxeer/contracts/BetaUsdl.sol:BetaUsdl", "deployedBytecode"))
        genesis["app_state"]["evm"]["codes"] = [{"address": PERSISTENT_BLUEPRINT,
                                                  "code": base64.b64encode(runtime).decode()}]
    genesis_path.write_text(json.dumps(genesis))
    command(binary, "collect-gentxs", "--home", str(home))
    comet_url = "http://127.0.0.1:" + str(port())
    evm_port = port()
    app = home / "config/app.toml"
    config = app.read_text()
    for key, value in [("http_enabled", "true"), ("http_address", '"127.0.0.1"'),
                       ("http_port", str(evm_port)), ("ws_enabled", "false")]:
        config = re.sub(r"^" + key + r" = .*", key + " = " + value, config, flags=re.MULTILINE)
    app.write_text(config)
    with (directory / "paxd.log").open("wb") as log:
        process = subprocess.Popen([binary, "start", "--home", str(home), "--mode", "validator",
                                    "--rpc.laddr", comet_url.replace("http:", "tcp:"),
                                    "--p2p.laddr", "tcp://127.0.0.1:" + str(port()),
                                    "--p2p.pex=false", "--grpc.enable=false", "--grpc-web.enable=false",
                                    "--rpc.pprof-laddr", "", "--concurrency-workers", "4"],
                                   stdout=log, stderr=log)
    try:
        rpc = Rpc("http://127.0.0.1:" + str(evm_port))
        for _ in range(300):
            if process.poll() is not None:
                raise RuntimeError("disposable paxd exited; inspect paxd.log")
            try:
                rpc.call("eth_chainId", [])
                with urllib.request.urlopen(comet_url + "/genesis_chunked?chunk=0", timeout=2):
                    pass
                break
            except (OSError, ValueError):
                time.sleep(.2)
        else:
            raise RuntimeError("disposable paxd readiness deadline")
        yield rpc, comet_url, genesis_path
    finally:
        process.terminate()
        process.wait(timeout=30)


@contextlib.contextmanager
def boundary(directory, rpc, genesis_path, comet_url):
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

        def do_GET(self):
            if self.path == "/genesis.json":
                response = genesis_path.read_bytes()
            elif self.path.startswith("/genesis_chunked?chunk="):
                with urllib.request.urlopen(comet_url + self.path, timeout=20) as upstream:
                    response = upstream.read()
            else:
                self.send_error(404)
                return
            self.send_response(200)
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

        def do_POST(self):
            body = self.rfile.read(int(self.headers['Content-Length']))
            calls = json.loads(body)
            for call in calls if isinstance(calls, list) else [calls]:
                method = call['method']
                if (method.startswith(('anvil_', 'evm_')) or method in ('eth_sendTransaction', 'eth_accounts')
                        or (method == 'eth_getBlockByNumber' and call['params'][0] in ('0x0', 'earliest'))):
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
    @classmethod
    def setUpClass(cls):
        (ROOT / 'qual-logs').mkdir(exist_ok=True)
        target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / '.lane-target'))
        cls.boundary_binary = Path(os.environ.get('LAYERX_PAXEER_BOUNDARY_BIN',
                                                  target / 'debug/layerx-paxeer-boundary'))
        cls.proof_binary = Path(os.environ.get('LAYERX_CUSTODY_PROOF_BIN',
                                               ROOT / 'build/bin/layerx-custody-proof'))
        cls.builder = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin')) / 'layerx-genesis-build'
        for binary in (cls.boundary_binary, cls.proof_binary, cls.builder):
            if not binary.is_file() or not os.access(binary, os.X_OK):
                raise RuntimeError(f'real custody prerequisite is missing: {binary}')
        threads = min(4, int(os.environ.get('CARGO_BUILD_JOBS', '4')),
                      int(os.environ.get('RAYON_NUM_THREADS', '4')))
        if threads <= 0:
            raise ValueError('positive contract build concurrency required')
        command('forge', 'build', 'platform/hosted/paxeer/contracts/BetaUsdl.sol',
                'contracts/governance/LayerXBetaTimelock.sol', 'contracts/custody/AssetRegistry.sol',
                'contracts/custody/LayerXVault.sol',
                'paxeer-network/loadtest/contracts/evm/lib/solmate/src/tokens/WETH.sol',
                '--threads', str(threads))
        cls.artifacts = ROOT / 'build/forge-artifacts'
        cls.vault_artifact = cls.artifacts / 'LayerXVault.sol/LayerXVault.json'

    def test_real_paxeer_genesis_code_at_denied_blueprint_address(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'qual-logs') as temporary:
            directory = Path(temporary)
            with comet_chain(directory, blueprint_code=True) as (rpc, comet_url, genesis_path), \
                    boundary(directory, rpc, genesis_path, comet_url) as (url, refused):
                identity = {'chain_id': int(rpc.call('eth_chainId', []), 16),
                            'genesis_sha256': '0x' + hashlib.sha256(genesis_path.read_bytes()).hexdigest(),
                            'comet_chain_id': json.loads(genesis_path.read_bytes())['chain_id'],
                            'genesis_source': 'published', 'rpc_origins': [url],
                            'ca_sha256': '0x' + hashlib.sha256((directory / 'ca.pem').read_bytes()).hexdigest()}
                identity_path = directory / 'identity.json'
                identity_path.write_text(json.dumps(identity))
                self.assertEqual(int(rpc.call('eth_call', [{'to': PERSISTENT_BLUEPRINT,
                                                          'data': '0x313ce567'}, 'latest']), 16), 6)
                with self.assertRaisesRegex(ValueError, 'persistent blueprint code refused'):
                    disposable_rpc(url, str(directory / 'ca.pem'), identity_path)
                self.assertEqual(refused, [])

    def test_synthetic_evm_and_unrelated_comet_association_is_refused(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'qual-logs') as temporary:
            directory = Path(temporary)
            with comet_chain(directory) as (paxeer, comet_url, genesis_path), chain(directory) as rpc, \
                    boundary(directory, rpc, genesis_path, comet_url) as (url, refused):
                private = ec.generate_private_key(ec.SECP256K1())
                write_new(directory / 'signer.key',
                          ('0x' + private.private_numbers().private_value.to_bytes(32, 'big').hex()).encode())
                public = private.public_key().public_bytes(Encoding.X962, PublicFormat.UncompressedPoint)
                address = '0x' + eth_hash(public[1:])[-20:].hex()
                rpc.call('eth_sendTransaction', [{'from': rpc.call('eth_accounts', [])[0],
                                                  'to': address, 'value': hex(10 ** 21)}])
                genesis = '0x' + hashlib.sha256(genesis_path.read_bytes()).hexdigest()
                denied = '0x' + PERSISTENT_GENESIS.hex()
                self.assertNotEqual(genesis, denied)
                identity = {'chain_id': 125, 'genesis_sha256': genesis,
                            'comet_chain_id': json.loads(genesis_path.read_bytes())['chain_id'],
                            'genesis_source': 'published',
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
                for field, value in [('genesis_sha256', denied), ('chain_id', 31337),
                                     ('comet_chain_id', PERSISTENT_COMET_CHAIN_ID),
                                     ('comet_chain_id', 'wrong-chain'),
                                     ('genesis_sha256', '0x' + 'ab' * 32),
                                     ('ca_sha256', '0x' + '00' * 32), ('rpc_origins', ['https://localhost:1'])]:
                    identity_path.write_text(json.dumps({**identity, field: value}))
                    with self.assertRaises(ValueError):
                        disposable_rpc(url, ca, identity_path)
                identity_path.write_text(json.dumps(identity))
                paxeer_dir = directory / 'paxeer-boundary'
                paxeer_dir.mkdir()
                with boundary(paxeer_dir, paxeer, genesis_path, comet_url) as (paxeer_url, paxeer_refused):
                    paxeer_identity = {**identity, 'chain_id': int(paxeer.call('eth_chainId', []), 16),
                                       'rpc_origins': [paxeer_url], 'ca_sha256': '0x' + hashlib.sha256(
                                           (paxeer_dir / 'ca.pem').read_bytes()).hexdigest()}
                    paxeer_identity_path = paxeer_dir / 'identity.json'
                    paxeer_identity_path.write_text(json.dumps(paxeer_identity))
                    verified_paxeer = disposable_rpc(paxeer_url, str(paxeer_dir / 'ca.pem'), paxeer_identity_path)
                    self.assertEqual(verified_paxeer.genesis_sha256, unhex(genesis, 32))
                    chunked = genesis_document(verified_paxeer, 'chunked')
                    paxeer_identity.update(genesis_source='chunked',
                                           genesis_sha256='0x' + hashlib.sha256(chunked).hexdigest())
                    paxeer_identity_path.write_text(json.dumps(paxeer_identity))
                    self.assertEqual(disposable_rpc(paxeer_url, str(paxeer_dir / 'ca.pem'),
                                                    paxeer_identity_path).genesis_sha256,
                                     hashlib.sha256(chunked).digest())
                    self.assertEqual(paxeer_refused, [])
                os.chmod(directory / 'signer.key', 0o644)
                with self.assertRaises(ValueError):
                    signer(verified, directory / 'signer.key')
                os.chmod(directory / 'signer.key', 0o600)
                self.assertNotEqual(int(paxeer.call('eth_chainId', []), 16), 125)
                authority = ed25519.Ed25519PrivateKey.generate()
                authority_path = directory / 'attestor.key'
                write_new(authority_path, authority.private_bytes_raw())
                request = dict(operation='status', expected=dict(
                    genesis_sha256=genesis, comet_chain_id=identity['comet_chain_id'],
                    chain_id=125, vault='', runtime_sha256='', confirmations=0),
                    bundle=dict(version='paxeer-custody-state-v2',
                        genesis=base64.b64encode(genesis_path.read_bytes()).decode(),
                        history=[], state_height=0, finalized_height=0, state=[]))
                history = directory / 'rejected-history'
                result = subprocess.run([str(self.proof_binary), '--history-state', str(history),
                    '--attestor-key', str(authority_path)], input=json.dumps(request).encode(),
                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30, check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(b'genesis identity bounds', result.stderr)
                self.assertEqual(result.stdout, b'')
                self.assertFalse(history.exists())
                self.assertEqual(refused, [])

    def test_real_signed_ca_verified_deployment_and_identity_refusals(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'qual-logs') as temporary:
            directory = Path(temporary)
            with owned_chain(directory, self.artifacts) as chain:
                with boundaries(directory, chain, self.boundary_binary) as (origins, ca_path, identity_path):
                    url, observer_url = origins
                    ca = str(ca_path)
                    identity = json.loads(identity_path.read_bytes())
                    genesis = identity['genesis_sha256']
                    denied = '0x' + PERSISTENT_GENESIS.hex()
                    self.assertNotEqual(genesis, denied)
                    self.assertEqual(identity['chain_id'], 125)
                    self.assertEqual(identity['comet_chain_id'], PERSISTENT_COMET_CHAIN_ID)
                    verified = disposable_rpc(url, ca, identity_path)
                    observer = disposable_rpc(observer_url, ca, identity_path)
                    self.assertEqual(verified.genesis_sha256, unhex(genesis, 32))
                    self.assertEqual(observer.genesis_sha256, verified.genesis_sha256)
                    self.assertEqual(genesis_document(verified, 'boundary'),
                                     genesis_document(observer, 'boundary'))
                    key_file = directory / 'signer.key'
                    write_new(key_file, ('0x' + bytes(chain.account.key).hex()).encode())
                    self.assertEqual(signer(verified, key_file), chain.account.address.lower())
                    for endpoint in ('https://127.0.0.1:18545', 'https://localhost:19443'):
                        with self.assertRaises(ValueError):
                            disposable_rpc(endpoint, ca, identity_path)
                    with self.assertRaises(OSError):
                        Rpc(url).call('eth_chainId', [])
                    for field, value in [('genesis_sha256', denied), ('chain_id', 31337),
                                         ('comet_chain_id', 'custody-disposable-1'),
                                         ('comet_chain_id', 'wrong-chain'),
                                         ('genesis_sha256', '0x' + 'ab' * 32),
                                         ('ca_sha256', '0x' + '00' * 32),
                                         ('rpc_origins', ['https://localhost:1'])]:
                        identity_path.write_text(json.dumps({**identity, field: value}))
                        with self.assertRaises(ValueError):
                            disposable_rpc(url, ca, identity_path)
                    identity_path.write_text(json.dumps(identity))
                    os.chmod(key_file, 0o644)
                    with self.assertRaises(ValueError):
                        signer(verified, key_file)
                    os.chmod(key_file, 0o600)
                    for method in ('anvil_mine', 'evm_mine', 'eth_accounts', 'eth_sendTransaction'):
                        with self.assertRaises(ValueError):
                            verified.call(method, [])
                    actor = ed25519.Ed25519PrivateKey.generate()
                    actor_public = actor.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
                    name = ('agent:did:layerx:' + actor_public.hex() + ':main').encode()
                    beneficiary = '0x' + hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hexdigest()
                    output = directory / 'custody.json'
                    command('python3', str(ROOT / 'tests/bridge/deploy_local_custody.py'),
                            '--allow-local-chain', '--rpc', url, '--ca-bundle', ca,
                            '--disposable-identity', str(identity_path), '--key-file', str(key_file),
                            '--asset', '0x' + hashlib.sha256(b'test asset').hexdigest(),
                            '--beneficiary', beneficiary, '--amount', '1000000000000000000',
                            '--output', str(output))
                    deployed = json.loads(output.read_text())
                    self.assertEqual(deployed['chain_id'], 125)
                    self.assertEqual(deployed['comet_chain_id'], identity['comet_chain_id'])
                    self.assertEqual(deployed['genesis_sha256'], genesis)
                    code = unhex(verified.call('eth_getCode', [deployed['vault'], 'latest']))
                    self.assertEqual(deployed['runtime_sha256'], '0x' + hashlib.sha256(code).hexdigest())
                    self.assertEqual(int(verified.call('eth_getBalance', [deployed['token'], 'latest']), 16), 10 ** 18)
                    authority = ed25519.Ed25519PrivateKey.generate()
                    authority_path = directory / 'attestor.key'
                    write_new(authority_path, authority.private_bytes_raw())
                    profile = directory / 'custody.profile'
                    history = directory / 'secrets/custody-history'
                    create_profile(SimpleNamespace(rpc=origins, ca_bundle=ca,
                        disposable_identity=str(identity_path), chain_id=125, network_id=402,
                        vault=deployed['vault'], runtime_sha256=deployed['runtime_sha256'],
                        asset=deployed['asset'], confirmations=2, attestor_key=str(authority_path),
                        output=str(profile), vault_artifact=str(self.vault_artifact), history_state=str(history)))
                    self.assertEqual(profile.read_bytes()[:5], b'LXBC2')
                    genesis_metadata = directory / 'genesis-metadata'
                    write_new(genesis_metadata, metadata(unhex(deployed['asset'], 32), actor_public, os.urandom(32)))
                    invocation = ['python3', str(ROOT / 'tests/bridge/custody_genesis.py'),
                                  '--profile', str(profile), '--builder', str(self.builder),
                                  '--genesis-metadata', str(genesis_metadata)]
                    with self.assertRaises(ValueError):
                        command(*invocation, '--output', str(directory / 'refused-genesis'))
                    self.assertFalse((directory / 'refused-genesis').exists())
                    command(*invocation, '--output', str(directory / 'genesis'), '--rpc', url,
                            '--ca-bundle', ca, '--disposable-identity', str(identity_path))
                    self.assertTrue((directory / 'genesis/artifacts/genesis.manifest').is_file())
                    evidence_args = ['python3', str(ROOT / 'tests/bridge/local_credit_evidence.py'),
                        '--rpc', url, '--rpc', observer_url, '--ca-bundle', ca,
                        '--disposable-identity', str(identity_path), '--custody', str(output),
                        '--asset', deployed['asset'], '--attestor-key', str(authority_path),
                        '--attestor-public-key', '0x' + authority.public_key().public_bytes(
                            Encoding.Raw, PublicFormat.Raw).hex(),
                        '--beneficiary-key', '0x' + actor_public.hex(), '--network-id', '402',
                        '--confirmations', '2', '--vault-artifact', str(self.vault_artifact),
                        '--history-state', str(history)]
                    command(*evidence_args, '--output', str(directory / 'evidence'))
                    produced_profile = (directory / 'evidence/custody.profile').read_bytes()
                    self.assertEqual(produced_profile, profile.read_bytes())
                    credit = (directory / 'evidence/custody.credit').read_bytes()
                    self.assertEqual(len(credit), 427)
                    self.assertEqual(credit[:5], b'LXDC2')
                    self.assertEqual(credit[37:41], (402).to_bytes(4, 'big'))
                    authority.public_key().verify(credit[363:], b'LX:CUSTODY:CREDIT:v2' + credit[:363])
                    with self.assertRaises(InvalidSignature):
                        authority.public_key().verify(credit[363:], b'LX:CUSTODY:CREDIT:v1' + credit[:363])
                    for option, value in [('--asset', '0x' + '00' * 32),
                                          ('--attestor-public-key', '0x' + '00' * 32),
                                          ('--beneficiary-key', '0x' + '00' * 32), ('--rpc', url)]:
                        invalid = evidence_args.copy()
                        selected = len(invalid) - 1 - invalid[::-1].index(option)
                        invalid[selected + 1] = value
                        rejected = directory / ('rejected-' + option[2:])
                        with self.assertRaises(ValueError):
                            command(*invalid, '--output', str(rejected))
                        self.assertFalse((rejected / 'custody.credit').exists())


if __name__ == '__main__':
    unittest.main()
