import json
import errno
import os
from pathlib import Path
import socket
import stat
import struct
import subprocess

from provision import protected_json, require, strict_pairs

MAX_PACKET = 8192
DEADLINE = 30


def peer(connection, uid, gid):
    _, observed_uid, observed_gid = struct.unpack('3i', connection.getsockopt(
        socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i')))
    require((observed_uid, observed_gid) == (uid, gid), 'onboarding signer', 'authenticated process peer')


def packet(connection):
    data, _, flags, _ = connection.recvmsg(MAX_PACKET)
    require(data and not flags & (socket.MSG_TRUNC | socket.MSG_CTRUNC),
            'onboarding signer', 'complete bounded packet')
    value = json.loads(data, object_pairs_hook=strict_pairs)
    require(type(value) is dict, 'onboarding signer', 'typed packet')
    return value


def send(connection, value):
    data = json.dumps(value, separators=(',', ':')).encode()
    require(len(data) <= MAX_PACKET, 'onboarding signer', 'response bound')
    require(connection.send(data) == len(data), 'onboarding signer', 'atomic packet send')


def request(configuration, operation, body):
    path = Path(configuration['socket'])
    info = path.lstat()
    require(path.is_absolute() and path.parent.resolve() == path.parent
            and stat.S_ISSOCK(info.st_mode) and info.st_uid == configuration['peer_uid']
            and info.st_gid == configuration['peer_gid'] and stat.S_IMODE(info.st_mode) == 0o660,
            path, 'protected signer endpoint')
    with socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET) as connection:
        connection.settimeout(DEADLINE)
        connection.connect(str(path))
        peer(connection, configuration['peer_uid'], configuration['peer_gid'])
        send(connection, dict(version=1, operation=operation, body=body))
        response = packet(connection)
    require(set(response) == {'version', 'result'} and response['version'] == 1
            and type(response['result']) is dict, path, 'successful signer response')
    return response['result']


def serve(configuration_file):
    configuration = protected_json(configuration_file)
    require(set(configuration) == {'socket', 'client_uid', 'client_gid', 'executable', 'principal'},
            configuration_file, 'signer configuration')
    path = Path(configuration['socket'])
    parent = path.parent.lstat()
    require(path.is_absolute() and path.parent.resolve() == path.parent and stat.S_ISDIR(parent.st_mode)
            and parent.st_uid == os.geteuid() and parent.st_gid == os.getegid()
            and stat.S_IMODE(parent.st_mode) == 0o750,
            path, 'private signer runtime directory')
    prepare_socket(path)
    require(configuration['client_uid'] != os.geteuid()
            and type(configuration['client_uid']) is int and 0 < configuration['client_uid'] < 2**32
            and type(configuration['client_gid']) is int and 0 <= configuration['client_gid'] < 2**32,
            path, 'distinct explicit native peer')
    executable = Path(configuration['executable'])
    require(executable.is_absolute() and executable.resolve() == executable and executable.is_file(),
            executable, 'actual onboarding executable')
    with socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET) as listener:
        listener.bind(str(path))
        path.chmod(0o660)
        inode = path.lstat().st_ino
        listener.listen(4)
        try:
            while True:
                connection, _ = listener.accept()
                with connection:
                    connection.settimeout(DEADLINE)
                    try:
                        peer(connection, configuration['client_uid'], configuration['client_gid'])
                        value = packet(connection)
                        require(set(value) == {'version', 'operation', 'body'} and value['version'] == 1
                                and value['operation'] in ('sign', 'settlement-recipient') and type(value['body']) is dict
                                and value['body'].get('principal') == configuration['principal'],
                                path, 'configured sponsor operation')
                        completed = subprocess.run([str(executable), value['operation']],
                            input=json.dumps(value['body']).encode(), stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL, timeout=DEADLINE, check=True)
                        require(len(completed.stdout) <= MAX_PACKET, path, 'signer output bound')
                        send(connection, dict(version=1, result=json.loads(completed.stdout)))
                    except (OSError, ValueError, subprocess.SubprocessError):
                        try:
                            send(connection, dict(version=1, error='onboarding_request_refused'))
                        except OSError:
                            pass
        finally:
            current = path.lstat()
            if stat.S_ISSOCK(current.st_mode) and current.st_ino == inode:
                path.unlink()


def prepare_socket(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return
    require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.geteuid()
            and info.st_gid == os.getegid() and stat.S_IMODE(info.st_mode) == 0o660,
            path, 'owned stale signer endpoint')
    with socket.socket(socket.AF_UNIX, socket.SOCK_SEQPACKET) as probe:
        probe.setblocking(False)
        require(probe.connect_ex(str(path)) == errno.ECONNREFUSED, path, 'inactive signer endpoint')
    current = path.lstat()
    require((current.st_dev, current.st_ino) == (info.st_dev, info.st_ino), path, 'unchanged signer endpoint')
    path.unlink()


if __name__ == '__main__':
    import sys
    if len(sys.argv) != 2:
        raise SystemExit('onboarding signer configuration required')
    serve(sys.argv[1])
