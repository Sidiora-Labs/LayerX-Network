import argparse
import time
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import big, require, sha, write_new
from deploy_local_custody import command


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--profile', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--builder', required=True)
    args = parser.parse_args()
    profile = Path(args.profile).read_bytes()
    require(len(profile) == 207 and profile[:5] == b'LXBC1' and profile[205:] == big(3, 2),
            'protocol-three custody profile required')
    require(int.from_bytes(profile[5:13], "big") != 125, "persistent chain ID refused")
    directory = Path(args.output).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    key = Ed25519PrivateKey.generate()
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    write_new(directory / 'sequencer.key', key.private_bytes_raw())
    write_new(directory / 'sequencer.pub', public)
    request = (b'LXGB\x01' + big(3, 2) + profile[201:205] + big(int(time.time() * 1000), 8)
               + big(1, 2) + big(7, 2) + b'parameter-version'.ljust(32, b'\0') + big(1, 32)
               + big(1, 2) + sha(b'layerx-beta-guarantor:' + public.hex().encode())
               + b'\x02' + public + big(0, 16) + profile[97:129] + big(1, 4))
    request += b''.join(big(value, 8) for value in (1, 1, 1, 1, 1, 8, 8, 64, 8))
    request += big(1, 8) + b'\x01' + big(1, 4)
    request += b''.join(big(value, 8) for value in (1, 1, 2, 4, 1, 1, 100, 100, 1, 1, 10, 1, 1000))
    require(len(request) == 395, 'genesis request length')
    write_new(directory / 'request.lxgb', request)
    command(args.builder, str(directory / 'request.lxgb'), str(directory / 'sequencer.key'),
            str(directory / 'artifacts'), '--custody-profile', str(Path(args.profile).resolve()))
    require((directory / 'artifacts/genesis.manifest').is_file(), 'signed manifest required')
    print('Signed custody genesis artifacts built; external registration still required')


if __name__ == '__main__':
    main()
