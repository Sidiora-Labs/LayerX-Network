from __future__ import annotations

import contextlib
import io
import json
import os
import re
import subprocess
import tempfile
import unittest
from pathlib import Path
from typing import Mapping, Sequence

from tools.jev.checks import CHECKS, pr
from tools.jev.cli import main
from tools.jev.client import API_KEY_ENV, Evaluation, parse_answer, parse_evaluation
from tools.jev.report import Finding, Report


ROOT = Path(__file__).resolve().parents[3]
TEMPLATE = ROOT / ".github" / "pull_request_template.md"
ANCHOR = re.compile(r"^.+:[0-9]+$")
SHA = re.compile(r"^[0-9a-f]{40}$")
REQUEST_DIGEST = "a" * 64
RESPONSE_DIGEST = "b" * 64


def git(*arguments: str) -> str:
    completed = subprocess.run(
        ("git", *arguments),
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    return completed.stdout


def head_base(depth: int = 1) -> str:
    return git("rev-parse", f"HEAD~{depth}").strip()


def evaluation_for(answers: Mapping[str, Mapping[str, object]]) -> Evaluation:
    document = {
        "model": "typesafe/jev-1.13-20260917",
        "answers": dict(answers),
        "usage": {"input_tokens": 512, "output_tokens": 40, "cost": 0.0000215},
        "id": "gen-dec-1789717233-test",
    }
    return parse_evaluation(document, "typesafe/jev-1.13", REQUEST_DIGEST, RESPONSE_DIGEST)


def body_for(ticked: Sequence[int], extra: Sequence[str] = ()) -> str:
    lines = [
        "## Summary",
        "",
        "Recorded the wave 8 verify gates in the beta spec and its task mirror.",
        "",
        "## Test evidence",
        "",
        "```",
        "command: python3 -m unittest tools.jev.tests.test_pr",
        "exit code: 0",
        "```",
        "",
        "## Checklist",
        "",
    ]
    for index, item in enumerate(pr.template_items(ROOT), start=1):
        lines.append(f"- [{'x' if index in ticked else ' '}] {item}")
    lines.extend(extra)
    lines.append("")
    return "\n".join(lines)


def line_of(body: str, fragment: str) -> int:
    for number, line in enumerate(body.splitlines(), start=1):
        if fragment in line:
            return number
    raise AssertionError(f"{fragment!r} is not in the body")


class DiffCollectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.base = head_base()
        self.diff = pr.collect_diff(ROOT, self.base, "HEAD")

    def test_changed_files_match_git_name_only(self) -> None:
        expected = {
            line.strip()
            for line in git("diff", "--name-only", f"{self.base}..HEAD").splitlines()
            if line.strip()
        }
        self.assertTrue(expected)
        self.assertEqual({change.path for change in self.diff.changes}, expected)
        for change in self.diff.changes:
            self.assertIn(change.status[:1], set("AMDRCTU"))
            self.assertNotIn("\t", change.path)

    def test_stat_summary_matches_numstat(self) -> None:
        insertions = 0
        deletions = 0
        for line in git("diff", "--numstat", f"{self.base}..HEAD").splitlines():
            fields = line.split("\t")
            if len(fields) < 3 or fields[0] == "-":
                continue
            insertions += int(fields[0])
            deletions += int(fields[1])
        self.assertEqual(self.diff.stat.insertions, insertions)
        self.assertEqual(self.diff.stat.deletions, deletions)
        self.assertEqual(self.diff.stat.files, len(self.diff.changes))
        self.assertIn("changed", self.diff.stat.text)

    def test_empty_range_yields_zero_stat(self) -> None:
        empty = pr.collect_diff(ROOT, "HEAD", "HEAD")
        self.assertEqual(empty.changes, ())
        self.assertEqual(empty.stat, pr.StatSummary(files=0, insertions=0, deletions=0, text=""))

    def test_state_carries_the_file_list_and_no_diff_content(self) -> None:
        state = pr.changes_state(self.diff.changes, self.diff.stat)
        encoded = json.dumps(state)
        self.assertNotIn("@@", encoded)
        self.assertNotIn("diff --git", encoded)
        self.assertNotIn("+++", encoded)
        self.assertEqual(state["changed_file_count"], len(self.diff.changes))
        self.assertEqual(state["listed_file_count"], len(self.diff.changes))
        self.assertEqual(
            state["changed_files"],
            [f"{change.status} {change.path}" for change in self.diff.changes],
        )
        self.assertEqual(
            set(state["signals"]),
            {"tests_touched", "workflows_touched", "makefile_touched", "docs_only"},
        )
        self.assertEqual(
            sum(state["counts"].values()),
            len(self.diff.changes),
        )

    def test_unknown_revision_is_a_configuration_error(self) -> None:
        with self.assertRaises(pr.JevConfigError):
            pr.collect_diff(ROOT, "not-a-real-revision", "HEAD")


class CommitCollectionTests(unittest.TestCase):
    def test_commits_carry_the_real_message_and_stat(self) -> None:
        base = head_base(3)
        commits = pr.collect_commits(ROOT, base, "HEAD")
        expected = [
            line.strip()
            for line in git("log", "--no-merges", "--format=%H", f"{base}..HEAD").splitlines()
            if line.strip()
        ]
        expected.reverse()
        self.assertEqual([commit.sha for commit in commits], expected)
        for commit in commits:
            self.assertTrue(SHA.match(commit.sha))
            self.assertEqual(commit.subject, git("log", "-1", "--format=%s", commit.sha).strip())
            self.assertEqual(commit.message, git("log", "-1", "--format=%B", commit.sha).strip())
            self.assertTrue(commit.changes)
            self.assertEqual(commit.stat.files, len(commit.changes))

    def test_commit_state_keeps_the_message_and_drops_the_diff(self) -> None:
        commits = pr.collect_commits(ROOT, head_base(), "HEAD")
        self.assertEqual(len(commits), 1)
        state = pr.commit_state(commits[0])
        self.assertEqual(state["commit"], commits[0].sha)
        self.assertEqual(state["subject"], commits[0].subject)
        self.assertEqual(state["message"], commits[0].message)
        self.assertFalse(state["message_truncated"])
        self.assertNotIn("@@", json.dumps(state))


class SignalTests(unittest.TestCase):
    def test_test_paths_are_detected(self) -> None:
        tracked = [line.strip() for line in git("ls-files", "tools").splitlines() if line.strip()]
        self.assertTrue(pr.touches_tests(tracked))
        self.assertTrue(pr.touches_tests(["contracts/test/Vault.t.sol"]))
        self.assertTrue(pr.touches_tests(["apps/human/e2e/deposit.spec.ts"]))
        self.assertTrue(pr.touches_tests(["node/src/ledger_test.rs"]))
        self.assertFalse(pr.touches_tests(["tools/jev/client.py", "README.md"]))

    def test_workflow_and_makefile_paths_are_detected(self) -> None:
        self.assertTrue(pr.touches_workflows([".github/workflows/ci.yml"]))
        self.assertFalse(pr.touches_workflows([".github/labeler.yml"]))
        self.assertTrue(pr.touches_makefile(["Makefile"]))
        self.assertTrue(pr.touches_makefile(["node/Makefile"]))
        self.assertTrue(pr.touches_makefile(["build/rules.mk"]))
        self.assertFalse(pr.touches_makefile(["tools/jev/cli.py"]))

    def test_docs_only_needs_every_path_to_be_a_document(self) -> None:
        self.assertTrue(pr.is_docs_only(["README.md", "docs/architecture/overview.rst"]))
        self.assertFalse(pr.is_docs_only(["README.md", "tools/jev/cli.py"]))
        self.assertFalse(pr.is_docs_only([]))

    def test_status_counts_cover_every_change(self) -> None:
        counts = pr.status_counts(
            (
                pr.FileChange(status="A", path="a.py"),
                pr.FileChange(status="M", path="b.py"),
                pr.FileChange(status="D", path="c.py"),
                pr.FileChange(status="R100", path="d.py"),
                pr.FileChange(status="T", path="e.py"),
            )
        )
        self.assertEqual(
            counts,
            {"added": 1, "modified": 1, "deleted": 1, "renamed": 1, "other": 1},
        )


class ChecklistTests(unittest.TestCase):
    def test_template_items_come_from_the_real_template(self) -> None:
        items = pr.template_items(ROOT)
        self.assertEqual(len(items), 7)
        self.assertTrue(items[0].startswith("I changed KVX/source documents"))
        self.assertIn("DCO", items[5])
        self.assertIn("No weakening", items[6])
        for item in items:
            self.assertTrue(item.strip())
            self.assertNotIn("[ ]", item)

    def test_untouched_template_ticks_nothing(self) -> None:
        body = TEMPLATE.read_text(encoding="utf-8")
        self.assertEqual(pr.ticked_items(pr.template_items(ROOT), body), ())

    def test_only_ticked_template_items_are_returned(self) -> None:
        body = body_for((1, 3, 6), extra=["- [x] I fed the cat."])
        items = pr.ticked_items(pr.template_items(ROOT), body)
        self.assertEqual([item.index for item in items], [1, 3, 6])
        for item in items:
            self.assertEqual(item.line, line_of(body, item.text))
            self.assertTrue(pr.same_item(item.text, item.template_text))
        self.assertNotIn("I fed the cat.", [item.text for item in items])

    def test_edited_wording_still_matches_its_template_item(self) -> None:
        template = pr.template_items(ROOT)
        body = "\n".join(
            [
                "## Checklist",
                "",
                f"- [X]   {template[1].rstrip('.').upper()}  ",
                f"- [x] {template[5]} Every commit carries the trailer.",
                "",
            ]
        )
        items = pr.ticked_items(template, body)
        self.assertEqual([item.index for item in items], [2, 6])
        self.assertEqual(items[0].line, 3)
        self.assertEqual(items[1].line, 4)

    def test_unrelated_short_text_does_not_match(self) -> None:
        self.assertFalse(pr.same_item("I added tests.", "I fed the cat."))
        self.assertFalse(pr.same_item("", "I fed the cat."))


class SectionTests(unittest.TestCase):
    def test_summary_section_is_read_from_the_real_template(self) -> None:
        body = TEMPLATE.read_text(encoding="utf-8")
        line, text = pr.section(body, "Summary")
        self.assertEqual(line, 1)
        self.assertTrue(text.startswith("Describe the change"))
        self.assertNotIn("## Specification", text)

    def test_missing_section_reports_line_zero(self) -> None:
        self.assertEqual(pr.section("no headings here", "Summary"), (0, ""))


class QuestionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.body = body_for((1, 3))
        self.items = pr.ticked_items(pr.template_items(ROOT), self.body)
        self.diff = pr.collect_diff(ROOT, head_base(), "HEAD")

    def test_body_questions_cover_summary_verification_and_ticked_items(self) -> None:
        questions = pr.body_questions(self.items)
        self.assertEqual(
            set(questions),
            {pr.SUMMARY_QUESTION, pr.VERIFICATION_QUESTION, "checklist_1", "checklist_3"},
        )
        self.assertEqual(questions[pr.SUMMARY_QUESTION]["type"], "noul")
        self.assertEqual(questions[pr.SUMMARY_QUESTION]["instructions"], pr.SUMMARY_STATEMENT)
        self.assertEqual(questions[pr.VERIFICATION_QUESTION]["type"], "choice")
        self.assertEqual(
            set(questions[pr.VERIFICATION_QUESTION]["criteria"]),
            {"none", "local_tests", "ci_gates", "production_certification"},
        )
        for item in self.items:
            instructions = questions[f"checklist_{item.index}"]["instructions"]
            self.assertIn(item.text, instructions)

    def test_body_state_carries_the_prose_and_the_ticked_item_text(self) -> None:
        state = pr.body_state(self.diff, self.body, self.items)
        self.assertEqual(state["range"], f"{self.diff.base}..{self.diff.head}")
        self.assertTrue(state["summary"].startswith("Recorded the wave 8 verify gates"))
        self.assertEqual(state["body"], self.body)
        self.assertFalse(state["body_truncated"])
        self.assertEqual(
            state["ticked_checklist"],
            {f"checklist_{item.index}": item.text for item in self.items},
        )
        self.assertEqual(
            set(state["ticked_checklist"]),
            {name for name in pr.body_questions(self.items) if name.startswith("checklist_")},
        )
        self.assertNotIn("@@", json.dumps(state))

    def test_commit_questions_ask_for_honesty_and_unevidenced_claims(self) -> None:
        questions = pr.commit_questions()
        self.assertEqual(
            set(questions), {pr.COMMIT_CLAIM_QUESTION, pr.COMMIT_HONESTY_QUESTION}
        )
        self.assertEqual(questions[pr.COMMIT_CLAIM_QUESTION]["type"], "noul")
        self.assertEqual(questions[pr.COMMIT_CLAIM_QUESTION]["instructions"], pr.CLAIM_STATEMENT)
        self.assertEqual(
            set(questions[pr.COMMIT_HONESTY_QUESTION]["criteria"]),
            {"consistent", "overclaims", "underclaims", "unclear"},
        )


class ConfidenceTests(unittest.TestCase):
    def test_noul_confidence_follows_the_expected_direction(self) -> None:
        answer = parse_answer("q", {"type": "noul", "noul": 0.12})
        self.assertAlmostEqual(pr.noul_confidence(answer, expect_true=True), 0.12)
        self.assertAlmostEqual(pr.noul_confidence(answer, expect_true=False), 0.88)

    def test_choice_confidence_uses_the_accepted_probability_mass(self) -> None:
        answer = parse_answer(
            "q",
            {
                "type": "choice",
                "choice": "ci_gates",
                "probabilities": {
                    "none": 0.05,
                    "local_tests": 0.15,
                    "ci_gates": 0.7,
                    "production_certification": 0.1,
                },
                "confidence": 0.7,
            },
        )
        self.assertAlmostEqual(pr.choice_confidence(answer, pr.ACCEPTED_LEVELS), 0.9)

    def test_choice_confidence_falls_back_to_the_reported_confidence(self) -> None:
        accepted = parse_answer(
            "q", {"type": "choice", "choice": "consistent", "confidence": 0.64}
        )
        rejected = parse_answer(
            "q", {"type": "choice", "choice": "overclaims", "confidence": 0.64}
        )
        self.assertAlmostEqual(pr.choice_confidence(accepted, pr.ACCEPTED_HONESTY), 0.64)
        self.assertAlmostEqual(pr.choice_confidence(rejected, pr.ACCEPTED_HONESTY), 0.36)


class BodyFindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.body = body_for((1, 3))
        self.items = pr.ticked_items(pr.template_items(ROOT), self.body)
        self.path = Path("/tmp/jev-pr-body.md")

    def findings(self, verification: str, summary: float = 0.12) -> list[Finding]:
        probabilities = {
            "none": 0.02,
            "local_tests": 0.08,
            "ci_gates": 0.2,
            "production_certification": 0.7,
        }
        if verification != pr.PRODUCTION_LEVEL:
            probabilities = {
                "none": 0.02,
                "local_tests": 0.88,
                "ci_gates": 0.08,
                "production_certification": 0.02,
            }
        evaluation = evaluation_for(
            {
                pr.SUMMARY_QUESTION: {"type": "noul", "noul": summary},
                pr.VERIFICATION_QUESTION: {
                    "type": "choice",
                    "choice": verification,
                    "probabilities": probabilities,
                    "confidence": 0.7,
                },
                "checklist_1": {"type": "noul", "noul": 0.93},
                "checklist_3": {"type": "noul", "noul": 0.41},
            }
        )
        return pr.body_findings(evaluation, self.path, self.body, self.items)

    def test_every_finding_is_anchored_and_named_for_the_check(self) -> None:
        findings = self.findings("local_tests")
        self.assertEqual(len(findings), 4)
        for finding in findings:
            self.assertEqual(finding.check, "pr")
            self.assertTrue(ANCHOR.match(finding.anchor), finding.anchor)
            self.assertTrue(finding.anchor.startswith(f"{self.path}:"))
            self.assertGreaterEqual(finding.confidence, 0.0)
            self.assertLessEqual(finding.confidence, 1.0)
            self.assertIn(finding.route, ("auto", "review", "escalate"))

    def test_summary_and_checklist_anchors_point_at_their_body_lines(self) -> None:
        findings = {finding.subject: finding for finding in self.findings("local_tests")}
        self.assertEqual(findings["summary"].anchor, f"{self.path}:1")
        self.assertEqual(
            findings["verification"].anchor,
            f"{self.path}:{line_of(self.body, '## Test evidence')}",
        )
        for item in self.items:
            anchor = findings[f"checklist:{item.index}"].anchor
            self.assertEqual(anchor, f"{self.path}:{line_of(self.body, item.text)}")

    def test_inconsistent_summary_escalates_and_keeps_the_probability(self) -> None:
        findings = {finding.subject: finding for finding in self.findings("local_tests")}
        summary = findings["summary"]
        self.assertEqual(summary.question, pr.SUMMARY_STATEMENT)
        self.assertAlmostEqual(summary.answer, 0.12)
        self.assertAlmostEqual(summary.confidence, 0.12)
        self.assertEqual(summary.route, "escalate")
        self.assertAlmostEqual(summary.detail["probabilities"]["true"], 0.12)

    def test_supported_checklist_item_routes_to_auto_and_unsupported_to_review(self) -> None:
        findings = {finding.subject: finding for finding in self.findings("local_tests")}
        supported = findings["checklist:1"]
        unsupported = findings["checklist:3"]
        self.assertEqual(supported.route, "auto")
        self.assertAlmostEqual(supported.confidence, 0.93)
        self.assertIn(self.items[0].text, supported.question)
        self.assertEqual(supported.detail["template_item"], self.items[0].template_text)
        self.assertEqual(unsupported.route, "escalate")
        self.assertAlmostEqual(unsupported.confidence, 0.41)

    def test_claimed_production_certification_is_flagged(self) -> None:
        flagged = {finding.subject: finding for finding in self.findings(pr.PRODUCTION_LEVEL)}
        verification = flagged["verification"]
        self.assertEqual(verification.answer, pr.PRODUCTION_LEVEL)
        self.assertEqual(verification.route, "escalate")
        self.assertTrue(verification.detail["flagged"])
        self.assertAlmostEqual(verification.confidence, 0.3)
        accepted = {finding.subject: finding for finding in self.findings("local_tests")}
        self.assertEqual(accepted["verification"].route, "auto")
        self.assertFalse(accepted["verification"].detail["flagged"])

    def test_missing_answers_produce_no_findings(self) -> None:
        evaluation = evaluation_for({"unrelated": {"type": "noul", "noul": 0.5}})
        self.assertEqual(pr.body_findings(evaluation, self.path, self.body, self.items), [])


class CommitFindingTests(unittest.TestCase):
    def setUp(self) -> None:
        self.commit = pr.collect_commits(ROOT, head_base(), "HEAD")[0]

    def test_commit_findings_are_anchored_on_the_commit_record(self) -> None:
        evaluation = evaluation_for(
            {
                pr.COMMIT_CLAIM_QUESTION: {"type": "noul", "noul": 0.87},
                pr.COMMIT_HONESTY_QUESTION: {
                    "type": "choice",
                    "choice": "overclaims",
                    "probabilities": {
                        "consistent": 0.1,
                        "overclaims": 0.8,
                        "underclaims": 0.05,
                        "unclear": 0.05,
                    },
                    "confidence": 0.8,
                },
            }
        )
        findings = pr.commit_findings(evaluation, self.commit)
        self.assertEqual(len(findings), 2)
        for finding in findings:
            self.assertEqual(finding.check, "pr")
            self.assertEqual(finding.anchor, self.commit.sha)
            self.assertTrue(SHA.match(finding.anchor))
            self.assertEqual(finding.subject, self.commit.sha[:12])
            self.assertEqual(finding.detail["subject"], self.commit.subject)
        claim, honesty = findings
        self.assertAlmostEqual(claim.confidence, 0.13)
        self.assertEqual(claim.route, "escalate")
        self.assertEqual(honesty.answer, "overclaims")
        self.assertAlmostEqual(honesty.confidence, 0.1)
        self.assertEqual(honesty.route, "escalate")

    def test_consistent_message_routes_to_auto(self) -> None:
        evaluation = evaluation_for(
            {
                pr.COMMIT_CLAIM_QUESTION: {"type": "noul", "noul": 0.04},
                pr.COMMIT_HONESTY_QUESTION: {
                    "type": "choice",
                    "choice": "consistent",
                    "probabilities": {
                        "consistent": 0.94,
                        "overclaims": 0.03,
                        "underclaims": 0.02,
                        "unclear": 0.01,
                    },
                    "confidence": 0.94,
                },
            }
        )
        for finding in pr.commit_findings(evaluation, self.commit):
            self.assertEqual(finding.route, "auto")


class CommentTests(unittest.TestCase):
    def report(self) -> Report:
        return Report(
            check="pr",
            revision="8506f3e17cde5c08605c6897098a3fd749cda0a5",
            model="typesafe/jev-1.13-20260917",
            findings=[
                Finding(
                    check="pr",
                    subject="summary",
                    anchor="body.md:1",
                    question=pr.SUMMARY_STATEMENT,
                    answer=0.12,
                    confidence=0.12,
                    route="escalate",
                    detail={},
                ),
                Finding(
                    check="pr",
                    subject="checklist:3",
                    anchor="body.md:19",
                    question=pr.checklist_statement("I added real positive coverage."),
                    answer=0.62,
                    confidence=0.62,
                    route="review",
                    detail={},
                ),
                Finding(
                    check="pr",
                    subject="verification",
                    anchor="body.md:9",
                    question=pr.VERIFICATION_INSTRUCTIONS,
                    answer="local_tests",
                    confidence=0.96,
                    route="auto",
                    detail={},
                ),
            ],
            calls=1,
            input_tokens=512,
            cost_usd=0.0000215,
        )

    def test_comment_lists_only_review_and_escalate_findings(self) -> None:
        text = pr.render_comment(self.report())
        self.assertIn("## Jev advisory (non-blocking)", text)
        self.assertIn("- model: typesafe/jev-1.13-20260917", text)
        self.assertIn("- revision: 8506f3e17cde5c08605c6897098a3fd749cda0a5", text)
        self.assertIn("- surfaced: 2 of 3 findings", text)
        self.assertIn("body.md:1", text)
        self.assertIn("body.md:19", text)
        self.assertNotIn("body.md:9", text)
        self.assertNotIn("local_tests", text)
        self.assertIn("`0.12`", text)
        self.assertIn(pr.COMMENT_FOOTER, text)

    def test_comment_puts_escalations_before_reviews(self) -> None:
        lines = pr.render_comment(self.report()).splitlines()
        escalate = next(index for index, line in enumerate(lines) if "**escalate**" in line)
        review = next(index for index, line in enumerate(lines) if "**review**" in line)
        self.assertLess(escalate, review)

    def test_comment_without_surfaced_findings_says_so(self) -> None:
        report = Report(check="pr", revision="8506f3e17", model="typesafe/jev-1.13")
        text = pr.render_comment(report)
        self.assertIn("_no review or escalate findings_", text)
        self.assertIn("- surfaced: 0 of 0 findings", text)

    def test_write_comment_creates_the_file(self) -> None:
        report = self.report()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "advisory" / "pr.md"
            written = pr.write_comment(path, report)
            self.assertEqual(written, path)
            self.assertEqual(path.read_text(encoding="utf-8"), pr.render_comment(report))


class CliTests(unittest.TestCase):
    def test_the_check_is_registered_under_its_name(self) -> None:
        self.assertIs(CHECKS["pr"], pr.CHECK)
        self.assertEqual(pr.CHECK_NAME, "pr")

    def test_dry_run_exits_zero_without_writing_a_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "reports"
            body = Path(directory) / "body.md"
            body.write_text(body_for((1, 3)), encoding="utf-8")
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
                code = main(
                    [
                        "pr",
                        "--base",
                        head_base(),
                        "--head",
                        "HEAD",
                        "--body",
                        str(body),
                        "--out",
                        str(out),
                        "--dry-run",
                    ]
                )
            self.assertEqual(code, 0)
            self.assertIn("dry run", stderr.getvalue())
            self.assertIn("4 questions", stderr.getvalue())
            self.assertFalse((out / "pr.json").exists())

    def test_missing_body_file_is_a_configuration_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr), contextlib.redirect_stdout(io.StringIO()):
                code = main(
                    [
                        "pr",
                        "--base",
                        head_base(),
                        "--body",
                        str(Path(directory) / "absent.md"),
                        "--out",
                        str(Path(directory) / "reports"),
                    ]
                )
            self.assertEqual(code, 2)
            self.assertIn("is not configured", stderr.getvalue())

    def test_run_without_prose_writes_an_empty_report_and_touches_nothing(self) -> None:
        before = git("status", "--porcelain")
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "reports"
            comment = Path(directory) / "advisory.md"
            with contextlib.redirect_stdout(io.StringIO()):
                code = main(
                    [
                        "pr",
                        "--base",
                        head_base(),
                        "--out",
                        str(out),
                        "--comment",
                        str(comment),
                    ]
                )
            self.assertEqual(code, 0)
            document = json.loads((out / "pr.json").read_text(encoding="utf-8"))
            self.assertEqual(document["check"], "pr")
            self.assertEqual(document["findings"], [])
            self.assertEqual(document["calls"], 0)
            self.assertEqual(document["cost_usd"], 0.0)
            self.assertEqual(len(document["revision"]), 40)
            self.assertIn("_no review or escalate findings_", comment.read_text(encoding="utf-8"))
        self.assertEqual(git("status", "--porcelain"), before)


class LiveTests(unittest.TestCase):
    def setUp(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")

    def test_live_body_run_reports_anchored_findings(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "reports"
            body = Path(directory) / "body.md"
            comment = Path(directory) / "advisory.md"
            body.write_text(body_for((1, 3, 6)), encoding="utf-8")
            with contextlib.redirect_stdout(io.StringIO()):
                code = main(
                    [
                        "pr",
                        "--base",
                        head_base(),
                        "--head",
                        "HEAD",
                        "--body",
                        str(body),
                        "--out",
                        str(out),
                        "--comment",
                        str(comment),
                    ]
                )
            self.assertEqual(code, 0)
            document = json.loads((out / "pr.json").read_text(encoding="utf-8"))
            self.assertEqual(document["check"], "pr")
            self.assertTrue(document["model"].startswith("typesafe/jev-1.13"))
            self.assertEqual(document["calls"], 1)
            self.assertGreater(document["input_tokens"], 0)
            self.assertGreater(document["cost_usd"], 0.0)
            subjects = {finding["subject"] for finding in document["findings"]}
            self.assertEqual(
                subjects,
                {"summary", "verification", "checklist:1", "checklist:3", "checklist:6"},
            )
            for finding in document["findings"]:
                self.assertEqual(finding["check"], "pr")
                self.assertTrue(ANCHOR.match(finding["anchor"]), finding["anchor"])
                self.assertGreaterEqual(finding["confidence"], 0.0)
                self.assertLessEqual(finding["confidence"], 1.0)
                self.assertIn(finding["route"], ("auto", "review", "escalate"))
            verification = next(
                finding for finding in document["findings"] if finding["subject"] == "verification"
            )
            self.assertIn(
                verification["answer"],
                {"none", "local_tests", "ci_gates", "production_certification"},
            )
            advisory = comment.read_text(encoding="utf-8")
            self.assertIn("## Jev advisory (non-blocking)", advisory)
            self.assertIn(document["model"], advisory)
            self.assertIn(document["revision"], advisory)

    def test_live_commit_run_judges_every_commit(self) -> None:
        base = head_base()
        with tempfile.TemporaryDirectory() as directory:
            out = Path(directory) / "reports"
            with contextlib.redirect_stdout(io.StringIO()):
                code = main(
                    ["pr", "--base", base, "--head", "HEAD", "--commits", "--out", str(out)]
                )
            self.assertEqual(code, 0)
            document = json.loads((out / "pr.json").read_text(encoding="utf-8"))
            self.assertEqual(document["calls"], 1)
            self.assertEqual(len(document["findings"]), 2)
            sha = pr.collect_commits(ROOT, base, "HEAD")[0].sha
            questions = set()
            for finding in document["findings"]:
                self.assertEqual(finding["anchor"], sha)
                self.assertEqual(finding["subject"], sha[:12])
                questions.add(finding["question"])
            self.assertEqual(questions, {pr.CLAIM_STATEMENT, pr.HONESTY_INSTRUCTIONS})


if __name__ == "__main__":
    unittest.main()
