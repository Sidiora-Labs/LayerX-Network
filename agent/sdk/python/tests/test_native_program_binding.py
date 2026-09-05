from dataclasses import replace
import json
from pathlib import Path
import unittest
from layerx_sdk.native_program_call import decode_native_program_call
from layerx_sdk.programs import NativeProgramRequest, _wire
from layerx_sdk.program_wire import decode_signed_program_call


class NativeProgramBindingTest(unittest.TestCase):
    def test_real_signed_native_binding_and_mismatches(self):
        fixture = json.loads((Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures/native-program-call-v3.json").read_text())
        native = decode_native_program_call(bytes.fromhex(fixture["payload_hex"]))
        request = NativeProgramRequest(native, 1000, bytes.fromhex(fixture["signed_activity_hex"]))
        self.assertEqual(decode_signed_program_call(request).activity_id, fixture["activity_id_hex"])
        self.assertEqual(_wire(request)["payload_encoding"], "native-v1")
        for field, value in {"program_id": bytes([0x22]) * 32, "guest_abi": 2, "entrypoint": "other", "calldata": b"1", "capabilities": b"\0\1", "access_declaration": b"other", "response_capacity": 17, "resources": (999,) + native.resources[1:]}.items():
            with self.subTest(field=field), self.assertRaises(ValueError):
                decode_signed_program_call(replace(request, native_call=replace(native, **{field: value})))
        with self.assertRaises(ValueError):
            decode_signed_program_call(replace(request, fee_limit=999))
