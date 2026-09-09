import argparse
import importlib.util
import os
from pathlib import Path
import subprocess
import sys

from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tests/support'))
from lxgb_metadata import metadata


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--builder', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    work = args.output.resolve()
    work.mkdir(mode=0o700, parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('prepare_beta', ROOT / 'platform/hosted/paxeer/prepare-beta.py')
    producer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(producer)
    seed = os.urandom(32)
    with (work / 'signer').open('xb') as out:
        os.chmod(work / 'signer', 0o600)
        out.write(seed)
    issuer = ed25519.Ed25519PrivateKey.from_private_bytes(seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    asset = os.urandom(32)
    salt = os.urandom(32)
    suffix = metadata(asset, issuer, salt)
    members = []
    for index in range(1, 4):
        public = ec.generate_private_key(ec.SECP256K1()).public_key().public_bytes(Encoding.X962, PublicFormat.CompressedPoint)
        members.append(dict(guarantor_id=index.to_bytes(32, 'big').hex(), public_key=public.hex()))
    request = producer.genesis_request(members, 77, asset.hex(), 1700000000000, suffix)
    assert request[:5] == b'LXGB\x02' and request.endswith(suffix)
    for label, payload, accepted in [('valid', request, True), ('truncated', request[:-1], False),
                                     ('trailing', request + b'\0', False),
                                     ('missing-metadata', request[:-len(suffix)], False)]:
        path = work / f'{label}.lxgb'
        path.write_bytes(payload)
        result = subprocess.run([str(args.builder.resolve()), str(path), str(work / 'signer'), str(work / label)],
                                capture_output=True)
        assert (result.returncode == 0) == accepted, f'{label}: exit {result.returncode}'
        assert (work / label / 'genesis.manifest').exists() == accepted
        print(f'{label}: native builder exit={result.returncode}; signed artifacts={accepted}')
    for suffix_input in [b'', suffix[:219], bytes(16384)]:
        try:
            producer.genesis_request(members, 77, asset.hex(), 1700000000000, suffix_input)
        except ValueError:
            pass
        else:
            raise AssertionError('producer metadata bounds were not enforced')
    print('LXGB v2 producer preserves supplied metadata; native signing and malformed-request refusals passed')


if __name__ == '__main__':
    main()
