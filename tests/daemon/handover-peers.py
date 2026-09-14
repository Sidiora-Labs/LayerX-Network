import datetime
import ipaddress
import json
import os
from pathlib import Path
import runpy
import shutil
import socket
import subprocess
import sys
import tempfile
import time

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID
from eth_account import Account
from custody_chain import artifact, from_environment, govern

ROOT = Path(__file__).resolve().parents[2]
PUBLICATION = runpy.run_path(str(ROOT / 'tests/daemon/guarantor-publication-chain.py'))
CODEC = PUBLICATION['p']
SETTLEMENT = PUBLICATION['s']
COMMON = PUBLICATION['r']


def environment_file(path):
    return dict(line.split('=', 1) for line in path.read_text().splitlines() if line)


def authorize(export, inputs, recipient, vault):
    header = PUBLICATION['decode_header'](export['canonical_header'])
    checkpoint = SETTLEMENT.checkpoint_hash(header, b'')
    request = dict(chain_id=125,
        header=[CODEC.hx(value) if isinstance(value, bytes) else value for value in header],
        checkpoint_id=CODEC.hx(checkpoint), native_facts=export['native_facts'])
    balances, _, deposits, profile = CODEC.native_request(SETTLEMENT, request, header, checkpoint)
    authorities = {}
    for value in (0x11, 0x33):
        key = ed25519.Ed25519PrivateKey.from_private_bytes(bytes([value]) * 32)
        authorities[key.public_key().public_bytes_raw()] = key
    bindings = []
    for fact in balances:
        key = authorities.get(fact['authority'])
        assert key is not None, 'actual settlement balance authority is unavailable'
        message = (b'LX:SETTLE:RECIPIENT:v1\0' + header[1].to_bytes(4, 'big') + fact['account'] +
            fact['asset'] + recipient + checkpoint)
        bindings.append(dict(account=CODEC.hx(fact['account']), asset=CODEC.hx(fact['asset']),
            recipient=CODEC.hx(recipient), request_anchor=CODEC.hx(checkpoint), signature=CODEC.hx(key.sign(message))))
    registration = None
    if deposits:
        assert profile is not None and profile[13:33] == bytes.fromhex(vault[2:])
        reference = bytes(12) + bytes.fromhex(vault[2:])
        leaves = []
        for fact in deposits:
            leaf = (b'LX:PAXEER:DEPOSIT:LEAF:v1' + fact['identity'] + reference + fact['asset'] +
                fact['amount'] + checkpoint + header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))
            leaves.append(CODEC.sha(b'LXP/v1/merkle-leaf\0' + leaf))
        while len(leaves) > 1:
            leaves = [CODEC.sha(b'LXP/v1/merkle-internal\0' + leaves[index] +
                leaves[min(index + 1, len(leaves) - 1)]) for index in range(0, len(leaves), 2)]
        statement = (b'LX:PAXEER:DEPOSIT:ROOT:v1' + checkpoint + header[7] + leaves[0] + reference +
            header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))
        key = ed25519.Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32)
        registration = dict(vault=vault, custody_reference=CODEC.hx(reference), signature=CODEC.hx(key.sign(statement)))
    destination = inputs / (checkpoint.hex() + '.json')
    CODEC.atomic_json(destination, dict(version=2, checkpoint_id=CODEC.hx(checkpoint),
        recipient_bindings=bindings, deposit_registration=registration))
    os.chown(destination, 0, 4021)
    destination.chmod(0o440)


def tls_files(directory):
    directory.mkdir(mode=0o755)
    authority = ec.generate_private_key(ec.SECP256R1())
    now = datetime.datetime.now(datetime.timezone.utc)
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'LayerX handover peers')])
    ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
        .public_key(authority.public_key()).serial_number(x509.random_serial_number())
        .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
        .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
        .sign(authority, hashes.SHA256()))
    (directory / 'ca.pem').write_bytes(ca.public_bytes(serialization.Encoding.PEM))
    (directory / 'ca.pem').chmod(0o644)
    for index in (1, 2):
        target = directory / str(index)
        target.mkdir(mode=0o700)
        key = ec.generate_private_key(ec.SECP256R1())
        subject = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, f'guarantor-{index}')])
        cert = (x509.CertificateBuilder().subject_name(subject).issuer_name(ca.subject)
            .public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
            .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]), critical=False)
            .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH, ExtendedKeyUsageOID.CLIENT_AUTH]), critical=False)
            .sign(authority, hashes.SHA256()))
        (target / 'cert.pem').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        (target / 'key.pem').write_bytes(key.private_bytes(serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        for path in (target, *target.iterdir()):
            os.chown(path, 4021, 4021)
            path.chmod(0o700 if path.is_dir() else 0o600)


def setup(native, chain):
    assert chain.rpc('eth_chainId', []) == '0x7d'
    settlement = environment_file(native / 'settlement.env')
    custody = json.loads(Path(os.environ['LAYERX_TEST_CUSTODY_FILE']).read_text())
    artifacts = Path(os.environ['LAYERX_TEST_CUSTODY_ARTIFACTS'])
    bond, registry, vault = settlement['LAYERX_NODE_SETTLEMENT_CONTRACT'], settlement['LAYERX_NODE_CHECKPOINT_REGISTRY'], custody['vault']
    manager = chain.deploy(artifact(artifacts, 'CheckpointChallengeManager'),
        'constructor(address,address,address,address,uint64,uint128,bytes32,uint192)',
        [registry, bond, chain.account.address, chain.account.address, '3600', '1', COMMON.word('a5'), str(1 << 128)])
    chain.send(bond, 'setSlashingAuthority(address)', manager)
    govern(chain, custody['timelock'], vault, 'setGuarantorBond(address)', bond)
    deposit_authority = ed25519.Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32).public_key().public_bytes_raw()
    govern(chain, custody['timelock'], vault, 'setDepositRootAuthority(bytes32)', '0x' + deposit_authority.hex())
    asset = COMMON.run('cast', 'keccak', 'USDL')
    govern(chain, custody['timelock'], custody['registry'],
        'registerAsset(bytes32,address,uint8,uint128,uint128)', asset, COMMON.USDL, '6', '1', str(2 ** 128 - 1))
    chain.send(COMMON.USDL, 'mint(address,uint256)', chain.account.address, '1000')
    chain.send(COMMON.USDL, 'approve(address,uint256)', vault, '1000')
    chain.send(vault, 'deposit(bytes32,uint256,bytes32)', asset, '1000', custody['beneficiary'])
    for target, signature, arguments, expected in (
        (COMMON.USDL, 'balanceOf(address)', (vault,), 1000),
        (vault, 'totalCustodied(bytes32)', (asset,), 1000),
        (bond, 'custodiedValue()', (), 1000),
        (bond, 'minimumBond()', (), 100),
    ):
        assert int(chain.view(target, signature, *arguments), 16) == expected
    submitter = Account.create()
    submitter_file = native / 'handover-submitter.key'
    descriptor = os.open(submitter_file, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        output.write('0x' + submitter.key.hex())
    os.chown(submitter_file, 4021, 4021)
    chain.transaction('0x', submitter.address, value=10 ** 20)
    return submitter


def run(native, build, exports, count, lni_socket):
    chain = from_environment(os.environ['LAYERX_TEST_WITHDRAW_RPC'])
    assert chain.rpc('eth_chainId', []) == '0x7d' and count >= 3
    settlement = environment_file(native / 'settlement.env')
    custody = json.loads(Path(os.environ['LAYERX_TEST_CUSTODY_FILE']).read_text())
    bond, registry, vault = settlement['LAYERX_NODE_SETTLEMENT_CONTRACT'], settlement['LAYERX_NODE_CHECKPOINT_REGISTRY'], custody['vault']
    assert int(chain.view(bond, 'custodiedValue()'), 16) == 1000
    assert int(chain.view(bond, 'minimumBond()'), 16) == 100
    assert chain.view(vault, 'guarantorBond()')[-40:].lower() == bond[2:].lower()
    deposit_authority = ed25519.Ed25519PrivateKey.from_private_bytes(bytes([0x77]) * 32).public_key().public_bytes_raw()
    assert bytes.fromhex(chain.view(vault, 'depositRootAuthority()')[2:]) == deposit_authority
    output = native / 'handover-peers'
    output.mkdir(mode=0o755)
    output.chmod(0o755)
    inputs = output / 'publication-inputs'
    inputs.mkdir(mode=0o750)
    os.chown(inputs, 0, 4021)
    inputs.chmod(0o750)
    for batch in range(1, count + 1):
        authorize(json.loads((exports / f'{batch}.json').read_text()), inputs,
            bytes.fromhex(chain.account.address[2:]), vault)
    members = []
    for index in (1, 2):
        key = ec.derive_private_key(index, ec.SECP256K1())
        public = key.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.CompressedPoint)
        members.append(dict(guarantor_id='0x' + index.to_bytes(32, 'big').hex(), public_key='0x' + public.hex(),
            signer=Account.from_key(index.to_bytes(32, 'big')).address))
    domain = json.loads((ROOT / 'contracts/config/checkpoint-settlement.json').read_text())
    domain['settlement_domains']['beta'] = dict(protocol_version=3, paxeer_chain_id=125, network_id=77,
        settlement_contract=registry, guarantor_bond=bond, guarantor_set=members)
    configuration = output / 'checkpoint-settlement.json'
    configuration.write_text(json.dumps(domain))
    configuration.chmod(0o644)
    shared = output / 'submitter-lock'
    shared.mkdir(mode=0o700)
    os.chown(shared, 4021, 4021)
    with tempfile.TemporaryDirectory(prefix='lxp-handover-peer-support-') as temporary:
        support = Path(temporary)
        support.chmod(0o755)
        binary = support / 'layerx-guarantor'
        shutil.copy2(build / 'bin/layerx-guarantor', binary)
        binary.chmod(0o755)
        for name in ('settlement.py', 'publication.py'):
            shutil.copy2(ROOT / 'cmd/layerx-guarantor' / name, support / name)
            (support / name).chmod(0o644)
        python = Path(sys.executable)
        if sys.prefix != sys.base_prefix:
            shutil.copytree(sys.prefix, support / 'venv', symlinks=True)
            python = support / 'venv/bin' / Path(sys.executable).name
        tls_files(support / 'tls')
        submitter_file = native / 'handover-submitter.key'
        submitter = Account.from_key(submitter_file.read_text().strip())
        ports = [COMMON.free_port(), COMMON.free_port()]
        configurations = []
        for index in (1, 2):
            identity = native / f'guarantor-{index}/identity'
            for name, source in (('identities.txt', native / 'data/identities.txt'),
                                  ('genesis.registration', native / 'data/genesis/genesis.registration')):
                (identity / name).write_bytes(source.read_bytes())
                os.chown(identity / name, 0, 4021)
                (identity / name).chmod(0o440)
            state = output / f'producer-{index}'
            state.mkdir(mode=0o700)
            os.chown(state, 4021, 4021)
            key = ec.derive_private_key(index, ec.SECP256K1())
            key_file = support / f'guarantor-{index}.key'
            key_file.write_bytes(key.private_bytes(serialization.Encoding.PEM,
                serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
            os.chown(key_file, 4021, 4021)
            key_file.chmod(0o400)
            for batch in range(1, count - 1):
                source = native / f'handover-finalize/certificate-{batch}/guarantor-{index}.attestation'
                target = state / f'{batch:020d}-{index:064x}.attestation'
                shutil.copyfile(source, target)
                os.chown(target, 4021, 4021)
                target.chmod(0o600)
            env = os.environ | environment_file(identity / 'producer.env') | settlement
            env.update(LAYERX_GUARANTOR_ID=members[index - 1]['guarantor_id'],
                LAYERX_NODE_SNAPSHOT=str(identity / 'genesis.lxs'),
                LAYERX_NODE_GENESIS_MANIFEST=str(identity / 'genesis.manifest'),
                LAYERX_NODE_GENESIS_REGISTRATION=str(identity / 'genesis.registration'),
                LAYERX_NODE_IDENTITIES=str(identity / 'identities.txt'),
                LAYERX_GUARANTOR_NODE_CONFIG=str(identity / 'node.conf'),
                LAYERX_GUARANTOR_KEY_FILE=str(key_file), LAYERX_GUARANTOR_STATE_DIR=str(state),
                LAYERX_GUARANTOR_LNI_SOCKET=lni_socket, LAYERX_GUARANTOR_SETTLEMENT_FILE=str(configuration),
                LAYERX_GUARANTOR_SETTLEMENT_DOMAIN='beta', LAYERX_GUARANTOR_SUBMITTER_KEY_FILE=str(submitter_file),
                LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE=str(shared / 'submitter.lock'),
                LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR=str(inputs), LAYERX_GUARANTOR_PYTHON=str(python),
                LAYERX_GUARANTOR_SETTLEMENT_HELPER=str(support / 'settlement.py'),
                LAYERX_GUARANTOR_LISTEN_PORT=str(ports[index - 1]),
                LAYERX_GUARANTOR_PEER_URL=f'https://127.0.0.1:{ports[2 - index]}',
                LAYERX_GUARANTOR_TLS_CA_FILE=str(support / 'tls/ca.pem'),
                LAYERX_GUARANTOR_TLS_CERT_FILE=str(support / f'tls/{index}/cert.pem'),
                LAYERX_GUARANTOR_TLS_KEY_FILE=str(support / f'tls/{index}/key.pem'))
            configurations.append(env)
        processes, logs = [], []
        def stop_processes():
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
            for log in logs:
                log.close()
            processes.clear()
            logs.clear()
        try:
            submitted = None
            for attempt in range(2):
                suffix = '' if attempt == 0 else '-restart'
                for index, env in enumerate(configurations, 1):
                    log = (output / f'producer-{index}{suffix}.log').open('wb')
                    logs.append(log)
                    processes.append(subprocess.Popen(['setpriv', '--reuid=4021', '--regid=4021',
                        '--clear-groups', str(binary)], cwd=support, env=env, stdout=log, stderr=log))
                deadline = time.monotonic() + 180
                while time.monotonic() < deadline:
                    assert all(process.poll() is None for process in processes), 'live handover guarantor exited'
                    completed = True
                    for index in (1, 2):
                        lines = (output / f'producer-{index}{suffix}.log').read_text().splitlines()
                        completed &= (output / f'producer-{index}/{count:020d}.finality').is_file() and (
                            f'registered batch={count}' in lines or f'observed registration batch={count}' in lines)
                    if completed:
                        break
                    time.sleep(.2)
                else:
                    raise AssertionError('live handover guarantor finality deadline')
                for batch in range(1, count + 1):
                    assert int(chain.view(registry, 'checkpointAtBatch(uint64)', str(batch)), 16) != 0
                for index in (1, 2):
                    lines = (output / f'producer-{index}{suffix}.log').read_text().splitlines()
                    assert f'attested batch={count - 1}' in lines and f'attested batch={count}' in lines
                nonce = chain.rpc('eth_getTransactionCount', [submitter.address, 'latest'])
                if attempt == 0:
                    submitted = nonce
                else:
                    assert nonce == submitted, 'guarantor restart repeated a settled transaction'
                stop_processes()
            print('two live bonded guarantors replayed both epochs, finalized replacement-key batches, and rebuilt historical signer trust after restart', flush=True)
        finally:
            stop_processes()
