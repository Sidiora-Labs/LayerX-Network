import argparse
import hashlib
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tests/support'))
from lxgb_metadata import metadata


def validate_reads(raw, salt, issuer_public, stage):
    lines = raw.decode().splitlines()
    assert len(lines) == 5
    records = bytes.fromhex(lines[1].split('payload=')[1])
    assert records[:2] == b'\0\1' and int.from_bytes(records[42:44], 'big') == 1
    record = records[46:]
    assert len(record) == int.from_bytes(records[44:46], 'big')
    did = ('did:layerx:' + issuer_public.hex()).encode()
    issuer = hashlib.sha256(b'LXP/v1/did-id\0' + len(did).to_bytes(2, 'big') + did).digest()
    asset = hashlib.sha256(b'LX:ASSET:v1' + issuer + salt).digest()
    assert record[:2] == b'\0\3' and record[2:34] == asset
    assert record[34:38] == b'\x03TOK' and record[38] == 6
    reference_length = int.from_bytes(record[40:42], 'big')
    offset = 42 + reference_length
    assert record[offset] == 0
    name_length = record[offset + 1]
    assert record[offset + 2:offset + 2 + name_length] == b'Token'
    offset += 2 + name_length
    assert int.from_bytes(record[offset:offset + 16], 'big') == 10000
    assert record[offset + 16:offset + 48] == issuer and record[offset + 48] == 1
    supply = 0 if stage < 3 else 9000 if stage == 3 else 8900
    assert int.from_bytes(record[offset + 49:offset + 65], 'big') == supply
    assert record[offset + 65:] == salt
    fee = bytes.fromhex(lines[2].split('payload=')[1])
    assert fee[2:42] == records[2:42]
    assert int.from_bytes(fee[46:62], 'big') == 0
    for actor, line in enumerate(lines[3:]):
        encoded = bytes.fromhex(line.split('payload=')[1])
        count = int.from_bytes(encoded[2:4], 'big')
        expected_count = int(stage >= (1 if actor == 0 else 2))
        assert count == expected_count
        cursor = 4
        for _ in range(count):
            cursor += 32
            length = int.from_bytes(encoded[cursor:cursor + 4], 'big'); cursor += 4
            value = encoded[cursor:cursor + length]; cursor += length
            name_length = int.from_bytes(value[:2], 'big')
            balance = int.from_bytes(value[3 + name_length:19 + name_length], 'big')
            assert value[19 + name_length:51 + name_length] == asset
            expected = (supply - (20 if stage == 7 else 0)) if actor == 0 else (20 if stage == 7 else 0)
            assert balance == expected
            proof_length = int.from_bytes(encoded[cursor:cursor + 4], 'big'); cursor += 4 + proof_length
        assert cursor == len(encoded)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--bin-dir', type=Path, required=True)
    parser.add_argument('--client', type=Path, required=True)
    parser.add_argument('--poll', action='store_true')
    args = parser.parse_args()
    work = args.output.resolve()
    work.mkdir(mode=0o755, parents=True, exist_ok=False)
    native = args.bin_dir.resolve()
    client = args.client.resolve()
    asset = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')
    public = {}
    for name, seed in [('sequencer', 0x22), ('treasury', 0x11), ('bob', 0x12)]:
        path = work / name
        with path.open('xb') as out:
            os.chmod(path, 0o600)
            out.write(bytes([seed]) * 32)
        public[name] = Ed25519PrivateKey.from_private_bytes(bytes([seed]) * 32).public_key().public_bytes_raw()
    salt = os.urandom(32)
    (work / 'salt').write_bytes(salt)
    (work / 'metadata').write_bytes(metadata(asset, public['treasury'], os.urandom(32)))
    ports = []
    sockets = []
    for _ in range(3):
        sock = socket.socket()
        sock.bind(('127.0.0.1', 0))
        sockets.append(sock)
        ports.append(sock.getsockname()[1])
    assert 18545 not in ports and 6379 not in ports
    environment = dict(os.environ, LAYERX_NODE_PAXEER_CHAIN_ID='31337',
                       LAYERX_NODE_SETTLEMENT_CONTRACT='0x' + '1' * 40,
                       LAYERX_NODE_CHECKPOINT_REGISTRY='0x' + '2' * 40,
                       LAYERX_NODE_PAXEER_RPC_ADDRESS='127.0.0.1',
                       LAYERX_NODE_PAXEER_RPC_PORT=str(ports[2]))
    bootstrap = ['bash', 'platform/hosted/node/bootstrap.sh', '--data-dir', str(work / 'data'),
                 '--run-dir', str(work / 'run'), '--network-id', '77',
                 '--sequencer-key', str(work / 'sequencer'), '--treasury-key', str(work / 'treasury'),
                 '--genesis-metadata', str(work / 'metadata'), '--lni-uid', '4021', '--lni-gid', '4021',
                 '--program-port', str(ports[0]), '--replica-port', str(ports[1]),
                 '--layerxd', str(native / 'layerxd'), '--genesis-build', str(native / 'layerx-genesis-build')]
    with (work / 'bootstrap.log').open('wb') as log:
        subprocess.run(bootstrap, cwd=ROOT, env=environment, stdout=log, stderr=log, check=True)
    bob_did = ('did:layerx:' + public['bob'].hex()).encode()
    with (work / 'data/identities.txt').open('a') as out:
        out.write(f'{bob_did.hex()}:{public["bob"].hex()}:0\n')
    for sock in sockets:
        sock.close()
    processes = []
    logs = []

    def start(role):
        log = (work / f'{role}.log').open('ab')
        logs.append(log)
        command = 'set -a; source "$1"; export LAYERX_PAY_TIMING=1; exec "$2" "$3" "$4"'
        process = subprocess.Popen(['bash', '-c', command, 'pay1', str(work / f'data/{role}.env'),
                                    str(native / 'layerxd'), '--serve' if role == 'sequencer' else '--authority-replica',
                                    str(work / f'data/{role}.conf')], cwd=ROOT, stdout=log, stderr=log)
        processes.append(process)
        return process

    def stop(process):
        process.terminate()
        process.wait(timeout=30)
        assert process.returncode == 0, f'daemon exit {process.returncode}'
        processes.remove(process)

    def ready(process, path=None, port=None):
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            assert process.poll() is None, f'daemon stopped: {process.returncode}'
            try:
                with socket.socket(socket.AF_UNIX if path else socket.AF_INET) as sock:
                    sock.settimeout(0.2)
                    sock.connect(str(path) if path else ('127.0.0.1', port))
                return
            except OSError:
                time.sleep(0.05)
        raise RuntimeError('daemon startup deadline')

    def invoke(operation, sequence, label):
        command = ['setpriv', '--reuid=4021', '--regid=4021', '--clear-groups', str(client),
                   str(work / 'run/layerxd.lni.sock'), str(work / 'salt'), operation, str(sequence),
                   'poll' if args.poll else 'wait']
        with (work / f'{label}.log').open('wb') as log:
            subprocess.run(command, stdout=log, stderr=log, check=True, timeout=30)
        return (work / f'{label}.log').read_bytes()

    try:
        replica = start('replica')
        ready(replica, port=ports[1])
        sequencer = start('sequencer')
        ready(sequencer, path=work / 'run/layerxd.lni.sock')
        steps = [('register', 0), ('open', 1), ('open-bob', 0), ('mint', 2), ('burn', 3),
                 ('grant-issue', 4), ('grant-revoke', 5), ('sends', 6)]
        for index, (operation, sequence) in enumerate(steps):
            invoke(operation, sequence, f'{index}-{operation}')
            before = invoke('read', 0, f'{index}-before')
            validate_reads(before, salt, public['treasury'], index)
            stop(sequencer)
            sequencer = start('sequencer')
            ready(sequencer, path=work / 'run/layerxd.lni.sock')
            after = invoke('read', 0, f'{index}-after')
            assert before == after, f'{operation}: committed reads changed after restart'
            print(f'{operation}: signed receipt verified; asset, fee and DID reads identical across restart', flush=True)
        stop(sequencer)
        stop(replica)
    finally:
        for process in processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
        for log in logs:
            log.close()


if __name__ == '__main__':
    main()
