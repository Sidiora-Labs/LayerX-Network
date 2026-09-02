from __future__ import annotations

import hashlib
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen

from tools.qualification.release_runner import EVIDENCE_SPECS, SCHEMA


ROOT = Path(__file__).resolve().parents[2]
DRIVER = ROOT / "tools" / "qualification" / "beta_driver.py"
PLATFORM_MANIFEST = ROOT / "platform" / "Cargo.toml"
NETWORK_ID = "402"
SOURCE_IDENTITY = "beta-driver-test-source-identity"
DRIVER_TIMEOUT_SECONDS = 3600.0
READY_TIMEOUT_SECONDS = 60.0


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as candidate:
        candidate.bind(("127.0.0.1", 0))
        return candidate.getsockname()[1]


def cli_json(argv: list[str], environment: dict[str, str], stdin_text: str | None = None) -> dict:
    completed = subprocess.run(
        argv,
        cwd=ROOT,
        env=environment,
        input=stdin_text,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(
            f"{' '.join(argv)} exited {completed.returncode}: {completed.stdout}{completed.stderr}"
        )
    for line in reversed(completed.stdout.splitlines()):
        candidate = line.strip()
        if candidate.startswith("{"):
            document = json.loads(candidate)
            if document.get("ok") is not True:
                raise RuntimeError(f"{' '.join(argv)} did not succeed: {candidate}")
            return document["data"]
    raise RuntimeError(f"{' '.join(argv)} printed no JSON object: {completed.stdout}")


class LocalEmulatorStack:
    def __init__(self, work: Path) -> None:
        self.work = work
        self.home = work / "home"
        self.home.mkdir()
        self.target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "platform" / "target")))
        self.binary = self.target / "debug" / "layerx"
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        self.process: subprocess.Popen[bytes] | None = None
        self.log = work / "emulator.log"
        self.anchor = ""
        self.seed_file = ""
        self.source = ""
        self.destination = ""

    def cli_environment(self) -> dict[str, str]:
        environment = dict(os.environ)
        for name in tuple(environment):
            if name.startswith("LAYERX_"):
                environment.pop(name)
        environment["HOME"] = str(self.home)
        environment["LAYERX_CREDENTIAL_STORE"] = "mock"
        return environment

    def build_cli(self) -> None:
        subprocess.run(
            (
                "cargo",
                "build",
                "--offline",
                "--manifest-path",
                str(PLATFORM_MANIFEST),
                "--locked",
                "-p",
                "layerx-platform-cli",
                "--features",
                "test-credential-store",
            ),
            cwd=ROOT,
            check=True,
        )
        if not self.binary.is_file() or not os.access(self.binary, os.X_OK):
            raise RuntimeError(f"built layerx binary not found at {self.binary}")

    def provision(self) -> None:
        provisioned = cli_json(
            [str(self.binary), "--json", "emulator", "provision"], self.cli_environment()
        )
        self.anchor = provisioned["sequencer_trust_anchor"]
        self.seed_file = provisioned["sequencer_seed_file"]
        if len(self.anchor) != 64 or not Path(self.seed_file).is_file():
            raise RuntimeError(f"emulator provisioning is incomplete: {provisioned}")

    def start(self) -> None:
        with self.log.open("wb") as log:
            self.process = subprocess.Popen(
                (
                    str(self.binary),
                    "emulator",
                    "up",
                    "--listen",
                    f"127.0.0.1:{self.port}",
                    "--sequencer-seed-file",
                    self.seed_file,
                ),
                cwd=ROOT,
                env=self.cli_environment(),
                stdout=log,
                stderr=subprocess.STDOUT,
            )
        deadline = time.monotonic() + READY_TIMEOUT_SECONDS
        while True:
            if self.process.poll() is not None:
                raise RuntimeError(
                    f"emulator exited {self.process.returncode}: {self.log.read_text(errors='replace')}"
                )
            try:
                with urlopen(f"{self.url}/healthz", timeout=5) as response:
                    if response.status == 200 and b'"status":"ready"' in response.read():
                        return
            except (URLError, OSError):
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError(f"emulator at {self.url} did not become ready")
            time.sleep(0.2)

    def fund_accounts(self) -> None:
        environment = self.cli_environment()
        cli_json(
            [
                str(self.binary),
                "--json",
                "environment",
                "use",
                "emulator",
                "--endpoint",
                self.url,
                "--network-id",
                NETWORK_ID,
                "--sequencer-trust-anchor",
                self.anchor,
            ],
            environment,
        )
        seed_hex = Path(self.seed_file).read_text(encoding="utf-8").strip()
        sequencer = cli_json(
            [str(self.binary), "--json", "key", "import", "sequencer"],
            environment,
            stdin_text=seed_hex + "\n",
        )
        if sequencer["public_key"] != self.anchor:
            raise RuntimeError("imported sequencer key does not match the provisioned anchor")
        funded = cli_json(
            [
                str(self.binary),
                "--json",
                "account",
                "create",
                "--key",
                "sequencer",
                "--initial-amount",
                "1000000",
            ],
            environment,
        )
        cli_json([str(self.binary), "--json", "key", "create", "recipient"], environment)
        recipient = cli_json(
            [
                str(self.binary),
                "--json",
                "account",
                "create",
                "--key",
                "recipient",
                "--initial-amount",
                "0",
            ],
            environment,
        )
        self.source = funded["account"]
        self.destination = recipient["account"]

    def stop(self) -> None:
        if self.process is None:
            return
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.process = None

    def driver_environment(self) -> dict[str, str]:
        environment = self.cli_environment()
        environment.update(
            {
                "LAYERX_BIN": str(self.binary),
                "LAYERX_CLI_ENVIRONMENT": "emulator",
                "LAYERX_NETWORK_ID": NETWORK_ID,
                "LAYERX_SEQUENCER_TRUST_ANCHOR": self.anchor,
                "LAYERX_API_TOKEN": "emulator-does-not-authenticate-callers",
                "LAYERX_SOURCE": self.source,
                "LAYERX_DESTINATION": self.destination,
                "LAYERX_CURRENCY": "LXP",
                "LAYERX_AMOUNT": "250",
                "CARGO_NET_OFFLINE": "true",
            }
        )
        return environment


class BetaDriverTests(unittest.TestCase):
    stack: LocalEmulatorStack
    work: Path
    agentd_url: str
    paxeer_url: str

    @classmethod
    def setUpClass(cls) -> None:
        build_root = ROOT / "build" / "beta-driver-test"
        build_root.mkdir(parents=True, exist_ok=True)
        cls.work = Path(tempfile.mkdtemp(prefix="run-", dir=build_root))
        cls.stack = LocalEmulatorStack(cls.work)
        cls.stack.build_cli()
        cls.stack.provision()
        cls.stack.start()
        try:
            cls.stack.fund_accounts()
        except Exception:
            cls.stack.stop()
            raise
        cls.agentd_url = f"http://127.0.0.1:{free_port()}"
        cls.paxeer_url = f"http://127.0.0.1:{free_port()}"

    @classmethod
    def tearDownClass(cls) -> None:
        cls.stack.stop()

    def run_driver(self, gate: str, output: Path) -> tuple[subprocess.CompletedProcess[str], dict]:
        argv = [
            sys.executable,
            str(DRIVER),
            "run",
            "--gate",
            gate,
            "--output",
            str(output),
            "--source-identity",
            SOURCE_IDENTITY,
            "--node-url",
            self.stack.url,
            "--agentd-url",
            self.agentd_url,
            "--human-service-url",
            self.stack.url,
            "--paxeer-testnet-url",
            self.paxeer_url,
        ]
        completed = subprocess.run(
            argv,
            cwd=ROOT,
            env=self.stack.driver_environment(),
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            timeout=DRIVER_TIMEOUT_SECONDS,
        )
        (output / "driver-output.log").write_text(completed.stdout, encoding="utf-8")
        manifest = output / "evidence.json"
        self.assertTrue(manifest.is_file(), completed.stdout[-4000:])
        evidence = json.loads(manifest.read_text(encoding="utf-8"))
        return completed, evidence

    def assert_manifest_shape(self, gate: str, evidence: dict, output: Path) -> dict[str, dict]:
        self.assertEqual(evidence["schema"], SCHEMA)
        self.assertEqual(evidence["gate"], gate)
        self.assertIs(evidence["real_stack"], True)
        self.assertEqual(evidence["source_identity"], SOURCE_IDENTITY)
        self.assertEqual(evidence["driver_sha256"], hashlib.sha256(DRIVER.read_bytes()).hexdigest())
        self.assertEqual(
            evidence["components"],
            {
                "node": self.stack.url,
                "agentd": self.agentd_url,
                "human_service": self.stack.url,
                "paxeer_testnet": self.paxeer_url,
            },
        )
        cases = {record["id"]: record for record in evidence["cases"]}
        self.assertEqual(len(cases), len(evidence["cases"]))
        self.assertEqual(set(cases), set(EVIDENCE_SPECS[gate].cases))
        for case_id, record in cases.items():
            self.assertIn(record["status"], ("passed", "failed"), case_id)
            self.assertTrue(record["executions"] or record["status"] == "failed", case_id)
            if record["status"] == "passed":
                self.assertIsNone(record["reason"], case_id)
                kinds = {artifact["kind"] for artifact in record["artifacts"]}
                self.assertEqual(kinds, set(EVIDENCE_SPECS[gate].artifact_kinds), case_id)
                for artifact in record["artifacts"]:
                    path = output / artifact["path"]
                    self.assertTrue(path.is_file() and path.stat().st_size > 0, artifact)
                    self.assertEqual(
                        hashlib.sha256(path.read_bytes()).hexdigest(), artifact["sha256"], artifact
                    )
            else:
                self.assertIsInstance(record["reason"], str, case_id)
                self.assertTrue(record["reason"], case_id)
                self.assertEqual(record["artifacts"], [], case_id)
        return cases

    def test_adoption_inventory_executes_against_the_local_emulator_stack(self) -> None:
        gate = "platform-qualify-adoption"
        output = self.work / gate
        completed, evidence = self.run_driver(gate, output)
        cases = self.assert_manifest_shape(gate, evidence, output)
        rust = cases["rust/first-payment"]
        self.assertEqual(rust["status"], "passed", rust["reason"])
        result = json.loads((output / next(
            artifact["path"] for artifact in rust["artifacts"] if artifact["kind"] == "result"
        )).read_text(encoding="utf-8"))
        self.assertEqual(result["journey"]["state"], "done")
        self.assertEqual(result["journey"]["evidence"][0]["verification"], "receipt-verified")
        self.assertEqual(result["endpoint"], self.stack.url)
        timing = json.loads((output / next(
            artifact["path"] for artifact in rust["artifacts"] if artifact["kind"] == "timing"
        )).read_text(encoding="utf-8"))
        self.assertIs(timing["within_bound"], True)
        deploy = cases["programs/five-minute-deploy-and-paid-call"]
        self.assertEqual(deploy["status"], "failed")
        self.assertIn("registry-list", deploy["reason"])
        failed = sorted(case_id for case_id, record in cases.items() if record["status"] == "failed")
        self.assertTrue(failed)
        self.assertEqual(completed.returncode, 1, completed.stdout[-4000:])
        for case_id in failed:
            self.assertIn(f"case {case_id}: failed: ", completed.stdout)
        self.assertIn("case rust/first-payment: passed", completed.stdout)

    def test_multichain_inventory_reports_absent_infrastructure_as_failed(self) -> None:
        gate = "multichain-qualify"
        output = self.work / gate
        completed, evidence = self.run_driver(gate, output)
        cases = self.assert_manifest_shape(gate, evidence, output)
        one_ledger = cases["surfaces/one-ledger"]
        executions = {execution["label"]: execution for execution in one_ledger["executions"]}
        self.assertEqual(executions["schema-check"]["exit_code"], 0, one_ledger["reason"])
        self.assertEqual(
            executions["settlement-domain-rejections"]["exit_code"], 0, one_ledger["reason"]
        )
        self.assertIn("schema-conformance", executions)
        failed_steps = [label for label, execution in executions.items() if execution["exit_code"]]
        if failed_steps:
            self.assertEqual(one_ledger["status"], "failed")
            self.assertIn(failed_steps[0], one_ledger["reason"])
        else:
            self.assertEqual(one_ledger["status"], "passed", one_ledger["reason"])
        expected_inputs = {
            "mirrors/offline-verification": "LAYERX_MIRROR_VERIFY_CONFIG",
            "mirrors/tamper-rejection": "LAYERX_MIRROR_VERIFY_CONFIG",
            "surfaces/paxeer-exclusivity": "LAYERX_PAXEER_CHAIN_ID",
            "reference-ramp/end-to-end": "LAYERX_RAMP_URL",
            "reference-ramp/labelling": "LAYERX_RAMP_URL",
        }
        for case_id, variable in expected_inputs.items():
            record = cases[case_id]
            self.assertEqual(record["status"], "failed", case_id)
            self.assertIn("owner input missing", record["reason"], case_id)
            self.assertIn(variable, record["reason"], case_id)
            self.assertIn(f"case {case_id}: failed: ", completed.stdout)
        self.assertEqual(completed.returncode, 1, completed.stdout[-4000:])

    def test_driver_refuses_to_overwrite_existing_case_evidence(self) -> None:
        output = self.work / "occupied"
        (output / "cases").mkdir(parents=True)
        completed = subprocess.run(
            [
                sys.executable,
                str(DRIVER),
                "run",
                "--gate",
                "multichain-qualify",
                "--output",
                str(output),
                "--source-identity",
                SOURCE_IDENTITY,
                "--node-url",
                self.stack.url,
                "--agentd-url",
                self.agentd_url,
                "--human-service-url",
                self.stack.url,
                "--paxeer-testnet-url",
                self.paxeer_url,
            ],
            cwd=ROOT,
            env=self.stack.driver_environment(),
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.assertEqual(completed.returncode, 2)
        self.assertIn("already holds case evidence", completed.stderr)
        self.assertFalse((output / "evidence.json").exists())


if __name__ == "__main__":
    unittest.main()
