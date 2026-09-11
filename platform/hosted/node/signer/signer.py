#!/usr/bin/env python3
# LayerX treasury signer.
#
# The treasury Ed25519 identity is held by this process alone. Every other
# component asks for a signature over a 32-byte digest across a unix socket
# instead of reading the seed, so the seed never reaches the node data
# directory, the node environment files or any other container.
#
# Usage:
#   signer.py --socket PATH --allowed-uid U[,U...] [options]
#
# Required:
#   --socket PATH           Unix socket to listen on. Its directory is created
#                           mode 0700 when it is missing; the socket itself is
#                           mode 0600, or 0660 with --socket-group.
#   --allowed-uid U[,U...]  Peer uids admitted by SO_PEERCRED. The signer's own
#                           effective uid is always admitted.
#
# Options:
#   --provider file|command   Signing provider. Default file.
#   --key-file PATH           File provider material: 32 raw bytes or 64 hex
#                             characters, a regular owned file that is not
#                             readable by others and not writable by its group.
#   --provider-command STRING Command provider program (see provider.py).
#   --provider-timeout-seconds N  Command provider deadline. Default 10.
#   --socket-group GID        Group that owns the socket; the socket is 0660.
#   --public-key-file PATH    Publish the treasury public key hex (mode 0644).
#   --request-timeout-seconds N   Per-connection deadline. Default 5.
#
# Requests are one line; the reply is one JSON object and the connection closes:
#
#   public-key\n     -> {"public_key":"<64 hex>","did":"did:layerx:<64 hex>",
#                        "provider":"file"}
#   sign <64 hex>\n  -> {"public_key":"<64 hex>","digest":"<64 hex>",
#                        "signature":"<128 hex>"}
#   anything else    -> {"error":{"code":"unknown_request","retry":"never"}}
import argparse
import json
import os
import re
import signal
import socket
import socketserver
import stat
import struct
import sys
import threading

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from provider import ProviderError, load, verify

DIGEST_PATTERN = re.compile(r'^[0-9a-f]{64}$')
MAXIMUM_REQUEST_BYTES = 256


class Signer:
    def __init__(self, backend, allowed_uids):
        self._backend = backend
        self._allowed_uids = frozenset(allowed_uids) | {os.geteuid()}
        self._lock = threading.Lock()
        self._public_key = backend.public_key()
        if len(self._public_key) != 32:
            raise ProviderError('the provider advertised no Ed25519 public key')

    @property
    def public_key(self):
        return self._public_key

    @property
    def did(self):
        return 'did:layerx:' + self._public_key.hex()

    @property
    def provider_name(self):
        return self._backend.name

    def admits(self, uid):
        return uid in self._allowed_uids

    def sign(self, digest):
        with self._lock:
            if self._backend.public_key() != self._public_key:
                raise ProviderError('the provider changed its public key')
            signature = self._backend.sign(digest)
        if not verify(self._public_key, digest, signature):
            raise ProviderError('the provider signature does not verify')
        return signature

    def close(self):
        self._backend.close()


class Handler(socketserver.BaseRequestHandler):
    def handle(self):
        self.request.settimeout(self.server.request_timeout)
        try:
            credentials = self.request.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED,
                                                  struct.calcsize('3i'))
            _, uid, _ = struct.unpack('3i', credentials)
            if not self.server.signer.admits(uid):
                self.reply({'error': {'code': 'peer_refused', 'retry': 'never'}})
                self.drain()
                return
            self.reply(self.answer(self.read_line()))
        except (OSError, socket.timeout):
            return

    def drain(self):
        # A refused peer is answered before its request line is read, so the
        # bytes still in flight are taken off the connection: closing over
        # unread data resets it and destroys the refusal the peer must see.
        self.request.shutdown(socket.SHUT_WR)
        remaining = MAXIMUM_REQUEST_BYTES
        while remaining > 0:
            chunk = self.request.recv(remaining)
            if not chunk:
                return
            remaining -= len(chunk)

    def read_line(self):
        buffered = b''
        while b'\n' not in buffered:
            if len(buffered) > MAXIMUM_REQUEST_BYTES:
                return ''
            chunk = self.request.recv(MAXIMUM_REQUEST_BYTES)
            if not chunk:
                break
            buffered += chunk
        line = buffered.split(b'\n', 1)[0]
        if len(line) > MAXIMUM_REQUEST_BYTES:
            return ''
        return line.decode('ascii', 'replace').strip()

    def answer(self, line):
        signer = self.server.signer
        if line == 'public-key':
            return {'public_key': signer.public_key.hex(), 'did': signer.did,
                    'provider': signer.provider_name}
        if line.startswith('sign '):
            digest = line[5:]
            if not DIGEST_PATTERN.match(digest):
                return {'error': {'code': 'digest_refused', 'retry': 'never'}}
            try:
                signature = signer.sign(bytes.fromhex(digest))
            except ProviderError:
                return {'error': {'code': 'signature_refused', 'retry': 'never'}}
            return {'public_key': signer.public_key.hex(), 'digest': digest,
                    'signature': signature.hex()}
        return {'error': {'code': 'unknown_request', 'retry': 'never'}}

    def reply(self, message):
        self.request.sendall((json.dumps(message, separators=(',', ':')) + '\n').encode())


class Server(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True
    allow_reuse_address = False
    request_queue_size = 32


def positive(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('must be a positive integer')
    return number


def uid_list(value):
    uids = []
    for item in value.split(','):
        item = item.strip()
        if not item.isdigit():
            raise argparse.ArgumentTypeError('--allowed-uid takes decimal uids')
        uids.append(int(item))
    if not uids:
        raise argparse.ArgumentTypeError('--allowed-uid takes at least one uid')
    return uids


def prepare_socket_path(path):
    if len(path.encode()) >= 108:
        raise ValueError('the signer socket path is too long')
    directory = os.path.dirname(path) or '.'
    os.makedirs(directory, mode=0o700, exist_ok=True)
    try:
        existing = os.lstat(path)
    except FileNotFoundError:
        return
    if not stat.S_ISSOCK(existing.st_mode):
        raise ValueError('the signer socket path is not a socket')
    os.unlink(path)


def publish_public_key(path, public_key):
    temporary = path + '.tmp'
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW, 0o644)
    with os.fdopen(descriptor, 'w') as handle:
        handle.write(public_key.hex() + '\n')
        handle.flush()
        os.fsync(handle.fileno())
    os.chmod(temporary, 0o644)
    os.replace(temporary, path)


def main(argv):
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument('--socket', required=True)
    parser.add_argument('--allowed-uid', required=True, type=uid_list)
    parser.add_argument('--provider', default='file', choices=('file', 'command'))
    parser.add_argument('--key-file', default='')
    parser.add_argument('--provider-command', default='')
    parser.add_argument('--provider-timeout-seconds', default=10, type=positive)
    parser.add_argument('--socket-group', default=None, type=int)
    parser.add_argument('--public-key-file', default='')
    parser.add_argument('--request-timeout-seconds', default=5, type=positive)
    arguments = parser.parse_args(argv)

    backend = load(arguments.provider, arguments.key_file, arguments.provider_command,
                   arguments.provider_timeout_seconds)
    try:
        signer = Signer(backend, arguments.allowed_uid)
    except BaseException:
        backend.close()
        raise
    if arguments.public_key_file:
        publish_public_key(arguments.public_key_file, signer.public_key)
    prepare_socket_path(arguments.socket)
    previous = os.umask(0o177 if arguments.socket_group is None else 0o117)
    try:
        server = Server(arguments.socket, Handler)
    finally:
        os.umask(previous)
    server.signer = signer
    server.request_timeout = arguments.request_timeout_seconds
    if arguments.socket_group is not None:
        os.chown(arguments.socket, -1, arguments.socket_group)
        os.chmod(arguments.socket, 0o660)
    else:
        os.chmod(arguments.socket, 0o600)

    def stop(_number, _frame):
        threading.Thread(target=server.shutdown, daemon=True).start()

    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    sys.stderr.write(f'treasury-signer: {signer.provider_name} provider serving {arguments.socket}\n')
    sys.stderr.flush()
    try:
        server.serve_forever(poll_interval=0.1)
    finally:
        server.server_close()
        signer.close()
        try:
            os.unlink(arguments.socket)
        except FileNotFoundError:
            pass
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main(sys.argv[1:]))
    except (ProviderError, ValueError, OSError) as failure:
        sys.exit(f'treasury-signer: {failure}')
