from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal, Mapping


Route = Literal["auto", "review", "escalate"]
ROUTE_ORDER: Mapping[str, int] = {"escalate": 0, "review": 1, "auto": 2}
COLUMNS = ("Route", "Confidence", "Subject", "Anchor", "Question", "Answer", "Detail")


def route_for(confidence: float, *, auto_at: float = 0.9, review_at: float = 0.5) -> Route:
    if confidence >= auto_at:
        return "auto"
    if confidence >= review_at:
        return "review"
    return "escalate"


@dataclass(frozen=True)
class Finding:
    check: str
    subject: str
    anchor: str
    question: str
    answer: float | str
    confidence: float
    route: Route
    detail: Mapping[str, object]


@dataclass
class Report:
    check: str
    revision: str
    model: str
    findings: list[Finding] = field(default_factory=list)
    calls: int = 0
    input_tokens: int = 0
    cost_usd: float = 0.0

    def ranked(self) -> list[Finding]:
        return sorted(
            self.findings,
            key=lambda finding: (
                ROUTE_ORDER.get(finding.route, len(ROUTE_ORDER)),
                -finding.confidence,
                finding.subject,
                finding.question,
            ),
        )

    def to_json(self) -> str:
        document = {
            "check": self.check,
            "revision": self.revision,
            "model": self.model,
            "calls": self.calls,
            "input_tokens": self.input_tokens,
            "cost_usd": self.cost_usd,
            "findings": [
                {
                    "check": finding.check,
                    "subject": finding.subject,
                    "anchor": finding.anchor,
                    "question": finding.question,
                    "answer": finding.answer,
                    "confidence": finding.confidence,
                    "route": finding.route,
                    "detail": dict(finding.detail),
                }
                for finding in self.ranked()
            ],
        }
        return json.dumps(document, indent=2, sort_keys=True) + "\n"

    def to_markdown(self) -> str:
        lines = [
            f"# {self.check}",
            "",
            f"- revision: {self.revision}",
            f"- model: {self.model}",
            f"- calls: {self.calls}",
            f"- input tokens: {self.input_tokens}",
            f"- cost: ${self.cost_usd:.6f}",
            f"- findings: {len(self.findings)}",
            "",
            "| " + " | ".join(COLUMNS) + " |",
            "| " + " | ".join("---" for _ in COLUMNS) + " |",
        ]
        for finding in self.ranked():
            lines.append(
                "| "
                + " | ".join(
                    (
                        finding.route,
                        f"{finding.confidence:.2f}",
                        _cell(finding.subject),
                        _cell(finding.anchor),
                        _cell(finding.question),
                        _cell(finding.answer),
                        _cell(json.dumps(dict(finding.detail), sort_keys=True)),
                    )
                )
                + " |"
            )
        if not self.findings:
            lines.append("")
            lines.append("_no findings_")
        lines.append("")
        return "\n".join(lines)

    def write(self, out_dir: Path) -> tuple[Path, Path]:
        out_dir.mkdir(parents=True, exist_ok=True)
        json_path = out_dir / f"{self.check}.json"
        md_path = out_dir / f"{self.check}.md"
        json_path.write_text(self.to_json(), encoding="utf-8")
        md_path.write_text(self.to_markdown(), encoding="utf-8")
        return json_path, md_path


def _cell(value: object) -> str:
    text = value if isinstance(value, str) else f"{value}"
    return text.replace("|", "\\|").replace("\r", " ").replace("\n", " ").strip()


def git_revision(repo_root: Path) -> str:
    completed = subprocess.run(
        ("git", "rev-parse", "HEAD"),
        cwd=repo_root,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    if completed.returncode != 0:
        return "unknown"
    return completed.stdout.strip() or "unknown"
