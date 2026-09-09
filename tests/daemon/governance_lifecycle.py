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
    def __init__(self, work):
        self.root = Path(work) / 'human-evidence-input'
        self.config = json.loads((self.root / 'owner-native.json').read_text())
        self.owner = json.loads((self.root / 'owner-admission.json').read_text())
        self.key = Ed25519PrivateKey.from_private_bytes(Path(self.config['owner_seed_file']).read_bytes())
        self.public = self.key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        self.did = self.owner['did'].encode()
        self.did_id = digest(b'did-id', struct.pack('>H', len(self.did)) + self.did)
        self.common = ['--socket', self.config['node_socket'], '--network-id', str(self.config['network_id']),
                       '--protocol-version', '3', '--actor', self.did.decode()]
        self.output = self.root / 'governance-run'
        self.output.mkdir(mode=0o700, exist_ok=True)

    def command(self, *args):
        result = subprocess.run([self.config['layerxctl'], *args, *self.common], capture_output=True)
        assert result.returncode == 0, result.stderr.decode()
        return json.loads(result.stdout)

    def state(self):
        return self.command('read-state')

    def submit(self, ordinal, payload, label, expected=0):
        state = self.state()
        now = time.time_ns() // 1000000
        fields = (b'\1\0\3\2' + struct.pack('>I', self.config['network_id'])
            + b'\3' + struct.pack('>I', 0x70000 | ordinal) + b'\4' + span(self.did)
            + b'\5' + span(self.public) + b'\6' + struct.pack('>Q', state['account_sequence'])
            + b'\7' + struct.pack('>QQ', now, now + 300000) + b'\10' + span(os.urandom(32))
            + b'\11' + bytes(16) + b'\12' + span(digest(b'payload-hash', payload)) + b'\13' + span(payload))
        signed = b'\0\3\x10\1\14' + fields + b'\14' + span(self.key.sign(digest(b'signature-preimage', b'\0\3\x10\1\13' + fields)))
        activity_id = digest(b'activity-id', signed)
        path = self.output / (label + '.activity')
        protected_write(path, signed)
        ack = self.command('submit', '--public-key', self.public.hex(), '--activity', str(path))
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
