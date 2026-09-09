#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


reference = load('finality_chain', ROOT / 'tests/daemon/finality-authority-chain.py')
s = load('settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')


def signed_attestations(header, digest, bond, accounts, timestamp):
    result = []
    for index, account in enumerate(accounts, 1):
        fields = [header[0], header[1], 31337, bond, header[2], digest, digest, index.to_bytes(32, 'big'), header[3], header[11], True, True, 31, timestamp]
        message = s.hashlib.sha256(b'LXP/v2/guarantor-attestation\0' + s.encode_packed(s.ATTESTATION_TYPES[:14], fields)).digest()
        signature = s.keys.PrivateKey(bytes(account.key)).sign_msg_hash(message)
        result.append(tuple(fields + [account.address, signature.r.to_bytes(32, 'big'), signature.s.to_bytes(32, 'big'), signature.v + 27]))
    return result


def json_values(data):
    return [('0x' + value.hex()) if isinstance(value, bytes) else value for value in data]


def main():
    artifacts = ROOT / 'build/guarantor-settlement-contracts/artifacts'
    reference.run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol', 'contracts/custody/LayerXVault.sol', 'contracts/custody/AssetRegistry.sol', 'platform/hosted/paxeer/contracts/BetaUsdl.sol', '--out', str(artifacts), '--cache-path', str(ROOT / 'build/guarantor-settlement-contracts/cache'))
    token = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
    accounts = [s.Account.create(), s.Account.create()]
    submitter = s.Account.create()
    with tempfile.TemporaryDirectory(prefix='guarantor-settlement-') as directory:
        work = Path(directory)
        genesis = {'config': {'chainId': 31337}, 'timestamp': '0x3e8', 'gasLimit': '0x1c9c380', 'difficulty': '0x0', 'alloc': {reference.USDL: {'balance': '0x0', 'code': token['deployedBytecode']['object'], 'storage': {'0x' + '00' * 32: '0x' + '00' * 12 + reference.ADMIN[2:]}}, reference.ADMIN: {'balance': hex(10 ** 24)}, submitter.address: {'balance': hex(10 ** 20)}}}
        (work / 'genesis.json').write_text(json.dumps(genesis))
        port = reference.free_port()
        chain = reference.Chain(port)
        process = None
        try:
            with (work / 'anvil.log').open('w') as output:
                process = subprocess.Popen(['anvil', '--host', '127.0.0.1', '--port', str(port), '--chain-id', '31337', '--timestamp', '1000', '--hardfork', 'cancun', '--init', str(work / 'genesis.json'), '--silent'], cwd=ROOT, stdout=output, stderr=output)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError('test Anvil exited')
                try:
                    assert chain.rpc('eth_chainId', []) == '0x7a69'
                    break
                except (OSError, reference.http.client.HTTPException):
                    time.sleep(.1)
            else:
                raise RuntimeError('test Anvil readiness deadline')
            asset = reference.run('cast', 'keccak', 'USDL')
            asset_registry = chain.deploy(json.loads((artifacts / 'AssetRegistry.sol/AssetRegistry.json').read_text()), 'constructor(address,address,bytes32,uint192)', [reference.ADMIN, reference.ADMIN, reference.word('a3'), str(1 << 128)])
            vault = chain.deploy(json.loads((artifacts / 'LayerXVault.sol/LayerXVault.json').read_text()), 'constructor(address,address,address,bytes32,uint192)', [asset_registry, reference.ADMIN, reference.ADMIN, reference.word('a4'), str(1 << 128)])
            chain.send(asset_registry, 'registerAsset(bytes32,address,uint8,uint128,uint128)', asset, reference.USDL, '6', '1', '1000000')
            bond = chain.deploy(json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()), 'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)', [reference.ADMIN, reference.ADMIN, reference.USDL, vault, asset, '2', '42', '1000', '86400', reference.word('a1'), str(1 << 128)])
            chain.send(reference.USDL, 'mint(address,uint256)', reference.ADMIN, '2000')
            chain.send(reference.USDL, 'approve(address,uint256)', bond, '2000')
            for index, account in enumerate(accounts, 1):
                identity = '0x' + f'{index:064x}'
                chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)', identity, account.address, reference.ADMIN, '1', str(index))
                chain.send(bond, 'depositBond(bytes32,uint256)', identity, '1000')
            chain.send(vault, 'setGuarantorBond(address)', bond)
            chain.send(reference.USDL, 'mint(address,uint256)', reference.ADMIN, '1000')
            chain.send(reference.USDL, 'approve(address,uint256)', vault, '1000')
            chain.send(vault, 'deposit(bytes32,uint256,bytes32)', asset, '1000', reference.word('01'))
            registry = chain.deploy(json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()), 'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)', [bond, '2', '42', '2', '32', '3600', '60', reference.word('13'), reference.word('12'), reference.word('11'), reference.word('a2'), str(1 << 128)])
            vector = json.loads((ROOT / 'tests/vectors/checkpoint/fresh.json').read_text())
            header = s.values(s.HEADER_TYPES, list(vector['header'].values())[:15])
            proof = b'PROOF'
            digest = s.checkpoint_hash(header, proof)
            attestations = signed_attestations(header, digest, bond, accounts, header[13] + 1000)
            key_path = work / 'submitter.key'
            descriptor = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'w') as output:
                output.write('0x' + bytes(submitter.key).hex() + '\n')
            request = {'rpc_url': f'http://127.0.0.1:{port}', 'chain_id': 31337, 'settlement_contract': bond, 'checkpoint_registry': registry, 'header': json_values(header), 'validity_proof': '0x' + proof.hex(), 'checkpoint_id': '0x' + digest.hex(), 'attestations': [json_values(a) for a in attestations], 'submitter_key_file': str(key_path)}
            rpc = s.RPC(request['rpc_url'])
            policy = s.membership(rpc, dict(request, epoch=header[2], guarantors=[{'guarantor_id': '0x' + a[7].hex(), 'signer': a[14]} for a in attestations]))
            assert policy['minimum_bond'] == 100
            assert policy['version'] == 5
            unbonded = signed_attestations(header, digest, bond, [s.Account.create(), accounts[1]], header[13] + 1000)
            try:
                s.register(rpc, dict(request, attestations=[json_values(a) for a in unbonded]))
                raise AssertionError('unbonded attestation accepted')
            except ValueError as error:
                assert str(error) == 'unbonded guarantor'
            stale = signed_attestations(header, digest, bond, accounts, header[13] + 3600001)
            stale_data = s.calldata('registerCheckpoint(' + s.HEADER + ',bytes,' + s.ATTESTATION + '[])', (s.HEADER, 'bytes', s.ATTESTATION + '[]'), (header, proof, stale))
            stale_receipt = chain.transaction(stale_data, registry, success=False)
            assert not stale_receipt['logs']
            before = int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16)
            workers = []
            try:
                for index in range(2):
                    input_path = work / f'request-{index}.json'
                    input_path.write_text(json.dumps(dict(request, wire_output=str(work / f'result-{index}.wire'))))
                    workers.append(subprocess.Popen([sys.executable, str(ROOT / 'cmd/layerx-guarantor/settlement.py'), 'register', str(input_path), str(work / f'result-{index}.json')], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True))
                for worker in workers:
                    output, errors = worker.communicate(timeout=150)
                    assert worker.returncode == 0, errors
                    assert output == ''
            finally:
                for worker in workers:
                    if worker.poll() is None:
                        worker.terminate()
                        worker.wait(timeout=10)
            results = [json.loads((work / f'result-{index}.json').read_text()) for index in range(2)]
            first, second = sorted(results, key=lambda item: item['already_registered'])
            assert first['already_registered'] is False and first['set_version'] == 5
            assert first['members'][0]['bond_amount'] == 1000
            after = int(rpc.call('eth_getTransactionCount', [submitter.address, 'latest']), 16)
            assert second['already_registered'] is True
            assert first['transaction_id'] == second['transaction_id']
            for index in range(2):
                assert (work / f'result-{index}.wire').read_bytes() == s.wire_encode('register', results[index])
            assert after == before + 1
            print(json.dumps({'minimum_bond': policy['minimum_bond'], 'registered': True, 'duplicate_observed_without_resubmit': True, 'unbonded_rejected': True, 'stale_attested_at_contract_reverted': True, 'observed_block_number': first['observed_block_number'], 'membership_version': first['set_version']}, sort_keys=True))
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)

    subprocess.run([sys.executable, str(ROOT / 'tests/daemon/withdraw-custody.py')], cwd=ROOT,
        env=os.environ | {'LAYERX_TEST_SETTLEMENT_PUBLICATION': '1'}, check=True)


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, reference.terminate)
    signal.signal(signal.SIGINT, reference.terminate)
    main()
