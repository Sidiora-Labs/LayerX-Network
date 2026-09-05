from __future__ import annotations

from dataclasses import dataclass
import re
import struct


@dataclass(frozen=True)
class NativeProgramCall:
    program_id: bytes
    guest_abi: int
    entrypoint: str
    calldata: bytes
    capabilities: bytes
    access_declaration: bytes
    response_capacity: int
    resources: tuple[int, int, int, int, int, int, int]


def encode_native_program_call(call: NativeProgramCall) -> bytes:
    if (len(call.program_id) != 32 or call.program_id == bytes(32)
            or isinstance(call.guest_abi, bool) or call.guest_abi not in (1, 2)
            or re.fullmatch(r"[A-Za-z0-9_.]{1,128}", call.entrypoint) is None
            or len(call.calldata) > 1_048_576 or len(call.capabilities) > 65_535
            or len(call.access_declaration) > 1_048_576
            or isinstance(call.response_capacity, bool) or not isinstance(call.response_capacity, int)
            or not 0 <= call.response_capacity <= 1_048_576 or len(call.resources) != 7
            or any(isinstance(value, bool) or not isinstance(value, int) or not 0 <= value < 1 << 64 for value in call.resources)):
        raise ValueError("invalid native program call")
    entrypoint = call.entrypoint.encode("ascii")
    return (struct.pack(">32sHHI HII 7Q", call.program_id, call.guest_abi, len(entrypoint),
                        len(call.calldata), len(call.capabilities), len(call.access_declaration),
                        call.response_capacity, *call.resources)
            + entrypoint + call.calldata + call.capabilities + call.access_declaration)


def decode_native_program_call(payload: bytes) -> NativeProgramCall:
    if len(payload) < 106:
        raise ValueError("invalid native program call")
    fields = struct.unpack(">32sHHI HII 7Q", payload[:106])
    lengths = fields[2:6]
    if 106 + sum(lengths) != len(payload):
        raise ValueError("invalid native program call")
    offset = 106
    bodies = []
    for length in lengths:
        bodies.append(payload[offset:offset + length])
        offset += length
    call = NativeProgramCall(fields[0], fields[1], bodies[0].decode("ascii"),
                             bodies[1], bodies[2], bodies[3], fields[6], fields[7:])
    encode_native_program_call(call)
    return call
