#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
TAG = b'LXP/Paxeer/membership-mirror/v1\x00'


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


reference = load('finality_chain', ROOT / 'tests/daemon/finality-authority-chain.py')
s = load('settlement', ROOT / 'cmd/layerx-guarantor/settlement.py')


def expected_commitment(chain_id, contract, epoch, policy, members):
    preimage = TAG + struct.pack('>Q', chain_id) + s.raw(contract, 20)
    preimage += struct.pack('>QQ', policy['version'], epoch)
    preimage += policy['minimum_bond'].to_bytes(16, 'big')
    preimage += struct.pack('>QI', 0, len(policy['members']))
    for entry, member in zip(policy['members'], members):
        public_key = s.raw(member['public_key'], 33)
        preimage += s.raw(entry['guarantor_id'], 32) + public_key
        preimage += entry['bond_amount'].to_bytes(16, 'big')
        preimage += struct.pack('>QQQ', entry['joined_epoch'], 0, 0)
        preimage += bytes([1]) + struct.pack('>I', 1)
        preimage += public_key
        preimage += struct.pack('>QQQ', entry['joined_epoch'], 0, entry['authorization_version'])
    return hashlib.sha256(preimage).digest()


def drive(binary, env, state_dir, epoch, custodied, bps, protocol, chain=None, network=None):
    command = [str(binary), str(state_dir), str(epoch), str(custodied), str(bps), str(protocol)]
    if chain is not None:
        command += [str(chain), str(network)]
    finished = subprocess.run(command, cwd=ROOT, env=env, capture_output=True, text=True,
                              timeout=300)
    fields = {}
    for line in finished.stdout.splitlines():
        if '=' in line:
            key, value = line.split('=', 1)
            fields[key] = value
    return finished.returncode, fields, finished.stderr


def deploy_bond(chain, artifacts, vault, asset, bps):
    return chain.deploy(
        json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()),
        'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
        [reference.ADMIN, reference.ADMIN, reference.USDL, vault, asset, '2', '42', str(bps),
         '86400', reference.word('a1'), str(1 << 128)])


def main():
    binary = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / 'build/tests/lxp_test_paxeer_membership_sync'
    if not binary.exists():
        raise RuntimeError(f'missing membership sync driver: {binary}')
    artifacts = ROOT / 'build/paxeer-membership-sync-contracts/artifacts'
    reference.run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol',
                  'contracts/custody/LayerXVault.sol', 'contracts/custody/AssetRegistry.sol',
                  'platform/hosted/paxeer/contracts/BetaUsdl.sol', '--out', str(artifacts),
                  '--cache-path', str(ROOT / 'build/paxeer-membership-sync-contracts/cache'))
    token = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
    accounts = [s.Account.create(), s.Account.create()]
    submitter = s.Account.create()
    epoch = 7
    with tempfile.TemporaryDirectory(prefix='paxeer-membership-sync-') as directory:
        work = Path(directory)
        state_dir = work / 'state'
        state_dir.mkdir()
        genesis = {'config': {'chainId': 31337}, 'timestamp': '0x3e8', 'gasLimit': '0x1c9c380',
                   'difficulty': '0x0',
                   'alloc': {reference.USDL: {'balance': '0x0',
                                              'code': token['deployedBytecode']['object'],
                                              'storage': {'0x' + '00' * 32: '0x' + '00' * 12 + reference.ADMIN[2:]}},
                             reference.ADMIN: {'balance': hex(10 ** 24)},
                             submitter.address: {'balance': hex(10 ** 20)}}}
        (work / 'genesis.json').write_text(json.dumps(genesis))
        port = reference.free_port()
        chain = reference.Chain(port)
        process = None
        try:
            with (work / 'anvil.log').open('w') as output:
                process = subprocess.Popen(
                    ['anvil', '--host', '127.0.0.1', '--port', str(port), '--chain-id', '31337',
                     '--timestamp', '1000', '--hardfork', 'cancun', '--init',
                     str(work / 'genesis.json'), '--silent'], cwd=ROOT, stdout=output, stderr=output)
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
            asset_registry = chain.deploy(
                json.loads((artifacts / 'AssetRegistry.sol/AssetRegistry.json').read_text()),
                'constructor(address,address,bytes32,uint192)',
                [reference.ADMIN, reference.ADMIN, reference.word('a3'), str(1 << 128)])
            vault = chain.deploy(
                json.loads((artifacts / 'LayerXVault.sol/LayerXVault.json').read_text()),
                'constructor(address,address,address,bytes32,uint192)',
                [asset_registry, reference.ADMIN, reference.ADMIN, reference.word('a4'), str(1 << 128)])
            chain.send(asset_registry, 'registerAsset(bytes32,address,uint8,uint128,uint128)', asset,
                       reference.USDL, '6', '1', '1000000')
            bond = deploy_bond(chain, artifacts, vault, asset, 1000)
            foreign_bond = deploy_bond(chain, artifacts, vault, asset, 1000)
            chain.send(reference.USDL, 'mint(address,uint256)', reference.ADMIN, '4000')
            chain.send(reference.USDL, 'approve(address,uint256)', bond, '4000')
            members = []
            for index, account in enumerate(accounts, 1):
                identity = '0x' + f'{index:064x}'
                chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)', identity,
                           account.address, reference.ADMIN, '1', str(index))
                chain.send(bond, 'depositBond(bytes32,uint256)', identity, '1000')
                public_key = s.keys.PrivateKey(bytes(account.key)).public_key.to_compressed_bytes()
                members.append({'guarantor_id': identity, 'public_key': '0x' + public_key.hex(),
                                'signer': account.address})
            members.sort(key=lambda member: member['guarantor_id'])
            chain.send(vault, 'setGuarantorBond(address)', bond)
            chain.send(reference.USDL, 'mint(address,uint256)', reference.ADMIN, '1000')
            chain.send(reference.USDL, 'approve(address,uint256)', vault, '1000')
            chain.send(vault, 'deposit(bytes32,uint256,bytes32)', asset, '1000', reference.word('01'))
            registry = chain.deploy(
                json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()),
                'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
                [bond, '2', '42', '2', '32', '3600', '60', reference.word('13'), reference.word('12'),
                 reference.word('11'), reference.word('a2'), str(1 << 128)])
            key_path = work / 'submitter.key'
            descriptor = os.open(key_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'w') as output:
                output.write('0x' + bytes(submitter.key).hex() + '\n')
            document = json.loads((ROOT / 'contracts/config/checkpoint-settlement.json').read_text())
            document['settlement_domains']['beta'] = {
                'protocol_version': 2, 'paxeer_chain_id': 31337, 'network_id': 42,
                'settlement_contract': registry, 'guarantor_bond': bond, 'guarantor_set': members}
            settlement_path = work / 'checkpoint-settlement.json'
            settlement_path.write_text(json.dumps(document))
            env = os.environ | {
                'LAYERX_NODE_PAXEER_CHAIN_ID': '31337',
                'LAYERX_NODE_SETTLEMENT_CONTRACT': bond,
                'LAYERX_NODE_CHECKPOINT_REGISTRY': registry,
                'LAYERX_NODE_PAXEER_RPC_ADDRESS': '127.0.0.1',
                'LAYERX_NODE_PAXEER_RPC_PORT': str(port),
                'LAYERX_GUARANTOR_SETTLEMENT_FILE': str(settlement_path),
                'LAYERX_GUARANTOR_SETTLEMENT_DOMAIN': 'beta',
                'LAYERX_GUARANTOR_SUBMITTER_KEY_FILE': str(key_path),
                'LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE': str(work / 'submitter.lock'),
                'LAYERX_GUARANTOR_PYTHON': sys.executable,
                'LAYERX_GUARANTOR_SETTLEMENT_HELPER': str(ROOT / 'cmd/layerx-guarantor/settlement.py')}
            rpc = s.RPC(f'http://127.0.0.1:{port}')
            request = {'chain_id': 31337, 'settlement_contract': bond, 'checkpoint_registry': registry,
                       'epoch': epoch,
                       'guarantors': [{'guarantor_id': m['guarantor_id'], 'signer': m['signer']}
                                      for m in members]}
            policy = s.membership(rpc, request)
            assert policy['minimum_bond'] == 100, policy
            assert policy['version'] == 5, policy
            code, fields, errors = drive(binary, env, state_dir, epoch, 1000, 1000, 2)
            assert code == 0, (code, fields, errors)
            assert fields['stage'] == 'complete' and fields['status'] == '0', fields
            assert fields['pre_availability'] == '1', fields
            assert fields['availability'] == '2', fields
            assert fields['repeated_availability'] == '2', fields
            assert fields['commitment_matches'] == '1', fields
            assert fields['chain_id'] == '31337' and fields['network_id'] == '42', fields
            assert fields['contract'].lower() == bond.lower(), fields
            assert fields['membership_version'] == '5', fields
            assert fields['mirror_version'] == '5', fields
            assert fields['observed_epoch'] == str(epoch), fields
            assert fields['member_count'] == '2', fields
            assert int(fields['minimum_bond'], 16) == 100, fields
            assert int(fields['custodied_value'], 16) == 1000, fields
            bound = expected_commitment(31337, bond, epoch, policy, members)
            assert fields['commitment'] == '0x' + bound.hex(), (fields['commitment'], bound.hex())
            for index, member in enumerate(members):
                assert fields[f'member{index}_id'].lower() == member['guarantor_id'].lower(), fields
                assert fields[f'member{index}_key'].lower() == member['public_key'].lower(), fields
                assert int(fields[f'member{index}_bond'], 16) == 1000, fields
                assert fields[f'member{index}_joined'] == '1', fields
                assert fields[f'member{index}_eligible'] == '1', fields

            code, fields, errors = drive(binary, env, state_dir, epoch, 2000, 1000, 2)
            assert code == 1 and fields['stage'] == 'sync' and fields['status'] == '-213', \
                (code, fields, errors)
            code, fields, errors = drive(binary, env, state_dir, epoch, 1000, 1000, 2, 31338, 42)
            assert code == 1 and fields['stage'] == 'sync' and fields['status'] == '-204', \
                (code, fields, errors)
            code, fields, errors = drive(binary, env, state_dir, epoch, 1000, 1000, 2, 31337, 43)
            assert code == 1 and fields['stage'] == 'sync' and fields['status'] == '-204', \
                (code, fields, errors)

            foreign_document = json.loads(settlement_path.read_text())
            foreign_document['settlement_domains']['beta']['guarantor_bond'] = foreign_bond
            foreign_path = work / 'foreign-settlement.json'
            foreign_path.write_text(json.dumps(foreign_document))
            foreign_env = env | {'LAYERX_GUARANTOR_SETTLEMENT_FILE': str(foreign_path),
                                 'LAYERX_NODE_SETTLEMENT_CONTRACT': foreign_bond}
            code, fields, errors = drive(binary, foreign_env, state_dir, epoch, 1000, 1000, 2)
            assert code == 1 and fields['stage'] == 'sync' and fields['status'] == '-213', \
                (code, fields, errors)

            chain.send(bond, 'depositBond(bytes32,uint256)', members[1]['guarantor_id'], '700')
            advanced = s.membership(rpc, request)
            assert advanced['version'] == 6, advanced
            code, fields, errors = drive(binary, env, state_dir, epoch, 1000, 1000, 2)
            assert code == 0 and fields['availability'] == '2', (code, fields, errors)
            assert fields['membership_version'] == '6', fields
            assert int(fields['member1_bond'], 16) == 1700, fields
            rebound = expected_commitment(31337, bond, epoch, advanced, members)
            assert fields['commitment'] == '0x' + rebound.hex(), (fields['commitment'], rebound.hex())
            assert rebound != bound
            print(json.dumps({'bound_membership_version': 5, 'rebound_membership_version': 6,
                              'commitment_recomputed_independently': True,
                              'foreign_chain_id_refused': -204, 'foreign_network_id_refused': -204,
                              'foreign_bond_contract_refused': -213,
                              'minimum_bond_divergence_refused': -213,
                              'guarantor_bond_contract': bond, 'chain_id': 31337},
                             sort_keys=True))
        finally:
            if process is not None and process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, reference.terminate)
    signal.signal(signal.SIGINT, reference.terminate)
    main()
