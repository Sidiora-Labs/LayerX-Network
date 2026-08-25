from __future__ import annotations

from hashlib import sha512

_ED25519_P = 2**255 - 19
_ED25519_Q = 2**252 + 27742317777372353535851937790883648493
_ED25519_D = -121665 * pow(121666, _ED25519_P - 2, _ED25519_P) % _ED25519_P
_ED25519_SQRT_MINUS_ONE = pow(2, (_ED25519_P - 1) // 4, _ED25519_P)

_SECP256K1_P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
_SECP256K1_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
_SECP256K1_GX = 0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798
_SECP256K1_GY = 0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8
_U64_MASK = (1 << 64) - 1
_KECCAK_ROTATIONS = (
    0, 1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15,
    21, 8, 18, 2, 61, 56, 14,
)
_KECCAK_ROUND_CONSTANTS = (
    0x0000000000000001, 0x0000000000008082, 0x800000000000808A,
    0x8000000080008000, 0x000000000000808B, 0x0000000080000001,
    0x8000000080008081, 0x8000000000008009, 0x000000000000008A,
    0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
    0x000000008000808B, 0x800000000000008B, 0x8000000000008089,
    0x8000000000008003, 0x8000000000008002, 0x8000000000000080,
    0x000000000000800A, 0x800000008000000A, 0x8000000080008081,
    0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
)

_ExtendedPoint = tuple[int, int, int, int]
_AffinePoint = tuple[int, int]


def _ed25519_recover_x(y: int, sign: int) -> int | None:
    if y >= _ED25519_P:
        return None
    square = (y * y - 1) * pow(_ED25519_D * y * y + 1, _ED25519_P - 2, _ED25519_P) % _ED25519_P
    if square == 0:
        return None if sign else 0
    root = pow(square, (_ED25519_P + 3) // 8, _ED25519_P)
    if (root * root - square) % _ED25519_P != 0:
        root = root * _ED25519_SQRT_MINUS_ONE % _ED25519_P
    if (root * root - square) % _ED25519_P != 0:
        return None
    if root & 1 != sign:
        root = _ED25519_P - root
    return root


def _ed25519_add(left: _ExtendedPoint, right: _ExtendedPoint) -> _ExtendedPoint:
    a = (left[1] - left[0]) * (right[1] - right[0]) % _ED25519_P
    b = (left[1] + left[0]) * (right[1] + right[0]) % _ED25519_P
    c = 2 * left[3] * right[3] * _ED25519_D % _ED25519_P
    d = 2 * left[2] * right[2] % _ED25519_P
    e, f, g, h = b - a, d - c, d + c, b + a
    return (e * f % _ED25519_P, g * h % _ED25519_P, f * g % _ED25519_P, e * h % _ED25519_P)


def _ed25519_multiply(scalar: int, point: _ExtendedPoint) -> _ExtendedPoint:
    result: _ExtendedPoint = (0, 1, 1, 0)
    addend = point
    remaining = scalar
    while remaining > 0:
        if remaining & 1:
            result = _ed25519_add(result, addend)
        addend = _ed25519_add(addend, addend)
        remaining >>= 1
    return result


def _ed25519_equal(left: _ExtendedPoint, right: _ExtendedPoint) -> bool:
    if (left[0] * right[2] - right[0] * left[2]) % _ED25519_P != 0:
        return False
    return (left[1] * right[2] - right[1] * left[2]) % _ED25519_P == 0


def _ed25519_base() -> _ExtendedPoint:
    y = 4 * pow(5, _ED25519_P - 2, _ED25519_P) % _ED25519_P
    x = _ed25519_recover_x(y, 0)
    if x is None:
        raise ValueError("ed25519-base-point")
    return (x, y, 1, x * y % _ED25519_P)


_ED25519_G = _ed25519_base()


def _ed25519_decompress(encoded: bytes) -> _ExtendedPoint | None:
    if len(encoded) != 32:
        return None
    value = int.from_bytes(encoded, "little")
    sign = value >> 255
    y = value & ((1 << 255) - 1)
    x = _ed25519_recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % _ED25519_P)


def verify_ed25519(public_key: bytes, signature: bytes, message: bytes) -> bool:
    if len(public_key) != 32 or len(signature) != 64:
        return False
    key_point = _ed25519_decompress(public_key)
    if key_point is None:
        return False
    commitment = _ed25519_decompress(signature[:32])
    if commitment is None:
        return False
    scalar = int.from_bytes(signature[32:], "little")
    if scalar >= _ED25519_Q:
        return False
    challenge = int.from_bytes(
        sha512(signature[:32] + public_key + message).digest(), "little"
    ) % _ED25519_Q
    return _ed25519_equal(
        _ed25519_multiply(scalar, _ED25519_G),
        _ed25519_add(commitment, _ed25519_multiply(challenge, key_point)),
    )


def _secp256k1_add(left: _AffinePoint | None, right: _AffinePoint | None) -> _AffinePoint | None:
    if left is None:
        return right
    if right is None:
        return left
    if left[0] == right[0] and (left[1] + right[1]) % _SECP256K1_P == 0:
        return None
    if left == right:
        numerator = 3 * left[0] * left[0] % _SECP256K1_P
        denominator = 2 * left[1] % _SECP256K1_P
    else:
        numerator = (right[1] - left[1]) % _SECP256K1_P
        denominator = (right[0] - left[0]) % _SECP256K1_P
    slope = numerator * pow(denominator, _SECP256K1_P - 2, _SECP256K1_P) % _SECP256K1_P
    x = (slope * slope - left[0] - right[0]) % _SECP256K1_P
    y = (slope * (left[0] - x) - left[1]) % _SECP256K1_P
    return (x, y)


def _secp256k1_multiply(scalar: int, point: _AffinePoint) -> _AffinePoint | None:
    result: _AffinePoint | None = None
    addend: _AffinePoint | None = point
    remaining = scalar
    while remaining > 0 and addend is not None:
        if remaining & 1:
            result = _secp256k1_add(result, addend)
        addend = _secp256k1_add(addend, addend)
        remaining >>= 1
    return result


def _secp256k1_decompress(encoded: bytes) -> _AffinePoint | None:
    if len(encoded) != 33 or encoded[0] not in (2, 3):
        return None
    x = int.from_bytes(encoded[1:], "big")
    if x >= _SECP256K1_P:
        return None
    square = (pow(x, 3, _SECP256K1_P) + 7) % _SECP256K1_P
    y = pow(square, (_SECP256K1_P + 1) // 4, _SECP256K1_P)
    if y * y % _SECP256K1_P != square:
        return None
    if y & 1 != encoded[0] & 1:
        y = _SECP256K1_P - y
    return (x, y)


def verify_secp256k1(public_key: bytes, signature: bytes, digest: bytes) -> bool:
    if len(signature) != 64 or len(digest) != 32:
        return False
    key_point = _secp256k1_decompress(public_key)
    if key_point is None:
        return False
    r = int.from_bytes(signature[:32], "big")
    s = int.from_bytes(signature[32:], "big")
    if not 1 <= r < _SECP256K1_N or not 1 <= s < _SECP256K1_N:
        return False
    inverse = pow(s, _SECP256K1_N - 2, _SECP256K1_N)
    message = int.from_bytes(digest, "big")
    combined = _secp256k1_add(
        _secp256k1_multiply(message * inverse % _SECP256K1_N, (_SECP256K1_GX, _SECP256K1_GY)),
        _secp256k1_multiply(r * inverse % _SECP256K1_N, key_point),
    )
    if combined is None:
        return False
    return combined[0] % _SECP256K1_N == r


def _rotate_u64(value: int, count: int) -> int:
    if count == 0:
        return value & _U64_MASK
    return ((value << count) | (value >> (64 - count))) & _U64_MASK


def _keccak_permute(state: list[int]) -> None:
    for constant in _KECCAK_ROUND_CONSTANTS:
        columns = [
            state[index] ^ state[index + 5] ^ state[index + 10]
            ^ state[index + 15] ^ state[index + 20]
            for index in range(5)
        ]
        for y in range(5):
            for x in range(5):
                state[x + 5 * y] ^= columns[(x - 1) % 5] ^ _rotate_u64(
                    columns[(x + 1) % 5], 1
                )
        rotated = [0] * 25
        for y in range(5):
            for x in range(5):
                rotated[y + 5 * ((2 * x + 3 * y) % 5)] = _rotate_u64(
                    state[x + 5 * y], _KECCAK_ROTATIONS[x + 5 * y]
                )
        for y in range(5):
            offset = 5 * y
            for x in range(5):
                state[offset + x] = rotated[offset + x] ^ (
                    (~rotated[offset + (x + 1) % 5])
                    & rotated[offset + (x + 2) % 5]
                )
                state[offset + x] &= _U64_MASK
        state[0] ^= constant


def _keccak256(message: bytes) -> bytes:
    rate = 136
    padded = bytearray(message)
    padded.append(0x01)
    padded.extend(bytes((-len(padded)) % rate))
    padded[-1] ^= 0x80
    state = [0] * 25
    for offset in range(0, len(padded), rate):
        block = padded[offset:offset + rate]
        for lane in range(rate // 8):
            state[lane] ^= int.from_bytes(block[lane * 8:(lane + 1) * 8], "little")
        _keccak_permute(state)
    return b"".join(lane.to_bytes(8, "little") for lane in state[:rate // 8])[:32]


def verify_recoverable_secp256k1(
    public_key: bytes,
    signature: bytes,
    signature_v: int,
    signer: bytes,
    digest: bytes,
) -> bool:
    if (
        len(public_key) != 33
        or len(signature) != 64
        or len(signer) != 20
        or len(digest) != 32
        or signature_v not in (27, 28)
    ):
        return False
    key_point = _secp256k1_decompress(public_key)
    if key_point is None:
        return False
    r = int.from_bytes(signature[:32], "big")
    s = int.from_bytes(signature[32:], "big")
    if not 1 <= r < _SECP256K1_N or not 1 <= s <= _SECP256K1_N // 2:
        return False
    recovered_r = _secp256k1_decompress(
        bytes((2 + signature_v - 27,)) + r.to_bytes(32, "big")
    )
    if recovered_r is None or _secp256k1_multiply(_SECP256K1_N, recovered_r) is not None:
        return False
    inverse_r = pow(r, _SECP256K1_N - 2, _SECP256K1_N)
    message = int.from_bytes(digest, "big")
    recovered_key = _secp256k1_add(
        _secp256k1_multiply(
            s * inverse_r % _SECP256K1_N,
            recovered_r,
        ),
        _secp256k1_multiply(
            (-message) * inverse_r % _SECP256K1_N,
            (_SECP256K1_GX, _SECP256K1_GY),
        ),
    )
    if recovered_key is None or recovered_key != key_point:
        return False
    if not verify_secp256k1(public_key, signature, digest):
        return False
    uncompressed = recovered_key[0].to_bytes(32, "big") + recovered_key[1].to_bytes(32, "big")
    return _keccak256(uncompressed)[12:] == signer


class LayerXSignatureVerifier:
    __slots__ = ()

    def verify_ed25519(self, public_key: bytes, signature: bytes, digest: bytes) -> bool:
        return verify_ed25519(public_key, signature, digest)

    def verify_recoverable_secp256k1(
        self,
        public_key: bytes,
        signature: bytes,
        signature_v: int,
        signer: bytes,
        digest: bytes,
    ) -> bool:
        return verify_recoverable_secp256k1(
            public_key, signature, signature_v, signer, digest
        )
