import datetime
import json
import os
from pathlib import Path
import socket
import shutil
import traceback
import subprocess
import sys
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

repo = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(repo / 'platform/hosted/human'))
from provision import write_json
from owner_native import produce
sys.path.insert(0, str(repo / "tests/daemon"))
from governance_lifecycle import session
from owner_checkpoint import checkpoint, hosted
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from custody_credit import Rpc, unhex
from deploy_local_custody import signer, deploy, calldata, command


def run(work, asset, rpc_port):
    assert os.geteuid() == 0, 'real native owner production requires root for isolated LNI peer credentials'
    work.chmod(0o755)
    ports = []
    for _ in range(3):
        with socket.socket() as reserved:
            reserved.bind(('127.0.0.1', 0))
            ports.append(reserved.getsockname()[1])
    program_port, replica_port, authority_port = ports
    inputs = work / 'human-evidence-input'
    processes = []
    logs = []
    for name in ('sequencer', 'treasury'):
        path = work / (name + '.seed')
        path.write_bytes(os.urandom(32))
        path.chmod(0o600)
    env = dict(os.environ)
    bootstrap = ['bash', str(repo / 'platform/hosted/node/bootstrap.sh'), '--data-dir', str(work / 'node'),
        '--run-dir', str(work / 'run'), '--network-id', '77', '--asset', asset,
        '--custody-profile', str(inputs / 'custody.profile'), '--settlement-env', str(work / 'settlement.env'), '--sequencer-key', str(work / 'sequencer.seed'),
        '--treasury-key', str(work / 'treasury.seed'), '--lni-uid', '4021', '--lni-gid', '4021',
        '--program-port', str(program_port), '--replica-port', str(replica_port),
        '--layerxd', str(repo / 'build/bin/layerxd'), '--genesis-build', str(repo / 'build/bin/layerx-genesis-build')]
    with open(work / 'bootstrap.log', 'wb') as log:
        subprocess.run(bootstrap, env=env, stdout=log, stderr=log, check=True)
    rpc = Rpc(f'http://127.0.0.1:{rpc_port}')
    account = signer(rpc, work / 'payer.key')
    custody = json.loads((inputs / 'owner-custody.json').read_text())
    config_hash = '0x' + 'ab' * 32
    bond = deploy(rpc, account, 'contracts/GuarantorBond.sol:GuarantorBond', account, account,
                  '0x85FcD13735F4309833A503EE804ea32395851479', custody['vault'], command('cast', 'keccak', 'USDL'),
                  3, 77, 1000, 86400, config_hash, 1)
    descriptor = (work / 'node/genesis/paxeer-deployment-descriptor.lxgd').read_bytes()
    assert descriptor[:5] == b'LXGD\1' and len(descriptor) == 105
    roots = [descriptor[i:i + 32] for i in (9, 41, 73)]
    registry = deploy(rpc, account, 'contracts/CheckpointRegistry.sol:CheckpointRegistry', bond, 3, 77, 2, 32,
                      3600, 60, *['0x' + value.hex() for value in roots], config_hash, 1)
    for field, expected in zip(('genesisManifestDigest()', 'genesisCanonicalStateRoot()', 'genesisReceiptRoot()'), roots):
        observed = rpc.call('eth_call', [dict(to=registry, data=calldata(field)), 'latest'])
        assert unhex(observed, 32) == expected
    registration = work / 'node/genesis/genesis.registration'
    registration.write_bytes(b'LXGR\1' + (77).to_bytes(4, 'big') + bytes(8) + roots[2] + roots[2] + b'\1')
    registration.chmod(0o600)
    settlement = dict(LAYERX_NODE_PAXEER_CHAIN_ID='31337', LAYERX_NODE_SETTLEMENT_CONTRACT=bond,
        LAYERX_NODE_CHECKPOINT_REGISTRY=registry, LAYERX_NODE_PAXEER_RPC_ADDRESS='127.0.0.1',
        LAYERX_NODE_PAXEER_RPC_PORT=str(rpc_port))
    (work / 'settlement.env').write_text(''.join(name + '=' + value + '\n' for name, value in settlement.items()))
    subprocess.run(['bash', str(repo / 'platform/hosted/node/bootstrap.sh'), '--check-settlement', str(work / 'settlement.env')],
                   stdout=subprocess.DEVNULL, check=True)
    env.update(settlement)
    public = {}
    for line in (work / 'node/node.env').read_text().splitlines():
        name, value = line.split('=', 1)
        public[name] = value
    tls = work / 'tls'
    tls.mkdir(mode=0o700)
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'owner-production')])
    now = datetime.datetime.now(datetime.timezone.utc)
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
            .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(minutes=1))
            .not_valid_after(now + datetime.timedelta(days=1))
            .add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost')]), critical=False)
            .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True).sign(key, hashes.SHA256()))
    (tls / 'cert.der').write_bytes(cert.public_bytes(serialization.Encoding.DER))
    (tls / 'ca.pem').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
    (tls / 'key.der').write_bytes(key.private_bytes(serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    token = tls / 'authority.token'
    token.write_text(os.urandom(32).hex())
    # Supply the replica credential directly through its protected generated file.
    authority = work / 'authority'
    authority.mkdir(mode=0o700)
    cli = work / 'layerxctl'
    shutil.copyfile(repo / 'cmd/layerxctl/target/debug/layerxctl', cli)
    cli.chmod(0o755)
    config = dict(node_socket=str(work / 'run/layerxd.lni.sock'), network_id=77,
        owner_seed_file=str(work / 'human-owner/owner.seed'), pending_seed_file=str(work / 'human-owner/pending.seed'),
        sequencer_public_key=public['LAYERX_NODE_SEQUENCER_PUBLIC_KEY'], layerxctl=str(cli),
        fee_limit=0, authority_url=f'https://localhost:{authority_port}', authority_token_file=str(token),
        authority_ca_file=str(tls / 'ca.pem'), authority_state_root=str(authority))
    write_json(inputs / 'owner-native.json', config)
    for base in (inputs, work / 'human-owner', tls, authority):
        for path in [base, *base.rglob('*')]:
            os.chown(path, 4021, 4021)
            path.chmod(0o700 if path.is_dir() else 0o600)
    os.chown(work / 'human-owner-result.json', 4021, 4021)
    replica_token = work / 'node/secrets/replica-token'
    os.chown(replica_token, 4021, 4021)
    (work / 'node').chmod(0o755)
    (work / 'node/secrets').chmod(0o755)

    def start(command, label, environment=None):
        log = open(work / (label + '.log'), 'wb')
        logs.append(log)
        process = subprocess.Popen(command, stdout=log, stderr=log, env=environment)
        processes.append(process)
        return process

    native_command = None
    sequencer = None
    try:
        for role, option in (('replica', '--authority-replica'), ('sequencer', '--serve')):
            launched = start(['bash', '-c', 'set -a; source "$1"; exec "$2" "$3" "$4"', 'owner-production',
                   str(work / f'node/{role}.env'), str(repo / 'build/bin/layerxd'), option,
                   str(work / f'node/{role}.conf')], role, env)
            if role == 'sequencer':
                sequencer = launched
                native_command = launched.args
            if role == 'replica':
                for _ in range(100):
                    try:
                        with socket.create_connection(('127.0.0.1', replica_port), timeout=0.1):
                            break
                    except OSError:
                        time.sleep(0.1)
        for _ in range(100):
            assert all(p.poll() is None for p in processes), 'native startup failed; see retained logs'
            if (work / 'run/layerxd.lni.sock').exists():
                break
            time.sleep(0.1)
        identity = work / 'node/identities.txt'
        pending = identity.with_suffix('.pending')
        pending.write_bytes(identity.read_bytes() + (inputs / 'owner-admission.txt').read_bytes())
        pending.chmod(0o600)
        os.replace(pending, identity)
        authority_env = dict(os.environ, LAYERX_AUTHORITY_LISTEN=f'127.0.0.1:{authority_port}',
            LAYERX_AUTHORITY_TLS_CERT_DER=str(tls / 'cert.der'), LAYERX_AUTHORITY_TLS_KEY_DER=str(tls / 'key.der'),
            LAYERX_AUTHORITY_TOKEN_FILES=str(token), LAYERX_AUTHORITY_REPLICA_URL=f'http://127.0.0.1:{replica_port}',
            LAYERX_AUTHORITY_REPLICA_BEARER_TOKEN_FILE=str(replica_token),
            LAYERX_AUTHORITY_REPLICA_ID=public['LAYERX_NODE_REPLICA_ID'],
            LAYERX_AUTHORITY_LNI_SOCKET=config['node_socket'], LAYERX_AUTHORITY_PROTOCOL_NETWORK_ID='77',
            LAYERX_AUTHORITY_NETWORK_ID='owner-production', LAYERX_AUTHORITY_WIRE_VERSION='3',
            LAYERX_AUTHORITY_SEQUENCER_ID=public['LAYERX_NODE_SEQUENCER_ID'],
            LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY=config['sequencer_public_key'],
            LAYERX_AUTHORITY_FIRST_BATCH='1', LAYERX_AUTHORITY_LAST_BATCH=str(2 ** 64 - 1))
        peer = ['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups']
        service = start([*peer, str(repo / 'platform/target/debug/layerx-receipt-authority')], 'authority-service', authority_env)
        for _ in range(100):
            assert service.poll() is None, 'authority startup failed'
            try:
                with socket.create_connection(('127.0.0.1', authority_port), timeout=0.1):
                    break
            except OSError:
                time.sleep(0.1)
        with open(work / 'producer.log', 'wb') as log:
            child = os.fork()
            if child == 0:
                os.dup2(log.fileno(), 1)
                os.dup2(log.fileno(), 2)
                os.setgroups([])
                os.setgid(4021)
                os.setuid(4021)
                try:
                    produce(work)
                    if "--governance" in sys.argv:
                        session(work)
                except BaseException:
                    traceback.print_exc()
                    sys.stderr.flush()
                    os._exit(1)
                os._exit(0)
            deadline = time.monotonic() + 90
            while True:
                finished, result = os.waitpid(child, os.WNOHANG)
                if finished:
                    assert result == 0, 'owner producer failed; see producer.log'
                    break
                if time.monotonic() >= deadline:
                    os.kill(child, 15)
                    os.waitpid(child, 0)
                    raise AssertionError('owner producer timeout')
                time.sleep(0.1)
        registration = json.loads((inputs / 'owner-registration.json').read_text())
        binding = json.loads((inputs / 'owner-admission.json').read_text())
        assert registration['owner_account'] == binding['owner_account']
        assert registration['identity']['did'] == binding['did']
        assert len(list(authority.glob('*.json'))) == 4
        assert len(list((inputs / 'owner-native-run').glob('*.receipt'))) == 4
        if '--checkpoint' in sys.argv:
            checkpoint(work, public, settlement, rpc, account, start, key, cert, 10)
            hosted(work, config, authority_env, start, service)
        sequencer.terminate()
        assert sequencer.wait(timeout=15) == 0
        start(native_command, 'sequencer-restart', env)
        for _ in range(100):
            state = subprocess.run([*peer, config['layerxctl'], 'read-state', '--socket', config['node_socket'],
                '--network-id', '77', '--protocol-version', '3', '--actor', binding['did']], capture_output=True)
            if state.returncode == 0:
                break
            time.sleep(0.1)
        expected_sequence = 10 if '--governance' in sys.argv else 4
        assert state.returncode == 0 and json.loads(state.stdout)['account_sequence'] == expected_sequence
        print('real native credit, identity, rotation and recovery producer passed with authenticated replica evidence and restart')
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=15)
        for log in logs:
            log.close()
