#!/usr/bin/env python3
"""EVM key, ABI and JSON-RPC transaction helper for the Paxeer bring-up.

The bring-up talks to the native LayerX precompiles with plain JSON-RPC: this module derives
addresses, encodes calldata, signs EIP-155 transactions and waits for receipts. Secret-key
operations use the `cryptography` package the bring-up already requires; Keccak-256 and the
public recovery-id computation are implemented here because the Python standard library offers
neither.

  evm.py address KEY_FILE                       checksummed address of the key
  evm.py public-key KEY_FILE                    compressed secp256k1 public key
  evm.py keccak HEX
  evm.py checksum ADDRESS
  evm.py calldata 'name(types)' ARG...
  evm.py chain-id --rpc URL
  evm.py balance --rpc URL ADDRESS
  evm.py call --rpc URL TO 'name(types)(outputs)' ARG...     JSON list of the outputs; integers as decimal strings
  evm.py send --rpc URL --chain N --key-file F [--value WEI] TO ['name(types)' ARG...]

Key files hold one 0x-prefixed 32-byte secret; keys never appear on a command line. TLS trust
comes from --ca PEM or SSL_CERT_FILE.
"""
import json
import ssl
import sys
import time
import urllib.request

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.asymmetric.utils import Prehashed, decode_dss_signature

P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
G = (0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
     0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8)

_ROUND = [0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
          0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
          0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
          0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
          0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
          0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008]
_ROTATION = [[0, 36, 3, 41, 18], [1, 44, 10, 45, 2], [62, 6, 43, 15, 61],
             [28, 55, 25, 21, 56], [27, 20, 39, 8, 14]]
_MASK = (1 << 64) - 1


def _rol(value, shift):
    return ((value << shift) | (value >> (64 - shift))) & _MASK if shift else value


def _permute(a):
    for constant in _ROUND:
        c = [a[x][0] ^ a[x][1] ^ a[x][2] ^ a[x][3] ^ a[x][4] for x in range(5)]
        d = [c[(x - 1) % 5] ^ _rol(c[(x + 1) % 5], 1) for x in range(5)]
        a = [[a[x][y] ^ d[x] for y in range(5)] for x in range(5)]
        b = [[0] * 5 for _ in range(5)]
        for x in range(5):
            for y in range(5):
                b[y][(2 * x + 3 * y) % 5] = _rol(a[x][y], _ROTATION[x][y])
        a = [[b[x][y] ^ (~b[(x + 1) % 5][y] & _MASK & b[(x + 2) % 5][y]) for y in range(5)] for x in range(5)]
        a[0][0] ^= constant
    return a


def keccak(data):
    """Keccak-256 with the original 0x01 padding, as the EVM uses it."""
    data = bytes(data)
    rate = 136
    padded = bytearray(data)
    padded.append(0x01)
    padded.extend(bytes(-len(padded) % rate))
    padded[-1] |= 0x80
    state = [[0] * 5 for _ in range(5)]
    for offset in range(0, len(padded), rate):
        block = padded[offset:offset + rate]
        for index in range(rate // 8):
            state[index % 5][index // 5] ^= int.from_bytes(block[8 * index:8 * index + 8], 'little')
        state = _permute(state)
    return b''.join(state[index % 5][index // 5].to_bytes(8, 'little') for index in range(4))


def unhex(value, length=None):
    if not isinstance(value, str) or not value.startswith(('0x', '0X')):
        raise ValueError('expected 0x-prefixed hex')
    raw = bytes.fromhex(value[2:])
    if length is not None and len(raw) != length:
        raise ValueError('expected %d bytes of hex' % length)
    return raw


def checksum(address):
    raw = address if isinstance(address, bytes) else unhex(address, 20)
    if len(raw) != 20:
        raise ValueError('address is not 20 bytes')
    lower = raw.hex()
    digest = keccak(lower.encode('ascii')).hex()
    return '0x' + ''.join(c.upper() if int(digest[i], 16) >= 8 else c for i, c in enumerate(lower))


def read_key(path):
    with open(path, 'r', encoding='ascii') as handle:
        text = handle.read().strip()
    secret = int.from_bytes(unhex(text if text.startswith(('0x', '0X')) else '0x' + text, 32), 'big')
    if not 0 < secret < N:
        raise ValueError('key file does not hold a secp256k1 secret')
    return secret


def public_point(secret):
    numbers = ec.derive_private_key(secret, ec.SECP256K1()).public_key().public_numbers()
    return numbers.x, numbers.y


def compressed_public_key(secret):
    x, y = public_point(secret)
    return bytes([2 + (y & 1)]) + x.to_bytes(32, 'big')


def address_of_point(point):
    return keccak(point[0].to_bytes(32, 'big') + point[1].to_bytes(32, 'big'))[12:]


def address_of(secret):
    return address_of_point(public_point(secret))


def decompress(compressed):
    compressed = bytes(compressed)
    if len(compressed) != 33 or compressed[0] not in (2, 3):
        raise ValueError('not a compressed secp256k1 public key')
    numbers = ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256K1(), compressed).public_numbers()
    return numbers.x, numbers.y


def _add(a, b):
    if a is None:
        return b
    if b is None:
        return a
    if a[0] == b[0]:
        if (a[1] + b[1]) % P == 0:
            return None
        slope = 3 * a[0] * a[0] * pow(2 * a[1], -1, P) % P
    else:
        slope = (b[1] - a[1]) * pow(b[0] - a[0], -1, P) % P
    x = (slope * slope - a[0] - b[0]) % P
    return x, (slope * (a[0] - x) - a[1]) % P


def _mul(point, scalar):
    result = None
    while scalar:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def recover(digest, r, s, parity):
    """Public point that signed digest with (r, s); only public values are involved."""
    y = pow((pow(r, 3, P) + 7) % P, (P + 1) // 4, P)
    if y * y % P != (pow(r, 3, P) + 7) % P:
        return None
    if y & 1 != parity:
        y = P - y
    inverse = pow(r, -1, N)
    z = int.from_bytes(digest, 'big')
    return _add(_mul((r, y), s * inverse % N), _mul(G, -z * inverse % N))


def sign_digest(secret, digest):
    """(parity, r, s) with low s over a 32-byte digest."""
    key = ec.derive_private_key(secret, ec.SECP256K1())
    r, s = decode_dss_signature(key.sign(digest, ec.ECDSA(Prehashed(hashes.SHA256()))))
    if s > N // 2:
        s = N - s
    expected = public_point(secret)
    for parity in (0, 1):
        if recover(digest, r, s, parity) == expected:
            return parity, r, s
    raise ValueError('signature does not recover to the signing key')


def rlp(item):
    if isinstance(item, int):
        item = item.to_bytes((item.bit_length() + 7) // 8, 'big')
    if isinstance(item, (bytes, bytearray)):
        item = bytes(item)
        if len(item) == 1 and item[0] < 0x80:
            return item
        return _rlp_length(len(item), 0x80) + item
    body = b''.join(rlp(entry) for entry in item)
    return _rlp_length(len(body), 0xC0) + body


def _rlp_length(length, offset):
    if length < 56:
        return bytes([offset + length])
    encoded = length.to_bytes((length.bit_length() + 7) // 8, 'big')
    return bytes([offset + 55 + len(encoded)]) + encoded


def sign_transaction(secret, chain_id, nonce, gas_price, gas, to, value, data):
    fields = [nonce, gas_price, gas, to, value, data]
    parity, r, s = sign_digest(secret, keccak(rlp(fields + [chain_id, 0, 0])))
    return rlp(fields + [chain_id * 2 + 35 + parity, r, s])


def _split(text):
    parts, depth, current = [], 0, ''
    for char in text:
        if char == ',' and depth == 0:
            parts.append(current)
            current = ''
            continue
        depth += char in '(['
        depth -= char in ')]'
        current += char
    if current:
        parts.append(current)
    return [part.strip() for part in parts]


def parse_signature(signature):
    """'name(inputs)' or 'name(inputs)(outputs)' -> (name, input types, output types)."""
    start = signature.index('(')
    depth = 0
    for end in range(start, len(signature)):
        depth += signature[end] == '('
        depth -= signature[end] == ')'
        if depth == 0:
            break
    else:
        raise ValueError('unbalanced signature')
    outputs = signature[end + 1:]
    if outputs and not (outputs.startswith('(') and outputs.endswith(')')):
        raise ValueError('malformed output types')
    return signature[:start], _split(signature[start + 1:end]), _split(outputs[1:-1]) if outputs else []


def selector(signature):
    name, inputs, _ = parse_signature(signature)
    return keccak((name + '(' + ','.join(inputs) + ')').encode('ascii'))[:4]


def _dynamic(kind):
    return kind in ('bytes', 'string')


def _integer(value):
    if isinstance(value, bool):
        raise ValueError('boolean is not an integer')
    return value if isinstance(value, int) else int(value, 0)


def _encode_static(kind, value):
    if kind == 'address':
        return bytes(12) + (value if isinstance(value, bytes) else unhex(value, 20))
    if kind == 'bool':
        if value in (True, 'true'):
            return (1).to_bytes(32, 'big')
        if value in (False, 'false'):
            return bytes(32)
        raise ValueError('bool argument must be true or false')
    if kind.startswith('uint'):
        bits = int(kind[4:] or 256)
        number = _integer(value)
        if not 0 <= number < 1 << bits:
            raise ValueError('%s out of range' % kind)
        return number.to_bytes(32, 'big')
    if kind.startswith('bytes'):
        width = int(kind[5:])
        raw = value if isinstance(value, bytes) else unhex(value, width)
        if len(raw) != width or not 1 <= width <= 32:
            raise ValueError('%s width' % kind)
        return raw + bytes(32 - width)
    raise ValueError('unsupported ABI type ' + kind)


def encode(types, values):
    if len(types) != len(values):
        raise ValueError('expected %d arguments, got %d' % (len(types), len(values)))
    head, tail = [], b''
    for kind, value in zip(types, values):
        if _dynamic(kind):
            raw = value.encode() if kind == 'string' else (value if isinstance(value, bytes) else unhex(value))
            head.append(None)
            tail_entry = len(raw).to_bytes(32, 'big') + raw + bytes(-len(raw) % 32)
            head[-1] = tail_entry
        else:
            head.append(_encode_static(kind, value))
    offset = 32 * len(types)
    out = b''
    for kind, entry in zip(types, head):
        if _dynamic(kind):
            out += offset.to_bytes(32, 'big')
            offset += len(entry)
            tail += entry
        else:
            out += entry
    return out + tail


def calldata(signature, *values):
    _, inputs, _ = parse_signature(signature)
    return '0x' + (selector(signature) + encode(inputs, list(values))).hex()


def decode(types, data):
    data = bytes(data)
    if len(data) < 32 * len(types):
        raise ValueError('return data shorter than its head')
    values = []
    for index, kind in enumerate(types):
        word = data[32 * index:32 * index + 32]
        if _dynamic(kind):
            offset = int.from_bytes(word, 'big')
            length = int.from_bytes(data[offset:offset + 32], 'big')
            raw = data[offset + 32:offset + 32 + length]
            if len(raw) != length:
                raise ValueError('truncated dynamic return value')
            values.append(raw.decode() if kind == 'string' else '0x' + raw.hex())
        elif kind == 'address':
            if word[:12] != bytes(12):
                raise ValueError('address return value has high bits')
            values.append(checksum(word[12:]))
        elif kind == 'bool':
            if int.from_bytes(word, 'big') > 1:
                raise ValueError('bool return value out of range')
            values.append(word[31] == 1)
        elif kind.startswith('uint'):
            values.append(int.from_bytes(word, 'big'))
        elif kind.startswith('bytes'):
            values.append('0x' + word[:int(kind[5:])].hex())
        else:
            raise ValueError('unsupported ABI type ' + kind)
    return values


class Rpc:
    def __init__(self, url, ca=None, timeout=30):
        self.url = url
        self.timeout = timeout
        self.context = None
        if url.startswith('https://'):
            self.context = ssl.create_default_context(cafile=ca)
        elif not url.startswith('http://'):
            raise ValueError('RPC URL must be http or https')
        self.serial = 0

    def request(self, method, params):
        self.serial += 1
        body = json.dumps({'jsonrpc': '2.0', 'id': self.serial, 'method': method, 'params': params}).encode()
        request = urllib.request.Request(self.url, body, {'Content-Type': 'application/json'})
        with urllib.request.urlopen(request, timeout=self.timeout, context=self.context) as response:
            answer = json.loads(response.read())
        if answer.get('id') != self.serial:
            raise ValueError('JSON-RPC answer id mismatch')
        if answer.get('error') is not None:
            raise ValueError('%s refused: %s' % (method, json.dumps(answer['error'])))
        return answer.get('result')

    def chain_id(self):
        return int(self.request('eth_chainId', []), 16)

    def balance(self, address, block='latest'):
        return int(self.request('eth_getBalance', [checksum(address), block]), 16)

    def eth_call(self, to, signature, *values, block='latest'):
        _, _, outputs = parse_signature(signature)
        result = self.request('eth_call', [{'to': checksum(to), 'data': calldata(signature, *values)}, block])
        return decode(outputs, unhex(result))

    def receipt(self, transaction, deadline=120):
        end = time.monotonic() + deadline
        while time.monotonic() < end:
            receipt = self.request('eth_getTransactionReceipt', [transaction])
            if receipt is not None:
                return receipt
            time.sleep(0.5)
        raise ValueError('no receipt for %s within %d seconds' % (transaction, deadline))

    def send(self, secret, chain_id, to, data='0x', value=0, deadline=120):
        if self.chain_id() != chain_id:
            raise ValueError('endpoint does not serve chain %d' % chain_id)
        sender = checksum(address_of(secret))
        target = checksum(to)
        call = {'from': sender, 'to': target, 'data': data, 'value': hex(value)}
        gas = int(self.request('eth_estimateGas', [call]), 16)
        gas += gas // 5
        gas_price = int(self.request('eth_gasPrice', []), 16)
        nonce = int(self.request('eth_getTransactionCount', [sender, 'pending']), 16)
        raw = sign_transaction(secret, chain_id, nonce, gas_price, gas, unhex(target, 20), value, unhex(data))
        transaction = self.request('eth_sendRawTransaction', ['0x' + raw.hex()])
        if unhex(transaction, 32) != keccak(raw):
            raise ValueError('endpoint returned another transaction hash')
        receipt = self.receipt(transaction, deadline)
        if receipt.get('status') != '0x1':
            raise ValueError('transaction %s failed: %s' % (transaction, json.dumps(receipt)))
        return receipt


def _options(arguments, names):
    found, rest = {}, []
    index = 0
    while index < len(arguments):
        if arguments[index] in names:
            if index + 1 == len(arguments):
                raise ValueError(arguments[index] + ' needs a value')
            found[arguments[index]] = arguments[index + 1]
            index += 2
        else:
            rest.append(arguments[index])
            index += 1
    return found, rest


def main(arguments):
    if not arguments:
        raise ValueError('usage: evm.py address|public-key|keccak|checksum|calldata|chain-id|balance|call|send ...')
    mode = arguments[0]
    options, rest = _options(arguments[1:], ('--rpc', '--ca', '--chain', '--key-file', '--value', '--timeout'))
    if mode == 'address' and len(rest) == 1:
        return checksum(address_of(read_key(rest[0])))
    if mode == 'public-key' and len(rest) == 1:
        return compressed_public_key(read_key(rest[0])).hex()
    if mode == 'keccak' and len(rest) == 1:
        return '0x' + keccak(unhex(rest[0])).hex()
    if mode == 'checksum' and len(rest) == 1:
        return checksum(rest[0])
    if mode == 'calldata' and rest:
        return calldata(rest[0], *rest[1:])
    if '--rpc' not in options:
        raise ValueError(mode + ' needs --rpc URL')
    rpc = Rpc(options['--rpc'], options.get('--ca'))
    if mode == 'chain-id' and not rest:
        return str(rpc.chain_id())
    if mode == 'balance' and len(rest) == 1:
        return str(rpc.balance(rest[0]))
    if mode == 'call' and len(rest) >= 2:
        return json.dumps([str(value) if type(value) is int else value for value in rpc.eth_call(rest[0], rest[1], *rest[2:])])
    if mode == 'send' and rest and '--chain' in options and '--key-file' in options:
        data = calldata(rest[1], *rest[2:]) if len(rest) > 1 else '0x'
        return json.dumps(rpc.send(read_key(options['--key-file']), int(options['--chain']), rest[0], data,
                                   int(options.get('--value', '0')), int(options.get('--timeout', '120'))))
    raise ValueError('unrecognised %s invocation' % mode)


if __name__ == '__main__':
    try:
        print(main(sys.argv[1:]))
    except (OSError, ValueError, KeyError) as error:
        raise SystemExit('evm.py: %s' % error) from None
