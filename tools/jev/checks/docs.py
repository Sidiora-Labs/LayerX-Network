from __future__ import annotations

import argparse
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

from tools.jev.checks import register
from tools.jev.client import Evaluation, JevClient, JevConfigError, choice, noul
from tools.jev.report import Finding, Report, git_revision, route_for


CHECK_NAME = "docs"
REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_SOURCE = "README.md"
DEFAULT_TRANSLATIONS = "docs/readme/README.*.md"
PREAMBLE_TITLE = "(preamble)"
PAIRING_QUESTION = "section_pairing"
MISSING_SECTION = "missing_section"
EXTRA_SECTION = "extra_section"
SAME_CLAIMS = "same_claims"
DRIFT_KIND = "drift_kind"
FAITHFUL = "faithful"
SAME_CLAIMS_AT = 0.5
CERTAIN = 1.0

DRIFT_KINDS: Mapping[str, str] = {
    FAITHFUL: "The translation carries the same claims as the source section and nothing else.",
    "missing_content": (
        "The translation drops a claim, link, row or command that the source section states."
    ),
    "extra_content": (
        "The translation adds a claim, link, row or command that the source section does not state."
    ),
    "stale_reference": (
        "The translation names a path, command, endpoint, flag or name that the source section "
        "no longer names."
    ),
    "mistranslated": "The translation states something that contradicts the source section.",
}

QUESTIONS: Mapping[str, Mapping[str, object]] = {
    SAME_CLAIMS: noul(
        "the translated section states the same claims as the source "
        "with nothing added or dropped"
    ),
    DRIFT_KIND: choice(
        "Classify how the translated section drifts from the source section.",
        DRIFT_KINDS,
    ),
}

FENCE = re.compile(r"^\s{0,3}(?:`{3,}|~{3,})")
HEADING = re.compile(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
MARKDOWN_IMAGE = re.compile(r"!\[[^\]]*\]\([^)]*\)")
HTML_IMAGE = re.compile(r"<img\b[^>]*>")
MARKDOWN_LINK = re.compile(r"\[([^\]]*)\]\([^)]*\)")
BARE_URL = re.compile(r"<?\bhttps?://[^\s<>)\"']+>?")
HTML_TAG = re.compile(r"</?[a-zA-Z][^>]*>")
BLANKS = re.compile(r"[ \t]+")


@dataclass(frozen=True)
class Section:
    path: Path
    index: int
    level: int
    title: str
    line: int
    body: str


@dataclass(frozen=True)
class Pair:
    index: int
    source: Section
    translation: Section


def relative(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def anchor_for(section: Section) -> str:
    return f"{relative(section.path)}:{section.line}"


def resolve_path(value: str) -> Path:
    path = Path(value)
    return path if path.is_absolute() else REPO_ROOT / path


def resolve_translations(pattern: str) -> list[Path]:
    candidate = Path(pattern)
    if candidate.is_absolute():
        anchor = Path(candidate.anchor)
        return sorted(anchor.glob(candidate.relative_to(anchor).as_posix()))
    return sorted(REPO_ROOT.glob(pattern))


def _append(
    sections: list[Section],
    path: Path,
    level: int,
    title: str,
    line: int,
    lines: Sequence[str],
) -> None:
    body = "\n".join(lines).strip("\n")
    if level == 0 and not body.strip():
        return
    sections.append(
        Section(path=path, index=len(sections), level=level, title=title, line=line, body=body)
    )


def split_sections(path: Path, text: str | None = None) -> list[Section]:
    content = path.read_text(encoding="utf-8") if text is None else text
    sections: list[Section] = []
    level = 0
    title = PREAMBLE_TITLE
    line = 1
    body: list[str] = []
    fenced = False
    for number, raw in enumerate(content.splitlines(), 1):
        if FENCE.match(raw):
            fenced = not fenced
            body.append(raw)
            continue
        heading = None if fenced else HEADING.match(raw)
        if heading is None:
            body.append(raw)
            continue
        _append(sections, path, level, title, line, body)
        level = len(heading.group(1))
        title = heading.group(2).strip()
        line = number
        body = []
    _append(sections, path, level, title, line, body)
    return sections


def pair_sections(
    source: Sequence[Section],
    translation: Sequence[Section],
) -> tuple[list[Pair], list[Section], list[Section]]:
    shared = min(len(source), len(translation))
    pairs = [
        Pair(index=index, source=source[index], translation=translation[index])
        for index in range(shared)
    ]
    return pairs, list(source[shared:]), list(translation[shared:])


def sanitize(body: str) -> str:
    kept: list[str] = []
    fenced = False
    for raw in body.splitlines():
        if FENCE.match(raw):
            fenced = not fenced
            continue
        if fenced:
            continue
        cleaned = MARKDOWN_IMAGE.sub(" ", raw)
        cleaned = HTML_IMAGE.sub(" ", cleaned)
        cleaned = MARKDOWN_LINK.sub(r"\1", cleaned)
        cleaned = BARE_URL.sub(" ", cleaned)
        cleaned = HTML_TAG.sub(" ", cleaned)
        cleaned = BLANKS.sub(" ", cleaned).strip()
        if not cleaned:
            if kept and kept[-1]:
                kept.append("")
            continue
        kept.append(cleaned)
    while kept and not kept[-1]:
        kept.pop()
    return "\n".join(kept)


def unpaired_findings(
    unpaired_source: Sequence[Section],
    unpaired_translation: Sequence[Section],
    translation_path: Path,
) -> list[Finding]:
    target = relative(translation_path)
    findings: list[Finding] = []
    for section in unpaired_source:
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=f"{target} {section.title}",
                anchor=anchor_for(section),
                question=PAIRING_QUESTION,
                answer=MISSING_SECTION,
                confidence=CERTAIN,
                route=route_for(CERTAIN),
                detail={
                    "index": section.index,
                    "level": section.level,
                    "source_title": section.title,
                    "source_anchor": anchor_for(section),
                    "translation": target,
                },
            )
        )
    for section in unpaired_translation:
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=f"{target} {section.title}",
                anchor=anchor_for(section),
                question=PAIRING_QUESTION,
                answer=EXTRA_SECTION,
                confidence=CERTAIN,
                route=route_for(CERTAIN),
                detail={
                    "index": section.index,
                    "level": section.level,
                    "translation_title": section.title,
                    "translation_anchor": anchor_for(section),
                    "translation": target,
                },
            )
        )
    return findings


def pair_findings(pair: Pair, evaluation: Evaluation) -> list[Finding]:
    same = evaluation.answers.get(SAME_CLAIMS)
    drift = evaluation.answers.get(DRIFT_KIND)
    subject = f"{relative(pair.translation.path)} {pair.translation.title}"
    anchor = anchor_for(pair.translation)
    shared = {
        "index": pair.index,
        "source_title": pair.source.title,
        "translation_title": pair.translation.title,
        "source_anchor": anchor_for(pair.source),
        "translation_anchor": anchor,
    }
    findings: list[Finding] = []
    if same is not None and float(same.value) < SAME_CLAIMS_AT:
        detail = dict(shared)
        detail["probability"] = float(same.value)
        if drift is not None:
            detail[DRIFT_KIND] = drift.value
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=subject,
                anchor=anchor,
                question=SAME_CLAIMS,
                answer=float(same.value),
                confidence=same.confidence,
                route=route_for(same.confidence),
                detail=detail,
            )
        )
    if drift is not None and drift.value != FAITHFUL:
        detail = dict(shared)
        detail["probabilities"] = dict(drift.probabilities)
        if same is not None:
            detail[SAME_CLAIMS] = float(same.value)
        findings.append(
            Finding(
                check=CHECK_NAME,
                subject=subject,
                anchor=anchor,
                question=DRIFT_KIND,
                answer=drift.value,
                confidence=drift.confidence,
                route=route_for(drift.confidence),
                detail=detail,
            )
        )
    return findings


class DocsCheck:
    def add_arguments(self, parser: argparse.ArgumentParser) -> None:
        parser.add_argument("--source", default=DEFAULT_SOURCE)
        parser.add_argument("--translations", default=DEFAULT_TRANSLATIONS)

    def run(self, args: argparse.Namespace, client: JevClient) -> Report:
        source_path = resolve_path(args.source)
        if not source_path.is_file():
            raise JevConfigError(f"source readme {source_path} does not exist")
        source_sections = split_sections(source_path)
        report = Report(check=CHECK_NAME, revision=git_revision(REPO_ROOT), model=client.model)
        for translation_path in resolve_translations(args.translations):
            if translation_path.resolve() == source_path.resolve():
                continue
            translation_sections = split_sections(translation_path)
            pairs, unpaired_source, unpaired_translation = pair_sections(
                source_sections, translation_sections
            )
            report.findings.extend(
                unpaired_findings(unpaired_source, unpaired_translation, translation_path)
            )
            for pair in pairs:
                state = {
                    "section": pair.source.title,
                    "source": sanitize(pair.source.body),
                    "translation": sanitize(pair.translation.body),
                }
                if not state["source"] and not state["translation"]:
                    continue
                evaluation = client.evaluate(state, QUESTIONS)
                report.calls += 1
                report.input_tokens += int(evaluation.usage.get("input_tokens", 0.0))
                report.cost_usd += float(evaluation.usage.get("cost", 0.0))
                report.model = evaluation.model
                report.findings.extend(pair_findings(pair, evaluation))
        return report


CHECK = register(CHECK_NAME)(DocsCheck())
add_arguments = CHECK.add_arguments
run = CHECK.run
