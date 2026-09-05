#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
usage: tools/ci/beta-contract-check.sh [--contract PATH] [--yaml-parser auto|pyyaml|builtin]

Checks the canonical beta contract (platform/docs/content/beta.md by default)
against the sources it governs and fails on any disagreement. Every violation
is listed on stderr; the exit status is 1 when at least one violation exists,
2 on usage or environment errors, 0 otherwise.

Contract format. The contract carries the line "<!-- id: beta_contract -->"
and fixed H2 headings, each holding pipe-delimited markdown tables whose
first row is the header and second row the separator:
  Identity                   Key | Value: id, readiness_claim (true|false),
                             beta_domain, required_rung_functional,
                             required_rung_hosted, rung_order
  Surfaces and journeys      Surface | Journey | Class | Required rung |
                             Reached rung | Source; Class is functional or
                             hosted and fixes the required rung
  Beta endpoints and hostnames, Network id, Wire protocol version, Beta CA
                             Key | Value | Source
  Artifact set               Ecosystem | Registry | Surface | Packages |
                             Publication job (present|absent), a Key | Value
                             table (artifact_manifest_path,
                             artifact_manifest_status,
                             artifact_manifest_emitter,
                             artifact_manifest_verifier,
                             artifact_manifest_verification_job,
                             artifact_manifest_workflow_artifact,
                             release_tag_format, source_digest) and the H3
                             "Install coordinates" table
                             Language | Coordinate | Ecosystem
  Documentation journeys     Page | Surface | Journey
  Unknown-state behaviour, Architecture summary
                             non-empty prose
  External dependencies      Dependency | Production counterpart |
                             Beta counterpart | Owner input names
  Beta-versus-production differences
                             Key | Difference; the key set is exactly
                             ui_polish, visual_regression,
                             automated_accessibility, usability_studies,
                             performance_budgets_and_soak,
                             external_security_audit,
                             production_infrastructure,
                             production_certification
  Contradictions             Key | Canonical value | Divergent source |
                             Divergent value | Resolving task

Sources and extraction rules:
  * platform/hosted/{testnet,gateway,webhooks}/deployment.yaml and
    platform/ramps/deployment.yaml are multi-document Kubernetes manifests.
    With --yaml-parser pyyaml (the default when PyYAML imports) each document
    is loaded with yaml.safe_load_all; with builtin each document is the text
    between --- separators and its facts are read line by line: "kind:",
    the metadata name (flow-style "metadata: {name: X" or the first
    two-space-indented "name:"), ConfigMap "data:" entries,
    "{name: X, value: Y}" and "- name: X / value: Y" environment entries,
    "host:" and "hosts: [...]", "secretName:" and Service "port:" values.
    Both parsers yield the same facts: kind, name, ConfigMap data,
    environment values, ingress hosts, secret names and Service ports. A
    named value (LAYERX_* below) is read from environment entries and from
    ConfigMap data alike.
  * platform/docs/content/install.md: the emulator line
    "layerx environment use emulator --endpoint <url> --network-id <id>" and
    the "| Language | Install |" table, whose coordinate is the last word of
    the first backtick span of each row.
  * platform/docs/content/environments/emulator.md: the same emulator line.
  * platform/docs/testnet.md (docs page environments/testnet): the values in
    "LXP wire protocol version `N`", "network ID `N`", "at `<url>`",
    "The developer gateway is `<url>`" and the faucet claims URL origin.
  * platform/hosted/testnet/src/lib.rs: TESTNET_NETWORK_ID and the
    public_endpoint, gateway_endpoint, faucet_endpoint and status_endpoint
    literals; agent/crates/layerx-wire/src/limits.rs: PROTOCOL_VERSION.
  * platform/hosted/testnet/status.json: the origin of every component source.
  * .github/workflows/platform.yml: the LAYERX_TESTNET_URL, LAYERX_GATEWAY_URL
    and LAYERX_FAUCET_URL environment values (every occurrence must agree),
    the agent framework matrix "framework: [...]", the CA names and the
    publication recognisers per ecosystem: npm "npm publish", crates-io
    "cargo publish", pypi "twine upload" or "pypi-publish", maven-central
    "mvn deploy" or "gradle publish", nuget "dotnet nuget push", go-modules
    and swiftpm "git tag" or "git push --tags".
  * platform/release/registries.kvx: [release] registries, tag_format and
    source_digest; [registry.<id>] distribution and packages.
  * platform/docs/site.kvx: every [page.<id>] section is a docs journey.
  * platform/examples/reference-apps.json, platform/hosted/*,
    platform/middleware/* (except conformance), platform/integrations/*,
    interop/crates/layerx-*, programs/crates/layerx-programs-* and
    tools/qualification/release_runner.py journey_cases() derive the surface
    ids reference-app-<name>, hosted-<dir>, middleware-<dir>,
    integration-<dir>, interop-<crate>, programs-<crate> and the human-web
    journeys the contract must list.
  * interop/deploy/mirror/*.json: every deployment network name must be named
    in the External dependencies table.

Checks: every value the contract states equals the value its sources carry;
every derived surface and docs journey is listed; every rung is in the
vocabulary and matches its class; each ecosystem's publication job matches
the workflow; artifact_manifest_status is not_emitted exactly while the
manifest file is absent; the release-verification job emits the manifest
with the emitter, verifies published bytes with the verifier and retains the
manifest under the workflow artifact name, and release-promotion needs it;
the docs name only manifest-listed artifacts: while the manifest file is
absent and the contract states not_emitted, every install coordinate must be
a package identity declared in platform/release/registries.kvx (a Maven
coordinate by its group:artifact, its version checked against
package_semver) or it is an install_package_unlisted contradiction; once the
manifest file exists, every install coordinate must match a manifest entry
(name, ecosystem and, when the coordinate carries one, version), the
manifest must list every declared package and nothing undeclared, and the
manifest file absent while the contract does not state not_emitted makes
every install coordinate an unlisted violation; the differences key set is
closed; the
Contradictions table lists exactly the cross-source disagreements the check
computes (gateway_hostname, faucet_hostname, docs_wire_protocol_version,
protocol_network_id, placeholder_hostname, testnet_gateway_url_port,
install_package_unlisted) with their divergent values; and readiness_claim
must be false while any contradiction exists or any surface is below its
required rung.
EOF
}

beta_contract_check() {
    local root contract="" yaml_parser="auto"
    root=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
    while [ "$#" -gt 0 ]; do
        case $1 in
        --contract)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            contract=$2
            shift 2
            ;;
        --yaml-parser)
            [ "$#" -ge 2 ] || { usage >&2; return 2; }
            yaml_parser=$2
            shift 2
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
    case $yaml_parser in
    auto | pyyaml | builtin) ;;
    *)
        usage >&2
        return 2
        ;;
    esac
    contract=${contract:-$root/platform/docs/content/beta.md}
    [ -f "$contract" ] || { echo "beta-contract-check: contract not found: $contract" >&2; return 2; }
    command -v python3 >/dev/null 2>&1 || { echo "beta-contract-check: python3 is required" >&2; return 2; }
    python3 - "$root" "$contract" "$yaml_parser" <<'PY'
import importlib.util
import json
import os
import re
import sys
from pathlib import Path
from urllib.parse import urlsplit

root = Path(sys.argv[1])
contract_path = Path(sys.argv[2])
yaml_parser = sys.argv[3]
violations = []


def violation(message):
    violations.append(message)


def read(relative):
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        violation(f"{relative}: cannot read ({error})")
        return ""


def strip_kvx_comment(line):
    out = []
    quoted = False
    index = 0
    while index < len(line):
        char = line[index]
        if quoted and char == "\\":
            out.append(line[index : index + 2])
            index += 2
            continue
        if char == '"':
            quoted = not quoted
        elif char == "#" and not quoted:
            break
        out.append(char)
        index += 1
    return "".join(out)


def kvx_unquote(value):
    out = []
    index = 1
    while index < len(value) - 1:
        char = value[index]
        if char == "\\":
            index += 1
            char = value[index]
        out.append(char)
        index += 1
    return "".join(out)


def kvx_list(value):
    items = []
    current = []
    quoted = False
    for char in value[1:-1]:
        if char == '"':
            quoted = not quoted
        if char == "," and not quoted:
            items.append("".join(current).strip())
            current = []
            continue
        current.append(char)
    tail = "".join(current).strip()
    if tail:
        items.append(tail)
    return [kvx_unquote(item) if item.startswith('"') else item for item in items]


def parse_kvx(relative):
    sections = {}
    section = None
    for raw in read(relative).splitlines():
        line = strip_kvx_comment(raw).strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1]
            sections.setdefault(section, {})
            continue
        if "=" not in line or section is None:
            violation(f"{relative}: unparseable line {line!r}")
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        value = value.strip()
        if value.startswith('"') and value.endswith('"'):
            sections[section][key] = kvx_unquote(value)
        elif value.startswith("[") and value.endswith("]"):
            sections[section][key] = kvx_list(value)
        else:
            sections[section][key] = value
    return sections


def parse_contract(text):
    sections = {}
    current = None
    h2 = None
    lines = text.splitlines()
    index = 0
    while index < len(lines):
        line = lines[index]
        heading = re.match(r"^(#{2,3})\s+(.*\S)\s*$", line)
        if heading:
            title = heading.group(2)
            if len(heading.group(1)) == 2:
                h2 = title
                current = title
            else:
                current = f"{h2}/{title}" if h2 else title
            if current in sections:
                violation(f"contract: heading '{current}' appears twice")
            sections.setdefault(current, {"tables": [], "lines": []})
            index += 1
            continue
        if line.startswith("|") and current is not None:
            rows = []
            while index < len(lines) and lines[index].startswith("|"):
                cells = [cell.strip() for cell in lines[index].strip().strip("|").split("|")]
                rows.append(cells)
                index += 1
            if len(rows) >= 2 and all(re.fullmatch(r":?-{3,}:?", cell) for cell in rows[1]):
                width = len(rows[0])
                for row in rows[2:]:
                    if len(row) != width:
                        violation(f"contract: row {row} under '{current}' has {len(row)} cells, header has {width}")
                sections[current]["tables"].append({"header": rows[0], "rows": [row for row in rows[2:] if len(row) == width]})
            else:
                violation(f"contract: malformed table under '{current}'")
            continue
        if current is not None:
            sections[current]["lines"].append(line)
        index += 1
    return sections


def table(sections, heading, header, position=0):
    section = sections.get(heading)
    if section is None:
        violation(f"contract: heading '{heading}' is missing")
        return []
    tables = section["tables"]
    if len(tables) <= position:
        violation(f"contract: heading '{heading}' lacks table {position + 1} with header {header}")
        return []
    found = tables[position]
    if found["header"] != header:
        violation(f"contract: table {position + 1} under '{heading}' has header {found['header']}, expected {header}")
        return []
    return found["rows"]


def key_values(rows, heading):
    values = {}
    for row in rows:
        if row[0] in values:
            violation(f"contract: key '{row[0]}' repeated under '{heading}'")
        values[row[0]] = row[1]
    return values


def expect(heading, values, key, actual, source):
    if key not in values:
        violation(f"contract: '{heading}' lacks key {key}")
        return
    if values[key] != actual:
        violation(f"contract: {key} is {values[key]!r} but {source} carries {actual!r}")


class Facts:
    def __init__(self, kind, name):
        self.kind = kind
        self.name = name
        self.data = {}
        self.env = {}
        self.hosts = []
        self.secrets = set()
        self.ports = []


def facts_from_object(document):
    metadata = document.get("metadata") or {}
    facts = Facts(str(document.get("kind", "")), str(metadata.get("name", "")))
    if facts.kind == "ConfigMap":
        facts.data = {str(key): str(value) for key, value in (document.get("data") or {}).items()}

    def walk(node):
        if isinstance(node, dict):
            if "secretName" in node:
                facts.secrets.add(str(node["secretName"]))
            if isinstance(node.get("host"), str):
                facts.hosts.append(node["host"])
            if isinstance(node.get("hosts"), list):
                facts.hosts.extend(str(host) for host in node["hosts"])
            if "name" in node and "value" in node and not isinstance(node["value"], (dict, list)):
                facts.env[str(node["name"])] = str(node["value"])
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(document)
    if facts.kind == "Service":
        for port in ((document.get("spec") or {}).get("ports") or []):
            if isinstance(port, dict) and "port" in port:
                facts.ports.append(str(port["port"]))
    return facts


def facts_from_text(document):
    kind = re.search(r"^kind:\s*(\S+)", document, re.M)
    name = re.search(r"^metadata:\s*\{\s*name:\s*([^,}\s]+)", document, re.M) or re.search(r"^  name:\s*(\S+)", document, re.M)
    facts = Facts(kind.group(1) if kind else "", name.group(1) if name else "")
    if facts.kind == "ConfigMap":
        block = re.search(r"^data:\s*\n((?:  \S.*\n?)+)", document, re.M)
        if block:
            for entry in re.finditer(r"^  ([A-Za-z0-9_.-]+):\s*(.*)$", block.group(1), re.M):
                facts.data[entry.group(1)] = entry.group(2).strip().strip('"').strip("'")
    for entry in re.finditer(r"\{\s*name:\s*([A-Za-z0-9_]+)\s*,\s*value:\s*(\"[^\"]*\"|'[^']*'|[^,}\s]+)\s*\}", document):
        facts.env[entry.group(1)] = entry.group(2).strip('"').strip("'")
    for entry in re.finditer(r"-\s*name:\s*([A-Za-z0-9_]+)\s*\n\s*value:\s*(.+)$", document, re.M):
        facts.env[entry.group(1)] = entry.group(2).strip().strip('"').strip("'")
    for entry in re.finditer(r"\bhost:\s*(\S+)", document):
        facts.hosts.append(entry.group(1))
    for entry in re.finditer(r"\bhosts:\s*\[([^\]]*)\]", document):
        facts.hosts.extend(host.strip() for host in entry.group(1).split(",") if host.strip())
    for entry in re.finditer(r"\bsecretName:\s*(\S+)", document):
        facts.secrets.add(entry.group(1).rstrip("}],"))
    if facts.kind == "Service":
        for entry in re.finditer(r"\bport:\s*(\d+)", document):
            facts.ports.append(entry.group(1))
    return facts


def load_manifest(relative):
    text = read(relative)
    parser = yaml_parser
    if parser == "auto":
        parser = "pyyaml" if importlib.util.find_spec("yaml") else "builtin"
    if parser == "pyyaml":
        import yaml

        documents = []
        try:
            for document in yaml.safe_load_all(text):
                if isinstance(document, dict):
                    documents.append(facts_from_object(document))
        except yaml.YAMLError as error:
            violation(f"{relative}: PyYAML cannot parse the manifest ({error})")
        return documents
    return [facts_from_text(chunk) for chunk in re.split(r"^---\s*$", text, flags=re.M) if chunk.strip()]


def manifest_value(documents, name):
    values = {facts.env[name] for facts in documents if name in facts.env}
    values |= {facts.data[name] for facts in documents if name in facts.data}
    if len(values) > 1:
        violation(f"manifest value {name} differs between documents: {sorted(values)}")
    return next(iter(values), None)


def manifest_hosts(documents, kind="Ingress"):
    hosts = []
    for facts in documents:
        if facts.kind == kind:
            for host in facts.hosts:
                if host not in hosts:
                    hosts.append(host)
    return hosts


def manifest_secrets(documents):
    secrets = set()
    for facts in documents:
        secrets |= facts.secrets
    return secrets


def origin(url):
    parts = urlsplit(url)
    return f"{parts.scheme}://{parts.netloc}"


def url_port(url):
    parts = urlsplit(url)
    if parts.port is not None:
        return str(parts.port)
    return {"https": "443", "http": "80"}.get(parts.scheme, "")


contract_text = contract_path.read_text(encoding="utf-8")
if "<!-- id: beta_contract -->" not in contract_text:
    violation("contract: identifier line '<!-- id: beta_contract -->' is missing")
sections = parse_contract(contract_text)

identity = key_values(table(sections, "Identity", ["Key", "Value"]), "Identity")
if identity.get("id") != "beta_contract":
    violation(f"contract: Identity id is {identity.get('id')!r}, expected 'beta_contract'")
readiness = identity.get("readiness_claim")
if readiness not in ("true", "false"):
    violation(f"contract: readiness_claim is {readiness!r}, expected true or false")
readiness_claimed = readiness == "true"
comment = re.search(r"^<!-- readiness_claim: (\S+) -->$", contract_text, re.M)
if comment is None or comment.group(1) != readiness:
    violation("contract: the '<!-- readiness_claim: ... -->' line must carry the Identity readiness_claim value")
rung_order = [rung.strip() for rung in identity.get("rung_order", "").split("<")]
expected_rungs = ["source_present", "statically_coherent", "built", "tested", "runtime_proven", "deployment_proven", "owner_certified"]
if rung_order != expected_rungs:
    violation(f"contract: rung_order is {identity.get('rung_order')!r}, expected {' < '.join(expected_rungs)!r}")
rung_index = {rung: index for index, rung in enumerate(expected_rungs)}
required_by_class = {
    "functional": identity.get("required_rung_functional"),
    "hosted": identity.get("required_rung_hosted"),
}
if required_by_class["functional"] != "runtime_proven":
    violation(f"contract: required_rung_functional is {required_by_class['functional']!r}, expected 'runtime_proven'")
if required_by_class["hosted"] != "deployment_proven":
    violation(f"contract: required_rung_hosted is {required_by_class['hosted']!r}, expected 'deployment_proven'")
beta_domain = identity.get("beta_domain", "")
if not beta_domain:
    violation("contract: Identity lacks beta_domain")

surface_rows = table(sections, "Surfaces and journeys", ["Surface", "Journey", "Class", "Required rung", "Reached rung", "Source"])
surfaces = {}
journeys = set()
below_required = []
for row in surface_rows:
    surface, journey, klass, required, reached, source = row
    if klass not in required_by_class:
        violation(f"contract: surface {surface} has class {klass!r}, expected functional or hosted")
    elif required != required_by_class[klass]:
        violation(f"contract: surface {surface} ({klass}) requires {required!r}, expected {required_by_class[klass]!r}")
    for rung in (required, reached):
        if rung not in rung_index:
            violation(f"contract: surface {surface} names rung {rung!r} outside the vocabulary")
    if required in rung_index and reached in rung_index and rung_index[reached] < rung_index[required]:
        below_required.append(surface)
    if not source:
        violation(f"contract: surface {surface} has no source")
    if surface in surfaces and surfaces[surface] != klass:
        violation(f"contract: surface {surface} is listed with classes {surfaces[surface]} and {klass}")
    surfaces[surface] = klass
    journeys.add((surface, journey))
if readiness_claimed and below_required:
    violation(f"contract: readiness is claimed while surfaces are below their required rung: {sorted(set(below_required))}")

static_surfaces = {
    "native-core": "functional",
    "native-daemon": "functional",
    "settlement-contracts": "functional",
    "mirror-ethereum": "hosted",
    "mirror-solana": "hosted",
    "agent-daemon": "functional",
    "agent-mcp": "functional",
    "human-service": "functional",
    "human-web": "functional",
    "platform-cli": "functional",
    "emulator": "functional",
    "docs-site": "functional",
    "ramps-toolkit": "functional",
    "reference-ramp": "hosted",
    "multichain-paxeer-boundary": "hosted",
    "multichain-one-ledger": "functional",
}
derived = dict(static_surfaces)


def directories(relative):
    path = root / relative
    if not path.is_dir():
        violation(f"{relative}: directory is missing")
        return []
    return sorted(entry.name for entry in path.iterdir() if entry.is_dir())


for name in directories("platform/hosted"):
    derived[f"hosted-{name}"] = "hosted"
for name in directories("platform/middleware"):
    if name != "conformance":
        derived[f"middleware-{name}"] = "functional"
for name in directories("platform/integrations"):
    derived[f"integration-{name}"] = "functional"
for name in directories("interop/crates"):
    stem = name[len("layerx-"):] if name.startswith("layerx-") else name
    derived["interop-" + stem[len("interop-"):] if stem.startswith("interop-") else f"interop-{stem}"] = "functional"
for name in directories("programs/crates"):
    if name.startswith("layerx-programs-"):
        derived[f"programs-{name[len('layerx-programs-'):]}"] = "functional"
try:
    reference_apps = json.loads(read("platform/examples/reference-apps.json"))
    for application in reference_apps.get("applications", []):
        derived[f"reference-app-{application['name']}"] = "functional"
except (ValueError, KeyError, TypeError) as error:
    violation(f"platform/examples/reference-apps.json: cannot read applications ({error})")

workflow = read(".github/workflows/platform.yml")
matrix = re.search(r"^\s+framework:\s*\[([^\]]*)\]", workflow, re.M)
frameworks = [item.strip() for item in matrix.group(1).split(",") if item.strip()] if matrix else []
if not frameworks:
    violation(".github/workflows/platform.yml: agent framework matrix 'framework: [...]' is missing")
for framework in frameworks:
    derived[f"agent-framework-{framework}"] = "functional"

for surface, klass in sorted(derived.items()):
    if surface not in surfaces:
        violation(f"contract: surface {surface} ({klass}) is missing from Surfaces and journeys")
    elif surfaces[surface] != klass:
        violation(f"contract: surface {surface} has class {surfaces[surface]!r}, expected {klass!r}")

runner_path = root / "tools/qualification/release_runner.py"
spec = importlib.util.spec_from_file_location("layerx_release_runner", runner_path)
runner = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = runner
try:
    spec.loader.exec_module(runner)
    human_journeys = sorted({case.split("/", 1)[1] for case in runner.journey_cases()})
except Exception as error:
    human_journeys = []
    violation(f"tools/qualification/release_runner.py: cannot import journey_cases ({error})")
for journey in human_journeys:
    if ("human-web", journey) not in journeys:
        violation(f"contract: human-web journey {journey} from release_runner.journey_cases() is missing")
for journey in ("deploy", "paid-call", "restart"):
    if ("programs-runtime", journey) not in journeys:
        violation(f"contract: programs-runtime journey {journey} is missing")

testnet = load_manifest("platform/hosted/testnet/deployment.yaml")
gateway = load_manifest("platform/hosted/gateway/deployment.yaml")
webhooks = load_manifest("platform/hosted/webhooks/deployment.yaml")
ramps = load_manifest("platform/ramps/deployment.yaml")
node = load_manifest("platform/hosted/node/deployment.yaml")
identity_manifest = load_manifest("platform/hosted/identity/deployment.yaml")
paxeer = load_manifest("platform/hosted/paxeer/deployment.yaml")
registry_manifest = load_manifest("platform/hosted/registry/deployment.yaml")
install = read("platform/docs/content/install.md")
emulator_doc = read("platform/docs/content/environments/emulator.md")
docs_testnet = read("platform/docs/testnet.md")
testnet_lib = read("platform/hosted/testnet/src/lib.rs")
wire_limits = read("agent/crates/layerx-wire/src/limits.rs")
registries = parse_kvx("platform/release/registries.kvx")
site = parse_kvx("platform/docs/site.kvx")

endpoints = key_values(table(sections, "Beta endpoints and hostnames", ["Key", "Value", "Source"]), "Beta endpoints and hostnames")
network = key_values(table(sections, "Network id", ["Key", "Value", "Source"]), "Network id")
wire = key_values(table(sections, "Wire protocol version", ["Key", "Value", "Source"]), "Wire protocol version")
ca = key_values(table(sections, "Beta CA", ["Key", "Value", "Source"]), "Beta CA")

contradictions = []


def contradiction(key, canonical, divergent):
    contradictions.append((key, canonical, divergent))


def workflow_env(name):
    values = sorted(set(re.findall(rf"^\s+{name}:\s*(\S+)\s*$", workflow, re.M)))
    if not values:
        violation(f".github/workflows/platform.yml: {name} is not set")
        return None
    if len(values) > 1:
        violation(f".github/workflows/platform.yml: {name} carries different values {values}")
    return values[0]


def rust_literal(text, relative, field):
    match = re.search(rf"{re.escape(field)}:\s*\"([^\"]+)\"", text)
    if match is None:
        violation(f"{relative}: {field} literal is missing")
        return None
    return match.group(1)


testnet_ingress_hosts = manifest_hosts(testnet)
testnet_ingress = [facts for facts in testnet if facts.kind == "Ingress" and facts.name == "layerx-testnet-public"]
if not testnet_ingress or not testnet_ingress[0].hosts:
    violation("platform/hosted/testnet/deployment.yaml: Ingress layerx-testnet-public with a host is missing")
else:
    expect("Beta endpoints and hostnames", endpoints, "testnet_public_url", f"https://{testnet_ingress[0].hosts[0]}", "platform/hosted/testnet/deployment.yaml Ingress layerx-testnet-public")
for key, name in (("testnet_public_url", "LAYERX_TESTNET_URL"), ("gateway_url", "LAYERX_GATEWAY_URL"), ("faucet_url", "LAYERX_FAUCET_URL")):
    value = workflow_env(name)
    if value is not None:
        expect("Beta endpoints and hostnames", endpoints, key, value, f".github/workflows/platform.yml {name}")
for key, field in (("testnet_public_url", "public_endpoint"), ("gateway_url", "gateway_endpoint"), ("faucet_url", "faucet_endpoint"), ("status_url", "status_endpoint")):
    value = rust_literal(testnet_lib, "platform/hosted/testnet/src/lib.rs", field)
    if value is not None:
        expect("Beta endpoints and hostnames", endpoints, key, value, f"platform/hosted/testnet/src/lib.rs {field}")
try:
    status_surface = json.loads(read("platform/hosted/testnet/status.json"))
    for component in status_surface.get("components", []):
        expect("Beta endpoints and hostnames", endpoints, "testnet_public_url", origin(component["source"]), f"platform/hosted/testnet/status.json component {component.get('id')}")
except (ValueError, KeyError, TypeError) as error:
    violation(f"platform/hosted/testnet/status.json: cannot read components ({error})")

docs_public = re.search(r"at `(https://[^`]+)`", docs_testnet)
docs_gateway = re.search(r"The developer gateway is `(https://[^`]+)`", docs_testnet)
docs_faucet = re.search(r"`(https://faucet[^`]+)/v1/faucet/claims`", docs_testnet)
docs_status = re.search(r"`(https://status[^`/]+)/testnet-resets\.ics`", docs_testnet)
docs_wire = re.search(r"LXP wire protocol version `(\d+)`", docs_testnet)
docs_network = re.search(r"network ID `(\d+)`", docs_testnet)
for key, match, label in (
    ("testnet_public_url", docs_public, "public endpoint"),
    ("gateway_url", docs_gateway, "developer gateway"),
    ("faucet_url", docs_faucet, "faucet claims origin"),
    ("status_url", docs_status, "status host"),
):
    if match is None:
        violation(f"platform/docs/testnet.md: {label} is missing")
    else:
        expect("Beta endpoints and hostnames", endpoints, key, match.group(1), f"platform/docs/testnet.md {label}")

for key, name in (
    ("testnet_core_url", "LAYERX_TESTNET_CORE_URL"),
    ("testnet_core_admin_url", "LAYERX_TESTNET_CORE_ADMIN_URL"),
    ("testnet_gateway_url", "LAYERX_TESTNET_GATEWAY_URL"),
    ("testnet_paxeer_url", "LAYERX_TESTNET_PAXEER_URL"),
):
    value = manifest_value(testnet, name)
    if value is None:
        violation(f"platform/hosted/testnet/deployment.yaml: {name} is not set")
    else:
        expect("Beta endpoints and hostnames", endpoints, key, value, f"platform/hosted/testnet/deployment.yaml {name}")
for key, name in (
    ("gateway_component_url", "LAYERX_GATEWAY_COMPONENT_URL"),
    ("gateway_authority_url", "LAYERX_GATEWAY_AUTHORITY_URL"),
    ("gateway_identity_url", "LAYERX_GATEWAY_IDENTITY_URL"),
    ("gateway_program_registry_url", "LAYERX_GATEWAY_PROGRAM_REGISTRY_URL"),
):
    value = manifest_value(gateway, name)
    if value is None:
        violation(f"platform/hosted/gateway/deployment.yaml: {name} is not set")
    else:
        expect("Beta endpoints and hostnames", endpoints, key, value, f"platform/hosted/gateway/deployment.yaml {name}")

developer_hosts = manifest_hosts(webhooks)
if not developer_hosts:
    violation("platform/hosted/webhooks/deployment.yaml: no Ingress host")
elif len(developer_hosts) > 1:
    violation(f"platform/hosted/webhooks/deployment.yaml: Ingress hosts differ: {developer_hosts}")
else:
    expect("Beta endpoints and hostnames", endpoints, "developer_host", developer_hosts[0], "platform/hosted/webhooks/deployment.yaml Ingress")
ramp_hosts = manifest_hosts(ramps)
if len(ramp_hosts) != 1:
    violation(f"platform/ramps/deployment.yaml: expected one Ingress host, found {ramp_hosts}")
else:
    expect("Beta endpoints and hostnames", endpoints, "ramp_host", ramp_hosts[0], "platform/ramps/deployment.yaml Ingress")

emulator_values = set()
for relative, text in (("platform/docs/content/install.md", install), ("platform/docs/content/environments/emulator.md", emulator_doc)):
    match = re.search(r"layerx environment use emulator --endpoint (\S+) --network-id (\d+)", text)
    if match is None:
        violation(f"{relative}: emulator 'layerx environment use' line is missing")
        continue
    emulator_values.add(match.groups())
    expect("Beta endpoints and hostnames", endpoints, "emulator_endpoint", match.group(1), f"{relative} emulator endpoint")
    expect("Network id", network, "network_id", match.group(2), f"{relative} --network-id")
lib_network = re.search(r"TESTNET_NETWORK_ID:\s*u32\s*=\s*(\d+)", testnet_lib)
if lib_network is None:
    violation("platform/hosted/testnet/src/lib.rs: TESTNET_NETWORK_ID is missing")
else:
    expect("Network id", network, "network_id", lib_network.group(1), "platform/hosted/testnet/src/lib.rs TESTNET_NETWORK_ID")
if docs_network is None:
    violation("platform/docs/testnet.md: network ID is missing")
else:
    expect("Network id", network, "network_id", docs_network.group(1), "platform/docs/testnet.md network ID")
gateway_network = manifest_value(gateway, "LAYERX_GATEWAY_NETWORK_ID")
if gateway_network is None:
    violation("platform/hosted/gateway/deployment.yaml: LAYERX_GATEWAY_NETWORK_ID is not set")
else:
    expect("Network id", network, "gateway_network_id", gateway_network, "platform/hosted/gateway/deployment.yaml LAYERX_GATEWAY_NETWORK_ID")
protocol_network = manifest_value(gateway, "LAYERX_GATEWAY_PROTOCOL_NETWORK_ID")
if protocol_network is None:
    violation("platform/hosted/gateway/deployment.yaml: LAYERX_GATEWAY_PROTOCOL_NETWORK_ID is not set")
elif protocol_network != network.get("network_id"):
    contradiction("protocol_network_id", network.get("network_id", ""), protocol_network)

configmaps = [facts for facts in testnet if facts.kind == "ConfigMap" and facts.name == "layerx-testnet-release"]
if not configmaps:
    violation("platform/hosted/testnet/deployment.yaml: ConfigMap layerx-testnet-release is missing")
else:
    release_data = configmaps[0].data
    for key, name in (("wire_protocol_version", "lxp-wire-protocol-version"), ("package_semver", "package-semver")):
        if name not in release_data:
            violation(f"platform/hosted/testnet/deployment.yaml: ConfigMap layerx-testnet-release lacks {name}")
        else:
            expect("Wire protocol version", wire, key, release_data[name], f"platform/hosted/testnet/deployment.yaml ConfigMap layerx-testnet-release {name}")
for documents, relative, name in (
    (gateway, "platform/hosted/gateway/deployment.yaml", "LAYERX_GATEWAY_LXP_WIRE_VERSION"),
    (webhooks, "platform/hosted/webhooks/deployment.yaml", "LAYERX_WEBHOOKS_LXP_WIRE_VERSION"),
):
    value = manifest_value(documents, name)
    if value is None:
        violation(f"{relative}: {name} is not set")
    else:
        expect("Wire protocol version", wire, "wire_protocol_version", value, f"{relative} {name}")
wire_constant = re.search(r"pub const STATE_COMMITMENT_PROTOCOL_VERSION:\s*u16\s*=\s*(\d+);", wire_limits)
if wire_constant is None:
    violation("agent/crates/layerx-wire/src/limits.rs: STATE_COMMITMENT_PROTOCOL_VERSION is missing")
else:
    expect("Wire protocol version", wire, "wire_protocol_version", wire_constant.group(1), "agent/crates/layerx-wire/src/limits.rs STATE_COMMITMENT_PROTOCOL_VERSION")
legacy_default = re.search(r"pub const PROTOCOL_VERSION:\s*u16\s*=\s*(\d+);", wire_limits)
if legacy_default is None or legacy_default.group(1) != "2":
    violation("agent/crates/layerx-wire/src/limits.rs: default protocol compatibility must remain 2")
if docs_wire is None:
    violation("platform/docs/testnet.md: LXP wire protocol version is missing")
elif docs_wire.group(1) != wire.get("wire_protocol_version"):
    contradiction("docs_wire_protocol_version", wire.get("wire_protocol_version", ""), docs_wire.group(1))
jvm_version = re.search(r"`com\.sidiora\.layerx:layerx-sdk:([0-9][^`]*)`", install)
if jvm_version is None:
    violation("platform/docs/content/install.md: JVM coordinate with a version is missing")
else:
    expect("Wire protocol version", wire, "package_semver", jvm_version.group(1), "platform/docs/content/install.md JVM coordinate version")

gateway_secrets = manifest_secrets(gateway)
testnet_secrets = manifest_secrets(testnet)
for key, secrets, relative in (
    ("internal_ca_secret", gateway_secrets, "platform/hosted/gateway/deployment.yaml"),
    ("gateway_ingress_tls_secret", gateway_secrets, "platform/hosted/gateway/deployment.yaml"),
    ("testnet_control_tls_secret", testnet_secrets, "platform/hosted/testnet/deployment.yaml"),
    ("testnet_ingress_tls_secret", testnet_secrets, "platform/hosted/testnet/deployment.yaml"),
):
    if key not in ca:
        violation(f"contract: 'Beta CA' lacks key {key}")
    elif ca[key] not in secrets:
        violation(f"contract: {key} {ca[key]!r} is not a secretName in {relative}")
for key in ("ca_file_env", "ca_ci_secret"):
    if key not in ca:
        violation(f"contract: 'Beta CA' lacks key {key}")
    elif not re.search(rf"\b{re.escape(ca[key])}\b", workflow):
        violation(f"contract: {key} {ca[key]!r} is not named in .github/workflows/platform.yml")
if not ca.get("ca_material"):
    violation("contract: 'Beta CA' lacks ca_material")

gateway_hosts = manifest_hosts(gateway)
gateway_host = urlsplit(endpoints.get("gateway_url", "")).hostname or ""
for host in gateway_hosts:
    if host != gateway_host:
        contradiction("gateway_hostname", gateway_host, host)
if not gateway_hosts:
    violation("platform/hosted/gateway/deployment.yaml: no Ingress host")
all_ingress_hosts = testnet_ingress_hosts + gateway_hosts + developer_hosts + ramp_hosts
faucet_host = urlsplit(endpoints.get("faucet_url", "")).hostname or ""
if faucet_host and faucet_host not in all_ingress_hosts:
    contradiction("faucet_hostname", faucet_host, "(no ingress host)")
for host in all_ingress_hosts:
    if beta_domain and host != beta_domain and not host.endswith("." + beta_domain):
        contradiction("placeholder_hostname", beta_domain, host)
service_ports = {}
for facts in testnet + gateway + webhooks + ramps + node + identity_manifest + paxeer + registry_manifest:
    if facts.kind == "Service" and facts.name:
        service_ports.setdefault(facts.name, []).extend(facts.ports)
for service, relative in (
    ("layerx-pending-core", "platform/hosted/node/deployment.yaml"),
    ("layerx-pending-core-admin", "platform/hosted/node/deployment.yaml"),
    ("layerx-receipt-authority", "platform/hosted/node/deployment.yaml"),
    ("layerx-agent-boundary", "platform/hosted/node/deployment.yaml"),
    ("layerx-identity", "platform/hosted/identity/deployment.yaml"),
    ("paxeer-boundary", "platform/hosted/paxeer/deployment.yaml"),
):
    if service not in service_ports:
        violation(f"{relative}: Service {service} is missing")
for key in (
    "testnet_core_url",
    "testnet_core_admin_url",
    "testnet_gateway_url",
    "testnet_paxeer_url",
    "gateway_component_url",
    "gateway_authority_url",
    "gateway_identity_url",
    "gateway_program_registry_url",
):
    value = endpoints.get(key)
    if not value:
        continue
    service = (urlsplit(value).hostname or "").split(".", 1)[0]
    ports = service_ports.get(service)
    if not ports:
        contradiction(f"{key}_service", service, "(no Service in the hosted manifests)")
    elif url_port(value) not in ports:
        contradiction(f"{key}_port", "/".join(ports), url_port(value))

release = registries.get("release", {})
registry_ids = release.get("registries", [])
if not isinstance(registry_ids, list) or not registry_ids:
    violation("platform/release/registries.kvx: [release] registries is missing")
    registry_ids = []
artifact_rows = table(sections, "Artifact set", ["Ecosystem", "Registry", "Surface", "Packages", "Publication job"])
artifact_keys = key_values(table(sections, "Artifact set", ["Key", "Value"], 1), "Artifact set")
recognisers = {
    "npm": r"\bnpm publish\b",
    "crates-io": r"\bcargo publish\b",
    "pypi": r"\btwine upload\b|pypi-publish",
    "maven-central": r"\bmvn deploy\b|\bgradle publish\b",
    "nuget": r"\bdotnet nuget push\b",
    "go-modules": r"\bgit tag\b|\bgit push --tags\b",
    "swiftpm": r"\bgit tag\b|\bgit push --tags\b",
}
listed = [row[0] for row in artifact_rows]
if listed != list(registry_ids):
    violation(f"contract: Artifact set ecosystems {listed} differ from platform/release/registries.kvx registries {list(registry_ids)}")
for row in artifact_rows:
    ecosystem, registry, surface, packages, job = row
    declared = registries.get(f"registry.{ecosystem}")
    if declared is None:
        violation(f"contract: ecosystem {ecosystem} has no [registry.{ecosystem}] in platform/release/registries.kvx")
        continue
    if registry != declared.get("distribution"):
        violation(f"contract: ecosystem {ecosystem} registry {registry!r} differs from distribution {declared.get('distribution')!r}")
    if surface not in surfaces:
        violation(f"contract: ecosystem {ecosystem} names unknown surface {surface!r}")
    declared_packages = declared.get("packages", [])
    if not isinstance(declared_packages, list):
        declared_packages = [declared_packages]
    listed_packages = [item.strip() for item in packages.split(",") if item.strip()]
    if listed_packages != sorted(declared_packages):
        violation(f"contract: ecosystem {ecosystem} packages {listed_packages} differ from registries.kvx packages {sorted(declared_packages)}")
    recogniser = recognisers.get(ecosystem)
    if recogniser is None:
        violation(f"contract: ecosystem {ecosystem} has no publication recogniser")
        continue
    present = "present" if re.search(recogniser, workflow) else "absent"
    if job != present:
        violation(f"contract: ecosystem {ecosystem} publication job is {job!r} but .github/workflows/platform.yml shows {present!r}")
manifest_path = artifact_keys.get("artifact_manifest_path", "")
manifest_status = artifact_keys.get("artifact_manifest_status")
manifest_exists = False
artifact_manifest = None
declared_by_registry = {}
for ecosystem in registry_ids:
    declared = registries.get(f"registry.{ecosystem}", {}).get("packages", [])
    declared_by_registry[ecosystem] = list(declared) if isinstance(declared, list) else [declared]


def load_artifact_manifest(relative):
    text = read(relative)
    if not text:
        return None
    try:
        document = json.loads(text)
    except ValueError as error:
        violation(f"{relative}: not JSON ({error})")
        return None
    if not isinstance(document, dict) or document.get("schema") != "layerx/artifact-manifest/1":
        violation(f"{relative}: schema is not layerx/artifact-manifest/1")
        return None
    entries = document.get("artifacts")
    if not isinstance(entries, list) or not entries:
        violation(f"{relative}: artifacts list is missing or empty")
        return None
    required = ("name", "version", "registry", "digest", "digest_of", "signature", "sbom", "attestation", "source_revision", "published", "install_check")
    listed = []
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict) or any(field not in entry for field in required):
            violation(f"{relative}: artifacts[{index}] lacks one of {', '.join(required)}")
            continue
        if entry["registry"] not in registry_ids:
            violation(f"{relative}: {entry['name']}@{entry['version']} names unknown registry {entry['registry']!r}")
            continue
        if entry["name"] not in declared_by_registry[entry["registry"]]:
            violation(f"{relative}: {entry['name']}@{entry['version']} from {entry['registry']} is not declared in platform/release/registries.kvx")
            continue
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", str(entry["digest"])):
            violation(f"{relative}: {entry['name']}@{entry['version']} digest {entry['digest']!r} is not sha256:<64 hex>")
        if not re.fullmatch(r"[0-9a-f]{40}", str(entry["source_revision"])):
            violation(f"{relative}: {entry['name']}@{entry['version']} source_revision {entry['source_revision']!r} is not a 40-hex commit")
        listed.append(entry)
    for ecosystem, packages in declared_by_registry.items():
        for package in packages:
            if not any(entry["registry"] == ecosystem and entry["name"] == package for entry in listed):
                violation(f"{relative}: declared package {package} from {ecosystem} is not listed")
    return listed


if not manifest_path:
    violation("contract: Artifact set lacks artifact_manifest_path")
else:
    manifest_exists = (root / manifest_path).is_file()
    expected_status = "emitted" if manifest_exists else "not_emitted"
    if manifest_status != expected_status:
        violation(f"contract: artifact_manifest_status is {manifest_status!r} but {manifest_path} {'exists' if manifest_exists else 'does not exist'}")
    if manifest_exists:
        artifact_manifest = load_artifact_manifest(manifest_path)
expect("Artifact set", artifact_keys, "release_tag_format", str(release.get("tag_format", "")), "platform/release/registries.kvx tag_format")
expect("Artifact set", artifact_keys, "source_digest", str(release.get("source_digest", "")), "platform/release/registries.kvx source_digest")

verification_job = re.search(r"^  (release-verification):\n((?:    .*\n|\n)*)", workflow, re.M)
if verification_job is None:
    violation(".github/workflows/platform.yml: job release-verification is missing")
    verification_text = ""
    expect("Artifact set", artifact_keys, "artifact_manifest_verification_job", "", ".github/workflows/platform.yml release-verification job")
else:
    verification_text = verification_job.group(2)
    expect("Artifact set", artifact_keys, "artifact_manifest_verification_job", verification_job.group(1), ".github/workflows/platform.yml release-verification job")
emitter = re.search(r"-p layerx-platform-release -- manifest\b", verification_text)
expect("Artifact set", artifact_keys, "artifact_manifest_emitter", "layerx-platform-release -- manifest" if emitter else "", ".github/workflows/platform.yml release-verification manifest step")
verifier = re.search(r"-p layerx-platform-release -- verify\b", verification_text)
expect("Artifact set", artifact_keys, "artifact_manifest_verifier", "layerx-platform-release -- verify" if verifier else "", ".github/workflows/platform.yml release-verification verify step")
if verification_text and not re.search(r"--fetch\s", verification_text):
    violation(".github/workflows/platform.yml: release-verification does not fetch the published artifacts from their registries (--fetch)")
retained = re.search(r"uses: actions/upload-artifact@[^\n]*\n\s+with:\n\s+name: (\S+)\n\s+path: (\S+)", verification_text)
expect("Artifact set", artifact_keys, "artifact_manifest_workflow_artifact", retained.group(1) if retained else "", ".github/workflows/platform.yml release-verification upload-artifact name")
if retained and not retained.group(2).endswith("/" + Path(manifest_path).name):
    violation(f".github/workflows/platform.yml: release-verification retains {retained.group(2)}, not a file named {Path(manifest_path).name}")
promotion_needs = re.search(r"^  release-promotion:\n(?:    .*\n)*?    needs: \[([^\]]*)\]", workflow, re.M)
if promotion_needs is None:
    violation(".github/workflows/platform.yml: job release-promotion has no needs list")
elif "release-verification" not in [item.strip() for item in promotion_needs.group(1).split(",")]:
    violation(".github/workflows/platform.yml: release-promotion does not need release-verification")

install_rows = table(sections, "Artifact set/Install coordinates", ["Language", "Coordinate", "Ecosystem"])
install_table = re.search(r"^\| Language \| Install \|\n\|[-| ]+\|\n((?:\|.*\n?)+)", install, re.M)
documented = {}
if install_table is None:
    violation("platform/docs/content/install.md: '| Language | Install |' table is missing")
else:
    for line in install_table.group(1).splitlines():
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        span = re.search(r"`([^`]+)`", cells[1]) if len(cells) == 2 else None
        if span is None:
            violation(f"platform/docs/content/install.md: install row {line!r} has no backtick coordinate")
            continue
        documented[cells[0]] = span.group(1).split()[-1]
contract_install = {row[0]: (row[1], row[2]) for row in install_rows}
for language, coordinate in documented.items():
    if language not in contract_install:
        violation(f"contract: install coordinate for {language} ({coordinate}) is missing")
    elif contract_install[language][0] != coordinate:
        violation(f"contract: install coordinate for {language} is {contract_install[language][0]!r} but install.md carries {coordinate!r}")


def coordinate_identity(coordinate, ecosystem):
    if ecosystem == "maven-central":
        parts = coordinate.split(":")
        if len(parts) == 3:
            return ":".join(parts[:2]), parts[2]
    return coordinate, None


for language, (coordinate, ecosystem) in contract_install.items():
    if language not in documented:
        violation(f"contract: install coordinate for {language} is not in install.md")
    if ecosystem not in registry_ids:
        violation(f"contract: install coordinate for {language} names unknown ecosystem {ecosystem!r}")
        continue
    identity, pinned = coordinate_identity(coordinate, ecosystem)
    if manifest_exists:
        if artifact_manifest is None:
            continue
        matches = [entry for entry in artifact_manifest if entry["registry"] == ecosystem and entry["name"] == identity]
        if not matches:
            violation(f"contract: install coordinate for {language} ({coordinate}) names an artifact {manifest_path} does not list for {ecosystem}")
        elif pinned is not None and all(entry["version"] != pinned for entry in matches):
            violation(f"contract: install coordinate for {language} ({coordinate}) pins version {pinned!r} but {manifest_path} lists {sorted({entry['version'] for entry in matches})}")
    elif manifest_status == "not_emitted":
        declared_packages = declared_by_registry.get(ecosystem, [])
        if identity not in declared_packages:
            contradiction("install_package_unlisted", ", ".join(sorted(declared_packages)), coordinate)
    else:
        violation(f"contract: install coordinate for {language} ({coordinate}) is unlisted: {manifest_path} is absent and the contract does not state artifact_manifest_status not_emitted")

page_ids = [section[len("page."):] for section in site if section.startswith("page.")]
if not page_ids:
    violation("platform/docs/site.kvx: no [page.*] sections")
docs_rows = table(sections, "Documentation journeys", ["Page", "Surface", "Journey"])
contract_pages = {}
for page, surface, journey in docs_rows:
    if page in contract_pages:
        violation(f"contract: docs page {page} listed twice")
    contract_pages[page] = surface
    for item in surface.split(","):
        if item.strip() not in surfaces:
            violation(f"contract: docs page {page} names unknown surface {item.strip()!r}")
    if not journey:
        violation(f"contract: docs page {page} has no journey")
for page in page_ids:
    if page not in contract_pages:
        violation(f"contract: docs journey {page} from platform/docs/site.kvx is not named")
for page in contract_pages:
    if page not in page_ids:
        violation(f"contract: docs journey {page} is not a page in platform/docs/site.kvx")

for heading in ("Unknown-state behaviour", "Architecture summary"):
    section = sections.get(heading)
    if section is None:
        violation(f"contract: heading '{heading}' is missing")
    elif not any(line.strip() for line in section["lines"]):
        violation(f"contract: '{heading}' is empty")

dependency_rows = table(sections, "External dependencies", ["Dependency", "Production counterpart", "Beta counterpart", "Owner input names"])
beta_counterparts = " ".join(row[2] for row in dependency_rows)
mirror_dir = root / "interop/deploy/mirror"
network_names = set()
for record in sorted(mirror_dir.glob("*.json")) if mirror_dir.is_dir() else []:
    try:
        document = json.loads(record.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        violation(f"{record.relative_to(root)}: cannot read ({error})")
        continue
    if isinstance(document.get("network"), str):
        network_names.add(document["network"])
    for deployment in document.get("deployments", []) or []:
        if isinstance(deployment, dict) and isinstance(deployment.get("network"), str):
            network_names.add(deployment["network"])
if not network_names:
    violation("interop/deploy/mirror: no deployment network names found")
for name in sorted(network_names):
    if name not in beta_counterparts:
        violation(f"contract: External dependencies do not name the mirror network {name}")
for row in dependency_rows:
    if not row[3]:
        violation(f"contract: dependency {row[0]!r} names no owner input")

difference_rows = table(sections, "Beta-versus-production differences", ["Key", "Difference"])
polish_boundary = {
    "ui_polish",
    "visual_regression",
    "automated_accessibility",
    "usability_studies",
    "performance_budgets_and_soak",
    "external_security_audit",
    "production_infrastructure",
    "production_certification",
}
difference_keys = [row[0] for row in difference_rows]
for key in difference_keys:
    if key not in polish_boundary:
        violation(f"contract: difference {key!r} is outside the polish boundary")
for key in sorted(polish_boundary - set(difference_keys)):
    violation(f"contract: difference {key} from the polish boundary is missing")
for row in difference_rows:
    if not row[1]:
        violation(f"contract: difference {row[0]} has no text")

contradiction_rows = table(sections, "Contradictions", ["Key", "Canonical value", "Divergent source", "Divergent value", "Resolving task"])
listed_contradictions = {(row[0], row[1], row[3]) for row in contradiction_rows}
computed_contradictions = set(contradictions)
for key, canonical, divergent in sorted(computed_contradictions - listed_contradictions):
    violation(f"contradiction {key}: canonical {canonical!r} disagrees with source value {divergent!r} and the contract does not list it")
for key, canonical, divergent in sorted(listed_contradictions - computed_contradictions):
    violation(f"contract: listed contradiction {key} (canonical {canonical!r}, divergent {divergent!r}) is not present in the sources")
for row in contradiction_rows:
    if not row[2] or not row[4]:
        violation(f"contract: contradiction {row[0]} lacks a divergent source or a resolving task")
if readiness_claimed and computed_contradictions:
    violation("contract: readiness is claimed while contradictions exist")

if violations:
    print(f"beta-contract-check: {len(violations)} violation(s)", file=sys.stderr)
    for message in violations:
        print(f"  {message}", file=sys.stderr)
    sys.exit(1)
print(
    f"beta-contract-check: ok ({len(surfaces)} surfaces, {len(journeys)} surface journeys, "
    f"{len(page_ids)} docs journeys, {len(registry_ids)} ecosystems, "
    f"{len(computed_contradictions)} contradictions listed, readiness_claim={readiness})"
)
PY
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    beta_contract_check "$@"
fi
