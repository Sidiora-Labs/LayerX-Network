#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence
from urllib.parse import urlsplit


SCHEMA = "layerx-qualification-evidence-v1"
DRIVER_ENV = "LAYERX_QUALIFICATION_DRIVER"
DRIVER_DIGEST_ENV = "LAYERX_QUALIFICATION_DRIVER_SHA256"
REAL_STACK_ENV = "LAYERX_QUALIFICATION_REAL_STACK"
ARTIFACT_ROOT_ENV = "LAYERX_QUALIFICATION_ARTIFACT_DIR"
URL_ENVIRONMENTS = {
    "node": "LAYERX_QUALIFICATION_NODE_URL",
    "agentd": "LAYERX_QUALIFICATION_AGENT_URL",
    "human_service": "LAYERX_QUALIFICATION_HUMAN_URL",
    "paxeer_testnet": "LAYERX_QUALIFICATION_PAXEER_URL",
}


def journey_cases() -> frozenset[str]:
    journeys = (
        "onboarding",
        "wallet-binding",
        "deposit",
        "move-money",
        "create-agent",
        "approval-grant",
        "approval-reject",
        "withdrawal-claim",
        "emergency-exit",
    )
    return frozenset(
        f"{shell}/{journey}" for shell in ("mobile", "desktop") for journey in journeys
    )


FAULT_STAGES = {
    "deposit": (
        "wallet-signed-before-registration",
        "registered",
        "custody-submitted",
        "custody-acknowledged",
        "finality-observed",
        "proof-built",
        "credit-prepared",
        "credit-submitted",
        "receipt-published",
        "acknowledged",
    ),
    "move-money": (
        "registered",
        "leg-prepared",
        "leg-submitted",
        "leg-tracked",
        "receipt-published",
        "acknowledged",
    ),
    "withdrawal": (
        "registered",
        "debit-prepared",
        "debit-submitted",
        "debit-receipt-published",
        "checkpoint-observed",
        "payout-submitted",
        "payout-receipt-published",
        "acknowledged",
    ),
    "withdrawal-claim": (
        "claimable-persisted",
        "claim-submitted",
        "challenge-window-observed",
        "finalisation-submitted",
        "payout-verified",
        "acknowledged",
    ),
    "emergency-exit": (
        "checkpoint-read",
        "wallet-signed-before-registration",
        "registered",
        "exit-submitted",
        "finality-observed",
        "acknowledged",
    ),
}


def fault_cases() -> frozenset[str]:
    modes = ("crash", "disconnect", "duplicate-delivery")
    return frozenset(
        f"{journey}/{stage}/{mode}"
        for journey, stages in FAULT_STAGES.items()
        for stage in stages
        for mode in modes
    )


@dataclass(frozen=True)
class EvidenceSpec:
    cases: frozenset[str]
    artifact_kinds: frozenset[str]


EVIDENCE_SPECS = {
    "human-qualify-journeys": EvidenceSpec(
        journey_cases(), frozenset(("receipt", "proof", "timing"))
    ),
    "human-qualify-fabrication": EvidenceSpec(
        frozenset(
            (
                "balances",
                "movement-outcomes",
                "agent-states",
                "approval-outcomes",
                "done-without-verifying-receipt",
            )
        ),
        frozenset(("tampered-input", "exported-evidence", "verifier-rejection")),
    ),
    "human-qualify-faults": EvidenceSpec(
        fault_cases(), frozenset(("injection", "result", "ledger-proof"))
    ),
    "human-qualify-perf": EvidenceSpec(
        frozenset(
            (
                "mobile/performance-budget",
                "desktop/performance-budget",
                "soak/deposit-awaiting-finality",
                "soak/withdrawal-awaiting-checkpoint",
                "soak/unactioned-approval",
                "soak/store-deletion-rebuild",
            )
        ),
        frozenset(("metrics", "resource-curve", "latency-curve")),
    ),
    "human-qualify-ui": EvidenceSpec(
        frozenset(
            (
                "mobile/state-matrix",
                "desktop/state-matrix",
                "mobile/visual-regression",
                "desktop/visual-regression",
                "automated-accessibility",
                "assistive-technology-core-journeys",
                "copy-integrity",
                "component-library-integrity",
            )
        ),
        frozenset(("result",)),
    ),
    "human-qualify-usability": EvidenceSpec(
        frozenset(
            (
                "mobile/onboarding",
                "mobile/deposit",
                "mobile/move-money",
                "mobile/create-agent",
            )
        ),
        frozenset(("protocol", "results", "defects")),
    ),
    "platform-qualify-adoption": EvidenceSpec(
        frozenset(
            (
                "rust/first-payment",
                "typescript/first-payment",
                "python/first-payment",
                "go/first-payment",
                "jvm/first-payment",
                "swift/first-payment",
                "dotnet/first-payment",
                "middleware/ten-line-integration",
                "programs/five-minute-deploy-and-paid-call",
            )
        ),
        frozenset(("published-artifact", "timing", "result")),
    ),
    "programs-qualify": EvidenceSpec(
        frozenset(
            (
                "real-node/hostile-program-gauntlet",
                "real-node/isolation",
                "real-node/determinism-differential",
                "real-node/metering",
                "ported-reference-contracts",
                "program-heavy-monetary-law-replay",
            )
        ),
        frozenset(("inventory", "result", "ledger-proof")),
    ),
    "interop-qualify": EvidenceSpec(
        frozenset(
            (
                "x402-v2/all-transports",
                "ap2/pinned-conformance",
                "ucp/pinned-conformance",
                "visa-trusted-agent/pinned-conformance",
                "portable-verification/layerx-to-external",
                "portable-verification/external-to-layerx",
                "migration/fault-injection",
                "fiat/fault-injection",
            )
        ),
        frozenset(("compatibility", "result", "proof")),
    ),
    "multichain-qualify": EvidenceSpec(
        frozenset(
            (
                "mirrors/offline-verification",
                "mirrors/tamper-rejection",
                "surfaces/paxeer-exclusivity",
                "surfaces/one-ledger",
                "reference-ramp/end-to-end",
                "reference-ramp/labelling",
            )
        ),
        frozenset(("proof", "tamper-rejection", "result")),
    ),
}


LOCAL_COMMANDS = {
    "human-qualify-journeys": (
        ("make", "--no-print-directory", "human-test-journeys"),
        ("make", "--no-print-directory", "human-test-agents"),
        ("make", "--no-print-directory", "human-test-approvals"),
        ("make", "--no-print-directory", "human-test-paxeer"),
    ),
    "human-qualify-fabrication": (
        ("make", "--no-print-directory", "human-test-explorer"),
        ("make", "--no-print-directory", "human-test-activity"),
        ("make", "--no-print-directory", "human-test-component"),
    ),
    "human-qualify-faults": (
        ("make", "--no-print-directory", "human-test-journeys"),
    ),
    "human-qualify-perf": (
        ("make", "--no-print-directory", "human-e2e-perf"),
    ),
    "human-qualify": (
        ("make", "--no-print-directory", "human-build"),
        ("make", "--no-print-directory", "human-test"),
        ("make", "--no-print-directory", "human-lint"),
        ("make", "--no-print-directory", "human-check"),
        ("make", "--no-print-directory", "human-check-ui"),
        ("make", "--no-print-directory", "human-check-bundle"),
        ("make", "--no-print-directory", "human-qualify-journeys"),
        ("make", "--no-print-directory", "human-qualify-fabrication"),
        ("make", "--no-print-directory", "human-qualify-faults"),
        ("make", "--no-print-directory", "human-qualify-perf"),
    ),
    "platform-qualify": (
        ("make", "--no-print-directory", "human-qualify"),
        ("make", "--no-print-directory", "platform-build-all"),
        ("make", "--no-print-directory", "platform-lint"),
        ("make", "--no-print-directory", "platform-test"),
        ("make", "--no-print-directory", "platform-test-sdks"),
        ("make", "--no-print-directory", "platform-test-middleware"),
        ("make", "--no-print-directory", "platform-test-docs"),
        ("make", "--no-print-directory", "platform-hosted-smoke"),
        ("make", "--no-print-directory", "platform-test-agent-install"),
        ("make", "--no-print-directory", "platform-emulator-conformance"),
        ("make", "--no-print-directory", "platform-real-agent-integration"),
        ("make", "--no-print-directory", "platform-test-mobile-artifacts"),
        ("make", "--no-print-directory", "platform-real-ios-integration"),
        ("make", "--no-print-directory", "platform-real-android-integration"),
        ("make", "--no-print-directory", "programs-test"),
        ("make", "--no-print-directory", "interop-test"),
        ("make", "--no-print-directory", "interop-test-migration-testnets"),
        ("make", "--no-print-directory", "interop-test-ramps-sandbox"),
        ("make", "--no-print-directory", "platform-release-check"),
    ),
}


EXTERNAL_GATES = {
    "human-qualify": ("human-qualify-ui", "human-qualify-usability"),
    "platform-qualify": (
        "platform-qualify-adoption",
        "programs-qualify",
        "interop-qualify",
        "multichain-qualify",
    ),
}


class QualificationFailure(RuntimeError):
    pass


def file_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_json(path: Path, payload: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def validated_url(value: str, name: str) -> str:
    candidate = urlsplit(value)
    if candidate.scheme not in ("http", "https") or not candidate.hostname:
        raise QualificationFailure(f"{name} must be an absolute HTTP or HTTPS URL")
    if candidate.username or candidate.password:
        raise QualificationFailure(f"{name} must not contain credentials")
    if candidate.query or candidate.fragment:
        raise QualificationFailure(f"{name} must not contain a query or fragment")
    return value


def validate_evidence(
    manifest_path: Path,
    evidence_root: Path,
    gate: str,
    expected_components: Mapping[str, str],
    source_identity: str,
    driver_digest: str,
) -> dict[str, object]:
    try:
        payload = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise QualificationFailure(f"{gate} evidence manifest is unreadable: {error}") from error
    if not isinstance(payload, dict):
        raise QualificationFailure(f"{gate} evidence manifest must be a JSON object")
    required_header = {
        "schema": SCHEMA,
        "gate": gate,
        "real_stack": True,
        "source_identity": source_identity,
        "driver_sha256": driver_digest,
    }
    for field, expected in required_header.items():
        if payload.get(field) != expected:
            raise QualificationFailure(f"{gate} evidence field {field!r} does not match the run")
    if payload.get("components") != dict(expected_components):
        raise QualificationFailure(f"{gate} evidence does not bind the exact real-stack endpoints")

    spec = EVIDENCE_SPECS[gate]
    cases = payload.get("cases")
    if not isinstance(cases, list):
        raise QualificationFailure(f"{gate} evidence cases must be a list")
    observed: dict[str, dict[str, object]] = {}
    observed_artifact_paths: set[Path] = set()
    evidence_root = evidence_root.resolve()
    for item in cases:
        if not isinstance(item, dict) or not isinstance(item.get("id"), str):
            raise QualificationFailure(f"{gate} contains an invalid case record")
        case_id = item["id"]
        if case_id in observed:
            raise QualificationFailure(f"{gate} contains duplicate case {case_id}")
        if item.get("status") != "passed":
            raise QualificationFailure(f"{gate} case {case_id} is not passed")
        artifacts = item.get("artifacts")
        if not isinstance(artifacts, list):
            raise QualificationFailure(f"{gate} case {case_id} has no artifact list")
        kinds: set[str] = set()
        for artifact in artifacts:
            if not isinstance(artifact, dict):
                raise QualificationFailure(f"{gate} case {case_id} has an invalid artifact")
            kind = artifact.get("kind")
            relative = artifact.get("path")
            expected_digest = artifact.get("sha256")
            if not isinstance(kind, str) or not isinstance(relative, str):
                raise QualificationFailure(f"{gate} case {case_id} has an incomplete artifact")
            if kind in kinds:
                raise QualificationFailure(f"{gate} case {case_id} repeats artifact kind {kind}")
            kinds.add(kind)
            candidate_path = Path(relative)
            if candidate_path.is_absolute() or ".." in candidate_path.parts:
                raise QualificationFailure(
                    f"{gate} case {case_id} artifact escapes its evidence root"
                )
            candidate = evidence_root.joinpath(candidate_path)
            try:
                resolved = candidate.resolve(strict=True)
                resolved.relative_to(evidence_root)
            except (OSError, ValueError) as error:
                raise QualificationFailure(
                    f"{gate} case {case_id} artifact is missing or outside its evidence root"
                ) from error
            if candidate.is_symlink() or not resolved.is_file() or resolved.stat().st_size == 0:
                raise QualificationFailure(
                    f"{gate} case {case_id} artifact is not a non-empty regular file"
                )
            if resolved in observed_artifact_paths:
                raise QualificationFailure(
                    f"{gate} reuses one artifact across cases or evidence kinds"
                )
            observed_artifact_paths.add(resolved)
            if not isinstance(expected_digest, str) or file_digest(resolved) != expected_digest:
                raise QualificationFailure(f"{gate} case {case_id} artifact digest does not match")
        missing_kinds = spec.artifact_kinds - kinds
        if missing_kinds:
            missing = ", ".join(sorted(missing_kinds))
            raise QualificationFailure(f"{gate} case {case_id} is missing artifacts: {missing}")
        observed[case_id] = item
    if set(observed) != set(spec.cases):
        missing = sorted(spec.cases - set(observed))
        unexpected = sorted(set(observed) - spec.cases)
        raise QualificationFailure(
            f"{gate} case inventory mismatch; missing={missing!r}; unexpected={unexpected!r}"
        )
    return payload


class ReleaseRunner:
    def __init__(self, root: Path, gate: str, environment: Mapping[str, str]) -> None:
        if gate not in LOCAL_COMMANDS:
            raise QualificationFailure(f"unknown qualification gate: {gate}")
        self.root = root.resolve()
        self.gate = gate
        self.environment = dict(environment)
        self.artifact_root = Path(
            self.environment.get(ARTIFACT_ROOT_ENV, str(self.root / "build" / "qualification"))
        ).resolve()
        self.output = self.artifact_root / gate
        self.commands: list[dict[str, object]] = []
        self.external_started: list[str] = []
        self.external_completed: list[str] = []
        self.planned = [" ".join(command) for command in LOCAL_COMMANDS[gate]]
        self.external_planned = (
            [gate] if gate in EVIDENCE_SPECS else list(EXTERNAL_GATES.get(gate, ()))
        )
        self.source_revision = self._capture(("git", "rev-parse", "HEAD")).strip()
        self.source_identity = self._compute_source_identity()

    def _capture(self, command: Sequence[str]) -> str:
        completed = subprocess.run(
            command,
            cwd=self.root,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if completed.returncode != 0:
            raise QualificationFailure(
                "could not inspect qualification source: "
                f"{' '.join(command)}: {completed.stderr.strip()}"
            )
        return completed.stdout

    def _compute_source_identity(self) -> str:
        digest = hashlib.sha256()
        digest.update(self.source_revision.encode("ascii"))
        digest.update(b"\0")
        diff = subprocess.run(
            ("git", "diff", "--binary", "HEAD", "--"),
            cwd=self.root,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if diff.returncode != 0:
            detail = diff.stderr.decode(errors="replace").strip()
            raise QualificationFailure(
                f"could not fingerprint tracked source changes: {detail}"
            )
        digest.update(diff.stdout)
        untracked = subprocess.run(
            ("git", "ls-files", "--others", "--exclude-standard", "-z"),
            cwd=self.root,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if untracked.returncode != 0:
            detail = untracked.stderr.decode(errors="replace").strip()
            raise QualificationFailure(
                f"could not fingerprint untracked source: {detail}"
            )
        generated_parts = {
            ".build",
            ".next",
            ".playwright",
            "__pycache__",
            "build",
            "node_modules",
            "target",
            "test-results",
        }
        for raw_path in sorted(item for item in untracked.stdout.split(b"\0") if item):
            relative = Path(os.fsdecode(raw_path))
            if any(part in generated_parts for part in relative.parts):
                continue
            candidate = self.root / relative
            try:
                candidate.resolve().relative_to(self.artifact_root)
            except ValueError:
                pass
            else:
                continue
            if not candidate.is_file() or candidate.is_symlink():
                continue
            digest.update(raw_path)
            digest.update(b"\0")
            digest.update(file_digest(candidate).encode("ascii"))
            digest.update(b"\0")
        return digest.hexdigest()

    def _status(self, state: str, failure: str | None = None) -> None:
        atomic_json(
            self.output / "status.json",
            {
                "commands": self.commands,
                "external_completed": self.external_completed,
                "external_started": self.external_started,
                "external_gates": self.external_planned,
                "failure": failure,
                "gate": self.gate,
                "planned_commands": self.planned,
                "schema": "layerx-qualification-status-v1",
                "source_identity": self.source_identity,
                "source_revision": self.source_revision,
                "state": state,
            },
        )

    def _preflight(self) -> tuple[Path, str, dict[str, str]]:
        if self.environment.get(REAL_STACK_ENV) != "1":
            raise QualificationFailure(f"{REAL_STACK_ENV}=1 is required")
        driver_value = self.environment.get(DRIVER_ENV, "")
        driver = Path(driver_value)
        if not driver.is_absolute() or not driver.is_file() or not os.access(driver, os.X_OK):
            raise QualificationFailure(f"{DRIVER_ENV} must name an absolute executable file")
        expected_driver_digest = self.environment.get(DRIVER_DIGEST_ENV, "")
        if len(expected_driver_digest) != 64 or any(
            character not in "0123456789abcdef" for character in expected_driver_digest
        ):
            raise QualificationFailure(f"{DRIVER_DIGEST_ENV} must be a lowercase SHA-256 digest")
        if file_digest(driver) != expected_driver_digest:
            raise QualificationFailure(
                "qualification driver digest does not match the pinned digest"
            )
        components: dict[str, str] = {}
        for component, variable in URL_ENVIRONMENTS.items():
            value = self.environment.get(variable, "")
            components[component] = validated_url(value, variable)
        if len(set(components.values())) != len(components):
            raise QualificationFailure("real-stack component endpoints must be distinct URLs")
        return driver, expected_driver_digest, components

    def _execute(self, command: Sequence[str], label: str) -> None:
        log_directory = self.output / "logs"
        log_directory.mkdir(parents=True, exist_ok=True)
        log_path = log_directory / f"{len(self.commands) + 1:02d}-{label}.log"
        with log_path.open("wb") as log:
            process = subprocess.Popen(
                command,
                cwd=self.root,
                env=self.environment,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
            )
            assert process.stdout is not None
            for chunk in iter(lambda: process.stdout.read(65536), b""):
                log.write(chunk)
                log.flush()
                sys.stdout.buffer.write(chunk)
                sys.stdout.buffer.flush()
            returncode = process.wait()
        record = {
            "argv": list(command),
            "exit_code": returncode,
            "log": str(log_path.relative_to(self.output)),
            "log_sha256": file_digest(log_path),
        }
        self.commands.append(record)
        self._status("running")
        if returncode != 0:
            raise QualificationFailure(
                f"command failed with exit {returncode}: {' '.join(command)}"
            )

    def _external(
        self,
        external_gate: str,
        driver: Path,
        driver_digest: str,
        components: Mapping[str, str],
    ) -> None:
        external_root = self.output / "external" / external_gate
        if external_root.exists():
            if external_root.is_symlink():
                raise QualificationFailure(
                    f"refusing symlinked qualification output {external_root}"
                )
            shutil.rmtree(external_root)
        external_root.mkdir(parents=True)
        command = [
            str(driver),
            "run",
            "--gate",
            external_gate,
            "--output",
            str(external_root),
            "--source-identity",
            self.source_identity,
        ]
        for component, endpoint in components.items():
            command.extend((f"--{component.replace('_', '-')}-url", endpoint))
        self.external_started.append(external_gate)
        self._status("running")
        self._execute(command, external_gate)
        manifest = external_root / "evidence.json"
        validated = validate_evidence(
            manifest,
            external_root,
            external_gate,
            components,
            self.source_identity,
            driver_digest,
        )
        atomic_json(external_root / "validated-evidence.json", validated)
        self.external_completed.append(external_gate)
        self._status("running")

    def _report(self, state: str, failure: str | None) -> None:
        if self.gate not in ("human-qualify", "platform-qualify"):
            return
        compatibility = self.root / "human" / "schema" / "human-api" / "compatibility.kvx"
        compatibility_record: dict[str, str] | None = None
        if compatibility.is_file():
            copied = self.output / "artifacts" / "human-api-compatibility.kvx"
            copied.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(compatibility, copied)
            compatibility_record = {
                "path": str(copied.relative_to(self.output)),
                "sha256": file_digest(copied),
            }
        guarantees = [
            {
                "gate": "receipt and canonical-ledger verification",
                "layer": "protocol-enforced",
            },
            {
                "gate": "authenticated preparation, submission and tracking",
                "layer": "agent-layer-enforced",
            },
            {
                "gate": "custody, journeys and verified rendering",
                "layer": "human-plane-enforced",
            },
        ]
        if self.gate == "platform-qualify":
            guarantees.extend(
                (
                    {
                        "gate": "SDK and developer surface parity",
                        "layer": "developer-platform-enforced",
                    },
                    {
                        "gate": "Programs monetary law and isolation",
                        "layer": "programs-plane-enforced",
                    },
                    {
                        "gate": "interop and multichain verification",
                        "layer": "interop-plane-enforced",
                    },
                )
            )
        completed_argv = {" ".join(record["argv"]): record for record in self.commands}
        unmet = []
        for planned in self.planned:
            record = completed_argv.get(planned)
            if record is None:
                unmet.append({"gate": planned, "state": "not-run"})
            elif record["exit_code"] != 0:
                unmet.append({"gate": planned, "state": "failed"})
        for external_gate in self.external_planned:
            if external_gate not in self.external_completed:
                state = "failed" if external_gate in self.external_started else "not-run"
                unmet.append({"gate": external_gate, "state": state})
        if failure and not unmet:
            unmet.append({"gate": "external evidence", "state": "failed", "detail": failure})
        external_evidence = []
        for external_gate in self.external_completed:
            evidence = self.output / "external" / external_gate / "validated-evidence.json"
            external_evidence.append(
                {
                    "gate": external_gate,
                    "path": str(evidence.relative_to(self.output)),
                    "sha256": file_digest(evidence),
                }
            )
        atomic_json(
            self.output / "report.json",
            {
                "compatibility_matrix": compatibility_record,
                "external_evidence": external_evidence,
                "failure": failure,
                "gate": self.gate,
                "guarantees": guarantees,
                "release_allowed": state == "passed" and not unmet,
                "schema": "layerx-qualification-report-v1",
                "source_identity": self.source_identity,
                "source_revision": self.source_revision,
                "state": state,
                "unmet_gates": unmet,
            },
        )

    def run(self) -> int:
        if self.output.is_symlink():
            print(
                f"qualification failed: refusing symlinked qualification output {self.output}",
                file=sys.stderr,
            )
            return 1
        self.output.mkdir(parents=True, exist_ok=True)
        self._status("running")
        try:
            driver, driver_digest, components = self._preflight()
            for index, command in enumerate(LOCAL_COMMANDS[self.gate], start=1):
                self._execute(command, f"local-{index:02d}")
                if self._compute_source_identity() != self.source_identity:
                    raise QualificationFailure("a qualification command changed the source tree")
            if self.gate in EVIDENCE_SPECS:
                self._external(self.gate, driver, driver_digest, components)
                if self._compute_source_identity() != self.source_identity:
                    raise QualificationFailure("the qualification driver changed the source tree")
            for external_gate in EXTERNAL_GATES.get(self.gate, ()):
                self._external(external_gate, driver, driver_digest, components)
                if self._compute_source_identity() != self.source_identity:
                    raise QualificationFailure("the qualification driver changed the source tree")
        except QualificationFailure as error:
            failure = str(error)
            self._report("failed", failure)
            self._status("failed", failure)
            print(f"qualification failed: {failure}", file=sys.stderr)
            return 1
        self._report("passed", None)
        self._status("passed")
        print(f"qualification passed: {self.output}")
        return 0


def repository_root() -> Path:
    return Path(__file__).resolve().parents[2]


def main(arguments: Sequence[str]) -> int:
    if len(arguments) != 1:
        print("usage: release_runner.py <qualification-target>", file=sys.stderr)
        return 2
    try:
        runner = ReleaseRunner(repository_root(), arguments[0], os.environ)
    except QualificationFailure as error:
        print(f"qualification failed: {error}", file=sys.stderr)
        return 1
    return runner.run()


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
