#!/usr/bin/env python3
# Client for the LayerX treasury signer socket.
#
# Usage:
#   client.py --socket PATH public-key   -> the treasury public key hex
#   client.py --socket PATH did          -> did:layerx:<public key hex>
#   client.py --socket PATH sign HEX64   -> the signature hex over one digest
import argparse
import json
import os
import re
import socket
import sys

DIGEST_PATTERN = re.compile(r'^[0-9a-f]{64}$')
MAXIMUM_REPLY_BYTES = 4096


class SignerError(Exception):
    pass


class SignerClient:
    def __init__(self, path, timeout=10.0):
        self._path = path
        self._timeout = timeout

    def request(self, line):
        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        connection.settimeout(self._timeout)
        try:
            try:
                connection.connect(self._path)
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


def main(argv):
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument('--socket', required=True)
    parser.add_argument('--timeout-seconds', default=10.0, type=float)
    parser.add_argument('request', choices=('public-key', 'did', 'sign'))
    parser.add_argument('digest', nargs='?', default=None)
    arguments = parser.parse_args(argv)
    client = SignerClient(arguments.socket, arguments.timeout_seconds)
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
