#!/usr/bin/env python3
import argparse
import json
import os
from pathlib import Path
import secrets
import subprocess
import time

ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
ROOT = Path(__file__).resolve().parent
BETA_PROTOCOL_VERSION = 3
BETA_ASSET_ID = 'b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898'


def cast(*args):
    result = subprocess.run(['cast', *args], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError('cast wallet operation failed')
    return result.stdout.strip()


def keypair(directory, name):
    key = '0x' + (secrets.randbelow(ORDER - 1) + 1).to_bytes(32, 'big').hex()
    with (directory / name).open('x') as file:
        file.write(key + '\n')
    address = cast('wallet', 'address', '--private-key', key)
    public = bytes.fromhex(cast('wallet', 'public-key', '--private-key', key).removeprefix('0x'))
    if len(public) == 65 and public[0] == 4:
        public = public[1:]
    if len(public) != 64:
        raise ValueError('invalid secp256k1 public key length')
    compressed = bytes([2 + (public[-1] & 1)]) + public[:32]
    return address, '0x' + compressed.hex()


def genesis_request(guarantors, network_id, asset_id, timestamp_ms):
    if not 0 < network_id < 2**32 or not 0 < timestamp_ms < 2**64:
        raise ValueError('invalid genesis network or timestamp')
    asset = bytes.fromhex(asset_id.removeprefix('0x'))
    if len(asset) != 32 or not any(asset) or len(guarantors) != 3:
        raise ValueError('beta genesis requires one asset and three guarantors')
    be = lambda value, width: value.to_bytes(width, 'big')
    request = bytearray(b'LXGB\x01' + be(BETA_PROTOCOL_VERSION, 2))
    request += be(network_id, 4) + be(timestamp_ms, 8) + be(1, 2) + be(7, 2)
    request += b'parameter-version'.ljust(32, b'\x00') + be(1, 32)
    request += be(len(guarantors), 2)
    previous = bytes(32)
    for member in guarantors:
        identifier = bytes.fromhex(member['guarantor_id'].removeprefix('0x'))
        public = bytes.fromhex(member['public_key'].removeprefix('0x'))
        if len(identifier) != 32 or identifier <= previous or len(public) != 33 or public[0] not in (2, 3):
            raise ValueError('invalid ordered genesis guarantor')
        request += identifier + public + bytes(16)
        previous = identifier
    request += asset + be(1, 4)
    for coefficient in (1, 1, 1, 1, 1, 8, 8, 64, 8):
        request += be(coefficient, 8)
    request += be(1, 8) + b'\x01' + be(1, 4)
    for price in (1, 1, 2, 4, 1, 1, 100):
        request += be(price, 8)
    for demand in (100, 1, 1, 10, 1, 1000):
        request += be(demand, 8)
    return bytes(request)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    parser.add_argument('--network-id', type=int, default=402)
    parser.add_argument('--asset-id', default=BETA_ASSET_ID)
    parser.add_argument('--timestamp-ms', type=int, default=int(time.time() * 1000))
    args = parser.parse_args()
    os.umask(0o077)
    target = args.directory.resolve()
    target.mkdir(mode=0o700, parents=False, exist_ok=False)
    keys = target/'keys'
    keys.mkdir(mode=0o700)
    deployer, _ = keypair(keys, 'deployer.key')
    (target/'deployer.address').write_text(deployer + '\n')
    deployment = json.loads((ROOT/'deployment-input.beta.json').read_text())
    deployment['protocol_version'] = BETA_PROTOCOL_VERSION
    for role in ['final_proposer', 'final_executor', 'emergency_council']:
        deployment[role], _ = keypair(keys, role + '.key')
    guarantors = []
    for sequence in range(1, 4):
        identifier = '0x' + sequence.to_bytes(32, 'big').hex()
        signer, public = keypair(keys, identifier + '.signer.key')
        controller, _ = keypair(keys, identifier + '.controller.key')
        guarantors.append({'guarantor_id': identifier, 'signer': signer, 'public_key': public,
                           'bond_controller': controller, 'joined_epoch': 1,
                           'governance_sequence': sequence, 'bond_amount': '1000000000'})
    (target/'guarantors.json').write_text(json.dumps(guarantors, indent=2) + '\n')
    (target/'deployment-input.json').write_text(json.dumps(deployment, indent=2) + '\n')
    (target/'genesis-request.lxgb').write_bytes(genesis_request(
        guarantors, args.network_id, args.asset_id, args.timestamp_ms))
    (keys/'genesis-signer.key').write_bytes(secrets.token_bytes(32))
    print(target)


if __name__ == '__main__':
    main()
