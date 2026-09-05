#!/usr/bin/env python3
import http.client
import json
import os
from pathlib import Path
import socket
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
USDL = '0x85FcD13735F4309833A503EE804ea32395851479'
ADMIN = '0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266'
SIGNERS = ['0x7e5f4552091a69125d5dfcb7b8c2659029395bdf',
           '0x2b5ad5c4795c026514f8317c7a215e218dccd6cf']
HEADER = '(uint16,uint32,uint64,uint64,uint64,uint64,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,bytes32,uint64,bytes32)'
ATTESTATION = '(uint16,uint32,uint64,address,uint64,bytes32,bytes32,bytes32,uint64,bytes32,bool,bool,uint8,uint64,address,bytes32,bytes32,uint8)'


def run(*args, env=None):
    result = subprocess.run(args, cwd=ROOT, env=env, check=True,
                            capture_output=True, text=True, timeout=300)
    return result.stdout.strip()


def word(first):
    return '0x' + first + '00' * 31


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


class Chain:
    def __init__(self, port):
        self.port = port
        self.identifier = 0

    def rpc(self, method, params):
        self.identifier += 1
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=10)
        try:
            connection.request('POST', '/', json.dumps({'jsonrpc': '2.0',
                'id': self.identifier, 'method': method, 'params': params}),
                {'Content-Type': 'application/json'})
            response = connection.getresponse()
            assert response.status == 200, response.status
            body = json.loads(response.read())
            assert body['id'] == self.identifier and 'error' not in body, body
            return body['result']
        finally:
            connection.close()

    def transaction(self, data, to=None, success=True):
        transaction = {'from': ADMIN, 'data': data, 'gas': hex(15_000_000)}
        if to is not None:
            transaction['to'] = to
        digest = self.rpc('eth_sendTransaction', [transaction])
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            receipt = self.rpc('eth_getTransactionReceipt', [digest])
            if receipt is not None:
                assert int(receipt['status'], 16) == int(success), receipt
                return receipt
            time.sleep(.05)
        raise AssertionError('transaction receipt deadline: ' + digest)

    def send(self, to, signature, *args):
        return self.transaction(run('cast', 'calldata', signature, *args), to)

    def deploy(self, artifact, signature, args):
        encoded = run('cast', 'abi-encode', signature, *args)
        receipt = self.transaction(artifact['bytecode']['object'] + encoded[2:])
        address = receipt['contractAddress']
        assert address and self.rpc('eth_getCode', [address, 'latest']) != '0x'
        return address


def main():
    binary = Path(sys.argv[1]) if len(sys.argv) == 2 else ROOT / 'build/tests/lxp_test_daemon_finality_authority'
    binary = binary.resolve()
    if not binary.is_file():
        raise RuntimeError('finality-authority C fixture is not built: ' + str(binary))
    with tempfile.TemporaryDirectory(prefix='layerx-finality-chain-') as temporary:
        work = Path(temporary)
        artifacts = ROOT / 'build/finality-authority-contracts/artifacts'
        run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol',
            'platform/hosted/paxeer/contracts/BetaUsdl.sol', '--out', str(artifacts),
            '--cache-path', str(ROOT / 'build/finality-authority-contracts/cache'))
        token = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
        genesis = {'config': {'chainId': 31337}, 'timestamp': '0x3e8',
                   'gasLimit': '0x1c9c380', 'difficulty': '0x0', 'alloc': {
                       USDL: {'balance': '0x0', 'code': token['deployedBytecode']['object'],
                              'storage': {'0x' + '00' * 32: '0x' + '00' * 12 + ADMIN[2:]}},
                       ADMIN: {'balance': hex(10 ** 24)}}}
        genesis_path = work / 'genesis.json'
        genesis_path.write_text(json.dumps(genesis))
        port = free_port()
        chain = Chain(port)
        process = None
        try:
            with (work / 'anvil.log').open('w') as log:
                process = subprocess.Popen(['anvil', '--host', '127.0.0.1', '--port', str(port),
                    '--chain-id', '31337', '--timestamp', '1000', '--hardfork', 'cancun',
                    '--init', str(genesis_path), '--silent'], cwd=ROOT, stdout=log, stderr=log)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError((work / 'anvil.log').read_text())
                try:
                    assert chain.rpc('eth_chainId', []) == '0x7a69'
                    break
                except (OSError, http.client.HTTPException):
                    time.sleep(.1)
            else:
                raise RuntimeError('Anvil readiness deadline')
            assert chain.rpc('eth_getCode', [USDL, 'latest']) == token['deployedBytecode']['object']
            asset = run('cast', 'keccak', 'USDL')
            bond = chain.deploy(json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()),
                'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
                [ADMIN, ADMIN, USDL, USDL, asset, '2', '42', '1000', '86400', word('a1'), str(1 << 128)])
            chain.send(USDL, 'mint(address,uint256)', ADMIN, '2000')
            chain.send(USDL, 'approve(address,uint256)', bond, '2000')
            for index, signer in enumerate(SIGNERS, 1):
                guarantor = '0x' + f'{index:064x}'
                chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)',
                           guarantor, signer, ADMIN, '1', str(index))
                chain.send(bond, 'depositBond(bytes32,uint256)', guarantor, '1000')
            version_data = run('cast', 'calldata', 'membershipVersion()')
            assert int(chain.rpc('eth_call', [{'to': bond, 'data': version_data}, 'latest']), 16) == 4
            registry = chain.deploy(json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()),
                'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
                [bond, '2', '42', '2', '32', '3600', '60', word('13'), word('12'), word('11'), word('a2'), str(1 << 128)])
            env = os.environ.copy()
            env.update(LAYERX_NODE_PAXEER_CHAIN_ID='31337', LAYERX_NODE_SETTLEMENT_CONTRACT=bond,
                       LAYERX_NODE_CHECKPOINT_REGISTRY=registry,
                       LAYERX_NODE_PAXEER_RPC_ADDRESS='127.0.0.1', LAYERX_NODE_PAXEER_RPC_PORT=str(port))
            vector = json.loads(run(str(binary), 'prepare', env=env))
            calldata = run('cast', 'calldata', f'registerCheckpoint({HEADER},bytes,{ATTESTATION}[])',
                           vector['header'], '0x50524f4f46', vector['attestations'])
            receipt = chain.transaction(calldata, registry)
            assert receipt['logs'], 'registration must emit the canonical event'
            reverted = chain.transaction(calldata, registry, success=False)
            assert not reverted['logs'], 'reverted duplicate must not emit a registration'
            try:
                output = run(str(binary), 'verify', receipt['transactionHash'],
                             str(int(receipt['blockNumber'], 16)), reverted['transactionHash'],
                             str(int(reverted['blockNumber'], 16)), env=env)
            except subprocess.CalledProcessError:
                print(json.dumps({'registration_receipt': receipt, 'guarantor_bond': bond,
                                  'checkpoint_registry': registry, 'vector': vector}), flush=True)
                raise
            print(output)
            print(json.dumps({'chain_id': 31337, 'guarantor_bond': bond, 'checkpoint_registry': registry,
                              'checkpoint_id': vector['checkpoint_id'],
                              'registration_transaction': receipt['transactionHash'],
                              'registration_block': int(receipt['blockNumber'], 16),
                              'reverted_transaction': reverted['transactionHash'],
                              'membership_version': 4}, sort_keys=True))
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)


def terminate(signum, _frame):
    raise SystemExit(128 + signum)


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    try:
        main()
    except subprocess.CalledProcessError as error:
        sys.stderr.write(error.stdout + error.stderr)
        raise
