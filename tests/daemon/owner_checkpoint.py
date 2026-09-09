import datetime
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import ssl
import sys
import time
import urllib.request
import urllib.parse

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ed25519, rsa
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID
from deploy_local_custody import send, calldata, deploy, govern
from custody_credit import eth_hash
from owner_native import Reader, digest


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def decode_header(settlement, encoded):
    value = settlement.raw(encoded)
    assert len(value) == 354 and value[2:5] == b'\x17\x01\x0f'
    result, offset = [], 5
    for index, kind in enumerate(settlement.HEADER_TYPES, 1):
        assert value[offset] == index
        offset += 1
        if kind == 'bytes32':
            assert int.from_bytes(value[offset:offset + 4], 'big') == 32
            offset += 4
            result.append(value[offset:offset + 32])
            offset += 32
        else:
            width = int(kind[4:]) // 8
            result.append(int.from_bytes(value[offset:offset + width], 'big'))
            offset += width
    assert offset == len(value) and result[0].to_bytes(2, 'big') == value[:2]
    return result


def authorize_publication(export, inputs, recipient, vault, signing_keys, deposit_key, settlement, publication):
    header = decode_header(settlement, export['canonical_header'])
    checkpoint_id = settlement.checkpoint_hash(header, b'')
    encoded_header = [publication.hx(value) if isinstance(value, bytes) else value for value in header]
    request = dict(chain_id=31337, header=encoded_header, checkpoint_id=publication.hx(checkpoint_id),
        native_facts=export['native_facts'])
    balances, _, deposits, profile = publication.native_request(settlement, request, header, checkpoint_id)
    authorities = {key.public_key().public_bytes(serialization.Encoding.Raw,
        serialization.PublicFormat.Raw): key for key in signing_keys}
    bindings = []
    for fact in balances:
        key = authorities.get(fact['authority'])
        assert key is not None, 'settlement balance authority unavailable'
        message = (b'LX:SETTLE:RECIPIENT:v1\0' + header[1].to_bytes(4, 'big') + fact['account'] +
            fact['asset'] + recipient + checkpoint_id)
        bindings.append(dict(account=publication.hx(fact['account']), asset=publication.hx(fact['asset']),
            recipient=publication.hx(recipient), request_anchor=publication.hx(checkpoint_id),
            signature=publication.hx(key.sign(message))))
    deposit_registration = None
    if deposits:
        assert profile is not None and profile[13:33] == bytes.fromhex(vault[2:])
        reference = bytes(12) + bytes.fromhex(vault[2:])
        ordering = []
        for fact in deposits:
            leaf = (b'LX:PAXEER:DEPOSIT:LEAF:v1' + fact['identity'] + reference + fact['asset'] +
                fact['amount'] + checkpoint_id + header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))
            ordering.append(publication.sha(b'LXP/v1/merkle-leaf\0' + leaf))
        level = list(ordering)
        while len(level) > 1:
            level = [publication.sha(b'LXP/v1/merkle-internal\0' + level[index] +
                level[min(index + 1, len(level) - 1)]) for index in range(0, len(level), 2)]
        registration = (b'LX:PAXEER:DEPOSIT:ROOT:v1' + checkpoint_id + header[7] + level[0] +
            reference + header[1].to_bytes(4, 'big') + header[0].to_bytes(2, 'big'))
        deposit_registration = dict(vault=vault, custody_reference=publication.hx(reference),
            signature=publication.hx(deposit_key.sign(registration)))
    document = dict(version=2, checkpoint_id=publication.hx(checkpoint_id), recipient_bindings=bindings,
        deposit_registration=deposit_registration)
    destination = inputs / (checkpoint_id.hex() + '.json')
    publication.atomic_json(destination, document)
    os.chown(destination, 0, 4021)
    destination.chmod(0o440)


def checkpoint(work, public, settlement, rpc, account, launch, ca_key, ca_cert, last_batch):
    repo = Path(__file__).resolve().parents[2]
    bond = settlement['LAYERX_NODE_SETTLEMENT_CONTRACT']
    registry = settlement['LAYERX_NODE_CHECKPOINT_REGISTRY']
    usdl = '0x85FcD13735F4309833A503EE804ea32395851479'
    send(rpc, account, usdl, calldata('mint(address,uint256)', account, 2000))
    send(rpc, account, usdl, calldata('approve(address,uint256)', bond, 2000))
    members = []
    for index, prefix in enumerate(('LAYERX_NODE_GENESIS_GUARANTOR', 'LAYERX_NODE_SECOND_GUARANTOR'), 1):
        from cryptography.hazmat.primitives.asymmetric import ec
        key = bytes.fromhex(public[prefix + '_PUBLIC_KEY'])
        unpacked = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256K1(), key).public_bytes(
            serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
        signer = '0x' + eth_hash(unpacked[1:])[-20:].hex()
        identifier = '0x' + public[prefix + '_ID']
        send(rpc, account, bond, calldata('activateGuarantor(bytes32,address,address,uint64,uint64)', identifier, signer, account, 1, index))
        send(rpc, account, bond, calldata('depositBond(bytes32,uint256)', identifier, 1000))
        members.append(dict(guarantor_id=identifier, public_key='0x' + key.hex(), signer=signer))
    domain = json.loads((repo / 'contracts/config/checkpoint-settlement.json').read_text())
    domain['settlement_domains']['beta'] = dict(protocol_version=3, paxeer_chain_id=31337,
        network_id=77, settlement_contract=registry, guarantor_bond=bond,
        guarantor_set=sorted(members, key=lambda member: member['guarantor_id']))
    config = work / 'checkpoint-settlement.json'
    config.write_text(json.dumps(domain))
    config.chmod(0o644)
    custody = json.loads((work / 'human-evidence-input/owner-custody.json').read_text())
    deposit_key = ed25519.Ed25519PrivateKey.from_private_bytes((work / 'attestor.seed').read_bytes())
    deposit_public = deposit_key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    assert (work / 'human-evidence-input/custody.profile').read_bytes()[65:97] == deposit_public
    manager = deploy(rpc, account, 'contracts/challenge/CheckpointChallengeManager.sol:CheckpointChallengeManager',
        registry, bond, account, account, 3600, 1, '0x' + 'ac' * 32, 1)
    send(rpc, account, bond, calldata('setSlashingAuthority(address)', manager))
    for signature, value in (('setGuarantorBond(address)', bond),
                             ('setDepositRootAuthority(bytes32)', '0x' + deposit_public.hex())):
        operation = calldata(signature, value)
        govern(rpc, account, custody['timelock'], custody['timelock'],
            calldata('setCallPermission(address,bytes4,bool)', custody['vault'], operation[:10], 'true'))
        govern(rpc, account, custody['timelock'], custody['vault'], operation)
    assert rpc.call('eth_call', [dict(to=bond, data=calldata('slashingAuthority()')), 'latest'])[-40:].lower() == manager[2:].lower()
    assert rpc.call('eth_call', [dict(to=custody['vault'], data=calldata('guarantorBond()')), 'latest'])[-40:].lower() == bond[2:].lower()
    assert bytes.fromhex(rpc.call('eth_call', [dict(to=custody['vault'],
        data=calldata('depositRootAuthority()')), 'latest'])[2:]) == deposit_public
    inputs = work / 'checkpoint-publication-inputs'
    inputs.mkdir(mode=0o750)
    os.chown(inputs, 0, 4021)
    inputs.chmod(0o750)
    exports = work / 'checkpoint-publication-exports'
    exports.mkdir(mode=0o700)
    settlement_codec = load('owner_checkpoint_settlement', repo / 'cmd/layerx-guarantor/settlement.py')
    publication = load('owner_checkpoint_publication', repo / 'cmd/layerx-guarantor/publication.py')
    signing_keys = [ed25519.Ed25519PrivateKey.from_private_bytes((work / path).read_bytes())
        for path in ('human-owner/owner.seed', 'human-owner/pending.seed', 'treasury.seed')]
    recipient = bytes.fromhex(account[2:])
    support = work / 'checkpoint-support'
    support.mkdir(mode=0o755)
    support.chmod(0o755)
    helper = support / 'helper'
    helper.mkdir(mode=0o755)
    helper.chmod(0o755)
    for name in ('settlement.py', 'publication.py'):
        shutil.copy2(repo / 'cmd/layerx-guarantor' / name, helper / name)
    python = Path(sys.executable)
    if sys.prefix != sys.base_prefix:
        virtualenv = support / 'venv'
        shutil.copytree(sys.prefix, virtualenv, symlinks=True)
        python = virtualenv / 'bin' / Path(sys.executable).name
    os.chown(work / 'payer.key', 4021, 4021)
    ports = []
    for _ in range(2):
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            ports.append(sock.getsockname()[1])
    producers = []
    for index in range(1, 3):
        identity = work / f'guarantor-{index}/identity'
        for name, source in [('identities.txt', work / 'node/identities.txt'),
                             ('genesis.registration', work / 'node/genesis/genesis.registration')]:
            (identity / name).write_bytes(source.read_bytes())
            os.chown(identity / name, 0, 4021)
            (identity / name).chmod(0o440)
        state = work / f'checkpoint-producer-{index}'
        state.mkdir(mode=0o700)
        os.chown(state, 4021, 4021)
        tls = work / f'checkpoint-tls-{index}'
        tls.mkdir(mode=0o700)
        tls_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        now = datetime.datetime.now(datetime.timezone.utc)
        name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, f'guarantor-{index}')])
        import ipaddress
        cert = (x509.CertificateBuilder().subject_name(name).issuer_name(ca_cert.subject)
                .public_key(tls_key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now - datetime.timedelta(minutes=1)).not_valid_after(now + datetime.timedelta(days=1))
                .add_extension(x509.SubjectAlternativeName([x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]), critical=False)
                .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH, ExtendedKeyUsageOID.CLIENT_AUTH]), critical=False)
                .sign(ca_key, hashes.SHA256()))
        (tls / 'cert.pem').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        (tls / 'ca.pem').write_bytes(ca_cert.public_bytes(serialization.Encoding.PEM))
        (tls / 'key.pem').write_bytes(tls_key.private_bytes(serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        for path in [tls, *tls.iterdir()]:
            os.chown(path, 4021, 4021)
            path.chmod(0o700 if path.is_dir() else 0o600)
        generated = dict(line.split('=', 1) for line in (identity / 'producer.env').read_text().splitlines())
        env = dict(os.environ, **generated)
        env.update(settlement)
        env.update(LAYERX_NODE_SNAPSHOT=str(identity / 'genesis.lxs'),
            LAYERX_NODE_GENESIS_MANIFEST=str(identity / 'genesis.manifest'),
            LAYERX_NODE_GENESIS_REGISTRATION=str(identity / 'genesis.registration'),
            LAYERX_NODE_IDENTITIES=str(identity / 'identities.txt'),
            LAYERX_GUARANTOR_NODE_CONFIG=str(identity / 'node.conf'),
            LAYERX_GUARANTOR_KEY_FILE=str(identity / 'key.pem'), LAYERX_GUARANTOR_STATE_DIR=str(state),
            LAYERX_GUARANTOR_LNI_SOCKET=str(work / 'run/layerxd.lni.sock'),
            LAYERX_GUARANTOR_SETTLEMENT_FILE=str(config), LAYERX_GUARANTOR_SETTLEMENT_DOMAIN='beta',
            LAYERX_GUARANTOR_SUBMITTER_KEY_FILE=str(work / 'payer.key'),
            LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE=str(work / 'tls/submitter.lock'),
            LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR=str(inputs),
            LAYERX_GUARANTOR_PYTHON=str(python),
            LAYERX_GUARANTOR_SETTLEMENT_HELPER=str(helper / 'settlement.py'),
            LAYERX_GUARANTOR_LISTEN_PORT=str(ports[index - 1]),
            LAYERX_GUARANTOR_PEER_URL=f'https://127.0.0.1:{ports[2 - index]}',
            LAYERX_GUARANTOR_TLS_CA_FILE=str(tls / 'ca.pem'),
            LAYERX_GUARANTOR_TLS_CERT_FILE=str(tls / 'cert.pem'), LAYERX_GUARANTOR_TLS_KEY_FILE=str(tls / 'key.pem'))
        if index == 1:
            invalid_state = work / 'checkpoint-invalid-settlement'
            invalid_state.mkdir(mode=0o700)
            os.chown(invalid_state, 4021, 4021)
            invalid_env = dict(env, LAYERX_GUARANTOR_STATE_DIR=str(invalid_state),
                LAYERX_NODE_SETTLEMENT_CONTRACT='0x' + (int(bond, 16) ^ 1).to_bytes(20, 'big').hex())
            invalid = launch(['setpriv', '--reuid=4021', '--regid=4021',
                '--groups=' + str(repo.stat().st_gid), str(repo / 'build/bin/layerx-guarantor'), '--once'],
                'checkpoint-invalid-settlement', invalid_env)
            assert invalid.wait(timeout=15) != 0, 'mismatched settlement binding accepted'
            invalid_log = (work / 'checkpoint-invalid-settlement.log').read_text()
            assert 'settlement refusal: settlement bond environment mismatch' in invalid_log
            assert 'configuration refused (-213)' in invalid_log
            print('mismatched settlement binding refused with -213', flush=True)
        replay_state = work / f'governance-replay-{index}'
        replay_state.mkdir(mode=0o700)
        os.chown(replay_state, 4021, 4021)
        replay_log = work / f'governance-replay-{index}.log'
        replay_env = dict(env)
        if index == 1:
            replay_env['LAYERX_TEST_PUBLICATION_EXPORT_DIR'] = str(exports)
        with replay_log.open('wb') as output:
            subprocess.run([str(repo / 'build/tests/lxp_test_guarantor_runtime'),
                str(identity / 'node.conf'), str(replay_state),
                str(work / 'node/checkpoints/da-bodies.log'), str(last_batch)],
                env=replay_env, stdout=output, stderr=output, check=True, timeout=120)
        if index == 1:
            for batch in range(1, last_batch + 1):
                authorize_publication(json.loads((exports / f'{batch}.json').read_text()), inputs,
                    recipient, custody['vault'], signing_keys, deposit_key, settlement_codec, publication)
        print(f'bonded guarantor {index} independently replayed all {last_batch} owner batches', flush=True)
        producers.append(launch(['setpriv', '--reuid=4021', '--regid=4021',
            '--groups=' + str(repo.stat().st_gid), str(repo / 'build/bin/layerx-guarantor')], f'checkpoint-producer-{index}', env))
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        assert all(process.poll() is None for process in producers), 'real guarantor producer refused'
        if all((work / f'checkpoint-producer-{index}' / f'{last_batch:020d}.finality').exists() for index in (1, 2)):
            break
        time.sleep(0.2)
    else:
        raise AssertionError('real checkpoint producer deadline')
    for batch in range(1, last_batch + 1):
        result = rpc.call('eth_call', [dict(to=registry,
            data=calldata('checkpointAtBatch(uint64)', batch)), 'latest'])
        assert int(result, 16) != 0, f'checkpoint batch {batch} not registered'
    print(f'real bonded guarantors independently replayed and registered all {last_batch} checkpoint certificates', flush=True)


def hosted(work, config, environment, launch, service):
    root = work / 'human-evidence-input'
    context = ssl.create_default_context(cafile=config['authority_ca_file'])
    general_token = Path(config['authority_token_file']).read_text()
    authority_root = Path(config['authority_state_root'])
    for path in sorted((root / 'governance-run').glob('*.receipt')):
        raw = path.read_bytes()
        reader = Reader(raw, path)
        reader.take(6)
        activity = reader.span(32)
        reader.take(8)
        for _ in range(3):
            reader.span(32)
        reader.take(4)
        for _ in range(reader.number(4)):
            reader.take(8)
            reader.span(32)
            reader.span(256)
        reader.take(16)
        batch = reader.span(32)
        receipt_digest = digest(b'receipt', raw[:-69] + b'\0')
        query = config['authority_url'] + '/v1/batches/' + batch.hex() + '/receipt-authority?receipt_digest=' + receipt_digest.hex()
        request = urllib.request.Request(query, headers={'Authorization': 'Bearer ' + general_token})
        with urllib.request.urlopen(request, context=context, timeout=15) as response:
            document = json.load(response)
        record = authority_root / (activity.hex() + '.json')
        record.write_text(json.dumps(dict(receipt_hex=raw.hex(), replica_document=document), sort_keys=True, separators=(',', ':')))
        os.chown(record, 4021, 4021)
        record.chmod(0o600)
    registration = json.loads((root / 'owner-registration.json').read_text())
    session = json.loads((root / 'governance-run/session.json').read_text())
    raw = (root / 'governance-run/session.receipt').read_bytes()
    reader = Reader(raw, 'session')
    reader.take(6)
    reference = dict(activity_id=reader.span(32).hex(), receipt_digest=digest(b'receipt', raw[:-69] + b'\0').hex())
    identity = registration['identity']
    identity['evidence'] = reference
    scope = dict(authority=registration['authority'], action_key=session['action_key'], capability_id=session['grant_id'],
        activity_types=[5], counterparties=[], assets=[], amount_ceiling='0', expiry_sequence=session['expiry_sequence'],
        enforceable_dimensions=[], evidence=reference)
    identity['capabilities'] = [scope]
    policy = dict(principals=[dict(tenant='owner-production', principal='owner', account_id=registration['owner_account'],
        asset_id='01' * 32, activities=sorted(path.stem for path in authority_root.glob('*.json')), budgets=[],
        maximum_age_seconds=3600, maximum_age_sequences=10000, identities=[identity])])
    registry_path = root / 'module-registry.json'
    repo = Path(__file__).resolve().parents[2]
    bootstrap = (repo / 'platform/hosted/node/bootstrap.sh').read_text().splitlines()
    metadata = dict(line.split('=', 1) for line in bootstrap if line.startswith(('ASSET_SYMBOL=', 'ASSET_CURRENCY=', 'ASSET_DECIMALS=')))
    with registry_path.open('wb') as output:
        subprocess.run([str(repo / 'build/bin/layerx-module-registry'), 'generate', '--network-id', '77', '--protocol-version', '3',
            '--asset', policy['principals'][0]['asset_id'], '--symbol', metadata['ASSET_SYMBOL'], '--currency', metadata['ASSET_CURRENCY'],
            '--decimals', metadata['ASSET_DECIMALS'], '--custody-profile', str(root / 'custody.profile')], stdout=output, check=True)
    registry_path.chmod(0o600)
    os.chown(registry_path, 4021, 4021)
    policy_path = root / 'checkpoint-policy.json'
    policy_path.write_text(json.dumps(policy))
    human_token = root / 'human-authority.token'
    human_token.write_text(os.urandom(32).hex())
    for path in (policy_path, human_token):
        path.chmod(0o600)
        os.chown(path, 4021, 4021)
    service.terminate()
    assert service.wait(timeout=15) == 0
    env = dict(environment, LAYERX_AUTHORITY_HUMAN_AGENT_TOKEN_FILE=str(human_token),
        LAYERX_AUTHORITY_HUMAN_AGENT_TENANT='owner-production', LAYERX_AUTHORITY_HUMAN_AGENT_PRINCIPAL='owner',
        LAYERX_AUTHORITY_PRINCIPAL_POLICY_FILE=str(policy_path), LAYERX_AUTHORITY_MODULE_REGISTRY_FILE=str(registry_path),
        LAYERX_AUTHORITY_CORE_CLOCK_HORIZON='100', LAYERX_AUTHORITY_STATE_ROOT=str(authority_root))
    server = launch(service.args, 'authority-checkpoint', env)
    params = dict(tenant='owner-production', principal='owner', did=identity['did'])

    def query(route, extra):
        request = urllib.request.Request(config['authority_url'] + '/v1/agent/' + route + '?' + urllib.parse.urlencode(dict(params, **extra)),
            headers={'Authorization': 'Bearer ' + human_token.read_text()})
        with urllib.request.urlopen(request, context=context, timeout=30) as response:
            return json.load(response)

    deadline = time.monotonic() + 30
    while True:
        assert server.poll() is None, 'Human checkpoint authority startup failed'
        try:
            result = query('capability-scope', {key: scope[key] for key in ('authority', 'action_key', 'capability_id')})
            break
        except (OSError, urllib.error.HTTPError):
            if time.monotonic() >= deadline:
                raise
            time.sleep(0.2)
    assert result['verification'] == 4 and result['expiry_sequence'] == session['expiry_sequence']
    assert result['action_key'] == session['action_key'] and result['canonical_core_bytes'] == session['summary']
    assert query('identity', {})['verification_level'] == 'checkpoint_finalised'
    for recovery in ('true', 'false'):
        assert query('key-policy', dict(recovery=recovery))['verification'] == 4
    for field in ('authority', 'action_key', 'capability_id'):
        altered = {key: scope[key] for key in ('authority', 'action_key', 'capability_id')}
        altered[field] = os.urandom(32).hex()
        try:
            query('capability-scope', altered)
            raise AssertionError('mismatched capability binding accepted')
        except urllib.error.HTTPError as error:
            assert error.code == 403
    print('real checkpoint-finalised identity, key policies and committed capability action/expiry verified positively; mismatched bindings refused', flush=True)
