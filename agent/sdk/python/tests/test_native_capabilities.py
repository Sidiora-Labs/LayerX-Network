import json
import unittest
from dataclasses import replace
from pathlib import Path

from layerx_sdk.native_capabilities import (
    NativeBalanceView,
    NativeCall,
    NativeEmitEvent,
    NativeProgramSpend,
    NativeReceiptRead,
    NativeSharedStorageRead,
    NativeSharedStorageWrite,
    NativeStorageRead,
    NativeStorageWrite,
    NativeTransfer402,
    decode_native_capability_set,
    derive_native_program_account,
    encode_native_capability_set,
    narrow_native_capability_set,
)
from layerx_sdk.verifier import programs_module_version_for_protocol

_CLASSES = dict(zip(
    ("StorageRead", "StorageWrite", "EmitEvent", "Call", "Transfer402", "ProgramSpend", "ReceiptRead", "BalanceView", "SharedStorageRead", "SharedStorageWrite"),
    (NativeStorageRead, NativeStorageWrite, NativeEmitEvent, NativeCall, NativeTransfer402, NativeProgramSpend, NativeReceiptRead, NativeBalanceView, NativeSharedStorageRead, NativeSharedStorageWrite),
))


def logical(entries):
    return tuple(_CLASSES[entry["kind"]](**{
        name: int(value) if name == "maximum_amount" else bytes.fromhex(value)
        for name, value in entry.items() if name not in ("kind", "tag")
    }) for entry in entries)


class NativeCapabilitiesTest(unittest.TestCase):
    def test_runtime_logical_grants_bytes_and_refusals(self):
        path = Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures/native-program-capabilities-v2.json"
        fixture = json.loads(path.read_text())
        parent, narrowed = logical(fixture["capabilities"]), logical(fixture["narrowed_capabilities"])
        self.assertEqual([entry["tag"] for entry in fixture["capabilities"]], [1,2,3,4,5,9,6,10,7,8])
        encoded = bytes.fromhex(fixture["canonical_hex"])
        self.assertEqual(encode_native_capability_set(parent), encoded)
        self.assertEqual(decode_native_capability_set(encoded), parent)
        self.assertEqual(encode_native_capability_set(narrowed).hex(), fixture["narrowed_hex"])
        self.assertEqual(narrow_native_capability_set(parent, narrowed), narrowed)
        self.assertIs(fixture["equal_narrowing_accepted"], True)
        self.assertEqual(narrow_native_capability_set(parent, parent), parent)
        self.assertEqual(len(fixture["escalation_cases"]), 3)
        for case in fixture["escalation_cases"]:
            self.assertEqual(case["parent"], "narrowed")
            self.assertIs(case["accepted"], False)
            child = logical(case["capabilities"])
            self.assertEqual(encode_native_capability_set(child).hex(), case["canonical_hex"])
            with self.assertRaises(ValueError): narrow_native_capability_set(narrowed, child)
        for length in range(len(encoded)):
            with self.assertRaises(ValueError): decode_native_capability_set(encoded[:length])
        for altered in (encoded + b"\0", encoded[:2] + b"\x0b" + encoded[3:], encoded[:2] + b"\2\1" + encoded[4:]):
            with self.assertRaises(ValueError): decode_native_capability_set(altered)
        with self.assertRaises(ValueError): encode_native_capability_set((*parent, parent[0]))
        spend = next(grant for grant in parent if isinstance(grant, NativeProgramSpend))
        maximum = []
        for index in range(238):
            seed = index.to_bytes(2, "big") + bytes(126)
            maximum.append(replace(spend, seed=seed, source_account=derive_native_program_account(spend.owner_program, seed)))
        self.assertEqual(len(encode_native_capability_set(maximum)), 65_452)
        with self.assertRaises(ValueError): encode_native_capability_set((*maximum, NativeCall(bytes([1]) * 32)))
        view = next(grant for grant in parent if isinstance(grant, NativeBalanceView))
        views = tuple(replace(view, account=bytes([index]) * 32) for index in range(1,33))
        self.assertEqual(len(decode_native_capability_set(encode_native_capability_set(views))), 32)
        with self.assertRaises(ValueError): encode_native_capability_set((*views, replace(view, account=bytes([33]) * 32)))
        for grant in parent:
            if isinstance(grant, NativeProgramSpend):
                changed = bytes([grant.source_account[0] ^ 1]) + grant.source_account[1:]
                with self.assertRaises(ValueError): encode_native_capability_set((replace(grant, source_account=changed),))
                with self.assertRaises(ValueError): encode_native_capability_set((replace(grant, seed=bytes(129)),))
            if isinstance(grant, NativeTransfer402):
                for amount in (0, True, 1 << 128):
                    with self.assertRaises(ValueError): encode_native_capability_set((replace(grant, maximum_amount=amount),))
            if isinstance(grant, NativeBalanceView):
                with self.assertRaises(ValueError): encode_native_capability_set((replace(grant, receipt_digest=bytes(32)),))

    def test_protocol_binding_rejects_bool_and_wrong_module_version(self):
        for protocol in (2, 3):
            for module in (1, 2, 3, 4):
                self.assertEqual(programs_module_version_for_protocol(protocol, module), (module == 4) == (protocol == 3))
        self.assertFalse(programs_module_version_for_protocol(2, True))
        self.assertFalse(programs_module_version_for_protocol(True, 1))
