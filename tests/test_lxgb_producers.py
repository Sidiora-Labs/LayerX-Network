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
from lxgb_metadata import metadata, metadata_withdrawal, metadata_modules


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
    withdrawal = metadata_withdrawal(asset, issuer, salt, 17)
    native_fee = (ROOT / 'tests/fixtures/fee-params-v3.bin').read_bytes()
    assert len(native_fee) == 255 and withdrawal[-255:] == native_fee
    assert len(withdrawal) == len(suffix) + 8 and withdrawal[:-257] == suffix[:-249]
    withdrawal_request = producer.genesis_request(members, 77, asset.hex(), 1700000000000, withdrawal)
    for label, payload, accepted in [('withdrawal', withdrawal_request, True),
                                     ('withdrawal-truncated', withdrawal_request[:-1], False),
                                     ('withdrawal-trailing', withdrawal_request + b'\0', False)]:
        path = work / f'{label}.lxgb'
        path.write_bytes(payload)
        result = subprocess.run([str(args.builder.resolve()), str(path), str(work / 'signer'), str(work / label)],
                                capture_output=True)
        assert (result.returncode == 0) == accepted, f'{label}: exit {result.returncode}'
        assert (work / label / 'genesis.manifest').exists() == accepted
        print(f'{label}: native builder exit={result.returncode}; signed artifacts={accepted}')
    assert (work / 'withdrawal/genesis.manifest').read_bytes() != (work / 'valid/genesis.manifest').read_bytes()
    assert (work / 'withdrawal/00000000000000000000.lxs').read_bytes() != (work / 'valid/00000000000000000000.lxs').read_bytes()
    prices = (4, 4, 4, 4, 4, 4, 0)
    modules = metadata_modules(asset, issuer, salt, 17, prices)
    native_modules = (ROOT / 'tests/fixtures/fee-params-v4.bin').read_bytes()
    assert len(native_modules) == 368 and modules[-368:] == native_modules
    assert producer.module_metadata(suffix, 17, prices) == modules
    assert producer.module_metadata(withdrawal, 17, prices) == modules
    assert producer.module_metadata(modules, 17, prices) == modules
    modules_request = producer.genesis_request(members, 77, asset.hex(), 1700000000000, modules)
    bad_count = bytearray(modules_request)
    bad_count[-113] = 6
    for label, payload, accepted in [('modules', modules_request, True),
                                     ('modules-truncated', modules_request[:-1], False),
                                     ('modules-trailing', modules_request + b'\0', False),
                                     ('modules-count', bytes(bad_count), False)]:
        path = work / f'{label}.lxgb'
        path.write_bytes(payload)
        result = subprocess.run([str(args.builder.resolve()), str(path), str(work / 'signer'), str(work / label)],
                                capture_output=True)
        assert (result.returncode == 0) == accepted, f'{label}: exit {result.returncode}'
        assert (work / label / 'genesis.manifest').exists() == accepted
        print(f'{label}: native builder exit={result.returncode}; signed artifacts={accepted}')
    assert (work / 'modules/genesis.manifest').read_bytes() != (work / 'withdrawal/genesis.manifest').read_bytes()
    assert (work / 'modules/00000000000000000000.lxs').read_bytes() != (work / 'withdrawal/00000000000000000000.lxs').read_bytes()
    for invalid in [(), prices[:-1], prices + (0,), (-1, *prices[1:]), (2**128, *prices[1:]), (True, *prices[1:])]:
        for emitter in [lambda: metadata_modules(asset, issuer, salt, 17, invalid),
                        lambda: producer.module_metadata(suffix, 17, invalid)]:
            try:
                emitter()
            except ValueError:
                pass
            else:
                raise AssertionError('module fee count or integer bound was not enforced')
    for index in range(7):
        changed = list(prices)
        changed[index] += 1
        try:
            producer.module_metadata(modules, 17, changed)
        except ValueError:
            pass
        else:
            raise AssertionError('supplied module schedule was silently replaced')
    for text in ['{}', '{"escrow":4,"escrow":4}',
                 '{"escrow":4,"budget":4,"stream":4,"service":4,"perps":4,"governance":4,"bridge":0,"asset":0}']:
        path = work / 'invalid-prices.json'
        path.write_text(text)
        try:
            producer.module_prices(path)
        except ValueError:
            pass
        else:
            raise AssertionError('noncanonical module fee configuration accepted')
    for price in (-1, 2**64, True):
        try:
            metadata_withdrawal(asset, issuer, salt, price)
        except ValueError:
            pass
        else:
            raise AssertionError('withdrawal price u64 bound was not enforced')
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
