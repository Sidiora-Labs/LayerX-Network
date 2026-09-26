"""Builders for xweb api requests.

The api payload a contract passes to the xweb precompile with kind 3, and the
credential envelopes sealed to the attestors ``getAttestors`` returns. The
payload bytes are exactly what modules/xweb/types decodes (api.go), and an
envelope is ECIES over secp256k1 with HKDF-SHA256 and AES-256-GCM
(envelope.go). modules/xweb/types/testdata/api-vectors.json and
envelope-vectors.json pin both.
"""

from __future__ import annotations

import re
import secrets
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Literal

from .account_derivation import keccak256

XWEB_PRECOMPILE = "0x0000000000000000000000000000000000001019"
XWEB_KIND_API = 3
XWEB_API_VERSION = 1
XWEB_METHOD_GET = 1
XWEB_METHOD_POST = 2
XWEB_LEVEL_MAJORITY = 0
XWEB_LEVEL_SINGLE = 1
XWEB_ENVELOPE_INFO = b"PAXEERX_WEB_API_ENVELOPE_V1"
XWEB_DEFAULT_MAX_PAYLOAD_BYTES = 8192

XWEB_MAX_URL_BYTES = 2048
XWEB_MAX_HEADERS = 16
XWEB_MAX_HEADER_NAME_BYTES = 64
XWEB_MAX_HEADER_VALUE_BYTES = 1024
XWEB_MAX_BODY_BYTES = 4096
XWEB_MAX_POINTERS = 16
XWEB_MAX_POINTER_BYTES = 256
XWEB_MAX_ENVELOPES = 64
XWEB_MAX_CREDENTIAL_HEADERS = 8
XWEB_MAX_CREDENTIAL_BYTES = 1024

XWEB_ENVELOPE_KEY_LENGTH = 33
XWEB_ENVELOPE_NONCE_LENGTH = 12
XWEB_ENVELOPE_TAG_LENGTH = 16
XWEB_ENVELOPE_OVERHEAD = 20 + XWEB_ENVELOPE_KEY_LENGTH + XWEB_ENVELOPE_NONCE_LENGTH + XWEB_ENVELOPE_TAG_LENGTH
XWEB_MAX_ENVELOPE_BYTES = XWEB_ENVELOPE_OVERHEAD + XWEB_MAX_CREDENTIAL_BYTES

XWebApiMethod = Literal["GET", "POST"]

_ZERO_ADDRESS = "0x" + "00" * 20
_ADDRESS = re.compile(r"0x[0-9a-fA-F]{40}")
_HEX = re.compile(r"0x(?:[0-9a-fA-F]{2})*")
_TOKEN = frozenset(b"!#$%&'*+-.^_`|~0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")
_AUTHORITY = frozenset(b"abcdefghijklmnopqrstuvwxyz0123456789.-:[]")
_RESTRICTED_HEADERS = frozenset(
    {"host", "content-length", "transfer-encoding", "connection", "keep-alive", "te", "trailer", "upgrade"}
)
_SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
_WORD = 32


class XWebApiError(ValueError):
    """A refused api payload, envelope or attestor set; ``code`` matches the TypeScript client."""

    def __init__(self, code: str, detail: str) -> None:
        super().__init__(f"{code}: {detail}")
        self.code = code


@dataclass(frozen=True)
class XWebApiHeader:
    name: str
    value: str


@dataclass(frozen=True)
class XWebApiPayload:
    """A decoded api payload. Addresses are lower-case 0x hex; envelopes are wire bytes."""

    method: XWebApiMethod
    level: int
    attestor: str
    url: str
    headers: tuple[XWebApiHeader, ...]
    body: bytes
    pointers: tuple[str, ...]
    envelopes: tuple[bytes, ...]


@dataclass(frozen=True)
class XWebAttestor:
    """One entry of getAttestors(); ``public_key`` is empty for an attestor that takes no envelopes."""

    signer: str
    payout: str
    public_key: bytes


@dataclass(frozen=True)
class XWebAttestorSet:
    attestors: tuple[XWebAttestor, ...]
    required: int


@dataclass(frozen=True)
class XWebEnvelopeRandomness:
    """The ephemeral private key and nonce of one envelope; supplied only to reproduce a vector."""

    ephemeral_private_key: bytes
    nonce: bytes


@dataclass(frozen=True)
class XWebApiCall:
    """The call a developer wants made; ``single`` names the one attestor that makes it."""

    method: XWebApiMethod
    url: str
    headers: tuple[XWebApiHeader, ...] = ()
    body: bytes | str = b""
    pointers: tuple[str, ...] = ()
    single: str | None = None


@dataclass(frozen=True)
class XWebSealedEnvelope:
    attestor: str
    envelope: bytes


@dataclass(frozen=True)
class XWebApiRequest:
    payload: bytes
    payload_hash: str
    origin: str
    level: int
    attestor: str
    envelopes: tuple[XWebSealedEnvelope, ...]


@dataclass(frozen=True)
class XWebPrecompileCall:
    """A transaction to the precompile; ``value`` is the attached PAX in wei."""

    to: str
    data: str
    value: int


def _api_error(detail: str) -> XWebApiError:
    return XWebApiError("invalid_api", detail)


def _crypto():
    try:
        from cryptography.hazmat.primitives import hashes, serialization
        from cryptography.hazmat.primitives.asymmetric import ec
        from cryptography.hazmat.primitives.ciphers.aead import AESGCM
        from cryptography.hazmat.primitives.kdf.hkdf import HKDF
    except ImportError as error:  # pragma: no cover - exercised only without the extra
        raise ImportError(
            "xweb credential envelopes need the 'cryptography' package: pip install 'layerx-sdk[derivation]'"
        ) from error
    return ec, hashes, serialization, AESGCM, HKDF


def _hex(data: bytes) -> str:
    return "0x" + data.hex()


def _unhex(value: str, label: str, code: str = "invalid_api") -> bytes:
    if not isinstance(value, str) or _HEX.fullmatch(value) is None:
        raise XWebApiError(code, f"{label} is not 0x-prefixed hex")
    return bytes.fromhex(value[2:])


def _address(value: str, label: str) -> bytes:
    if not isinstance(value, str) or _ADDRESS.fullmatch(value) is None:
        raise _api_error(f"{label} {value!r} is not a 20-byte address")
    return bytes.fromhex(value[2:])


def _utf8(text: str) -> bytes:
    return text.encode("utf-8", "surrogatepass")


def xweb_method_code(method: str) -> int:
    if method == "GET":
        return XWEB_METHOD_GET
    if method == "POST":
        return XWEB_METHOD_POST
    raise _api_error(f"method {method}, want GET or POST")


def _method_name(code: int) -> XWebApiMethod:
    if code == XWEB_METHOD_GET:
        return "GET"
    if code == XWEB_METHOD_POST:
        return "POST"
    raise _api_error(f"method {code}, want 1 (GET) or 2 (POST)")


def _valid_port(port: str) -> bool:
    return port == "" or re.fullmatch(r":[0-9]*", port) is not None


def xweb_api_origin(url: str) -> str:
    """``https://`` and the URL's authority: the origin every envelope of a payload is bound to."""
    raw = _utf8(url)
    if len(raw) == 0 or len(raw) > XWEB_MAX_URL_BYTES:
        raise _api_error(f"url is {len(raw)} bytes, want 1 to {XWEB_MAX_URL_BYTES}")
    for index, byte in enumerate(raw):
        if byte < 0x21 or byte > 0x7E:
            raise _api_error(f"url byte {index} is 0x{byte:02x}, not printable ASCII")
    if "#" in url:
        raise _api_error("url carries a fragment")
    if not url.startswith("https://"):
        raise _api_error(f"url {url!r} is not https")
    rest = url[len("https://") :]
    match = re.search(r"[/?]", rest)
    end = len(rest) if match is None else match.start()
    authority = rest[:end]
    if authority == "":
        raise _api_error(f"url {url!r} has no host")
    for character in authority:
        if ord(character) not in _AUTHORITY:
            raise _api_error(
                f"url authority {authority!r} holds {character!r}: only lower-case letters, digits and .-:[] are accepted"
            )
    if authority.startswith("["):
        close = authority.rfind("]")
        if close < 0 or not _valid_port(authority[close + 1 :]):
            raise _api_error(f"url {url!r} has a malformed host")
        hostname = authority[1:close]
    else:
        colon = authority.rfind(":")
        if colon >= 0 and not _valid_port(authority[colon:]):
            raise _api_error(f"url {url!r} has a malformed port")
        hostname = authority if colon < 0 else authority[:colon]
    query = rest.find("?")
    path = "" if match is None else rest[end : len(rest) if query < 0 else max(query, end)]
    for index, character in enumerate(path):
        if character == "%" and re.fullmatch(r"[0-9A-Fa-f]{2}", path[index + 1 : index + 3]) is None:
            raise _api_error(f"url {url!r} has an invalid escape")
    if hostname == "":
        raise _api_error(f"url {url!r} has no host")
    return "https://" + authority


def _validate_headers(what: str, headers: Sequence[XWebApiHeader], bound: int) -> None:
    if len(headers) > bound:
        raise _api_error(f"{len(headers)} {what}s, bound {bound}")
    seen: set[str] = set()
    for index, header in enumerate(headers):
        name = _utf8(header.name)
        if len(name) == 0 or len(name) > XWEB_MAX_HEADER_NAME_BYTES:
            raise _api_error(f"{what} {index} name is {len(name)} bytes, want 1 to {XWEB_MAX_HEADER_NAME_BYTES}")
        for byte in name:
            if byte not in _TOKEN:
                raise _api_error(f"{what} {index} name {header.name!r} holds {chr(byte)!r}, not a token character")
        lower = header.name.lower()
        if lower in _RESTRICTED_HEADERS:
            raise _api_error(f"{what} {index} is {header.name}, which the sidecar sets itself")
        if lower in seen:
            raise _api_error(f"{what} {index} repeats {header.name}")
        seen.add(lower)
        value = _utf8(header.value)
        if len(value) > XWEB_MAX_HEADER_VALUE_BYTES:
            raise _api_error(f"{what} {header.name} value is {len(value)} bytes, bound {XWEB_MAX_HEADER_VALUE_BYTES}")
        for at, byte in enumerate(value):
            if byte != 0x09 and (byte < 0x20 or byte > 0x7E):
                raise _api_error(f"{what} {header.name} value byte {at} is 0x{byte:02x}")


def _validate_pointers(pointers: Sequence[str]) -> None:
    if len(pointers) > XWEB_MAX_POINTERS:
        raise _api_error(f"{len(pointers)} pointers, bound {XWEB_MAX_POINTERS}")
    seen: set[str] = set()
    for index, pointer in enumerate(pointers):
        try:
            encoded = pointer.encode("utf-8")
        except UnicodeEncodeError:
            raise _api_error(f"pointer {index} is not UTF-8") from None
        if len(encoded) > XWEB_MAX_POINTER_BYTES:
            raise _api_error(f"pointer {index} is {len(encoded)} bytes, bound {XWEB_MAX_POINTER_BYTES}")
        if pointer != "" and not pointer.startswith("/"):
            raise _api_error(f"pointer {pointer!r} does not start with /")
        for at, character in enumerate(pointer):
            if character == "~" and pointer[at + 1 : at + 2] not in ("0", "1"):
                raise _api_error(f"pointer {pointer!r} has a ~ not followed by 0 or 1")
        if pointer in seen:
            raise _api_error(f"pointer {pointer!r} is repeated")
        seen.add(pointer)


def _public_key(data: bytes, label: str):
    ec = _crypto()[0]
    if len(data) != XWEB_ENVELOPE_KEY_LENGTH or data[0] not in (0x02, 0x03):
        raise XWebApiError("invalid_envelope", f"{label} is not a 33-byte compressed secp256k1 key")
    try:
        return ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256K1(), data)
    except ValueError:
        raise XWebApiError("invalid_envelope", f"{label} is not a point on secp256k1") from None


def xweb_attestor_address(public_key: bytes | str) -> str:
    """The EVM address a compressed secp256k1 public key signs as, lower-case."""
    serialization = _crypto()[2]
    key = _unhex(public_key, "public key", "invalid_envelope") if isinstance(public_key, str) else bytes(public_key)
    point = _public_key(key, "public key").public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint
    )
    return _hex(keccak256(point[1:])[12:])


def _validate_envelope(envelope: bytes, index: int) -> str:
    if len(envelope) < XWEB_ENVELOPE_OVERHEAD + 1 or len(envelope) > XWEB_MAX_ENVELOPE_BYTES:
        raise _api_error(
            f"envelope {index}: {len(envelope)} bytes, want {XWEB_ENVELOPE_OVERHEAD + 1} to {XWEB_MAX_ENVELOPE_BYTES}"
        )
    attestor = _hex(envelope[:20])
    if attestor == _ZERO_ADDRESS:
        raise _api_error(f"envelope {index}: addressed to the zero attestor")
    try:
        _public_key(envelope[20 : 20 + XWEB_ENVELOPE_KEY_LENGTH], "ephemeral key")
    except XWebApiError as error:
        raise _api_error(f"envelope {index}: {error}") from None
    return attestor


def _validate_payload(payload: XWebApiPayload) -> None:
    xweb_method_code(payload.method)
    attestor = _address(payload.attestor, "attestor")
    zero = attestor == bytes(20)
    if payload.level == XWEB_LEVEL_MAJORITY:
        if not zero:
            raise XWebApiError("invalid_level", f"majority level names attestor {payload.attestor.lower()}")
    elif payload.level == XWEB_LEVEL_SINGLE:
        if zero:
            raise XWebApiError("invalid_level", "single level names no attestor")
    else:
        raise XWebApiError("invalid_level", f"level {payload.level}")
    xweb_api_origin(payload.url)
    _validate_headers("header", payload.headers, XWEB_MAX_HEADERS)
    if len(payload.body) > XWEB_MAX_BODY_BYTES:
        raise _api_error(f"body is {len(payload.body)} bytes, bound {XWEB_MAX_BODY_BYTES}")
    if payload.method == "GET" and len(payload.body) != 0:
        raise _api_error(f"GET carries a {len(payload.body)}-byte body")
    _validate_pointers(payload.pointers)
    if len(payload.envelopes) > XWEB_MAX_ENVELOPES:
        raise _api_error(f"{len(payload.envelopes)} envelopes, bound {XWEB_MAX_ENVELOPES}")
    addressed: set[str] = set()
    for index, envelope in enumerate(payload.envelopes):
        to = _validate_envelope(envelope, index)
        if to in addressed:
            raise _api_error(f"envelope {index} repeats attestor {to}")
        addressed.add(to)
        if payload.level == XWEB_LEVEL_SINGLE and to != payload.attestor.lower():
            raise _api_error(f"envelope {index} is addressed to {to}, the single level names {payload.attestor.lower()}")


class _Writer:
    def __init__(self) -> None:
        self.out = bytearray()

    def u8(self, field: str, value: int) -> None:
        if value < 0 or value > 0xFF:
            raise _api_error(f"{field} count {value} does not fit one byte")
        self.out.append(value)

    def field(self, name: str, data: bytes) -> None:
        if len(data) > 0xFFFF:
            raise _api_error(f"{name} is {len(data)} bytes, over the uint16 length")
        self.out += len(data).to_bytes(2, "big") + data

    def headers(self, what: str, headers: Sequence[XWebApiHeader]) -> None:
        self.u8(what + "s", len(headers))
        for header in headers:
            self.field(what + " name", _utf8(header.name))
            self.field(what + " value", _utf8(header.value))


def encode_xweb_api_payload(payload: XWebApiPayload) -> bytes:
    """Validates ``payload`` and returns its api payload bytes, exactly as modules/xweb encodes them."""
    _validate_payload(payload)
    writer = _Writer()
    writer.out += bytes((XWEB_API_VERSION, xweb_method_code(payload.method), payload.level))
    writer.out += _address(payload.attestor, "attestor")
    writer.field("url", _utf8(payload.url))
    writer.headers("header", payload.headers)
    writer.field("body", payload.body)
    writer.u8("pointers", len(payload.pointers))
    for pointer in payload.pointers:
        writer.field("pointer", _utf8(pointer))
    writer.u8("envelopes", len(payload.envelopes))
    for envelope in payload.envelopes:
        writer.field("envelope", envelope)
    return bytes(writer.out)


class _Reader:
    def __init__(self, data: bytes) -> None:
        self.data = data
        self.at = 0

    def take(self, field: str, length: int) -> bytes:
        if len(self.data) - self.at < length:
            raise _api_error(f"payload ends inside {field} at byte {self.at}")
        out = self.data[self.at : self.at + length]
        self.at += length
        return out

    def u8(self, field: str) -> int:
        return self.take(field, 1)[0]

    def field(self, name: str) -> bytes:
        length = self.take(name + " length", 2)
        return self.take(name, int.from_bytes(length, "big"))

    def headers(self, what: str) -> tuple[XWebApiHeader, ...]:
        count = self.u8(what + " count")
        headers = []
        for index in range(count):
            name = self.field(f"{what} {index} name")
            value = self.field(f"{what} {index} value")
            headers.append(XWebApiHeader(name.decode("latin-1"), value.decode("latin-1")))
        return tuple(headers)


def decode_xweb_api_payload(raw: bytes) -> XWebApiPayload:
    """Decodes and validates api payload bytes; nothing may follow the last envelope."""
    reader = _Reader(bytes(raw))
    head = reader.take("the version, method, level and attestor", 3 + 20)
    if head[0] != XWEB_API_VERSION:
        raise _api_error(f"version {head[0]}, want {XWEB_API_VERSION}")
    method = _method_name(head[1])
    level = head[2]
    attestor = _hex(head[3:23])
    url = reader.field("url").decode("latin-1")
    headers = reader.headers("header")
    body = reader.field("body")
    pointers = []
    for index in range(reader.u8("pointer count")):
        try:
            pointers.append(reader.field(f"pointer {index}").decode("utf-8"))
        except UnicodeDecodeError:
            raise _api_error(f"pointer {index} is not UTF-8") from None
    envelopes = []
    for index in range(reader.u8("envelope count")):
        envelope = reader.field(f"envelope {index}")
        _validate_envelope(envelope, index)
        envelopes.append(envelope)
    rest = len(reader.data) - reader.at
    if rest != 0:
        raise _api_error(f"{rest} bytes follow the last envelope")
    payload = XWebApiPayload(method, level, attestor, url, headers, body, tuple(pointers), tuple(envelopes))
    _validate_payload(payload)
    return payload


def encode_xweb_credential(
    headers: Sequence[XWebApiHeader], public_headers: Sequence[XWebApiHeader] = ()
) -> bytes:
    """The plaintext a credential envelope carries: the credential headers in the payload's header encoding."""
    if len(headers) == 0:
        raise _api_error("credential carries no header")
    _validate_headers("credential header", headers, XWEB_MAX_CREDENTIAL_HEADERS)
    for header in headers:
        if any(other.name.lower() == header.name.lower() for other in public_headers):
            raise _api_error(f"credential header {header.name} repeats a public header")
    writer = _Writer()
    writer.headers("credential header", headers)
    if len(writer.out) > XWEB_MAX_CREDENTIAL_BYTES:
        raise _api_error(f"credential is {len(writer.out)} bytes, bound {XWEB_MAX_CREDENTIAL_BYTES}")
    return bytes(writer.out)


def seal_xweb_envelope(
    public_key: bytes | str,
    origin: str,
    plaintext: bytes,
    randomness: XWebEnvelopeRandomness | None = None,
) -> bytes:
    """Seals a credential plaintext to one attestor's compressed public key for ``origin``.

    Without ``randomness`` the ephemeral key and nonce come from the operating
    system's secure source; a fixed pair must never be reused.
    """
    ec, hashes, serialization, AESGCM, HKDF = _crypto()
    if len(plaintext) == 0 or len(plaintext) > XWEB_MAX_CREDENTIAL_BYTES:
        raise XWebApiError(
            "invalid_envelope", f"plaintext is {len(plaintext)} bytes, want 1 to {XWEB_MAX_CREDENTIAL_BYTES}"
        )
    recipient_bytes = (
        _unhex(public_key, "public key", "invalid_envelope") if isinstance(public_key, str) else bytes(public_key)
    )
    recipient = _public_key(recipient_bytes, "public key")
    attestor = bytes.fromhex(xweb_attestor_address(recipient_bytes)[2:])
    if randomness is None:
        ephemeral = ec.generate_private_key(ec.SECP256K1())
        nonce = secrets.token_bytes(XWEB_ENVELOPE_NONCE_LENGTH)
    else:
        scalar = int.from_bytes(randomness.ephemeral_private_key, "big")
        if len(randomness.ephemeral_private_key) != 32 or not 0 < scalar < _SECP256K1_ORDER:
            raise XWebApiError("invalid_envelope", "ephemeral private key is not a secp256k1 scalar")
        if len(randomness.nonce) != XWEB_ENVELOPE_NONCE_LENGTH:
            raise XWebApiError(
                "invalid_envelope", f"nonce is {len(randomness.nonce)} bytes, want {XWEB_ENVELOPE_NONCE_LENGTH}"
            )
        ephemeral = ec.derive_private_key(scalar, ec.SECP256K1())
        nonce = bytes(randomness.nonce)
    ephemeral_key = ephemeral.public_key().public_bytes(
        serialization.Encoding.X962, serialization.PublicFormat.CompressedPoint
    )
    shared = ephemeral.exchange(ec.ECDH(), recipient)
    key = HKDF(algorithm=hashes.SHA256(), length=32, salt=ephemeral_key, info=XWEB_ENVELOPE_INFO).derive(shared)
    ciphertext = AESGCM(key).encrypt(nonce, bytes(plaintext), attestor + origin.encode("ascii"))
    return attestor + ephemeral_key + nonce + ciphertext


def _envelope_recipients(call: XWebApiCall, attestors: XWebAttestorSet) -> tuple[XWebAttestor, ...]:
    if call.single is not None:
        named = next((entry for entry in attestors.attestors if entry.signer.lower() == call.single.lower()), None)
        if named is None:
            raise XWebApiError("unknown_attestor", f"the single level names {call.single.lower()}")
        if len(named.public_key) == 0:
            raise XWebApiError("invalid_attestors", f"attestor {named.signer.lower()} takes no credential envelopes")
        return (named,)
    keyed = tuple(entry for entry in attestors.attestors if len(entry.public_key) != 0)
    if len(keyed) < attestors.required or len(keyed) == 0:
        raise XWebApiError(
            "invalid_attestors",
            f"{len(keyed)} of {len(attestors.attestors)} attestors take credential envelopes, "
            f"a majority fulfilment needs {attestors.required}",
        )
    return keyed


def build_xweb_api_request(
    call: XWebApiCall,
    attestors: XWebAttestorSet,
    *,
    credential: Sequence[XWebApiHeader] | None = None,
    max_payload_bytes: int = XWEB_DEFAULT_MAX_PAYLOAD_BYTES,
    randomness: Callable[[str], XWebEnvelopeRandomness] | None = None,
) -> XWebApiRequest:
    """Builds an api request from ``call`` and the set getAttestors returns.

    The payload is what a contract passes to request(3, payload, callbackGas)
    or through XWebApi; with ``credential`` it carries one envelope per
    attestor that can make the call, and the credential never appears in the
    clear.
    """
    origin = xweb_api_origin(call.url)
    level = XWEB_LEVEL_MAJORITY if call.single is None else XWEB_LEVEL_SINGLE
    attestor = _ZERO_ADDRESS if call.single is None else _hex(_address(call.single, "single attestor"))
    if call.single is not None and not any(entry.signer.lower() == attestor for entry in attestors.attestors):
        raise XWebApiError("unknown_attestor", f"the single level names {attestor}")
    sealed: list[XWebSealedEnvelope] = []
    if credential is not None:
        plaintext = encode_xweb_credential(credential, call.headers)
        for recipient in _envelope_recipients(call, attestors):
            signer = _hex(_address(recipient.signer, "attestor signer"))
            derived = xweb_attestor_address(recipient.public_key)
            if derived != signer:
                raise XWebApiError("invalid_attestors", f"public key of {signer} belongs to {derived}")
            chosen = None if randomness is None else randomness(signer)
            sealed.append(XWebSealedEnvelope(signer, seal_xweb_envelope(recipient.public_key, origin, plaintext, chosen)))
    if len(sealed) > len(attestors.attestors):
        raise _api_error(f"{len(sealed)} envelopes for {len(attestors.attestors)} registered attestors")
    body = call.body.encode("utf-8") if isinstance(call.body, str) else bytes(call.body)
    payload = encode_xweb_api_payload(
        XWebApiPayload(
            call.method,
            level,
            attestor,
            call.url,
            tuple(call.headers),
            body,
            tuple(call.pointers),
            tuple(entry.envelope for entry in sealed),
        )
    )
    if len(payload) > max_payload_bytes:
        raise XWebApiError("payload_too_large", f"payload is {len(payload)} bytes, cap {max_payload_bytes}")
    return XWebApiRequest(payload, _hex(keccak256(payload)), origin, level, attestor, tuple(sealed))


def _selector(signature: str) -> str:
    return _hex(keccak256(signature.encode("ascii"))[:4])


def xweb_api_request_call(payload: bytes, callback_gas: int, fee: int) -> XWebPrecompileCall:
    """The request(uint8,bytes,uint64) transaction for an api payload, paying ``fee`` wei of PAX."""
    if not 0 < callback_gas < 1 << 64:
        raise XWebApiError("invalid_abi", f"callback gas {callback_gas} is not a positive uint64")
    if not 0 <= fee < 1 << 256:
        raise XWebApiError("invalid_abi", f"fee {fee} is not a uint256")
    padded = bytes(payload) + b"\x00" * (-len(payload) % _WORD)
    words = (
        XWEB_KIND_API.to_bytes(_WORD, "big")
        + (3 * _WORD).to_bytes(_WORD, "big")
        + callback_gas.to_bytes(_WORD, "big")
        + len(payload).to_bytes(_WORD, "big")
        + padded
    )
    return XWebPrecompileCall(XWEB_PRECOMPILE, _selector("request(uint8,bytes,uint64)") + words.hex(), fee)


def xweb_get_attestors_call_data() -> str:
    """The eth_call data of getAttestors()."""
    return _selector("getAttestors()")


class _AbiReader:
    def __init__(self, data: bytes) -> None:
        self.data = data

    def word(self, offset: int) -> bytes:
        if offset < 0 or offset + _WORD > len(self.data):
            raise XWebApiError("invalid_abi", f"getAttestors answer ends before byte {offset + _WORD}")
        return self.data[offset : offset + _WORD]

    def uint(self, offset: int, bits: int) -> int:
        value = int.from_bytes(self.word(offset), "big")
        if value >= 1 << bits:
            raise XWebApiError("invalid_abi", f"word at {offset} is not a uint{bits}")
        return value

    def address(self, offset: int) -> str:
        word = self.word(offset)
        if any(word[:12]):
            raise XWebApiError("invalid_abi", f"word at {offset} is not an address")
        return _hex(word[12:])

    def bytes_at(self, offset: int) -> bytes:
        length = self.uint(offset, 32)
        start = offset + _WORD
        if start + length > len(self.data):
            raise XWebApiError("invalid_abi", f"bytes at {offset} run past the answer")
        return self.data[start : start + length]


def decode_xweb_attestors(answer: str) -> XWebAttestorSet:
    """Decodes the answer of getAttestors() into the attestor set a request is built from."""
    reader = _AbiReader(_unhex(answer, "getAttestors answer", "invalid_abi"))
    array = reader.uint(0, 32)
    required = reader.uint(_WORD, 32)
    count = reader.uint(array, 32)
    base = array + _WORD
    attestors = []
    for index in range(count):
        entry = base + reader.uint(base + index * _WORD, 32)
        signer = reader.address(entry)
        try:
            payout = reader.bytes_at(entry + reader.uint(entry + _WORD, 32)).decode("utf-8")
        except UnicodeDecodeError:
            raise XWebApiError("invalid_abi", f"payout of {signer} is not UTF-8") from None
        public_key = reader.bytes_at(entry + reader.uint(entry + 2 * _WORD, 32))
        if public_key:
            derived = xweb_attestor_address(public_key)
            if derived != signer:
                raise XWebApiError("invalid_attestors", f"public key of {signer} belongs to {derived}")
        attestors.append(XWebAttestor(signer, payout, public_key))
    return XWebAttestorSet(tuple(attestors), required)
