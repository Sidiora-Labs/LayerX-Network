from __future__ import annotations

import argparse
import itertools
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Sequence

from tools.jev.checks import register
from tools.jev.client import (
    Answer,
    Evaluation,
    JevClient,
    batched,
    choice,
    noul,
    score,
)
from tools.jev.report import Finding, Report, git_revision, route_for


CHECK = "failures"
REPO_ROOT = Path(__file__).resolve().parents[3]
MESSAGE_LIMIT = 400
MAX_RECORDS = 200
MAX_PAIRS = 256
RECORD_BATCH = 12
PAIR_BATCH = 16
CLUSTER_THRESHOLD = 0.8
CATEGORY_QUESTION = "category"
MERGE_QUESTION = "merge_block_confidence"
LINK_QUESTION = "same_root_cause"
CATEGORIES = {
    "flaky_environment": (
        "Timing, ordering, ports, resource limits or the machine itself produced the failure, "
        "not the code under test."
    ),
    "fixture_or_vector_drift": (
        "A fixture, corpus seed, golden vector or recorded expectation no longer matches the "
        "behaviour the code now produces."
    ),
    "logic_regression": (
        "The code under test behaves incorrectly: a rule, state transition or validation no "
        "longer holds."
    ),
    "toolchain_or_build": (
        "The compiler, linker, dependency resolution, sanitizer setup or build configuration "
        "produced the failure."
    ),
    "arithmetic_or_bound": (
        "An overflow, underflow, rounding, index, length or capacity bound produced the failure."
    ),
}
MERGE_BLOCK_LABELS = ("unlikely", "possible", "likely")
MERGE_BLOCK_LEVELS = (
    "unlikely: this failure should not hold up a merge",
    "possible: this failure may hold up a merge until someone looks at it",
    "likely: this failure should hold up a merge",
)
FORMAT_ORDER = ("conformance", "fuzz", "cargo", "forge", "pytest", "ctest")
FORMAT_CHOICES = ("auto", "cargo", "ctest", "forge", "pytest", "conformance", "fuzz")

CARGO_HEADER = re.compile(r"^---- (?P<name>.+?) stdout ----\s*$")
CARGO_STOP = re.compile(r"^(?:failures:|successes:|test result:)")
CARGO_PANIC = re.compile(r"panicked at (?P<location>[^\s']+:\d+(?::\d+)?)")
CARGO_FRAME = re.compile(r"^\d+:\s")
HARNESS_ASSERT = re.compile(
    r"^(?P<file>[^\s:]+\.[ch]):(?P<line>\d+)\s+(?P<message>expected=.*)$"
)
HARNESS_FAIL = re.compile(r"^FAIL\s+(?P<name>\S+)\s*$")
HARNESS_PASS = re.compile(r"^PASS\s+\S+\s*$")
FORGE_SUITE = re.compile(r"^Ran \d+ tests? for (?P<path>[^\s:]+)(?::(?P<contract>\S+))?\s*$")
FORGE_FAIL = re.compile(
    r"^\s*\[FAIL(?:[.:]\s*(?:Reason:\s*)?(?P<reason>[^\]]*))?\]\s+(?P<name>[^\s(]+\([^)]*\))"
)
PYTEST_FAIL = re.compile(r"^FAILED\s+(?P<target>\S+)(?:\s+-\s+(?P<message>.*))?\s*$")
SANITIZER = re.compile(r"ERROR:\s+(?P<tool>[A-Za-z]*Sanitizer):\s*(?P<detail>.*?)\s*$")
SANITIZER_FRAME = re.compile(
    r"^\s*#\d+\s+0x[0-9a-fA-F]+\s+in\s+\S+\s+(?P<location>[^\s]+:\d+(?::\d+)?)"
)
NONDETERMINISM = re.compile(
    r"non-deterministic\s+(?P<target>\S+)\s+outcome for input derived from\s+(?P<path>.+?)\s*$"
)
SANITIZER_FRAME_WINDOW = 24
LINE_ANCHOR = re.compile(r"^\S+:\d+(?::\d+)?$")
SLASH_TOKEN = re.compile(r"\S*/\S*")
FILE_TOKEN = re.compile(r"\b[\w.\-]+\.(?:rs|c|h|ts|tsx|py|sol|mjs|js|json|hex)(?::\d+)*")
HEX_RUN = re.compile(r"0x[0-9a-fA-F]{6,}|\b[0-9a-fA-F]{16,}\b")
DIGIT_RUN = re.compile(r"\d+")
WHITESPACE = re.compile(r"\s+")


@dataclass(frozen=True)
class RawFailure:
    name: str
    location: str
    message: str


@dataclass(frozen=True)
class FailureRecord:
    identifier: str
    format: str
    name: str
    location: str
    message: str
    source: str


def redact(message: str) -> str:
    collapsed = WHITESPACE.sub(" ", message).strip()
    return HEX_RUN.sub("<hex>", collapsed)[:MESSAGE_LIMIT]


def normalise(message: str) -> str:
    text = SLASH_TOKEN.sub("<path>", message)
    text = FILE_TOKEN.sub("<path>", text)
    text = HEX_RUN.sub("<hex>", text)
    text = DIGIT_RUN.sub("<n>", text)
    return WHITESPACE.sub(" ", text).strip().lower()


def location_file(location: str) -> str:
    return location.split(":", 1)[0]


def anchor_for(record: FailureRecord) -> str:
    if LINE_ANCHOR.match(record.location):
        return record.location
    return record.identifier


def extract_cargo(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    name = ""
    block: list[str] = []

    def flush() -> None:
        if not name:
            return
        location = ""
        collected: list[str] = []
        for line in block:
            stripped = line.strip()
            panic = CARGO_PANIC.search(stripped)
            if panic is not None:
                if not location:
                    location = panic.group("location")
                continue
            if not stripped or stripped.startswith("note:") or CARGO_FRAME.match(stripped):
                continue
            collected.append(stripped)
        failures.append(RawFailure(name=name, location=location, message=" ".join(collected)))

    for line in text.splitlines():
        header = CARGO_HEADER.match(line)
        if header is not None:
            flush()
            name = header.group("name")
            block = []
            continue
        if name and CARGO_STOP.match(line):
            flush()
            name = ""
            block = []
            continue
        if name:
            block.append(line)
    flush()
    return failures


def extract_ctest(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    pending: list[tuple[str, str]] = []
    for line in text.splitlines():
        assertion = HARNESS_ASSERT.match(line)
        if assertion is not None:
            pending.append(
                (
                    f"{assertion.group('file')}:{assertion.group('line')}",
                    assertion.group("message"),
                )
            )
            continue
        failed = HARNESS_FAIL.match(line)
        if failed is not None:
            location = pending[0][0] if pending else ""
            message = " ".join(message for _, message in pending)
            failures.append(
                RawFailure(name=failed.group("name"), location=location, message=message)
            )
            pending = []
            continue
        if HARNESS_PASS.match(line):
            pending = []
    return failures


def extract_forge(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    suite = ""
    for line in text.splitlines():
        header = FORGE_SUITE.match(line)
        if header is not None:
            suite = header.group("path")
            continue
        failed = FORGE_FAIL.match(line)
        if failed is not None:
            reason = failed.group("reason") or ""
            failures.append(
                RawFailure(
                    name=failed.group("name"),
                    location=suite,
                    message=reason.strip(),
                )
            )
    return failures


def extract_pytest(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    for line in text.splitlines():
        failed = PYTEST_FAIL.match(line)
        if failed is None:
            continue
        target = failed.group("target")
        if "::" not in target:
            continue
        path, _, name = target.partition("::")
        failures.append(
            RawFailure(name=name, location=path, message=failed.group("message") or "")
        )
    return failures


def json_documents(text: str) -> list[object]:
    try:
        return [json.loads(text)]
    except json.JSONDecodeError:
        pass
    documents: list[object] = []
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            documents.append(json.loads(stripped))
        except json.JSONDecodeError:
            continue
    return documents


def check_results(document: object) -> list[Mapping[str, object]]:
    if isinstance(document, list):
        return [item for item in document if isinstance(item, dict)]
    if isinstance(document, dict):
        for key in ("results", "checks"):
            nested = document.get(key)
            if isinstance(nested, list):
                return [item for item in nested if isinstance(item, dict)]
        if "ok" in document:
            return [document]
    return []


def extract_conformance(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    for document in json_documents(text):
        for result in check_results(document):
            if result.get("ok") is not False:
                continue
            detail = result.get("detail")
            failures.append(
                RawFailure(
                    name=str(result.get("name", "")),
                    location="",
                    message="" if detail is None else str(detail),
                )
            )
    return failures


def extract_fuzz(text: str) -> list[RawFailure]:
    failures: list[RawFailure] = []
    lines = text.splitlines()
    for index, line in enumerate(lines):
        divergence = NONDETERMINISM.search(line)
        if divergence is not None:
            failures.append(
                RawFailure(
                    name=f"non-deterministic {divergence.group('target')} outcome",
                    location=divergence.group("path"),
                    message=line.strip(),
                )
            )
            continue
        report = SANITIZER.search(line)
        if report is None:
            continue
        detail = report.group("detail")
        kind = detail.split()[0] if detail else report.group("tool")
        location = ""
        for follower in lines[index + 1 : index + 1 + SANITIZER_FRAME_WINDOW]:
            frame = SANITIZER_FRAME.match(follower)
            if frame is not None:
                location = frame.group("location")
                break
        failures.append(
            RawFailure(
                name=f"{report.group('tool')}: {kind}",
                location=location,
                message=f"{report.group('tool')}: {detail}",
            )
        )
    return failures


EXTRACTORS: Mapping[str, Callable[[str], list[RawFailure]]] = {
    "cargo": extract_cargo,
    "ctest": extract_ctest,
    "forge": extract_forge,
    "pytest": extract_pytest,
    "conformance": extract_conformance,
    "fuzz": extract_fuzz,
}


def sniff(text: str) -> str:
    counts = {name: len(EXTRACTORS[name](text)) for name in FORMAT_ORDER}
    return max(FORMAT_ORDER, key=lambda name: (counts[name], -FORMAT_ORDER.index(name)))


def collect(paths: Sequence[Path], requested: str) -> list[FailureRecord]:
    records: list[FailureRecord] = []
    seen: set[tuple[str, str, str, str]] = set()
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="replace")
        selected = sniff(text) if requested == "auto" else requested
        for raw in EXTRACTORS[selected](text):
            key = (selected, raw.name, raw.location, raw.message)
            if key in seen:
                continue
            seen.add(key)
            records.append(
                FailureRecord(
                    identifier=f"{selected}-{len(records) + 1:03d}",
                    format=selected,
                    name=WHITESPACE.sub(" ", raw.name).strip(),
                    location=raw.location.strip(),
                    message=redact(raw.message),
                    source=str(path),
                )
            )
    return records[:MAX_RECORDS]


def record_state(records: Sequence[FailureRecord]) -> dict[str, object]:
    return {
        "failures": [
            {
                "id": record.identifier,
                "format": record.format,
                "name": record.name,
                "location": record.location,
                "message": record.message,
            }
            for record in records
        ]
    }


def triage_questions(records: Sequence[FailureRecord]) -> dict[str, Mapping[str, object]]:
    questions: dict[str, Mapping[str, object]] = {}
    for record in records:
        questions[f"{CATEGORY_QUESTION}::{record.identifier}"] = choice(
            f"The state lists failures. Classify the root cause of the failure whose id is "
            f"{record.identifier}.",
            CATEGORIES,
        )
        questions[f"{MERGE_QUESTION}::{record.identifier}"] = score(
            f"The state lists failures. Rate how strongly the failure whose id is "
            f"{record.identifier} should hold up a merge.",
            MERGE_BLOCK_LEVELS,
        )
    return questions


def link_name(pair: tuple[str, str]) -> str:
    return f"{LINK_QUESTION}::{pair[0]}::{pair[1]}"


def link_questions(pairs: Sequence[tuple[str, str]]) -> dict[str, Mapping[str, object]]:
    return {
        link_name(pair): noul(
            f"The state lists failures. The failure whose id is {pair[0]} and the failure whose "
            f"id is {pair[1]} have the same root cause."
        )
        for pair in pairs
    }


def candidate_pairs(records: Sequence[FailureRecord]) -> list[tuple[str, str]]:
    normalised = {record.identifier: normalise(record.message) for record in records}
    files = {record.identifier: location_file(record.location) for record in records}
    pairs: list[tuple[str, str]] = []
    for left, right in itertools.combinations(records, 2):
        same_file = bool(files[left.identifier]) and (
            files[left.identifier] == files[right.identifier]
        )
        same_message = bool(normalised[left.identifier]) and (
            normalised[left.identifier] == normalised[right.identifier]
        )
        if same_file or same_message:
            pairs.append((left.identifier, right.identifier))
    return pairs[:MAX_PAIRS]


@dataclass
class Union:
    parent: dict[str, str]

    def find(self, item: str) -> str:
        root = item
        while self.parent[root] != root:
            root = self.parent[root]
        while self.parent[item] != root:
            self.parent[item], item = root, self.parent[item]
        return root

    def merge(self, left: str, right: str) -> None:
        left_root = self.find(left)
        right_root = self.find(right)
        if left_root != right_root:
            self.parent[max(left_root, right_root)] = min(left_root, right_root)


def clusters(
    records: Sequence[FailureRecord],
    links: Mapping[tuple[str, str], float],
) -> list[list[str]]:
    order = [record.identifier for record in records]
    union = Union({identifier: identifier for identifier in order})
    for (left, right), probability in links.items():
        if left in union.parent and right in union.parent and probability >= CLUSTER_THRESHOLD:
            union.merge(left, right)
    grouped: dict[str, list[str]] = {}
    for identifier in order:
        grouped.setdefault(union.find(identifier), []).append(identifier)
    return [grouped[root] for root in sorted(grouped, key=order.index)]


def merge_label(answer: Answer) -> str:
    positions = range(len(MERGE_BLOCK_LABELS))
    nearest = min(positions, key=lambda index: abs(float(answer.value) - float(index)))
    return MERGE_BLOCK_LABELS[nearest]


def representative(
    index: Mapping[str, FailureRecord],
    members: Sequence[str],
) -> FailureRecord:
    return min(
        (index[identifier] for identifier in members),
        key=lambda record: (
            0 if LINE_ANCHOR.match(record.location) else 1,
            record.identifier,
        ),
    )


def cluster_detail(
    index: Mapping[str, FailureRecord],
    members: Sequence[str],
    links: Mapping[tuple[str, str], float],
    leader: FailureRecord,
) -> dict[str, object]:
    held = set(members)
    return {
        "members": list(members),
        "names": [index[identifier].name for identifier in members],
        "size": len(members),
        "format": leader.format,
        "log": leader.source,
        "location": leader.location,
        "message": leader.message,
        "links": {
            f"{left}~{right}": probability
            for (left, right), probability in sorted(links.items())
            if left in held and right in held and probability >= CLUSTER_THRESHOLD
        },
    }


def cluster_findings(
    index: Mapping[str, FailureRecord],
    members: Sequence[str],
    answers: Mapping[str, Answer],
    links: Mapping[tuple[str, str], float],
) -> list[Finding]:
    leader = representative(index, members)
    detail = cluster_detail(index, members, links, leader)
    anchor = anchor_for(leader)
    findings: list[Finding] = []
    category = answers.get(f"{CATEGORY_QUESTION}::{leader.identifier}")
    if category is not None:
        findings.append(
            Finding(
                check=CHECK,
                subject=leader.name,
                anchor=anchor,
                question=CATEGORY_QUESTION,
                answer=category.value,
                confidence=category.confidence,
                route=route_for(category.confidence),
                detail=dict(detail, probabilities=dict(category.probabilities)),
            )
        )
    blocking = answers.get(f"{MERGE_QUESTION}::{leader.identifier}")
    if blocking is not None:
        findings.append(
            Finding(
                check=CHECK,
                subject=leader.name,
                anchor=anchor,
                question=MERGE_QUESTION,
                answer=merge_label(blocking),
                confidence=blocking.confidence,
                route=route_for(blocking.confidence),
                detail=dict(
                    detail,
                    score=float(blocking.value),
                    probabilities=dict(blocking.probabilities),
                ),
            )
        )
    return findings


def account(report: Report, evaluation: Evaluation) -> None:
    report.calls += 1
    report.input_tokens += int(evaluation.usage.get("input_tokens", 0.0))
    report.cost_usd += float(evaluation.usage.get("cost", 0.0))


def evaluate_triage(
    client: JevClient,
    records: Sequence[FailureRecord],
    report: Report,
) -> dict[str, Answer]:
    answers: dict[str, Answer] = {}
    for batch in batched(records, RECORD_BATCH):
        evaluation = client.evaluate(record_state(batch), triage_questions(batch))
        account(report, evaluation)
        answers.update(evaluation.answers)
    return answers


def evaluate_links(
    client: JevClient,
    records: Sequence[FailureRecord],
    pairs: Sequence[tuple[str, str]],
    report: Report,
) -> dict[tuple[str, str], float]:
    index = {record.identifier: record for record in records}
    links: dict[tuple[str, str], float] = {}
    for batch in batched(pairs, PAIR_BATCH):
        involved = sorted({identifier for pair in batch for identifier in pair})
        state = record_state([index[identifier] for identifier in involved])
        evaluation = client.evaluate(state, link_questions(batch))
        account(report, evaluation)
        for pair in batch:
            answer = evaluation.answers.get(link_name(pair))
            if answer is not None and answer.kind == "noul":
                links[pair] = float(answer.value)
    return links


def evaluate_logs(paths: Sequence[Path], requested: str, client: JevClient) -> Report:
    records = collect(paths, requested)
    report = Report(check=CHECK, revision=git_revision(REPO_ROOT), model=client.model)
    if not records:
        return report
    answers = evaluate_triage(client, records, report)
    links = evaluate_links(client, records, candidate_pairs(records), report)
    index = {record.identifier: record for record in records}
    for members in clusters(records, links):
        report.findings.extend(cluster_findings(index, members, answers, links))
    return report


@dataclass(frozen=True)
class FailuresCheck:
    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--log-file", action="append", required=True, type=Path)
        parser.add_argument("--format", choices=FORMAT_CHOICES, default="auto")

    def run(self, args: argparse.Namespace, client: JevClient) -> Report:
        return evaluate_logs(list(args.log_file), args.format, client)


CHECK_OBJECT = register(CHECK)(FailuresCheck())
add_arguments = CHECK_OBJECT.add_arguments
run = CHECK_OBJECT.run
