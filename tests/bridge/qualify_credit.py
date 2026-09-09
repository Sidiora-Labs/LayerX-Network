import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time

from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from custody_credit import Rpc

ROOT = Path(__file__).resolve().parents[2]


def run(*args):
    subprocess.run([str(arg) for arg in args], cwd=ROOT, check=True)


@contextlib.contextmanager
def chain(directory, name, source=None, block=None):
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    if port == 18545:
        raise ValueError('persistent-chain port is forbidden')
    command = ['anvil', '--host', '127.0.0.1', '--port', str(port),
               '--chain-id', '31337', '--silent']
    if source is not None:
        command += ['--fork-url', source, '--fork-block-number', str(block)]
    with (directory / (name + '.log')).open('wb') as log:
        process = subprocess.Popen(command, cwd=directory, stdout=log, stderr=log)
        try:
            rpc = Rpc(f'http://127.0.0.1:{port}')
            deadline = time.monotonic() + 30
            while True:
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError('isolated chain startup failed')
                try:
                    if int(rpc.call('eth_chainId', []), 16) != 31337:
                        raise RuntimeError('wrong chain')
                    break
                except (OSError, ValueError):
                    time.sleep(0.1)
            yield rpc.url
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--build-dir', default='build')
    parser.add_argument('--maintenance', action='store_true')
    parser.add_argument('--compare-baseline', type=Path)
    args = parser.parse_args()
    build = (ROOT / args.build_dir).resolve()
    evidence = ROOT / 'build' / 'custody-qualification'
    evidence.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='credit-', dir=evidence) as temporary:
        work = Path(temporary)
        for name in ('actor', 'attestor', 'genesis'):
            descriptor = os.open(work / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'wb') as output:
                output.write(os.urandom(32))
        public = ed25519.Ed25519PrivateKey.from_private_bytes((work / 'actor').read_bytes()).public_key()
        public = public.public_bytes(Encoding.Raw, PublicFormat.Raw)
        did = 'did:layerx:' + public.hex()
        name = ('agent:' + did + ':main').encode()
        beneficiary = hashlib.sha256(b'LX:ACCOUNT:v1' + len(name).to_bytes(4, 'big') + name).digest()
        asset = hashlib.sha256(b'LayerX/native-custody-qualification/asset').digest()
        timestamp = int(time.time() * 1000)
        guarantor = ec.generate_private_key(ec.SECP256K1()).public_key().public_bytes(
            Encoding.X962, PublicFormat.CompressedPoint)
        def be(value, length):
            return value.to_bytes(length, 'big')
        request = (b'LXGB' + be(1, 1) + be(3, 2) + be(77, 4) + be(timestamp, 8) +
                   be(1, 2) + be(7, 2) + b'parameter-version'.ljust(32, b'\0') + be(1, 32) +
                   be(1, 2) + hashlib.sha256(guarantor).digest() + guarantor + be(0, 16) + asset +
                   be(1, 4) + b''.join(be(v, 8) for v in (1, 1, 1, 1, 1, 8, 8, 64, 8)) +
                   be(1, 8) + be(1, 1) + be(1, 4) +
                   b''.join(be(v, 8) for v in (1, 1, 2, 4, 1, 1, 100, 100, 1, 1, 10, 1, 1000)))
        if len(request) != 395:
            raise ValueError('genesis request length')
        (work / 'request.lxgb').write_bytes(request)
        with chain(work, 'custody') as first:
            run(sys.executable, 'tests/bridge/deploy_local_custody.py', '--allow-local-chain',
                '--rpc', first, '--asset', '0x' + asset.hex(), '--beneficiary', '0x' + beneficiary.hex(),
                '--amount', '1000000', '--output', work / 'custody.json')
            custody = json.loads((work / 'custody.json').read_text())
            with chain(work, 'observer', first, custody['fork_block']) as second:
                pair = ['--rpc', first, '--rpc', second]
                run(sys.executable, 'tests/bridge/custody_credit.py', 'profile', *pair,
                    '--chain-id', '31337', '--network-id', '77', '--vault', custody['vault'],
                    '--runtime-sha256', custody['runtime_sha256'], '--asset', '0x' + asset.hex(),
                    '--confirmations', '2', '--attestor-key', work / 'attestor', '--output', work / 'profile')
                run(ROOT / 'build/bin/layerx-genesis-build', work / 'request.lxgb', work / 'genesis',
                    work / 'genesis-output', '--custody-profile', work / 'profile')
                attest = [*pair, '--profile', work / 'profile', '--network-id', '77',
                          '--transaction', custody['transaction'], '--beneficiary', '0x' + beneficiary.hex(),
                          '--beneficiary-key', '0x' + public.hex(), '--expected-amount', '1000000',
                          '--attestor-key', work / 'attestor']
                run(sys.executable, 'tests/bridge/custody_credit.py', 'attest', *attest,
                    '--output', work / 'credit')
                run(build / 'tests/bridge/sign-credit', work / 'profile', work / 'credit', did,
                    work / 'actor', '0', timestamp, work / 'activity')
                run(build / 'tests/bridge/test-credit', work / 'genesis-output/genesis.manifest',
                    work / 'activity', work / 'actor')
                if args.compare_baseline:
                    inputs = [work / 'genesis-output/genesis.manifest', work / 'activity', work / 'actor']
                    run(args.compare_baseline.resolve(), *inputs, work / 'before.bin')
                    run(build / 'tests/bridge/test-credit', *inputs, work / 'after.bin')
                    before = (work / 'before.bin').read_bytes()
                    after = (work / 'after.bin').read_bytes()
                    if before != after:
                        raise AssertionError('bridge-credit state diff or receipt bytes changed')
                    diff_size = int.from_bytes(before[:8], 'big')
                    receipt_size = int.from_bytes(before[8:16], 'big')
                    if not diff_size or not receipt_size or len(before) != 16 + diff_size + receipt_size:
                        raise AssertionError('invalid comparison framing')
                    print(f'byte-identical bridge-credit state diff ({diff_size}) and receipt ({receipt_size})')
                if args.maintenance:
                    run(build / 'tests/lxp_test_maintenance_publication',
                        work / 'genesis-output/genesis.manifest', work / 'activity')
                run(sys.executable, 'tests/bridge/test_evidence.py', *attest)
    print('real custody signing, native verification, rollback, replay and evidence gates passed')


if __name__ == '__main__':
    main()
