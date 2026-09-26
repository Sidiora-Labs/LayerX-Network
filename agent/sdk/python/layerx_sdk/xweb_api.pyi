from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Literal

XWEB_PRECOMPILE: str
XWEB_KIND_API: int
XWEB_API_VERSION: int
XWEB_METHOD_GET: int
XWEB_METHOD_POST: int
XWEB_LEVEL_MAJORITY: int
XWEB_LEVEL_SINGLE: int
XWEB_ENVELOPE_INFO: bytes
XWEB_DEFAULT_MAX_PAYLOAD_BYTES: int
XWEB_MAX_URL_BYTES: int
XWEB_MAX_HEADERS: int
XWEB_MAX_HEADER_NAME_BYTES: int
XWEB_MAX_HEADER_VALUE_BYTES: int
XWEB_MAX_BODY_BYTES: int
XWEB_MAX_POINTERS: int
XWEB_MAX_POINTER_BYTES: int
XWEB_MAX_ENVELOPES: int
XWEB_MAX_CREDENTIAL_HEADERS: int
XWEB_MAX_CREDENTIAL_BYTES: int
XWEB_ENVELOPE_KEY_LENGTH: int
XWEB_ENVELOPE_NONCE_LENGTH: int
XWEB_ENVELOPE_TAG_LENGTH: int
XWEB_ENVELOPE_OVERHEAD: int
XWEB_MAX_ENVELOPE_BYTES: int

XWebApiMethod = Literal["GET", "POST"]

class XWebApiError(ValueError):
    code: str
    def __init__(self, code: str, detail: str) -> None: ...

@dataclass(frozen=True)
class XWebApiHeader:
    name: str
    value: str

@dataclass(frozen=True)
class XWebApiPayload:
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
    signer: str
    payout: str
    public_key: bytes

@dataclass(frozen=True)
class XWebAttestorSet:
    attestors: tuple[XWebAttestor, ...]
    required: int

@dataclass(frozen=True)
class XWebEnvelopeRandomness:
    ephemeral_private_key: bytes
    nonce: bytes

@dataclass(frozen=True)
class XWebApiCall:
    method: XWebApiMethod
    url: str
    headers: tuple[XWebApiHeader, ...] = ...
    body: bytes | str = ...
    pointers: tuple[str, ...] = ...
    single: str | None = ...

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
    to: str
    data: str
    value: int

def xweb_method_code(method: str) -> int: ...
def xweb_api_origin(url: str) -> str: ...
def xweb_attestor_address(public_key: bytes | str) -> str: ...
def encode_xweb_api_payload(payload: XWebApiPayload) -> bytes: ...
def decode_xweb_api_payload(raw: bytes) -> XWebApiPayload: ...
def encode_xweb_credential(
    headers: Sequence[XWebApiHeader], public_headers: Sequence[XWebApiHeader] = ...
) -> bytes: ...
def seal_xweb_envelope(
    public_key: bytes | str,
    origin: str,
    plaintext: bytes,
    randomness: XWebEnvelopeRandomness | None = ...,
) -> bytes: ...
def build_xweb_api_request(
    call: XWebApiCall,
    attestors: XWebAttestorSet,
    *,
    credential: Sequence[XWebApiHeader] | None = ...,
    max_payload_bytes: int = ...,
    randomness: Callable[[str], XWebEnvelopeRandomness] | None = ...,
) -> XWebApiRequest: ...
def xweb_api_request_call(payload: bytes, callback_gas: int, fee: int) -> XWebPrecompileCall: ...
def xweb_get_attestors_call_data() -> str: ...
def decode_xweb_attestors(answer: str) -> XWebAttestorSet: ...
