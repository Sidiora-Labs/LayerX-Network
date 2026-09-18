from __future__ import annotations

import argparse
import hashlib
import json
import os
import tempfile
import tomllib
import unittest
from pathlib import Path

from tools.jev import cli
from tools.jev.checks import CHECKS
from tools.jev.checks.deps import (
    ADVISORY_TEXT_LIMIT,
    CHECK,
    CHECK_NAME,
    DEV_ONLY,
    GRAPH_DEPTH,
    PRIORITY_OPTIONS,
    PRIORITY_QUESTION,
    PRODUCTION,
    UNKNOWN,
    URGENCY_LABELS,
    URGENCY_LEVELS,
    URGENCY_QUESTION,
    WORKSPACES,
    Advisory,
    DependencyIndex,
    advisory_questions,
    advisory_state,
    collect_sources,
    combine_dev_flags,
    findings_for,
    group_advisories,
    input_pair,
    parse_diagnostics,
    scan_workspace,
    urgency_label,
    workspace_manifests,
)
from tools.jev.client import API_KEY_ENV, DEFAULT_MODEL, JevClient, parse_evaluation
from tools.jev.report import Report, route_for


ROOT = Path(__file__).resolve().parents[3]
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "deps"
AGENT_CAPTURE = FIXTURES / "agent-advisories.jsonl"
INTEROP_CAPTURE = FIXTURES / "interop-advisories.jsonl"
PASTE_ADVISORY = "RUSTSEC-2024-0436"
RUSTLS_ADVISORY = "RUSTSEC-2026-0285"
DIGEST = "0" * 64
RESPONSE = {
    "model": "typesafe/jev-1.13-20260917",
    "answers": {
        "priority": {
            "type": "choice",
            "choice": "production_reachable",
            "probabilities": {
                "production_reachable": 0.91,
                "dev_only": 0.05,
                "already_mitigated": 0.02,
                "needs_human": 0.02,
            },
            "confidence": 0.91,
        },
        "urgency": {
            "type": "score",
            "score": 1.62,
            "legend": {"0": "low", "1": "medium", "2": "high"},
            "probabilities": {"0": 0.02, "1": 0.34, "2": 0.64},
            "confidence": 0.58,
        },
    },
    "usage": {"input_tokens": 512, "output_tokens": 64, "cost": 0.0000215},
    "id": "gen-dec-1789715517-deps",
}


def captured(path: Path, workspace: str) -> list:
    return parse_diagnostics(path.read_text(encoding="utf-8"), workspace)


def grouped() -> dict[str, Advisory]:
    diagnostics = captured(AGENT_CAPTURE, "agent") + captured(INTEROP_CAPTURE, "interop")
    advisories = group_advisories(diagnostics, DependencyIndex(repo_root=ROOT))
    return {advisory.advisory_id: advisory for advisory in advisories}


def parsed_args(argv: list[str]) -> argparse.Namespace:
    parser = cli.build_parser(CHECK_NAME)
    CHECK.add_arguments(parser)
    return parser.parse_args(argv)


def deny_digests() -> dict[str, str]:
    return {
        workspace: hashlib.sha256((ROOT / workspace / "deny.toml").read_bytes()).hexdigest()
        for workspace in WORKSPACES
    }


class ParseDiagnosticsTests(unittest.TestCase):
    def test_capture_yields_only_the_advisory_diagnostics(self) -> None:
        lines = INTEROP_CAPTURE.read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(lines), 4)
        diagnostics = captured(INTEROP_CAPTURE, "interop")
        self.assertEqual(
            sorted(diagnostic.advisory_id for diagnostic in diagnostics),
            [PASTE_ADVISORY, RUSTLS_ADVISORY],
        )
        self.assertTrue(all(diagnostic.workspace == "interop" for diagnostic in diagnostics))

    def test_unmaintained_diagnostic_keeps_crate_anchor_and_graph(self) -> None:
        diagnostic = next(
            item
            for item in captured(INTEROP_CAPTURE, "interop")
            if item.advisory_id == PASTE_ADVISORY
        )
        self.assertEqual(diagnostic.code, "unmaintained")
        self.assertEqual(diagnostic.severity, "error")
        self.assertEqual(diagnostic.crate, "paste")
        self.assertEqual(diagnostic.version, "1.0.15")
        self.assertEqual(diagnostic.anchor, "interop/Cargo.lock:109")
        self.assertEqual(diagnostic.message, "paste - no longer maintained")
        self.assertIn("archived the repository", diagnostic.description)
        self.assertEqual(diagnostic.graph_path[0], "paste 1.0.15")
        self.assertIn("wasmi_core 0.13.0", diagnostic.graph_path)
        self.assertLessEqual(len(diagnostic.graph_path), GRAPH_DEPTH)

    def test_vulnerability_diagnostic_keeps_its_own_lock_line(self) -> None:
        interop = next(
            item
            for item in captured(INTEROP_CAPTURE, "interop")
            if item.advisory_id == RUSTLS_ADVISORY
        )
        agent = next(
            item
            for item in captured(AGENT_CAPTURE, "agent")
            if item.advisory_id == RUSTLS_ADVISORY
        )
        self.assertEqual(interop.code, "vulnerability")
        self.assertEqual(interop.crate, "rustls")
        self.assertEqual(interop.version, "0.23.43")
        self.assertEqual(interop.anchor, "interop/Cargo.lock:128")
        self.assertEqual(agent.anchor, "agent/Cargo.lock:119")
        self.assertEqual(agent.graph_path[0], "rustls 0.23.43")

    def test_blank_summary_and_malformed_lines_are_skipped(self) -> None:
        text = "\n   \nnot json\n{\"type\": \"summary\", \"fields\": {\"advisories\": {}}}\n"
        self.assertEqual(parse_diagnostics(text, "agent"), [])


class DependencyScanTests(unittest.TestCase):
    def test_manifests_cover_the_declared_workspace_members(self) -> None:
        root = ROOT / "agent"
        manifests = workspace_manifests(root)
        document = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
        expected = {root / "Cargo.toml"}
        for member in document["workspace"]["members"]:
            expected.add(root / member / "Cargo.toml")
        self.assertEqual(set(manifests), expected)
        self.assertEqual(len(manifests), len(expected))
        self.assertNotIn(root / "qualification" / "pay2" / "Cargo.toml", set(manifests))

    def test_direct_dev_and_transitive_crates_are_separated(self) -> None:
        index = DependencyIndex(repo_root=ROOT)
        self.assertEqual(index.classify("agent", "rustls"), PRODUCTION)
        self.assertEqual(index.classify("agent", "native-tls"), DEV_ONLY)
        self.assertEqual(index.classify("agent", "paste"), UNKNOWN)
        self.assertEqual(index.classify("interop", "rustls"), PRODUCTION)

    def test_renamed_dependency_is_indexed_by_its_package_name(self) -> None:
        index = DependencyIndex(repo_root=ROOT)
        self.assertEqual(index.classify("agent", "layerx-programs-registry"), PRODUCTION)
        self.assertEqual(index.classify("agent", "layerx-programs"), UNKNOWN)

    def test_every_workspace_scans_into_disjoint_sets(self) -> None:
        for workspace in WORKSPACES:
            deps = scan_workspace(ROOT / workspace)
            self.assertTrue(deps.normal, workspace)
            self.assertEqual(deps.normal & deps.dev, frozenset(), workspace)

    def test_missing_workspace_is_unknown(self) -> None:
        index = DependencyIndex(repo_root=ROOT)
        self.assertEqual(workspace_manifests(ROOT / "not-a-workspace"), [])
        self.assertEqual(index.classify("not-a-workspace", "rustls"), UNKNOWN)


class GroupingTests(unittest.TestCase):
    def test_duplicate_ids_collapse_and_list_every_workspace(self) -> None:
        groups = grouped()
        self.assertEqual(sorted(groups), [PASTE_ADVISORY, RUSTLS_ADVISORY])
        paste = groups[PASTE_ADVISORY]
        self.assertEqual(paste.crate, "paste")
        self.assertEqual(paste.workspaces, ("agent", "interop"))
        self.assertEqual(paste.versions, ("1.0.15",))
        self.assertEqual(paste.anchors, ("agent/Cargo.lock:101", "interop/Cargo.lock:109"))
        self.assertEqual(paste.anchor, "agent/Cargo.lock:101")
        self.assertEqual(paste.dev_by_workspace, {"agent": UNKNOWN, "interop": UNKNOWN})
        self.assertEqual(paste.dev_flag, UNKNOWN)

    def test_production_usage_decides_the_group_flag(self) -> None:
        rustls = grouped()[RUSTLS_ADVISORY]
        self.assertEqual(rustls.code, "vulnerability")
        self.assertEqual(rustls.versions, ("0.23.43",))
        self.assertEqual(rustls.workspaces, ("agent", "interop"))
        self.assertEqual(rustls.dev_by_workspace, {"agent": PRODUCTION, "interop": PRODUCTION})
        self.assertEqual(rustls.dev_flag, PRODUCTION)

    def test_flag_precedence_prefers_production_then_dev(self) -> None:
        self.assertEqual(combine_dev_flags([UNKNOWN, DEV_ONLY, PRODUCTION]), PRODUCTION)
        self.assertEqual(combine_dev_flags([UNKNOWN, DEV_ONLY]), DEV_ONLY)
        self.assertEqual(combine_dev_flags([UNKNOWN]), UNKNOWN)
        self.assertEqual(combine_dev_flags([]), UNKNOWN)


class QuestionTests(unittest.TestCase):
    def test_questions_match_the_jev_wire_shape(self) -> None:
        questions = advisory_questions()
        self.assertEqual(set(questions), {PRIORITY_QUESTION, URGENCY_QUESTION})
        priority = questions[PRIORITY_QUESTION]
        self.assertEqual(priority["type"], "choice")
        self.assertEqual(
            set(priority["criteria"]),
            {"production_reachable", "dev_only", "already_mitigated", "needs_human"},
        )
        self.assertEqual(priority["criteria"], PRIORITY_OPTIONS)
        urgency = questions[URGENCY_QUESTION]
        self.assertEqual(urgency["type"], "score")
        self.assertEqual(urgency["criteria"], list(URGENCY_LEVELS))
        self.assertEqual(len(URGENCY_LEVELS), len(URGENCY_LABELS))

    def test_state_carries_the_dev_flag_and_the_advisory_text(self) -> None:
        state = advisory_state(grouped()[PASTE_ADVISORY])
        self.assertEqual(state["advisory_id"], PASTE_ADVISORY)
        self.assertEqual(state["crate"], "paste")
        self.assertEqual(state["kind"], "unmaintained")
        self.assertEqual(state["dependency_kind"], UNKNOWN)
        self.assertEqual(
            state["dependency_kind_by_workspace"], {"agent": UNKNOWN, "interop": UNKNOWN}
        )
        self.assertEqual(state["workspaces"], ["agent", "interop"])
        self.assertEqual(state["crate_versions"], ["1.0.15"])
        self.assertIn("archived the repository", state["advisory_text"])
        self.assertLessEqual(len(state["advisory_text"]), ADVISORY_TEXT_LIMIT)
        self.assertEqual(state["dependency_path"][0], "paste 1.0.15")
        self.assertEqual(json.loads(json.dumps(state))["crate"], "paste")

    def test_state_of_a_production_advisory_reports_production(self) -> None:
        state = advisory_state(grouped()[RUSTLS_ADVISORY])
        self.assertEqual(state["dependency_kind"], PRODUCTION)
        self.assertIn("TLS 1.3", state["title"])

    def test_urgency_label_rounds_and_clamps_the_score(self) -> None:
        self.assertEqual(urgency_label(0.0), "low")
        self.assertEqual(urgency_label(0.4), "low")
        self.assertEqual(urgency_label(0.62), "medium")
        self.assertEqual(urgency_label(1.06), "medium")
        self.assertEqual(urgency_label(1.7), "high")
        self.assertEqual(urgency_label(2.0), "high")
        self.assertEqual(urgency_label(4.5), "high")
        self.assertEqual(urgency_label(-1.0), "low")


class FindingTests(unittest.TestCase):
    def evaluation(self):
        return parse_evaluation(RESPONSE, DEFAULT_MODEL, DIGEST, DIGEST)

    def test_findings_carry_the_anchor_answer_and_route(self) -> None:
        advisory = grouped()[RUSTLS_ADVISORY]
        findings = findings_for(advisory, self.evaluation())
        self.assertEqual([finding.question for finding in findings], ["priority", "urgency"])
        self.assertTrue(all(finding.check == CHECK_NAME for finding in findings))
        self.assertTrue(all(finding.anchor == "agent/Cargo.lock:119" for finding in findings))
        self.assertTrue(
            all(finding.subject == f"{RUSTLS_ADVISORY} rustls" for finding in findings)
        )
        priority, urgency = findings
        self.assertEqual(priority.answer, "production_reachable")
        self.assertEqual(priority.confidence, 0.91)
        self.assertEqual(priority.route, route_for(0.91))
        self.assertEqual(priority.route, "auto")
        self.assertEqual(priority.detail["dependency_kind"], PRODUCTION)
        self.assertEqual(priority.detail["workspaces"], ["agent", "interop"])
        self.assertEqual(priority.detail["probabilities"]["production_reachable"], 0.91)
        self.assertEqual(urgency.answer, "high")
        self.assertEqual(urgency.route, route_for(0.58))
        self.assertEqual(urgency.route, "review")
        self.assertEqual(urgency.detail["score"], 1.62)
        self.assertEqual(urgency.detail["legend"], {"0": "low", "1": "medium", "2": "high"})

    def test_findings_survive_the_report_serialisation(self) -> None:
        advisory = grouped()[PASTE_ADVISORY]
        report = Report(
            check=CHECK_NAME,
            revision="fixture",
            model=DEFAULT_MODEL,
            findings=findings_for(advisory, self.evaluation()),
        )
        document = json.loads(report.to_json())
        self.assertEqual(document["check"], CHECK_NAME)
        self.assertEqual(len(document["findings"]), 2)
        self.assertEqual(
            {item["anchor"] for item in document["findings"]}, {"agent/Cargo.lock:101"}
        )
        self.assertIn(PASTE_ADVISORY, report.to_markdown())


class RegistrationTests(unittest.TestCase):
    def test_check_is_registered_under_deps(self) -> None:
        self.assertIs(CHECKS[CHECK_NAME], CHECK)
        self.assertEqual(CHECK_NAME, "deps")
        self.assertTrue(callable(CHECK.add_arguments))
        self.assertTrue(callable(CHECK.run))


class ArgumentTests(unittest.TestCase):
    def test_input_pairs_need_a_workspace(self) -> None:
        self.assertEqual(input_pair("out.jsonl=agent"), (Path("out.jsonl"), "agent"))
        for raw in ("out.jsonl", "=agent", "out.jsonl="):
            with self.assertRaises(argparse.ArgumentTypeError):
                input_pair(raw)

    def test_run_is_off_by_default_and_only_inputs_are_read(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            args = parsed_args(
                [
                    "--out",
                    directory,
                    "--input",
                    f"{INTEROP_CAPTURE}=interop",
                    "--repo-root",
                    str(ROOT),
                ]
            )
            self.assertFalse(args.run)
            self.assertIsNone(args.workspace)
            sources = collect_sources(args, ROOT)
        self.assertEqual(len(sources), 1)
        self.assertEqual(sources[0][0], "interop")
        self.assertEqual(len(parse_diagnostics(sources[0][1], "interop")), 2)


class DryRunTests(unittest.TestCase):
    def test_dry_run_exits_zero_and_leaves_the_repository_alone(self) -> None:
        before = deny_digests()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            code = cli.main(
                [
                    CHECK_NAME,
                    "--out",
                    str(out_dir),
                    "--input",
                    f"{AGENT_CAPTURE}=agent",
                    "--input",
                    f"{INTEROP_CAPTURE}=interop",
                    "--repo-root",
                    str(ROOT),
                    "--dry-run",
                ]
            )
            self.assertEqual(code, 0)
            self.assertFalse(out_dir.exists())
        self.assertEqual(deny_digests(), before)


class LiveDepsTests(unittest.TestCase):
    def test_live_triage_of_the_captured_advisories(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        before = deny_digests()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            args = parsed_args(
                [
                    "--out",
                    str(out_dir),
                    "--input",
                    f"{AGENT_CAPTURE}=agent",
                    "--input",
                    f"{INTEROP_CAPTURE}=interop",
                    "--repo-root",
                    str(ROOT),
                ]
            )
            client = JevClient(log_path=Path(directory) / "calls" / "jev.log")
            report = CHECK.run(args, client)
            self.assertEqual(report.check, CHECK_NAME)
            self.assertEqual(report.calls, 2)
            self.assertEqual(len(report.findings), 4)
            self.assertTrue(report.model.startswith(client.model))
            self.assertGreater(report.input_tokens, 0)
            self.assertGreaterEqual(report.cost_usd, 0.0)
            subjects = {finding.subject for finding in report.findings}
            self.assertEqual(
                subjects, {f"{PASTE_ADVISORY} paste", f"{RUSTLS_ADVISORY} rustls"}
            )
            for finding in report.findings:
                self.assertIn(finding.route, ("auto", "review", "escalate"))
                self.assertGreaterEqual(finding.confidence, 0.0)
                self.assertLessEqual(finding.confidence, 1.0)
                self.assertRegex(finding.anchor, r"^(agent|interop)/Cargo\.lock:\d+$")
                if finding.question == PRIORITY_QUESTION:
                    self.assertIn(finding.answer, PRIORITY_OPTIONS)
                else:
                    self.assertIn(finding.answer, URGENCY_LABELS)
            json_path, md_path = report.write(out_dir)
            self.assertEqual(
                len(json.loads(json_path.read_text(encoding="utf-8"))["findings"]), 4
            )
            self.assertIn(PASTE_ADVISORY, md_path.read_text(encoding="utf-8"))
            lines = (Path(directory) / "calls" / "jev.log").read_text(
                encoding="utf-8"
            ).splitlines()
            self.assertEqual(len(lines), 2)
        self.assertEqual(deny_digests(), before)


if __name__ == "__main__":
    unittest.main()
