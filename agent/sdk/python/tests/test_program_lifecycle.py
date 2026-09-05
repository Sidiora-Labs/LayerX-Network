from pathlib import Path
import json
import unittest
import importlib.util
from hashlib import sha256

from layerx_sdk.program_lifecycle import NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown, NativeProgramLifecycleRequest
from layerx_sdk.agent_http import _encode_program_mutation_body, _decode_program_boundary_error, _decode_envelope, ProgramBoundaryError
from layerx_sdk.production import PlatformSdkError
from layerx_sdk.program_wire import bind_signed_program_lifecycle
from layerx_sdk.programs import verify_lifecycle_recovery, resolve_lifecycle_response, resolve_lifecycle_failure


class ProgramLifecycleTest(unittest.TestCase):
    def test_boundary_refusal_and_unknown_envelopes(self):
        with self.assertRaises(ProgramBoundaryError) as raised:
            _decode_envelope(400, b'{"error":{"code":"invalid_program_payload","retry":"never"}}', "program.deploy")
        self.assertEqual(raised.exception.boundary_code, "invalid_program_payload")
        self.assertEqual(raised.exception.retry, "never")
        self.assertEqual(_decode_program_boundary_error(503, {"code": "node_unavailable", "retry": "after", "retry_after_seconds": 2}).retry_after_ms, 2000)
        unknown = {"state": "unknown", "activity_id": "11" * 32, "retry": "after", "retry_after_seconds": 2}
        self.assertEqual(_decode_envelope(202, json.dumps(unknown).encode(), "program.deploy"), unknown)
        for status, value in [(200, {"code": "invalid_program_payload", "retry": "never"}), (400, {"code": "invalid_program_payload", "retry": "never", "extra": 0}), (503, {"code": "node_unavailable", "retry": "after", "retry_after_seconds": True})]:
            with self.assertRaises(PlatformSdkError):
                _decode_program_boundary_error(status, value)

    def test_c_signed_fixtures_and_refusals(self):
        fixtures = Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures"
        source = Path(__file__).resolve().parents[4] / "platform/integrations/fastapi/layerx_fastapi/signatures.py"
        spec = importlib.util.spec_from_file_location("lifecycle_signatures", source)
        if spec is None or spec.loader is None:
            self.fail("signature verifier unavailable")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        signatures = module.LayerXSignatureVerifier()
        call_receipt = json.loads((fixtures / "receipt-programs-positive-v3.json").read_text())
        recovery = {"activity_id": "41" * 32, "receipt": call_receipt["canonical_receipt_hex"]}
        sequencer = bytes.fromhex(call_receipt["authorized_batch"]["sequencer_public_key_hex"])
        self.assertEqual(_decode_envelope(200, json.dumps({"result": recovery}).encode(), "program.receipt"), recovery)
        for result, expected in [(recovery, "42" * 32), ({**recovery, "program_id": "11" * 32}, recovery["activity_id"]), (recovery, recovery["activity_id"])]:
            with self.assertRaises((ValueError, PlatformSdkError)):
                verify_lifecycle_recovery(result, expected, sequencer, signatures)
        for name, ordinal, decoder in (
            ("deploy", 1, NativeProgramDeploy.decode), ("upgrade", 2, NativeProgramUpgrade.decode),
            *(("wind-down-" + operation, 7, NativeProgramWindDown.decode) for operation in ("route", "deprecate", "tombstone", "exit")),
        ):
            with self.subTest(name=name):
                fixture = json.loads((fixtures / ("native-program-" + name + "-v3.json")).read_text())
                payload = bytes.fromhex(fixture["payload_hex"])
                signed = bytes.fromhex(fixture["signed_activity_hex"])
                self.assertEqual(_encode_program_mutation_body(fixture["signed_activity_hex"]), signed)
                with self.assertRaises(PlatformSdkError):
                    _encode_program_mutation_body({"activity": fixture["signed_activity_hex"]})
                unsigned = bytearray(signed[:-69]); unsigned[4] = 11
                preimage = sha256(b"LXP/v1/signature-preimage\0" + unsigned).digest()
                self.assertTrue(signatures.verify_ed25519(bytes.fromhex(fixture["public_key_hex"]), signed[-64:], preimage))
                value = decoder(payload)
                self.assertEqual(value.encode(), payload)
                request = NativeProgramLifecycleRequest(ordinal, payload, signed)
                bound = request.bind(fixture["idempotency_key_hex"])
                expected_unknown = {"state": "unknown", "activity_id": fixture["activity_id_hex"], "idempotency_key": fixture["idempotency_key_hex"], "retained_signed_activity": fixture["signed_activity_hex"]}
                damaged = bytearray.fromhex(call_receipt["canonical_receipt_hex"])
                damaged[-1] ^= 1
                for response in (
                    {"state": "executed", "activity_id": bound.activity_id, "receipt": damaged.hex(), "terminal_payload": "", "call_graph": ""},
                    {"state": "refused", "activity_id": bound.activity_id, "receipt": "00", "terminal_payload": "", "call_graph": ""},
                    {"state": "unknown", "activity_id": "00" * 32, "retry": "after", "retry_after_seconds": 2},
                ):
                    decoded = _decode_envelope(200, json.dumps({"result": response}).encode(), "program.deploy")
                    self.assertEqual(resolve_lifecycle_response(decoded, bound, sequencer, signatures), expected_unknown)
                for encoded in (b'{"result":', b'{"result":{},"extra":true}'):
                    with self.assertRaises(PlatformSdkError) as caught:
                        _decode_envelope(200, encoded, "program.deploy")
                    self.assertEqual(resolve_lifecycle_failure(caught.exception, bound), expected_unknown)
                refusal = _decode_program_boundary_error(400, {"code": "invalid_program_payload", "retry": "never"})
                with self.assertRaises(ProgramBoundaryError) as caught:
                    resolve_lifecycle_failure(refusal, bound)
                self.assertIs(caught.exception, refusal)
                hash_offset = len(signed) - 69 - len(payload) - 5 - 32
                bad_hash = bytearray(signed); bad_hash[hash_offset] ^= 1
                with self.assertRaises(ValueError):
                    bind_signed_program_lifecycle(bytes(bad_hash), payload, ordinal)
                if name == "deploy":
                    from dataclasses import replace
                    for size in (524_288, 524_289):
                        large_wasm = value.wasm + bytes(size - len(payload))
                        large_payload = replace(value, wasm=large_wasm, new_hash=sha256(large_wasm).digest()).encode()
                        self.assertEqual(len(large_payload), size)
                        mutated = signed[:hash_offset] + sha256(b"LXP/v1/payload-hash\0" + large_payload).digest() + b"\x0b" + size.to_bytes(4, "big") + large_payload + signed[-69:]
                        if size == 524_288:
                            self.assertEqual(bind_signed_program_lifecycle(mutated, None, ordinal).idempotency_key, fixture["idempotency_key_hex"])
                        else:
                            with self.assertRaises(ValueError):
                                bind_signed_program_lifecycle(mutated, None, ordinal)
                self.assertEqual(bound.activity_id, fixture["activity_id_hex"])
                self.assertEqual(bind_signed_program_lifecycle(signed, None, ordinal, fixture["idempotency_key_hex"]).activity_id, fixture["activity_id_hex"])
                with self.assertRaises(ValueError):
                    bind_signed_program_lifecycle(signed, None, ordinal, "00" * 32)
                with self.assertRaises(ValueError):
                    bind_signed_program_lifecycle(signed, None, True)
                for length in range(len(payload)):
                    with self.assertRaises(ValueError):
                        decoder(payload[:length])
                with self.assertRaises(ValueError):
                    decoder(payload + b"\0")
                with self.assertRaises((ValueError, TypeError)):
                    request.bind("00" * 32)
                for changed in (signed[:1] + b"\2" + signed[2:], signed + b"\0", signed[:17] + bytes([signed[17] ^ 1]) + signed[18:]):
                    with self.assertRaises((ValueError, TypeError)):
                        NativeProgramLifecycleRequest(ordinal, payload, changed)
                if ordinal in (1, 2):
                    changed = bytearray(payload); changed[68] ^= 1
                    with self.assertRaises(ValueError):
                        decoder(bytes(changed))
                    changed = bytearray(payload); changed[35] = 1
                    with self.assertRaises(ValueError):
                        decoder(bytes(changed))

    def test_policy_flags_and_bounds(self):
        from hashlib import sha256
        wasm = b"\0asm\x01\0\0\0"
        program = b"\1" * 32
        digest = sha256(wasm).digest()
        with self.assertRaises(ValueError):
            NativeProgramDeploy(program, 2, program, digest, wasm).encode()
        with self.assertRaises(ValueError):
            NativeProgramDeploy(program, 2, bytes(32), digest, wasm, bytes(953)).encode()
        with self.assertRaises(ValueError):
            NativeProgramUpgrade(program, 2, digest, digest, wasm, clear_interface=True).encode()
        with self.assertRaises(ValueError):
            NativeProgramWindDown(program, 1, seed=bytes(129)).encode()
