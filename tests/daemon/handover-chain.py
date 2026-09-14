import json
import os
from pathlib import Path
import runpy
import shutil
import tempfile
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
        consumer = os.environ.get('LAYERX_TEST_HANDOVER_CONSUMER_BIN')
        if consumer is not None:
            assert os.environ.get('LAYERX_TEST_HANDOVER_PEERS') == '1'
            public_exports = output / f'exports-{count}'
            assert (public_exports / 'handover-genesis.bin').read_bytes() == (
                native / 'data/genesis/genesis-handover-trust.lxt').read_bytes()
            with tempfile.TemporaryDirectory(prefix='lxp-handover-consumer-', dir='/tmp') as temporary:
                client_directory = Path(temporary)
                os.chown(client_directory, 4021, 4021)
                client_directory.chmod(0o700)
                executable = client_directory / 'native-handover-history'
                shutil.copyfile(consumer, executable)
                executable.chmod(0o755)
                for public_file in public_exports.iterdir():
                    if public_file.name == 'handover-genesis.bin' or public_file.name.startswith(('retired-', 'unauthorized-')):
                        destination = client_directory / public_file.name
                        shutil.copyfile(public_file, destination)
                        os.chown(destination, 4021, 4021)
                        destination.chmod(0o400)
                settlement = json.loads((native / 'handover-peers/checkpoint-settlement.json').read_text())
                domain = settlement['settlement_domains']['beta']
                registration = (native / 'data/genesis/paxeer-registration-request.lxrr').read_bytes()
                policy = dict(version='1', url=os.environ['LAYERX_TEST_WITHDRAW_RPC'],
                    transport='local-emulator', trust_anchor_der='', chain_id='125',
                    request_timeout_ms='8000', registry=domain['settlement_contract'].removeprefix('0x').lower(),
                    guarantor_bond=domain['guarantor_bond'].removeprefix('0x').lower(),
                    protocol_version='3', network_id='77', canonical_genesis_root=registration[9:41].hex(),
                    confirmations='1')
                policy_path = client_directory / 'handover-finality.conf'
                policy_path.write_text(''.join(f'{key}={value}\n' for key, value in policy.items()))
                os.chown(policy_path, 4021, 4021)
                policy_path.chmod(0o400)
                invoke(['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups',
                        executable, os.environ['LAYERX_TEST_HANDOVER_LNI_SOCKET'],
                        client_directory, count], os.environ,
                       output / 'public-history-consumer.log')
                read_consumer = os.environ.get('LAYERX_TEST_HANDOVER_READ_CONSUMER_BIN')
                if read_consumer is not None:
                    reads_executable = client_directory / 'native-handover-reads'
                    shutil.copyfile(read_consumer, reads_executable)
                    reads_executable.chmod(0o755)
                    invoke(['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups',
                            reads_executable, os.environ['LAYERX_TEST_HANDOVER_LNI_SOCKET'],
                            client_directory, count], os.environ,
                           output / 'public-history-reads.log')
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
    submitter = None
    if os.environ.get('LAYERX_TEST_HANDOVER_PEERS') == '1':
        peers = runpy.run_path(str(ROOT / 'tests/daemon/handover-peers.py'))
        submitter = peers['setup'](native, chain)
    administrator = chain.account.address
    chain.send(COMMON['USDL'], 'mint(address,uint256)', administrator, '2000')
    chain.send(COMMON['USDL'], 'approve(address,uint256)', bond, '2000')
    for index, signer in enumerate(COMMON['SIGNERS'], 1):
        identifier = '0x' + index.to_bytes(32, 'big').hex()
        chain.send(bond, 'activateGuarantor(bytes32,address,address,uint64,uint64)',
            identifier, signer, administrator, '1', str(index))
        chain.send(bond, 'depositBond(bytes32,uint256)', identifier, '1000')
    membership_version = int(chain.view(bond, 'membershipVersion()'), 16)
    assert 4 <= membership_version < 2 ** 64
    environment['LAYERX_TEST_DA_BONDED_SET_VERSION'] = str(membership_version)
    checkpoint_id = None
    certificate_directory = None
    for batch in range(1, count + 1):
        exported = json.loads((output / f'exports-{count}/{batch}.json').read_text())
        header = bytes.fromhex(exported['canonical_header'].removeprefix('0x'))
        path = output / f'header-{batch}.bin'
        path.write_bytes(header)
        certificate_directory = output / f'certificate-{batch}'
        certificate_directory.mkdir(mode=0o700)
        native_environment = environment | {'LAYERX_TEST_DA_HEADER_FILE': str(path)}
        if batch == 1:
            for invalid_version in ('0', '3', '04', '+4', '4x', str(2 ** 64)):
                with (output / ('refuse-membership-version-' + invalid_version + '.log')).open('wb') as log:
                    rejected = subprocess.run([str(build / 'tests/lxp_test_daemon_finality_authority'), 'prepare'],
                        cwd=ROOT, env=native_environment | {'LAYERX_TEST_DA_BONDED_SET_VERSION': invalid_version},
                        stdout=log, stderr=log, timeout=30)
                assert rejected.returncode != 0, 'invalid bonded set version accepted'
        prepared = subprocess.run([str(build / 'tests/lxp_test_daemon_finality_authority'), 'prepare', str(certificate_directory)],
            cwd=ROOT, env=native_environment, check=True, capture_output=True, timeout=30)
        vector = json.loads(prepared.stdout)
        calldata = COMMON['run']('cast', 'calldata',
            f"registerCheckpoint({COMMON['HEADER']},bytes,{COMMON['ATTESTATION']}[])",
            vector['header'], '0x', vector['attestations'])
        receipt = chain.transaction(calldata, registry, signer=submitter)
        assert int(receipt['status'], 16) == 1 and receipt['logs']
        observed = int(chain.rpc('eth_getBlockByNumber', [receipt['blockNumber'], False])['timestamp'], 16) * 1000
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
