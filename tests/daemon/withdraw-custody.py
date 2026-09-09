import hashlib
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import time

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/bridge'))
from qualify_credit import chain

COMMON = runpy.run_path(str(ROOT / 'tests/daemon/finality-authority-chain.py'))
ASSET = 'b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898'


def run(*args, **kwargs):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True, **kwargs)


def register(work, url):
    request = (work / 'data/genesis/paxeer-registration-request.lxrr').read_bytes()
    assert len(request) == 73
    manifest = hashlib.sha256((work / 'data/genesis/genesis.manifest').read_bytes()).hexdigest()
    artifacts = ROOT / 'build/withdraw-contracts/artifacts'
    rpc = COMMON['Chain'](int(url.rsplit(':', 1)[1]))
    token = COMMON['USDL']
    admin = COMMON['ADMIN']
    artifact = json.loads((artifacts / 'BetaUsdl.sol/BetaUsdl.json').read_text())
    rpc.rpc('anvil_setCode', [token, artifact['deployedBytecode']['object']])
    bond = rpc.deploy(json.loads((artifacts / 'GuarantorBond.sol/GuarantorBond.json').read_text()),
        'constructor(address,address,address,address,bytes32,uint16,uint32,uint32,uint64,bytes32,uint192)',
        [admin, admin, token, token, COMMON['run']('cast', 'keccak', 'USDL'), '3', '77', '1000', '86400', COMMON['word']('a1'), str(1 << 128)])
    registry = rpc.deploy(json.loads((artifacts / 'CheckpointRegistry.sol/CheckpointRegistry.json').read_text()),
        'constructor(address,uint16,uint32,uint16,uint16,uint64,uint64,bytes32,bytes32,bytes32,bytes32,uint192)',
        [bond, '3', '77', '2', '32', '3600', '60', '0x' + manifest,
         '0x' + request[9:41].hex(), '0x' + request[41:73].hex(), COMMON['word']('a2'), str(1 << 128)])
    observed = rpc.rpc('eth_call', [{'to': registry, 'data': COMMON['run']('cast', 'calldata', 'latestFinalisedStateRoot()')}, 'latest'])
    assert bytes.fromhex(observed[2:]) == request[41:73]
    (work / 'data/genesis/genesis.registration').write_bytes(
        b'LXGR\x01' + (77).to_bytes(4, 'big') + bytes(8) + request[41:73] * 2 + b'\x01')


def main():
    if len(sys.argv) == 4 and sys.argv[1] == '--register':
        register(Path(sys.argv[2]), sys.argv[3])
        return
    assert os.geteuid() == 0
    logs = ROOT / 'qual-logs/set1'
    work = Path(tempfile.mkdtemp(prefix='e-daemon-custody-', dir=logs))
    work.chmod(0o755)
    print('withdraw custody evidence:', work, flush=True)
    seed = bytes([0x11]) * 32
    public = Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    did = 'did:layerx:' + public.hex()
    name = ('agent:' + did + ':main').encode()
    beneficiary = hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).hexdigest()
    for name, value in [('actor', seed), ('attestor', bytes([0x55]) * 32)]:
        (work / name).write_bytes(value)
        (work / name).chmod(0o600)
    run('forge', 'build', 'contracts/GuarantorBond.sol', 'contracts/CheckpointRegistry.sol',
        'platform/hosted/paxeer/contracts/BetaUsdl.sol', '--out', 'build/withdraw-contracts/artifacts',
        '--cache-path', 'build/withdraw-contracts/cache')
    with chain(work, 'custody') as first:
        run(sys.executable, 'tests/bridge/deploy_local_custody.py', '--allow-local-chain', '--rpc', first,
            '--asset', '0x' + ASSET, '--beneficiary', '0x' + beneficiary,
            '--amount', '1000000', '--output', work / 'custody.json')
        custody = json.loads((work / 'custody.json').read_text())
        with chain(work, 'observer', first, custody['fork_block']) as second:
            pair = ['--rpc', first, '--rpc', second]
            run(sys.executable, 'tests/bridge/custody_credit.py', 'profile', *pair, '--chain-id', '31337',
                '--network-id', '77', '--vault', custody['vault'], '--runtime-sha256', custody['runtime_sha256'],
                '--asset', '0x' + ASSET, '--confirmations', '2', '--attestor-key', work / 'attestor', '--output', work / 'profile')
            run(sys.executable, 'tests/bridge/custody_credit.py', 'attest', *pair, '--profile', work / 'profile',
                '--network-id', '77', '--transaction', custody['transaction'], '--beneficiary', '0x' + beneficiary,
                '--beneficiary-key', '0x' + public.hex(), '--expected-amount', '1000000',
                '--attestor-key', work / 'attestor', '--output', work / 'credit')
            run('build/tests/bridge/sign-credit', work / 'profile', work / 'credit', did, work / 'actor',
                '0', str(int(time.time() * 1000)), work / 'activity')
            (work / 'activity').chmod(0o644)
            env = os.environ | {'LAYERX_TEST_WITHDRAW_PROFILE': str(work / 'profile'),
                'LAYERX_TEST_WITHDRAW_CREDIT': str(work / 'activity'), 'LAYERX_TEST_WITHDRAW_RPC': first,
                'LAYERX_TEST_ADMISSION_LOG_DIR': str(work)}
            run('bash', 'tests/daemon/program-admission.sh', 'build', '--withdraw', env=env)
    print('custody-funded WITHDRAW execution and crash replay passed')


if __name__ == '__main__':
    main()
