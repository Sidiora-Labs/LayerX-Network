#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "deploy-contracts.sh"


class DeploymentProfileTests(unittest.TestCase):
    def check_profile(self, value, *, chain="125", record=None):
        with tempfile.TemporaryDirectory(prefix="layerx-profile-") as directory:
            root = Path(directory)
            input_file = root / "input.json"
            record_file = root / "record.json"
            input_file.write_text(json.dumps(value))
            if record is not None:
                record_file.write_text(json.dumps(record))
            env = dict(os.environ)
            env.update(
                LAYERX_PAXEER_DEPLOYMENT_INPUT=str(input_file),
                LAYERX_PAXEER_DEPLOYMENT_RECORD=str(record_file),
                LAYERX_PAXEER_CHAIN_ID=chain,
            )
            return subprocess.run(
                ["bash", str(SCRIPT), "check-profile"],
                env=env, capture_output=True, text=True, check=False,
            )

    def test_immediate_beta_is_explicit(self):
        result = self.check_profile({"timelock_profile": "immediate-beta", "timelock_delay": 0})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "immediate-beta\n")

    def test_standard_remains_the_default(self):
        result = self.check_profile({"timelock_delay": 86400})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "standard\n")

    def test_nonzero_immediate_delay_is_rejected(self):
        result = self.check_profile({"timelock_profile": "immediate-beta", "timelock_delay": 1})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("delay must be exactly zero", result.stderr)

    def test_unknown_profile_is_rejected(self):
        result = self.check_profile({"timelock_profile": "unknown", "timelock_delay": 0})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unknown timelock profile", result.stderr)

    def test_existing_standard_deployment_cannot_be_reinterpreted(self):
        result = self.check_profile(
            {"timelock_profile": "immediate-beta", "timelock_delay": 0},
            record={"input": {"timelock_delay": 86400}},
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("differs from selected input", result.stderr)

    def test_immediate_profile_rejects_other_chains(self):
        result = self.check_profile(
            {"timelock_profile": "immediate-beta", "timelock_delay": 0}, chain="1",
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires chain 125", result.stderr)


if __name__ == "__main__":
    unittest.main()
