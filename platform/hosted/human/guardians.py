import argparse
import hashlib
from pathlib import Path
import struct

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PrivateFormat, PublicFormat, NoEncryption

from owner_native import protected_write
from provision import h32, require, write_json


def generate(work_dir, secrets_dir, identities):
    root = Path(work_dir) / 'human-evidence-input'
    custody = Path(secrets_dir) / 'human-guardians'
    require(len(identities) == 3 and len(set(identities)) == 3, root, 'three distinct operator identities')
    for identity in identities:
        h32(identity, root, 'operator identity')
    root.mkdir(mode=0o700, exist_ok=True)
    custody.mkdir(mode=0o700)
    members = []
    for role, identity in zip(('guarantor-1', 'guarantor-2', 'sequencer'), identities):
        key = Ed25519PrivateKey.generate()
        public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw).hex()
        protected_write(custody / (role + '.seed'), key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption()))
        members.append(dict(role=role, identity=identity, public_key=public, operator='cluster', independent=False))
    keys = sorted(member['public_key'] for member in members)
    commitment = hashlib.sha256(b'LX:HUMAN:RECOVERY:v1\0' + struct.pack('>HH', 2, 3)
                                + b''.join(bytes.fromhex(key) for key in keys)).digest()
    write_json(root / 'recovery-guardians.json', dict(public_keys=keys))
    write_json(root / 'recovery-guardian-bindings.json', dict(version=1, threshold=2, members=members))
    write_json(root / 'recovery-policy.json', dict(root=list(commitment), threshold=2, delay_seconds=86400))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--work-dir', required=True)
    parser.add_argument('--secrets-dir', required=True)
    parser.add_argument('--identity', action='append', required=True)
    args = parser.parse_args()
    generate(args.work_dir, args.secrets_dir, args.identity)
