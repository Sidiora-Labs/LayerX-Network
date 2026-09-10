from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from hashlib import sha256
from itertools import pairwise

MAX_NATIVE_CAPABILITIES = 238
MAX_NATIVE_BALANCE_VIEWS = 32
MAX_NATIVE_CAPABILITY_BYTES = 65_452


class _ImmutableGrant:
    def __post_init__(self) -> None:
        for name, value in vars(self).items():
            if isinstance(value, (bytes, bytearray, memoryview)):
                object.__setattr__(self, name, bytes(value))


@dataclass(frozen=True)
class NativeStorageRead(_ImmutableGrant):
    pass


@dataclass(frozen=True)
class NativeStorageWrite(_ImmutableGrant):
    pass


@dataclass(frozen=True)
class NativeEmitEvent(_ImmutableGrant):
    pass


@dataclass(frozen=True)
class NativeCall(_ImmutableGrant):
    program: bytes


@dataclass(frozen=True)
class NativeTransfer402(_ImmutableGrant):
    asset: bytes
    to: bytes
    maximum_amount: int


@dataclass(frozen=True)
class NativeProgramSpend(_ImmutableGrant):
    owner_program: bytes
    seed: bytes
    source_account: bytes
    asset: bytes
    to: bytes
    maximum_amount: int


@dataclass(frozen=True)
class NativeReceiptRead(_ImmutableGrant):
    receipt_digest: bytes


@dataclass(frozen=True)
class NativeBalanceView(_ImmutableGrant):
    account: bytes
    asset: bytes
    receipt_digest: bytes


@dataclass(frozen=True)
class NativeSharedStorageRead(_ImmutableGrant):
    pass


@dataclass(frozen=True)
class NativeSharedStorageWrite(_ImmutableGrant):
    pass


NativeCapability = NativeStorageRead | NativeStorageWrite | NativeEmitEvent | NativeCall | NativeTransfer402 | NativeProgramSpend | NativeReceiptRead | NativeBalanceView | NativeSharedStorageRead | NativeSharedStorageWrite
_ORDER = (NativeStorageRead, NativeStorageWrite, NativeEmitEvent, NativeCall, NativeTransfer402, NativeProgramSpend, NativeReceiptRead, NativeBalanceView, NativeSharedStorageRead, NativeSharedStorageWrite)
_TAGS = (1, 2, 3, 4, 5, 9, 6, 10, 7, 8)


def _fixed(value: bytes) -> bytes:
    if not isinstance(value, bytes) or len(value) != 32 or value == bytes(32):
        raise ValueError("native capability identifier")
    return value


def _amount(value: int) -> bytes:
    if type(value) is not int or not 0 < value < 1 << 128:
        raise ValueError("native capability amount")
    return value.to_bytes(16, "big")


def derive_native_program_account(owner: bytes, seed: bytes) -> bytes:
    if not isinstance(seed, bytes) or len(seed) > 128:
        raise ValueError("native capability seed")
    return sha256(b"LayerX/programs/program-account/v1\0" + _fixed(owner) + len(seed).to_bytes(4, "big") + seed).digest()


def _key(grant: NativeCapability) -> tuple[int, tuple[bytes, ...]]:
    rank = _ORDER.index(type(grant))
    if isinstance(grant, NativeCall): return rank, (grant.program,)
    if isinstance(grant, NativeTransfer402): return rank, (grant.asset, grant.to)
    if isinstance(grant, NativeProgramSpend): return rank, (grant.owner_program, grant.seed, grant.source_account, grant.asset, grant.to)
    if isinstance(grant, NativeReceiptRead): return rank, (grant.receipt_digest,)
    if isinstance(grant, NativeBalanceView): return rank, (grant.account, grant.asset)
    return rank, ()


def _encode_grant(grant: NativeCapability) -> bytes:
    tag = bytes((_TAGS[_ORDER.index(type(grant))],))
    if isinstance(grant, NativeCall): return tag + _fixed(grant.program)
    if isinstance(grant, NativeTransfer402): return tag + _fixed(grant.asset) + _fixed(grant.to) + _amount(grant.maximum_amount)
    if isinstance(grant, NativeProgramSpend):
        if derive_native_program_account(grant.owner_program, grant.seed) != grant.source_account:
            raise ValueError("native program account binding")
        return tag + grant.owner_program + len(grant.seed).to_bytes(2, "big") + grant.seed + _fixed(grant.source_account) + _fixed(grant.asset) + _fixed(grant.to) + _amount(grant.maximum_amount)
    if isinstance(grant, NativeReceiptRead): return tag + _fixed(grant.receipt_digest)
    if isinstance(grant, NativeBalanceView): return tag + _fixed(grant.account) + _fixed(grant.asset) + _fixed(grant.receipt_digest)
    return tag


def encode_native_capability_set(grants: Sequence[NativeCapability]) -> bytes:
    if len(grants) > MAX_NATIVE_CAPABILITIES or sum(isinstance(grant, NativeBalanceView) for grant in grants) > MAX_NATIVE_BALANCE_VIEWS:
        raise ValueError("native capability count")
    encoded = [(_key(grant), _encode_grant(grant)) for grant in grants]
    encoded.sort(key=lambda entry: entry[0])
    if any(left[0] == right[0] for left, right in pairwise(encoded)):
        raise ValueError("duplicate native capability key")
    result = len(encoded).to_bytes(2, "big") + b"".join(value for _, value in encoded)
    if len(result) > MAX_NATIVE_CAPABILITY_BYTES:
        raise ValueError("native capability encoding bound")
    return result


class _Cursor:
    def __init__(self, encoded: bytes) -> None:
        self.encoded = encoded
        self.offset = 0

    def take(self, length: int) -> bytes:
        if length < 0 or length > len(self.encoded) - self.offset:
            raise ValueError("truncated native capability")
        result = self.encoded[self.offset:self.offset + length]
        self.offset += length
        return result

    def number(self, length: int) -> int:
        return int.from_bytes(self.take(length), "big")


def _decode_grant(reader: _Cursor) -> NativeCapability:
    tag = reader.number(1)
    if tag == 1: return NativeStorageRead()
    if tag == 2: return NativeStorageWrite()
    if tag == 3: return NativeEmitEvent()
    if tag == 4: return NativeCall(reader.take(32))
    if tag == 5: return NativeTransfer402(reader.take(32), reader.take(32), reader.number(16))
    if tag == 6: return NativeReceiptRead(reader.take(32))
    if tag == 7: return NativeSharedStorageRead()
    if tag == 8: return NativeSharedStorageWrite()
    if tag == 9:
        owner = reader.take(32)
        length = reader.number(2)
        if length > 128: raise ValueError("native capability seed")
        return NativeProgramSpend(owner, reader.take(length), reader.take(32), reader.take(32), reader.take(32), reader.number(16))
    if tag == 10: return NativeBalanceView(reader.take(32), reader.take(32), reader.take(32))
    raise ValueError("unknown native capability tag")


def decode_native_capability_set(encoded: bytes) -> tuple[NativeCapability, ...]:
    if not isinstance(encoded, bytes) or not 2 <= len(encoded) <= MAX_NATIVE_CAPABILITY_BYTES:
        raise ValueError("native capability encoding bound")
    reader = _Cursor(encoded)
    count = reader.number(2)
    if count > MAX_NATIVE_CAPABILITIES: raise ValueError("native capability count")
    grants = tuple(_decode_grant(reader) for _ in range(count))
    if reader.offset != len(encoded) or encode_native_capability_set(grants) != encoded:
        raise ValueError("noncanonical native capability set")
    return grants


def narrow_native_capability_set(parent: Sequence[NativeCapability], requested: Sequence[NativeCapability]) -> tuple[NativeCapability, ...]:
    parents = {_key(grant): grant for grant in decode_native_capability_set(encode_native_capability_set(parent))}
    children = decode_native_capability_set(encode_native_capability_set(requested))
    for child in children:
        ancestor = parents.get(_key(child))
        if ancestor is None: raise ValueError("missing native capability authority")
        if isinstance(child, (NativeTransfer402, NativeProgramSpend)) and (
                not isinstance(ancestor, type(child)) or child.maximum_amount > ancestor.maximum_amount):
            raise ValueError("increased native capability amount")
        if isinstance(child, NativeBalanceView) and (
                not isinstance(ancestor, NativeBalanceView) or child.receipt_digest != ancestor.receipt_digest):
            raise ValueError("changed native capability receipt")
    return children
