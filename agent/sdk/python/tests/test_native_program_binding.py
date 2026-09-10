import json
import unittest
from dataclasses import replace
from pathlib import Path

from layerx_sdk.native_program_call import (
    decode_native_program_call,
    encode_native_program_call,
)
from layerx_sdk.program_wire import decode_signed_program_call
from layerx_sdk.programs import NativeProgramRequest, _wire


class NativeProgramBindingTest(unittest.TestCase):
    def test_real_signed_native_binding_and_mismatches(self):
        fixture = json.loads((Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures/native-program-call-v3.json").read_text())
        native = decode_native_program_call(bytes.fromhex(fixture["payload_hex"]))
        request = NativeProgramRequest(native, 1000, bytes.fromhex(fixture["signed_activity_hex"]))
        self.assertEqual(decode_signed_program_call(request).activity_id, fixture["activity_id_hex"])
        self.assertEqual(_wire(request)["payload_encoding"], "native-v1")
        program_id = bytearray(native.program_id)
        index = next(i for i, value in enumerate(program_id) if value)
        program_id[index] = 1 if program_id[index] != 1 else 2
        mutate_bytes = lambda value: bytes([value[0] ^ 1]) + value[1:] if value else b"\1"
        for field, value in {
            "program_id": bytes(program_id),
            "guest_abi": 2 if native.guest_abi == 1 else 1,
            "entrypoint": ("a" if native.entrypoint[0] != "a" else "b") + native.entrypoint[1:],
            "calldata": mutate_bytes(native.calldata),
            "capabilities": mutate_bytes(native.capabilities),
            "access_declaration": mutate_bytes(native.access_declaration),
            "response_capacity": (native.response_capacity + 1) % 1_048_577,
            "resources": (native.resources[0] ^ 1,) + native.resources[1:],
        }.items():
            with self.subTest(field=field):
                self.assertNotEqual(value, getattr(native, field))
                changed = replace(native, **{field: value})
                encode_native_program_call(changed)
                with self.assertRaises(ValueError):
                    decode_signed_program_call(replace(request, native_call=changed))
        with self.assertRaises(ValueError):
            decode_signed_program_call(replace(request, fee_limit=request.fee_limit - 1))
