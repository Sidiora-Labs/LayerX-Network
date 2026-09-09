import json
import os
from pathlib import Path
import struct
import subprocess
import time

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from owner_native import Reader, digest, protected_write, receipt, receipt_fields, span


class Governance:
    def __init__(self, work, signer=None, actor=None, output="governance-run"):
        self.root = Path(work) / 'human-evidence-input'
        self.config = json.loads((self.root / 'owner-native.json').read_text())
        self.owner = json.loads((self.root / 'owner-admission.json').read_text())
        self.key = signer if signer is not None else Ed25519PrivateKey.from_private_bytes(Path(self.config['owner_seed_file']).read_bytes())
        self.public = self.key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        self.did = (actor or self.owner['did']).encode()
        self.did_id = digest(b'did-id', struct.pack('>H', len(self.did)) + self.did)
        self.common = ['--socket', self.config['node_socket'], '--network-id', str(self.config['network_id']),
                       '--protocol-version', '3', '--actor', self.did.decode()]
        self.output = self.root / output
        self.output.mkdir(mode=0o700, exist_ok=True)

    def command(self, *args):
        result = subprocess.run([self.config['layerxctl'], *args, *self.common], capture_output=True)
        assert result.returncode == 0, result.stderr.decode()
        return json.loads(result.stdout)

    def state(self):
        return self.command('read-state')

    def submit(self, ordinal, payload, label, expected=0, signer=None, admission_refused=False):
        state = self.state()
        now = time.time_ns() // 1000000
        key = signer or self.key
        public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        fields = (b'\1\0\3\2' + struct.pack('>I', self.config['network_id'])
            + b'\3' + struct.pack('>I', 0x70000 | ordinal) + b'\4' + span(self.did)
            + b'\5' + span(public) + b'\6' + struct.pack('>Q', state['account_sequence'])
            + b'\7' + struct.pack('>QQ', now, now + 300000) + b'\10' + span(os.urandom(32))
            + b'\11' + bytes(16) + b'\12' + span(digest(b'payload-hash', payload)) + b'\13' + span(payload))
        signed = b'\0\3\x10\1\14' + fields + b'\14' + span(key.sign(digest(b'signature-preimage', b'\0\3\x10\1\13' + fields)))
        activity_id = digest(b'activity-id', signed)
        path = self.output / (label + '.activity')
        protected_write(path, signed)
        if admission_refused:
            result = subprocess.run([self.config['layerxctl'], 'submit', '--public-key', public.hex(),
                                     '--activity', str(path), *self.common], capture_output=True)
            assert result.returncode != 0, 'unauthorized signer admitted'
            assert self.state() == state, 'refused signer changed native head or account sequence'
            return None
        ack = self.command('submit', '--public-key', public.hex(), '--activity', str(path))
        assert ack['activity_id'] == activity_id.hex() and ack['state'] == 'acknowledged'
        raw = receipt(self.config['node_socket'], activity_id)
        protected_write(self.output / (label + '.receipt'), raw)
        assert raw[-69:-64] == b'\1\0\0\0\x40'
        Ed25519PublicKey.from_public_bytes(bytes.fromhex(self.config['sequencer_public_key'])).verify(
            raw[-64:], digest(b'receipt', raw[:-69] + b'\0'))
        reader = Reader(raw, label)
        assert reader.take(6) == b'\0\3\x52\1\0\3'
        assert reader.span(32) == activity_id
        sequence = reader.number(8)
        assert sequence == state['global_sequence'] + 1
        for _ in range(3):
            assert len(reader.span(32)) == 32
        result = int.from_bytes(reader.take(4), 'big', signed=True)
        assert result == expected, (label, result, expected)
        count = reader.number(4)
        assert self.state()['account_sequence'] == state['account_sequence'] + 1
        if expected != 0:
            assert count == 0, 'refused Governance execution published effects'
            return None
        return receipt_fields(raw, bytes.fromhex(self.config['sequencer_public_key']), label)

    def grant(self, key, revision, not_before, not_after):
        return (b'\0\1\x20\1\1' + span(self.did_id) + span(self.did_id) + b'\2' + span(key)
                + struct.pack('>QHH', 2, 5, 5) + span(bytes(32)) + bytes(48 + 8 + 32 + 8)
                + span(bytes(32)) + struct.pack('>QQQ', not_before, not_after, revision)
                + b'\0' + bytes(8) + span(bytes(64)))


def session(work):
    native = Governance(work)
    registration = json.loads((native.root / 'owner-registration.json').read_text())
    revision = registration['identity']['revocation_sequence']
    key = Ed25519PrivateKey.generate().public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    now = time.time_ns() // 1000000
    grant = native.grant(key, revision, now - 1000, now + 3600000)
    grant_id = digest(b'authority-hash', grant)
    expiry = native.state()['global_sequence'] + 1000
    action = os.urandom(32)
    payload = b'\x71\5\1\3' + span(grant) + struct.pack('>Q', expiry) + span(action)
    native.submit(5, b'\x71\5\0\1' + span(grant), 'old-shape', -3)
    native.submit(5, b'\x71\5\1\3' + span(grant) + bytes(8) + span(action), 'zero-expiry', -204)
    native.submit(5, b'\x71\5\1\3' + span(grant) + struct.pack('>Q', expiry) + span(bytes(32)), 'zero-action', -204)
    native.submit(5, payload + b'\0', 'trailing', -2)
    result = native.submit(5, payload, 'session')
    summaries = [effect[4] for effect in result['effects'] if effect[0] == 7 and effect[1] == 0x7145]
    expected = (b'LXGS2' + grant_id + native.did_id + native.public + action + key
                + struct.pack('>QQHHQQQ', expiry, 2, 5, 5, now - 1000, now + 3600000, revision))
    assert summaries == [expected]
    native.submit(5, payload, 'duplicate-grant', -301)
    protected_write(native.output / 'session.json', json.dumps(dict(
        grant_id=grant_id.hex(), action_key=action.hex(), expiry_sequence=expiry,
        payload=payload.hex(), summary=expected.hex(), sequence=result['sequence'],
        account_sequence=native.state()['account_sequence'])).encode())
    print('real versioned session grant, committed action/expiry, old-shape, zero-bound, trailing and duplicate refusals passed', flush=True)


def lifecycle(work, treasury_seed, restart=False):
    owner = Governance(work)
    treasury = json.loads((Path(work) / 'node/treasury.json').read_text())
    treasury_key = Ed25519PrivateKey.from_private_bytes(treasury_seed)
    native = Governance(work, treasury_key, treasury['did'], 'rotation-run')
    now = time.time_ns() // 1000000
    current = json.loads((owner.output / 'session.json').read_text())
    grant_id = bytes.fromhex(current['grant_id'])

    def revoke(identifier, reason=1, sequence=None):
        if sequence is None:
            sequence = owner.state()['global_sequence'] + 1
        return b'\x71\6\0\3' + identifier + bytes([reason]) + struct.pack('>Q', sequence)

    if restart:
        saved = json.loads((native.output / 'rotation.json').read_text())
        assert native.state()['account_sequence'] == saved['account_sequence']
        native.submit(2, bytes.fromhex(saved['payload']), 'restart-pending-rotation', -204)
        owner.submit(6, revoke(grant_id), 'restart-double-revoke', -203)
        owner.submit(5, bytes.fromhex(current['payload']), 'restart-stale-grant', -204)
        latest = json.loads((owner.output / 'new-session.json').read_text())
        owner.submit(5, bytes.fromhex(latest['payload']), 'restart-duplicate-grant', -301)
        result = owner.submit(6, revoke(bytes.fromhex(latest['grant_id'])), 'restart-revoke')
        state = next(effect[4] for effect in result['effects'] if effect[1] == 0x7110)
        assert int.from_bytes(state[69:77], 'big') == result['sequence']
        print('real Governance restart preserved rotation, grants and revocation; duplicate/stale/double-revoke refusals and subsequent revoke passed', flush=True)
        return

    registered = native.submit(1, b'\x71\1\0\2' + native.did_id + native.public, 'registration')
    assert registered['module'] == 7
    pending = Ed25519PrivateKey.generate()
    pending_public = pending.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    begin, end = now + 60000, now + 120000

    def rotation(key=pending_public, start=begin, stop=end, effective=None, did=None):
        if effective is None:
            effective = native.state()['global_sequence'] + 1000
        return b'\x71\2\0\4' + (did or native.did_id) + key + struct.pack('>QQQ', start, stop, effective)

    for label, payload, code in [
        ('zero-key', rotation(bytes(32)), -204),
        ('noncanonical-key', rotation(bytes([255]) * 32), -204),
        ('same-key', rotation(native.public), -204),
        ('past-begin', rotation(start=1), -204),
        ('end-before-begin', rotation(stop=begin - 1), -204),
        ('equal-window', rotation(stop=begin), -204),
        ('effective-past', rotation(effective=1), -204),
        ('wrong-did', rotation(did=os.urandom(32)), -204),
        ('short-payload', rotation()[:-1], -3),
        ('trailing-payload', rotation() + b'\0', -3),
    ]:
        native.submit(2, payload, label, code)
    native.submit(2, rotation(), 'unregistered-signer', signer=Ed25519PrivateKey.generate(), admission_refused=True)
    payload = rotation()
    result = native.submit(2, payload, 'rotation')
    state = next(effect[4] for effect in result['effects'] if effect[1] == 0x7110)
    assert state[111:143] == pending_public and state[143:167] == payload[68:92]
    native.submit(2, payload, 'pending-rotation', -204)
    native.submit(3, b'\x71\3\0\5' + native.did_id + os.urandom(32) + struct.pack('>HQQ', 2, 60, 120),
                  'pending-cannot-replace-owner-policy', -204, signer=pending)
    recovery_payload = b'\x71\3\0\5' + native.did_id + os.urandom(32) + struct.pack('>HQQ', 2, 60, 120)
    recovery = native.submit(3, recovery_payload, 'recovery-after-refusals')
    later = next(effect[4] for effect in recovery['effects'] if effect[1] == 0x7110)
    assert later[111:175] == state[111:175], 'refused rotation changed committed pending policy'
    native.submit(6, revoke(grant_id), 'wrong-grantor', -204)
    protected_write(native.output / 'rotation.json', json.dumps(dict(payload=payload.hex(),
        account_sequence=native.state()['account_sequence'])).encode())

    for label, payload, code in [
        ('unknown-grant', revoke(os.urandom(32)), -7),
        ('zero-reason', revoke(grant_id, 0), -204),
        ('unknown-reason', revoke(grant_id, 6), -204),
        ('past-sequence', revoke(grant_id, sequence=1), -204),
        ('future-sequence', revoke(grant_id, sequence=2 ** 64 - 1), -204),
        ('short-revoke', revoke(grant_id)[:-1], -3),
        ('trailing-revoke', revoke(grant_id) + b'\0', -3),
    ]:
        owner.submit(6, payload, label, code)
    revoked = owner.submit(6, revoke(grant_id), 'revoke')
    events = [effect[4] for effect in revoked['effects'] if effect[1] == 0x7106]
    assert events == [grant_id + b'\1' + struct.pack('>Q', revoked['sequence'])]
    owner.submit(6, revoke(grant_id), 'double-revoke', -203)
    owner.submit(5, bytes.fromhex(current['payload']), 'stale-revocation', -204)
    session_key = Ed25519PrivateKey.generate().public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    grant = owner.grant(session_key, revoked['sequence'], now - 1000, now + 3600000)
    action = os.urandom(32)
    expiry = owner.state()['global_sequence'] + 1000
    payload = b'\x71\5\1\3' + span(grant) + struct.pack('>Q', expiry) + span(action)
    accepted = owner.submit(5, payload, 'new-session-after-revoke')
    summary = next(effect[4] for effect in accepted['effects'] if effect[1] == 0x7145)
    assert summary[101:133] == action and int.from_bytes(summary[201:209], 'big') == revoked['sequence']
    protected_write(owner.output / 'new-session.json', json.dumps(dict(
        payload=payload.hex(), grant_id=digest(b'authority-hash', grant).hex())).encode())
    protected_write(owner.output / 'before-restart.json', json.dumps(dict(account_sequence=owner.state()['account_sequence'])).encode())
    print('real Governance rotation key/time/sequence/DID/encoding/signer negatives, pending-policy integrity, revoke reason/sequence/unknown/duplicate negatives and fresh grant passed', flush=True)
