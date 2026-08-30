from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256
from typing import Literal, Mapping, cast

from .verifier import ProgramReceiptOutcome

_ACTIVITY_DOMAIN = b"LXP/v1/activity-id\0"
_PAYLOAD_DOMAIN = b"LXP/v1/payload-hash\0"
_CALL_DOMAIN = b"LayerX/programs/call/v1\0"
_EXECUTION_V2 = b"LXP/program-execution/v2\0"
_EXECUTION_V3 = b"LXP/program-execution/v3\0"
_EXECUTION_V4 = b"LXP/program-execution/v4\0"
_OCCUPANCY = b"LXP/program-execution-with-occupancy/v1\0"
_AUTHORITY = b"LXP/program-execution-with-transfer-authority/v2\0"
_FAILURE = b"LXP/programs/failure-detail/v1\0"
_RESOURCE = b"LXP/programs/resource-detail/v1\0"
_SETTLEMENT = b"LXP/programs/settlement-failure/v1\0"
_CALLBACK = b"LXP/programs/callback-failure/v1\0"
_TRANSFER_SET_V1 = b"LayerX/programs/402LXP/transfer-set/v1\0"
_TRANSFER_SET_V2 = b"LayerX/programs/402LXP/transfer-set/v2\0"
_PROGRAM_AUTHORITY = b"LayerX/programs/402LXP/program-authority/v1\0"
_PROGRAM_FUNDING = b"LayerX/programs/402LXP/program-funding/v1\0"
_PROGRAM_ACCOUNT = b"LayerX/programs/program-account/v1\0"
_EVENT_ENVELOPE = b"LayerX/programs/events/v1\0"
_OCCUPANCY_V1 = b"LXP/storage-occupancy-settlement/v1\0"
_OCCUPANCY_V2 = b"LXP/storage-occupancy-settlement/v2\0"
_OCCUPANCY_V3 = b"LXP/storage-occupancy-settlement/v3\0"
_OCCUPANCY_MANDATE = b"LXP/storage-occupancy-mandate/v1\0"
_MERKLE_LEAF = b"LXP/v1/merkle-leaf\0"
_MERKLE_INTERNAL = b"LXP/v1/merkle-internal\0"
_ACCOUNT_DERIVATION = b"LX:ACCOUNT:v1"
_FEE_TREASURY_LABEL = b"system:fees"
_MAX_U128 = (1 << 128) - 1
_CAPABILITIES: Mapping[str, int] = {"storage_read": 1, "storage_write": 2, "transfer": 3, "emit_event": 4, "compose": 5}
_MAX_TRACE = 34 + 65_536 * 52
_MAX_GRAPH = len(b"LayerX/programs/call-graph/v1\0") + 32 + 16 + 8 + 64 * 68


@dataclass(frozen=True)
class DecodedSignedProgramCall:
    activity_id: str
    idempotency_key: str
    not_before: int
    not_after: int
    canonical_bytes: bytes


@dataclass(frozen=True)
class DecodedProgramTerminal:
    outcome: Mapping[str, object]
    usage: Mapping[str, object]


def decode_signed_program_call(call: object, expected_idempotency_key: str | None = None) -> DecodedSignedProgramCall:
    canonical = bytes(getattr(call, "signed_activity"))
    reader = _Reader(canonical)
    if reader.u16() != 1 or reader.u16() != 0x1001 or reader.byte() != 12:
        _fail("signed activity header")
    _field(reader, 1)
    if reader.u16() not in (1, 2):
        _fail("signed activity protocol")
    _field(reader, 2); reader.u32()
    _field(reader, 3)
    if reader.u32() != 0x0009_0003:
        _fail("signed activity type")
    _field(reader, 4); actor = reader.sized_u32(255)
    _field(reader, 5); authority = reader.sized_u32(524_288)
    _field(reader, 6); reader.u64()
    _field(reader, 7); not_before = reader.u64(); not_after = reader.u64()
    _field(reader, 8); idempotency = reader.sized_u32(32, 32)
    _field(reader, 9); reader.u128()
    _field(reader, 10); payload_hash = reader.sized_u32(32, 32)
    _field(reader, 11); payload = reader.sized_u32(524_288)
    _field(reader, 12); signature = reader.sized_u32(128)
    reader.end()
    if not_after < not_before:
        _fail("signed activity bounds")
    if payload_hash != sha256(_PAYLOAD_DOMAIN + payload).digest():
        _fail("signed activity payload hash")
    _decode_call_payload(payload, call)
    key = idempotency.hex()
    if expected_idempotency_key is not None and key != expected_idempotency_key:
        _fail("signed activity idempotency")
    return DecodedSignedProgramCall(
        sha256(_ACTIVITY_DOMAIN + canonical).hexdigest(),
        key,
        not_before,
        not_after,
        canonical,
    )


def assert_fresh_simulation_observation(
    observed_at: int,
    binding: DecodedSignedProgramCall,
    now: int,
    maximum_age_milliseconds: int,
) -> None:
    if (
        isinstance(observed_at, bool)
        or not isinstance(observed_at, int)
        or isinstance(now, bool)
        or not isinstance(now, int)
        or isinstance(maximum_age_milliseconds, bool)
        or not isinstance(maximum_age_milliseconds, int)
        or not 0 < maximum_age_milliseconds <= (1 << 64) - 1
        or observed_at < binding.not_before
        or observed_at > binding.not_after
        or observed_at > now
        or now - observed_at > maximum_age_milliseconds
    ):
        _fail("simulation observation bounds")


def decode_and_verify_program_terminal(
    terminal_payload: bytes,
    call_graph: bytes,
    expected_program_id: str,
    receipt: ProgramReceiptOutcome,
    protocol_version: int,
) -> DecodedProgramTerminal:
    if not call_graph or sha256(call_graph).digest() != receipt.call_graph_root:
        _fail("program call graph root")
    inner = terminal_payload
    authorization: bytes | None = None
    authority_root: bytes | None = None
    occupancy: bytes | None = None
    if inner.startswith(_AUTHORITY):
        wrapper = _Reader(inner[len(_AUTHORITY):])
        inner = wrapper.sized_u32(1_048_576)
        authorization = wrapper.sized_u32(1_048_576)
        authority_root = wrapper.fixed(32)
        wrapper.end()
    if inner.startswith(_OCCUPANCY):
        wrapper = _Reader(inner[len(_OCCUPANCY):])
        inner = wrapper.sized_u32(1_048_576)
        occupancy = wrapper.sized_u32(65_536)
        wrapper.end()
    if inner.startswith(_AUTHORITY) or inner.startswith(_OCCUPANCY):
        _fail("program terminal wrapper order")

    usage: Mapping[str, object] | None = None
    candidate = False
    successful = False
    if inner.startswith(_EXECUTION_V2) or inner.startswith(_EXECUTION_V3):
        if receipt.terminal_kind != 1 or receipt.abi_version != 1:
            _fail("legacy terminal kind")
        traced = inner.startswith(_EXECUTION_V3)
        decoded = _decode_legacy(inner[len(_EXECUTION_V3 if traced else _EXECUTION_V2):], traced)
        _bind_metadata(decoded, receipt)
        outcome: Mapping[str, object] = {"kind": "legacy_completed", "code": receipt.result_code, "values": decoded["values"]}
        usage = cast(Mapping[str, object], decoded["usage"])
        successful = True
    elif inner.startswith(_EXECUTION_V4):
        candidate = True
        decoded = _decode_candidate(inner[len(_EXECUTION_V4):])
        if decoded["kind"] != receipt.terminal_kind or receipt.abi_version != 2 or decoded["program"] != expected_program_id:
            _fail("candidate terminal binding")
        _bind_metadata(decoded, receipt)
        if decoded["graph"] != call_graph:
            _fail("candidate call graph")
        if decoded["outcome"] == "success":
            outcome = {"kind": "completed", "code": decoded["code"], "response": cast(bytes, decoded["response"]).hex()}
            successful = True
        elif decoded["outcome"] == "failure":
            outcome = {"kind": "refused", "failure": {"kind": "guest_refused", "code": receipt.result_code}}
        else:
            outcome = {"kind": "refused", "failure": {"kind": "resource"}}
        usage = cast(Mapping[str, object], decoded["usage"])
    elif inner.startswith(_FAILURE):
        if receipt.terminal_kind != 2:
            _fail("failure terminal kind")
        _decode_failure(inner[len(_FAILURE):])
        outcome = {"kind": "refused", "failure": {"kind": "guest_refused", "code": receipt.result_code}}
    elif inner.startswith(_RESOURCE):
        if receipt.terminal_kind != 3:
            _fail("resource terminal kind")
        reader = _Reader(inner[len(_RESOURCE):]); _decode_resource(reader, False); reader.end()
        outcome = {"kind": "refused", "failure": {"kind": "resource"}}
    elif inner.startswith(_SETTLEMENT):
        if receipt.terminal_kind != 2 or len(inner) != len(_SETTLEMENT) + 1 or inner[-1] not in range(1, 13):
            _fail("settlement terminal")
        outcome = {"kind": "refused", "failure": {"kind": "guest_refused", "code": receipt.result_code}}
    elif inner.startswith(_CALLBACK):
        if receipt.terminal_kind != 2 or len(inner) != len(_CALLBACK) + 5:
            _fail("callback terminal")
        outcome = {"kind": "refused", "failure": {"kind": "guest_refused", "code": receipt.result_code}}
    else:
        _fail("unknown terminal domain")

    occupancy_required = protocol_version == 2 and successful
    if (occupancy is not None) != occupancy_required:
        _fail("occupancy attachment presence")
    if occupancy is not None:
        if not occupancy:
            if receipt.occupancy_evidence_digest != bytes(32) or receipt.occupancy_transfer_root != bytes(32) or receipt.occupancy_byte_batches or receipt.occupancy_fee_units:
                _fail("empty occupancy attachment")
        else:
            if sha256(occupancy).digest() != receipt.occupancy_evidence_digest:
                _fail("occupancy evidence digest")
            settlement = _decode_occupancy_settlement(occupancy)
            if (settlement["byte_batches"] != receipt.occupancy_byte_batches
                    or settlement["fee_units"] != receipt.occupancy_fee_units
                    or _occupancy_transfer_root(settlement, receipt.occupancy_asset_id) != receipt.occupancy_transfer_root):
                _fail("occupancy receipt binding")
    elif receipt.occupancy_evidence_digest != bytes(32) or receipt.occupancy_transfer_root != bytes(32) or receipt.occupancy_byte_batches or receipt.occupancy_fee_units:
        _fail("unexpected occupancy commitment")
    transfer_present = receipt.transfer_root != bytes(32)
    if ((authorization is not None) != transfer_present if candidate else authorization is not None):
        _fail("transfer authority presence")
    if authorization is not None:
        if not authorization or authority_root != receipt.transfer_root:
            _fail("transfer authority root")
        _verify_authorization_root(authorization, cast(bytes, authority_root))
    if protocol_version not in (1, 2):
        _fail("program receipt protocol")
    return DecodedProgramTerminal(outcome, usage if usage is not None else _receipt_usage(receipt))


def _decode_call_payload(payload: bytes, call: object) -> None:
    reader = _Reader(payload)
    if reader.fixed(len(_CALL_DOMAIN)) != _CALL_DOMAIN:
        _fail("program call domain")
    if reader.fixed(32).hex() != getattr(call, "program_id") or reader.u64() != getattr(call, "fuel") or reader.u128() != getattr(call, "fee_limit"):
        _fail("program call budget")
    capabilities = getattr(call, "capabilities")
    count = reader.u16()
    if count != len(capabilities) or count > 5:
        _fail("program call capabilities")
    prior = 0
    for capability in capabilities:
        tag = reader.byte()
        if tag != _CAPABILITIES.get(capability) or tag <= prior:
            _fail("program call capability tag")
        prior = tag
    if reader.sized_u32(1_048_576) != getattr(call, "calldata"):
        _fail("program call calldata")
    reader.end()


def _decode_legacy(encoded: bytes, traced: bool) -> dict[str, object]:
    reader = _Reader(encoded)
    runtime = reader.u16(); abi = reader.u16(); metering = reader.u32()
    if not runtime or abi != 1 or not metering:
        _fail("legacy metadata")
    count = reader.u128()
    if count > reader.remaining() // 5:
        _fail("legacy value count")
    values: list[Mapping[str, object]] = []
    for _ in range(count):
        tag = reader.byte()
        if tag == 1:
            values.append({"type": "i32", "value": reader.i32()})
        elif tag == 2:
            values.append({"type": "i64", "value": str(reader.i64())})
        else:
            _fail("legacy value tag")
    usage = _usage(reader.u64(), reader.u64(), reader.u64(), reader.u64(), reader.u32(), 0, reader.u128())
    if traced:
        if reader.byte() != 1:
            _fail("legacy trace tag")
        reader.sized_u64(_MAX_TRACE)
    reader.end()
    return {"runtime": runtime, "abi": 1, "fee": 0, "metering": metering, "usage": usage, "values": tuple(values)}


def _decode_candidate(encoded: bytes) -> dict[str, object]:
    reader = _Reader(encoded)
    runtime = reader.u16(); fee = reader.u32(); metering = reader.u32()
    if not runtime or not fee or not metering:
        _fail("candidate metadata")
    count = reader.u64()
    if count > reader.remaining() // 5:
        _fail("candidate value count")
    for _ in range(count):
        tag = reader.byte()
        if tag == 1: reader.i32()
        elif tag == 2: reader.i64()
        else: _fail("candidate value tag")
    usage = _usage(reader.u64(), reader.u64(), reader.u64(), reader.u64(), reader.u32(), reader.u64(), reader.u128())
    trace = reader.byte()
    if trace == 1: reader.sized_u64(_MAX_TRACE)
    elif trace != 0: _fail("candidate trace tag")
    program = reader.fixed(32).hex()
    if reader.u16() != 2:
        _fail("candidate ABI")
    tag = reader.byte()
    result: dict[str, object] = {"runtime": runtime, "abi": 2, "fee": fee, "metering": metering, "usage": usage, "program": program}
    if tag == 0:
        code = reader.i32()
        if code < 0: _fail("candidate result code")
        result.update({"kind": 1, "outcome": "success", "code": code, "response": reader.sized_u64(1_048_576)})
    elif tag == 1:
        _decode_program_failure(reader.sized_u64(4_136)); result.update({"kind": 2, "outcome": "failure"})
    elif tag == 2:
        _decode_resource(reader, True, usage); result.update({"kind": 3, "outcome": "resource"})
    else:
        _fail("candidate outcome tag")
    result["graph"] = reader.sized_u64(_MAX_GRAPH)
    reader.end()
    return result


def _decode_failure(encoded: bytes) -> None:
    reader = _Reader(encoded); tag = reader.byte(); payload = _Reader(reader.sized_u32(1_048_576)); reader.end()
    if tag == 1: _decode_program_failure(payload.rest())
    elif tag == 2: _decode_composition(payload)
    elif tag == 3: _decode_entrypoint(payload)
    elif tag == 4: _decode_abi(payload)
    else: _fail("failure terminal tag")
    payload.end()


def _decode_composition(reader: _Reader) -> None:
    tag = reader.byte()
    if tag in (1, 9, 10, 11, 20, 21, 22): return
    if tag == 2:
        if reader.byte() not in (1, 2) or reader.byte() not in (1, 2): _fail("composition revision")
    elif tag == 23: reader.fixed(76); reader.fixed(76)
    elif tag in (3, 4): reader.fixed(32)
    elif tag in (5, 6, 7): reader.u32(); reader.u32()
    elif tag == 8: reader.fixed(32); reader.u32(); reader.u32()
    elif tag == 12: reader.i32()
    elif tag == 13: reader.u64(); reader.u64()
    elif tag == 14: reader.fixed(32); reader.i32()
    elif tag == 15: _decode_program_failure(reader.rest())
    elif tag == 16: _decode_abi(reader)
    elif tag == 17: _decode_fault(reader)
    elif tag == 18: _decode_meter_failure(reader)
    elif tag == 19: _decode_response(reader)
    else: _fail("composition failure tag")


def _decode_entrypoint(reader: _Reader) -> None:
    tag = reader.byte()
    if tag == 1: reader.u64(); reader.u64()
    elif tag in (2, 3, 4): return
    elif tag in (5, 6): reader.i32()
    elif tag == 7: _decode_fault(reader)
    elif tag == 8: _decode_meter_failure(reader)
    else: _fail("entrypoint failure tag")


def _decode_abi(reader: _Reader) -> None:
    tag = reader.byte()
    if tag in (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 14, 15): return
    if tag == 11:
        if reader.byte() not in range(1, 12): _fail("storage failure tag")
    elif tag == 12: _decode_meter_failure(reader)
    else: _fail("ABI failure tag")


def _decode_meter_failure(reader: _Reader) -> None:
    tag = reader.byte()
    if tag == 1:
        resource = reader.byte(); limit = reader.u64(); attempted = reader.u64()
        if resource not in range(1, 8) or attempted <= limit: _fail("meter budget failure")
    elif tag == 2:
        if reader.byte() not in range(1, 8): _fail("meter counter failure")
    elif tag != 3: _fail("meter failure tag")


def _decode_fault(reader: _Reader) -> None:
    tag = reader.byte()
    if tag in (1, 2, 16): reader.sized_u32(1_048_576).decode("utf-8", "strict")
    elif tag in (3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15): return
    elif tag == 14: _decode_meter_failure(reader)
    else: _fail("execution fault tag")


def _decode_response(reader: _Reader) -> None:
    tag = reader.byte()
    if tag in (1, 2): reader.u64(); reader.u64()
    elif tag in (3, 4): return
    elif tag == 5: reader.i32(); reader.i32()
    elif tag == 6: _decode_meter_failure(reader)
    else: _fail("response failure tag")


def _decode_program_failure(encoded: bytes) -> None:
    reader = _Reader(encoded); program = reader.fixed(32); refusal = reader.u32(); reason = reader.sized_u32(4_096); reader.end()
    if program == bytes(32) or refusal not in (1, 2, 3, 4, 5, 254, 255) or (refusal in (254, 255) and reason):
        _fail("program failure payload")


def _decode_resource(reader: _Reader, candidate: bool, usage: Mapping[str, object] | None = None) -> None:
    tag = reader.byte(); resource = reader.byte()
    if (candidate and resource not in range(0, 7)) or (not candidate and resource not in range(1, 8)):
        _fail("resource kind")
    if tag == (0 if candidate else 1):
        limit = reader.u64(); attempted = reader.u64()
        if attempted <= limit or (candidate and usage is not None and _usage_for(usage, resource) > limit):
            _fail("resource refusal bounds")
    elif tag != (1 if candidate else 2):
        _fail("resource refusal tag")


def _usage_for(usage: Mapping[str, object], resource: int) -> int:
    fields = ("cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes", "output_values", "output_bytes")
    return int(usage[fields[resource]]) if resource < len(fields) else 0


def _bind_metadata(decoded: Mapping[str, object], receipt: ProgramReceiptOutcome) -> None:
    if decoded["runtime"] != receipt.runtime_version or decoded["abi"] != receipt.abi_version or decoded["fee"] != receipt.fee_schedule_version or decoded["metering"] != receipt.metering_schedule_version or decoded["usage"] != _receipt_usage(receipt):
        _fail("terminal receipt metadata")


def _receipt_usage(receipt: ProgramReceiptOutcome) -> Mapping[str, object]:
    return _usage(receipt.cpu_fuel, receipt.memory_bytes, receipt.storage_read_bytes, receipt.storage_write_bytes, receipt.output_values, receipt.output_bytes, receipt.fee_units)


def _usage(cpu: int, memory: int, read: int, write: int, values: int, output: int, fee: int) -> Mapping[str, object]:
    return {"cpu_fuel": str(cpu), "memory_bytes": str(memory), "storage_read_bytes": str(read), "storage_write_bytes": str(write), "output_values": values, "output_bytes": str(output), "fee_units": str(fee)}


def _verify_authorization_root(encoded: bytes, expected: bytes) -> None:
    reader = _Reader(encoded)
    candidate = encoded.startswith(_TRANSFER_SET_V2)
    domain = _TRANSFER_SET_V2 if candidate else _TRANSFER_SET_V1
    if reader.fixed(len(domain)) != domain:
        _fail("transfer authorization domain")
    _nonzero(reader.fixed(32), "transfer program")
    principal = reader.fixed(32); _nonzero(principal, "transfer principal")
    _nonzero(reader.fixed(32), "transfer invocation authority")
    _decode_frame(reader)
    _decode_event_envelope(reader.fixed(reader.u32()))
    calls = reader.u64()
    if calls > 64:
        _fail("transfer call count")
    for _ in range(calls):
        _nonzero(reader.fixed(32), "transfer caller")
        _nonzero(reader.fixed(32), "transfer callee")
        _nonzero(reader.fixed(32), "transfer call principal")
        _decode_frame(reader); _decode_frame(reader)
        _decode_capability_set(reader.fixed(reader.u32()), candidate)
    leg_count = reader.u64()
    if not leg_count or leg_count > 256:
        _fail("transfer leg count")
    kernel_legs: list[bytes] = []
    total = 0
    for _ in range(leg_count):
        frame = _decode_frame(reader)
        source = principal
        authority: Mapping[str, object] | None = None
        funding: Mapping[str, bytes] | None = None
        if candidate:
            source_tag = reader.byte()
            if source_tag == 1:
                source = reader.fixed(32); _nonzero(source, "transfer principal source")
                if source != principal:
                    _fail("transfer principal authority")
            elif source_tag == 2:
                authority = _decode_program_authority(reader.sized_u32(1_048_576))
                source = cast(bytes, authority["source"])
            elif source_tag == 3:
                source = reader.fixed(32); _nonzero(source, "transfer funding principal")
                if source != principal:
                    _fail("transfer funding authority")
                funding = _decode_program_funding(reader.sized_u32(1_048_576))
            else:
                _fail("transfer source tag")
        asset = reader.fixed(32); to = reader.fixed(32); amount = reader.u128(); program = reader.fixed(32)
        _nonzero(asset, "transfer asset"); _nonzero(to, "transfer destination"); _nonzero(program, "transfer leg program")
        if not amount:
            _fail("transfer amount")
        if authority is not None and (authority["owner"] != program or authority["frame"] != frame
                or authority["asset"] != asset or authority["to"] != to or authority["amount"] != amount):
            _fail("program transfer authority")
        if funding is not None and (funding["owner"] != program or funding["destination"] != to or funding["asset"] != asset):
            _fail("program funding authority")
        total = _checked_u128_add(total, amount, "transfer total")
        kernel_legs.append(b"\0" + source + to + asset + amount.to_bytes(16, "big") + (1).to_bytes(2, "big"))
    reader.end()
    if _merkle_root(kernel_legs) != expected:
        _fail("transfer authorization root")


def _decode_program_authority(encoded: bytes) -> Mapping[str, object]:
    reader = _Reader(encoded)
    if reader.fixed(len(_PROGRAM_AUTHORITY)) != _PROGRAM_AUTHORITY:
        _fail("program authority domain")
    owner = reader.fixed(32); _nonzero(owner, "program authority owner")
    seed_length = reader.u16()
    if seed_length > 128:
        _fail("program authority seed")
    seed = reader.fixed(seed_length); source = reader.fixed(32); frame = _decode_frame(reader)
    asset = reader.fixed(32); to = reader.fixed(32); amount = reader.u128(); reader.end()
    _nonzero(asset, "program authority asset"); _nonzero(to, "program authority destination")
    if not amount or _derive_program_account(owner, seed) != source:
        _fail("program authority derivation")
    return {"owner": owner, "frame": frame, "source": source, "asset": asset, "to": to, "amount": amount}


def _decode_program_funding(encoded: bytes) -> Mapping[str, bytes]:
    reader = _Reader(encoded)
    if reader.fixed(len(_PROGRAM_FUNDING)) != _PROGRAM_FUNDING:
        _fail("program funding domain")
    owner = reader.fixed(32); _nonzero(owner, "program funding owner")
    seed_length = reader.u16()
    if seed_length > 128:
        _fail("program funding seed")
    seed = reader.fixed(seed_length); destination = reader.fixed(32); asset = reader.fixed(32); reader.end()
    _nonzero(destination, "program funding destination"); _nonzero(asset, "program funding asset")
    if _derive_program_account(owner, seed) != destination:
        _fail("program funding derivation")
    return {"owner": owner, "destination": destination, "asset": asset}


def _derive_program_account(owner: bytes, seed: bytes) -> bytes:
    return sha256(_PROGRAM_ACCOUNT + owner + len(seed).to_bytes(4, "big") + seed).digest()


def _decode_event_envelope(encoded: bytes) -> None:
    reader = _Reader(encoded)
    if reader.fixed(len(_EVENT_ENVELOPE)) != _EVENT_ENVELOPE:
        _fail("program event domain")
    count = reader.u32()
    if count > 64:
        _fail("program event count")
    for _ in range(count):
        _nonzero(reader.fixed(32), "event program"); _nonzero(reader.fixed(32), "event principal")
        _decode_frame(reader); reader.sized_u32(64); reader.sized_u32(65_536)
    reader.end()


def _decode_frame(reader: _Reader) -> bytes:
    path = reader.fixed(8); depth = reader.byte()
    if depth > 8 or any(value == 0 for value in path[:depth]) or any(path[depth:]):
        _fail("call frame")
    return path + bytes((depth,))


def _decode_capability_set(encoded: bytes, candidate: bool) -> None:
    if len(encoded) < 2 or len(encoded) > 65_535:
        _fail("capability encoding length")
    reader = _Reader(encoded); count = reader.u16()
    if count > 269:
        _fail("capability count")
    prior: tuple[int, tuple[bytes, ...]] | None = None
    balance_views = 0
    for _ in range(count):
        tag = reader.byte()
        if tag == 1: key = (0, ())
        elif tag == 2: key = (1, ())
        elif tag == 3: key = (2, ())
        elif tag == 4:
            program = reader.fixed(32); _nonzero(program, "call capability program"); key = (3, (program,))
        elif tag == 5:
            asset = reader.fixed(32); to = reader.fixed(32); maximum = reader.u128()
            _nonzero(asset, "transfer capability asset"); _nonzero(to, "transfer capability destination")
            if not maximum: _fail("transfer capability amount")
            key = (4, (asset, to))
        elif tag == 9 and candidate:
            owner = reader.fixed(32); _nonzero(owner, "program spend owner"); seed_length = reader.u16()
            if seed_length > 128: _fail("program spend seed")
            seed = reader.fixed(seed_length); source = reader.fixed(32); asset = reader.fixed(32); to = reader.fixed(32); maximum = reader.u128()
            _nonzero(asset, "program spend asset"); _nonzero(to, "program spend destination")
            if not maximum or _derive_program_account(owner, seed) != source: _fail("program spend account")
            key = (5, (owner, seed, source, asset, to))
        elif tag == 6:
            digest = reader.fixed(32); _nonzero(digest, "receipt capability"); key = (6, (digest,))
        elif tag == 10 and candidate:
            account = reader.fixed(32); asset = reader.fixed(32); digest = reader.fixed(32)
            _nonzero(account, "balance capability account"); _nonzero(asset, "balance capability asset"); _nonzero(digest, "balance capability receipt")
            balance_views += 1
            if balance_views > 32: _fail("balance capability count")
            key = (7, (account, asset))
        elif tag == 7: key = (8, ())
        elif tag == 8: key = (9, ())
        else: _fail("capability tag")
        if prior is not None and prior >= key:
            _fail("capability canonical order")
        prior = key
    reader.end()


def _decode_occupancy_settlement(encoded: bytes) -> Mapping[str, object]:
    if len(encoded) > 65_536:
        _fail("occupancy evidence length")
    if encoded.startswith(_OCCUPANCY_V1) or encoded.startswith(_OCCUPANCY_V2):
        return _decode_legacy_occupancy(encoded)
    reader = _Reader(encoded)
    if reader.fixed(len(_OCCUPANCY_V3)) != _OCCUPANCY_V3:
        _fail("occupancy evidence domain")
    batch = reader.u64(); occupancy_price = _decode_occupancy_schedule(reader, True)
    declared_units = reader.u128(); declared_fee = reader.u128(); declared_paid = reader.u128(); declared_arrears = reader.u128()
    count = reader.u32()
    if count > 256:
        _fail("occupancy position count")
    byte_batches = fee_units = paid_units = arrears_units = 0
    prior_namespace: bytes | None = None
    charges: list[Mapping[str, object]] = []
    for _ in range(count):
        namespace = _decode_storage_namespace(reader)
        canonical = cast(bytes, namespace["canonical"])
        if prior_namespace is not None and prior_namespace >= canonical:
            _fail("occupancy namespace order")
        prior_namespace = canonical
        payer = reader.fixed(32); _nonzero(payer, "occupancy payer")
        principal = namespace.get("principal")
        if principal is not None and principal != payer:
            _fail("occupancy payer scope")
        root_program = reader.fixed(32); _nonzero(root_program, "occupancy root program")
        activity = reader.fixed(32); from_batch = reader.u64(); to_batch = reader.u64()
        recorded_bytes = reader.u64(); final_bytes = reader.u64(); units = reader.u128(); price = reader.u64()
        accrued = reader.u128(); prior_arrears = reader.u128(); amount_due = reader.u128(); authorized_added = reader.u128()
        disposition = reader.byte()
        if disposition not in range(1, 6): _fail("occupancy disposition")
        arrears_after = reader.u128(); maximum_bytes = reader.u64(); maximum_price = reader.u64(); reader.u128(); mandate = reader.fixed(32)
        if to_batch < from_batch: _fail("occupancy batch interval")
        expected_units = _checked_u128_multiply(recorded_bytes, to_batch - from_batch, "occupancy units")
        expected_fee = _checked_u128_multiply(expected_units, price, "occupancy fee")
        expected_due = _checked_u128_add(prior_arrears, expected_fee, "occupancy due")
        migration = disposition == 5
        if (to_batch != batch or (not migration and price != occupancy_price) or units != expected_units or accrued != expected_fee
                or amount_due != expected_due or final_bytes > maximum_bytes or (not migration and (mandate == bytes(32) or activity == bytes(32)))
                or (migration and (price or accrued or prior_arrears or amount_due or arrears_after or mandate != bytes(32)
                    or activity != bytes(32) or root_program != namespace["program"]))
                or ((disposition == 4) != (price > maximum_price)) or (disposition == 1 and arrears_after)
                or (disposition != 1 and arrears_after != amount_due)):
            _fail("occupancy charge semantics")
        if authorized_added:
            expected_mandate = sha256(b"".join((_OCCUPANCY_MANDATE, payer, root_program, activity, cast(bytes, namespace["wire"]),
                maximum_bytes.to_bytes(8, "big"), maximum_price.to_bytes(8, "big"), authorized_added.to_bytes(16, "big")))).digest()
            if mandate != expected_mandate:
                _fail("occupancy mandate")
        byte_batches = _checked_u128_add(byte_batches, units, "occupancy usage")
        fee_units = _checked_u128_add(fee_units, accrued, "occupancy fees")
        if disposition == 1: paid_units = _checked_u128_add(paid_units, amount_due, "occupancy paid")
        else: arrears_units = _checked_u128_add(arrears_units, arrears_after, "occupancy arrears")
        charges.append({"payer": payer, "amount_due": amount_due, "paid": disposition == 1, "arrears_after": arrears_after})
    reader.end()
    if (byte_batches, fee_units, paid_units, arrears_units) != (declared_units, declared_fee, declared_paid, declared_arrears):
        _fail("occupancy declared usage")
    return {"byte_batches": byte_batches, "fee_units": fee_units, "charges": tuple(charges)}


def _decode_legacy_occupancy(encoded: bytes) -> Mapping[str, object]:
    versioned = encoded.startswith(_OCCUPANCY_V2); domain = _OCCUPANCY_V2 if versioned else _OCCUPANCY_V1
    reader = _Reader(encoded)
    if reader.fixed(len(domain)) != domain: _fail("legacy occupancy domain")
    batch = reader.u64(); occupancy_price = _decode_occupancy_schedule(reader, versioned)
    declared_units = reader.u128(); declared_fee = reader.u128(); count = reader.u64()
    if count > 256: _fail("legacy occupancy count")
    byte_batches = fee_units = 0
    charges: list[Mapping[str, object]] = []
    for _ in range(count):
        _decode_storage_namespace(reader); payer = reader.fixed(32); _nonzero(payer, "legacy occupancy payer")
        from_batch = reader.u64(); to_batch = reader.u64(); recorded_bytes = reader.u64(); reader.u64()
        units = reader.u128(); price = reader.u64(); accrued = reader.u128()
        if to_batch < from_batch: _fail("legacy occupancy batch interval")
        expected_units = _checked_u128_multiply(recorded_bytes, to_batch - from_batch, "legacy occupancy units")
        if (to_batch != batch or units != expected_units or price != occupancy_price
                or accrued != _checked_u128_multiply(units, price, "legacy occupancy fee")):
            _fail("legacy occupancy charge")
        byte_batches = _checked_u128_add(byte_batches, units, "legacy occupancy usage")
        fee_units = _checked_u128_add(fee_units, accrued, "legacy occupancy fees")
        charges.append({"payer": payer, "amount_due": accrued, "paid": True, "arrears_after": 0})
    reader.end()
    if (byte_batches, fee_units) != (declared_units, declared_fee): _fail("legacy occupancy declared usage")
    return {"byte_batches": byte_batches, "fee_units": fee_units, "charges": tuple(charges)}


def _decode_occupancy_schedule(reader: _Reader, versioned: bool) -> int:
    version = reader.u32() if versioned else 1
    if not version: _fail("occupancy schedule version")
    prices = tuple(reader.u64() for _ in range(7))
    return prices[-1]


def _decode_storage_namespace(reader: _Reader) -> Mapping[str, object]:
    length = reader.byte()
    if length not in (33, 65): _fail("storage namespace length")
    canonical = reader.fixed(length); program = canonical[:32]; _nonzero(program, "storage namespace program")
    tag = canonical[32]; principal: bytes | None = None
    if tag == 0 and length == 65:
        principal = canonical[33:]; _nonzero(principal, "storage namespace principal")
    elif not (tag == 1 and length == 33) and not (tag == 2 and length == 65):
        _fail("storage namespace tag")
    result: dict[str, object] = {"canonical": canonical, "wire": bytes((length,)) + canonical, "program": program}
    if principal is not None: result["principal"] = principal
    return result


def _occupancy_transfer_root(settlement: Mapping[str, object], asset: bytes) -> bytes:
    if len(asset) != 32: _fail("occupancy asset length")
    _nonzero(asset, "occupancy asset")
    payers: dict[bytes, list[int]] = {}
    for charge in cast(tuple[Mapping[str, object], ...], settlement["charges"]):
        payer = cast(bytes, charge["payer"]); values = payers.setdefault(payer, [0, 0, 0])
        values[0] = _checked_u128_add(values[0], cast(int, charge["amount_due"]), "occupancy payer due")
        if charge["paid"]: values[1] = _checked_u128_add(values[1], cast(int, charge["amount_due"]), "occupancy payer paid")
        values[2] = _checked_u128_add(values[2], cast(int, charge["arrears_after"]), "occupancy payer arrears")
    treasury = sha256(_ACCOUNT_DERIVATION + (11).to_bytes(4, "big") + _FEE_TREASURY_LABEL).digest()
    legs = [b"\0" + payer + treasury + asset + values[1].to_bytes(16, "big") + (23).to_bytes(2, "big")
        for payer, values in sorted(payers.items()) if (values[0] or values[2]) and values[1]]
    return _merkle_root(legs)


def _merkle_root(legs: list[bytes]) -> bytes:
    if not legs: return bytes(32)
    level = [sha256(_MERKLE_LEAF + leg).digest() for leg in legs]
    while len(level) > 1:
        level = [sha256(_MERKLE_INTERNAL + level[index] + (level[index + 1] if index + 1 < len(level) else level[index])).digest()
            for index in range(0, len(level), 2)]
    return level[0]


def _checked_u128_add(left: int, right: int, boundary: str) -> int:
    value = left + right
    if value > _MAX_U128: _fail(boundary)
    return value


def _checked_u128_multiply(left: int, right: int, boundary: str) -> int:
    value = left * right
    if value > _MAX_U128: _fail(boundary)
    return value


def _nonzero(value: bytes, boundary: str) -> None:
    if value == bytes(len(value)): _fail(boundary)


def _field(reader: _Reader, expected: int) -> None:
    if reader.byte() != expected: _fail("signed activity field tag")


def _fail(boundary: str) -> Literal[False]:
    raise ValueError(f"invalid {boundary}")


class _Reader:
    __slots__ = ("_value", "_offset")

    def __init__(self, value: bytes) -> None:
        self._value = value
        self._offset = 0

    def remaining(self) -> int: return len(self._value) - self._offset
    def fixed(self, length: int) -> bytes:
        end = self._offset + length
        if length < 0 or end > len(self._value): _fail("canonical bytes")
        result = self._value[self._offset:end]; self._offset = end; return result
    def byte(self) -> int: return self.fixed(1)[0]
    def u16(self) -> int: return int.from_bytes(self.fixed(2), "big")
    def u32(self) -> int: return int.from_bytes(self.fixed(4), "big")
    def u64(self) -> int: return int.from_bytes(self.fixed(8), "big")
    def u128(self) -> int: return int.from_bytes(self.fixed(16), "big")
    def i32(self) -> int: return int.from_bytes(self.fixed(4), "big", signed=True)
    def i64(self) -> int: return int.from_bytes(self.fixed(8), "big", signed=True)
    def sized_u32(self, maximum: int, exact: int | None = None) -> bytes:
        length = self.u32()
        if length > maximum or (exact is not None and length != exact): _fail("canonical u32 length")
        return self.fixed(length)
    def sized_u64(self, maximum: int) -> bytes:
        length = self.u64()
        if length > maximum: _fail("canonical u64 length")
        return self.fixed(length)
    def rest(self) -> bytes: return self.fixed(self.remaining())
    def end(self) -> None:
        if self.remaining(): _fail("trailing canonical bytes")
