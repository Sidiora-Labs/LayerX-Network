from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Literal, Protocol

from .verifier import AuthorizedReceiptBatch, LocalSignatureVerifier

WEB_CONTENT_DOMAIN: bytes
WEB_CONTENT_FETCH: int
WEB_CONTENT_SEARCH: int
WEB_SEARCH_CURRENCIES: tuple[str, ...]
WEB_SEARCH_PAYER_HEADER: str
WEB_SEARCH_MAX_RESULTS: int

WebSearchScheme = Literal["metered", "exact"]

class WebSearchError(Exception):
    code: str
    status: int | None
    def __init__(self, code: str, status: int | None = ...) -> None: ...

@dataclass(frozen=True)
class WebContent:
    kind: int
    payload: bytes
    media_type: str
    text: str

@dataclass(frozen=True)
class WebSearchOffer:
    scheme: WebSearchScheme
    network: str
    amount: str
    asset: str
    pay_to: str
    max_timeout_seconds: int
    currency: str
    account: str
    payer: str | None = ...
    purpose_hash: str | None = ...

@dataclass(frozen=True)
class WebSearchPreference:
    currency: str
    scheme: WebSearchScheme

@dataclass(frozen=True)
class WebSearchMeteredPayment:
    grant: bytes
    idempotency_key: str

@dataclass(frozen=True)
class WebSearchExactPayment:
    receipt: bytes

class WebSearchPayer(Protocol):
    @property
    def did(self) -> str | None: ...
    @property
    def preferences(self) -> Sequence[WebSearchPreference]: ...
    def metered(self, offer: WebSearchOffer, target: str) -> WebSearchMeteredPayment: ...
    def exact(self, offer: WebSearchOffer, target: str) -> WebSearchExactPayment: ...

@dataclass(frozen=True)
class WebSearchAssetTerms:
    asset_id: str
    max_amount: int

@dataclass(frozen=True)
class WebSettlement:
    scheme: WebSearchScheme
    currency: str
    network: str
    asset: str
    amount: str
    payer: str
    receipt_digest: str
    transaction: str

@dataclass(frozen=True)
class WebSearchResult:
    url: str
    title: str
    snippet: str

@dataclass(frozen=True)
class WebSearchResponse:
    results: tuple[WebSearchResult, ...]
    settlement: WebSettlement | None

@dataclass(frozen=True)
class WebFetchResponse:
    url: str
    final_url: str
    media_type: str
    digest: str
    text: str
    settlement: WebSettlement | None

@dataclass(frozen=True)
class WebContentResponse:
    digest: str
    canonical: bytes
    content: WebContent
    settlement: WebSettlement | None

def web_content_bytes(kind: int, payload: bytes | str, media_type: str, text: str) -> bytes: ...
def content_digest(canonical: bytes) -> str: ...
def decode_web_content(canonical: bytes) -> WebContent: ...
def decode_payer_grant(canonical: bytes) -> dict[str, object]: ...

class WebSearchClient:
    def __init__(
        self,
        endpoint: str,
        network: str,
        native_asset: str,
        assets: Mapping[str, WebSearchAssetTerms],
        payer: WebSearchPayer,
        authority: Callable[[bytes, WebSearchOffer], AuthorizedReceiptBatch],
        signatures: LocalSignatureVerifier,
        *,
        protocol_version: int | None = ...,
        now: Callable[[], int] | None = ...,
        pending_attempts: int = ...,
    ) -> None: ...
    def search(self, query: str) -> WebSearchResponse: ...
    def fetch(self, url: str) -> WebFetchResponse: ...
    def content(self, digest: str) -> WebContentResponse: ...
