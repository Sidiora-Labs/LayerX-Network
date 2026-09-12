import hashlib
import json
import os
from pathlib import Path
import re
import socket
import ssl
import stat
import struct
import subprocess
import time
import urllib.request
import urllib.parse

from provision import Refused, fields, h32, protected_bytes, protected_json, require, uint, write_json

GUARDIAN_ROLES = ('guarantor-1', 'guarantor-2', 'sequencer')
GUARDIAN_EPOCH_MAXIMUM = 64


def digest(domain, data):
    return hashlib.sha256(b'LXP/v1/' + domain + b'\0' + data).digest()


def span(data):
    return struct.pack('>I', len(data)) + data


class Reader:
    def __init__(self, data, path):
        self.data, self.offset, self.path = data, 0, path

    def take(self, count):
        require(0 <= count <= len(self.data) - self.offset, self.path, 'truncated native encoding')
        result = self.data[self.offset:self.offset + count]
        self.offset += count
        return result

    def number(self, count):
        return int.from_bytes(self.take(count), 'big')

    def span(self, maximum):
        length = self.number(4)
        require(length <= maximum, self.path, 'native field bound')
        return self.take(length)

    def finish(self):
        require(self.offset == len(self.data), self.path, 'native trailing bytes')


def protected_write(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())


def receipt(socket_path, activity_id):
    def read_exact(connection, size):
        data = bytearray()
        while len(data) < size:
            part = connection.recv(size - len(data))
            require(bool(part), socket_path, 'receipt connection closed')
            data.extend(part)
        return bytes(data)

    def exchange(connection, tag, correlation, payload):
        envelope = struct.pack('>HHHQ', 1, 4, tag, correlation) + span(payload) + span(b'')
        connection.sendall(span(envelope))
        length = int.from_bytes(read_exact(connection, 4), 'big')
        require(22 <= length <= 1212416, socket_path, 'receipt frame bound')
        reader = Reader(read_exact(connection, length), socket_path)
        require(reader.take(4) == b'\0\1\0\4', socket_path, 'LNI version')
        returned_tag = reader.number(2)
        require(reader.number(8) == correlation, socket_path, 'LNI correlation')
        data = reader.span(1212416)
        require(not reader.span(1212416), socket_path, 'unexpected LNI proof')
        reader.finish()
        require(returned_tag == tag + 1, socket_path, 'LNI refusal')
        return data

    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(10)
        connection.connect(socket_path)
        exchange(connection, 1, 0, b'')
        for attempt in range(100):
            data = exchange(connection, 5, attempt + 1, b'\1' + activity_id)
            if data:
                return data
            time.sleep(0.1)
    raise Refused(f'{socket_path}: committed receipt unavailable; do not resubmit with a new idempotency key')


def receipt_fields(data, public_key, path):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    r = Reader(data, path)
    require(r.take(6) == b'\0\3\x52\1\0\3', path, 'protocol 3 receipt')
    activity = r.span(32)
    sequence = r.number(8)
    previous, resulting, activity_root = r.span(32), r.span(32), r.span(32)
    require(all(len(v) == 32 for v in (activity, previous, resulting, activity_root)), path, 'receipt hashes')
    require(r.number(4) == 0, path, 'successful native execution')
    count = r.number(4)
    require(count <= 512, path, 'effect count')
    effects = []
    for ordinal in range(count):
        module, index, event = r.number(2), r.number(2), r.number(2)
        kind, monetary, root, body = r.number(1), r.number(1), r.span(32), r.span(256)
        require(index == ordinal and kind <= 3 and monetary <= 1 and len(root) == 32, path, 'effect encoding')
        effects.append((module, event, kind, monetary, body))
    r.take(16)
    batch = r.span(32)
    module, version = r.number(2), r.number(4)
    r.take(4)
    operation = r.number(1)
    asset = r.span(32)
    amount = r.number(16)
    source = r.span(32)
    r.take(40)
    target = r.span(32)
    r.take(32)
    for _ in range(3):
        require(len(r.span(32)) == 32, path, 'receipt commitment')
    timestamp = r.number(8)
    unsigned = data[:r.offset] + b'\0'
    require(r.number(1) == 1, path, 'sequencer signature')
    signature = r.span(64)
    r.finish()
    receipt_digest = digest(b'receipt', unsigned)
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, receipt_digest)
    except Exception as error:
        raise Refused(f'{path}: native receipt signature refused') from error
    if module == 8:
        credits = [body for emitted_module, event, kind, monetary, body in effects
                   if emitted_module == 8 and event == 1 and kind == 3 and not monetary]
        balances = [body for emitted_module, event, kind, monetary, body in effects
                    if emitted_module == 8 and event == 2 and kind == 3 and not monetary]
        require(operation == 0 and amount == 0 and target == bytes(32) and asset == bytes(32)
                and len(credits) == 1 and len(credits[0]) == 208
                and len(balances) == 1 and len(balances[0]) == 112, path, 'canonical custody credit events')
        event, balance = credits[0], balances[0]
        asset, target, amount = event[32:64], event[64:96], int.from_bytes(event[96:112], 'big')
        require(amount > 0 and int.from_bytes(event[176:192], 'big') + amount == int.from_bytes(event[192:208], 'big')
                and balance[32:48] == balance[48:64]
                and int.from_bytes(balance[64:80], 'big') + amount == int.from_bytes(balance[80:96], 'big'),
                path, 'committed credit supply and balance conservation')
    return dict(activity_id=activity.hex(), receipt_digest=receipt_digest.hex(), sequence=sequence,
                batch=batch.hex(), module=module, version=version, operation=operation,
                asset=asset.hex(), amount=amount, source=source.hex(), target=target.hex(),
                timestamp=timestamp, effects=effects)


def guardian_binding_message(role, identity, epoch, public_key):
    return (b'LX:HUMAN:GUARDIAN:BINDING:v1\0' + struct.pack('>HH', epoch, len(role))
            + role.encode('ascii') + bytes.fromhex(identity) + public_key)


def guardian_rotation_message(role, identity, epoch, predecessor, successor):
    return (b'LX:HUMAN:GUARDIAN:ROTATION:v1\0' + struct.pack('>HH', epoch, len(role))
            + role.encode('ascii') + bytes.fromhex(identity) + predecessor + successor)


def guardian_commitment(threshold, keys):
    return hashlib.sha256(b'LX:HUMAN:RECOVERY:v1\0' + struct.pack('>HH', threshold, len(keys)) + b''.join(keys)).digest()


def guardian_signature(value, path, field):
    require(type(value) is str and re.fullmatch('[0-9a-f]{128}', value) is not None
            and int(value, 16) != 0, path, field)
    return bytes.fromhex(value)


def guardian_verified(public_key, signature, message, path, field):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(signature, message)
    except Exception as error:
        raise Refused(f'{path}: invalid {field}') from error


def guardian_enrollment_key(value, path):
    fields(value, 'role identity public_key epoch custody signature', path, 'guardian enrollment')
    require(value['role'] in GUARDIAN_ROLES, path, 'guardian role')
    h32(value['identity'], path, 'guardian operator identity')
    h32(value['public_key'], path, 'guardian public key')
    uint(value['epoch'], 16, path, 'guardian epoch', 1)
    require(value['epoch'] <= GUARDIAN_EPOCH_MAXIMUM, path, 'guardian epoch bound')
    require(type(value['custody']) is str and value['custody'].startswith('/'), path, 'guardian custody directory')
    custody = Path(value['custody'])
    require(custody.name == value['role'] + '-e' + str(value['epoch'])
            and custody.parent.name == 'human-guardians', path, 'guardian custody directory')
    public = bytes.fromhex(value['public_key'])
    guardian_verified(public, guardian_signature(value['signature'], path, 'guardian binding signature'),
                      guardian_binding_message(value['role'], value['identity'], value['epoch'], public),
                      path, 'guardian binding signature')
    return public


def guardian_member_key(member, path):
    fields(member, 'role identity public_key epoch custody signature rotation', path, 'guardian binding')
    public = guardian_enrollment_key({name: member[name] for name in member if name != 'rotation'}, path)
    rotation = member['rotation']
    if member['epoch'] == 1:
        require(rotation is None, path, 'genesis guardian rotation')
        return public
    fields(rotation, 'predecessor signature', path, 'guardian rotation')
    predecessor = rotation['predecessor']
    previous = guardian_member_key(predecessor, path)
    require(predecessor['role'] == member['role'] and predecessor['identity'] == member['identity']
            and predecessor['epoch'] == member['epoch'] - 1 and previous != public
            and predecessor['custody'] != member['custody'], path, 'guardian rotation chain')
    guardian_verified(previous, guardian_signature(rotation['signature'], path, 'guardian rotation signature'),
                      guardian_rotation_message(member['role'], member['identity'], member['epoch'], previous, public),
                      path, 'guardian rotation signature')
    return public


def guardian_set(document, path, threshold):
    fields(document, 'version threshold public_keys members', path, 'guardian set')
    uint(document['version'], 16, path, 'guardian set version', 1)
    require(document['threshold'] == threshold, path, 'guardian threshold binding')
    members, published = document['members'], document['public_keys']
    require(type(members) is list and type(published) is list and len(published) == len(members)
            and 0 < threshold <= len(members) <= len(GUARDIAN_ROLES), path, 'guardian threshold')
    keys, roles, identities, custody = [], set(), set(), set()
    for member in members:
        keys.append(guardian_member_key(member, path))
        roles.add(member['role'])
        identities.add(member['identity'])
        custody.add(member['custody'])
        require(member['epoch'] <= document['version'], path, 'guardian set version')
    require(len(roles) == len(identities) == len(custody) == len(set(keys)) == len(members),
            path, 'separately enrolled guardians')
    require(document['version'] == max(member['epoch'] for member in members), path, 'guardian set version')
    for key in published:
        h32(key, path, 'guardian public key')
    keys = sorted(keys)
    require(sorted(bytes.fromhex(key) for key in published) == keys, path, 'guardian public key manifest')
    return keys


def _produce(work_dir):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    root = Path(work_dir) / 'human-evidence-input'
    path = root / 'owner-native.json'
    config = protected_json(path)
    fields(config, 'node_socket network_id owner_seed_file pending_seed_file sequencer_public_key layerxctl fee_limit authority_url authority_token_file authority_ca_file authority_state_root', path, 'native producer configuration')
    uint(config['network_id'], 32, path, 'network_id', 1)
    uint(config['fee_limit'], 128, path, 'fee_limit')
    h32(config['sequencer_public_key'], path, 'sequencer_public_key')
    url = urllib.parse.urlsplit(config['authority_url'])
    require(url.scheme == 'https' and bool(url.hostname) and url.username is None and url.password is None
            and not url.query and not url.fragment, path, 'HTTPS receipt authority')
    require(Path(config['layerxctl']).is_absolute(), path, 'absolute layerxctl path')
    evidence_dir = Path(config['authority_state_root'])
    info = evidence_dir.lstat()
    require(evidence_dir.is_absolute() and evidence_dir.resolve() == evidence_dir and stat.S_ISDIR(info.st_mode)
            and info.st_uid == os.geteuid() and stat.S_IMODE(info.st_mode) == 0o700, evidence_dir, 'protected authority state directory')
    seed = protected_bytes(config['owner_seed_file'], 32)
    require(len(seed) == 32, config['owner_seed_file'], 'custody owner Ed25519 seed')
    signer = Ed25519PrivateKey.from_private_bytes(seed)
    public = signer.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    pending = protected_bytes(config['pending_seed_file'], 32)
    require(len(pending) == 32, config['pending_seed_file'], 'custody rotation Ed25519 seed')
    pending_public = Ed25519PrivateKey.from_private_bytes(pending).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    require(pending_public != public, config['pending_seed_file'], 'distinct rotation key')
    owner_path = Path(work_dir) / 'human-owner-result.json'
    owner = protected_json(owner_path)
    did = owner['did'].encode()
    require(0 < len(did) <= 255, owner_path, 'owner DID')
    did_id = digest(b'did-id', struct.pack('>H', len(did)) + did)
    name = b'agent:' + did + b':main'
    account = hashlib.sha256(b'LX:ACCOUNT:v1' + span(name)).digest()
    admission_path = root / 'owner-admission.json'
    admission = protected_json(admission_path)
    require(admission == dict(did=did.decode(), public_key=public.hex(), owner_account=account.hex()),
            admission_path, 'post-LXIP owner admission binding')
    policy_path = root / 'recovery-policy.json'
    policy = protected_json(policy_path)
    from provision import recovery_policy, owner_registration
    recovery_policy(policy, policy_path)
    require(owner['recovery_root'] == policy['root'] and owner['recovery_threshold'] == policy['threshold']
            and owner['recovery_delay_seconds'] == policy['delay_seconds'], owner_path, 'recovery binding')
    guardians_path = root / 'recovery-guardians.json'
    keys = guardian_set(protected_json(guardians_path), guardians_path, policy['threshold'])
    commitment = guardian_commitment(policy['threshold'], keys)
    require(list(commitment) == policy['root'], policy_path, 'guardian commitment')
    credit_path = root / 'custody-credit.bin'
    credit = protected_bytes(credit_path, 427)
    require(len(credit) == 427 and credit[:5] in (b'LXDC1', b'LXDC2') and credit[107:139] == account
            and credit[139:171] == public, credit_path, 'real custody credit beneficiary binding')
    if credit[:5] == b'LXDC2':
        require(credit[359:363] == b'\0\0\0\1' and
                2 <= int.from_bytes(credit[215:223], 'big') <= int.from_bytes(credit[287:295], 'big') < 8192,
                credit_path, 'Comet custody state evidence binding')
    output = root / 'owner-registration.json'
    require(not output.exists(), output, 'existing registration requires reconciliation')
    run_dir = root / 'owner-native-run'
    run_dir.mkdir(mode=0o700)
    common = ['--socket', config['node_socket'], '--network-id', str(config['network_id']),
              '--protocol-version', '3', '--actor', did.decode()]

    def command(arguments, label):
        completed = subprocess.run([config['layerxctl'], *arguments, *common], capture_output=True)
        if completed.returncode != 0:
            protected_write(run_dir / 'layerxctl-refusal.txt', completed.stderr)
        require(completed.returncode == 0, label, 'layerxctl refused or submission outcome unknown')
        return json.loads(completed.stdout)

    def execute(ordinal, payload, label, module=7):
        state = command(['read-state'], path)
        sequence = state['account_sequence']
        now = time.time_ns() // 1000000
        idempotency = hashlib.sha256(b'LX:DEPOSIT:NULLIFIER:v1' + payload[43:75]).digest() if module == 8 else os.urandom(32)
        fields_bytes = (b'\1' + struct.pack('>H', 3) + b'\2' + struct.pack('>I', config['network_id'])
            + b'\3' + struct.pack('>I', (module << 16) | ordinal) + b'\4' + span(did)
            + b'\5' + span(public) + b'\6' + struct.pack('>Q', sequence)
            + b'\7' + struct.pack('>QQ', now, now + 300000) + b'\10' + span(idempotency)
            + b'\11' + config['fee_limit'].to_bytes(16, 'big') + b'\12' + span(digest(b'payload-hash', payload))
            + b'\13' + span(payload))
        unsigned = b'\0\3\x10\1\13' + fields_bytes
        signed = b'\0\3\x10\1\14' + fields_bytes + b'\14' + span(signer.sign(digest(b'signature-preimage', unsigned)))
        activity_id = digest(b'activity-id', signed)
        activity_path = run_dir / (label + '.activity')
        protected_write(activity_path, signed)
        ack = command(['submit', '--public-key', public.hex(), '--activity', str(activity_path)], activity_path)
        require(ack['activity_id'] == activity_id.hex() and ack['state'] == 'acknowledged', activity_path, 'durable acknowledgement')
        raw = receipt(config['node_socket'], activity_id)
        receipt_path = run_dir / (label + '.receipt')
        protected_write(receipt_path, raw)
        result = receipt_fields(raw, bytes.fromhex(config['sequencer_public_key']), receipt_path)
        require(result['activity_id'] == activity_id.hex() and result['module'] == module and result['version'] == 1,
                receipt_path, 'receipt activity/module binding')
        token = protected_bytes(config['authority_token_file'], 4096).decode('ascii')
        request = urllib.request.Request(config['authority_url'].rstrip('/') + '/internal/v1/activities/' + activity_id.hex() + '/authority',
                                         headers={'Authorization': 'Bearer ' + token})
        context = ssl.create_default_context(cafile=config['authority_ca_file'])
        with urllib.request.urlopen(request, context=context, timeout=30) as response:
            verified = json.loads(response.read(1048577))
        require(verified['activity_id'] == activity_id.hex() and verified['batch_id'] == result['batch']
                and verified['sequencer_public_key'] == config['sequencer_public_key'], receipt_path, 'authorized batch binding')
        proof_url = config['authority_url'].rstrip('/') + '/v1/batches/' + result['batch'] + '/receipt-authority?receipt_digest=' + result['receipt_digest']
        request = urllib.request.Request(proof_url, headers={'Authorization': 'Bearer ' + token})
        with urllib.request.urlopen(request, context=context, timeout=30) as response:
            document = json.loads(response.read(1048577))
        record = json.dumps(dict(receipt_hex=raw.hex(), replica_document=document),
                            sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()
        protected_write(evidence_dir / (activity_id.hex() + '.json'), record)
        return result

    credited = execute(1, credit, 'credit', 8)
    require(credited['target'] == account.hex() and credited['amount'] > 0, credit_path, 'committed owner account credit')
    execute(1, b'\x71\1\0\2' + did_id + public, 'identity')
    state = command(['read-state'], path)
    now = time.time_ns() // 1000000
    delay_ms = policy['delay_seconds'] * 1000
    rotation = execute(2, b'\x71\2\0\4' + did_id + pending_public
        + struct.pack('>QQQ', now + delay_ms, now + 2 * delay_ms, state['global_sequence'] + 2), 'rotation')
    recovery = execute(3, b'\x71\3\0\5' + did_id + commitment + struct.pack('>HQQ', policy['threshold'], policy['delay_seconds'], policy['delay_seconds']), 'recovery')

    def native_state(result):
        values = [body for module, event, kind, monetary, body in result['effects'] if module == 7 and event == 0x7110 and kind == 3 and not monetary]
        require(len(values) == 1 and len(values[0]) == 223 and values[0][:5] == b'LXGI1'
                and values[0][5:37] == did_id, output, 'native committed identity snapshot')
        return values[0]

    def reference(result):
        return {key: result[key] for key in ('activity_id', 'receipt_digest')}

    def key_policy(result, recovery):
        state = native_state(result)
        revision, delay, maximum = (175, 183, 191) if recovery else (167, 199, 207)
        read = lambda offset: int.from_bytes(state[offset:offset + 8], 'big')
        minimum, maximum_delay = read(delay), read(maximum)
        if not recovery:
            minimum = (minimum + 999) // 1000
            maximum_delay //= 1000
        require(minimum > 0 and maximum_delay >= minimum, output, 'native policy delay bounds')
        return dict(policy_revision=read(revision), required_delay_seconds=minimum,
                    maximum_delay_seconds=maximum_delay, effective_sequence=result['sequence'], evidence=reference(result))

    state = native_state(recovery)
    registration = dict(owner_account=account.hex(), authority=public.hex(), identity=dict(did=did.decode(),
        authorities=[dict(kind='primary_key', id=public.hex())], revocation_sequence=int.from_bytes(state[69:77], 'big'),
        frozen=False, evidence=reference(recovery), capabilities=[], rotation=key_policy(rotation, False), recovery=key_policy(recovery, True)))
    from provision import identity
    identity(registration['identity'], output)
    write_json(output, registration)
    owner_registration(work_dir, owner_did=did.decode())


def produce(work_dir):
    try:
        _produce(work_dir)
    except Refused:
        raise
    except (OSError, ValueError, KeyError, TypeError, OverflowError) as error:
        raise Refused(f'{Path(work_dir) / "human-evidence-input/owner-native.json"}: native provisioning refused; preserve owner-native-run for reconciliation') from error


def prepare_admission(work_dir, secrets_dir):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption
    from provision import owner_result
    root = Path(work_dir) / 'human-evidence-input'
    owner = owner_result(work_dir, Path(work_dir) / 'human-owner-result.json')
    did = owner['did'].encode('ascii')
    require(0 < len(did) <= 255 and did.startswith(b'did:layerx:act_'), root, 'exact LXIP DID')
    custody = Path(secrets_dir) / 'human-owner'
    custody.mkdir(mode=0o700)
    public = None
    for name in ('owner', 'pending'):
        key = Ed25519PrivateKey.generate()
        protected_write(custody / (name + '.seed'), key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
        if name == 'owner':
            public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    account = hashlib.sha256(b'LX:ACCOUNT:v1' + span(b'agent:' + did + b':main')).hexdigest()
    write_json(root / 'owner-admission.json', dict(did=did.decode(), public_key=public.hex(), owner_account=account))
    protected_write(root / 'owner-admission.txt', did.hex().encode() + b':' + public.hex().encode() + b':0\n')
