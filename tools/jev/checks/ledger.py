from __future__ import annotations

import argparse
import random
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Mapping, Sequence

from tools.jev.checks import register
from tools.jev.client import Evaluation, JevClient, choice, noul
from tools.jev.report import Finding, Report, git_revision, route_for


CHECK_NAME = "ledger"
ROOT = Path(__file__).resolve().parents[3]
LEDGER_GLOB = "spec/*/qualification.kvx"
OBSERVATION_PREFIX = "observation."
RECORD_FIELDS = ("task", "file", "symbol", "observed", "assumption", "resolution", "severity")
KEY_CHARACTERS = "_-."
SEVERITY_QUESTION = "severity"
CLOSURE_QUESTION = "resolution_closes"
PAIR_QUESTION = "same_root_cause"
SEVERITY_INSTRUCTIONS = (
    "This is one observation record from a LayerX qualification ledger. "
    "Which severity does the observed text, read together with the stated assumption "
    "and any resolution, actually carry?"
)
SEVERITY_RUBRIC: Mapping[str, str] = {
    "blocker": (
        "A functional gate cannot pass or a required surface is unproven: a declared command "
        "failed, never ran, or no executed evidence covers the deliverable, and the work it "
        "names cannot be qualified until someone acts on it."
    ),
    "suspect": (
        "A likely defect that is not yet confirmed: the text describes behaviour that reads "
        "as wrong, contradictory or unsafe, but no run has demonstrated the failure."
    ),
    "assumption": (
        "A stated premise standing in for missing authority: the text records a reading, a "
        "decision or a default adopted because an owner answer or an external interface was "
        "unavailable, and it holds only while that premise holds."
    ),
    "note": (
        "Informational, no action: the text records context, a bound, a measurement or a "
        "repair that already landed, and nothing downstream waits on it."
    ),
}
CLOSURE_INSTRUCTIONS = "The resolution text fully closes the issue described in observed."
PAIR_INSTRUCTIONS = "These two observations describe the same root cause."
CLOSURE_THRESHOLD = 0.5
PAIR_THRESHOLD = 0.8
SHINGLE_WORDS = 4
SHARED_SHINGLES = 6
DEFAULT_MAX_PAIRS = 300
INPUT_TOKENS_KEY = "input_tokens"
COST_KEY = "cost"


@dataclass(frozen=True)
class Record:
    identifier: str
    path: str
    line: int
    task: str
    file: str
    symbol: str
    observed: str
    assumption: str
    resolution: str
    severity: str

    def anchor(self) -> str:
        return f"{self.path}:{self.line}"

    def state(self) -> dict[str, str]:
        payload = {"observed": self.observed, "assumption": self.assumption}
        if self.resolution:
            payload["resolution"] = self.resolution
        return payload


def strip_comment(line: str) -> str:
    out: list[str] = []
    quoted = False
    index = 0
    while index < len(line):
        character = line[index]
        if quoted and character == "\\":
            out.append(character)
            if index + 1 < len(line):
                out.append(line[index + 1])
            index += 2
            continue
        if character == '"':
            quoted = not quoted
        elif character == "#" and not quoted:
            break
        out.append(character)
        index += 1
    return "".join(out)


def unquote(value: str) -> str:
    text = value.strip()
    if len(text) < 2 or not text.startswith('"') or not text.endswith('"'):
        return text
    body = text[1:-1]
    out: list[str] = []
    index = 0
    while index < len(body):
        character = body[index]
        if character == "\\" and index + 1 < len(body) and body[index + 1] in '"\\':
            out.append(body[index + 1])
            index += 2
            continue
        out.append(character)
        index += 1
    return "".join(out)


def is_key(text: str) -> bool:
    return bool(text) and all(
        character.isalnum() or character in KEY_CHARACTERS for character in text
    )


def parse_ledger(text: str, path: str) -> list[Record]:
    blocks: list[tuple[str, int, dict[str, str]]] = []
    for number, raw in enumerate(text.splitlines(), start=1):
        line = strip_comment(raw.rstrip("\r")).strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            blocks.append((line[1:-1].strip(), number, {}))
            continue
        if not blocks or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        if is_key(key):
            blocks[-1][2][key] = unquote(value)
    records: list[Record] = []
    for name, number, fields in blocks:
        if not name.startswith(OBSERVATION_PREFIX):
            continue
        values = {field: fields.get(field, "") for field in RECORD_FIELDS}
        records.append(
            Record(
                identifier=name[len(OBSERVATION_PREFIX) :],
                path=path,
                line=number,
                **values,
            )
        )
    return records


def display_path(path: Path) -> str:
    resolved = path.resolve()
    try:
        return str(resolved.relative_to(ROOT))
    except ValueError:
        return str(resolved)


def load_records(paths: Sequence[Path]) -> list[Record]:
    records: list[Record] = []
    for path in paths:
        records.extend(parse_ledger(path.read_text(encoding="utf-8"), display_path(path)))
    return records


def judgeable(records: Sequence[Record]) -> list[Record]:
    return [record for record in records if record.observed]


def select(records: Sequence[Record], sample: int, seed: int) -> list[Record]:
    if sample <= 0 or sample >= len(records):
        return list(records)
    indices = sorted(random.Random(seed).sample(range(len(records)), sample))
    return [records[index] for index in indices]


def words(text: str) -> list[str]:
    out: list[str] = []
    current: list[str] = []
    for character in text.lower():
        if character.isalnum():
            current.append(character)
            continue
        if current:
            out.append("".join(current))
            current = []
    if current:
        out.append("".join(current))
    return out


def shingles(text: str, size: int = SHINGLE_WORDS) -> frozenset[tuple[str, ...]]:
    tokens = words(text)
    return frozenset(
        tuple(tokens[index : index + size]) for index in range(len(tokens) - size + 1)
    )


def candidate_pairs(
    records: Sequence[Record],
    max_pairs: int = DEFAULT_MAX_PAIRS,
) -> list[tuple[int, int]]:
    if max_pairs <= 0:
        return []
    grams = [shingles(record.observed) for record in records]
    pairs: list[tuple[int, int]] = []
    for first in range(len(records)):
        for second in range(first + 1, len(records)):
            left = records[first]
            right = records[second]
            same_file = bool(left.file) and left.file == right.file
            same_symbol = bool(left.symbol) and left.symbol == right.symbol
            shared = len(grams[first] & grams[second])
            if not (same_file or same_symbol or shared >= SHARED_SHINGLES):
                continue
            pairs.append((first, second))
            if len(pairs) >= max_pairs:
                return pairs
    return pairs


def cluster(count: int, edges: Iterable[tuple[int, int]]) -> list[list[int]]:
    parent = list(range(count))

    def find(item: int) -> int:
        while parent[item] != item:
            parent[item] = parent[parent[item]]
            item = parent[item]
        return item

    for first, second in edges:
        left = find(first)
        right = find(second)
        if left != right:
            parent[max(left, right)] = min(left, right)
    groups: dict[int, list[int]] = {}
    for item in range(count):
        groups.setdefault(find(item), []).append(item)
    return [members for _, members in sorted(groups.items()) if len(members) > 1]


def severity_questions(record: Record) -> dict[str, dict[str, object]]:
    questions = {SEVERITY_QUESTION: choice(SEVERITY_INSTRUCTIONS, SEVERITY_RUBRIC)}
    if record.resolution:
        questions[CLOSURE_QUESTION] = noul(CLOSURE_INSTRUCTIONS)
    return questions


def pair_state(left: Record, right: Record) -> dict[str, object]:
    return {"first": left.state(), "second": right.state()}


def pair_questions() -> dict[str, dict[str, object]]:
    return {PAIR_QUESTION: noul(PAIR_INSTRUCTIONS)}


def severity_findings(record: Record, evaluation: Evaluation) -> list[Finding]:
    findings: list[Finding] = []
    proposed = evaluation.answers.get(SEVERITY_QUESTION)
    if proposed is not None and proposed.kind == "choice" and proposed.value != record.severity:
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=record.identifier,
                anchor=record.anchor(),
                question=SEVERITY_QUESTION,
                answer=proposed.value,
                confidence=proposed.confidence,
                route=route_for(proposed.confidence),
                detail={
                    "recorded": record.severity,
                    "proposed": proposed.value,
                    "probabilities": dict(proposed.probabilities),
                    "task": record.task,
                    "file": record.file,
                    "symbol": record.symbol,
                },
            )
        )
    closure = evaluation.answers.get(CLOSURE_QUESTION)
    if closure is not None and closure.kind == "noul" and closure.value < CLOSURE_THRESHOLD:
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=record.identifier,
                anchor=record.anchor(),
                question=CLOSURE_QUESTION,
                answer=closure.value,
                confidence=closure.confidence,
                route=route_for(closure.confidence),
                detail={
                    "recorded": record.severity,
                    "closes": closure.value,
                    "threshold": CLOSURE_THRESHOLD,
                    "task": record.task,
                    "file": record.file,
                },
            )
        )
    return findings


def duplicate_findings(
    records: Sequence[Record],
    probabilities: Mapping[tuple[int, int], float],
) -> list[Finding]:
    edges = [pair for pair, value in probabilities.items() if value >= PAIR_THRESHOLD]
    findings: list[Finding] = []
    for members in cluster(len(records), edges):
        inside = set(members)
        linked = {
            pair: value
            for pair, value in sorted(probabilities.items())
            if value >= PAIR_THRESHOLD and pair[0] in inside and pair[1] in inside
        }
        confidence = min(linked.values()) if linked else PAIR_THRESHOLD
        identifiers = [records[index].identifier for index in members]
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=", ".join(identifiers),
                anchor=records[members[0]].anchor(),
                question=PAIR_QUESTION,
                answer=float(len(members)),
                confidence=confidence,
                route=route_for(confidence),
                detail={
                    "members": identifiers,
                    "anchors": [records[index].anchor() for index in members],
                    "severities": [records[index].severity for index in members],
                    "tasks": [records[index].task for index in members],
                    "pairs": {
                        f"{records[first].identifier}|{records[second].identifier}": value
                        for (first, second), value in linked.items()
                    },
                },
            )
        )
    return findings


def tally(report: Report, evaluation: Evaluation) -> None:
    report.calls += 1
    report.input_tokens += int(evaluation.usage.get(INPUT_TOKENS_KEY, 0.0))
    report.cost_usd += evaluation.usage.get(COST_KEY, 0.0)


def ledger_paths(args: argparse.Namespace) -> list[Path]:
    if args.kvx:
        return list(args.kvx)
    return sorted(ROOT.glob(LEDGER_GLOB))


def add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--kvx", action="append", default=[], type=Path)
    parser.add_argument("--sample", type=int, default=0)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--severity-only", action="store_true")
    parser.add_argument("--dedup-only", action="store_true")
    parser.add_argument("--max-pairs", type=int, default=DEFAULT_MAX_PAIRS)


def run(args: argparse.Namespace, client: JevClient) -> Report:
    records = select(judgeable(load_records(ledger_paths(args))), args.sample, args.seed)
    report = Report(check=CHECK_NAME, revision=git_revision(ROOT), model=client.model)
    if not args.dedup_only:
        for record in records:
            evaluation = client.evaluate(record.state(), severity_questions(record))
            tally(report, evaluation)
            report.findings.extend(severity_findings(record, evaluation))
    if not args.severity_only:
        probabilities: dict[tuple[int, int], float] = {}
        for first, second in candidate_pairs(records, args.max_pairs):
            evaluation = client.evaluate(
                pair_state(records[first], records[second]), pair_questions()
            )
            tally(report, evaluation)
            answer = evaluation.answers.get(PAIR_QUESTION)
            if answer is not None and answer.kind == "noul":
                probabilities[(first, second)] = float(answer.value)
        report.findings.extend(duplicate_findings(records, probabilities))
    return report


class LedgerCheck:
    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        add_arguments(parser)

    def run(self, args: argparse.Namespace, client: JevClient) -> Report:
        return run(args, client)


CHECK = register(CHECK_NAME)(LedgerCheck())
