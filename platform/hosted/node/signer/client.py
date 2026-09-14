#!/usr/bin/env python3
# Client for the LayerX treasury signer socket.
#
# Usage:
#   client.py --socket PATH public-key   -> the treasury public key hex
#   client.py --socket PATH did          -> did:layerx:<public key hex>
#   client.py --socket PATH sign HEX64   -> the signature hex over one digest
#   client.py --socket PATH --expected-public-key HEX64 bind HEX240 -> typed binding signature
import argparse
import json
import os
import re
import socket
import struct
import sys

DIGEST_PATTERN = re.compile(r'^[0-9a-f]{64}$')
MAXIMUM_REPLY_BYTES = 4096


class SignerError(Exception):
    pass


class SignerClient:
    def __init__(self, path, timeout=10.0, expected_peer_uid=None, expected_peer_gid=None,
                 expected_public_key=None):
        self._path = path
        self._timeout = timeout
        if (expected_peer_uid is None) != (expected_peer_gid is None) \
                or any(type(value) is not int or not 0 <= value <= 0xffffffff
                       for value in (expected_peer_uid, expected_peer_gid) if value is not None):
            raise SignerError('expected signer UID and GID must be paired unsigned integers')
        if expected_public_key is not None \
                and (not isinstance(expected_public_key, bytes) or len(expected_public_key) != 32):
            raise SignerError('expected treasury public key must hold 32 bytes')
        self._expected_peer = None if expected_peer_uid is None \
            else (expected_peer_uid, expected_peer_gid)
        self._expected_public_key = expected_public_key

    def request(self, line):
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.settimeout(self._timeout)
        try:
            try:
                connection.connect(self._path)
                if self._expected_peer is not None:
                    credentials = connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED,
                                                        struct.calcsize('iII'))
                    _, uid, gid = struct.unpack('iII', credentials)
                    if (uid, gid) != self._expected_peer:
                        raise SignerError('the treasury signer peer differs from its binding')
                connection.sendall((line + '\n').encode())
                buffered = b''
                while b'\n' not in buffered and len(buffered) <= MAXIMUM_REPLY_BYTES:
                    chunk = connection.recv(MAXIMUM_REPLY_BYTES)
                    if not chunk:
                        break
                    buffered += chunk
            except (OSError, socket.timeout) as error:
                raise SignerError(f'the treasury signer did not answer: {error}') from error
        finally:
            connection.close()
        if b'\n' not in buffered or len(buffered) > MAXIMUM_REPLY_BYTES:
            raise SignerError('the treasury signer reply is malformed')
        try:
            reply = json.loads(buffered.split(b'\n', 1)[0].decode())
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise SignerError('the treasury signer reply is not JSON') from error
        if not isinstance(reply, dict):
            raise SignerError('the treasury signer reply is not an object')
        if 'error' in reply:
            code = reply['error'].get('code') if isinstance(reply['error'], dict) else None
            raise SignerError(f'the treasury signer refused the request: {code}')
        if self._expected_public_key is not None \
                and reply.get('public_key') != self._expected_public_key.hex():
            raise SignerError('the treasury signer public key differs from its binding')
        return reply

    def public_key(self):
        reply = self.request('public-key')
        public_key = reply.get('public_key')
        if not isinstance(public_key, str) or not DIGEST_PATTERN.match(public_key):
            raise SignerError('the treasury signer answered no public key')
        return bytes.fromhex(public_key)

    def did(self):
        reply = self.request('public-key')
        did = reply.get('did')
        public_key = reply.get('public_key')
        if not isinstance(did, str) or not isinstance(public_key, str) \
                or did != 'did:layerx:' + public_key or not DIGEST_PATTERN.match(public_key):
            raise SignerError('the treasury signer answered no DID')
        return did

    def sign(self, digest):
        if len(digest) != 32:
            raise SignerError('a treasury signature covers a 32-byte digest')
        reply = self.request('sign ' + digest.hex())
        signature = reply.get('signature')
        if reply.get('digest') != digest.hex():
            raise SignerError('the treasury signer answered a different digest')
        if not isinstance(signature, str) or len(signature) != 128 \
                or not re.match(r'^[0-9a-f]{128}$', signature):
            raise SignerError('the treasury signer answered no signature')
        return bytes.fromhex(signature)

    def bind(self, network_id, account, asset, recipient, checkpoint):
        if self._expected_public_key is None:
            raise SignerError('recipient binding requires an explicit treasury public key')
        if type(network_id) is not int or not 0 < network_id <= 0xffffffff \
                or any(not isinstance(value, bytes) or len(value) != length or not any(value)
                       for value, length in ((account, 32), (asset, 32), (recipient, 20),
                                             (checkpoint, 32))):
            raise SignerError('recipient binding coordinates are invalid')
        binding = network_id.to_bytes(4, 'big') + account + asset + recipient + checkpoint
        reply = self.request('bind ' + binding.hex())
        signature = reply.get('signature')
        if set(reply) != {'public_key', 'binding', 'signature'} \
                or reply.get('binding') != binding.hex() \
                or not isinstance(signature, str) \
                or re.fullmatch('[0-9a-f]{128}', signature) is None:
            raise SignerError('the treasury signer answered a different recipient binding')
        signature = bytes.fromhex(signature)
        if __package__:
            from .provider import verify_binding
        else:
            from provider import verify_binding
        if not verify_binding(self._expected_public_key, binding, signature):
            raise SignerError('the treasury recipient binding signature does not verify')
        return signature


def main(argv):
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument('--socket', required=True)
    parser.add_argument('--timeout-seconds', default=10.0, type=float)
    parser.add_argument('--expected-peer-uid', type=int)
    parser.add_argument('--expected-peer-gid', type=int)
    parser.add_argument('--expected-public-key')
    parser.add_argument('request', choices=('public-key', 'did', 'sign', 'bind'))
    parser.add_argument('digest', nargs='?', default=None)
    arguments = parser.parse_args(argv)
    if arguments.expected_public_key is not None \
            and re.fullmatch('[0-9a-f]{64}', arguments.expected_public_key) is None:
        raise SignerError('expected public key takes 64 lowercase hex characters')
    client = SignerClient(arguments.socket, arguments.timeout_seconds,
                          arguments.expected_peer_uid, arguments.expected_peer_gid,
                          bytes.fromhex(arguments.expected_public_key)
                          if arguments.expected_public_key else None)
    if arguments.request == 'bind':
        if arguments.digest is None or re.fullmatch('[0-9a-f]{240}', arguments.digest) is None:
            raise SignerError('bind takes one 240-character lowercase hex binding')
        binding = bytes.fromhex(arguments.digest)
        signature = client.bind(int.from_bytes(binding[:4], 'big'), binding[4:36],
                                binding[36:68], binding[68:88], binding[88:120])
        sys.stdout.write(signature.hex() + '\n')
        return 0
    if arguments.request == 'public-key':
        if arguments.digest is not None:
            raise SignerError('public-key takes no argument')
        sys.stdout.write(client.public_key().hex() + '\n')
        return 0
    if arguments.request == 'did':
        if arguments.digest is not None:
            raise SignerError('did takes no argument')
        sys.stdout.write(client.did() + '\n')
        return 0
    if arguments.digest is None or not DIGEST_PATTERN.match(arguments.digest):
        raise SignerError('sign takes one 64-character lowercase hex digest')
    sys.stdout.write(client.sign(bytes.fromhex(arguments.digest)).hex() + '\n')
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main(sys.argv[1:]))
    except SignerError as failure:
        sys.exit(f'treasury-signer-client: {failure}')
