import contextlib
import hashlib
import http.client
import json
import os
from pathlib import Path
import runpy
import socket
import ssl
import subprocess
import tempfile
import time
from urllib.parse import urlsplit

from eth_account import Account
from eth_utils import to_checksum_address

ROOT = Path(__file__).resolve().parents[2]
COMMON = runpy.run_path(str(ROOT / 'tests/daemon/finality-authority-chain.py'))
USDL = COMMON['USDL']
FORBIDDEN_PORTS = {18545, 19443, 6379}


def command(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True, **kwargs)


def artifact(directory, name):
    return json.loads((directory / f'{name}.sol/{name}.json').read_text())


class Chain(COMMON['Chain']):
    def __init__(self, identity_path):
        self.identity_path = Path(identity_path)
        identity = json.loads(self.identity_path.read_text())
        parsed = urlsplit(identity['rpc'])
        assert parsed.scheme == 'http' and parsed.hostname == '127.0.0.1'
        assert parsed.port not in FORBIDDEN_PORTS and parsed.path == ''
        os.kill(identity['pid'], 0)
        process_args = Path(f"/proc/{identity['pid']}/cmdline").read_bytes().split(b'\0')
        assert os.fsencode(identity['home']) in process_args
        assert b'start' in process_args and b'--home' in process_args
        self.account = Account.from_key(Path(identity['deployer_key']).read_bytes())
        self.url = identity['rpc']
        self.directory = Path(identity['evidence_dir'])
        super().__init__(parsed.port)
        assert self.rpc('eth_chainId', []) == '0x7d'
        anchor = self.rpc('eth_getBlockByNumber', [hex(identity['anchor_number']), False])
        assert anchor['hash'] == identity['anchor_hash']
        assert self.account.address == identity['deployer']

    def transaction(self, data, to=None, success=True, value=0):
        assert self.rpc('eth_chainId', []) == '0x7d'
        transaction = {
            'chainId': 125,
            'nonce': int(self.rpc('eth_getTransactionCount', [self.account.address, 'pending']), 16),
            'data': data,
            'gas': 15_000_000,
            'gasPrice': int(self.rpc('eth_gasPrice', []), 16),
            'value': value,
        }
        if to is not None:
            transaction['to'] = to_checksum_address(to)
        signed = self.account.sign_transaction(transaction)
        digest = self.rpc('eth_sendRawTransaction', ['0x' + bytes(signed.raw_transaction).hex()])
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            receipt = self.rpc('eth_getTransactionReceipt', [digest])
            if receipt is not None:
                assert int(receipt['status'], 16) == int(success), receipt
                with (self.directory / 'transactions.jsonl').open('a') as evidence:
                    evidence.write(json.dumps(receipt, sort_keys=True) + '\n')
                return receipt
            time.sleep(.1)
        raise AssertionError('transaction receipt deadline: ' + digest)

    def view(self, address, signature, *args):
        encoded = COMMON['run']('cast', 'calldata', signature, *args)
        return self.rpc('eth_call', [{'to': address, 'data': encoded}, 'latest'])


def from_environment(url):
    chain = Chain(os.environ['LAYERX_TEST_CUSTODY_CHAIN_FILE'])
    assert chain.url == url
    return chain


def govern(chain, timelock, target, signature, *args):
    data = COMMON['run']('cast', 'calldata', signature, *args)
    assert int(chain.view(timelock, 'minDelay()'), 16) == 0

    def execute(destination, encoded):
        nonce = int(chain.view(timelock, 'operationNonce()'), 16)
        salt = '0x' + hashlib.sha256((destination + encoded + str(nonce)).encode()).hexdigest()
        chain.send(timelock, 'schedule(address,uint256,bytes,bytes32,uint64)',
                   destination, '0', encoded, salt, '0')
        chain.send(timelock, 'execute(address,uint256,bytes,bytes32,uint256)',
                   destination, '0', encoded, salt, str(nonce))

    execute(timelock, COMMON['run']('cast', 'calldata', 'setCallPermission(address,bytes4,bool)',
                                  target, data[:10], 'true'))
    execute(target, data)


@contextlib.contextmanager
def owned_chain(work, artifacts):
    with tempfile.TemporaryDirectory(prefix='lxp-custody-paxd-', dir='/tmp') as temporary:
        private = Path(temporary)
        ports, reservations = [], []
        for _ in range(7):
            reservation = socket.socket()
            reservation.bind(('127.0.0.1', 0))
            assert reservation.getsockname()[1] not in FORBIDDEN_PORTS
            ports.append(reservation.getsockname()[1])
            reservations.append(reservation)
        account = Account.create()
        key_file = private / 'deployer.key'
        descriptor = os.open(key_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as output:
            output.write(bytes(account.key))
        token = artifact(artifacts, 'BetaUsdl')
        runtime = token['deployedBytecode']['object']
        runtime_file = private / 'BetaUsdl.runtime.hex'
        runtime_file.write_text(runtime + '\n')
        chain_home = private / 'chain'
        env = {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_PAXEER_')}
        env.update(LAYERX_PAXEER_HOME=str(chain_home), LAYERX_PAXEER_CHAIN_ID='125',
                   LAYERX_PAXEER_DEPLOYER_ADDRESS=account.address,
                   LAYERX_PAXEER_USDL_RUNTIME=str(runtime_file))
        env['GOMAXPROCS'] = str(min(4, int(env.get('GOMAXPROCS', '4'))))
        assert int(env['GOMAXPROCS']) > 0
        for name, port in zip(('EVM', 'EVM_WS', 'RPC', 'P2P', 'GRPC', 'GRPC_WEB', 'API'), ports):
            env[f'LAYERX_PAXEER_{name}_PORT'] = str(port)
        with (work / 'paxd-init.log').open('w') as log:
            command('bash', 'platform/hosted/paxeer/init-chain.sh', env=env, stdout=log, stderr=log)
        genesis_bytes = (chain_home / 'config/genesis.json').read_bytes()
        (work / 'paxeer-genesis.json').write_bytes(genesis_bytes)
        for reservation in reservations:
            reservation.close()
        process = None
        try:
            with (work / 'paxd.log').open('w') as log:
                process = subprocess.Popen([env.get('PAXD', 'paxd'), 'start', '--home', str(chain_home)],
                                           cwd=ROOT, env=env, stdout=log, stderr=log)
            reader = COMMON['Chain'](ports[0])
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                assert process.poll() is None, 'owned Paxeer process exited'
                try:
                    assert reader.rpc('eth_chainId', []) == '0x7d'
                    if int(reader.rpc('eth_blockNumber', []), 16) > 0:
                        break
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(.1)
            else:
                raise AssertionError('owned Paxeer readiness deadline')
            assert reader.rpc('eth_getCode', [USDL, 'latest']).lower() == runtime.lower()
            identity = {
                'rpc': f'http://127.0.0.1:{ports[0]}', 'pid': process.pid,
                'home': str(chain_home), 'deployer_key': str(key_file), 'deployer': account.address,
                'evidence_dir': str(work),
                'comet_url': f'http://127.0.0.1:{ports[2]}',
                'anchor_number': int(reader.rpc('eth_blockNumber', []), 16),
            }
            identity['anchor_hash'] = reader.rpc('eth_getBlockByNumber', [hex(identity['anchor_number']), False])['hash']
            identity_path = private / 'owned-chain.json'
            identity_path.write_text(json.dumps(identity))
            chain = Chain(identity_path)
            chain.directory = work
            assert int(chain.view(USDL, 'owner()'), 16) == int(account.address, 16)
            assert int(chain.view(USDL, 'decimals()'), 16) == 6
            provenance = {
                'chain_id': 125, 'deployer': account.address, 'usdl': USDL,
                'genesis_sha256': hashlib.sha256(genesis_bytes).hexdigest(),
                'anchor_number': identity['anchor_number'], 'anchor_hash': identity['anchor_hash'],
                'token_runtime_sha256': hashlib.sha256(bytes.fromhex(runtime.removeprefix('0x'))).hexdigest(),
                'token_source_sha256': hashlib.sha256((ROOT / 'platform/hosted/paxeer/contracts/BetaUsdl.sol').read_bytes()).hexdigest(),
                'initializer_sha256': hashlib.sha256((ROOT / 'platform/hosted/paxeer/init-chain.sh').read_bytes()).hexdigest(),
            }
            (work / 'chain-provenance.json').write_text(json.dumps(provenance, sort_keys=True) + '\n')
            yield chain
        finally:
            for reservation in reservations:
                reservation.close()
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


@contextlib.contextmanager
def boundaries(work, source, binary):
    identity = json.loads(source.identity_path.read_text())
    private = source.identity_path.parent
    processes = []
    try:
        with (work / 'boundary-certificates.log').open('w') as log:
            command('openssl', 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
                    '-nodes', '-keyout', private / 'ca.key', '-out', private / 'ca.pem', '-days', '1',
                    '-subj', '/CN=LayerX custody qualification CA', stdout=log, stderr=log)
            command('openssl', 'req', '-new', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256',
                    '-nodes', '-keyout', private / 'tls.key', '-out', private / 'server.csr',
                    '-subj', '/CN=localhost', stdout=log, stderr=log)
            (private / 'extensions').write_text(
                'subjectAltName=DNS:localhost,IP:127.0.0.1\nbasicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\n')
            command('openssl', 'x509', '-req', '-in', private / 'server.csr', '-CA', private / 'ca.pem',
                    '-CAkey', private / 'ca.key', '-CAcreateserial', '-days', '1', '-extfile', private / 'extensions',
                    '-out', private / 'cert.pem', stdout=log, stderr=log)
            command('openssl', 'x509', '-in', private / 'cert.pem', '-outform', 'DER', '-out', private / 'cert.der',
                    stdout=log, stderr=log)
            command('openssl', 'pkcs8', '-topk8', '-nocrypt', '-in', private / 'tls.key', '-outform', 'DER',
                    '-out', private / 'tls.der', stdout=log, stderr=log)
        ca = work / 'boundary-ca.pem'
        ca.write_bytes((private / 'ca.pem').read_bytes())
        context = ssl.create_default_context(cafile=ca)
        origins = []
        genesis = None
        for index in range(2):
            port = COMMON['free_port']()
            assert port not in FORBIDDEN_PORTS
            env = os.environ | {
                'LAYERX_PAXEER_CHAIN_ID': '125', 'LAYERX_PAXEER_BOUNDARY_LISTEN': f'127.0.0.1:{port}',
                'LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER': str(private / 'cert.der'),
                'LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER': str(private / 'tls.der'),
                'LAYERX_PAXEER_NODE_URL': source.url, 'LAYERX_PAXEER_COMET_URL': identity['comet_url'],
            }
            with (work / f'boundary-{index}.log').open('w') as log:
                process = subprocess.Popen([str(binary)], cwd=ROOT, env=env, stdout=log, stderr=log)
            processes.append(process)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                assert process.poll() is None, 'owned boundary exited'
                connection = http.client.HTTPSConnection('localhost', port, context=context, timeout=5)
                try:
                    connection.request('GET', '/readyz')
                    response = connection.getresponse()
                    response.read()
                    if response.status == 200:
                        break
                except (OSError, http.client.HTTPException):
                    pass
                finally:
                    connection.close()
                time.sleep(.1)
            else:
                raise AssertionError('owned boundary readiness deadline')
            connection = http.client.HTTPSConnection('localhost', port, context=context, timeout=5)
            try:
                connection.request('GET', '/genesis')
                response = connection.getresponse()
                document = response.read(64 * 1024 * 1024 + 1)
                assert response.status == 200 and len(document) <= 64 * 1024 * 1024
                assert response.headers.get_all('X-LayerX-Genesis-SHA256') == [hashlib.sha256(document).hexdigest()]
            finally:
                connection.close()
            if genesis is None:
                expected = json.loads((work / 'paxeer-genesis.json').read_bytes())
                abci = expected['consensus_params']['abci']
                assert type(abci['vote_extensions_enable_height']) is int
                abci['vote_extensions_enable_height'] = str(abci['vote_extensions_enable_height'])
                assert json.loads(document) == expected
                genesis = document
            else:
                assert document == genesis
            origins.append(f'https://localhost:{port}')
        (work / 'boundary-genesis.json').write_bytes(genesis)
        disposable = {
            'rpc_origins': origins, 'chain_id': 125, 'comet_chain_id': json.loads(genesis)['chain_id'],
            'genesis_sha256': '0x' + hashlib.sha256(genesis).hexdigest(),
            'ca_sha256': '0x' + hashlib.sha256(ca.read_bytes()).hexdigest(), 'genesis_source': 'boundary',
        }
        path = work / 'disposable-identity.json'
        path.write_text(json.dumps(disposable, sort_keys=True) + '\n')
        yield ['--rpc', origins[0], '--rpc', origins[1], '--ca-bundle', str(ca), '--disposable-identity', str(path)]
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
