from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.qualification.release_runner import (
    EVIDENCE_SPECS,
    SCHEMA,
    QualificationFailure,
    validate_evidence,
)


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "tools" / "qualification" / "release_runner.py"


def digest(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


class ReleaseRunnerTests(unittest.TestCase):
    def test_missing_real_stack_fails_before_any_local_gate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            environment = dict(os.environ)
            for name in tuple(environment):
                if name.startswith("LAYERX_QUALIFICATION_"):
                    environment.pop(name)
            environment["LAYERX_QUALIFICATION_ARTIFACT_DIR"] = temporary
            completed = subprocess.run(
                [sys.executable, str(RUNNER), "human-qualify-journeys"],
                cwd=ROOT,
                env=environment,
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertEqual(completed.returncode, 1)
            status = json.loads(
                (Path(temporary) / "human-qualify-journeys" / "status.json").read_text(
                    encoding="utf-8"
                )
            )
            self.assertEqual(status["state"], "failed")
            self.assertEqual(status["commands"], [])
            self.assertIn("LAYERX_QUALIFICATION_REAL_STACK=1", status["failure"])

    def test_aggregate_failure_report_refuses_release_and_lists_unrun_gates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            environment = dict(os.environ)
            for name in tuple(environment):
                if name.startswith("LAYERX_QUALIFICATION_"):
                    environment.pop(name)
            environment["LAYERX_QUALIFICATION_ARTIFACT_DIR"] = temporary
            completed = subprocess.run(
                [sys.executable, str(RUNNER), "human-qualify"],
                cwd=ROOT,
                env=environment,
                check=False,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            self.assertEqual(completed.returncode, 1)
            report = json.loads(
                (Path(temporary) / "human-qualify" / "report.json").read_text(encoding="utf-8")
            )
            self.assertFalse(report["release_allowed"])
            unmet = {(entry["gate"], entry["state"]) for entry in report["unmet_gates"]}
            self.assertIn(("make --no-print-directory human-build", "not-run"), unmet)
            self.assertIn(("human-qualify-ui", "not-run"), unmet)
            self.assertIn(("human-qualify-usability", "not-run"), unmet)

    def test_evidence_validation_uses_exact_inventory_and_real_files(self) -> None:
        gate = "human-qualify-fabrication"
        components = {
            "node": "https://node.example.test",
            "agentd": "https://agent.example.test",
            "human_service": "https://human.example.test",
            "paxeer_testnet": "https://paxeer.example.test",
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cases = []
            for case_id in sorted(EVIDENCE_SPECS[gate].cases):
                artifacts = []
                for kind in sorted(EVIDENCE_SPECS[gate].artifact_kinds):
                    relative = Path("cases") / case_id / f"{kind}.bin"
                    candidate = root / relative
                    candidate.parent.mkdir(parents=True, exist_ok=True)
                    content = f"{case_id}:{kind}".encode()
                    candidate.write_bytes(content)
                    artifacts.append(
                        {"kind": kind, "path": relative.as_posix(), "sha256": digest(content)}
                    )
                cases.append({"id": case_id, "status": "passed", "artifacts": artifacts})
            payload = {
                "schema": SCHEMA,
                "gate": gate,
                "real_stack": True,
                "source_identity": "source-identity",
                "driver_sha256": "d" * 64,
                "components": components,
                "cases": cases,
            }
            manifest = root / "evidence.json"
            manifest.write_text(json.dumps(payload), encoding="utf-8")
            validated = validate_evidence(
                manifest,
                root,
                gate,
                components,
                "source-identity",
                "d" * 64,
            )
            self.assertEqual(validated["gate"], gate)
            payload["cases"].pop()
            manifest.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(QualificationFailure, "case inventory mismatch"):
                validate_evidence(
                    manifest,
                    root,
                    gate,
                    components,
                    "source-identity",
                    "d" * 64,
                )

    def test_evidence_artifacts_cannot_escape_the_gate_directory(self) -> None:
        gate = "human-qualify-fabrication"
        components = {
            "node": "https://node.example.test",
            "agentd": "https://agent.example.test",
            "human_service": "https://human.example.test",
            "paxeer_testnet": "https://paxeer.example.test",
        }
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            outside = root.parent / f"{root.name}-outside-qualification-artifact"
            outside.write_bytes(b"not accepted")
            try:
                artifacts = [
                    {
                        "kind": kind,
                        "path": "../outside-qualification-artifact",
                        "sha256": digest(b"not accepted"),
                    }
                    for kind in sorted(EVIDENCE_SPECS[gate].artifact_kinds)
                ]
                cases = [
                    {"id": case_id, "status": "passed", "artifacts": artifacts}
                    for case_id in sorted(EVIDENCE_SPECS[gate].cases)
                ]
                manifest = root / "evidence.json"
                manifest.write_text(
                    json.dumps(
                        {
                            "schema": SCHEMA,
                            "gate": gate,
                            "real_stack": True,
                            "source_identity": "source-identity",
                            "driver_sha256": "d" * 64,
                            "components": components,
                            "cases": cases,
                        }
                    ),
                    encoding="utf-8",
                )
                with self.assertRaisesRegex(QualificationFailure, "escapes its evidence root"):
                    validate_evidence(
                        manifest,
                        root,
                        gate,
                        components,
                        "source-identity",
                        "d" * 64,
                    )
            finally:
                outside.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
