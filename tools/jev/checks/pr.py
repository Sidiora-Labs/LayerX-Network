from __future__ import annotations

import argparse
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

from tools.jev.checks import register
from tools.jev.client import Answer, Evaluation, JevClient, JevConfigError, choice, noul
from tools.jev.report import Finding, Report, Route, git_revision, route_for


CHECK_NAME = "pr"
REPO_ROOT = Path(__file__).resolve().parents[3]
TEMPLATE_PATH = Path(".github") / "pull_request_template.md"
COMMENT_TITLE = "Jev advisory (non-blocking)"
COMMENT_FOOTER = (
    "Probabilistic advisory output. It blocks nothing, gates nothing and certifies nothing."
)
TEST_MARKERS = ("tests/", "test_", "_test", ".t.sol", "e2e")
DOC_SUFFIXES = (".md", ".rst", ".txt", ".adoc")
DOC_PREFIXES = ("docs/", "doc/")
WORKFLOW_PREFIX = ".github/workflows/"
MAKEFILE_NAMES = ("Makefile", "makefile", "GNUmakefile")
MAKEFILE_SUFFIX = ".mk"
FILE_LIST_LIMIT = 200
BODY_LIMIT = 8000
MESSAGE_LIMIT = 4000
MIN_ITEM_OVERLAP = 16
COST_PER_INPUT_TOKEN = 0.042 / 1_000_000
SUMMARY_QUESTION = "summary_consistent"
VERIFICATION_QUESTION = "verification_level"
CHECKLIST_PREFIX = "checklist_"
COMMIT_CLAIM_QUESTION = "message_overclaims"
COMMIT_HONESTY_QUESTION = "message_honesty"
PRODUCTION_LEVEL = "production_certification"
SUMMARY_STATEMENT = "The summary describes changes consistent with the changed file list."
CLAIM_STATEMENT = "The commit message claims work that the commit stat does not evidence."
VERIFICATION_INSTRUCTIONS = "Which verification level does this pull request body claim?"
HONESTY_INSTRUCTIONS = "How does the commit message compare with the files the commit changed?"
SUMMARY_CRITERIA = {
    "true": "Every claim in the summary could plausibly come from these files and line counts.",
    "false": "The summary describes work that these files and line counts do not contain.",
}
CLAIM_CRITERIA = {
    "true": "The message names work that the changed files and line counts do not show.",
    "false": "The message stays within what the changed files and line counts show.",
}
VERIFICATION_LEVELS = {
    "none": "The body reports no verification, or lists no command and no outcome.",
    "local_tests": "The body claims builds, tests or checks that were run locally.",
    "ci_gates": "The body claims continuous integration gates or required checks passed.",
    "production_certification": (
        "The body claims production certification, deployment authorization or release sign-off."
    ),
}
HONESTY_LEVELS = {
    "consistent": "The message matches the changed files and line counts.",
    "overclaims": "The message claims more or broader work than the changed files evidence.",
    "underclaims": "The message describes less work than the changed files show.",
    "unclear": "The message is too vague to compare with the changed files.",
}
ACCEPTED_LEVELS = frozenset(("none", "local_tests", "ci_gates"))
ACCEPTED_HONESTY = frozenset(("consistent",))
CHECKLIST_LINE = re.compile(r"^\s*[-*+]\s*\[([ xX])\]\s*(.+?)\s*$")
HEADING_LINE = re.compile(r"^\s*#{1,6}\s+(.*?)\s*$")
STAT_FILES = re.compile(r"(\d+) files? changed")
STAT_INSERTIONS = re.compile(r"(\d+) insertions?\(\+\)")
STAT_DELETIONS = re.compile(r"(\d+) deletions?\(-\)")
ITEM_NOISE = re.compile(r"[*`_]+")
WHITESPACE = re.compile(r"\s+")
FIELD = "\x1f"
RECORD = "\x1e"


@dataclass(frozen=True)
class FileChange:
    status: str
    path: str


@dataclass(frozen=True)
class StatSummary:
    files: int
    insertions: int
    deletions: int
    text: str


@dataclass(frozen=True)
class ChecklistItem:
    index: int
    line: int
    text: str
    template_text: str


@dataclass(frozen=True)
class DiffSummary:
    base: str
    head: str
    changes: tuple[FileChange, ...]
    stat: StatSummary


@dataclass(frozen=True)
class CommitSummary:
    sha: str
    subject: str
    message: str
    changes: tuple[FileChange, ...]
    stat: StatSummary


def _git(repo_root: Path, arguments: Sequence[str]) -> str:
    completed = subprocess.run(
        ("git", *arguments),
        cwd=repo_root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        raise JevConfigError(
            f"git {' '.join(arguments)} failed ({completed.returncode}): "
            f"{completed.stderr.strip()}"
        )
    return completed.stdout


def parse_name_status(text: str) -> tuple[FileChange, ...]:
    changes: list[FileChange] = []
    for line in text.splitlines():
        if not line.strip():
            continue
        fields = line.split("\t")
        if len(fields) < 2:
            continue
        status = fields[0].strip()
        path = fields[-1].strip()
        if not status or not path:
            continue
        changes.append(FileChange(status=status, path=path))
    return tuple(changes)


def parse_stat_summary(text: str) -> StatSummary:
    lines = [line for line in text.splitlines() if line.strip()]
    if not lines:
        return StatSummary(files=0, insertions=0, deletions=0, text="")
    last = lines[-1].strip()
    files = STAT_FILES.search(last)
    insertions = STAT_INSERTIONS.search(last)
    deletions = STAT_DELETIONS.search(last)
    if files is None:
        return StatSummary(files=0, insertions=0, deletions=0, text="")
    return StatSummary(
        files=int(files.group(1)),
        insertions=int(insertions.group(1)) if insertions else 0,
        deletions=int(deletions.group(1)) if deletions else 0,
        text=last,
    )


def collect_diff(repo_root: Path, base: str, head: str) -> DiffSummary:
    span = f"{base}..{head}"
    changes = parse_name_status(_git(repo_root, ("diff", "--name-status", span)))
    stat = parse_stat_summary(_git(repo_root, ("diff", "--stat", span)))
    return DiffSummary(base=base, head=head, changes=changes, stat=stat)


def collect_commits(repo_root: Path, base: str, head: str) -> tuple[CommitSummary, ...]:
    span = f"{base}..{head}"
    raw = _git(
        repo_root,
        ("log", "--no-merges", f"--format=%H{FIELD}%s{FIELD}%B{RECORD}", span),
    )
    commits: list[CommitSummary] = []
    for record in raw.split(RECORD):
        entry = record.strip("\n")
        if not entry.strip():
            continue
        fields = entry.split(FIELD)
        if len(fields) < 3:
            continue
        sha = fields[0].strip()
        changes = parse_name_status(_git(repo_root, ("show", "--name-status", "--format=", sha)))
        stat = parse_stat_summary(_git(repo_root, ("show", "--stat", "--format=", sha)))
        commits.append(
            CommitSummary(
                sha=sha,
                subject=fields[1].strip(),
                message=fields[2].strip(),
                changes=changes,
                stat=stat,
            )
        )
    commits.reverse()
    return tuple(commits)


def touches_tests(paths: Sequence[str]) -> bool:
    return any(marker in path for path in paths for marker in TEST_MARKERS)


def touches_workflows(paths: Sequence[str]) -> bool:
    return any(path.startswith(WORKFLOW_PREFIX) for path in paths)


def touches_makefile(paths: Sequence[str]) -> bool:
    for path in paths:
        name = path.rsplit("/", 1)[-1]
        if name in MAKEFILE_NAMES or name.endswith(MAKEFILE_SUFFIX):
            return True
    return False


def is_docs_only(paths: Sequence[str]) -> bool:
    if not paths:
        return False
    for path in paths:
        if path.startswith(DOC_PREFIXES):
            continue
        if path.endswith(DOC_SUFFIXES):
            continue
        return False
    return True


def status_counts(changes: Sequence[FileChange]) -> dict[str, int]:
    counts = {"added": 0, "modified": 0, "deleted": 0, "renamed": 0, "other": 0}
    for change in changes:
        letter = change.status[:1].upper()
        if letter == "A":
            counts["added"] += 1
        elif letter == "M":
            counts["modified"] += 1
        elif letter == "D":
            counts["deleted"] += 1
        elif letter in ("R", "C"):
            counts["renamed"] += 1
        else:
            counts["other"] += 1
    return counts


def changes_state(changes: Sequence[FileChange], stat: StatSummary) -> dict[str, object]:
    listed = list(changes[:FILE_LIST_LIMIT])
    paths = [change.path for change in changes]
    return {
        "changed_files": [f"{change.status} {change.path}" for change in listed],
        "changed_file_count": len(changes),
        "listed_file_count": len(listed),
        "counts": status_counts(changes),
        "lines": {"insertions": stat.insertions, "deletions": stat.deletions},
        "stat": stat.text,
        "signals": {
            "tests_touched": touches_tests(paths),
            "workflows_touched": touches_workflows(paths),
            "makefile_touched": touches_makefile(paths),
            "docs_only": is_docs_only(paths),
        },
    }


def normalize_item(text: str) -> str:
    stripped = ITEM_NOISE.sub("", text)
    collapsed = WHITESPACE.sub(" ", stripped).strip().lower()
    return collapsed.rstrip(".")


def same_item(left: str, right: str) -> bool:
    first = normalize_item(left)
    second = normalize_item(right)
    if not first or not second:
        return False
    if first == second:
        return True
    if len(second) >= MIN_ITEM_OVERLAP and second in first:
        return True
    return len(first) >= MIN_ITEM_OVERLAP and first in second


def parse_checklist(text: str) -> tuple[tuple[int, bool, str], ...]:
    entries: list[tuple[int, bool, str]] = []
    for number, line in enumerate(text.splitlines(), start=1):
        match = CHECKLIST_LINE.match(line)
        if match is None:
            continue
        entries.append((number, match.group(1).lower() == "x", match.group(2)))
    return tuple(entries)


def template_items(repo_root: Path) -> tuple[str, ...]:
    path = repo_root / TEMPLATE_PATH
    if not path.is_file():
        return ()
    text = path.read_text(encoding="utf-8")
    return tuple(item for _, _, item in parse_checklist(text))


def ticked_items(template: Sequence[str], body: str) -> tuple[ChecklistItem, ...]:
    entries = [entry for entry in parse_checklist(body) if entry[1]]
    items: list[ChecklistItem] = []
    for index, candidate in enumerate(template, start=1):
        for line, _, text in entries:
            if same_item(candidate, text):
                items.append(
                    ChecklistItem(index=index, line=line, text=text, template_text=candidate)
                )
                break
    return tuple(items)


def section(body: str, title: str) -> tuple[int, str]:
    lines = body.splitlines()
    start = 0
    for number, line in enumerate(lines, start=1):
        match = HEADING_LINE.match(line)
        if match is not None and normalize_item(match.group(1)) == normalize_item(title):
            start = number
            break
    if start == 0:
        return 0, ""
    collected: list[str] = []
    for line in lines[start:]:
        if HEADING_LINE.match(line) is not None:
            break
        collected.append(line)
    return start, "\n".join(collected).strip()


def body_state(
    diff: DiffSummary,
    body: str,
    items: Sequence[ChecklistItem],
) -> dict[str, object]:
    state = changes_state(diff.changes, diff.stat)
    state["range"] = f"{diff.base}..{diff.head}"
    _, summary = section(body, "Summary")
    state["summary"] = summary or body[:BODY_LIMIT].strip()
    state["body"] = body[:BODY_LIMIT]
    state["body_truncated"] = len(body) > BODY_LIMIT
    state["ticked_checklist"] = {
        f"{CHECKLIST_PREFIX}{item.index}": item.text for item in items
    }
    return state


def commit_state(commit: CommitSummary) -> dict[str, object]:
    state = changes_state(commit.changes, commit.stat)
    state["commit"] = commit.sha
    state["subject"] = commit.subject
    state["message"] = commit.message[:MESSAGE_LIMIT]
    state["message_truncated"] = len(commit.message) > MESSAGE_LIMIT
    return state


def checklist_statement(text: str) -> str:
    return f'The diff evidence plausibly supports this ticked checklist item: "{text}"'


def body_questions(items: Sequence[ChecklistItem]) -> dict[str, dict[str, object]]:
    questions: dict[str, dict[str, object]] = {
        SUMMARY_QUESTION: noul(SUMMARY_STATEMENT, SUMMARY_CRITERIA),
        VERIFICATION_QUESTION: choice(VERIFICATION_INSTRUCTIONS, VERIFICATION_LEVELS),
    }
    for item in items:
        questions[f"{CHECKLIST_PREFIX}{item.index}"] = noul(checklist_statement(item.text))
    return questions


def commit_questions() -> dict[str, dict[str, object]]:
    return {
        COMMIT_CLAIM_QUESTION: noul(CLAIM_STATEMENT, CLAIM_CRITERIA),
        COMMIT_HONESTY_QUESTION: choice(HONESTY_INSTRUCTIONS, HONESTY_LEVELS),
    }


def noul_confidence(answer: Answer, *, expect_true: bool) -> float:
    probability = float(answer.value)
    return probability if expect_true else 1.0 - probability


def choice_confidence(answer: Answer, accepted: frozenset[str]) -> float:
    total = sum(answer.probabilities.values())
    if total > 0.0:
        mass = sum(value for key, value in answer.probabilities.items() if key in accepted)
        return mass / total
    if isinstance(answer.value, str) and answer.value in accepted:
        return answer.confidence
    return 1.0 - answer.confidence


def _detail(answer: Answer, extra: Mapping[str, object]) -> dict[str, object]:
    detail: dict[str, object] = {"probabilities": dict(answer.probabilities)}
    detail["model_confidence"] = answer.confidence
    detail.update(extra)
    return detail


def body_findings(
    evaluation: Evaluation,
    body_path: Path,
    body: str,
    items: Sequence[ChecklistItem],
) -> list[Finding]:
    findings: list[Finding] = []
    summary_line, _ = section(body, "Summary")
    evidence_line, _ = section(body, "Test evidence")
    answer = evaluation.answers.get(SUMMARY_QUESTION)
    if answer is not None:
        confidence = noul_confidence(answer, expect_true=True)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject="summary",
                anchor=f"{body_path}:{summary_line or 1}",
                question=SUMMARY_STATEMENT,
                answer=answer.value,
                confidence=confidence,
                route=route_for(confidence),
                detail=_detail(answer, {"summary_section": summary_line > 0}),
            )
        )
    answer = evaluation.answers.get(VERIFICATION_QUESTION)
    if answer is not None:
        confidence = choice_confidence(answer, ACCEPTED_LEVELS)
        flagged = answer.value == PRODUCTION_LEVEL
        route: Route = "escalate" if flagged else route_for(confidence)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject="verification",
                anchor=f"{body_path}:{evidence_line or 1}",
                question=VERIFICATION_INSTRUCTIONS,
                answer=answer.value,
                confidence=confidence,
                route=route,
                detail=_detail(answer, {"flagged": flagged}),
            )
        )
    for item in items:
        answer = evaluation.answers.get(f"{CHECKLIST_PREFIX}{item.index}")
        if answer is None:
            continue
        confidence = noul_confidence(answer, expect_true=True)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=f"checklist:{item.index}",
                anchor=f"{body_path}:{item.line}",
                question=checklist_statement(item.text),
                answer=answer.value,
                confidence=confidence,
                route=route_for(confidence),
                detail=_detail(answer, {"template_item": item.template_text}),
            )
        )
    return findings


def commit_findings(evaluation: Evaluation, commit: CommitSummary) -> list[Finding]:
    findings: list[Finding] = []
    answer = evaluation.answers.get(COMMIT_CLAIM_QUESTION)
    if answer is not None:
        confidence = noul_confidence(answer, expect_true=False)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=commit.sha[:12],
                anchor=commit.sha,
                question=CLAIM_STATEMENT,
                answer=answer.value,
                confidence=confidence,
                route=route_for(confidence),
                detail=_detail(answer, {"subject": commit.subject}),
            )
        )
    answer = evaluation.answers.get(COMMIT_HONESTY_QUESTION)
    if answer is not None:
        confidence = choice_confidence(answer, ACCEPTED_HONESTY)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=commit.sha[:12],
                anchor=commit.sha,
                question=HONESTY_INSTRUCTIONS,
                answer=answer.value,
                confidence=confidence,
                route=route_for(confidence),
                detail=_detail(answer, {"subject": commit.subject}),
            )
        )
    return findings


def _answer_text(value: float | str) -> str:
    if isinstance(value, str):
        return value
    return f"{value:.2f}"


def render_comment(report: Report) -> str:
    surfaced = [finding for finding in report.ranked() if finding.route in ("review", "escalate")]
    lines = [
        f"## {COMMENT_TITLE}",
        "",
        f"- check: {report.check}",
        f"- model: {report.model}",
        f"- revision: {report.revision}",
        f"- surfaced: {len(surfaced)} of {len(report.findings)} findings",
        "",
    ]
    if not surfaced:
        lines.append("_no review or escalate findings_")
    for finding in surfaced:
        question = WHITESPACE.sub(" ", finding.question).strip()
        lines.append(
            f"- **{finding.route}** `{finding.anchor}` {finding.subject}: {question} "
            f"answer `{_answer_text(finding.answer)}` confidence {finding.confidence:.2f}"
        )
    lines.append("")
    lines.append(COMMENT_FOOTER)
    lines.append("")
    return "\n".join(lines)


def write_comment(path: Path, report: Report) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_comment(report), encoding="utf-8")
    return path


def _account(report: Report, evaluation: Evaluation) -> None:
    if report.calls == 0 and evaluation.model:
        report.model = evaluation.model
    report.calls += 1
    input_tokens = int(evaluation.usage.get("input_tokens", 0.0))
    report.input_tokens += input_tokens
    report.cost_usd += float(
        evaluation.usage.get("cost", input_tokens * COST_PER_INPUT_TOKEN)
    )


def add_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--body", type=Path)
    parser.add_argument("--commits", action="store_true")
    parser.add_argument("--comment", type=Path)


def run(args: argparse.Namespace, client: JevClient) -> Report:
    diff = collect_diff(REPO_ROOT, args.base, args.head)
    report = Report(check=CHECK_NAME, revision=git_revision(REPO_ROOT), model=client.model)
    if args.body is not None:
        body_path = Path(args.body)
        if not body_path.is_file():
            raise JevConfigError(f"pull request body {body_path} does not exist")
        body = body_path.read_text(encoding="utf-8")
        items = ticked_items(template_items(REPO_ROOT), body)
        evaluation = client.evaluate(body_state(diff, body, items), body_questions(items))
        _account(report, evaluation)
        report.findings.extend(body_findings(evaluation, body_path, body, items))
    if args.commits:
        for commit in collect_commits(REPO_ROOT, args.base, args.head):
            evaluation = client.evaluate(commit_state(commit), commit_questions())
            _account(report, evaluation)
            report.findings.extend(commit_findings(evaluation, commit))
    if args.comment is not None:
        write_comment(Path(args.comment), report)
    return report


class PrCheck:
    add_arguments = staticmethod(add_arguments)
    run = staticmethod(run)


CHECK = register(CHECK_NAME)(PrCheck())
