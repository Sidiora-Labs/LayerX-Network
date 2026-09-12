import json
import unittest

from layerx_sdk.mirror import decode_mirror_process_result, MirrorVerificationError


class MirrorProcessTests(unittest.TestCase):
    def test_process_exit_and_requested_batch_bind_success(self):
        output = json.dumps({"ok": True, "verification": {
            "level": "receipt-verified", "batchNumber": "3",
            "headerDigest": "11" * 32, "evidenceDigest": "22" * 32,
            "sourceId": "source", "target": "mirror", "canonicalPosition": "3",
            "provenance": "Canonical", "latestBatch": None, "batchLag": "0",
            "failoverCount": 0, "agreeingSources": 1, "checkpointLevel": "unavailable",
        }}).encode()
        self.assertEqual(decode_mirror_process_result(0, output, 3).batch_number, 3)
        for status in [1, -15]:
            with self.assertRaises(MirrorVerificationError):
                decode_mirror_process_result(status, output, 3)
        with self.assertRaises(MirrorVerificationError):
            decode_mirror_process_result(0, output, 4)


if __name__ == "__main__":
    unittest.main()
