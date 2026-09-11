#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: tools/ci/beta-report.sh [--ledger PATH] [--contract PATH] [--spec PATH]
                               [--output PATH] [--revision REVISION]
                               [--check] [--stdout]

Renders the LayerX beta go/no-go report (spec/layerx-beta/report.md by default)
from the executed-evidence ledger and the canonical beta contract, and prints a
one-line summary on stdout. The exit status is 0 when the report is rendered,
1 in --check mode when the report on disk or the contract disagrees with the
evidence, and 2 on usage or environment errors.

  --ledger PATH    evidence ledger (default spec/layerx-beta/qualification.kvx)
  --contract PATH  beta contract (default platform/docs/content/beta.md)
  --spec PATH      feature spec (default spec/layerx-beta/spec.kvx)
  --output PATH    report to write (default spec/layerx-beta/report.md)
  --revision REV   release-candidate revision; overrides the contract value and
                   the LAYERX_BETA_RELEASE_CANDIDATE environment variable
  --check          write nothing; fail when the report on disk is not the
                   report this evidence renders, when the contract states a
                   reached rung the evidence does not support, when the
                   contract report rows disagree with the decision, or when the
                   contract claims readiness with a surface below its rung
  --stdout         write the report to stdout instead of --output

Evidence rules:
  * Only [gate.<task>.<n>] records are evidence. Observation records never
    raise a rung, never clear a stop condition and never change the decision;
    they are counted by severity as context for the owner (req 12.3).
  * Only a gate record whose revision is the release-candidate revision and
    whose outcome is pass raises a rung, covers an acceptance criterion or
    clears a stop condition (decision.revision_binding). Records on any other
    revision are history and are listed as such; a record with outcome fail or
    blocked is listed and makes the decision no-go.
  * The release-candidate revision is --revision, else
    LAYERX_BETA_RELEASE_CANDIDATE, else the contract Identity value
    release_candidate; a value that is absent, empty, "unset", "none" or not a
    40-hex commit identifier leaves the revision undeclared, and no gate record
    is then evidence for anything.
  * A gate command is reduced to the targets it names: the targets of a make
    command after any leading VAR=value words and options, the gate name of a
    "python3 tools/qualification/release_runner.py <gate>" command, and the
    script path otherwise. A target that names a gate of
    tools/qualification/release_runner.py expands to the targets of that
    gate's LOCAL_COMMANDS and to its EXTERNAL_GATES, transitively, so one
    record for a runner gate carries the evidence of every command the runner
    planned. When the runner module cannot be loaded the expansion is skipped
    and the report says so.
  * Each target raises the surfaces the evidence map below names, to the rung
    that map names. A target outside the map raises nothing and is listed
    under unmapped targets; a surface no map entry names is listed under
    surfaces no target can raise. A surface whose highest reachable rung over
    the whole map is below its required rung is listed with that ceiling: no
    gate command the repository defines can prove it at the rung the contract
    requires. The report never derives a rung from a task status, an
    observation, a document or a plan.

Stop conditions (req 12.6), transcribed from the table in
spec/layerx-beta/tasks.notes.md with the targets that clear them:
  clean bootstrap, independent receipt verification and unknown-outcome
  reconciliation  <- beta-qualify-journey
  effective revocation                                <- the agentd, MCP and
  SDK suites of task 2.2
  journey-transitive readiness  <- platform-hosted-topology-check and
  platform-hosted-smoke
  artifact identity  <- platform-release-check, and the artifact manifest the
  contract names must exist and list every ecosystem the contract declares
  every surface at its beta rung  <- beta-qualify, and no surface below its
  required rung
  scope closure  <- beta-contract-check, and every acceptance criterion of the
  feature spec covered by a passing release-candidate gate record

The decision line is go only when a release-candidate revision is declared, at
least one gate record stands on it, no record on it failed or was blocked,
every surface is at its required rung and every stop condition is clear.
A beta invitation additionally requires a recorded owner go decision: a
[gate.5.4.<n>] record on the release-candidate revision with outcome pass whose
note begins "owner go decision:" and names the decider (task 5.4 do_3).

The report is a function of the ledger, the contract, the feature spec, the
release-candidate revision and the artifact manifest alone. It carries no
timestamp and no HEAD-relative fact, so rendering it twice on one tree yields
the same bytes and --check can compare them.
EOF
}

beta_report() {
    local root ledger="" contract="" spec="" output="" revision="" mode=render
    root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
    while [ "$#" -gt 0 ]; do
        case $1 in
        --ledger)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            ledger=$2
            shift 2
            ;;
        --contract)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            contract=$2
            shift 2
            ;;
        --spec)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            spec=$2
            shift 2
            ;;
        --output)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            output=$2
            shift 2
            ;;
        --revision)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            revision=$2
            shift 2
            ;;
        --check)
            mode=check
            shift
            ;;
        --stdout)
            mode=stdout
            shift
            ;;
        -h | --help)
            usage
            return 0
            ;;
        *)
            usage >&2
            return 2
            ;;
        esac
    done
    ledger=${ledger:-spec/layerx-beta/qualification.kvx}
    contract=${contract:-platform/docs/content/beta.md}
    spec=${spec:-spec/layerx-beta/spec.kvx}
    output=${output:-spec/layerx-beta/report.md}
    revision=${revision:-${LAYERX_BETA_RELEASE_CANDIDATE:-}}
    command -v python3 >/dev/null 2>&1 || { echo "beta-report: python3 is required" >&2; return 2; }
    local path
    for path in "$ledger" "$contract" "$spec"; do
        case $path in
        /*) [ -f "$path" ] || { echo "beta-report: not found: $path" >&2; return 2; } ;;
        *) [ -f "$root/$path" ] || { echo "beta-report: not found: $path" >&2; return 2; } ;;
        esac
    done
    python3 - "$root" "$ledger" "$contract" "$spec" "$output" "$revision" "$mode" <<'PY'
import hashlib
import importlib.util
import json
import re
import sys
from pathlib import Path

root, ledger_arg, contract_arg, spec_arg, output_arg, revision_arg, mode = sys.argv[1:8]
root = Path(root)

RUNGS = (
    "source_present",
    "statically_coherent",
    "built",
    "tested",
    "runtime_proven",
    "deployment_proven",
    "owner_certified",
)
RUNG_INDEX = {rung: index for index, rung in enumerate(RUNGS)}

GENERATOR = "tools/ci/beta-report.sh"

NATIVE_CORE = ("native-core",)
NATIVE = ("native-core", "native-daemon")
AGENT_CRATES = ("agent-daemon", "agent-mcp", "sdk-rust")
HUMAN = ("human-service", "human-web")
PLATFORM_CRATES = (
    "platform-cli",
    "emulator",
    "hosted-testnet",
    "hosted-faucet",
    "hosted-gateway",
    "hosted-registry",
    "hosted-webhooks",
    "hosted-dashboard",
    "hosted-core",
    "hosted-authority",
    "hosted-agent-boundary",
    "hosted-identity",
    "hosted-paxeer",
    "hosted-internal",
    "ramps-toolkit",
)
SDKS = (
    "sdk-typescript",
    "sdk-python",
    "sdk-rust",
    "sdk-go",
    "sdk-jvm",
    "sdk-swift",
    "sdk-dotnet",
)
PROGRAMS = (
    "programs-runtime",
    "programs-interpreter",
    "programs-market",
    "programs-protocol-adapter",
    "programs-registry",
    "programs-sandbox",
)
INTEROP = (
    "interop-x402",
    "interop-ap2",
    "interop-ucp",
    "interop-visa-tap",
    "interop-portable",
    "interop-migrate",
    "interop-fiat",
    "interop-mirror",
    "interop-gateway",
    "interop-service",
)
MIDDLEWARE = (
    "middleware-buyer",
    "middleware-seller",
    "middleware-merchant",
    "middleware-agent",
)
REFERENCE_APPS = (
    "reference-app-buyer-agent",
    "reference-app-paid-api",
    "reference-app-merchant-shop",
    "reference-app-marketplace",
)
AGENT_FRAMEWORKS = (
    "agent-framework-mcp",
    "agent-framework-a2a",
    "agent-framework-openai",
    "agent-framework-anthropic",
    "agent-framework-langchain",
    "agent-framework-vercel-ai",
)
CLUSTER_HOSTED = (
    "hosted-testnet",
    "hosted-gateway",
    "hosted-faucet",
    "hosted-registry",
    "hosted-identity",
    "hosted-core",
    "hosted-authority",
    "hosted-agent-boundary",
    "hosted-node",
    "hosted-paxeer",
    "hosted-internal",
)

EVIDENCE_MAP_RAW = {
    "build": ("built", NATIVE_CORE),
    "layerxd": ("built", ("native-daemon",)),
    "layerx-genesis-build": ("built", ("native-daemon",)),
    "agent-build": ("built", AGENT_CRATES),
    "human-build": ("built", HUMAN),
    "platform-build": ("built", PLATFORM_CRATES),
    "platform-build-all": (
        "built",
        PLATFORM_CRATES
        + SDKS
        + (
            "integration-spring",
            "integration-express",
            "integration-next",
            "integration-fastapi",
            "integration-agents",
        ),
    ),
    "programs-build": ("built", PROGRAMS),
    "interop-build": ("built", INTEROP),
    "platform-test-mobile-artifacts": ("built", ("integration-ios", "integration-android")),
    "platform-sdk-check": ("statically_coherent", SDKS),
    "platform-lint": ("statically_coherent", PLATFORM_CRATES),
    "human-lint": ("statically_coherent", HUMAN),
    "human-check": ("statically_coherent", ("human-web",)),
    "human-check-bundle": ("statically_coherent", ("human-web",)),
    "platform-hosted-topology-check": ("statically_coherent", ("hosted-tests", "hosted-agentd")),
    "platform-hosted-agentd-check": ("statically_coherent", ("hosted-agentd",)),
    "test-daemon-lni-admission": ("tested", NATIVE),
    "test-admission": ("tested", NATIVE),
    "test-batch-wal-recovery": ("tested", NATIVE),
    "test-snapshot": ("tested", NATIVE),
    "test-state-root": ("tested", NATIVE_CORE),
    "test-protocol": ("tested", NATIVE_CORE),
    "test-result": ("tested", NATIVE_CORE),
    "test-asset-withdraw": ("tested", NATIVE_CORE),
    "test-bridge-withdraw": ("tested", NATIVE_CORE),
    "qualify-faults": ("tested", NATIVE),
    "qualify-replay": ("tested", NATIVE),
    "test-contracts": ("tested", ("settlement-contracts",)),
    "agent-test-lni-schema": ("tested", ("agent-daemon",)),
    "agent-test-client-submit-focused": ("tested", ("agent-daemon",)),
    "agent-test-agentd-handshake-gate": ("tested", ("agent-daemon",)),
    "agent-test-agentd-session": ("tested", ("agent-daemon",)),
    "agent-test-agentd-revocation": ("tested", ("agent-daemon",)),
    "agent-test-agentd-tenant-resolve": ("tested", ("agent-daemon",)),
    "agent-test-agentd-subscription": ("tested", ("agent-daemon",)),
    "agent-test-agentd-delivery": ("tested", ("agent-daemon",)),
    "agent-test-agentd-gaps": ("tested", ("agent-daemon",)),
    "agent-test-agentd-webhook": ("tested", ("agent-daemon",)),
    "agent-test-boundary": ("tested", ("agent-daemon",)),
    "agent-test-wire-hashing": ("tested", ("agent-daemon",)),
    "agent-test-proof-checkpoint": ("tested", ("agent-daemon",)),
    "agent-test-contract-schema": ("tested", ("agent-daemon",)),
    "agent-qualify-faults": ("tested", ("agent-daemon",)),
    "agent-qualify-fuzz": ("tested", ("agent-daemon",)),
    "agent-qualify-wire": ("tested", ("agent-daemon",)),
    "agent-qualify-boundary": ("tested", ("agent-daemon",)),
    "agent-test-mcp-scope": ("tested", ("agent-mcp",)),
    "agent-test-mcp-readonly": ("tested", ("agent-mcp",)),
    "agent-test-mcp-write": ("tested", ("agent-mcp",)),
    "agent-test-sdk-rust": ("tested", ("sdk-rust",)),
    "human-test": ("tested", HUMAN),
    "human-test-service": ("tested", ("human-service",)),
    "human-test-activity": ("tested", ("human-service",)),
    "human-qualify-faults": ("tested", ("human-service",)),
    "human-test-unit": ("tested", ("human-service",)),
    "human-test-integration": ("tested", ("human-service",)),
    "human-test-intents": ("tested", ("human-service",)),
    "human-test-journeys": ("tested", ("human-service",)),
    "human-test-agents": ("tested", ("human-service",)),
    "human-test-approvals": ("tested", ("human-service",)),
    "human-test-explorer": ("tested", ("human-service",)),
    "human-test-notify": ("tested", ("human-service",)),
    "human-test-fault": ("tested", ("human-service",)),
    "human-test-property": ("tested", ("human-service",)),
    "human-test-component": ("tested", ("human-web",)),
    "human-e2e-foundation": ("built", ("human-web",)),
    "human-test-paxeer": ("tested", ("multichain-paxeer-boundary",)),
    "platform-test": ("tested", PLATFORM_CRATES),
    "platform-test-tooling": ("tested", ("platform-cli", "emulator", "hosted-faucet", "hosted-testnet")),
    "platform-test-registry": ("tested", ("hosted-registry",)),
    "platform-test-sdks": ("tested", SDKS),
    "platform-verify-sdks": ("tested", SDKS),
    "platform-test-middleware": (
        ("tested", MIDDLEWARE),
        (
            "built",
            (
                "integration-express",
                "integration-next",
                "integration-fastapi",
                "integration-agents",
                "integration-ios",
                "integration-android",
            ),
        ),
        ("statically_coherent", REFERENCE_APPS),
    ),
    "platform-test-reference-apps": ("statically_coherent", REFERENCE_APPS),
    "platform/middleware/examples/examples-check.sh": ("tested", ("middleware-examples",)),
    "platform-test-docs": ("tested", ("docs-site",)),
    "programs-test": ("tested", PROGRAMS),
    "programs-core-test": ("tested", PROGRAMS),
    "programs-conservation": ("tested", PROGRAMS),
    "interop-test": ("tested", INTEROP),
    "interop-test-mirrors": ("tested", ("interop-mirror", "multichain-one-ledger")),
    "interop-test-ramps": ("tested", ("ramps-toolkit",)),
    "human-test-journey": ("runtime_proven", ("human-web",)),
    "human-e2e-journeys": ("runtime_proven", ("human-web",)),
    "human-e2e-settings": ("runtime_proven", ("human-web",)),
    "human-e2e-explorer": ("runtime_proven", ("human-web",)),
    "human-test-e2e": ("runtime_proven", ("human-web",)),
    "human-e2e": ("runtime_proven", ("human-web",)),
    "human-test-visual": ("runtime_proven", ("human-web",)),
    "human-test-e2e-long": ("runtime_proven", ("human-web",)),
    "human-e2e-perf": ("runtime_proven", ("human-web",)),
    "platform-test-agent-install": ("runtime_proven", ("agent-daemon", "platform-cli")),
    "platform-emulator-conformance": ("runtime_proven", ("emulator",)),
    "platform-real-agent-integration": ("runtime_proven", AGENT_FRAMEWORKS + ("integration-agents",)),
    "platform-real-ios-integration": ("runtime_proven", ("integration-ios",)),
    "platform-real-android-integration": ("runtime_proven", ("integration-android",)),
    "interop-test-migration-testnets": ("runtime_proven", ("interop-migrate",)),
    "interop-test-ramps-sandbox": ("runtime_proven", ("ramps-toolkit", "reference-ramp")),
    "platform-qualify-adoption": ("runtime_proven", ("docs-site", "platform-cli") + SDKS),
    "programs-qualify": ("runtime_proven", PROGRAMS),
    "interop-qualify": ("runtime_proven", INTEROP),
    "beta-qualify-journey": (
        "runtime_proven",
        ("platform-cli", "emulator", "native-core", "native-daemon", "agent-daemon",
         "hosted-gateway", "hosted-faucet", "hosted-testnet"),
    ),
    "platform/middleware/examples/examples-live-check.sh": ("runtime_proven", ("middleware-examples",)),
    "platform/hosted/agentd/live-check.sh": ("deployment_proven", ("hosted-agentd",)),
    "platform/hosted/agentd/probe.sh": ("deployment_proven", ("hosted-agentd",)),
    "platform-beta-cluster-up": (
        "deployment_proven",
        CLUSTER_HOSTED + ("hosted-human", "hosted-webhooks", "hosted-dashboard", "hosted-agentd"),
    ),
    "platform-hosted-smoke": ("deployment_proven", CLUSTER_HOSTED),
    "multichain-qualify": (
        "deployment_proven",
        ("mirror-ethereum", "mirror-solana", "multichain-paxeer-boundary", "multichain-one-ledger"),
    ),
}

EVIDENCE_MAP = {
    target: (entry,) if isinstance(entry[0], str) else entry
    for target, entry in EVIDENCE_MAP_RAW.items()
}

STOP_CONDITIONS = (
    (
        "clean_bootstrap",
        "Clean bootstrap fails from the published path",
        ("beta-qualify-journey",),
        ("1.5", "1.6"),
        None,
    ),
    (
        "independent_receipt_verification",
        "Success is not independently verifiable",
        ("beta-qualify-journey",),
        ("12.4",),
        None,
    ),
    (
        "unknown_outcome_reconciliation",
        "An unknown outcome cannot be reconciled",
        ("beta-qualify-journey",),
        ("12.4",),
        None,
    ),
    (
        "effective_revocation",
        "Revocation is ineffective at any boundary",
        (
            "agent-test-agentd-session",
            "agent-test-agentd-revocation",
            "agent-test-agentd-tenant-resolve",
            "agent-test-agentd-subscription",
            "agent-test-agentd-delivery",
            "agent-test-agentd-gaps",
            "agent-test-agentd-webhook",
            "agent-test-mcp-scope",
            "agent-test-mcp-readonly",
            "agent-test-mcp-write",
            "agent-test-sdk-rust",
            "agent-test-contract-schema",
            "platform-sdk-check",
            "human-test-service",
        ),
        ("4.3", "4.6"),
        None,
    ),
    (
        "journey_transitive_readiness",
        "Readiness is not journey-transitive",
        ("platform-hosted-topology-check", "platform-hosted-smoke"),
        ("7.4", "7.5"),
        None,
    ),
    (
        "artifact_identity",
        "Artifacts lack identity",
        ("platform-release-check",),
        ("8.2", "8.3", "8.4"),
        "artifact_manifest",
    ),
    (
        "every_surface_at_rung",
        "Any surface below its beta rung",
        ("beta-qualify",),
        ("12.4", "13.3"),
        "surfaces_at_rung",
    ),
    (
        "scope_closure",
        "Scope closure is unverified",
        ("beta-contract-check",),
        ("13.4",),
        "criteria_covered",
    ),
)

violations = []
notes = []


def read(path):
    return path.read_text(encoding="utf-8")


def strip_comment(line):
    out = []
    quoted = False
    index = 0
    while index < len(line):
        character = line[index]
        if quoted and character == "\\":
            out.append(character)
            index += 1
            if index < len(line):
                out.append(line[index])
                index += 1
            continue
        if character == '"':
            quoted = not quoted
        elif character == "#" and not quoted:
            break
        out.append(character)
        index += 1
    return "".join(out)


def unquote(value):
    out = []
    index = 1
    while index < len(value) - 1:
        character = value[index]
        if character == "\\":
            index += 1
            if index < len(value) - 1:
                out.append(value[index])
                index += 1
            continue
        out.append(character)
        index += 1
    return "".join(out)


def split_list(value):
    items = []
    item = ""
    quoted = False
    index = 1
    while index < len(value) - 1:
        character = value[index]
        if quoted and character == "\\":
            item += character + value[index + 1: index + 2]
            index += 2
            continue
        if character == '"':
            quoted = not quoted
            item += character
        elif character == "," and not quoted:
            items.append(item.strip())
            item = ""
        else:
            item += character
        index += 1
    if item.strip():
        items.append(item.strip())
    return [unquote(item) if item.startswith('"') and item.endswith('"') else item for item in items]


def parse_kvx(text):
    records = []
    current = None
    for line in text.splitlines():
        stripped = strip_comment(line).strip()
        if not stripped:
            continue
        if stripped.startswith("[") and stripped.endswith("]"):
            current = (stripped[1:-1], {})
            records.append(current)
            continue
        if current is None or "=" not in stripped:
            continue
        key, _, value = stripped.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"') and len(value) >= 2:
            current[1][key] = unquote(value)
        elif value.startswith("[") and value.endswith("]"):
            current[1][key] = split_list(value)
        else:
            current[1][key] = value
    return records


def parse_contract(text):
    sections = {}
    current = None
    heading = None
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        match = re.match(r"^(#{2,3})\s+(.*\S)\s*$", line)
        if match:
            title = match.group(2)
            if len(match.group(1)) == 2:
                heading = title
                current = title
            else:
                current = f"{heading}/{title}" if heading else title
            sections.setdefault(current, [])
            index += 1
            continue
        if line.startswith("|") and current is not None:
            rows = []
            while index < len(lines) and lines[index].startswith("|"):
                rows.append([cell.strip() for cell in lines[index].strip().strip("|").split("|")])
                index += 1
            if len(rows) >= 2 and all(re.fullmatch(r":?-{3,}:?", cell) for cell in rows[1]):
                width = len(rows[0])
                sections[current].append(
                    {"header": rows[0], "rows": [row for row in rows[2:] if len(row) == width]}
                )
            continue
        index += 1
    return sections


def contract_table(sections, heading, header, position=0):
    tables = sections.get(heading, [])
    if len(tables) <= position:
        violations.append(f"contract: heading '{heading}' lacks table {position + 1} with header {header}")
        return []
    found = tables[position]
    if found["header"] != header:
        violations.append(
            f"contract: table {position + 1} under '{heading}' has header {found['header']}, expected {header}"
        )
        return []
    return found["rows"]


def key_values(rows):
    return {row[0]: row[1] for row in rows}


def cell(value):
    return value.replace("|", "\\|")


def digest(path):
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


ledger_path = root / ledger_arg
contract_path = root / contract_arg
spec_path = root / spec_arg
output_path = root / output_arg

ledger_records = parse_kvx(read(ledger_path))
spec_records = parse_kvx(read(spec_path))
contract_sections = parse_contract(read(contract_path))

identity = key_values(contract_table(contract_sections, "Identity", ["Key", "Value"]))
surface_rows = contract_table(
    contract_sections,
    "Surfaces and journeys",
    ["Surface", "Journey", "Class", "Required rung", "Reached rung", "Source"],
)
artifact_rows = contract_table(
    contract_sections,
    "Artifact set",
    ["Ecosystem", "Registry", "Surface", "Packages", "Publication job"],
)
artifact_keys = key_values(contract_table(contract_sections, "Artifact set", ["Key", "Value"], 1))

required_by_class = {
    "functional": identity.get("required_rung_functional", "runtime_proven"),
    "hosted": identity.get("required_rung_hosted", "deployment_proven"),
}

declared = revision_arg.strip()
declared_source = "--revision or LAYERX_BETA_RELEASE_CANDIDATE"
if not declared:
    declared = (identity.get("release_candidate") or identity.get("release_candidate_revision") or "").strip()
    declared_source = "contract Identity release_candidate"
if declared.lower() in ("", "unset", "none", "undeclared", "-"):
    release_candidate = ""
    release_candidate_source = "undeclared"
elif re.fullmatch(r"[0-9a-f]{40}", declared):
    release_candidate = declared
    release_candidate_source = declared_source
else:
    release_candidate = ""
    release_candidate_source = "undeclared"
    violations.append(
        f"beta-report: release-candidate value {declared!r} from {declared_source} is not a 40-hex commit identifier"
    )

gate_keys = ("task", "reqs", "revision", "command", "environment", "started_at", "outcome", "evidence", "note")
gates = []
malformed = 0
observation_severity = {}
observation_blockers_by_task = {}
observations = 0
for name, record in ledger_records:
    if name.startswith("gate."):
        if any(key not in record for key in gate_keys):
            malformed += 1
            continue
        gates.append((name, record))
    elif name.startswith("observation."):
        observations += 1
        severity = record.get("severity", "unset")
        observation_severity[severity] = observation_severity.get(severity, 0) + 1
        if severity == "blocker":
            task = record.get("task", "unset")
            observation_blockers_by_task[task] = observation_blockers_by_task.get(task, 0) + 1
    else:
        malformed += 1

runner_error = ""
local_commands = {}
external_gates = {}
runner_path = root / "tools/qualification/release_runner.py"
try:
    module_spec = importlib.util.spec_from_file_location("layerx_beta_release_runner", runner_path)
    module = importlib.util.module_from_spec(module_spec)
    sys.modules[module_spec.name] = module
    module_spec.loader.exec_module(module)
    local_commands = dict(module.LOCAL_COMMANDS)
    external_gates = dict(module.EXTERNAL_GATES)
except Exception as error:
    runner_error = f"{type(error).__name__}: {error}"


def command_targets(command):
    words = command.split()
    while words and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", words[0]):
        words = words[1:]
    if not words:
        return []
    if words[0] == "make":
        return [word for word in words[1:] if not word.startswith("-") and "=" not in word]
    if words[0] in ("sh", "bash", "python3", "python", "node"):
        rest = [word for word in words[1:] if not word.startswith("-")]
        if not rest:
            return []
        if rest[0] == "tools/qualification/release_runner.py":
            return rest[1:2]
        return rest[:1]
    return words[:1]


def expand(targets):
    expanded = []
    seen = set()
    pending = list(targets)
    while pending:
        target = pending.pop(0)
        if target in seen:
            continue
        seen.add(target)
        expanded.append(target)
        for command in local_commands.get(target, ()):
            words = [word for word in command if not word.startswith("-")]
            if words and words[0] == "make":
                pending.extend(words[1:])
        pending.extend(external_gates.get(target, ()))
    return expanded


surfaces = []
for row in surface_rows:
    surface, journey, klass, required, reached, source = row
    surfaces.append(
        {
            "surface": surface,
            "journey": journey,
            "class": klass,
            "required": required,
            "contract_reached": reached,
            "source": source,
        }
    )
surface_ids = {entry["surface"] for entry in surfaces}

reached = {surface: "source_present" for surface in surface_ids}
raised_by = {surface: [] for surface in surface_ids}
covered_criteria = {}
target_records = {}
release_gates = []
other_gates = []
unmapped_targets = {}
failed_or_blocked = []

for name, record in gates:
    revision = record.get("revision", "")
    outcome = record.get("outcome", "")
    if release_candidate and revision == release_candidate:
        release_gates.append((name, record))
        if outcome != "pass":
            failed_or_blocked.append((name, record))
            continue
    else:
        other_gates.append((name, record))
        continue
    targets = expand(command_targets(record.get("command", "")))
    for target in targets:
        target_records.setdefault(target, []).append(name)
        entries = EVIDENCE_MAP.get(target)
        if entries is None:
            if target not in local_commands:
                unmapped_targets.setdefault(target, []).append(name)
            continue
        for rung, entry_surfaces in entries:
            for surface in entry_surfaces:
                if surface not in surface_ids:
                    violations.append(
                        f"beta-report: evidence map target '{target}' names surface '{surface}', which the contract does not list"
                    )
                    continue
                if RUNG_INDEX[rung] > RUNG_INDEX[reached[surface]]:
                    reached[surface] = rung
                    raised_by[surface] = [name]
                elif rung == reached[surface] and name not in raised_by[surface]:
                    raised_by[surface].append(name)
    for requirement in record.get("reqs", []):
        covered_criteria.setdefault(requirement, []).append(name)

criteria = []
for name, record in spec_records:
    if not name.startswith("req."):
        continue
    number = name[len("req."):]
    for key in record:
        if re.fullmatch(r"ac_[0-9]+", key):
            criteria.append(f"{number}.{key[len('ac_'):]}")
criteria.sort(key=lambda item: [int(part) for part in item.split(".")])
uncovered_criteria = [item for item in criteria if item not in covered_criteria]

manifest_path = artifact_keys.get("artifact_manifest_path", "")
manifest_status = artifact_keys.get("artifact_manifest_status", "")
manifest_exists = bool(manifest_path) and (root / manifest_path).is_file()
manifest_ecosystems = set()
manifest_error = ""
if manifest_exists:
    try:
        document = json.loads(read(root / manifest_path))
        entries = document.get("artifacts", document if isinstance(document, list) else [])
        for entry in entries:
            if isinstance(entry, dict):
                registry = entry.get("registry") or entry.get("ecosystem")
                if registry:
                    manifest_ecosystems.add(str(registry))
    except (OSError, ValueError) as error:
        manifest_error = f"{type(error).__name__}: {error}"
declared_ecosystems = [row[0] for row in artifact_rows]
missing_ecosystems = [item for item in declared_ecosystems if item not in manifest_ecosystems]

below_required = [
    entry
    for entry in surfaces
    if RUNG_INDEX[reached[entry["surface"]]] < RUNG_INDEX.get(entry["required"], len(RUNGS))
]
surfaces_at_rung = not below_required

stop_rows = []
stop_clear = 0
for identifier, statement, targets, refs, extra in STOP_CONDITIONS:
    covering = []
    missing = []
    for target in targets:
        records = target_records.get(target, [])
        if records:
            covering.extend(record for record in records if record not in covering)
        else:
            missing.append(target)
    clear = not missing
    detail = ""
    if extra == "artifact_manifest":
        if not manifest_exists:
            clear = False
            detail = f"artifact manifest {manifest_path or '(unnamed)'} is absent; contract states artifact_manifest_status {manifest_status or '(unset)'}"
        elif manifest_error:
            clear = False
            detail = f"artifact manifest {manifest_path} is unreadable ({manifest_error})"
        elif missing_ecosystems:
            clear = False
            detail = "artifact manifest lists no artifact for " + ", ".join(missing_ecosystems)
        else:
            detail = f"artifact manifest {manifest_path} lists every declared ecosystem"
    elif extra == "surfaces_at_rung":
        if below_required:
            clear = False
            detail = f"{len(below_required)} of {len(surfaces)} surface rows are below their required rung"
        else:
            detail = "every surface row is at its required rung"
    elif extra == "criteria_covered":
        if uncovered_criteria:
            clear = False
            detail = f"{len(uncovered_criteria)} of {len(criteria)} acceptance criteria are not covered by a passing release-candidate gate record"
        else:
            detail = "every acceptance criterion is covered by a passing release-candidate gate record"
    if missing:
        if len(missing) > 3:
            missing_text = (
                f"no passing release-candidate gate record covers {len(missing)} of {len(targets)} named targets"
            )
        else:
            missing_text = "no passing release-candidate gate record covers " + ", ".join(missing)
        detail = f"{missing_text}; {detail}" if detail else missing_text
    if clear:
        stop_clear += 1
    stop_rows.append(
        {
            "id": identifier,
            "statement": statement,
            "clear": clear,
            "records": covering,
            "targets": targets,
            "refs": refs,
            "detail": detail,
        }
    )

owner_decision = ""
owner_decision_record = ""
for name, record in release_gates:
    if record.get("task") == "5.4" and record.get("outcome") == "pass":
        note = record.get("note", "")
        if note.startswith("owner go decision:"):
            owner_decision = note[len("owner go decision:"):].strip()
            owner_decision_record = name

reasons = []
if not release_candidate:
    reasons.append(
        "no release-candidate revision is declared, so no gate record is evidence "
        "(declare it with --revision, LAYERX_BETA_RELEASE_CANDIDATE or the contract Identity value release_candidate)"
    )
elif not release_gates:
    reasons.append(f"no gate record stands on the release-candidate revision {release_candidate}")
if failed_or_blocked:
    outcomes = ", ".join(f"{name} ({record.get('outcome')})" for name, record in failed_or_blocked)
    reasons.append(f"{len(failed_or_blocked)} release-candidate gate record(s) did not pass: {outcomes}")
if below_required:
    reasons.append(f"{len(below_required)} of {len(surfaces)} surface rows are below their required rung")
not_clear = [row["id"] for row in stop_rows if not row["clear"]]
if not_clear:
    reasons.append(f"{len(not_clear)} of {len(stop_rows)} stop conditions are not clear: " + ", ".join(not_clear))

decision = "go" if not reasons else "no-go"
invitation = "permitted" if decision == "go" and owner_decision else "not permitted"

surfaces_at_required = sum(
    1
    for entry in surfaces
    if RUNG_INDEX[reached[entry["surface"]]] >= RUNG_INDEX.get(entry["required"], len(RUNGS))
)
mapped_surfaces = {surface for entries in EVIDENCE_MAP.values() for _, group in entries for surface in group}
unraisable = sorted(surface for surface in surface_ids if surface not in mapped_surfaces)

ceiling = {surface: "source_present" for surface in surface_ids}
for entries in EVIDENCE_MAP.values():
    for rung, group in entries:
        for surface in group:
            if surface in ceiling and RUNG_INDEX[rung] > RUNG_INDEX[ceiling[surface]]:
                ceiling[surface] = rung
unprovable = []
for entry in surfaces:
    surface = entry["surface"]
    if RUNG_INDEX[ceiling[surface]] < RUNG_INDEX[entry["required"]]:
        pair = (surface, ceiling[surface], entry["required"])
        if pair not in unprovable:
            unprovable.append(pair)

revision_history = {}
for name, record in other_gates:
    revision = record.get("revision", "(unset)")
    bucket = revision_history.setdefault(revision, {"pass": 0, "fail": 0, "blocked": 0, "records": 0})
    bucket["records"] += 1
    outcome = record.get("outcome", "")
    if outcome in bucket:
        bucket[outcome] += 1

lines = []
lines.append("<!-- id: beta_report -->")
lines.append(f"<!-- decision: {decision} -->")
lines.append("")
lines.append("# LayerX Network beta go/no-go report")
lines.append("")
lines.append(
    f"Generated by `tools/ci/beta-report.sh` from `{ledger_arg}` and `{contract_arg}`. "
    "Do not edit by hand: every value below is computed from gate records and the contract, "
    "and `tools/ci/beta-report.sh --check` fails when this file is not what that evidence renders."
)
lines.append("")
lines.append("## Decision")
lines.append("")
lines.append(f"**{decision}**")
lines.append("")
lines.append("| Key | Value |")
lines.append("| --- | --- |")
lines.append("| id | beta_report |")
lines.append(f"| decision | {decision} |")
lines.append(f"| release_candidate | {release_candidate or 'undeclared'} |")
lines.append(f"| release_candidate_source | {release_candidate_source} |")
lines.append(f"| ledger | {ledger_arg} |")
lines.append(f"| ledger_digest | {digest(ledger_path)} |")
lines.append(f"| contract | {contract_arg} |")
lines.append(f"| contract_digest | {digest(contract_path)} |")
lines.append(f"| feature_spec | {spec_arg} |")
lines.append(f"| generator | {GENERATOR} |")
lines.append(f"| gate_records_total | {len(gates)} |")
lines.append(f"| gate_records_at_release_candidate | {len(release_gates)} |")
lines.append(f"| gate_records_not_passing_at_release_candidate | {len(failed_or_blocked)} |")
lines.append(f"| gate_records_on_other_revisions | {len(other_gates)} |")
lines.append(f"| surface_rows | {len(surfaces)} |")
lines.append(f"| surface_rows_at_required_rung | {surfaces_at_required} |")
lines.append(f"| stop_conditions_clear | {stop_clear} of {len(stop_rows)} |")
lines.append(f"| acceptance_criteria_covered | {len(criteria) - len(uncovered_criteria)} of {len(criteria)} |")
lines.append(f"| observation_records | {observations} |")
lines.append(f"| owner_go_decision | {owner_decision or 'not recorded'} |")
lines.append(f"| beta_invitation | {invitation} |")
lines.append("")
if reasons:
    lines.append("The decision is `no-go` because:")
    lines.append("")
    for reason in reasons:
        lines.append(f"- {reason}")
else:
    lines.append(
        "Every surface is at its required rung and every stop condition is clear on the "
        "release-candidate revision. A beta invitation additionally requires the recorded owner go decision above."
    )
lines.append("")
lines.append("## Release-candidate binding")
lines.append("")
lines.append(
    "Only a gate record whose revision is the release-candidate revision and whose outcome is `pass` "
    "is evidence. Records on any other revision are history and are listed below unaggregated into any rung."
)
lines.append("")
lines.append("| Revision | Gate records | pass | fail | blocked | Counted as evidence |")
lines.append("| --- | --- | --- | --- | --- | --- |")
if release_candidate:
    passing = len(release_gates) - len(failed_or_blocked)
    failing = sum(1 for _, record in failed_or_blocked if record.get("outcome") == "fail")
    blocked = sum(1 for _, record in failed_or_blocked if record.get("outcome") == "blocked")
    lines.append(
        f"| {release_candidate} | {len(release_gates)} | {passing} | {failing} | {blocked} | yes (release candidate) |"
    )
for revision in sorted(revision_history):
    bucket = revision_history[revision]
    lines.append(
        f"| {revision} | {bucket['records']} | {bucket['pass']} | {bucket['fail']} | {bucket['blocked']} | no |"
    )
if not release_candidate and not revision_history:
    lines.append("| (none) | 0 | 0 | 0 | 0 | no |")
lines.append("")
lines.append("## Surfaces and reached rungs")
lines.append("")
lines.append(
    "The reached rung of a surface is the highest rung any passing release-candidate gate record raises it to "
    "through the evidence map of `tools/ci/beta-report.sh`. No task status, observation or document raises a rung."
)
lines.append("")
lines.append("| Surface | Journey | Class | Required rung | Reached rung | At required rung | Raised by |")
lines.append("| --- | --- | --- | --- | --- | --- | --- |")
for entry in surfaces:
    surface = entry["surface"]
    at_rung = "yes" if RUNG_INDEX[reached[surface]] >= RUNG_INDEX.get(entry["required"], len(RUNGS)) else "no"
    records = ", ".join(raised_by[surface]) if raised_by[surface] else "no gate record"
    lines.append(
        f"| {cell(surface)} | {cell(entry['journey'])} | {cell(entry['class'])} | {cell(entry['required'])} "
        f"| {reached[surface]} | {at_rung} | {cell(records)} |"
    )
lines.append("")
lines.append("## Stop conditions")
lines.append("")
lines.append(
    "The eight stop conditions of req 12.6, with the clearing gate targets transcribed from "
    "`spec/layerx-beta/tasks.notes.md`. A condition is clear only when a passing release-candidate "
    "gate record covers every target named for it."
)
lines.append("")
lines.append("| Stop condition | Clear | Cleared by | Requires | Detail |")
lines.append("| --- | --- | --- | --- | --- |")
for row in stop_rows:
    records = ", ".join(row["records"]) if row["clear"] and row["records"] else "no gate record"
    lines.append(
        f"| {cell(row['statement'])} | {'clear' if row['clear'] else 'not clear'} | {cell(records)} "
        f"| {cell(', '.join(row['targets']))} | {cell(row['detail'] or 'req ' + ', '.join(row['refs']))} |"
    )
lines.append("")
lines.append("## Acceptance criteria covered by release-candidate gate records")
lines.append("")
lines.append(
    "A criterion is covered when a passing release-candidate gate record lists it in `reqs`. "
    "Scope closure requires full coverage."
)
lines.append("")
lines.append("| Requirement | Criteria | Covered | Uncovered |")
lines.append("| --- | --- | --- | --- |")
requirement_ids = []
for item in criteria:
    number = item.split(".")[0]
    if number not in requirement_ids:
        requirement_ids.append(number)
requirement_ids.sort(key=int)
for number in requirement_ids:
    items = [item for item in criteria if item.split(".")[0] == number]
    missing = [item for item in items if item in uncovered_criteria]
    lines.append(
        f"| req.{number} | {len(items)} | {len(items) - len(missing)} "
        f"| {', '.join(missing) if missing else 'none'} |"
    )
lines.append("")
lines.append("## Gate records on the release-candidate revision")
lines.append("")
if release_gates:
    lines.append("| Record | Task | Command | Outcome | Requirements | Evidence |")
    lines.append("| --- | --- | --- | --- | --- | --- |")
    for name, record in release_gates:
        lines.append(
            f"| {name} | {cell(record.get('task', ''))} | `{cell(record.get('command', ''))}` "
            f"| {cell(record.get('outcome', ''))} | {cell(', '.join(record.get('reqs', [])))} "
            f"| {cell(record.get('evidence', ''))} |"
        )
else:
    lines.append("No gate record stands on the release-candidate revision.")
lines.append("")
lines.append("## Evidence map coverage")
lines.append("")
lines.append(
    "A target outside the evidence map raises no surface; a surface no map entry names cannot be raised "
    "by any gate record and needs a map entry before it can leave `source_present`."
)
lines.append("")
lines.append("| Key | Value |")
lines.append("| --- | --- |")
lines.append(f"| mapped_targets | {len(EVIDENCE_MAP)} |")
lines.append(f"| surfaces_no_target_can_raise | {len(unraisable)} |")
lines.append(f"| surfaces_no_target_can_prove_at_required_rung | {len(unprovable)} |")
lines.append(f"| unmapped_targets_in_release_candidate_records | {len(unmapped_targets)} |")
lines.append(
    f"| runner_expansion | {'unavailable (' + runner_error + ')' if runner_error else 'tools/qualification/release_runner.py'} |"
)
lines.append(f"| malformed_ledger_records | {malformed} |")
lines.append("")
if unraisable:
    lines.append("Surfaces no evidence-map target names: " + ", ".join(f"`{surface}`" for surface in unraisable) + ".")
    lines.append("")
if unprovable:
    lines.append(
        "Surfaces whose highest reachable rung is below their required rung even when every mapped target passes. "
        "No gate command in the repository proves these surfaces at the rung the contract requires, so the gap is in "
        "the gate set, not in the ledger."
    )
    lines.append("")
    lines.append("| Surface | Highest reachable rung | Required rung |")
    lines.append("| --- | --- | --- |")
    for surface, highest, required in unprovable:
        lines.append(f"| {surface} | {highest} | {required} |")
    lines.append("")
if unmapped_targets:
    lines.append(
        "Targets named by release-candidate gate records that raise nothing: "
        + ", ".join(f"`{target}`" for target in sorted(unmapped_targets))
        + "."
    )
    lines.append("")
lines.append("## Observations")
lines.append("")
lines.append(
    "Observation records are context for the owner. They never raise a rung, never clear a stop condition "
    "and never change the decision line (req 12.3)."
)
lines.append("")
lines.append("| Severity | Records |")
lines.append("| --- | --- |")
for severity in ("blocker", "suspect", "assumption", "note"):
    lines.append(f"| {severity} | {observation_severity.get(severity, 0)} |")
for severity in sorted(set(observation_severity) - {"blocker", "suspect", "assumption", "note"}):
    lines.append(f"| {cell(severity)} | {observation_severity[severity]} |")
lines.append(f"| total | {observations} |")
lines.append("")
if observation_blockers_by_task:
    lines.append("Observations of severity `blocker` by the task they concern:")
    lines.append("")
    lines.append("| Task | Records |")
    lines.append("| --- | --- |")
    for task in sorted(observation_blockers_by_task, key=lambda item: [int(part) for part in item.split(".")] if re.fullmatch(r"[0-9]+(\.[0-9]+)*", item) else [10 ** 6]):
        lines.append(f"| {cell(task)} | {observation_blockers_by_task[task]} |")
    lines.append("")
rendered = "\n".join(lines).rstrip("\n") + "\n"

summary = (
    f"beta-report: {decision}; release candidate {release_candidate or 'undeclared'}; "
    f"{len(release_gates)} gate record(s) on it of {len(gates)}; "
    f"{surfaces_at_required} of {len(surfaces)} surface rows at their required rung; "
    f"{stop_clear} of {len(stop_rows)} stop conditions clear; "
    f"beta invitation {invitation}"
)

if mode == "stdout":
    sys.stdout.write(rendered)
    print(summary, file=sys.stderr)
    for violation in violations:
        print(f"  {violation}", file=sys.stderr)
    sys.exit(0)

if mode == "check":
    if not output_path.is_file():
        violations.append(f"beta-report: {output_arg} does not exist; render it with tools/ci/beta-report.sh")
    elif read(output_path) != rendered:
        violations.append(
            f"beta-report: {output_arg} is not the report this evidence renders; regenerate it with tools/ci/beta-report.sh"
        )
    for entry in surfaces:
        surface = entry["surface"]
        if entry["contract_reached"] != reached[surface]:
            violations.append(
                f"contract: surface {surface} ({entry['journey']}) states reached rung "
                f"{entry['contract_reached']!r} but the report records {reached[surface]!r}"
            )
    report_path_row = artifact_keys.get("report_path")
    if report_path_row is None:
        violations.append("contract: Artifact set lacks key report_path")
    elif report_path_row != output_arg:
        violations.append(f"contract: report_path is {report_path_row!r} but the report is {output_arg}")
    report_status_row = artifact_keys.get("report_status")
    if report_status_row is None:
        violations.append("contract: Artifact set lacks key report_status")
    elif report_status_row != decision:
        violations.append(
            f"contract: report_status is {report_status_row!r} but the report decision is {decision!r}"
        )
    report_generator_row = artifact_keys.get("report_generator")
    if report_generator_row is None:
        violations.append("contract: Artifact set lacks key report_generator")
    elif report_generator_row != GENERATOR:
        violations.append(
            f"contract: report_generator is {report_generator_row!r} but the report is rendered by {GENERATOR}"
        )
    if identity.get("readiness_claim") == "true" and below_required:
        violations.append(
            "contract: readiness is claimed while "
            f"{len(below_required)} surface row(s) are below their required rung"
        )
    if identity.get("readiness_claim") == "true" and decision != "go":
        violations.append("contract: readiness is claimed while the report decision is no-go")
    if violations:
        print(f"beta-report: {len(violations)} violation(s)", file=sys.stderr)
        for violation in violations:
            print(f"  {violation}", file=sys.stderr)
        sys.exit(1)
    print(summary)
    sys.exit(0)

output_path.parent.mkdir(parents=True, exist_ok=True)
output_path.write_text(rendered, encoding="utf-8")
print(f"beta-report: wrote {output_arg}")
print(summary)
if violations:
    for violation in violations:
        print(f"  {violation}", file=sys.stderr)
sys.exit(0)
PY
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    beta_report "$@"
fi
