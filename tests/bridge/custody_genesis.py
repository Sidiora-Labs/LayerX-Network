import argparse
import time
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from custody_credit import big, quantity, require, sha, write_new
from deploy_local_custody import command, disposable_rpc


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--profile', required=True)
    parser.add_argument('--output', required=True)
    parser.add_argument('--builder', required=True)
    parser.add_argument('--genesis-metadata', type=Path, required=True)
    parser.add_argument('--rpc')
    parser.add_argument('--ca-bundle')
    parser.add_argument('--disposable-identity')
    args = parser.parse_args()
    profile = Path(args.profile).read_bytes()
    require(len(profile) == 207 and profile[:5] == b'LXBC1' and profile[205:] == big(3, 2),
            'protocol-three custody profile required')
    chain_id = int.from_bytes(profile[5:13], "big")
    if chain_id == 125 or args.disposable_identity:
        require(args.rpc and args.ca_bundle and args.disposable_identity,
                "chain 125 requires verified disposable identity")
        rpc = disposable_rpc(args.rpc, args.ca_bundle, args.disposable_identity)
        require(quantity(rpc.call("eth_chainId", [])) == chain_id, "profile chain ID")
        require(rpc.genesis_sha256 == profile[169:201], "profile genesis identity")
    else:
        require(not args.rpc and not args.ca_bundle, "RPC verification requires disposable identity")
    directory = Path(args.output).resolve()
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    key = Ed25519PrivateKey.generate()
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    write_new(directory / 'sequencer.key', key.private_bytes_raw())
    write_new(directory / 'sequencer.pub', public)
    request = (b'LXGB\x02' + big(3, 2) + profile[201:205] + big(int(time.time() * 1000), 8)
               + big(1, 2) + big(7, 2) + b'parameter-version'.ljust(32, b'\0') + big(1, 32)
               + big(1, 2) + sha(b'layerx-beta-guarantor:' + public.hex().encode())
               + b'\x02' + public + big(0, 16) + profile[97:129] + big(1, 4))
    request += b''.join(big(value, 8) for value in (1, 1, 1, 1, 1, 8, 8, 64, 8))
    request += big(1, 8) + b'\x01' + big(1, 4)
    request += b''.join(big(value, 8) for value in (1, 1, 2, 4, 1, 1, 100, 100, 1, 1, 10, 1, 1000))
    require(len(request) == 395, 'genesis request length')
    metadata = args.genesis_metadata.read_bytes()
    require(219 < len(metadata) <= 16384 - len(request), 'genesis metadata bounds')
    request += metadata
    write_new(directory / 'request.lxgb', request)
    command(args.builder, str(directory / 'request.lxgb'), str(directory / 'sequencer.key'),
            str(directory / 'artifacts'), '--custody-profile', str(Path(args.profile).resolve()))
    require((directory / 'artifacts/genesis.manifest').is_file(), 'signed manifest required')
    print('Signed custody genesis artifacts built; external registration still required')


if __name__ == '__main__':
    main()
