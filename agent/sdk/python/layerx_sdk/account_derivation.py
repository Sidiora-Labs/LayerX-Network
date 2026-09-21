"""One secret, one account on both sides of the Paxeer X Network.

A Paxeer EVM account is a secp256k1 key and a LayerX identity is an Ed25519
key. Both come from one user secret: a BIP-39 mnemonic (BIP-32 at
m/44'/60'/0'/0/i and SLIP-0010 Ed25519 at m/44'/19544'/i'/0'), or, for a wallet
that never reveals its seed, one fixed EIP-712 signature fed to HKDF-SHA256.

Hashing, encoding and signature recovery (public data only) are standard
library code. Every private-key operation goes through the ``cryptography``
package, imported on first use: install ``layerx-sdk[derivation]``.
"""

from __future__ import annotations

import hashlib
import hmac
import re
import unicodedata
from dataclasses import dataclass, field
from typing import Literal

from .bip39_english import ENGLISH

LAYERX_COIN_TYPE = 19544
ACCOUNT_DERIVATION_ORIGIN = "https://app.paxeer.network"
ACCOUNT_DERIVATION_DOMAIN_NAME = "Paxeer X Network"
ACCOUNT_DERIVATION_DOMAIN_VERSION = "1"
ACCOUNT_DERIVATION_PURPOSE = "Derive your LayerX account key"
ACCOUNT_DERIVATION_VERSION = 1
ACCOUNT_DERIVATION_HKDF_SALT = b"paxeer-x-network/layerx-account-key/v1"
ACCOUNT_DERIVATION_HKDF_INFO_PREFIX = b"LX:ACCOUNT-KEY:v1"
ADDR_PRECOMPILE_ADDRESS = "0x0000000000000000000000000000000000001004"

_HARDENED = 0x80000000
_MAX_CHAIN_ID = (1 << 53) - 1
_ORIGIN = re.compile(r"https?://[a-z0-9.-]{1,253}(:[0-9]{1,5})?")
_ADDRESS = re.compile(r"0x[0-9a-fA-F]{40}")
_DOMAIN_TYPE = b"EIP712Domain(string name,string version,uint256 chainId)"
_MESSAGE_TYPE = (
    b"LayerXKeyDerivation(string purpose,string warning,address address,uint32 index,uint32 version)"
)
_SELECTOR_BIND_LAYERX = "dd9aa628"
_SELECTOR_LAYERX_BIND_NONCE = "cedd9ba2"
_SELECTOR_GET_UNIFIED_ACCOUNT = "357feed6"
_BIND_DOMAIN = b"LX:PAXEER-BIND:v1"
_WORD_INDEX = {word: position for position, word in enumerate(ENGLISH)}

# secp256k1 domain parameters.
_P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
_N = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
_G = (
    0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798,
    0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8,
)


class AccountDerivationError(ValueError):
    """A refused derivation or binding; ``code`` is stable across the SDKs."""

    def __init__(self, code: str, detail: str | None = None) -> None:
        super().__init__(code if detail is None else f"{code}: {detail}")
        self.code = code


@dataclass(frozen=True)
class DerivedAccount:
    """The pair one secret yields. ``evm_private_key`` is None when a wallet holds it."""

    index: int
    evm_address: str
    layerx_public_key: str
    did: str
    layerx_seed: bytes = field(repr=False)
    evm_private_key: bytes | None = field(repr=False)


def _cryptography():
    try:
        from cryptography.hazmat.primitives.asymmetric import ec, ed25519
    except ImportError as error:  # pragma: no cover - exercised only without the extra
        raise ImportError(
            "account derivation needs the 'cryptography' package: pip install 'layerx-sdk[derivation]'"
        ) from error
    return ec, ed25519


# Keccak-256 (the pre-standard padding Ethereum uses), over public data only.
_ROUND_CONSTANTS = (
    0x0000000000000001, 0x0000000000008082, 0x800000000000808A, 0x8000000080008000,
    0x000000000000808B, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008A, 0x0000000000000088, 0x0000000080008009, 0x000000008000000A,
    0x000000008000808B, 0x800000000000008B, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800A, 0x800000008000000A,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
)
_ROTATIONS = (
    (0, 36, 3, 41, 18),
    (1, 44, 10, 45, 2),
    (62, 6, 43, 15, 61),
    (28, 55, 25, 21, 56),
    (27, 20, 39, 8, 14),
)
_MASK = (1 << 64) - 1


def _rotate(value: int, shift: int) -> int:
    return ((value << shift) | (value >> (64 - shift))) & _MASK if shift else value


def _keccak_f(state: list[list[int]]) -> None:
    for constant in _ROUND_CONSTANTS:
        parity = [state[x][0] ^ state[x][1] ^ state[x][2] ^ state[x][3] ^ state[x][4] for x in range(5)]
        for x in range(5):
            mix = parity[(x - 1) % 5] ^ _rotate(parity[(x + 1) % 5], 1)
            for y in range(5):
                state[x][y] ^= mix
        moved = [[0] * 5 for _ in range(5)]
        for x in range(5):
            for y in range(5):
                moved[y][(2 * x + 3 * y) % 5] = _rotate(state[x][y], _ROTATIONS[x][y])
        for x in range(5):
            for y in range(5):
                state[x][y] = moved[x][y] ^ (~moved[(x + 1) % 5][y] & moved[(x + 2) % 5][y])
        state[0][0] ^= constant


def keccak256(data: bytes) -> bytes:
    rate = 136
    padded = bytearray(data)
    padded.append(0x01)
    padded.extend(b"\x00" * (-len(padded) % rate))
    padded[-1] |= 0x80
    state = [[0] * 5 for _ in range(5)]
    for block in range(0, len(padded), rate):
        for lane in range(rate // 8):
            x, y = lane % 5, lane // 5
            state[x][y] ^= int.from_bytes(padded[block + lane * 8 : block + lane * 8 + 8], "little")
        _keccak_f(state)
    return b"".join(state[lane % 5][lane // 5].to_bytes(8, "little") for lane in range(4))


# secp256k1 affine arithmetic, used only on public values (signature recovery).
_Point = tuple[int, int] | None


def _add(left: _Point, right: _Point) -> _Point:
    if left is None:
        return right
    if right is None:
        return left
    if left[0] == right[0]:
        if (left[1] + right[1]) % _P == 0:
            return None
        slope = 3 * left[0] * left[0] * pow(2 * left[1], -1, _P) % _P
    else:
        slope = (right[1] - left[1]) * pow(right[0] - left[0], -1, _P) % _P
    x = (slope * slope - left[0] - right[0]) % _P
    return x, (slope * (left[0] - x) - left[1]) % _P


def _multiply(point: _Point, scalar: int) -> _Point:
    result: _Point = None
    while scalar:
        if scalar & 1:
            result = _add(result, point)
        point = _add(point, point)
        scalar >>= 1
    return result


def _recover(digest: bytes, r: int, s: int, parity: int) -> bytes:
    y_squared = (pow(r, 3, _P) + 7) % _P
    y = pow(y_squared, (_P + 1) // 4, _P)
    if y * y % _P != y_squared:
        raise AccountDerivationError("invalid_wallet_signature")
    if y & 1 != parity:
        y = _P - y
    inverse = pow(r, -1, _N)
    z = int.from_bytes(digest, "big")
    public = _add(_multiply((r, y), s * inverse % _N), _multiply(_G, -z * inverse % _N))
    if public is None:
        raise AccountDerivationError("invalid_wallet_signature")
    return public[0].to_bytes(32, "big") + public[1].to_bytes(32, "big")


def _hex_bytes(text: str, code: str) -> bytes:
    body = text[2:] if text[:2] in ("0x", "0X") else text
    try:
        return bytes.fromhex(body)
    except ValueError as error:
        raise AccountDerivationError(code) from error


def _address_bytes(address: str) -> bytes:
    if not isinstance(address, str) or _ADDRESS.fullmatch(address) is None:
        raise AccountDerivationError("invalid_address")
    return bytes.fromhex(address[2:])


def _check_index(index: int) -> int:
    if not isinstance(index, int) or isinstance(index, bool) or not 0 <= index < _HARDENED:
        raise AccountDerivationError("index_out_of_range")
    return index


def _check_chain_id(chain_id: int) -> int:
    if not isinstance(chain_id, int) or isinstance(chain_id, bool) or not 0 <= chain_id <= _MAX_CHAIN_ID:
        raise AccountDerivationError("invalid_chain_id")
    return chain_id


def checksum_address(address: bytes | str) -> str:
    """Renders a 20-byte address with its EIP-55 checksum."""
    lower = (_address_bytes(address) if isinstance(address, str) else address).hex()
    digest = keccak256(lower.encode()).hex()
    return "0x" + "".join(
        character.upper() if int(digest[position], 16) >= 8 else character
        for position, character in enumerate(lower)
    )


def _evm_address(public_key_xy: bytes) -> str:
    return checksum_address(keccak256(public_key_xy)[12:])


def _account(index: int, evm_address: str, evm_private_key: bytes | None, layerx_seed: bytes) -> DerivedAccount:
    _, ed25519 = _cryptography()
    from cryptography.hazmat.primitives import serialization

    public = ed25519.Ed25519PrivateKey.from_private_bytes(layerx_seed).public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    return DerivedAccount(
        index=index,
        evm_address=evm_address,
        layerx_public_key=public.hex(),
        did=f"did:layerx:{public.hex()}",
        layerx_seed=layerx_seed,
        evm_private_key=evm_private_key,
    )


def _check_seed(seed: bytes) -> None:
    if not 16 <= len(seed) <= 64:
        raise AccountDerivationError("invalid_seed")


def slip10_ed25519(seed: bytes, path: tuple[int, ...] | list[int]) -> bytes:
    """SLIP-0010 Ed25519 private key at a hardened-only path; components carry no hardening bit."""
    _check_seed(seed)
    node = hmac.new(b"ed25519 seed", seed, hashlib.sha512).digest()
    for component in path:
        index = (_check_index(component) + _HARDENED).to_bytes(4, "big")
        node = hmac.new(node[32:], b"\x00" + node[:32] + index, hashlib.sha512).digest()
    return node[:32]


def _secp256k1_public(private_key: int) -> tuple[bytes, bytes]:
    ec, _ = _cryptography()
    numbers = ec.derive_private_key(private_key, ec.SECP256K1()).public_key().public_numbers()
    x, y = numbers.x.to_bytes(32, "big"), numbers.y.to_bytes(32, "big")
    return bytes([2 + (numbers.y & 1)]) + x, x + y


def _bip32_secp256k1(seed: bytes, path: tuple[int, ...]) -> int:
    node = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
    key, chain = int.from_bytes(node[:32], "big"), node[32:]
    if not 0 < key < _N:
        raise AccountDerivationError("invalid_seed")
    for component in path:
        index = component.to_bytes(4, "big")
        if component & _HARDENED:
            data = b"\x00" + key.to_bytes(32, "big") + index
        else:
            data = _secp256k1_public(key)[0] + index
        node = hmac.new(chain, data, hashlib.sha512).digest()
        tweak = int.from_bytes(node[:32], "big")
        child = (tweak + key) % _N
        if tweak >= _N or child == 0:
            raise AccountDerivationError("invalid_seed", "unusable BIP-32 child; use the next index")
        key, chain = child, node[32:]
    return key


def derive_from_seed(seed: bytes, index: int = 0) -> DerivedAccount:
    """Derives both keys of account ``index`` from a BIP-32 seed."""
    _check_index(index)
    _check_seed(seed)
    evm = _bip32_secp256k1(seed, (44 | _HARDENED, 60 | _HARDENED, _HARDENED, 0, index))
    return _account(
        index,
        _evm_address(_secp256k1_public(evm)[1]),
        evm.to_bytes(32, "big"),
        slip10_ed25519(seed, (44, LAYERX_COIN_TYPE, index, 0)),
    )


def _mnemonic_words(mnemonic: str) -> list[str]:
    words = unicodedata.normalize("NFKD", mnemonic).split()
    if len(words) not in (12, 15, 18, 21, 24) or any(word not in _WORD_INDEX for word in words):
        raise AccountDerivationError("invalid_mnemonic")
    bits = "".join(format(_WORD_INDEX[word], "011b") for word in words)
    entropy_bits = len(bits) * 32 // 33
    entropy = int(bits[:entropy_bits], 2).to_bytes(entropy_bits // 8, "big")
    checksum = format(hashlib.sha256(entropy).digest()[0], "08b")[: len(bits) - entropy_bits]
    if bits[entropy_bits:] != checksum:
        raise AccountDerivationError("invalid_mnemonic")
    return words


def derive_from_mnemonic(mnemonic: str, passphrase: str = "", index: int = 0) -> DerivedAccount:
    """Derives both keys of account ``index`` from an English BIP-39 mnemonic."""
    phrase = " ".join(_mnemonic_words(mnemonic))
    salt = "mnemonic" + unicodedata.normalize("NFKD", passphrase)
    seed = hashlib.pbkdf2_hmac("sha512", phrase.encode(), salt.encode(), 2048, 64)
    return derive_from_seed(seed, index)


def key_derivation_typed_data(
    chain_id: int, address: str, index: int = 0, origin: str = ACCOUNT_DERIVATION_ORIGIN
) -> dict[str, object]:
    """The one EIP-712 document an external wallet signs to derive a LayerX key.

    A different origin is a different message and therefore a different key.
    """
    if not isinstance(origin, str) or _ORIGIN.fullmatch(origin) is None:
        raise AccountDerivationError("invalid_origin")
    return {
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"},
            ],
            "LayerXKeyDerivation": [
                {"name": "purpose", "type": "string"},
                {"name": "warning", "type": "string"},
                {"name": "address", "type": "address"},
                {"name": "index", "type": "uint32"},
                {"name": "version", "type": "uint32"},
            ],
        },
        "primaryType": "LayerXKeyDerivation",
        "domain": {
            "name": ACCOUNT_DERIVATION_DOMAIN_NAME,
            "version": ACCOUNT_DERIVATION_DOMAIN_VERSION,
            "chainId": _check_chain_id(chain_id),
        },
        "message": {
            "purpose": ACCOUNT_DERIVATION_PURPOSE,
            "warning": f"Only sign this on {origin}. Anyone holding this signature controls your LayerX account.",
            "address": "0x" + _address_bytes(address).hex(),
            "index": _check_index(index),
            "version": ACCOUNT_DERIVATION_VERSION,
        },
    }


def key_derivation_hash(
    chain_id: int, address: str, index: int = 0, origin: str = ACCOUNT_DERIVATION_ORIGIN
) -> bytes:
    """The EIP-712 digest of :func:`key_derivation_typed_data`."""
    document = key_derivation_typed_data(chain_id, address, index, origin)
    message = document["message"]
    assert isinstance(message, dict)
    domain = keccak256(
        keccak256(_DOMAIN_TYPE)
        + keccak256(ACCOUNT_DERIVATION_DOMAIN_NAME.encode())
        + keccak256(ACCOUNT_DERIVATION_DOMAIN_VERSION.encode())
        + chain_id.to_bytes(32, "big")
    )
    struct = keccak256(
        keccak256(_MESSAGE_TYPE)
        + keccak256(ACCOUNT_DERIVATION_PURPOSE.encode())
        + keccak256(str(message["warning"]).encode())
        + _address_bytes(address).rjust(32, b"\x00")
        + index.to_bytes(32, "big")
        + ACCOUNT_DERIVATION_VERSION.to_bytes(32, "big")
    )
    return keccak256(b"\x19\x01" + domain + struct)


def normalize_wallet_signature(signature: bytes | str) -> bytes:
    """Brings a 65-byte r || s || v signature to low s and v of 27 or 28."""
    raw = _hex_bytes(signature, "invalid_wallet_signature") if isinstance(signature, str) else bytes(signature)
    if len(raw) != 65 or raw[64] not in (0, 1, 27, 28):
        raise AccountDerivationError("invalid_wallet_signature")
    parity = raw[64] & 1 if raw[64] < 27 else raw[64] - 27
    r, s = int.from_bytes(raw[:32], "big"), int.from_bytes(raw[32:64], "big")
    if not 0 < r < _N or not 0 < s < _N:
        raise AccountDerivationError("invalid_wallet_signature")
    if s > _N // 2:
        s, parity = _N - s, parity ^ 1
    return raw[:32] + s.to_bytes(32, "big") + bytes([27 + parity])


def _hkdf_sha256(key_material: bytes, salt: bytes, info: bytes) -> bytes:
    extracted = hmac.new(salt, key_material, hashlib.sha256).digest()
    return hmac.new(extracted, info + b"\x01", hashlib.sha256).digest()


def derive_from_wallet_signature(
    chain_id: int,
    address: str,
    signature: bytes | str,
    index: int = 0,
    origin: str = ACCOUNT_DERIVATION_ORIGIN,
) -> DerivedAccount:
    """Derives the LayerX key a wallet signature stands for.

    The signature is normalised and must recover to ``address`` before it is
    fed to HKDF-SHA256.
    """
    canonical = normalize_wallet_signature(signature)
    digest = key_derivation_hash(chain_id, address, index, origin)
    signer = _evm_address(
        _recover(
            digest,
            int.from_bytes(canonical[:32], "big"),
            int.from_bytes(canonical[32:64], "big"),
            canonical[64] - 27,
        )
    )
    if signer.lower() != address.lower():
        raise AccountDerivationError("wallet_signer_mismatch")
    info = (
        ACCOUNT_DERIVATION_HKDF_INFO_PREFIX
        + chain_id.to_bytes(32, "big")
        + _address_bytes(address)
        + index.to_bytes(4, "big")
    )
    return _account(index, signer, None, _hkdf_sha256(canonical, ACCOUNT_DERIVATION_HKDF_SALT, info))


@dataclass(frozen=True)
class LayerXBindState:
    """What the ``addr`` precompile holds for an EVM address."""

    bound_did_public_key: str | None
    nonce: int


@dataclass(frozen=True)
class BindCall:
    to: str
    data: str
    did_public_key: str
    signature: str
    nonce: int


@dataclass(frozen=True)
class BindPlan:
    action: Literal["already_bound", "bind"]
    call: BindCall | None = None


def _address_call(selector: str, address: str) -> str:
    return "0x" + selector + "00" * 12 + _address_bytes(address).hex()


def bind_nonce_call(address: str) -> str:
    """``eth_call`` data reading ``layerXBindNonce(address)``."""
    return _address_call(_SELECTOR_LAYERX_BIND_NONCE, address)


def bound_did_call(address: str) -> str:
    """``eth_call`` data reading the bound identity through ``getUnifiedAccount(address)``."""
    return _address_call(_SELECTOR_GET_UNIFIED_ACCOUNT, address)


def decode_bind_nonce(answer: str) -> int:
    raw = _hex_bytes(answer, "malformed_precompile_answer")
    if len(raw) != 32 or any(raw[:24]):
        raise AccountDerivationError("malformed_precompile_answer")
    return int.from_bytes(raw, "big")


def decode_bound_did(answer: str) -> str | None:
    raw = _hex_bytes(answer, "malformed_precompile_answer")
    if len(raw) < 128 or len(raw) % 32:
        raise AccountDerivationError("malformed_precompile_answer")
    key = raw[64:96]
    return key.hex() if any(key) else None


def plan_layerx_bind(derived: DerivedAccount, chain_id: int, state: LayerXBindState) -> BindPlan:
    """Plans first-use binding.

    Idempotent when the pair is already bound; refuses with
    ``bound_to_different_did``, never producing a call, when the address
    belongs to another identity.
    """
    if state.bound_did_public_key is not None:
        if state.bound_did_public_key.lower() == derived.layerx_public_key:
            return BindPlan("already_bound")
        raise AccountDerivationError(
            "bound_to_different_did", f"did:layerx:{state.bound_did_public_key.lower()}"
        )
    if not 0 <= state.nonce < 1 << 64:
        raise AccountDerivationError("malformed_precompile_answer")
    _, ed25519 = _cryptography()
    message = (
        _BIND_DOMAIN
        + _check_chain_id(chain_id).to_bytes(32, "big")
        + _address_bytes(derived.evm_address)
        + state.nonce.to_bytes(8, "big")
    )
    signature = ed25519.Ed25519PrivateKey.from_private_bytes(derived.layerx_seed).sign(message).hex()
    offset = (64).to_bytes(32, "big").hex()
    return BindPlan(
        "bind",
        BindCall(
            to=ADDR_PRECOMPILE_ADDRESS,
            data="0x" + _SELECTOR_BIND_LAYERX + derived.layerx_public_key + offset + offset + signature,
            did_public_key=derived.layerx_public_key,
            signature=signature,
            nonce=state.nonce,
        ),
    )


def bind_transaction_request(derived: DerivedAccount, call: BindCall) -> dict[str, str]:
    """The transaction fields a signer or wallet sends for a bind call."""
    return {"from": derived.evm_address, "to": call.to, "data": call.data, "value": "0x0"}
