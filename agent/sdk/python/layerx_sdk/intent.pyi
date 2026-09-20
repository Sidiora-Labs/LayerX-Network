from typing import Literal

from .agent_http import LayerXKeyCredential
from .production import IdempotencyKey, PlatformSdkError

IntentEndpointKind = Literal["paxeer-wallet", "human", "agent", "agent-budget"]
IntentDomain = Literal["paxeer", "layerx"]
IntentOperation = Literal["intent.plan", "intent.submit"]
HumanErrorCode = Literal[
    "unauthenticated", "session-expired", "step-up-required", "forbidden", "not-found",
    "invalid-request", "conflict", "rate-limited", "cursor-expired", "unavailable",
    "upstream-degraded", "challenge-expired", "refused-by-policy", "refused-by-budget",
    "refused-by-capability", "refused-by-protocol", "refused-by-limit", "quote-expired",
    "wallet-not-bound", "exit-unavailable", "already-decided", "hold-expired", "hold-defective",
    "archive-needs-disposition", "confirmation-mismatch", "not-suppressible", "support-unavailable",
    "support-conversation-unknown", "support-message-unknown",
]
HumanRetriability = Literal["retriable", "retriable-after", "structural", "final"]

INTENT_ENDPOINT_KINDS: tuple[IntentEndpointKind, ...]
INTENT_DOMAINS: tuple[IntentDomain, ...]
HUMAN_ERROR_CODES: tuple[HumanErrorCode, ...]
HUMAN_RETRIABILITIES: tuple[HumanRetriability, ...]

class IntentMoney:
    amount: int
    currency: str
    def __init__(self, amount: int, currency: str) -> None: ...

class IntentEndpointRef:
    kind: IntentEndpointKind
    account: str | None
    def __init__(self, kind: IntentEndpointKind, account: str | None) -> None: ...

class IntentConstraints:
    deadline: str
    max_fee: IntentMoney
    allow_top_up: bool
    def __init__(self, deadline: str, max_fee: IntentMoney, allow_top_up: bool) -> None: ...

class PlanIntentRequest:
    source: IntentEndpointRef
    destination: IntentEndpointRef
    asset_id: str
    money: IntentMoney
    constraints: IntentConstraints
    def __init__(self, source: IntentEndpointRef, destination: IntentEndpointRef, asset_id: str, money: IntentMoney, constraints: IntentConstraints) -> None: ...

class IntentLeg:
    index: int
    mechanism: str
    domain: IntentDomain
    source: IntentEndpointRef
    destination: IntentEndpointRef
    money: IntentMoney
    fee: IntentMoney
    def __init__(self, index: int, mechanism: str, domain: IntentDomain, source: IntentEndpointRef, destination: IntentEndpointRef, money: IntentMoney, fee: IntentMoney) -> None: ...

class IntentSigningRequirement:
    leg_index: int
    action_key: str
    signing_context: str
    authority: str
    def __init__(self, leg_index: int, action_key: str, signing_context: str, authority: str) -> None: ...

class IntentPlan:
    plan_digest: str
    journey_kind: str
    total_fee: IntentMoney
    legs: tuple[IntentLeg, ...]
    signing_requirements: tuple[IntentSigningRequirement, ...]
    def __init__(self, plan_digest: str, journey_kind: str, total_fee: IntentMoney, legs: tuple[IntentLeg, ...], signing_requirements: tuple[IntentSigningRequirement, ...]) -> None: ...

class IntentLegBinding:
    leg_index: int
    action_key: str
    actor: str
    authority: str
    relationship: str
    account_sequence: int
    not_before: int
    not_after: int
    fee_limit: IntentMoney
    def __init__(self, leg_index: int, action_key: str, actor: str, authority: str, relationship: str, account_sequence: int, not_before: int, not_after: int, fee_limit: IntentMoney) -> None: ...

class SubmitPlanRequest:
    plan_digest: str
    signed_digest: str
    bindings: tuple[IntentLegBinding, ...]
    def __init__(self, plan_digest: str, signed_digest: str, bindings: tuple[IntentLegBinding, ...]) -> None: ...

class IntentSubmission:
    journey_id: str
    plan_digest: str
    state: str
    state_copy_key: str
    def __init__(self, journey_id: str, plan_digest: str, state: str, state_copy_key: str) -> None: ...

class HumanIntentError(PlatformSdkError):
    status: int
    human_code: HumanErrorCode
    copy_key: str
    retriability: HumanRetriability
    trace: str
    field: str | None
    def __init__(self, status: int, human_code: HumanErrorCode, copy_key: str, retriability: HumanRetriability, trace: str, field: str | None = ..., retry_after_ms: int | None = ...) -> None: ...

class HumanIntentClient:
    def __init__(self, endpoint: str, *, credential: LayerXKeyCredential | None = ..., timeout: float = ..., maximum_response_bytes: int = ...) -> None: ...
    def plan_intent(self, request: PlanIntentRequest) -> IntentPlan: ...
    def submit_plan(self, request: SubmitPlanRequest, idempotency_key: IdempotencyKey) -> IntentSubmission: ...

def encode_plan_intent_request(request: PlanIntentRequest) -> dict[str, object]: ...
def encode_submit_plan_request(request: SubmitPlanRequest) -> dict[str, object]: ...
def decode_human_envelope(status: int, encoded: bytes) -> object: ...
def decode_human_error(status: int, value: object, trace: str) -> HumanIntentError: ...
def decode_intent_plan(value: object) -> IntentPlan: ...
def decode_intent_submission(value: object) -> IntentSubmission: ...
