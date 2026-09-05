import unittest
from layerx_sdk.native_program_call import NativeProgramCall, encode_native_program_call, decode_native_program_call


class NativeProgramCallTest(unittest.TestCase):
    def test_native_header_and_all_truncations(self):
        call = NativeProgramCall(bytes([0x11]) * 32, 1, "layerx_call", b"", b"\0\0",
                                 b"LayerX/programs/access-declaration/v1\0\0", 16,
                                 (1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096))
        encoded = encode_native_program_call(call)
        self.assertEqual(encoded[32:50], bytes([0, 1, 0, 11, 0, 0, 0, 0, 0, 2, 0, 0, 0, 39, 0, 0, 0, 16]))
        self.assertEqual(decode_native_program_call(encoded), call)
        for length in range(len(encoded)):
            with self.assertRaises(ValueError):
                decode_native_program_call(encoded[:length])
        with self.assertRaises(ValueError):
            decode_native_program_call(encoded + b"\0")
