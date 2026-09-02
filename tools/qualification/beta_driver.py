#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import time
from collections.abc import Callable, Mapping, Sequence
from datetime import datetime, timezone
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.qualification.release_runner import (
    EVIDENCE_SPECS,
    SCHEMA,
    QualificationFailure,
    atomic_json,
    file_digest,
    validated_url,
)

DRIVER_GATES = (
    "platform-qualify-adoption",
    "programs-qualify",
    "interop-qualify",
    "multichain-qualify",
)
STACK_FLAGS = (
    ("node", "--node-url"),
    ("agentd", "--agentd-url"),
    ("human_service", "--human-service-url"),
    ("paxeer_testnet", "--paxeer-testnet-url"),
)
MAKE = ("make", "--no-print-directory")
FIVE_MINUTES_SECONDS = 300.0
HTTP_TIMEOUT_SECONDS = 20.0
TAIL_LINES = 40
SAMPLES = ROOT / "platform" / "docs" / "samples"
SAMPLE_INPUTS = (
    "LAYERX_API_TOKEN",
    "LAYERX_SOURCE",
    "LAYERX_DESTINATION",
    "LAYERX_AMOUNT",
    "LAYERX_CURRENCY",
)
CLI_INPUTS = ("LAYERX_NETWORK_ID", "LAYERX_SEQUENCER_TRUST_ANCHOR")
CLI_ENVIRONMENT_INPUT = "LAYERX_CLI_ENVIRONMENT"
CLI_BINARY_INPUT = "LAYERX_BIN"
SETTLED_STATES = frozenset(("done", "done-finalised"))
MIRROR_VERIFY_INPUTS = (
    "LAYERX_MIRROR_VERIFY_CONFIG",
    "LAYERX_MIRROR_CANONICAL_REQUEST",
    "LAYERX_MIRROR_FAILOVER_REQUEST",
    "LAYERX_MIRROR_DIVERGENCE_REQUEST",
    "LAYERX_MIRROR_TAMPER_REQUEST",
    "LAYERX_MIRROR_TS_CONFORMANCE",
    "LAYERX_MIRROR_PYTHON_CONFORMANCE",
    "LAYERX_MIRROR_GO_CONFORMANCE",
    "LAYERX_MIRROR_JVM_CONFORMANCE",
    "LAYERX_MIRROR_SWIFT_CONFORMANCE",
    "LAYERX_MIRROR_DOTNET_CONFORMANCE",
)
MIRROR_TAMPER_INPUTS = (
    "LAYERX_MIRROR_VERIFY_CONFIG",
    "LAYERX_MIRROR_CANONICAL_REQUEST",
    "LAYERX_MIRROR_TAMPER_REQUEST",
)
MIRROR_LIVE_FORBIDDEN = ("LAYERX_NODE_URL", "LAYERX_GATEWAY_URL", "LAYERX_EXPLORER_API_ORIGIN")
MIGRATION_INPUTS = (
    "LAYERX_ETHEREUM_CONFIG",
    "LAYERX_ETHEREUM_ASSET_EVIDENCE",
    "LAYERX_ETHEREUM_HISTORY_EVIDENCE",
    "LAYERX_ETHEREUM_OWNERSHIP_EVIDENCE",
    "LAYERX_SOLANA_CONFIG",
    "LAYERX_SOLANA_ASSET_EVIDENCE",
    "LAYERX_SOLANA_HISTORY_EVIDENCE",
    "LAYERX_SOLANA_OWNERSHIP_EVIDENCE",
    "LAYERX_MIGRATION_SECRET_DIR",
)
RAMP_INPUTS = (
    "LAYERX_RAMP_URL",
    "LAYERX_RAMP_CA_PEM",
    "LAYERX_RAMP_CUSTOMER_TOKEN",
    "LAYERX_RAMP_OPERATOR_URL",
    "LAYERX_RAMP_OPERATOR_TOKEN",
    "LAYERX_RAMP_ON_QUOTE_ID",
    "LAYERX_RAMP_OFF_QUOTE_ID",
    "LAYERX_RAMP_OFF_GRANT_JSON",
    "LAYERX_RAMP_ON_ACCOUNT_SEQUENCE",
    "LAYERX_RAMP_OFF_RECEIVER_SEQUENCE",
)
PAXEER_CHAIN_INPUT = "LAYERX_PAXEER_CHAIN_ID"
CARGO_TEST_LINE = re.compile(r"^test (\S+) \.\.\. ok$", re.MULTILINE)
PIN_CONSTANT = re.compile(r'const ([A-Z][A-Z0-9_]*): &str = "([^"]*)";')
FAULT_BOUNDARY = re.compile(r"^\s*(LXP_FAULT_[A-Z_]+) = (\d+)", re.MULTILINE)
GAUNTLET_INVENTORY = ROOT / "programs" / "tests" / "gauntlet" / "attack-inventory.tsv"
FAULT_HEADER = ROOT / "include" / "layerx" / "lxp_fault.h"
PROGRAMS_DIRECTORY = ROOT / "programs"
AGENT_MANIFEST = ROOT / "agent" / "Cargo.toml"
HUMAN_MANIFEST = ROOT / "human" / "Cargo.toml"
INTEROP_MANIFEST = ROOT / "interop" / "Cargo.toml"
PLATFORM_MANIFEST = ROOT / "platform" / "Cargo.toml"
SCHEMA_CHECK_MANIFEST = ROOT / "human" / "tools" / "schema-check" / "Cargo.toml"


class CaseFailure(Exception):
    pass


class Stack:
    def __init__(self, node: str, agentd: str, human_service: str, paxeer_testnet: str) -> None:
        self.node = node
        self.agentd = agentd
        self.human_service = human_service
        self.paxeer_testnet = paxeer_testnet

    def components(self) -> dict[str, str]:
        return {
            "node": self.node,
            "agentd": self.agentd,
            "human_service": self.human_service,
            "paxeer_testnet": self.paxeer_testnet,
        }


class Execution:
    def __init__(
        self,
        label: str,
        argv: Sequence[str],
        cwd: Path,
        exit_code: int,
        log: Path,
        output: str,
        elapsed_seconds: float,
    ) -> None:
        self.label = label
        self.argv = tuple(argv)
        self.cwd = cwd
        self.exit_code = exit_code
        self.log = log
        self.output = output
        self.elapsed_seconds = elapsed_seconds

    def tail(self) -> str:
        lines = self.output.rstrip().splitlines()
        return "\n".join(lines[-TAIL_LINES:])

    def cargo_tests(self) -> list[str]:
        return CARGO_TEST_LINE.findall(self.output)

    def last_json(self) -> dict[str, object]:
        for line in reversed(self.output.splitlines()):
            candidate = line.strip()
            if not candidate.startswith("{"):
                continue
            try:
                parsed = json.loads(candidate)
            except json.JSONDecodeError:
                continue
            if isinstance(parsed, dict):
                return parsed
        raise CaseFailure(f"{self.label} printed no JSON object")


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def json_bytes(payload: object) -> bytes:
    return (json.dumps(payload, indent=2, sort_keys=True) + "\n").encode("utf-8")


def slug(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.-]+", "-", value).strip("-")


def bytes_digest(content: bytes) -> str:
    return hashlib.sha256(content).hexdigest()


class CaseContext:
    def __init__(
        self,
        gate: str,
        case_id: str,
        output_root: Path,
        stack: Stack,
        source_identity: str,
        environment: Mapping[str, str],
    ) -> None:
        self.gate = gate
        self.case_id = case_id
        self.output_root = output_root
        self.stack = stack
        self.source_identity = source_identity
        self.environment = dict(environment)
        self.directory = output_root / "cases" / slug(case_id)
        self.directory.mkdir(parents=True, exist_ok=False)
        self.artifacts: list[dict[str, str]] = []
        self.executions: list[Execution] = []
        self.started_at = utc_now()
        self.started = time.monotonic()

    def elapsed(self) -> float:
        return time.monotonic() - self.started

    def relative(self, path: Path) -> str:
        return path.resolve().relative_to(self.output_root.resolve()).as_posix()

    def require_env(self, *names: str) -> dict[str, str]:
        values: dict[str, str] = {}
        missing: list[str] = []
        for name in names:
            value = self.environment.get(name, "")
            if value:
                values[name] = value
            else:
                missing.append(name)
        if missing:
            raise CaseFailure(
                "owner input missing; set " + ", ".join(missing) + " in the driver environment"
            )
        return values

    def require_file(self, path: Path, description: str) -> Path:
        if not path.is_file():
            raise CaseFailure(f"{description} not found at {path}")
        return path

    def require_executable(self, path: Path, description: str) -> Path:
        if not path.is_file() or not os.access(path, os.X_OK):
            raise CaseFailure(f"{description} is not an executable file at {path}")
        return path

    def run(
        self,
        argv: Sequence[str],
        *,
        label: str,
        cwd: Path = ROOT,
        env: Mapping[str, str] | None = None,
        stdin_text: str | None = None,
        timeout: float | None = None,
        allow_failure: bool = False,
    ) -> Execution:
        log_directory = self.directory / "logs"
        log_directory.mkdir(parents=True, exist_ok=True)
        log = log_directory / f"{len(self.executions) + 1:02d}-{slug(label)}.log"
        started = time.monotonic()
        print(f"case {self.case_id}: $ {' '.join(argv)}", flush=True)
        try:
            completed = subprocess.run(
                list(argv),
                cwd=cwd,
                env=dict(env) if env is not None else self.environment,
                input=stdin_text.encode("utf-8") if stdin_text is not None else None,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=timeout,
                check=False,
            )
        except FileNotFoundError as error:
            raise CaseFailure(f"{label} could not start: {error}") from error
        except subprocess.TimeoutExpired as error:
            partial = error.stdout or b""
            log.write_bytes(partial)
            raise CaseFailure(f"{label} exceeded {timeout} seconds") from error
        elapsed = time.monotonic() - started
        header = (
            f"$ {' '.join(argv)}\ncwd: {cwd}\nexit: {completed.returncode}\n"
            f"elapsed_seconds: {elapsed:.3f}\n\n"
        ).encode("utf-8")
        log.write_bytes(header + completed.stdout)
        execution = Execution(
            label,
            argv,
            cwd,
            completed.returncode,
            log,
            completed.stdout.decode("utf-8", errors="replace"),
            elapsed,
        )
        self.executions.append(execution)
        if completed.returncode != 0 and not allow_failure:
            raise CaseFailure(
                f"{label} exited {completed.returncode}: {' '.join(argv)}\n{execution.tail()}"
            )
        return execution

    def make(self, *targets: str, label: str | None = None, env: Mapping[str, str] | None = None) -> Execution:
        return self.run(MAKE + targets, label=label or "make-" + "-".join(targets), env=env)

    def cargo_test(
        self,
        manifest: Path,
        package: str,
        *arguments: str,
        label: str,
        cwd: Path = ROOT,
        env: Mapping[str, str] | None = None,
    ) -> Execution:
        argv = ("cargo", "test", "--manifest-path", str(manifest), "--locked", "-p", package, *arguments)
        execution = self.run(argv, label=label, cwd=cwd, env=env)
        if not execution.cargo_tests():
            raise CaseFailure(f"{label} executed no test")
        return execution

    def http(
        self,
        url: str,
        *,
        method: str = "GET",
        payload: object | None = None,
        headers: Mapping[str, str] | None = None,
    ) -> tuple[int, bytes]:
        body = json_bytes(payload) if payload is not None else None
        request = Request(url, data=body, method=method)
        request.add_header("Accept", "application/json")
        if body is not None:
            request.add_header("Content-Type", "application/json")
        for name, value in (headers or {}).items():
            request.add_header(name, value)
        try:
            with urlopen(request, timeout=HTTP_TIMEOUT_SECONDS) as response:
                return response.status, response.read()
        except HTTPError as error:
            return error.code, error.read()
        except (URLError, OSError, ValueError) as error:
            raise CaseFailure(f"{method} {url} is unreachable: {error}") from error

    def http_json(self, url: str, **arguments: object) -> dict[str, object]:
        status, body = self.http(url, **arguments)
        if status < 200 or status >= 300:
            raise CaseFailure(f"{url} answered HTTP {status}: {body[:512].decode('utf-8', 'replace')}")
        try:
            parsed = json.loads(body)
        except json.JSONDecodeError as error:
            raise CaseFailure(f"{url} did not answer JSON: {error}") from error
        if not isinstance(parsed, dict):
            raise CaseFailure(f"{url} did not answer a JSON object")
        return parsed

    def register(self, kind: str, path: Path) -> Path:
        if any(artifact["kind"] == kind for artifact in self.artifacts):
            raise CaseFailure(f"artifact kind {kind} registered twice")
        if not path.is_file() or path.stat().st_size == 0:
            raise CaseFailure(f"artifact {kind} at {path} is not a non-empty file")
        self.artifacts.append(
            {"kind": kind, "path": self.relative(path), "sha256": file_digest(path)}
        )
        return path

    def artifact(self, kind: str, name: str, content: bytes) -> Path:
        directory = self.directory / "artifacts"
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / name
        if path.exists():
            raise CaseFailure(f"artifact file {name} already exists")
        path.write_bytes(content)
        return self.register(kind, path)

    def artifact_json(self, kind: str, name: str, payload: object) -> Path:
        return self.artifact(kind, name, json_bytes(payload))

    def artifact_copy(self, kind: str, source: Path) -> Path:
        directory = self.directory / "artifacts"
        directory.mkdir(parents=True, exist_ok=True)
        destination = directory / f"{kind}-{source.name}"
        if destination.exists():
            raise CaseFailure(f"artifact file {destination.name} already exists")
        shutil.copyfile(source, destination)
        return self.register(kind, destination)

    def artifact_log(self, kind: str, execution: Execution) -> Path:
        return self.register(kind, execution.log)

    def execution_records(self) -> list[dict[str, object]]:
        return [
            {
                "label": execution.label,
                "argv": list(execution.argv),
                "cwd": str(execution.cwd),
                "exit_code": execution.exit_code,
                "elapsed_seconds": round(execution.elapsed_seconds, 3),
                "log": self.relative(execution.log),
                "log_sha256": file_digest(execution.log),
            }
            for execution in self.executions
        ]

    def result(self, **details: object) -> Path:
        payload = {
            "case": self.case_id,
            "gate": self.gate,
            "source_identity": self.source_identity,
            "components": self.stack.components(),
            "executions": self.execution_records(),
            **details,
        }
        return self.artifact_json("result", "result.json", payload)

    def timing(self, bound_seconds: float) -> Path:
        elapsed = self.elapsed()
        payload = {
            "case": self.case_id,
            "started_at": self.started_at,
            "finished_at": utc_now(),
            "elapsed_seconds": round(elapsed, 3),
            "bound_seconds": bound_seconds,
            "within_bound": elapsed <= bound_seconds,
            "steps": [
                {"label": execution.label, "elapsed_seconds": round(execution.elapsed_seconds, 3)}
                for execution in self.executions
            ],
        }
        path = self.artifact_json("timing", "timing.json", payload)
        if elapsed > bound_seconds:
            raise CaseFailure(f"took {elapsed:.1f} seconds, over the {bound_seconds:.0f} second bound")
        return path

    def finish(self, kinds: frozenset[str]) -> None:
        present = {artifact["kind"] for artifact in self.artifacts}
        missing = sorted(kinds - present)
        if missing:
            raise CaseFailure("case produced no artifact of kind " + ", ".join(missing))


CaseFunction = Callable[[CaseContext], None]
CASES: dict[str, dict[str, CaseFunction]] = {gate: {} for gate in DRIVER_GATES}


def case(gate: str, case_id: str) -> Callable[[CaseFunction], CaseFunction]:
    def register(function: CaseFunction) -> CaseFunction:
        if case_id in CASES[gate]:
            raise ValueError(f"case {case_id} registered twice for {gate}")
        CASES[gate][case_id] = function
        return function

    return register


def crate_identity(directory: Path) -> dict[str, object]:
    manifest = directory / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    name = re.search(r'^name = "([^"]+)"', text, re.MULTILINE)
    version = re.search(r'^version = "([^"]+)"', text, re.MULTILINE)
    workspace_version = re.search(r"^version(?:\.workspace)? = true", text, re.MULTILINE)
    pins: dict[str, str] = {}
    for source in sorted((directory / "src").glob("*.rs")):
        pins.update(PIN_CONSTANT.findall(source.read_text(encoding="utf-8")))
    return {
        "crate": name.group(1) if name else directory.name,
        "version": version.group(1) if version else ("workspace" if workspace_version else None),
        "manifest": str(manifest.relative_to(ROOT)),
        "manifest_sha256": file_digest(manifest),
        "pinned_constants": pins,
    }


def file_inventory(paths: Sequence[Path]) -> list[dict[str, object]]:
    return [
        {
            "path": str(path.relative_to(ROOT)),
            "sha256": file_digest(path),
            "bytes": path.stat().st_size,
        }
        for path in paths
    ]


def node_package(sample: Path, name: str) -> dict[str, object]:
    for parent in (sample, *sample.parents):
        manifest = parent / "node_modules" / name / "package.json"
        if manifest.is_file():
            resolved = manifest.resolve()
            metadata = json.loads(resolved.read_text(encoding="utf-8"))
            record: dict[str, object] = {
                "package": metadata.get("name"),
                "version": metadata.get("version"),
                "resolved_from": str(manifest),
                "path": str(resolved),
                "package_json_sha256": file_digest(resolved),
            }
            main = metadata.get("main")
            if isinstance(main, str) and (resolved.parent / main).is_file():
                record["main"] = main
                record["main_sha256"] = file_digest(resolved.parent / main)
            return record
    raise CaseFailure(f"{name} is not installed for {sample.relative_to(ROOT)}; run make platform-sample-install")


def sample_environment(ctx: CaseContext, ecosystem: str) -> dict[str, str]:
    inputs = ctx.require_env(*SAMPLE_INPUTS)
    environment = dict(ctx.environment)
    environment.update(inputs)
    environment["LAYERX_API_URL"] = ctx.stack.human_service
    environment["LAYERX_PAYMENT_KEY"] = f"beta-{ecosystem}-first-payment-{secrets.token_hex(8)}"
    return environment


def settled_journey(execution: Execution) -> dict[str, object]:
    journey = execution.last_json()
    state = journey.get("state")
    if state not in SETTLED_STATES:
        raise CaseFailure(f"{execution.label} settled in state {state!r}: {json.dumps(journey)}")
    receipts = journey.get("receipts")
    if not isinstance(receipts, list) or not receipts:
        raise CaseFailure(f"{execution.label} reported no receipt evidence: {json.dumps(journey)}")
    return journey


def first_payment(
    ctx: CaseContext,
    ecosystem: str,
    sample: Path,
    published: dict[str, object],
    steps: Sequence[tuple[str, Sequence[str]]],
    environment: Mapping[str, str],
) -> None:
    execution: Execution | None = None
    for label, argv in steps:
        execution = ctx.run(
            argv, label=label, cwd=sample, env=environment, timeout=FIVE_MINUTES_SECONDS
        )
    assert execution is not None
    journey = settled_journey(execution)
    ctx.artifact_json(
        "published-artifact",
        "published-artifact.json",
        {"ecosystem": ecosystem, "sample": str(sample.relative_to(ROOT)), **published},
    )
    ctx.timing(FIVE_MINUTES_SECONDS)
    ctx.result(
        ecosystem=ecosystem,
        endpoint=ctx.stack.human_service,
        payment_key=environment["LAYERX_PAYMENT_KEY"],
        journey=journey,
    )


@case("platform-qualify-adoption", "typescript/first-payment")
def typescript_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-typescript"
    environment = sample_environment(ctx, "typescript")
    published = {
        "packages": [
            node_package(sample, "@sidiora/layerx-sdk"),
            node_package(sample, "@sidiora/layerx-buyer-middleware"),
        ],
        "node": ctx.run(("node", "--version"), label="node-version").output.strip(),
    }
    first_payment(ctx, "typescript", sample, published, (("run", ("node", "index.mjs")),), environment)


@case("platform-qualify-adoption", "python/first-payment")
def python_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-python"
    environment = sample_environment(ctx, "python")
    interpreter = Path(ctx.environment.get("PLATFORM_PYTHON_ENV", str(ROOT / "build" / "platform-python"))) / "bin" / "python"
    if not interpreter.is_file():
        raise CaseFailure(
            f"python sample dependencies are not installed at {interpreter}; run make platform-sample-install"
        )
    identity = ctx.run(
        (
            str(interpreter),
            "-c",
            "import importlib.metadata, json, layerx_sdk, layerx_transport\n"
            "print(json.dumps({'distribution': 'layerx-sdk', 'version': importlib.metadata.version('layerx-sdk'),"
            " 'layerx_sdk': layerx_sdk.__file__, 'layerx_transport': layerx_transport.__file__}))",
        ),
        label="sdk-identity",
        cwd=sample,
    ).last_json()
    published = {"interpreter": str(interpreter), "distribution": identity}
    first_payment(ctx, "python", sample, published, (("run", (str(interpreter), "main.py")),), environment)


@case("platform-qualify-adoption", "go/first-payment")
def go_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-go"
    environment = sample_environment(ctx, "go")
    environment["GOPROXY"] = "off"
    module = None
    for line in (sample / "go.mod").read_text(encoding="utf-8").splitlines():
        if line.startswith("require "):
            module = line.split()[1]
            break
    if module is None:
        raise CaseFailure("go sample declares no required LayerX module")
    listing = ctx.run(
        ("go", "list", "-m", "-json", module), label="sdk-identity", cwd=sample, env=environment
    ).last_json()
    published = {"module": listing, "go": ctx.run(("go", "version"), label="go-version").output.strip()}
    first_payment(ctx, "go", sample, published, (("run", ("go", "run", ".")),), environment)


@case("platform-qualify-adoption", "jvm/first-payment")
def jvm_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-jvm"
    environment = sample_environment(ctx, "jvm")
    pom = (sample / "pom.xml").read_text(encoding="utf-8")
    match = re.search(r"<artifactId>layerx-sdk</artifactId>\s*<version>([^<]+)</version>", pom)
    if match is None:
        raise CaseFailure("jvm sample does not depend on com.sidiora.layerx:layerx-sdk")
    version = match.group(1)
    repository = Path(
        ctx.environment.get("PLATFORM_MAVEN_REPOSITORY", str(Path.home() / ".m2" / "repository"))
    )
    jar = repository / "com" / "sidiora" / "layerx" / "layerx-sdk" / version / f"layerx-sdk-{version}.jar"
    ctx.require_file(jar, f"com.sidiora.layerx:layerx-sdk:{version}")
    published = {
        "artifact": f"com.sidiora.layerx:layerx-sdk:{version}",
        "jar": str(jar),
        "jar_sha256": file_digest(jar),
    }
    first_payment(
        ctx,
        "jvm",
        sample,
        published,
        (("build", ("mvn", "-o", "-q", "package")), ("run", ("mvn", "-o", "-q", "exec:java"))),
        environment,
    )


@case("platform-qualify-adoption", "swift/first-payment")
def swift_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-swift"
    environment = sample_environment(ctx, "swift")
    ctx.run(
        ("swift", "build"), label="build", cwd=sample, env=environment, timeout=FIVE_MINUTES_SECONDS
    )
    dependencies = ctx.run(
        ("swift", "package", "show-dependencies", "--format", "json"),
        label="sdk-identity",
        cwd=sample,
        env=environment,
    )
    published = {
        "dependencies": json.loads(dependencies.output[dependencies.output.index("{") :]),
        "swift": ctx.run(("swift", "--version"), label="swift-version").output.strip(),
    }
    first_payment(ctx, "swift", sample, published, (("run", ("swift", "run", "FirstPayment")),), environment)


@case("platform-qualify-adoption", "dotnet/first-payment")
def dotnet_first_payment(ctx: CaseContext) -> None:
    sample = SAMPLES / "first-payment-csharp"
    environment = sample_environment(ctx, "dotnet")
    environment.setdefault("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
    ctx.run(
        ("dotnet", "build", "--configuration", "Release"),
        label="build",
        cwd=sample,
        env=environment,
        timeout=FIVE_MINUTES_SECONDS,
    )
    assemblies = sorted(sample.glob("bin/Release/*/LayerX.Sdk.dll"))
    if not assemblies:
        raise CaseFailure("dotnet build produced no LayerX.Sdk.dll under bin/Release")
    published = {
        "assemblies": [{"path": str(path), "sha256": file_digest(path)} for path in assemblies],
        "dotnet": ctx.run(("dotnet", "--version"), label="dotnet-version").output.strip(),
    }
    first_payment(
        ctx,
        "dotnet",
        sample,
        published,
        (("run", ("dotnet", "run", "--configuration", "Release")),),
        environment,
    )


def layerx_binary(ctx: CaseContext) -> Path:
    configured = ctx.environment.get(CLI_BINARY_INPUT, "")
    if configured:
        return ctx.require_executable(Path(configured), CLI_BINARY_INPUT)
    located = shutil.which("layerx")
    if located is None:
        raise CaseFailure(f"layerx CLI not found; set {CLI_BINARY_INPUT} or put layerx on PATH")
    return Path(located)


def cli_environment(ctx: CaseContext, endpoint: str) -> tuple[Path, dict[str, str], str]:
    binary = layerx_binary(ctx)
    inputs = ctx.require_env(*CLI_INPUTS)
    environment = dict(ctx.environment)
    home = ctx.directory / "cli"
    home.mkdir(exist_ok=True)
    environment["LAYERX_CONFIG"] = str(home / "config.json")
    environment["LAYERX_INSTALL_ROOT"] = str(home)
    name = ctx.environment.get(CLI_ENVIRONMENT_INPUT, "emulator")
    ctx.run(
        (
            str(binary),
            "--json",
            "environment",
            "use",
            name,
            "--endpoint",
            endpoint,
            "--network-id",
            inputs["LAYERX_NETWORK_ID"],
            "--sequencer-trust-anchor",
            inputs["LAYERX_SEQUENCER_TRUST_ANCHOR"],
        ),
        label="environment-use",
        env=environment,
    )
    return binary, environment, name


def cli_output(execution: Execution, kind: str) -> dict[str, object]:
    document = execution.last_json()
    if document.get("ok") is not True or document.get("kind") != kind:
        raise CaseFailure(f"{execution.label} did not report {kind}: {json.dumps(document)}")
    data = document.get("data")
    if not isinstance(data, dict):
        raise CaseFailure(f"{execution.label} carried no data object")
    return data


@case("platform-qualify-adoption", "rust/first-payment")
def rust_first_payment(ctx: CaseContext) -> None:
    inputs = ctx.require_env(*SAMPLE_INPUTS)
    binary, environment, name = cli_environment(ctx, ctx.stack.human_service)
    ctx.run(
        (str(binary), "--json", "auth", "set", "--environment", name),
        label="auth-set",
        env=environment,
        stdin_text=inputs["LAYERX_API_TOKEN"] + "\n",
    )
    payment_key = f"beta-rust-first-payment-{secrets.token_hex(8)}"
    payment = cli_output(
        ctx.run(
            (
                str(binary),
                "--json",
                "payment",
                "test",
                "--from",
                inputs["LAYERX_SOURCE"],
                "--to",
                inputs["LAYERX_DESTINATION"],
                "--currency",
                inputs["LAYERX_CURRENCY"],
                "--amount",
                inputs["LAYERX_AMOUNT"],
                "--idempotency-key",
                payment_key,
            ),
            label="payment-test",
            env=environment,
        ),
        "payment.started",
    )
    journey = payment.get("journey")
    result = journey.get("result") if isinstance(journey, dict) else None
    if not isinstance(result, dict) or result.get("state") not in SETTLED_STATES:
        raise CaseFailure(f"payment test did not settle: {json.dumps(payment)}")
    evidence = result.get("evidence")
    if not isinstance(evidence, list) or not evidence:
        raise CaseFailure(f"payment test carried no receipt evidence: {json.dumps(result)}")
    first = evidence[0]
    if not isinstance(first, dict) or first.get("verification") != "receipt-verified":
        raise CaseFailure(f"payment evidence is not receipt-verified: {json.dumps(first)}")
    evidence_id = first.get("evidence_id")
    if not isinstance(evidence_id, str) or not evidence_id:
        raise CaseFailure("payment evidence carries no evidence_id")
    source_ref = first.get("source_ref")
    receipt_id = (
        source_ref.rsplit("/", 1)[1]
        if isinstance(source_ref, str) and source_ref.startswith("/v1/receipts/")
        else evidence_id
    )
    receipt = cli_output(
        ctx.run(
            (str(binary), "--json", "receipt", "get", receipt_id),
            label="receipt-get",
            env=environment,
        ),
        "receipt.read",
    )
    receipt_result = receipt.get("result")
    if not isinstance(receipt_result, dict):
        raise CaseFailure(f"receipt read carried no result envelope: {json.dumps(receipt)}")
    receipt_hex = receipt_result.get("receipt")
    if not isinstance(receipt_hex, str) or not receipt_hex:
        raise CaseFailure(f"receipt read returned no receipt bytes: {json.dumps(receipt)}")
    verification: dict[str, object] = {"performed": False, "reason": "receipt read carried no authority facts"}
    authority = receipt_result.get("authority")
    if isinstance(authority, dict):
        receipt_file = ctx.directory / "cli" / "receipt.hex"
        receipt_file.write_text(receipt_hex + "\n", encoding="utf-8")
        verified = cli_output(
            ctx.run(
                (
                    str(binary),
                    "--json",
                    "receipt",
                    "verify",
                    "--receipt",
                    str(receipt_file),
                    "--batch-id",
                    str(authority.get("batch_id")),
                    "--asset",
                    str(authority.get("asset")),
                    "--previous-state-root",
                    str(authority.get("previous_state_root")),
                    "--resulting-state-root",
                    str(authority.get("resulting_state_root")),
                    "--sequencer-public-key",
                    str(authority.get("sequencer_public_key")),
                ),
                label="receipt-verify",
                env=environment,
            ),
            "receipt.verified",
        )
        verification = {"performed": True, "verified": verified}
    version = ctx.run((str(binary), "--version"), label="cli-version").output.strip()
    ctx.artifact_json(
        "published-artifact",
        "published-artifact.json",
        {
            "ecosystem": "rust",
            "binary": str(binary),
            "binary_sha256": file_digest(binary),
            "version": version,
            "credential_store": ctx.environment.get("LAYERX_CREDENTIAL_STORE", "operating-system"),
        },
    )
    ctx.timing(FIVE_MINUTES_SECONDS)
    ctx.result(
        ecosystem="rust",
        endpoint=ctx.stack.human_service,
        payment_key=payment_key,
        journey=result,
        receipt={
            "evidence_id": evidence_id,
            "receipt_id": receipt_id,
            "receipt_sha256": bytes_digest(receipt_hex.encode("utf-8")),
        },
        local_verification=verification,
    )


@case("platform-qualify-adoption", "middleware/ten-line-integration")
def middleware_ten_line_integration(ctx: CaseContext) -> None:
    build_site = ROOT / "platform" / "docs" / "build" / "build_site.py"
    specification = importlib.util.spec_from_file_location("layerx_build_site", build_site)
    if specification is None or specification.loader is None:
        raise CaseFailure(f"could not load {build_site}")
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    try:
        samples = module.load_samples(ROOT / "platform" / "docs")
        measurements = module.measure_samples(samples)
    except module.DocumentationError as error:
        raise CaseFailure(f"documentation line budget failed: {error}") from error
    if not measurements:
        raise CaseFailure("the documentation gate measured no sample")
    workspaces = (
        "@sidiora/layerx-seller-middleware",
        "@sidiora/layerx-buyer-middleware",
        "@sidiora/layerx-merchant-middleware",
        "@sidiora/layerx-agent-middleware",
        "@sidiora/layerx-middleware-conformance",
    )
    for workspace in workspaces:
        ctx.run(
            ("npm", "run", "build", "--workspace", workspace),
            label=f"build-{workspace.split('/')[-1]}",
            timeout=FIVE_MINUTES_SECONDS,
        )
    conformance = ctx.run(
        ("npm", "run", "conformance", "--workspace", "@sidiora/layerx-middleware-conformance"),
        label="conformance",
        timeout=FIVE_MINUTES_SECONDS,
    )
    sample = SAMPLES / "first-payment-typescript"
    ctx.artifact_json(
        "published-artifact",
        "published-artifact.json",
        {
            "ecosystem": "middleware",
            "packages": [
                node_package(sample, "@sidiora/layerx-buyer-middleware"),
                node_package(sample, "@sidiora/layerx-seller-middleware"),
                node_package(sample, "@sidiora/layerx-merchant-middleware"),
                node_package(sample, "@sidiora/layerx-agent-middleware"),
            ],
        },
    )
    ctx.timing(FIVE_MINUTES_SECONDS)
    ctx.result(
        measured_region="integration",
        measurements=measurements,
        conformance={"log": ctx.relative(conformance.log), "tail": conformance.tail()},
    )


@case("platform-qualify-adoption", "programs/five-minute-deploy-and-paid-call")
def programs_five_minute_deploy_and_paid_call(ctx: CaseContext) -> None:
    quickstart = ROOT / "programs" / "sdk" / "rust" / "quickstart"
    binary, environment, _ = cli_environment(ctx, ctx.stack.node)
    registry = cli_output(
        ctx.run(
            (str(binary), "--json", "program", "registry", "list"),
            label="registry-list",
            env=environment,
        ),
        "program.registry_list",
    )
    build_environment = {
        name: value for name, value in ctx.environment.items() if name != "CARGO_TARGET_DIR"
    }
    build = ctx.run(
        ("sh", str(quickstart / "build.sh"), "all"),
        label="quickstart-build",
        env=build_environment,
        timeout=FIVE_MINUTES_SECONDS,
    )
    printed = [line.strip() for line in build.output.splitlines() if line.strip()]
    if not printed:
        raise CaseFailure("quickstart build printed no artifact path")
    wasm = ctx.require_file(Path(printed[-1]), "quickstart artifact layerx_quickstart.wasm")
    deployment_key = f"beta-programs-deploy-{secrets.token_hex(8)}"
    deployment = cli_output(
        ctx.run(
            (
                str(binary),
                "--json",
                "program",
                "deploy",
                str(wasm),
                "--idempotency-key",
                deployment_key,
            ),
            label="program-deploy",
            env=environment,
        ),
        "program.deployment_started",
    )
    walkthrough_environment = dict(environment)
    walkthrough_environment["PATH"] = f"{binary.parent}{os.pathsep}{environment.get('PATH', '')}"
    walkthrough = ctx.run(
        ("bash", str(ROOT / "platform" / "examples" / "agent-program-call" / "walkthrough.sh")),
        label="walkthrough",
        env=walkthrough_environment,
    )
    ctx.artifact_json(
        "published-artifact",
        "published-artifact.json",
        {
            "ecosystem": "programs",
            "artifact": str(wasm),
            "artifact_sha256": file_digest(wasm),
            "cli": str(binary),
            "cli_sha256": file_digest(binary),
        },
    )
    ctx.timing(FIVE_MINUTES_SECONDS)
    ctx.result(
        endpoint=ctx.stack.node,
        registry=registry,
        deployment_key=deployment_key,
        deployment=deployment,
        walkthrough_tail=walkthrough.tail(),
    )


def gauntlet_rows() -> list[dict[str, str]]:
    lines = GAUNTLET_INVENTORY.read_text(encoding="utf-8").splitlines()
    header = lines[0].split("\t")
    return [dict(zip(header, line.split("\t"))) for line in lines[1:] if line.strip()]


def fault_boundaries() -> list[dict[str, object]]:
    text = FAULT_HEADER.read_text(encoding="utf-8")
    return [{"boundary": name, "value": int(value)} for name, value in FAULT_BOUNDARY.findall(text)]


def runtime_test(ctx: CaseContext, label: str, *arguments: str) -> Execution:
    return ctx.cargo_test(
        PROGRAMS_DIRECTORY / "Cargo.toml",
        "layerx-programs-runtime",
        *arguments,
        label=label,
        cwd=PROGRAMS_DIRECTORY,
    )


def agent_test(ctx: CaseContext, label: str, suite: str, *filters: str) -> Execution:
    arguments = ("--test", suite) + (("--", *filters) if filters else ())
    return ctx.cargo_test(AGENT_MANIFEST, "layerx-agentd", *arguments, label=label)


def built_binary(ctx: CaseContext, name: str, label: str) -> Execution:
    relative = f"build/tests/{name}"
    ctx.make(relative, label=f"build-{name}")
    return ctx.run((str(ROOT / relative),), label=label)


def programs_case(
    ctx: CaseContext,
    *,
    result: Sequence[Execution],
    ledger_proof: Execution,
    inventory: dict[str, object],
) -> None:
    executed = [
        {"label": execution.label, "argv": list(execution.argv), "tests": execution.cargo_tests()}
        for execution in result
    ]
    ctx.artifact_json("inventory", "inventory.json", {"case": ctx.case_id, "executed": executed, **inventory})
    ctx.artifact_log("ledger-proof", ledger_proof)
    ctx.result(ledger_proof={"label": ledger_proof.label, "argv": list(ledger_proof.argv)})


@case("programs-qualify", "real-node/hostile-program-gauntlet")
def hostile_program_gauntlet(ctx: CaseContext) -> None:
    rows = gauntlet_rows()
    suites = sorted({row["suite"] for row in rows})
    execution = runtime_test(ctx, "gauntlet", *(argument for suite in suites for argument in ("--test", suite)))
    executed = set(execution.cargo_tests())
    missing = sorted(row["id"] for row in rows if row["test"] not in executed)
    if missing:
        raise CaseFailure("attack inventory rows did not execute: " + ", ".join(missing))
    ledger = built_binary(ctx, "programs_monetary_law", "kernel-monetary-law")
    ctx.artifact_copy("inventory", GAUNTLET_INVENTORY)
    ctx.artifact_log("ledger-proof", ledger)
    ctx.result(
        attacks=[{"id": row["id"], "suite": row["suite"], "test": row["test"]} for row in rows],
        executed_tests=sorted(executed),
    )


@case("programs-qualify", "real-node/isolation")
def isolation(ctx: CaseContext) -> None:
    execution = runtime_test(ctx, "isolation", "--test", "isolation")
    ledger = runtime_test(ctx, "runtime-monetary-law", "--test", "monetary_law")
    programs_case(ctx, result=(execution,), ledger_proof=ledger, inventory={"suite": "isolation"})


@case("programs-qualify", "real-node/determinism-differential")
def determinism_differential(ctx: CaseContext) -> None:
    execution = runtime_test(ctx, "replay-and-determinism", "--test", "replay", "--test", "determinism")
    ledger = built_binary(ctx, "programs_parallel_differential", "parallel-differential")
    programs_case(ctx, result=(execution,), ledger_proof=ledger, inventory={"suites": ["replay", "determinism"]})


@case("programs-qualify", "real-node/metering")
def metering(ctx: CaseContext) -> None:
    execution = ctx.make("test-metering")
    ledger = built_binary(ctx, "programs_metering_schedule", "metering-schedule")
    programs_case(ctx, result=(execution,), ledger_proof=ledger, inventory={"binaries": ["build/tests/test_metering"]})


@case("programs-qualify", "real-node/concurrent-same-account-transfers")
def concurrent_same_account_transfers(ctx: CaseContext) -> None:
    gateway = ctx.make("test-gateway-send")
    agent = agent_test(
        ctx, "agent-concurrent-duplicates", "idempotency", "concurrent_duplicates_produce_exactly_one_economic_effect"
    )
    ledger = ctx.make("test-ledger-send")
    programs_case(
        ctx,
        result=(gateway, agent),
        ledger_proof=ledger,
        inventory={"binaries": ["build/tests/test_gateway_send"]},
    )


@case("programs-qualify", "real-node/duplicate-idempotency-keys")
def duplicate_idempotency_keys(ctx: CaseContext) -> None:
    kernel = ctx.make("test-idempotency")
    agent = agent_test(
        ctx, "agent-repeated-key", "idempotency", "repeated_key_returns_original_and_changed_body_conflicts"
    )
    ledger = ctx.make("test-ledger-receive")
    programs_case(
        ctx,
        result=(kernel, agent),
        ledger_proof=ledger,
        inventory={"binaries": ["build/tests/lxp_test_idempotency"]},
    )


@case("programs-qualify", "real-node/lost-response-after-successful-commit")
def lost_response_after_successful_commit(ctx: CaseContext) -> None:
    agent = agent_test(ctx, "agent-lost-response", "unknown")
    daemon = ctx.make("test-daemon-lni-admission")
    ledger = ctx.make("test-ledger-set")
    programs_case(
        ctx,
        result=(agent, daemon),
        ledger_proof=ledger,
        inventory={"binaries": ["build/tests/test_daemon_lni_admission"]},
    )


CRASH_POINTS: dict[str, tuple[tuple[str, ...], tuple[str, ...]]] = {
    "real-node/crash-before-batch-wal": (("test-batch-wal-recovery",), ()),
    "real-node/crash-after-batch-wal": (("test-batch-wal-recovery", "test-log-durability"), ()),
    "real-node/crash-before-state-mutation": (("test-recovery",), ()),
    "real-node/crash-after-state-mutation": (("test-recovery", "test-projection"), ()),
    "real-node/crash-before-receipt-publication": (("test-finality-evidence",), ()),
    "real-node/crash-after-receipt-publication": (("test-finality-evidence", "test-rebuild"), ()),
    "real-node/crash-before-acknowledgement": (
        ("test-daemon-lni-admission",),
        ("idempotency", "post_restart_retry_reuses_original_result_and_pending_bytes"),
    ),
    "real-node/crash-after-acknowledgement": (
        ("test-daemon-lni-admission", "test-layerxd"),
        ("unknown", "acknowledgement_loss_is_resolved_only_by_the_existing_receipt"),
    ),
}


def crash_case(case_id: str) -> CaseFunction:
    targets, agent = CRASH_POINTS[case_id]

    def run_crash_point(ctx: CaseContext) -> None:
        results = [ctx.make(target) for target in targets]
        if agent:
            results.append(agent_test(ctx, f"agent-{agent[0]}", *agent))
        ledger = ctx.make("qualify-faults")
        programs_case(
            ctx,
            result=results,
            ledger_proof=ledger,
            inventory={
                "crash_point": case_id.rsplit("/", 1)[1],
                "targets": list(targets),
                "agent_tests": list(agent[1:]),
                "fault_boundaries": fault_boundaries(),
            },
        )

    return run_crash_point


for crash_point in CRASH_POINTS:
    case("programs-qualify", crash_point)(crash_case(crash_point))


@case("programs-qualify", "ported-reference-contracts")
def ported_reference_contracts(ctx: CaseContext) -> None:
    execution = ctx.make("programs-porting-v2-references")
    references = sorted((ROOT / "programs" / "porting").glob("*/reference-v2/target/wasm32-unknown-unknown/release/*.wasm"))
    if not references:
        raise CaseFailure("no ported reference artifact was produced under programs/porting")
    ledger = built_binary(ctx, "programs_call_activity", "call-activity")
    programs_case(
        ctx,
        result=(execution,),
        ledger_proof=ledger,
        inventory={"references": file_inventory(references)},
    )


@case("programs-qualify", "program-heavy-monetary-law-replay")
def program_heavy_monetary_law_replay(ctx: CaseContext) -> None:
    conservation = ctx.make("programs-conservation")
    replay = ctx.make("qualify-replay")
    programs_case(
        ctx,
        result=(conservation,),
        ledger_proof=replay,
        inventory={
            "suites": ["programs-conservation", "qualify-replay"],
            "expected_digest": str((ROOT / "tests" / "vectors" / "qualification_replay_10m.digest").relative_to(ROOT)),
        },
    )


def interop_test(ctx: CaseContext, label: str, package: str, *arguments: str) -> Execution:
    return ctx.cargo_test(INTEROP_MANIFEST, package, *arguments, label=label)


def interop_case(
    ctx: CaseContext,
    *,
    crate: str,
    result: Execution,
    proof: Execution,
    compatibility: dict[str, object],
) -> None:
    ctx.artifact_json(
        "compatibility",
        "compatibility.json",
        {"case": ctx.case_id, **crate_identity(ROOT / "interop" / "crates" / crate), **compatibility},
    )
    ctx.artifact_log("proof", proof)
    ctx.result(
        conformance_tests=result.cargo_tests(),
        proof={"label": proof.label, "argv": list(proof.argv), "tests": proof.cargo_tests()},
    )


def pinned_conformance(ctx: CaseContext, crate: str, package: str, target: str, proof_suite: str) -> None:
    pinned = interop_test(ctx, "pinned-spec", package, "--test", "pinned_spec")
    result = ctx.make(target)
    proof = interop_test(ctx, proof_suite, package, "--test", proof_suite)
    vectors = sorted(path for path in (ROOT / "interop" / "crates" / crate / "tests").rglob("*") if path.is_file() and path.suffix != ".rs")
    interop_case(
        ctx,
        crate=crate,
        result=result,
        proof=proof,
        compatibility={"pinned_spec_tests": pinned.cargo_tests(), "vectors": file_inventory(vectors)},
    )


@case("interop-qualify", "x402-v2/all-transports")
def x402_all_transports(ctx: CaseContext) -> None:
    pinned_conformance(ctx, "layerx-x402", "layerx-x402", "interop-test-x402", "transports")


@case("interop-qualify", "ap2/pinned-conformance")
def ap2_pinned_conformance(ctx: CaseContext) -> None:
    pinned_conformance(ctx, "layerx-ap2", "layerx-ap2", "interop-test-mandates", "mandates")


@case("interop-qualify", "ucp/pinned-conformance")
def ucp_pinned_conformance(ctx: CaseContext) -> None:
    pinned_conformance(ctx, "layerx-ucp", "layerx-ucp", "interop-test-ucp", "conformance_vectors")


@case("interop-qualify", "visa-trusted-agent/pinned-conformance")
def visa_trusted_agent_pinned_conformance(ctx: CaseContext) -> None:
    pinned_conformance(ctx, "layerx-visa-tap", "layerx-visa-tap", "interop-test-visa-tap", "conformance")


@case("interop-qualify", "portable-verification/layerx-to-external")
def portable_verification_layerx_to_external(ctx: CaseContext) -> None:
    vectors = interop_test(ctx, "receipt-vectors", "layerx-portable", "--test", "receipt_vectors")
    result = ctx.make("interop-test-portable")
    proof = interop_test(ctx, "external-verification", "layerx-portable", "--test", "external_verification")
    interop_case(
        ctx,
        crate="layerx-portable",
        result=result,
        proof=proof,
        compatibility={"direction": "layerx-to-external", "receipt_vector_tests": vectors.cargo_tests()},
    )


@case("interop-qualify", "portable-verification/external-to-layerx")
def portable_verification_external_to_layerx(ctx: CaseContext) -> None:
    vectors = interop_test(ctx, "receipt-vectors", "layerx-portable", "--test", "receipt_vectors")
    result = interop_test(ctx, "independent-verifier", "layerx-portable", "--test", "independent_verifier")
    proof = interop_test(
        ctx,
        "independent-verifier-rejection",
        "layerx-portable",
        "--test",
        "independent_verifier",
        "--",
        "independent_verifier_rejects_batch_mismatch",
        "independent_verifier_no_layerx_infrastructure_required",
    )
    interop_case(
        ctx,
        crate="layerx-portable",
        result=result,
        proof=proof,
        compatibility={"direction": "external-to-layerx", "receipt_vector_tests": vectors.cargo_tests()},
    )


@case("interop-qualify", "migration/fault-injection")
def migration_fault_injection(ctx: CaseContext) -> None:
    ctx.require_env(*MIGRATION_INPUTS)
    local = ctx.make("interop-test-migration")
    result = ctx.make("interop-test-migration-testnets")
    interop_case(
        ctx,
        crate="layerx-migrate",
        result=result,
        proof=local,
        compatibility={"testnet_inputs": list(MIGRATION_INPUTS), "live_tests": result.cargo_tests()},
    )


@case("interop-qualify", "fiat/fault-injection")
def fiat_fault_injection(ctx: CaseContext) -> None:
    result = interop_test(ctx, "fiat-adapter", "layerx-fiat")
    proof = interop_test(
        ctx,
        "fiat-fault-injection",
        "layerx-fiat",
        "--test",
        "adapter",
        "--",
        "provider_fault_injection_refuses_settlement_and_surfaces_honest_state",
        "idempotency_binds_provider_settlement_to_exactly_one_outcome",
        "receipt_mismatch_refuses_credit_and_preserves_honest_state",
    )
    toolkit = ctx.cargo_test(PLATFORM_MANIFEST, "layerx-ramp-toolkit", label="ramp-toolkit")
    interop_case(
        ctx,
        crate="layerx-fiat",
        result=result,
        proof=proof,
        compatibility={"ramp_toolkit_tests": toolkit.cargo_tests()},
    )


def mirror_verifier(ctx: CaseContext) -> Path:
    ctx.run(
        (
            "cargo",
            "build",
            "--locked",
            "--release",
            "--manifest-path",
            str(INTEROP_MANIFEST),
            "--package",
            "layerx-mirror",
            "--bin",
            "layerx-mirror-verify",
        ),
        label="build-mirror-verify",
    )
    target = Path(ctx.environment.get("CARGO_TARGET_DIR", str(ROOT / "interop" / "target")))
    return ctx.require_executable(target / "release" / "layerx-mirror-verify", "layerx-mirror-verify")


def mirror_verify(ctx: CaseContext, verifier: Path, config: str, request: str, label: str) -> tuple[Execution, dict[str, object]]:
    execution = ctx.run(
        (str(verifier), config),
        label=label,
        stdin_text=Path(request).read_text(encoding="utf-8"),
        allow_failure=True,
    )
    document = execution.last_json()
    return execution, document


@case("multichain-qualify", "mirrors/offline-verification")
def mirrors_offline_verification(ctx: CaseContext) -> None:
    inputs = ctx.require_env(*MIRROR_VERIFY_INPUTS)
    verifier = mirror_verifier(ctx)
    environment = {name: value for name, value in ctx.environment.items() if name not in MIRROR_LIVE_FORBIDDEN}
    environment["LAYERX_MIRROR_VERIFY_BIN"] = str(verifier)
    canonical, verdict = mirror_verify(
        ctx, verifier, inputs["LAYERX_MIRROR_VERIFY_CONFIG"], inputs["LAYERX_MIRROR_CANONICAL_REQUEST"], "canonical"
    )
    verification = verdict.get("verification")
    if (
        verdict.get("ok") is not True
        or not isinstance(verification, dict)
        or verification.get("provenance") != "Canonical"
        or not verification.get("sourceId")
    ):
        raise CaseFailure(f"canonical mirror verification was not accepted: {json.dumps(verdict)}")
    divergence, refusal = mirror_verify(
        ctx, verifier, inputs["LAYERX_MIRROR_VERIFY_CONFIG"], inputs["LAYERX_MIRROR_DIVERGENCE_REQUEST"], "divergence"
    )
    if refusal.get("ok") is not False or refusal.get("error") != "divergent":
        raise CaseFailure(f"divergent mirrors were not refused: {json.dumps(refusal)}")
    live = ctx.run(
        ("bash", str(ROOT / "scripts" / "qualify-mirror-verification-live.sh")),
        label="mirror-verification-live",
        env=environment,
    )
    ctx.artifact_log("proof", canonical)
    ctx.artifact_log("tamper-rejection", divergence)
    ctx.result(verifier=str(verifier), verifier_sha256=file_digest(verifier), live_tail=live.tail())


@case("multichain-qualify", "mirrors/tamper-rejection")
def mirrors_tamper_rejection(ctx: CaseContext) -> None:
    inputs = ctx.require_env(*MIRROR_TAMPER_INPUTS)
    verifier = mirror_verifier(ctx)
    canonical, verdict = mirror_verify(
        ctx, verifier, inputs["LAYERX_MIRROR_VERIFY_CONFIG"], inputs["LAYERX_MIRROR_CANONICAL_REQUEST"], "canonical"
    )
    if verdict.get("ok") is not True:
        raise CaseFailure(f"canonical mirror verification was not accepted: {json.dumps(verdict)}")
    tamper, refusal = mirror_verify(
        ctx, verifier, inputs["LAYERX_MIRROR_VERIFY_CONFIG"], inputs["LAYERX_MIRROR_TAMPER_REQUEST"], "tamper"
    )
    if refusal.get("ok") is not False or refusal.get("error") != "verification":
        raise CaseFailure(f"tampered mirror content was not refused: {json.dumps(refusal)}")
    archive = ctx.make("interop-test-mirrors")
    ctx.artifact_log("proof", canonical)
    ctx.artifact_log("tamper-rejection", tamper)
    ctx.result(verifier=str(verifier), verifier_sha256=file_digest(verifier), archive_tests=archive.cargo_tests())


@case("multichain-qualify", "surfaces/paxeer-exclusivity")
def paxeer_exclusivity(ctx: CaseContext) -> None:
    expected_chain = ctx.require_env(PAXEER_CHAIN_INPUT)[PAXEER_CHAIN_INPUT]
    chain = ctx.http_json(
        ctx.stack.paxeer_testnet,
        method="POST",
        payload={"jsonrpc": "2.0", "id": 1, "method": "eth_chainId", "params": []},
    )
    observed = chain.get("result")
    if not isinstance(observed, str) or int(observed, 16) != int(expected_chain, 0):
        raise CaseFailure(
            f"{ctx.stack.paxeer_testnet} reports chain {observed!r}, not {PAXEER_CHAIN_INPUT}={expected_chain}"
        )
    block = ctx.http_json(
        ctx.stack.paxeer_testnet,
        method="POST",
        payload={"jsonrpc": "2.0", "id": 2, "method": "eth_blockNumber", "params": []},
    )
    proof = ctx.artifact_json(
        "proof",
        "proof.json",
        {
            "paxeer_testnet": ctx.stack.paxeer_testnet,
            "expected_chain_id": expected_chain,
            "eth_chainId": chain,
            "eth_blockNumber": block,
            "observed_at": utc_now(),
        },
    )
    rejection = ctx.cargo_test(
        HUMAN_MANIFEST,
        "layerx-paxeer-client",
        "--test",
        "finality",
        "--",
        "endpoint_loss_reads_unreachable_with_last_known_stage",
        "reorg_displacement_is_reported_distinctly",
        "reverted_custody_transaction_reports_execution_outcome",
        label="paxeer-refusals",
    )
    result = ctx.make("human-test-paxeer")
    ctx.artifact_log("tamper-rejection", rejection)
    ctx.result(proof=ctx.relative(proof), paxeer_tests=result.cargo_tests())


@case("multichain-qualify", "surfaces/one-ledger")
def one_ledger(ctx: CaseContext) -> None:
    schema = ctx.run(
        (
            "cargo",
            "run",
            "--manifest-path",
            str(SCHEMA_CHECK_MANIFEST),
            "--locked",
            "--",
            str(ROOT / "human" / "schema" / "human-api"),
        ),
        label="schema-check",
    )
    if "human-api schema conformance passed" not in schema.output:
        raise CaseFailure(f"schema check did not report conformance:\n{schema.tail()}")
    rejection = ctx.run(
        (
            "cargo",
            "test",
            "--manifest-path",
            str(SCHEMA_CHECK_MANIFEST),
            "--locked",
            "--",
            "custody_claim_without_a_settlement_domain_declaration_is_rejected",
            "custody_claim_without_the_domain_field_is_rejected",
            "foreign_settlement_domain_on_a_custody_claim_is_rejected",
            "internal_movement_naming_a_settlement_domain_is_rejected",
        ),
        label="settlement-domain-rejections",
    )
    if len(rejection.cargo_tests()) < 4:
        raise CaseFailure(f"settlement-domain rejections did not all execute:\n{rejection.tail()}")
    conformance = ctx.run(
        ("cargo", "test", "--manifest-path", str(SCHEMA_CHECK_MANIFEST), "--locked"),
        label="schema-conformance",
    )
    ctx.artifact_log("proof", schema)
    ctx.artifact_log("tamper-rejection", rejection)
    ctx.result(conformance_tests=conformance.cargo_tests())


@case("multichain-qualify", "reference-ramp/end-to-end")
def reference_ramp_end_to_end(ctx: CaseContext) -> None:
    ctx.require_env(*RAMP_INPUTS)
    journey = ctx.make("interop-test-ramps-sandbox")
    proof = ctx.cargo_test(
        PLATFORM_MANIFEST,
        "layerx-ramp-toolkit",
        "--test",
        "contracts",
        "--",
        "done_requires_both_verified_legs_and_external_label",
        label="ramp-done-contract",
    )
    rejection = ctx.cargo_test(
        PLATFORM_MANIFEST,
        "layerx-ramp-toolkit",
        "--test",
        "contracts",
        "--",
        "digest_binds_authenticated_customer_and_direction",
        label="ramp-digest-binding",
    )
    ctx.artifact_log("proof", proof)
    ctx.artifact_log("tamper-rejection", rejection)
    ctx.result(sandbox_tail=journey.tail())


@case("multichain-qualify", "reference-ramp/labelling")
def reference_ramp_labelling(ctx: CaseContext) -> None:
    ctx.require_env(*RAMP_INPUTS)
    toolkit = ctx.make("interop-test-ramps")
    proof = ctx.cargo_test(
        PLATFORM_MANIFEST,
        "layerx-ramp-toolkit",
        "--test",
        "contracts",
        "--",
        "done_requires_both_verified_legs_and_external_label",
        "payment_direction_selects_direct_send_or_customer_grant",
        label="ramp-label-contract",
    )
    rejection = ctx.run(
        (
            "cargo",
            "test",
            "--manifest-path",
            str(SCHEMA_CHECK_MANIFEST),
            "--locked",
            "--",
            "custody_claim_without_a_settlement_domain_declaration_is_rejected",
            "foreign_settlement_domain_on_a_custody_claim_is_rejected",
        ),
        label="custody-label-rejections",
    )
    if len(rejection.cargo_tests()) < 2:
        raise CaseFailure(f"custody label rejections did not all execute:\n{rejection.tail()}")
    ctx.artifact_log("proof", proof)
    ctx.artifact_log("tamper-rejection", rejection)
    ctx.result(toolkit_tests=toolkit.cargo_tests())


def run_case(
    gate: str,
    case_id: str,
    output: Path,
    stack: Stack,
    source_identity: str,
    environment: Mapping[str, str],
) -> dict[str, object]:
    ctx = CaseContext(gate, case_id, output, stack, source_identity, environment)
    status = "passed"
    reason = None
    try:
        CASES[gate][case_id](ctx)
        ctx.finish(EVIDENCE_SPECS[gate].artifact_kinds)
    except CaseFailure as failure:
        status, reason = "failed", str(failure)
    except Exception as error:
        status, reason = "failed", f"{type(error).__name__}: {error}"
    elapsed = ctx.elapsed()
    if status == "passed":
        print(f"case {case_id}: passed ({elapsed:.1f}s)", flush=True)
    else:
        print(f"case {case_id}: failed: {reason}", flush=True)
    return {
        "id": case_id,
        "status": status,
        "reason": reason,
        "started_at": ctx.started_at,
        "finished_at": utc_now(),
        "elapsed_seconds": round(elapsed, 3),
        "artifacts": ctx.artifacts if status == "passed" else [],
        "executions": ctx.execution_records(),
    }


def parse_arguments(arguments: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(prog="beta_driver")
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--gate", required=True, choices=DRIVER_GATES)
    run.add_argument("--output", required=True)
    run.add_argument("--source-identity", required=True)
    for _, flag in STACK_FLAGS:
        run.add_argument(flag, required=True)
    return parser.parse_args(arguments)


def beta_driver(arguments: Sequence[str], environment: Mapping[str, str] | None = None) -> int:
    parsed = parse_arguments(arguments)
    environment = dict(os.environ if environment is None else environment)
    try:
        stack = Stack(
            *(
                validated_url(getattr(parsed, flag[2:].replace("-", "_")), flag)
                for _, flag in STACK_FLAGS
            )
        )
    except QualificationFailure as failure:
        print(f"beta_driver: {failure}", file=sys.stderr)
        return 2
    gate = parsed.gate
    spec = EVIDENCE_SPECS[gate]
    registered = set(CASES[gate])
    if registered != spec.cases:
        print(
            f"beta_driver: case inventory for {gate} drifted; missing={sorted(spec.cases - registered)!r}"
            f" unexpected={sorted(registered - spec.cases)!r}",
            file=sys.stderr,
        )
        return 2
    output = Path(parsed.output).resolve()
    if output.exists() and not output.is_dir():
        print(f"beta_driver: output {output} is not a directory", file=sys.stderr)
        return 2
    output.mkdir(parents=True, exist_ok=True)
    if (output / "cases").exists():
        print(f"beta_driver: output {output} already holds case evidence", file=sys.stderr)
        return 2
    started_at = utc_now()
    print(f"beta_driver: {gate} against {json.dumps(stack.components())}", flush=True)
    records = [
        run_case(gate, case_id, output, stack, parsed.source_identity, environment)
        for case_id in sorted(spec.cases)
    ]
    failed = [record for record in records if record["status"] != "passed"]
    manifest = {
        "schema": SCHEMA,
        "gate": gate,
        "real_stack": True,
        "source_identity": parsed.source_identity,
        "driver_sha256": file_digest(Path(__file__).resolve()),
        "components": stack.components(),
        "started_at": started_at,
        "finished_at": utc_now(),
        "cases": records,
    }
    atomic_json(output / "evidence.json", manifest)
    print(
        f"beta_driver: {gate} {len(records) - len(failed)}/{len(records)} cases passed;"
        f" evidence at {output / 'evidence.json'}",
        flush=True,
    )
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(beta_driver(sys.argv[1:]))
