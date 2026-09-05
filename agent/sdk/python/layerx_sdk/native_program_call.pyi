from dataclasses import dataclass

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

def encode_native_program_call(call: NativeProgramCall) -> bytes: ...
def decode_native_program_call(payload: bytes) -> NativeProgramCall: ...
