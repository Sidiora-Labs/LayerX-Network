import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from lxgb_metadata import metadata, metadata_modules

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--builder', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('prepare_beta', ROOT / 'platform/hosted/paxeer/prepare-beta.py')
    producer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(producer)
    work = Path(tempfile.mkdtemp(prefix='layerx-genesis-fixture-', dir='/tmp'))
    signer = ed25519.Ed25519PrivateKey.generate()
    seed = signer.private_bytes_raw()
    key = work / 'signer'
    key.write_bytes(seed)
    key.chmod(0o600)
    public = signer.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    issuer = ed25519.Ed25519PrivateKey.generate().public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    asset = bytes.fromhex(producer.BETA_ASSET_ID)
    salt = os.urandom(32)
    members = []
    for index in range(1, 4):
        member = ec.generate_private_key(ec.SECP256K1()).public_key()
        members.append({'guarantor_id': index.to_bytes(32, 'big').hex(),
                        'public_key': member.public_bytes(Encoding.X962, PublicFormat.CompressedPoint).hex()})
    for name, modules in (('previous', ()), ('current', producer.genesis_modules())):
        suffix = metadata(asset, issuer, salt) if name == 'previous' else metadata_modules(
            asset, issuer, salt, 0, producer.module_prices(ROOT / 'platform/hosted/node/genesis-module-fees.json'))
        request = producer.genesis_request(members, 77, asset.hex(), 1700000000000, suffix, modules)
        request_path = work / (name + '.lxgb')
        request_path.write_bytes(request)
        subprocess.run([str(args.builder.resolve()), str(request_path), str(key), str(work / name)],
                       cwd=ROOT, check=True, capture_output=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    names = ('genesis.manifest', '00000000000000000000.lxs',
             'paxeer-registration-request.lxrr', 'paxeer-deployment-descriptor.lxgd')
    for name in names:
        shutil.copyfile(work / 'current' / name, output / name)
    shutil.copyfile(work / 'current.lxgb', output / 'genesis-request.lxgb')
    (output / 'sequencer.public').write_bytes(public)
    roots = (work / 'current/paxeer-registration-request.lxrr').read_bytes()
    previous = (work / 'previous/paxeer-registration-request.lxrr').read_bytes()
    assert len(roots) == len(previous) == 73 and roots[:9] == previous[:9] == b'LXRR\x01' + (77).to_bytes(4, 'big')
    assert roots[9:41] != previous[9:41] and roots[41:] != previous[41:]
    expected = {'protocol_version': 3, 'network_id': 77, 'modules': list(producer.genesis_modules()),
                'native_fee_schedule_version': 4, 'native_fee_authority_version': 2,
                'module_prices': dict(zip(producer.MODULE_NAMES,
                    producer.module_prices(ROOT / 'platform/hosted/node/genesis-module-fees.json'))),
                'withdrawal_price': 0,
                'canonical_state_root': roots[9:41].hex(), 'receipt_state_root': roots[41:].hex(),
                'previous_canonical_state_root': previous[9:41].hex(),
                'previous_receipt_state_root': previous[41:].hex(),
                'sequencer_public_key': public.hex(),
                'sha256': {name: hashlib.sha256((output / name).read_bytes()).hexdigest() for name in names}}
    (output / 'expected.json').write_text(json.dumps(expected, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'canonical_state_root': expected['canonical_state_root'],
                      'receipt_state_root': expected['receipt_state_root'], 'builder_work': str(work)}))


if __name__ == '__main__':
    main()
