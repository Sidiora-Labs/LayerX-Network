import argparse
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import time

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/bridge'))
from custody_chain import artifact, boundaries, from_environment, govern, owned_chain, retain_custody_proofs

COMMON = runpy.run_path(str(ROOT / 'tests/daemon/finality-authority-chain.py'))
ASSET = 'b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898'


def run(*args, **kwargs):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True, **kwargs)


def retain_public_evidence(work, evidence):
    for source in work.rglob('*'):
        relative = source.relative_to(work)
        if (not source.is_file() or source.is_symlink() or 'secrets' in relative.parts
                or (source.suffix in ('.pem', '.key', '.env') and source.name != 'boundary-ca.pem')
                or source.name in ('actor', 'attestor', 'sequencer', 'treasury', 'client')):
            continue
        target = evidence / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)


def register(work, url):
    if os.environ.get('LAYERX_TEST_SETTLEMENT_PUBLICATION') == '1':
        module = runpy.run_path(str(ROOT / 'tests/daemon/guarantor-publication-chain.py'))
        module['setup'](work, url)
        return
    request = (work / 'data/genesis/paxeer-registration-request.lxrr').read_bytes()
    assert len(request) == 73
    manifest = hashlib.sha256((work / 'data/genesis/genesis.manifest').read_bytes()).hexdigest()
    artifacts = Path(os.environ['LAYERX_TEST_CUSTODY_ARTIFACTS'])
    rpc = from_environment(url)
    token = COMMON['USDL']
    admin = rpc.account.address
    custody = json.loads(Path(os.environ['LAYERX_TEST_CUSTODY_FILE']).read_text())
    bond = rpc.deploy(artifact(artifacts, 'GuarantorBond'),
        'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
        [admin, admin, token, custody['vault'], COMMON['run']('cast', 'keccak', 'USDL'), '3', '77', '1000', '86400', COMMON['word']('a1'), str(1 << 128)])
    registry = rpc.deploy(artifact(artifacts, 'CheckpointRegistry'),
        'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
        [bond, '3', '77', '2', '32', '3600', '60', '0x' + manifest,
         '0x' + request[9:41].hex(), '0x' + request[41:73].hex(), COMMON['word']('a2'), str(1 << 128)])
    observed = rpc.rpc('eth_call', [{'to': registry, 'data': COMMON['run']('cast', 'calldata', 'latestFinalisedStateRoot()')}, 'latest'])
    assert bytes.fromhex(observed[2:]) == request[41:73]
    (work / 'data/genesis/genesis.registration').write_bytes(
        b'LXGR\x01' + (77).to_bytes(4, 'big') + bytes(8) + request[41:73] * 2 + b'\x01')
    write_settlement(work, rpc, bond, registry)


def write_settlement(work, chain, bond, registry):
    (work / 'settlement.env').write_text(
        'LAYERX_NODE_PAXEER_CHAIN_ID=125\n'
        f'LAYERX_NODE_SETTLEMENT_CONTRACT={bond}\n'
        f'LAYERX_NODE_CHECKPOINT_REGISTRY={registry}\n'
        'LAYERX_NODE_PAXEER_RPC_ADDRESS=127.0.0.1\n'
        f'LAYERX_NODE_PAXEER_RPC_PORT={chain.port}\n')


def deposit(chain, artifacts, beneficiary, amount):
    admin = chain.account.address
    config = '0x' + hashlib.sha256(b'LayerX/local-custody/real-weth/v1').hexdigest()
    timelock = chain.deploy(artifact(artifacts, 'LayerXBetaTimelock'),
        'constructor(uint64,uint64,address,address,address,uint256,bytes32,uint192)',
        ['0', '172800', admin, admin, admin, '0', config, '1'])
    registry = chain.deploy(artifact(artifacts, 'AssetRegistry'),
        'constructor(address,address,bytes32,uint192)', [timelock, admin, config, '1'])
    token = chain.deploy(artifact(artifacts, 'WETH'), 'constructor()', [])
    vault = chain.deploy(artifact(artifacts, 'LayerXVault'),
        'constructor(address,address,address,bytes32,uint192)', [registry, timelock, admin, config, '1'])
    govern(chain, timelock, registry, 'registerAsset(bytes32,address,uint8,uint128,uint128)',
           '0x' + ASSET, token, '18', '1', str(2 ** 128 - 1))
    chain.transaction(COMMON['run']('cast', 'calldata', 'deposit()'), token, value=amount)
    chain.send(token, 'approve(address,uint256)', vault, str(amount))
    deposited = chain.send(vault, 'deposit(bytes32,uint256,bytes32)', '0x' + ASSET, str(amount), '0x' + beneficiary)
    assert int(chain.view(token, 'balanceOf(address)', vault), 16) == amount
    assert int(chain.rpc('eth_getBalance', [token, 'latest']), 16) == amount
    deadline = time.monotonic() + 30
    while int(chain.rpc('eth_blockNumber', []), 16) < int(deposited['blockNumber'], 16) + 2:
        assert time.monotonic() < deadline, 'custody confirmations deadline'
        time.sleep(.1)
    code = bytes.fromhex(chain.rpc('eth_getCode', [vault, 'latest'])[2:])
    return {'chain_id': 125, 'vault': vault, 'registry': registry, 'timelock': timelock,
            'token': token, 'asset': '0x' + ASSET, 'amount': str(amount), 'beneficiary': '0x' + beneficiary,
            'transaction': deposited['transactionHash'], 'runtime_sha256': '0x' + hashlib.sha256(code).hexdigest(),
            'fork_block': int(chain.rpc('eth_blockNumber', []), 16)}


def main():
    if len(sys.argv) == 4 and sys.argv[1] == '--register':
        register(Path(sys.argv[2]), sys.argv[3])
        return
    assert os.geteuid() == 0
    parser = argparse.ArgumentParser()
    parser.add_argument('build_dir', nargs='?', default='build')
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument('--module-maintenance', action='store_true')
    modes.add_argument('--metered-allowance', action='store_true')
    args = parser.parse_args()
    build = (ROOT / args.build_dir).resolve()
    mode = '--module-maintenance' if args.module_maintenance else '--metered-allowance' if args.metered_allowance else '--withdraw'
    amount = 1000000000 if args.metered_allowance else 1000000
    assert os.environ.get('LAYERX_TEST_SETTLEMENT_PUBLICATION') != '1' or mode == '--withdraw'
    logs = ROOT / 'qual-logs/set1'
    logs.mkdir(parents=True, exist_ok=True)
    evidence = Path(tempfile.mkdtemp(prefix='e-daemon-custody-', dir=logs))
    with tempfile.TemporaryDirectory(prefix='lxp-daemon-custody-', dir='/tmp') as directory:
        work = Path(directory)
        work.chmod(0o755)
        print('withdraw custody evidence:', evidence, flush=True)
        try:
            seed = bytes([0x11]) * 32
            public = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
            did = 'did:layerx:' + public.hex()
            name = ('agent:' + did + ':main').encode()
            beneficiary = hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hexdigest()
            for name, value in [('actor', seed), ('attestor', bytes([0x55]) * 32)]:
                (work / name).write_bytes(value)
                (work / name).chmod(0o600)
            artifacts = build / 'withdraw-contracts/artifacts'
            threads = min(4, int(os.environ.get('CARGO_BUILD_JOBS', '4')),
                          int(os.environ.get('RAYON_NUM_THREADS', '4')))
            assert threads > 0
            run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol',
                'platform/hosted/paxeer/contracts/BetaUsdl.sol', 'contracts/challenge/CheckpointChallengeManager.sol',
                'contracts/governance/LayerXBetaTimelock.sol', 'contracts/custody/AssetRegistry.sol',
                'contracts/custody/LayerXVault.sol', 'paxeer-network/loadtest/contracts/evm/lib/solmate/src/tokens/WETH.sol',
                '--threads', str(threads), '--out', artifacts, '--cache-path', build / 'withdraw-contracts/cache')
            target = Path(os.environ['CARGO_TARGET_DIR']).resolve()
            boundary_binary = Path(os.environ.get('LAYERX_PAXEER_BOUNDARY_BIN', target/'debug/layerx-paxeer-boundary')).resolve()
            if 'LAYERX_PAXEER_BOUNDARY_BIN' not in os.environ:
                run('cargo', 'build', '--manifest-path', 'platform/Cargo.toml', '--locked',
                    '--jobs', str(threads), '-p', 'layerx-platform-paxeer-boundary', '--bin', 'layerx-paxeer-boundary')
            assert boundary_binary.is_file(), 'explicit boundary executable unavailable'
            proof_binary = Path(os.environ.get('LAYERX_CUSTODY_PROOF_BIN', build/'bin/layerx-custody-proof')).resolve()
            if 'LAYERX_CUSTODY_PROOF_BIN' not in os.environ:
                proof_env = os.environ | {'GOCACHE': str(target/'go-cache'), 'GOMAXPROCS': str(threads)}
                run('make', 'custody-proof-build', 'BUILD_DIR='+str(build), 'PAXEER_GO_JOBS='+str(threads), env=proof_env)
            assert proof_binary.is_file(), 'explicit custody proof executable unavailable'
            os.environ['LAYERX_CUSTODY_PROOF_BIN'] = str(proof_binary)
            with owned_chain(work, artifacts) as first:
                custody = deposit(first, artifacts, beneficiary, amount)
                (work / 'custody.json').write_text(json.dumps(custody, sort_keys=True) + '\n')
                with boundaries(work, first, boundary_binary) as (origins, ca, identity):
                    retain_custody_proofs(work, origins, ca, identity, custody['vault'])
                    pair = ['--rpc', origins[0], '--rpc', origins[1], '--ca-bundle', str(ca), '--disposable-identity', str(identity),
                            '--vault-artifact', str(artifacts/'LayerXVault.sol/LayerXVault.json')]
                    run(sys.executable, 'tests/bridge/custody_credit.py', 'profile', *pair, '--chain-id', '125',
                        '--network-id', '77', '--vault', custody['vault'], '--runtime-sha256', custody['runtime_sha256'],
                        '--asset', '0x' + ASSET, '--confirmations', '2', '--attestor-key', work / 'attestor', '--output', work / 'profile')
                    run(sys.executable, 'tests/bridge/custody_credit.py', 'attest', *pair, '--profile', work / 'profile',
                        '--network-id', '77', '--transaction', custody['transaction'], '--beneficiary', '0x' + beneficiary,
                        '--beneficiary-key', '0x' + public.hex(), '--expected-amount', str(amount),
                        '--attestor-key', work / 'attestor', '--output', work / 'credit')
                    identity_record = json.loads(identity.read_bytes())
                    history = work/'secrets'/('custody-history-'+identity_record['genesis_sha256'][2:])
                    run(sys.executable, 'tests/bridge/test_comet_evidence.py', '--evidence', str(work/'credit.proof.json'),
                        '--history-state', str(history), '--attestor-key', str(work/'attestor'))
                    if os.environ.get('LAYERX_CUSTODY_FIXTURE_DIR'):
                        exported = Path(os.environ['LAYERX_CUSTODY_FIXTURE_DIR'])
                        exported.mkdir(parents=True, exist_ok=True)
                        for source, name in ((work/'profile', 'custody.profile'), (work/'credit', 'custody.credit')):
                            with (exported/name).open('xb') as output:
                                output.write(source.read_bytes())
                        proof_evidence = json.loads((work/'credit.proof.json').read_bytes())
                        fixture_request = proof_evidence['requests'][0]
                        exported_history = subprocess.run([str(proof_binary),
                            '--history-state', str(history), '--attestor-key', str(work/'attestor')],
                            input=json.dumps(fixture_request | {'operation': 'export'}).encode(),
                            capture_output=True, check=True, timeout=120)
                        fixture_request['bundle']['history'] = json.loads(exported_history.stdout)
                        with (exported/'state-credit.json').open('x') as output:
                            json.dump(fixture_request, output, sort_keys=True, separators=(',', ':'))
                            output.write('\n')
                    if os.environ.get('LAYERX_CUSTODY_HISTORY_WINDOW') == '1':
                        run(sys.executable, 'tests/bridge/comet_history.py', *pair,
                            '--history-state', history, '--evidence', work/'credit.proof.json',
                            '--profile', work/'profile', '--network-id', '77',
                            '--transaction', custody['transaction'], '--beneficiary', '0x'+beneficiary,
                            '--beneficiary-key', '0x'+public.hex(), '--expected-amount', str(amount),
                            '--attestor-key', work/'attestor', '--output', work/'credit-after-window')
                    run(build / 'tests/bridge/sign-credit', work / 'profile', work / 'credit', did, work / 'actor',
                        '0', str(int(time.time() * 1000)), work / 'activity')
                    (work / 'activity').chmod(0o644)
                    env = os.environ | {'LAYERX_TEST_WITHDRAW_PROFILE': str(work / 'profile'),
                        'LAYERX_TEST_WITHDRAW_CREDIT': str(work / 'activity'), 'LAYERX_TEST_WITHDRAW_RPC': first.url,
                        'LAYERX_TEST_ADMISSION_LOG_DIR': str(work), 'LAYERX_TEST_PYTHON': sys.executable,
                        'LAYERX_TEST_CUSTODY_ARTIFACTS': str(artifacts), 'LAYERX_TEST_CUSTODY_FILE': str(work / 'custody.json'),
                        'LAYERX_TEST_CUSTODY_CHAIN_FILE': str(first.identity_path), 'LAYERX_TEST_CUSTODY_BUILD_DIR': str(build)}
                    if os.environ.get('LAYERX_TEST_SETTLEMENT_PUBLICATION') == '1':
                        module = runpy.run_path(str(ROOT / 'tests/daemon/guarantor-publication-chain.py'))
                        module['drive'](work, env, first.url)
                    else:
                        run('bash', 'tests/daemon/program-admission.sh', build, mode, env=env)
        finally:
            for path in work.rglob('*'):
                if path.is_fifo() or path.is_socket():
                    path.unlink()
            retain_public_evidence(work, evidence)
    print(f'custody-funded {mode.removeprefix("--")} execution and crash replay passed')


if __name__ == '__main__':
    main()
