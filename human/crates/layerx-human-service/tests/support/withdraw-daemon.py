#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[5]


def main():
    work = Path(sys.argv[1])
    work.mkdir(mode=0o755)
    native = Path(os.environ.get('LAYERX_TEST_NATIVE_BIN_DIR', ROOT / 'build/bin'))
    sign_credit = Path(os.environ.get('LAYERX_TEST_SIGN_CREDIT_BIN', native.parent / 'tests/bridge/sign-credit'))
    uid = 4022 if os.getuid() != 4022 else 4023
    if os.getuid() != 0:
        raise RuntimeError('the native withdrawal fixture requires root to isolate its daemon uid')
    binaries = work / 'bin'
    binaries.mkdir()
    for name in ('layerxd', 'layerx-genesis-build', 'layerx-handover'):
        shutil.copy2(native / name, binaries / name)
    shutil.copy2(sign_credit, binaries / 'sign-credit')
    node = work / 'node'
    node.mkdir()
    for name in ('bootstrap.sh', 'data_directory.py', 'genesis_fees.py', 'genesis-modules.conf', 'sequencer-env.sh'):
        shutil.copy2(ROOT / 'platform/hosted/node' / name, node / name)
    shutil.copy2(ROOT / 'migrations/0007_history_index.sql', work / 'migrations.sql')
    shutil.copy2(ROOT / 'contracts/config/checkpoint-settlement.json', work / 'settlement.json')
    shutil.copy2(ROOT / 'tests/fixtures/custody/paxeer-state-v2/custody.profile', work / 'profile')
    shutil.copy2(ROOT / 'tests/fixtures/custody/paxeer-state-v2/custody.credit', work / 'credit')
    sys.path.insert(0, str(ROOT / 'tests/support'))
    from lxgb_metadata import metadata_withdrawal
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
    owner_seed = bytes([0x11]) * 32
    owner = Ed25519PrivateKey.from_private_bytes(owner_seed).public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    for name, value in (('owner', owner_seed), ('sequencer', bytes([0x22]) * 32)):
        (work / name).write_bytes(value)
        (work / name).chmod(0o600)
    asset = bytes.fromhex('b5a32b12029f8ddfb905f90f280f664b46390de0fc62770fc197dd87b18cd898')
    (work / 'metadata').write_bytes(metadata_withdrawal(asset, owner, os.urandom(32), 7))
    validated = subprocess.run(['bash', str(node / 'bootstrap.sh'), '--check-settlement',
                                str(ROOT / 'platform/hosted/node/tests/fixtures/settlement-configuration.txt')],
                               check=True, capture_output=True).stdout
    (work / 'settlement.env').write_bytes(validated)
    ports = []
    for _ in range(2):
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            ports.append(listener.getsockname()[1])
    for path in (work, *work.rglob('*')):
        os.chown(path, uid, uid)
    environment = {key: value for key, value in os.environ.items() if not key.startswith('LAYERX_NODE_')}
    environment['PATH'] = str(binaries) + os.pathsep + environment.get('PATH', '')
    children = []
    try:
        with (work / 'bootstrap.log').open('wb') as output:
            subprocess.run(['bash', str(node / 'bootstrap.sh'), '--data-dir', str(work / 'data'),
                            '--run-dir', str(work / 'run'), '--network-id', '77',
                            '--genesis-metadata', str(work / 'metadata'), '--custody-profile', str(work / 'profile'),
                            '--sequencer-key', str(work / 'sequencer'), '--treasury-key', str(work / 'owner'),
                            '--settlement-env', str(work / 'settlement.env'), '--settlement-document', str(work / 'settlement.json'),
                            '--migrations', str(work / 'migrations.sql'), '--lni-uid', str(os.getuid()), '--lni-gid', str(os.getgid()),
                            '--program-port', str(ports[0]), '--replica-port', str(ports[1]),
                            '--layerxd', str(binaries / 'layerxd'), '--genesis-build', str(binaries / 'layerx-genesis-build')],
                           check=True, stdout=output, stderr=subprocess.STDOUT, cwd=work, env=environment,
                           user=uid, group=uid, extra_groups=[], timeout=60)
        subprocess.run([str(binaries / 'sign-credit'), str(work / 'profile'), str(work / 'credit'),
                        'did:layerx:' + owner.hex(), str(work / 'owner'), '0', str(int(time.time() * 1000)),
                        str(work / 'credit.activity')], check=True, timeout=20)
        read_fd, write_fd = os.pipe()
        with (work / 'replica.log').open('wb') as output:
            replica = subprocess.Popen(['bash', '-c', 'set -a; source "$1"; exec "$2" --authority-replica "$3"',
                                        'withdraw-replica', str(work / 'data/replica.env'), str(binaries / 'layerxd'),
                                        str(work / 'data/replica.conf')], stdout=output, stderr=subprocess.STDOUT,
                                       env=environment | {'LAYERX_AUTHORITY_READY_FD': str(write_fd)}, pass_fds=(write_fd,),
                                       user=uid, group=uid, extra_groups=[])
        children.append(replica)
        os.close(write_fd)
        import select
        if not select.select([read_fd], [], [], 20)[0] or os.read(read_fd, 1) != b'R':
            raise RuntimeError('real replica readiness failed')
        os.close(read_fd)
        with (work / 'sequencer.log').open('wb') as output:
            sequencer = subprocess.Popen(['bash', '-c', 'source "$1"; layerx_sequencer_environment "$2"; exec "$3" --serve "$4"',
                                          'withdraw-sequencer', str(node / 'sequencer-env.sh'), str(work / 'data/sequencer.env'),
                                          str(binaries / 'layerxd'), str(work / 'data/sequencer.conf')], stdout=output,
                                         stderr=subprocess.STDOUT, env=environment, user=uid, group=uid, extra_groups=[])
        children.append(sequencer)
        endpoint = work / 'run/layerxd.lni.sock'
        for _ in range(200):
            if any(child.poll() is not None for child in children):
                raise RuntimeError('native fixture daemon exited before readiness')
            try:
                with socket.socket(socket.AF_UNIX) as connection:
                    connection.connect(str(endpoint))
                break
            except OSError:
                time.sleep(0.1)
        else:
            raise RuntimeError('native fixture LNI readiness timed out')
        (work / 'ready.json').write_text(json.dumps({'socket': str(endpoint), 'credit': str(work / 'credit.activity')}))
        sys.stdin.buffer.read()
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()


if __name__ == '__main__':
    main()
