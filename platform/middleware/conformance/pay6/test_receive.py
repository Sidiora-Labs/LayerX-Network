import copy
from pathlib import Path
import unittest

from layerx_sdk.x402_receive import (
    decode_receive, encode_grant, encode_receive,
    grant_authorization_message, receive_authorization_message,
)

FIXTURE = Path(__file__).with_name("receive.hex").read_text().splitlines()


class ReceiveTests(unittest.TestCase):
    def setUp(self):
        self.wire = bytes.fromhex(FIXTURE[0])
        self.receive = decode_receive(self.wire)

    def test_native_canonical_and_signing_preimages(self):
        self.assertEqual(encode_receive(self.receive), self.wire)
        self.assertEqual(grant_authorization_message(self.receive["payer_grant"]).hex(), FIXTURE[1])
        self.assertEqual(receive_authorization_message(self.receive).hex(), FIXTURE[2])
        self.assertEqual(encode_grant(self.receive["payer_grant"]), self.wire[387:])
        self.assertEqual(self.receive["amount"], str(2**64 + 25))
        self.assertEqual(self.receive["receiver_sequence"], str(2**64 - 1))

    def test_all_truncations_and_trailing_data(self):
        for length in range(len(self.wire)):
            with self.subTest(length=length), self.assertRaises(ValueError):
                decode_receive(self.wire[:length])
        with self.assertRaises(ValueError):
            decode_receive(self.wire + b"\0")

    def test_tag_field_count_and_boolean_canonicality(self):
        for offset in (0, 1, 2, 3, 547, 596):
            value = bytearray(self.wire)
            value[offset] = 2 if offset in (547, 596) else value[offset] ^ 255
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                decode_receive(bytes(value))

    def test_integer_bounds_and_types(self):
        for field, values in {
            "amount": ["-1", "01", "1.0", "", "١", str(2**128), 1, True],
            "receiver_sequence": [str(2**64), "-1", "01", 1.0],
        }.items():
            for value in values:
                invalid = dict(self.receive, **{field: value})
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    encode_receive(invalid)
        for value in (0, 1, "true", None):
            invalid = copy.deepcopy(self.receive)
            invalid["payer_grant"]["recurring"] = value
            with self.assertRaises(ValueError):
                encode_receive(invalid)

    def test_exact_keys_hex_and_maximums(self):
        for value in (dict(self.receive, unknown=1), {k: v for k, v in self.receive.items() if k != "asset"}):
            with self.assertRaises(ValueError):
                encode_receive(value)
        for value in ("AB" * 32, "0x" + "00" * 32, "00" * 31, "gg" * 32):
            with self.assertRaises(ValueError):
                encode_receive(dict(self.receive, asset=value))
        maximum = dict(self.receive, amount=str(2**128 - 1))
        self.assertEqual(decode_receive(encode_receive(maximum)), maximum)


if __name__ == "__main__":
    unittest.main()
