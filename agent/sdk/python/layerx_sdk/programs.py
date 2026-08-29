from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from typing import Literal, Mapping, cast

from .production import IdempotencyKey, ProductionClient
from .verifier import AuthorizedReceiptBatch, LocalSignatureVerifier, ReceiptVerification, verify_receipt_outcome

ProgramCapability = Literal["storage_read", "storage_write", "transfer", "emit_event", "compose"]
_CAPABILITY_ORDER: Mapping[str, int] = {"storage_read": 1, "storage_write": 2, "transfer": 3, "emit_event": 4, "compose": 5}
_MAX_CALLDATA = 1_048_576
_MAX_U64 = (1 << 64) - 1
_MAX_U128 = (1 << 128) - 1


@dataclass(frozen=True)
class ProgramCall:
    program_id: str
    calldata: bytes
    fuel: int
    fee_limit: int
    capabilities: tuple[ProgramCapability, ...]
    signed_activity: bytes


@dataclass(frozen=True)
class VerifiedProgramReceipt:
    verification: ReceiptVerification
    terminal_payload: bytes
    call_graph: bytes


def _hex32(value: str) -> bool:
    return len(value) == 64 and all(character in "0123456789abcdef" for character in value)


def _validate(call: ProgramCall) -> None:
    if not _hex32(call.program_id) or not 0 < call.fuel <= _MAX_U64 or not 0 <= call.fee_limit <= _MAX_U128:
        raise ValueError("invalid bounded program call")
    if len(call.calldata) > _MAX_CALLDATA or len(call.capabilities) > 5 or not 0 < len(call.signed_activity) <= _MAX_CALLDATA:
        raise ValueError("invalid bounded program call")
    prior = 0
    for capability in call.capabilities:
        current = _CAPABILITY_ORDER.get(capability)
        if current is None or current <= prior:
            raise ValueError("program capabilities must be canonical")
        prior = current


def _evidence_bytes(execution: Mapping[str, object], field: str) -> bytes:
    value = execution.get(field)
    if not isinstance(value, str) or len(value) > 2_097_152 or len(value) % 2 or any(character not in "0123456789abcdef" for character in value):
        raise ValueError("program evidence is not canonical hexadecimal")
    return bytes.fromhex(value)


def verify_program_receipt(
    execution: Mapping[str, object],
    authority: AuthorizedReceiptBatch,
    signatures: LocalSignatureVerifier,
) -> VerifiedProgramReceipt:
    activity_id = execution.get("activity_id")
    module_version = execution.get("module_version")
    guest_abi = execution.get("guest_abi_version")
    result_code = execution.get("result_code")
    if not isinstance(activity_id, str) or not _hex32(activity_id) or not isinstance(module_version, int) or module_version not in (1, 2, 3) or guest_abi not in (1, 2) or not isinstance(result_code, int):
        raise ValueError("invalid program execution evidence")
    receipt = _evidence_bytes(execution, "receipt")
    terminal_payload = _evidence_bytes(execution, "terminal_payload")
    call_graph = _evidence_bytes(execution, "call_graph")
    verification = verify_receipt_outcome(receipt, authority, signatures)
    protocol = verification.receipt
    outcome = protocol.program_outcome
    if protocol.module_id != 9 or protocol.operation != 3 or protocol.module_version != module_version or protocol.activity_id.hex() != activity_id or outcome is None or outcome.abi_version != guest_abi or outcome.result_code != result_code or not call_graph or sha256(terminal_payload).digest() != outcome.terminal_payload_root or sha256(call_graph).digest() != outcome.call_graph_root:
        raise ValueError("program receipt binding failed")
    return VerifiedProgramReceipt(verification, terminal_payload, call_graph)


class ProgramOperations:
    def __init__(self, client: ProductionClient) -> None:
        self._client = client

    def discover(self, program_id: str) -> Mapping[str, object]:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        return cast(Mapping[str, object], self._client.agent("program.discover", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}))

    def interface(self, program_id: str) -> Mapping[str, object]:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        return cast(Mapping[str, object], self._client.agent("program.interface", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}))

    def simulate(self, call: ProgramCall) -> Mapping[str, object]:
        _validate(call)
        return cast(Mapping[str, object], self._client.agent("program.simulate", _wire(call)))

    def submit(self, call: ProgramCall, idempotency_key: IdempotencyKey) -> Mapping[str, object]:
        _validate(call)
        return cast(Mapping[str, object], self._client.agent("program.call", _wire(call), idempotency_key=idempotency_key))

    def receipt(self, idempotency_key: str, expected_activity_id: str) -> Mapping[str, object]:
        if not _hex32(idempotency_key) or not _hex32(expected_activity_id):
            raise ValueError("invalid program receipt selector")
        result = cast(Mapping[str, object], self._client.agent("program.receipt", {"idempotency_key": idempotency_key, "expected_activity_id": expected_activity_id, "requested_verification_level": "sequencer-signed"}))
        if result.get("activity_id") != expected_activity_id:
            raise ValueError("program receipt selector binding failed")
        return result

    def activity(self, activity_id: str) -> Mapping[str, object]:
        if not _hex32(activity_id):
            raise ValueError("invalid activity id")
        return cast(Mapping[str, object], self._client.agent("program.activity", {"activity_id": activity_id, "requested_verification_level": "sequencer-signed"}))


def _wire(call: ProgramCall) -> Mapping[str, object]:
    return {"program_id": call.program_id, "calldata": call.calldata.hex(), "budget": {"fuel": str(call.fuel), "fee_limit": str(call.fee_limit)}, "capabilities": call.capabilities, "signed_activity": call.signed_activity.hex()}


def platform_sdk_programs() -> str:
    return "receipt-verified-program-operations-v1"
