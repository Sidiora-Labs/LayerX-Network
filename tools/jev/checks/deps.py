from __future__ import annotations

import argparse
import json
import subprocess
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable, Mapping, Sequence

from tools.jev.checks import register
from tools.jev.client import Evaluation, JevClient, choice, score
from tools.jev.report import Finding, Report, git_revision, route_for


CHECK_NAME = "deps"
REPO_ROOT = Path(__file__).resolve().parents[3]
WORKSPACES = ("agent", "human", "platform", "programs", "interop")
MANIFEST = "Cargo.toml"
LOCK_FILE = "Cargo.lock"
PRODUCTION = "production"
DEV_ONLY = "dev_only"
UNKNOWN = "unknown"
NORMAL_TABLES = ("dependencies", "build-dependencies")
DEV_TABLE = "dev-dependencies"
DENY_COMMAND = (
    "cargo",
    "deny",
    "--locked",
    "--format",
    "json",
    "check",
    "advisories",
    "bans",
    "sources",
)
DENY_TIMEOUT_SECONDS = 900.0
GRAPH_DEPTH = 12
ADVISORY_TEXT_LIMIT = 1200
PRIORITY_QUESTION = "priority"
URGENCY_QUESTION = "urgency"
PRIORITY_INSTRUCTIONS = (
    "The state holds one cargo-deny advisory against the LayerX Rust workspaces. "
    "Pick the option that matches how the advisory should be triaged. "
    "Use only the fields in the state."
)
PRIORITY_OPTIONS = {
    "production_reachable": (
        "The flagged crate is pulled in by a dependency that ships in production code, "
        "so the advisory reaches the deployed binaries."
    ),
    "dev_only": (
        "The flagged crate is pulled in only by development dependencies such as tests, "
        "benches or fuzz targets, so it does not reach the deployed binaries."
    ),
    "already_mitigated": (
        "The advisory text says the problem is withdrawn, purely informational, or already "
        "addressed in the recorded crate versions, so no dependency change is needed."
    ),
    "needs_human": (
        "The state does not carry enough evidence for any other option and a human "
        "maintainer has to decide."
    ),
}
URGENCY_INSTRUCTIONS = (
    "The state holds one cargo-deny advisory against the LayerX Rust workspaces. "
    "Rate how soon a maintainer has to act on it. Use only the fields in the state."
)
URGENCY_LABELS = ("low", "medium", "high")
URGENCY_LEVELS = (
    "low: informational or unreachable in practice, safe to leave for the next routine "
    "dependency bump",
    "medium: a real weakness that belongs in the next planned dependency update",
    "high: an exploitable weakness on a shipping path that has to be fixed before the "
    "next release",
)


@dataclass(frozen=True)
class Diagnostic:
    workspace: str
    advisory_id: str
    code: str
    severity: str
    crate: str
    version: str
    message: str
    description: str
    graph_path: tuple[str, ...]
    anchor: str


@dataclass(frozen=True)
class Advisory:
    advisory_id: str
    code: str
    severity: str
    crate: str
    message: str
    description: str
    versions: tuple[str, ...]
    workspaces: tuple[str, ...]
    anchors: tuple[str, ...]
    graph_path: tuple[str, ...]
    dev_flag: str
    dev_by_workspace: Mapping[str, str]

    @property
    def anchor(self) -> str:
        if self.anchors:
            return self.anchors[0]
        return f"advisory:{self.advisory_id}"


@dataclass(frozen=True)
class WorkspaceDeps:
    normal: frozenset[str]
    dev: frozenset[str]

    def classify(self, crate: str) -> str:
        if crate in self.normal:
            return PRODUCTION
        if crate in self.dev:
            return DEV_ONLY
        return UNKNOWN


@dataclass
class DependencyIndex:
    repo_root: Path
    scans: dict[str, WorkspaceDeps] = field(default_factory=dict)

    def workspace(self, workspace: str) -> WorkspaceDeps:
        cached = self.scans.get(workspace)
        if cached is None:
            cached = scan_workspace(self.repo_root / workspace)
            self.scans[workspace] = cached
        return cached

    def classify(self, workspace: str, crate: str) -> str:
        return self.workspace(workspace).classify(crate)


def load_manifest(path: Path) -> dict[str, object]:
    try:
        document = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError):
        return {}
    return document


def workspace_manifests(directory: Path) -> list[Path]:
    root = directory / MANIFEST
    if not root.is_file():
        return []
    manifests = [root]
    workspace = load_manifest(root).get("workspace")
    members = workspace.get("members") if isinstance(workspace, dict) else None
    if isinstance(members, list):
        for member in members:
            if not isinstance(member, str):
                continue
            if "*" in member:
                manifests.extend(sorted(directory.glob(f"{member}/{MANIFEST}")))
                continue
            candidate = directory / member / MANIFEST
            if candidate.is_file():
                manifests.append(candidate)
    ordered: list[Path] = []
    seen: set[Path] = set()
    for manifest in manifests:
        if manifest in seen:
            continue
        seen.add(manifest)
        ordered.append(manifest)
    return ordered


def declared_names(table: object) -> set[str]:
    if not isinstance(table, dict):
        return set()
    names: set[str] = set()
    for key, value in table.items():
        if isinstance(value, dict):
            renamed = value.get("package")
            names.add(renamed if isinstance(renamed, str) else str(key))
            continue
        names.add(str(key))
    return names


def scan_workspace(directory: Path) -> WorkspaceDeps:
    normal: set[str] = set()
    dev: set[str] = set()
    for manifest in workspace_manifests(directory):
        document = load_manifest(manifest)
        for table in NORMAL_TABLES:
            normal |= declared_names(document.get(table))
        dev |= declared_names(document.get(DEV_TABLE))
        targets = document.get("target")
        if not isinstance(targets, dict):
            continue
        for entry in targets.values():
            if not isinstance(entry, dict):
                continue
            for table in NORMAL_TABLES:
                normal |= declared_names(entry.get(table))
            dev |= declared_names(entry.get(DEV_TABLE))
    return WorkspaceDeps(normal=frozenset(normal), dev=frozenset(dev - normal))


def krate_of(node: object) -> tuple[str, str]:
    if not isinstance(node, dict):
        return "", ""
    krate = node.get("Krate")
    if not isinstance(krate, dict):
        return "", ""
    name = krate.get("name")
    version = krate.get("version")
    return (
        name if isinstance(name, str) else "",
        version if isinstance(version, str) else "",
    )


def graph_path(node: object, limit: int = GRAPH_DEPTH) -> tuple[str, ...]:
    path: list[str] = []
    current = node
    while isinstance(current, dict) and len(path) < limit:
        name, version = krate_of(current)
        if not name:
            break
        path.append(f"{name} {version}".strip())
        parents = current.get("parents")
        if not isinstance(parents, list) or not parents:
            break
        current = parents[0]
    return tuple(path)


def span_version(span: object) -> str:
    if not isinstance(span, str):
        return ""
    parts = span.split()
    if len(parts) < 2:
        return ""
    return parts[1]


def diagnostic_from(document: object, workspace: str) -> Diagnostic | None:
    if not isinstance(document, dict) or document.get("type") != "diagnostic":
        return None
    fields = document.get("fields")
    if not isinstance(fields, dict):
        return None
    advisory = fields.get("advisory")
    if not isinstance(advisory, dict):
        return None
    advisory_id = advisory.get("id")
    if not isinstance(advisory_id, str) or not advisory_id:
        return None
    graphs = fields.get("graphs")
    root = graphs[0] if isinstance(graphs, list) and graphs else None
    crate, version = krate_of(root)
    labels = fields.get("labels")
    label = labels[0] if isinstance(labels, list) and labels and isinstance(labels[0], dict) else {}
    if not crate:
        package = advisory.get("package")
        crate = package if isinstance(package, str) else ""
    if not version:
        version = span_version(label.get("span"))
    line = label.get("line")
    if isinstance(line, int) and not isinstance(line, bool):
        anchor = f"{workspace}/{LOCK_FILE}:{line}"
    else:
        anchor = f"{workspace}:{advisory_id}"
    code = fields.get("code")
    severity = fields.get("severity")
    message = fields.get("message")
    description = advisory.get("description")
    title = advisory.get("title")
    return Diagnostic(
        workspace=workspace,
        advisory_id=advisory_id,
        code=code if isinstance(code, str) else "",
        severity=severity if isinstance(severity, str) else "",
        crate=crate,
        version=version,
        message=message if isinstance(message, str) else (title if isinstance(title, str) else ""),
        description=description if isinstance(description, str) else "",
        graph_path=graph_path(root),
        anchor=anchor,
    )


def parse_diagnostics(text: str, workspace: str) -> list[Diagnostic]:
    diagnostics: list[Diagnostic] = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            document = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        diagnostic = diagnostic_from(document, workspace)
        if diagnostic is not None:
            diagnostics.append(diagnostic)
    return diagnostics


def combine_dev_flags(flags: Iterable[str]) -> str:
    recorded = list(flags)
    if PRODUCTION in recorded:
        return PRODUCTION
    if DEV_ONLY in recorded:
        return DEV_ONLY
    return UNKNOWN


def group_advisories(
    diagnostics: Sequence[Diagnostic],
    index: DependencyIndex,
) -> list[Advisory]:
    grouped: dict[str, list[Diagnostic]] = {}
    for diagnostic in diagnostics:
        grouped.setdefault(diagnostic.advisory_id, []).append(diagnostic)
    advisories: list[Advisory] = []
    for advisory_id, items in grouped.items():
        first = items[0]
        workspaces = tuple(sorted({item.workspace for item in items}))
        dev_by_workspace = {
            workspace: index.classify(workspace, first.crate) for workspace in workspaces
        }
        advisories.append(
            Advisory(
                advisory_id=advisory_id,
                code=first.code,
                severity=first.severity,
                crate=first.crate,
                message=first.message,
                description=first.description,
                versions=tuple(sorted({item.version for item in items if item.version})),
                workspaces=workspaces,
                anchors=tuple(sorted({item.anchor for item in items})),
                graph_path=first.graph_path,
                dev_flag=combine_dev_flags(dev_by_workspace.values()),
                dev_by_workspace=dev_by_workspace,
            )
        )
    advisories.sort(key=lambda advisory: advisory.advisory_id)
    return advisories


def advisory_state(advisory: Advisory) -> dict[str, object]:
    return {
        "advisory_id": advisory.advisory_id,
        "kind": advisory.code,
        "cargo_deny_severity": advisory.severity,
        "crate": advisory.crate,
        "crate_versions": list(advisory.versions),
        "workspaces": list(advisory.workspaces),
        "dependency_kind": advisory.dev_flag,
        "dependency_kind_by_workspace": dict(sorted(advisory.dev_by_workspace.items())),
        "dependency_path": list(advisory.graph_path),
        "title": advisory.message,
        "advisory_text": advisory.description[:ADVISORY_TEXT_LIMIT],
    }


def advisory_questions() -> dict[str, dict[str, object]]:
    return {
        PRIORITY_QUESTION: choice(PRIORITY_INSTRUCTIONS, PRIORITY_OPTIONS),
        URGENCY_QUESTION: score(URGENCY_INSTRUCTIONS, list(URGENCY_LEVELS)),
    }


def urgency_label(value: float) -> str:
    index = int(round(value))
    if index < 0:
        index = 0
    if index >= len(URGENCY_LABELS):
        index = len(URGENCY_LABELS) - 1
    return URGENCY_LABELS[index]


def advisory_detail(advisory: Advisory) -> dict[str, object]:
    return {
        "advisory_id": advisory.advisory_id,
        "crate": advisory.crate,
        "versions": list(advisory.versions),
        "code": advisory.code,
        "severity": advisory.severity,
        "workspaces": list(advisory.workspaces),
        "dependency_kind": advisory.dev_flag,
        "dependency_kind_by_workspace": dict(sorted(advisory.dev_by_workspace.items())),
        "anchors": list(advisory.anchors),
    }


def findings_for(advisory: Advisory, evaluation: Evaluation) -> list[Finding]:
    findings: list[Finding] = []
    subject = f"{advisory.advisory_id} {advisory.crate}".strip()
    priority = evaluation.answers.get(PRIORITY_QUESTION)
    if priority is not None:
        detail = advisory_detail(advisory)
        detail["probabilities"] = dict(priority.probabilities)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=subject,
                anchor=advisory.anchor,
                question=PRIORITY_QUESTION,
                answer=priority.value,
                confidence=priority.confidence,
                route=route_for(priority.confidence),
                detail=detail,
            )
        )
    urgency = evaluation.answers.get(URGENCY_QUESTION)
    if urgency is not None:
        detail = advisory_detail(advisory)
        detail["probabilities"] = dict(urgency.probabilities)
        detail["score"] = urgency.value
        detail["legend"] = dict(urgency.legend)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=subject,
                anchor=advisory.anchor,
                question=URGENCY_QUESTION,
                answer=urgency_label(float(urgency.value)),
                confidence=urgency.confidence,
                route=route_for(urgency.confidence),
                detail=detail,
            )
        )
    return findings


def run_cargo_deny(directory: Path) -> str:
    if not (directory / MANIFEST).is_file():
        return ""
    completed = subprocess.run(
        DENY_COMMAND,
        cwd=directory,
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        timeout=DENY_TIMEOUT_SECONDS,
    )
    return completed.stderr


def input_pair(raw: str) -> tuple[Path, str]:
    file_name, separator, workspace = raw.partition("=")
    if not separator or not file_name or not workspace:
        raise argparse.ArgumentTypeError(f"--input expects FILE=WORKSPACE, got {raw!r}")
    return Path(file_name), workspace


def selected_workspaces(names: Sequence[str] | None) -> tuple[str, ...]:
    if not names:
        return WORKSPACES
    return tuple(names)


def collect_sources(args: argparse.Namespace, repo_root: Path) -> list[tuple[str, str]]:
    sources: list[tuple[str, str]] = []
    for path, workspace in args.input or ():
        sources.append((workspace, path.read_text(encoding="utf-8", errors="replace")))
    if args.run:
        for workspace in selected_workspaces(args.workspace):
            sources.append((workspace, run_cargo_deny(repo_root / workspace)))
    return sources


@dataclass(frozen=True)
class DepsCheck:
    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument(
            "--input",
            action="append",
            type=input_pair,
            metavar="FILE=WORKSPACE",
        )
        parser.add_argument("--run", action="store_true")
        parser.add_argument("--workspace", action="append", choices=WORKSPACES)
        parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)

    def run(self, args: argparse.Namespace, client: JevClient) -> Report:
        repo_root = args.repo_root.resolve()
        diagnostics: list[Diagnostic] = []
        for workspace, text in collect_sources(args, repo_root):
            diagnostics.extend(parse_diagnostics(text, workspace))
        index = DependencyIndex(repo_root=repo_root)
        advisories = group_advisories(diagnostics, index)
        report = Report(
            check=CHECK_NAME,
            revision=git_revision(repo_root),
            model=client.model,
        )
        questions = advisory_questions()
        for advisory in advisories:
            evaluation = client.evaluate(advisory_state(advisory), questions)
            report.calls += 1
            report.input_tokens += int(evaluation.usage.get("input_tokens", 0.0))
            report.cost_usd += evaluation.usage.get("cost", 0.0)
            report.model = evaluation.model
            report.findings.extend(findings_for(advisory, evaluation))
        return report


CHECK = register(CHECK_NAME)(DepsCheck())
add_arguments = CHECK.add_arguments
run = CHECK.run
