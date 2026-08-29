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
    def __init__(self, client: ProductionClient, signatures: LocalSignatureVerifier) -> None:
        self._client = client
        self._signatures = signatures

    def discover(self, program_id: str) -> Mapping[str, object]:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        return _discovery(self._client.agent("program.discover", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}), program_id)

    def interface(self, program_id: str) -> Mapping[str, object]:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        return _interface(self._client.agent("program.interface", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}), program_id)

    def simulate(self, call: ProgramCall) -> Mapping[str, object]:
        _validate(call)
        result = _mapping(self._client.agent("program.simulate", _wire(call)))
        if result.get("committed") is not False:
            raise ValueError("committed program simulation")
        execution = _execution(result.get("execution"), "simulated")
        if execution["program_id"] != call.program_id:
            raise ValueError("program simulation binding failed")
        verify_program_receipt(execution, _authority(execution), self._signatures)
        _verify_simulation(result.get("simulation_evidence"), execution, self._signatures)
        return result

    def submit(self, call: ProgramCall, idempotency_key: IdempotencyKey) -> Mapping[str, object]:
        _validate(call)
        if not _hex32(str(idempotency_key)):
            raise ValueError("invalid program idempotency key")
        result = self._client.agent("program.call", _wire(call), idempotency_key=idempotency_key)
        return _submission(result, self._signatures, program_id=call.program_id, idempotency_key=str(idempotency_key), retained_signed_activity=call.signed_activity.hex())

    def receipt(self, idempotency_key: str, expected_activity_id: str) -> Mapping[str, object]:
        if not _hex32(idempotency_key) or not _hex32(expected_activity_id):
            raise ValueError("invalid program receipt selector")
        result = self._client.agent("program.receipt", {"idempotency_key": idempotency_key, "expected_activity_id": expected_activity_id, "requested_verification_level": "sequencer-signed"})
        return _submission(result, self._signatures, activity_id=expected_activity_id, idempotency_key=idempotency_key)

    def activity(self, activity_id: str) -> Mapping[str, object]:
        if not _hex32(activity_id):
            raise ValueError("invalid activity id")
        result = self._client.agent("program.activity", {"activity_id": activity_id, "requested_verification_level": "sequencer-signed"})
        return _submission(result, self._signatures, activity_id=activity_id)


def _submission(
    value: object,
    signatures: LocalSignatureVerifier,
    *,
    program_id: str | None = None,
    activity_id: str | None = None,
    idempotency_key: str | None = None,
    retained_signed_activity: str | None = None,
) -> Mapping[str, object]:
    result = _mapping(value)
    state = result.get("state")
    if state == "unknown":
        actual_activity = _hex_field(result, "activity_id", 32, exact=True)
        actual_idempotency = _hex_field(result, "idempotency_key", 32, exact=True)
        retained = _hex_field(result, "retained_signed_activity", _MAX_CALLDATA)
        if (activity_id is not None and actual_activity != activity_id) or (idempotency_key is not None and actual_idempotency != idempotency_key) or (retained_signed_activity is not None and retained != retained_signed_activity):
            raise ValueError("program unknown binding failed")
        return result
    if state not in {"executed", "refused"}:
        raise ValueError("invalid program submission state")
    execution = _execution(result, cast(Literal["executed", "refused"], state))
    if (program_id is not None and execution["program_id"] != program_id) or (activity_id is not None and execution["activity_id"] != activity_id) or (idempotency_key is not None and execution.get("idempotency_key") != idempotency_key):
        raise ValueError("program execution binding failed")
    verify_program_receipt(execution, _authority(execution), signatures)
    return execution


def _execution(value: object, expected_state: Literal["executed", "refused", "simulated"]) -> Mapping[str, object]:
    execution = _mapping(value)
    if execution.get("state") != expected_state:
        raise ValueError("invalid program execution state")
    _hex_field(execution, "activity_id", 32, exact=True)
    _hex_field(execution, "program_id", 32, exact=True)
    _hex_field(execution, "state_root", 32, exact=True)
    _hex_field(execution, "receipt", _MAX_CALLDATA)
    _hex_field(execution, "terminal_payload", _MAX_CALLDATA)
    _hex_field(execution, "call_graph", _MAX_CALLDATA)
    for field in ("global_sequence",):
        _decimal(execution.get(field), (1 << 64) - 1)
    if not isinstance(execution.get("module_version"), int) or isinstance(execution.get("module_version"), bool) or execution["module_version"] not in (1, 2, 3) or not isinstance(execution.get("guest_abi_version"), int) or isinstance(execution.get("guest_abi_version"), bool) or execution.get("guest_abi_version") not in (1, 2) or not isinstance(execution.get("result_code"), int) or isinstance(execution.get("result_code"), bool):
        raise ValueError("invalid program execution metadata")
    outcome = _mapping(execution.get("outcome"))
    if (expected_state == "refused") != (outcome.get("kind") == "refused"):
        raise ValueError("program state/outcome mismatch")
    usage = _mapping(execution.get("usage"))
    for field in ("cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes", "output_bytes"):
        _decimal(usage.get(field), (1 << 64) - 1)
    _decimal(usage.get("fee_units"), _MAX_U128)
    if not isinstance(usage.get("output_values"), int) or isinstance(usage.get("output_values"), bool) or not 0 <= cast(int, usage["output_values"]) <= (1 << 32) - 1:
        raise ValueError("invalid program usage")
    return execution


def _authority(execution: Mapping[str, object]) -> AuthorizedReceiptBatch:
    authority = _mapping(execution.get("authority"))
    return AuthorizedReceiptBatch(
        batch_id=bytes.fromhex(_hex_field(authority, "batch_id", 32, exact=True)),
        asset=bytes.fromhex(_hex_field(authority, "asset", 32, exact=True)),
        previous_state_root=bytes.fromhex(_hex_field(authority, "previous_state_root", 32, exact=True)),
        resulting_state_root=bytes.fromhex(_hex_field(authority, "resulting_state_root", 32, exact=True)),
        sequencer_public_key=bytes.fromhex(_hex_field(authority, "sequencer_public_key", 32, exact=True)),
    )


def _verify_simulation(value: object, execution: Mapping[str, object], signatures: LocalSignatureVerifier) -> None:
    evidence = _mapping(value)
    boundary_id = bytes.fromhex(_hex_field(evidence, "boundary_id", 32, exact=True))
    public_key = bytes.fromhex(_hex_field(evidence, "public_key", 32, exact=True))
    expected_boundary = sha256(b"LayerX/emulator/simulation-boundary/v1\0" + public_key).digest()
    if boundary_id != expected_boundary or evidence.get("committed") is not False:
        raise ValueError("simulation boundary mismatch")
    evidence_activity = _hex_field(evidence, "activity_id", 32, exact=True)
    previous = bytes.fromhex(_hex_field(evidence, "previous_state_root", 32, exact=True))
    hypothetical = _hex_field(evidence, "hypothetical_state_root", 32, exact=True)
    sequence = _decimal(evidence.get("observed_sequence"), (1 << 64) - 1)
    observed_at = _decimal(evidence.get("observed_at"), (1 << 64) - 1)
    signature = bytes.fromhex(_hex_field(evidence, "signature", 64, exact=True))
    if evidence_activity != execution["activity_id"] or hypothetical != execution["state_root"]:
        raise ValueError("simulation evidence binding failed")
    signed = b"LayerX/agent/program-simulation-evidence/v1\0" + boundary_id + bytes.fromhex(evidence_activity) + previous + bytes.fromhex(hypothetical) + sequence.to_bytes(8, "big") + observed_at.to_bytes(8, "big") + b"\0"
    if not signatures.verify_ed25519(public_key, signature, sha256(signed).digest()):
        raise ValueError("simulation evidence signature mismatch")


def _discovery(value: object, program_id: str) -> Mapping[str, object]:
    result = _mapping(value)
    if _hex_field(result, "program_id", 32, exact=True) != program_id or result.get("verification") != "registry-receipt-and-current-head-verified":
        raise ValueError("unverified program discovery")
    for field in ("observed_sequence", "observed_at", "valid_through"):
        _decimal(result.get(field), (1 << 64) - 1)
    return result


def _interface(value: object, program_id: str) -> Mapping[str, object]:
    result = _mapping(value)
    if _hex_field(result, "program_id", 32, exact=True) != program_id or result.get("verification") != "deployment-interface-and-current-head-verified":
        raise ValueError("unverified program interface")
    for field in ("observed_sequence", "observed_at", "valid_through"):
        _decimal(result.get(field), (1 << 64) - 1)
    return result


def _mapping(value: object) -> Mapping[str, object]:
    if not isinstance(value, Mapping) or any(not isinstance(key, str) for key in value):
        raise ValueError("invalid program document")
    return cast(Mapping[str, object], value)


def _hex_field(value: Mapping[str, object], field: str, maximum: int, *, exact: bool = False) -> str:
    candidate = value.get(field)
    if not isinstance(candidate, str) or len(candidate) % 2 or len(candidate) > maximum * 2 or (exact and len(candidate) != maximum * 2) or any(character not in "0123456789abcdef" for character in candidate):
        raise ValueError("invalid program hexadecimal field")
    return candidate


def _decimal(value: object, maximum: int) -> int:
    if not isinstance(value, str) or not value.isascii() or not value.isdigit() or (value != "0" and value.startswith("0")):
        raise ValueError("invalid program decimal")
    parsed = int(value)
    if parsed > maximum:
        raise ValueError("program decimal overflow")
    return parsed


def _wire(call: ProgramCall) -> Mapping[str, object]:
    return {"program_id": call.program_id, "calldata": call.calldata.hex(), "budget": {"fuel": str(call.fuel), "fee_limit": str(call.fee_limit)}, "capabilities": call.capabilities, "signed_activity": call.signed_activity.hex()}


def platform_sdk_programs() -> str:
    return "receipt-verified-program-operations-v1"
