import hashlib
import json
import struct

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from guardians import generate
from provision import Refused


def test_real_guardian_custody_and_commitment(tmp_path):
    identities = [hashlib.sha256(str(i).encode()).hexdigest() for i in range(3)]
    generate(tmp_path, tmp_path, identities)
    root = tmp_path / 'human-evidence-input'
    bindings = json.loads((root / 'recovery-guardian-bindings.json').read_text())
    keys = []
    for member in bindings['members']:
        seed = tmp_path / 'human-guardians' / (member['role'] + '.seed')
        assert seed.stat().st_mode & 0o777 == 0o600
        public = Ed25519PrivateKey.from_private_bytes(seed.read_bytes()).public_key()
        public.verify(Ed25519PrivateKey.from_private_bytes(seed.read_bytes()).sign(b'guardian binding'), b'guardian binding')
        assert public.public_bytes(Encoding.Raw, PublicFormat.Raw).hex() == member['public_key']
        assert member['operator'] == 'cluster' and member['independent'] is False
        keys.append(bytes.fromhex(member['public_key']))
    assert [m['identity'] for m in bindings['members']] == identities
    assert len(set(keys)) == 3
    policy = json.loads((root / 'recovery-policy.json').read_text())
    assert policy['threshold'] == 2
    assert bytes(policy['root']) == hashlib.sha256(b'LX:HUMAN:RECOVERY:v1\0' + struct.pack('>HH', 2, 3) + b''.join(sorted(keys))).digest()
    with pytest.raises(FileExistsError):
        generate(tmp_path, tmp_path, identities)


def test_guardians_refuse_duplicate_identity(tmp_path):
    with pytest.raises(Refused):
        generate(tmp_path, tmp_path, ['01' * 32] * 3)
    assert not (tmp_path / 'human-guardians').exists()
