from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from tools.jev.report import Finding, Report, git_revision, route_for


ROOT = Path(__file__).resolve().parents[3]


def finding(subject: str, confidence: float, route: str, question: str = "fit") -> Finding:
    return Finding(
        check="pr-coherence",
        subject=subject,
        anchor=f"tools/jev/{subject}.py:1",
        question=question,
        answer="billing",
        confidence=confidence,
        route=route,
        detail={"probabilities": {"billing": confidence}},
    )


def table_rows(markdown: str) -> list[list[str]]:
    rows = []
    for line in markdown.splitlines():
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if cells[0] in ("Route", "---"):
            continue
        rows.append(cells)
    return rows


class RouteTests(unittest.TestCase):
    def test_default_thresholds(self) -> None:
        self.assertEqual(route_for(1.0), "auto")
        self.assertEqual(route_for(0.9), "auto")
        self.assertEqual(route_for(0.8999), "review")
        self.assertEqual(route_for(0.5), "review")
        self.assertEqual(route_for(0.4999), "escalate")
        self.assertEqual(route_for(0.0), "escalate")

    def test_custom_thresholds(self) -> None:
        self.assertEqual(route_for(0.75, auto_at=0.7, review_at=0.3), "auto")
        self.assertEqual(route_for(0.35, auto_at=0.7, review_at=0.3), "review")
        self.assertEqual(route_for(0.29, auto_at=0.7, review_at=0.3), "escalate")


class ReportTests(unittest.TestCase):
    def report(self) -> Report:
        return Report(
            check="pr-coherence",
            revision="8506f3e17",
            model="typesafe/jev-1.13-20260917",
            findings=[
                finding("client", 0.95, "auto"),
                finding("report", 0.6, "review"),
                finding("cli", 0.2, "escalate"),
                finding("checks", 0.8, "review"),
                finding("main", 0.4, "escalate"),
            ],
            calls=3,
            input_tokens=1281,
            cost_usd=0.000054,
        )

    def test_markdown_puts_escalations_first_then_highest_confidence(self) -> None:
        rows = table_rows(self.report().to_markdown())
        self.assertEqual(
            [(row[0], row[1]) for row in rows],
            [
                ("escalate", "0.40"),
                ("escalate", "0.20"),
                ("review", "0.80"),
                ("review", "0.60"),
                ("auto", "0.95"),
            ],
        )

    def test_markdown_header_carries_the_run_identity(self) -> None:
        markdown = self.report().to_markdown()
        self.assertIn("# pr-coherence", markdown)
        self.assertIn("- revision: 8506f3e17", markdown)
        self.assertIn("- model: typesafe/jev-1.13-20260917", markdown)
        self.assertIn("- calls: 3", markdown)
        self.assertIn("- input tokens: 1281", markdown)
        self.assertIn("- cost: $0.000054", markdown)

    def test_markdown_escapes_table_separators(self) -> None:
        report = Report(
            check="pr-coherence",
            revision="8506f3e17",
            model="typesafe/jev-1.13",
            findings=[finding("a|b", 0.55, "review", question="left|right")],
        )
        rows = table_rows(report.to_markdown())
        self.assertEqual(len(rows), 1)
        self.assertIn("a\\|b", report.to_markdown())

    def test_empty_report_renders_without_rows(self) -> None:
        report = Report(check="pr-coherence", revision="8506f3e17", model="typesafe/jev-1.13")
        self.assertEqual(table_rows(report.to_markdown()), [])
        self.assertIn("_no findings_", report.to_markdown())
        self.assertEqual(json.loads(report.to_json())["findings"], [])

    def test_json_keeps_the_ranked_order_and_every_field(self) -> None:
        document = json.loads(self.report().to_json())
        self.assertEqual(document["check"], "pr-coherence")
        self.assertEqual(document["revision"], "8506f3e17")
        self.assertEqual(document["calls"], 3)
        self.assertEqual(document["input_tokens"], 1281)
        self.assertAlmostEqual(document["cost_usd"], 0.000054)
        self.assertEqual(
            [item["subject"] for item in document["findings"]],
            ["main", "cli", "checks", "report", "client"],
        )
        first = document["findings"][0]
        self.assertEqual(
            set(first),
            {
                "check",
                "subject",
                "anchor",
                "question",
                "answer",
                "confidence",
                "route",
                "detail",
            },
        )
        self.assertEqual(first["route"], "escalate")
        self.assertEqual(first["anchor"], "tools/jev/main.py:1")
        self.assertEqual(first["detail"], {"probabilities": {"billing": 0.4}})

    def test_write_emits_files_named_after_the_check(self) -> None:
        report = self.report()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            json_path, md_path = report.write(out_dir)
            self.assertEqual(json_path, out_dir / "pr-coherence.json")
            self.assertEqual(md_path, out_dir / "pr-coherence.md")
            self.assertEqual(json_path.read_text(encoding="utf-8"), report.to_json())
            self.assertEqual(md_path.read_text(encoding="utf-8"), report.to_markdown())
            self.assertEqual(
                json.loads(json_path.read_text(encoding="utf-8"))["model"],
                "typesafe/jev-1.13-20260917",
            )


class RevisionTests(unittest.TestCase):
    def test_git_revision_reads_the_repository_head(self) -> None:
        revision = git_revision(ROOT)
        self.assertEqual(len(revision), 40)
        self.assertTrue(all(character in "0123456789abcdef" for character in revision))


if __name__ == "__main__":
    unittest.main()
