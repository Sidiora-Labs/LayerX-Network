from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from threading import RLock
from time import time_ns
from typing import Callable, Literal, Mapping, cast

from .production import IdempotencyKey, PlatformSdkError, ProductionClient, SdkErrorCode
from .program_wire import (
    DecodedSignedProgramCall,
    assert_fresh_simulation_observation,
    decode_and_verify_program_terminal,
    decode_signed_program_call,
)
from .verifier import AuthorizedReceiptBatch, LocalSignatureVerifier, ReceiptVerification, verify_receipt_outcome

ProgramCapability = Literal["storage_read", "storage_write", "transfer", "emit_event", "compose"]
_CAPABILITY_ORDER: Mapping[str, int] = {"storage_read": 1, "storage_write": 2, "transfer": 3, "emit_event": 4, "compose": 5}
_MAX_CALLDATA = 1_048_576
_MAX_U64 = (1 << 64) - 1
_MAX_U128 = (1 << 128) - 1
_DEFAULT_MAXIMUM_SIMULATION_AGE_MILLISECONDS = 300_000


@dataclass(frozen=True)
class ProgramCall:
    program_id: str
    calldata: bytes
    fuel: int
    fee_limit: int
    capabilities: tuple[ProgramCapability, ...]
    signed_activity: bytes


ProgramLifecycle = Literal["active", "deprecated", "tombstoned"]
ProgramSourceStatus = Literal["unpublished", "verified", "mismatch"]


@dataclass(frozen=True)
class ProgramSource:
    status: ProgramSourceStatus
    source_digest: str | None = None
    environment_digest: str | None = None
    pipeline: str | None = None
    expected_code_hash: str | None = None
    reproduced_artifact_digest: str | None = None


@dataclass(frozen=True)
class ProgramDiscovery:
    program_id: str
    lifecycle: ProgramLifecycle
    version: int
    code_hash: str
    abi_version: int
    receipt_digest: str
    state_root: str
    observed_sequence: int
    observed_at: int
    valid_through: int
    verification: Literal["server-side-receipt-verification-only"] = "server-side-receipt-verification-only"


@dataclass(frozen=True)
class ProgramInterface:
    program_id: str
    version: int
    code_hash: str
    abi_version: int
    interface: bytes
    interface_digest: str
    receipt_digest: str
    state_root: str
    observed_sequence: int
    observed_at: int
    valid_through: int
    source: ProgramSource
    verification: Literal["server-side-receipt-verification-only"] = "server-side-receipt-verification-only"


@dataclass(frozen=True)
class ProgramTrustContext:
    sequencer_public_key: bytes
    clock_milliseconds: Callable[[], int] = lambda: time_ns() // 1_000_000
    maximum_simulation_age_milliseconds: int = _DEFAULT_MAXIMUM_SIMULATION_AGE_MILLISECONDS

    def __post_init__(self) -> None:
        if (
            not isinstance(self.sequencer_public_key, bytes)
            or len(self.sequencer_public_key) != 32
            or self.sequencer_public_key == bytes(32)
            or not callable(self.clock_milliseconds)
            or isinstance(self.maximum_simulation_age_milliseconds, bool)
            or not isinstance(self.maximum_simulation_age_milliseconds, int)
            or not 0 < self.maximum_simulation_age_milliseconds <= _MAX_U64
        ):
            raise ValueError("invalid pinned sequencer key")
        object.__setattr__(self, "sequencer_public_key", bytes(self.sequencer_public_key))

    def now_milliseconds(self) -> int:
        value = self.clock_milliseconds()
        if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= _MAX_U64:
            raise ValueError("invalid trust clock")
        return value


@dataclass(frozen=True)
class VerifiedProgramReceipt:
    verification: ReceiptVerification
    terminal_payload: bytes
    call_graph: bytes


def _hex32(value: str) -> bool:
    return len(value) == 64 and all(character in "0123456789abcdef" for character in value)


def _validate(call: ProgramCall) -> None:
    if not _hex32(call.program_id) or call.program_id == "0" * 64 or not 0 < call.fuel <= _MAX_U64 or not 0 <= call.fee_limit <= _MAX_U128:
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
    trust: ProgramTrustContext,
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
    if authority.sequencer_public_key != trust.sequencer_public_key or _mapping(execution.get("authority")).get("sequencer_public_key") != trust.sequencer_public_key.hex():
        raise ValueError("program sequencer authority mismatch")
    verification = verify_receipt_outcome(receipt, authority, signatures)
    protocol = verification.receipt
    outcome = protocol.program_outcome
    authority_document = _mapping(execution.get("authority"))
    if protocol.module_id != 9 or protocol.operation != 3 or protocol.module_version != module_version or protocol.activity_id.hex() != activity_id or outcome is None or outcome.abi_version != guest_abi or outcome.result_code != result_code or protocol.batch_id.hex() != execution.get("batch_id") or execution.get("batch_id") != authority_document.get("batch_id") or str(protocol.global_sequence) != execution.get("global_sequence") or protocol.previous_state_root.hex() != authority_document.get("previous_state_root") or protocol.resulting_state_root.hex() != execution.get("state_root") or execution.get("state_root") != authority_document.get("resulting_state_root") or verification.receipt_digest.hex() != execution.get("receipt_digest") or not call_graph or sha256(terminal_payload).digest() != outcome.terminal_payload_root or sha256(call_graph).digest() != outcome.call_graph_root:
        raise ValueError("program receipt binding failed")
    terminal = decode_and_verify_program_terminal(terminal_payload, call_graph, cast(str, execution["program_id"]), outcome, protocol.protocol_version)
    if terminal.usage != execution.get("usage") or terminal.outcome != execution.get("outcome"):
        raise ValueError("program terminal document binding failed")
    return VerifiedProgramReceipt(verification, terminal_payload, call_graph)


class ProgramOperations:
    def __init__(self, client: ProductionClient, signatures: LocalSignatureVerifier, trust: ProgramTrustContext) -> None:
        self._client = client
        self._signatures = signatures
        self._trust = trust
        self._heads: dict[str, tuple[str, int, int, int]] = {}
        self._heads_lock = RLock()

    def discover(self, program_id: str) -> ProgramDiscovery:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        result = _discovery(self._client.agent("program.discover", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}), program_id, self._trust.now_milliseconds())
        self._remember_head(program_id, _head(result))
        return result

    def interface(self, program_id: str) -> ProgramInterface:
        if not _hex32(program_id):
            raise ValueError("invalid program id")
        result = _interface(self._client.agent("program.interface", {"program_id": program_id, "requested_verification_level": "sequencer-signed"}), program_id, self._trust.now_milliseconds())
        self._remember_head(program_id, _head(result))
        return result

    def simulate(self, call: ProgramCall) -> Mapping[str, object]:
        _validate(call)
        signed = decode_signed_program_call(call)
        head = self._remembered_head(call.program_id)
        if head is None:
            raise ValueError("a fresh discovered program head is required before simulation")
        _fresh(head, self._trust.now_milliseconds())
        result = _mapping(self._client.agent("program.simulate", _wire(call)))
        self._require_current_head(call.program_id, head)
        _fresh(head, self._trust.now_milliseconds())
        _exact(result, ("committed", "execution", "simulation_evidence"))
        if result.get("committed") is not False:
            raise ValueError("committed program simulation")
        execution = _execution(result.get("execution"), "simulated")
        if execution["program_id"] != call.program_id or execution["activity_id"] != signed.activity_id:
            raise ValueError("program simulation binding failed")
        verified = verify_program_receipt(execution, _authority(execution, self._trust), self._signatures, self._trust)
        _verify_simulation(
            result.get("simulation_evidence"),
            execution,
            verified,
            head,
            signed,
            self._signatures,
            self._trust,
        )
        self._require_current_head(call.program_id, head)
        _fresh(head, self._trust.now_milliseconds())
        return result

    def _remember_head(self, program_id: str, candidate: tuple[str, int, int, int]) -> None:
        with self._heads_lock:
            current = self._heads.get(program_id)
            if current is not None and (
                candidate[1] < current[1]
                or candidate[2] < current[2]
                or (
                    candidate[1] == current[1]
                    and (candidate[0] != current[0] or candidate[3] < current[3])
                )
            ):
                raise ValueError("program head rollback or conflict")
            self._heads[program_id] = candidate

    def _remembered_head(self, program_id: str) -> tuple[str, int, int, int] | None:
        with self._heads_lock:
            return self._heads.get(program_id)

    def _require_current_head(self, program_id: str, expected: tuple[str, int, int, int]) -> None:
        with self._heads_lock:
            if self._heads.get(program_id) != expected:
                raise ValueError("program head changed during simulation")

    def submit(self, call: ProgramCall, idempotency_key: IdempotencyKey) -> Mapping[str, object]:
        _validate(call)
        if not _hex32(str(idempotency_key)):
            raise ValueError("invalid program idempotency key")
        signed = decode_signed_program_call(call, str(idempotency_key))
        retained = signed.canonical_bytes.hex()
        try:
            result = self._client.agent("program.call", _wire(call), idempotency_key=idempotency_key)
            return _submission(result, self._signatures, self._trust, program_id=call.program_id, activity_id=signed.activity_id, idempotency_key=str(idempotency_key), retained_signed_activity=retained)
        except Exception as error:
            if _definitive_service_refusal(error):
                raise
            return {"state": "unknown", "activity_id": signed.activity_id, "idempotency_key": str(idempotency_key), "retained_signed_activity": retained}

    def receipt(self, idempotency_key: str, expected_activity_id: str) -> Mapping[str, object]:
        if not _hex32(idempotency_key) or not _hex32(expected_activity_id):
            raise ValueError("invalid program receipt selector")
        result = self._client.agent("program.receipt", {"idempotency_key": idempotency_key, "expected_activity_id": expected_activity_id, "requested_verification_level": "sequencer-signed"})
        return _submission(result, self._signatures, self._trust, activity_id=expected_activity_id, idempotency_key=idempotency_key)

    def activity(self, activity_id: str) -> Mapping[str, object]:
        if not _hex32(activity_id):
            raise ValueError("invalid activity id")
        result = self._client.agent("program.activity", {"activity_id": activity_id, "requested_verification_level": "sequencer-signed"})
        return _submission(result, self._signatures, self._trust, activity_id=activity_id)


def _submission(
    value: object,
    signatures: LocalSignatureVerifier,
    trust: ProgramTrustContext,
    *,
    program_id: str | None = None,
    activity_id: str | None = None,
    idempotency_key: str | None = None,
    retained_signed_activity: str | None = None,
) -> Mapping[str, object]:
    result = _mapping(value)
    state = result.get("state")
    if state == "unknown":
        _exact(result, ("state", "activity_id", "idempotency_key"), ("retained_signed_activity",))
        actual_activity = _hex_field(result, "activity_id", 32, exact=True)
        actual_idempotency = _hex_field(result, "idempotency_key", 32, exact=True)
        retained = None if result.get("retained_signed_activity") is None else _hex_field(result, "retained_signed_activity", _MAX_CALLDATA)
        if (activity_id is not None and actual_activity != activity_id) or (idempotency_key is not None and actual_idempotency != idempotency_key) or (retained_signed_activity is not None and retained is not None and retained != retained_signed_activity):
            raise ValueError("program unknown binding failed")
        bound: dict[str, object] = {"state": "unknown", "activity_id": actual_activity, "idempotency_key": actual_idempotency}
        if retained is not None or retained_signed_activity is not None:
            bound["retained_signed_activity"] = retained if retained is not None else retained_signed_activity
        return bound
    if state not in {"executed", "refused"}:
        raise ValueError("invalid program submission state")
    execution = _execution(result, cast(Literal["executed", "refused"], state))
    if (program_id is not None and execution["program_id"] != program_id) or (activity_id is not None and execution["activity_id"] != activity_id) or (idempotency_key is not None and execution.get("idempotency_key") != idempotency_key):
        raise ValueError("program execution binding failed")
    verify_program_receipt(execution, _authority(execution, trust), signatures, trust)
    return execution


def _execution(value: object, expected_state: Literal["executed", "refused", "simulated"]) -> Mapping[str, object]:
    execution = _mapping(value)
    _exact(execution, ("state", "activity_id", "program_id", "guest_abi_version", "module_version", "batch_id",
        "global_sequence", "result_code", "state_root", "receipt", "receipt_digest", "terminal_payload",
        "call_graph", "authority", "usage", "outcome", "verification"), ("idempotency_key",))
    if execution.get("state") != expected_state:
        raise ValueError("invalid program execution state")
    _hex_field(execution, "activity_id", 32, exact=True)
    _hex_field(execution, "program_id", 32, exact=True)
    _hex_field(execution, "batch_id", 32, exact=True)
    _hex_field(execution, "state_root", 32, exact=True)
    _hex_field(execution, "receipt", _MAX_CALLDATA)
    _hex_field(execution, "receipt_digest", 32, exact=True)
    _hex_field(execution, "terminal_payload", _MAX_CALLDATA)
    _hex_field(execution, "call_graph", _MAX_CALLDATA)
    for field in ("global_sequence",):
        _decimal(execution.get(field), (1 << 64) - 1)
    if not isinstance(execution.get("module_version"), int) or isinstance(execution.get("module_version"), bool) or execution["module_version"] not in (1, 2, 3) or not isinstance(execution.get("guest_abi_version"), int) or isinstance(execution.get("guest_abi_version"), bool) or execution.get("guest_abi_version") not in (1, 2) or not isinstance(execution.get("result_code"), int) or isinstance(execution.get("result_code"), bool):
        raise ValueError("invalid program execution metadata")
    if execution.get("verification") != "receipt-terminal-and-call-graph-verified":
        raise ValueError("invalid program verification status")
    outcome = _program_outcome(execution.get("outcome"))
    if (expected_state == "refused" and outcome.get("kind") != "refused") or (expected_state == "executed" and outcome.get("kind") == "refused"):
        raise ValueError("program state/outcome mismatch")
    usage = _mapping(execution.get("usage"))
    _exact(usage, ("cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes", "output_values", "output_bytes", "fee_units"))
    for field in ("cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes", "output_bytes"):
        _decimal(usage.get(field), (1 << 64) - 1)
    _decimal(usage.get("fee_units"), _MAX_U128)
    if not isinstance(usage.get("output_values"), int) or isinstance(usage.get("output_values"), bool) or not 0 <= cast(int, usage["output_values"]) <= (1 << 32) - 1:
        raise ValueError("invalid program usage")
    authority = _mapping(execution.get("authority"))
    _exact(authority, ("batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key"))
    for field in ("batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key"):
        _hex_field(authority, field, 32, exact=True)
    result = dict(execution)
    result["outcome"] = outcome
    result["usage"] = dict(usage)
    result["authority"] = dict(authority)
    return result


def _authority(execution: Mapping[str, object], trust: ProgramTrustContext) -> AuthorizedReceiptBatch:
    authority = _mapping(execution.get("authority"))
    if authority.get("sequencer_public_key") != trust.sequencer_public_key.hex():
        raise ValueError("program sequencer key does not match pin")
    return AuthorizedReceiptBatch(
        batch_id=bytes.fromhex(_hex_field(authority, "batch_id", 32, exact=True)),
        asset=bytes.fromhex(_hex_field(authority, "asset", 32, exact=True)),
        previous_state_root=bytes.fromhex(_hex_field(authority, "previous_state_root", 32, exact=True)),
        resulting_state_root=bytes.fromhex(_hex_field(authority, "resulting_state_root", 32, exact=True)),
        sequencer_public_key=trust.sequencer_public_key,
    )


def _verify_simulation(
    value: object,
    execution: Mapping[str, object],
    verified: VerifiedProgramReceipt,
    head: tuple[str, int, int, int],
    binding: DecodedSignedProgramCall,
    signatures: LocalSignatureVerifier,
    trust: ProgramTrustContext,
) -> None:
    evidence = _mapping(value)
    _exact(evidence, ("boundary_id", "activity_id", "previous_state_root", "hypothetical_state_root",
        "observed_sequence", "observed_at", "committed", "public_key", "signature"))
    boundary_id = bytes.fromhex(_hex_field(evidence, "boundary_id", 32, exact=True))
    public_key = bytes.fromhex(_hex_field(evidence, "public_key", 32, exact=True))
    if public_key != trust.sequencer_public_key:
        raise ValueError("simulation sequencer key does not match pin")
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
    state_root, head_sequence, head_observed_at, valid_through = head
    protocol = verified.verification.receipt
    now = trust.now_milliseconds()
    _fresh(head, now)
    assert_fresh_simulation_observation(
        observed_at,
        binding,
        now,
        trust.maximum_simulation_age_milliseconds,
    )
    if previous.hex() != state_root or sequence != head_sequence or protocol.previous_state_root.hex() != state_root or protocol.global_sequence != sequence + 1 or observed_at < head_observed_at or observed_at > valid_through or observed_at > now:
        raise ValueError("stale or mismatched simulation head")


def _discovery(value: object, program_id: str, now: int) -> ProgramDiscovery:
    result = _mapping(value)
    _exact(result, ("program_id", "lifecycle", "version", "code_hash", "abi_version", "receipt_digest",
        "state_root", "observed_sequence", "observed_at", "valid_through", "verification"))
    if _hex_field(result, "program_id", 32, exact=True) != program_id or result.get("verification") != "registry-receipt-and-current-head-verified":
        raise ValueError("unverified program discovery")
    if result.get("lifecycle") not in ("active", "deprecated", "tombstoned") or not _integer(result.get("version"), 1, (1 << 32) - 1) or not _integer(result.get("abi_version"), 1, 2):
        raise ValueError("invalid program discovery")
    for field in ("code_hash", "receipt_digest", "state_root"):
        _hex_field(result, field, 32, exact=True)
    for field in ("observed_sequence", "observed_at", "valid_through"):
        _decimal(result.get(field), (1 << 64) - 1)
    documented = ProgramDiscovery(program_id, cast(ProgramLifecycle, result["lifecycle"]), cast(int, result["version"]),
        cast(str, result["code_hash"]), cast(int, result["abi_version"]), cast(str, result["receipt_digest"]),
        cast(str, result["state_root"]), _decimal(result["observed_sequence"], _MAX_U64),
        _decimal(result["observed_at"], _MAX_U64), _decimal(result["valid_through"], _MAX_U64))
    _fresh(_head(documented), now)
    return documented


def _interface(value: object, program_id: str, now: int) -> ProgramInterface:
    result = _mapping(value)
    _exact(result, ("program_id", "version", "code_hash", "abi_version", "interface", "interface_digest",
        "receipt_digest", "state_root", "observed_sequence", "observed_at", "valid_through", "source", "verification"))
    if _hex_field(result, "program_id", 32, exact=True) != program_id or result.get("verification") != "deployment-interface-and-current-head-verified":
        raise ValueError("unverified program interface")
    if not _integer(result.get("version"), 1, (1 << 32) - 1) or not _integer(result.get("abi_version"), 1, 2):
        raise ValueError("invalid program interface")
    for field in ("code_hash", "interface_digest", "receipt_digest", "state_root"):
        _hex_field(result, field, 32, exact=True)
    interface = bytes.fromhex(_hex_field(result, "interface", 952))
    if not interface or sha256(interface).hexdigest() != result.get("interface_digest"):
        raise ValueError("program interface digest mismatch")
    source = _program_source(result.get("source"))
    for field in ("observed_sequence", "observed_at", "valid_through"):
        _decimal(result.get(field), (1 << 64) - 1)
    documented = ProgramInterface(program_id, cast(int, result["version"]), cast(str, result["code_hash"]),
        cast(int, result["abi_version"]), interface, cast(str, result["interface_digest"]),
        cast(str, result["receipt_digest"]), cast(str, result["state_root"]),
        _decimal(result["observed_sequence"], _MAX_U64), _decimal(result["observed_at"], _MAX_U64),
        _decimal(result["valid_through"], _MAX_U64), _typed_program_source(source))
    _fresh(_head(documented), now)
    return documented


def _mapping(value: object) -> Mapping[str, object]:
    if not isinstance(value, Mapping) or any(not isinstance(key, str) for key in value):
        raise ValueError("invalid program document")
    return cast(Mapping[str, object], value)


def _exact(value: Mapping[str, object], required: tuple[str, ...], optional: tuple[str, ...] = ()) -> None:
    if any(field not in value for field in required) or any(field not in required and field not in optional for field in value):
        raise ValueError("invalid program document fields")


def _integer(value: object, minimum: int, maximum: int) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and minimum <= value <= maximum


def _program_source(value: object) -> Mapping[str, object]:
    source = _mapping(value)
    status = source.get("status")
    if status == "unpublished":
        _exact(source, ("status",))
        return {"status": status}
    if status == "verified":
        _exact(source, ("status", "source_digest", "environment_digest", "pipeline"))
        if source.get("pipeline") != "sha256-source-artifact-reproducible-build-v1":
            raise ValueError("invalid program source pipeline")
        return {"status": status, "source_digest": _hex_field(source, "source_digest", 32, exact=True),
            "environment_digest": _hex_field(source, "environment_digest", 32, exact=True), "pipeline": source["pipeline"]}
    if status == "mismatch":
        _exact(source, ("status", "expected_code_hash", "reproduced_artifact_digest"))
        return {"status": status, "expected_code_hash": _hex_field(source, "expected_code_hash", 32, exact=True),
            "reproduced_artifact_digest": _hex_field(source, "reproduced_artifact_digest", 32, exact=True)}
    raise ValueError("invalid program source status")


def _program_outcome(value: object) -> Mapping[str, object]:
    outcome = _mapping(value)
    kind = outcome.get("kind")
    if kind == "completed":
        _exact(outcome, ("kind", "code", "response"))
        if not _integer(outcome.get("code"), 0, (1 << 31) - 1):
            raise ValueError("invalid completed program outcome")
        return {"kind": kind, "code": outcome["code"], "response": _hex_field(outcome, "response", _MAX_CALLDATA)}
    if kind == "legacy_completed":
        _exact(outcome, ("kind", "code", "values"))
        values = outcome.get("values")
        if not _integer(outcome.get("code"), -(1 << 31), (1 << 31) - 1) or not isinstance(values, list) or len(values) > _MAX_CALLDATA // 5:
            raise ValueError("invalid legacy program outcome")
        decoded: list[Mapping[str, object]] = []
        for raw in values:
            item = _mapping(raw); _exact(item, ("type", "value"))
            if item.get("type") == "i32" and _integer(item.get("value"), -(1 << 31), (1 << 31) - 1):
                decoded.append({"type": "i32", "value": item["value"]})
            elif item.get("type") == "i64" and isinstance(item.get("value"), str):
                text = cast(str, item["value"])
                if _signed_decimal(text) and -(1 << 63) <= int(text) <= (1 << 63) - 1:
                    decoded.append({"type": "i64", "value": text})
                else:
                    raise ValueError("invalid legacy program value")
            else:
                raise ValueError("invalid legacy program value")
        return {"kind": kind, "code": outcome["code"], "values": tuple(decoded)}
    if kind == "refused":
        _exact(outcome, ("kind", "failure"))
        return {"kind": kind, "failure": _program_failure(outcome.get("failure"))}
    raise ValueError("invalid program outcome")


def _program_failure(value: object) -> Mapping[str, object]:
    failure = _mapping(value); kind = failure.get("kind")
    if kind in ("depth_exceeded", "fanout_exceeded"):
        _exact(failure, ("kind", "limit", "attempted"))
        if not _integer(failure.get("limit"), 0, (1 << 32) - 1) or not _integer(failure.get("attempted"), 0, (1 << 32) - 1):
            raise ValueError("invalid program failure bounds")
        return dict(failure)
    if kind == "guest_refused":
        _exact(failure, ("kind", "code"))
        if not _integer(failure.get("code"), -(1 << 31), (1 << 31) - 1):
            raise ValueError("invalid program failure code")
        return dict(failure)
    if kind in ("unknown_program", "reentrancy", "authority", "resource", "response", "fault"):
        _exact(failure, ("kind",))
        return {"kind": kind}
    raise ValueError("invalid program failure")


def _signed_decimal(value: str) -> bool:
    if not value.isascii() or not value:
        return False
    digits = value[1:] if value.startswith("-") else value
    return bool(digits) and digits.isdigit() and (digits == "0" or not digits.startswith("0")) and value != "-0"


def _typed_program_source(value: Mapping[str, object]) -> ProgramSource:
    return ProgramSource(status=cast(ProgramSourceStatus, value["status"]),
        source_digest=cast(str | None, value.get("source_digest")),
        environment_digest=cast(str | None, value.get("environment_digest")),
        pipeline=cast(str | None, value.get("pipeline")),
        expected_code_hash=cast(str | None, value.get("expected_code_hash")),
        reproduced_artifact_digest=cast(str | None, value.get("reproduced_artifact_digest")))


def _head(value: ProgramDiscovery | ProgramInterface) -> tuple[str, int, int, int]:
    return (value.state_root, value.observed_sequence, value.observed_at, value.valid_through)


def _fresh(value: tuple[str, int, int, int], now: int) -> None:
    _, _, observed_at, valid_through = value
    if observed_at > now or now > valid_through or valid_through < observed_at:
        raise ValueError("stale program head")


def _definitive_service_refusal(error: BaseException) -> bool:
    return isinstance(error, PlatformSdkError) and error.retry == "never" and error.code in {
        SdkErrorCode.INVALID_ARGUMENT, SdkErrorCode.IDEMPOTENCY_REQUIRED, SdkErrorCode.PROTOCOL_INCOMPATIBILITY,
        SdkErrorCode.UNAVAILABLE_CAPABILITY, SdkErrorCode.CORE_REJECTION, SdkErrorCode.POLICY_REFUSAL,
        SdkErrorCode.CAPABILITY_REFUSAL, SdkErrorCode.BUDGET_REFUSAL, SdkErrorCode.IDEMPOTENCY_CONFLICT,
    }


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
