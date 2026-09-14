import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import time

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/bridge'))
from custody_chain import from_environment

PUBLICATION = runpy.run_path(str(ROOT / 'tests/daemon/guarantor-publication-chain.py'))
COMMON = runpy.run_path(str(ROOT / 'tests/daemon/finality-authority-chain.py'))


def invoke(arguments, environment, output):
    with output.open('wb') as log:
        subprocess.run([str(value) for value in arguments], cwd=ROOT, env=environment,
                       stdout=log, stderr=log, check=True, timeout=180)


def main():
    assert len(sys.argv) == 5
    stage, native, build, scenario = sys.argv[1], Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4])
    assert stage in ('finalize', 'replay')
    ready = json.loads((scenario / 'handover-ready.json').read_text())
    count = ready['batch'] + (2 if stage == 'replay' else 0)
    assert count > 1
    output = native / ('handover-' + stage)
    output.mkdir(mode=0o700)
    PUBLICATION['replay'](native, output, count, build)
    if stage == 'replay':
        current = json.loads((output / f'exports-{count}/{count}.json').read_text())
        header = PUBLICATION['decode_header'](current['canonical_header'])
        assert header[2] == 2 and header[3] == count
        retained = os.environ.get('LAYERX_TEST_HANDOVER_DIVERGENCE')
        os.environ['LAYERX_TEST_HANDOVER_DIVERGENCE'] = '1'
        divergent = native / 'handover-divergence'
        divergent.mkdir(mode=0o700)
        state = divergent / 'state'
        state.mkdir(mode=0o700)
        with (divergent / 'replay.log').open('wb') as log:
            subprocess.run(['bash', '-c', 'source platform/hosted/node/sequencer-env.sh\nlayerx_sequencer_environment "$1"\nset -a\nsource "$7"\nexec "$2" "$3" "$4" "$5" "$6"', 'replay',
                str(native / 'data/sequencer.env'), str(build / 'tests/lxp_test_guarantor_runtime'),
                str(native / 'data/sequencer.conf'), str(state), str(native / 'data/checkpoints/da-bodies.log'), str(count), str(native / 'settlement.env')],
                cwd=ROOT, env=os.environ, stdout=log, stderr=log, check=True, timeout=180)
        if retained is None:
            del os.environ['LAYERX_TEST_HANDOVER_DIVERGENCE']
        else:
            os.environ['LAYERX_TEST_HANDOVER_DIVERGENCE'] = retained
        assert (state / 'replay-halt').is_file()
        print('independent guarantor replay verified old and new epochs, rollback, every state witness, and persistent authenticated divergence refusal')
        if os.environ.get('LAYERX_TEST_HANDOVER_PEERS') == '1':
            peers = runpy.run_path(str(ROOT / 'tests/daemon/handover-peers.py'))
            peers['run'](native, build, output / f'exports-{count}', count,
                os.environ['LAYERX_TEST_HANDOVER_LNI_SOCKET'])
        return
    environment = os.environ.copy()
    settlement = dict(line.split('=', 1) for line in (native / 'settlement.env').read_text().splitlines())
    assert set(settlement) == {'LAYERX_NODE_PAXEER_CHAIN_ID', 'LAYERX_NODE_SETTLEMENT_CONTRACT',
        'LAYERX_NODE_CHECKPOINT_REGISTRY', 'LAYERX_NODE_PAXEER_RPC_ADDRESS', 'LAYERX_NODE_PAXEER_RPC_PORT'}
    environment.update(settlement)
    chain = from_environment(os.environ['LAYERX_TEST_WITHDRAW_RPC'])
    assert chain.rpc('eth_chainId', []) == '0x7d'
    bond = settlement['LAYERX_NODE_SETTLEMENT_CONTRACT']
    registry = settlement['LAYERX_NODE_CHECKPOINT_REGISTRY']
    administrator = chain.account.address
    chain.send(COMMON['USDL'], 'mint(address,uint256)', administrator, '2000')
    chain.send(COMMON['USDL'], 'approve(address,uint256)', bond, '2000')
    for index, signer in enumerate(COMMON['SIGNERS'], 1):
        identifier = '0x' + index.to_bytes(32, 'big').hex()
        chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)',
            identifier, signer, administrator, '1', str(index))
        chain.send(bond, 'depositBond(bytes32,uint256)', identifier, '1000')
    checkpoint_id = None
    certificate_directory = None
    for batch in range(1, count + 1):
        exported = json.loads((output / f'exports-{count}/{batch}.json').read_text())
        header = bytes.fromhex(exported['canonical_header'].removeprefix('0x'))
        path = output / f'header-{batch}.bin'
        path.write_bytes(header)
        native_environment = environment | {'LAYERX_TEST_DA_HEADER_FILE': str(path)}
        prepared = subprocess.run([str(build / 'tests/lxp_test_daemon_finality_authority'), 'prepare'],
            cwd=ROOT, env=native_environment, check=True, capture_output=True, timeout=30)
        vector = json.loads(prepared.stdout)
        calldata = COMMON['run']('cast', 'calldata',
            f"registerCheckpoint({COMMON['HEADER']},bytes,{COMMON['ATTESTATION']}[])",
            vector['header'], '0x', vector['attestations'])
        receipt = chain.transaction(calldata, registry)
        assert int(receipt['status'], 16) == 1 and receipt['logs']
        observed = int(chain.rpc('eth_getBlockByNumber', [receipt['blockNumber'], False])['timestamp'], 16) * 1000
        certificate_directory = output / f'certificate-{batch}'
        certificate_directory.mkdir(mode=0o700)
        invoke([build / 'tests/lxp_test_daemon_finality_authority', 'emit', receipt['transactionHash'],
            int(receipt['blockNumber'], 16), observed, certificate_directory], native_environment,
            output / f'emit-{batch}.log')
        checkpoint_id = vector['checkpoint_id'].removeprefix('0x')
    replacement = Ed25519PrivateKey.from_private_bytes(bytes([0x44]) * 32).public_key().public_bytes_raw()
    now = int(time.time() * 1000)
    activity = scenario / 'handover.activity'
    arguments = [build / 'bin/layerx-handover', '--issue', native / 'data/genesis/genesis.manifest',
        native / 'data/checkpoints/da-bodies.log', certificate_directory / 'checkpoint.bin',
        certificate_directory / 'finality.bin', native / 'treasury', replacement.hex(), checkpoint_id,
        ready['identity_sequence'], now, now + 300000, 0, os.urandom(32).hex(), activity]
    fifo = output / 'invalid-input.fifo'
    os.mkfifo(fifo, 0o600)
    for index, name in ((2, 'manifest'), (3, 'history'), (4, 'checkpoint'), (5, 'finality'), (6, 'key')):
        invalid_input = arguments.copy()
        invalid_input[index] = fifo
        with (output / f'refuse-fifo-{name}.log').open('wb') as log:
            refused_input = subprocess.run([str(value) for value in invalid_input], cwd=ROOT,
                env=environment, stdout=log, stderr=log, timeout=10)
        assert refused_input.returncode != 0 and not activity.exists()
    invoke(arguments, environment, output / 'issue.log')
    activity.chmod(0o644)
    verify = [build / 'bin/layerx-handover', '--verify-key', native / 'data/genesis/genesis.manifest',
        native / 'data/checkpoints/da-bodies.log', replacement.hex(), activity]
    invoke(verify, environment, output / 'replacement-key.log')
    invalid = verify.copy()
    invalid[-2] = Ed25519PrivateKey.from_private_bytes(bytes([0x45]) * 32).public_key().public_bytes_raw().hex()
    with (output / 'wrong-replacement.log').open('wb') as log:
        refused = subprocess.run([str(value) for value in invalid], cwd=ROOT, env=environment,
            stdout=log, stderr=log, timeout=60)
    assert refused.returncode != 0
    print(f'actual funded predecessor batches 1..{count} independently replayed and finalized on disposable chain 125')


if __name__ == '__main__':
    main()
