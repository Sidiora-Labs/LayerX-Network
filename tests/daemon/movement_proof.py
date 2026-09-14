import datetime
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import socket
import ssl
import subprocess
import time
import urllib.request

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

from custody_credit import Rpc
from deploy_local_custody import calldata, deploy


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def boundary(work, upstream, host, target, launch, ca_key, ca_cert):
    directory = work / ('boundary-' + host)
    directory.mkdir(mode=0o700)
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    now = datetime.datetime.now(datetime.timezone.utc)
    alternative = x509.DNSName(host) if host == 'localhost' else x509.IPAddress(ipaddress.ip_address(host))
    certificate = (x509.CertificateBuilder()
        .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, host)]))
        .issuer_name(ca_cert.subject).public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=1))
        .not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(x509.SubjectAlternativeName([alternative]), critical=False)
        .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
        .sign(ca_key, hashes.SHA256()))
    (directory / 'certificate.der').write_bytes(certificate.public_bytes(serialization.Encoding.DER))
    (directory / 'key.der').write_bytes(key.private_bytes(serialization.Encoding.DER,
        serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    selected = port()
    environment = dict(os.environ, LAYERX_PAXEER_CHAIN_ID='31337',
        LAYERX_PAXEER_NODE_URL=upstream, LAYERX_PAXEER_BOUNDARY_LISTEN=f'127.0.0.1:{selected}',
        LAYERX_PAXEER_BOUNDARY_TLS_CERT_DER=str(directory / 'certificate.der'),
        LAYERX_PAXEER_BOUNDARY_TLS_KEY_DER=str(directory / 'key.der'))
    environment.pop('LAYERX_PAXEER_COMET_URL', None)
    process = launch([str(target / 'debug/layerx-paxeer-boundary')], 'movement-' + host, environment)
    origin = f'https://{host}:{selected}'
    trust = ssl.create_default_context(cafile=str(work / 'ca.pem'))
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=trust))
    for _ in range(100):
        assert process.poll() is None, 'actual movement TLS boundary exited'
        try:
            with opener.open(origin + '/readyz', timeout=1) as response:
                assert response.status == 200 and json.load(response)['chain_id'] == 31337
                return origin
        except OSError:
            time.sleep(0.1)
    raise AssertionError('actual movement TLS boundary did not become ready')


def run(work, settlement, rpc, account, launch, ca_key, ca_cert, batch):
    repo = Path(__file__).resolve().parents[2]
    target = Path(os.environ.get('CARGO_TARGET_DIR', repo / '.lane-target')).resolve()
    evidence = work / 'movement-evidence'
    evidence.mkdir(mode=0o700)
    inputs = work / 'human-evidence-input'
    custody = json.loads((inputs / 'owner-custody.json').read_text())
    owner = json.loads((inputs / 'owner-admission.json').read_text())
    transaction = json.loads((inputs / 'custody-deposit.json').read_text())['transactionHash']
    registry = settlement['LAYERX_NODE_CHECKPOINT_REGISTRY']
    checkpoint = rpc.call('eth_call', [dict(to=registry,
        data=calldata('checkpointAtBatch(uint64)', batch)), 'latest'])
    assert rpc.call('eth_chainId', []) == '0x7a69' and int(checkpoint, 16) != 0
    manager = '0x' + rpc.call('eth_call', [dict(to=settlement['LAYERX_NODE_SETTLEMENT_CONTRACT'],
        data=calldata('slashingAuthority()')), 'latest'])[-40:]
    configuration = '0x' + hashlib.sha256(b'LayerX/movement-proof/contracts/v1').hexdigest()
    nullifiers = deploy(rpc, account, 'contracts/storage/WithdrawalNullifierRegistry.sol:WithdrawalNullifierRegistry',
        custody['timelock'], account, configuration, 1)
    claims = deploy(rpc, account, 'contracts/WithdrawalClaims.sol:WithdrawalClaims',
        registry, manager, nullifiers, custody['vault'], configuration, 1)
    exits = deploy(rpc, account, 'contracts/EmergencyExit.sol:EmergencyExit',
        registry, manager, nullifiers, custody['vault'], custody['timelock'], account, 3600, configuration, 1)
    observer_port = port()
    tip = rpc.call('eth_blockNumber', [])
    observer = launch(['anvil', '--silent', '--host', '127.0.0.1', '--port', str(observer_port),
        '--chain-id', '31337', '--accounts', '0', '--fork-url', rpc.url,
        '--fork-block-number', str(int(tip, 16))], 'movement-observer')
    observer_rpc = Rpc(f'http://127.0.0.1:{observer_port}')
    for _ in range(100):
        assert observer.poll() is None, 'actual movement observer exited'
        try:
            assert observer_rpc.call('eth_chainId', []) == '0x7a69'
            assert observer_rpc.call('eth_getBlockByNumber', [tip, False])['hash'] == rpc.call(
                'eth_getBlockByNumber', [tip, False])['hash']
            break
        except OSError:
            time.sleep(0.1)
    else:
        raise AssertionError('actual movement observer did not become ready')
    (evidence / 'ca.pem').write_bytes(ca_cert.public_bytes(serialization.Encoding.PEM))
    (evidence / 'ca.der').write_bytes(ca_cert.public_bytes(serialization.Encoding.DER))
    origins = [boundary(evidence, url, host, target, launch, ca_key, ca_cert)
        for url, host in ((rpc.url, 'localhost'), (observer_rpc.url, '127.0.0.1'))]
    profile = (inputs / 'custody.profile').read_bytes()
    (evidence / 'profile.bin').write_bytes(profile)
    (evidence / ('credit-' + transaction[2:] + '.bin')).write_bytes((inputs / 'custody-credit.bin').read_bytes())
    values = dict(MODE='evidence-only', DEADLINE_SECONDS='20', MAX_FRAME_BYTES='1048576',
        PROTOCOL_VERSION='3', NETWORK_ID='77', PAXEER_CHAIN_ID='31337',
        PAXEER_CA_DER=str(evidence / 'ca.der'), PAXEER_RPC_URLS=json.dumps(origins),
        PAXEER_MINIMUM_AGREEMENT='2', PAXEER_CONFIRMATIONS='1',
        PAXEER_VAULT=custody['vault'], PAXEER_CHECKPOINT_REGISTRY=registry,
        PAXEER_CLAIMS_CONTRACT=claims, PAXEER_EXIT_CONTRACT=exits,
        PAXEER_CHECKPOINT_AUTHORITY='0x' + profile[65:97].hex(),
        CUSTODY_REFERENCE='0x' + bytes(12).hex() + custody['vault'][2:],
        CUSTODY_PROFILE=str(evidence / 'profile.bin'), EVIDENCE_ROOT=str(evidence),
        STATE_ROOT=str(evidence / 'state'), SOCKET=str(evidence / 'movement.sock'),
        ALLOWED_UID=str(os.geteuid()), ALLOWED_GID=str(os.getegid()),
        CHECKPOINT_INTERVAL_SECONDS='1', PAXEER_BLOCK_SECONDS='1',
        REMINDER_INTERVAL_SECONDS='1', POLL_SECONDS='1', DELAYED_AFTER_POLLS='2')
    environment = dict(os.environ, **{'LAYERX_HUMAN_MOVEMENT_PROVIDER_' + key: value for key, value in values.items()})
    command = [str(target / 'debug/layerx-human-movement-provider'), '--publish-deposit-proof',
        transaction, checkpoint, 'agent:' + owner['did'] + ':main']
    with (work / 'movement-proof.log').open('wb') as log:
        for _ in range(2):
            subprocess.run(command, env=environment, stdout=log, stderr=log, check=True, timeout=120)
        path = evidence / ('deposit-' + transaction[2:] + '.bin')
        original = path.read_bytes()
        for index, invalid in ((3, '0x' + hashlib.sha256(b'absent checkpoint').hexdigest()),
                               (4, 'agent:did:layerx:unrelated-recipient:main')):
            refused = list(command)
            refused[index] = invalid
            assert subprocess.run(refused, env=environment, stdout=log, stderr=log, timeout=120).returncode != 0
            assert path.read_bytes() == original
        environment.update(LAYERX_HUMAN_MOVEMENT_TEST_PAXEER_RPC=origins[0],
            LAYERX_HUMAN_MOVEMENT_TEST_TRANSACTION=transaction)
        subprocess.run(['cargo', 'test', '--locked', '--manifest-path', str(repo / 'human/Cargo.toml'),
            '-p', 'layerx-human-movement-provider',
            'tests::live_paxeer_deposit_proof_is_reverified_after_restart', '--', '--ignored', '--exact'],
            env=environment, stdout=log, stderr=log, check=True, timeout=120)
    print('real custody publication quorum, private evidence export, wrong bindings and unchanged live restart gate passed', flush=True)
