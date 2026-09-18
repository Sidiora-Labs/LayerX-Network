from __future__ import annotations

import contextlib
import io
import json
import os
import tempfile
import unittest
from pathlib import Path
from typing import Mapping
from unittest import mock

from tools.jev.checks import CHECKS
from tools.jev.checks.ledger import (
    CHECK,
    CHECK_NAME,
    CLOSURE_QUESTION,
    PAIR_QUESTION,
    PAIR_THRESHOLD,
    ROOT,
    SEVERITY_QUESTION,
    SEVERITY_RUBRIC,
    Record,
    candidate_pairs,
    cluster,
    duplicate_findings,
    judgeable,
    load_records,
    pair_questions,
    pair_state,
    parse_ledger,
    select,
    severity_findings,
    severity_questions,
    shingles,
    tally,
)
from tools.jev.cli import main
from tools.jev.client import API_KEY_ENV, DEFAULT_MODEL, Evaluation, JevClient, parse_evaluation
from tools.jev.report import Report


LEDGER = "spec/layerx-beta/qualification.kvx"
USAGE = {"input_tokens": 512, "output_tokens": 0, "cost": 0.0000215}
AGREEING_SEVERITY = {
    "type": "choice",
    "choice": "note",
    "probabilities": {"blocker": 0.02, "suspect": 0.01, "assumption": 0.01, "note": 0.96},
    "confidence": 0.96,
}
DISAGREEING_SEVERITY = {
    "type": "choice",
    "choice": "blocker",
    "probabilities": {"blocker": 0.72, "suspect": 0.2, "assumption": 0.05, "note": 0.03},
    "confidence": 0.72,
}
EXCERPT = r"""[observation.2.4.2]
task = "2.4"
file = "human/apps/web/src/explorer/mirror-server.ts"
symbol = "npm run lint"
observed = "npx eslint --max-warnings=0 \"src/**/*.{ts,tsx}\" \"copy/**/*.ts\" in human/apps/web exits 1 with 8 errors in src/api/index.ts:23, src/auth/session.ts:15, src/explorer/mirror-server.ts:49 (four restrict-template-expressions), src/journeys/agents/shell.ts:6 and src/journeys/home/model.ts:108. git diff --stat main over those five files is empty, so they are byte-identical to main 15267435. tsc --noEmit, node policy/check.mjs and eslint over the files this task touched all pass."
assumption = "Pre-existing lint failures outside this task's touches; left unmodified. The task verify_cmd (next build and node tests) does not run eslint, so the failures do not gate this task, but npm run lint fails on main until their owning tasks repair them."
severity = "suspect"

[observation.8.4.1]
task = "8.4"
file = "agent/crates/layerx-types/build.rs include/layerx/lxp_result.h agent/crates/layerx-types/src/result.rs"
symbol = "protocol_result_code_parity"
observed = "At revision 27c783ac893e004eed3dbd04bd2295f8badbe31d the command make test-programs-oracle-read && make programs-abi-drift exited 2 in the programs-build prerequisite with 'protocol result-code parity failure: result code 38 drifted: header declares AuthThresholdUnmet = -220, src/result.rs declares SequenceGap = -300'; log /root/lx-target/programs/task-8-4-v3-verify.log."
assumption = "All three files are byte-identical to origin/main and the drift arrives from merged commit bf5056244, which added LXP_ERR_AUTH_THRESHOLD_UNMET/DUPLICATE_SIGNER/NOT_MATURE to the C header without mirroring them in result.rs, so the fix belongs to the authority-kinds task; the ABI half of this gate, make programs-abi-drift, exits 0 on its own at this revision."
resolution = "Resolved on this branch by commit 4ec6f185f, a cherry-pick of PR 332 that mirrors AuthThresholdUnmet, AuthDuplicateSigner and AuthNotMature into agent/crates/layerx-types/src/result.rs; at revision 4ec6f185f the full verify_cmd exited 0."
severity = "note"

[observation.8.1.1]
task = "8.1"
file = "programs/crates/layerx-programs-registry/tests/lxt721_reference.rs"
symbol = "lxt721::reference_interface"
observed = "At revision 5b1c6c3b0b07f0f59a9990dc73091ce743b866d0 the command cg spec start 8.1 exited 1 with '12 task(s) already in_progress (limit 12)', so the task verify_cmd cd programs && cargo test --locked -p layerx-programs-registry --test lxt721_reference never ran; log /root/lx-target/wave8/spec-done-8.1.log."
assumption = "The twelve slots are held by pre-existing in_progress tasks 3.7 and 6.1-6.11 from earlier waves, and the close-out brief forbids --force, so this gate needs the owner to release those claims or raise spec/workflow.kvx max_in_progress before it can be rerun."
resolution = "The owner cleared the twelve stale in_progress slots with cg spec reconcile --repair, so this gate was rerun at revision 3686a7ce909ef7f60aff4b225b523030241de90b: cg spec done 8.1 exited 0 and the task is done; log /root/lx-target/wave8/spec-done-8.1.log."
severity = "blocker"

[gate.5.1.1]
task = "5.1"
reqs = ["12.2","12.4","12.5"]
revision = "e68e3cf8b7144318a34b475c3843b0a8ba38053e"
command = "make build layerxd layerx-genesis-build"
environment = "vmi3050659 x86_64; bash: GNU bash, version 5.2.21(1)-release (x86_64-pc-linux-gnu); make: GNU Make 4.3; cc: cc (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0; gcc-13: gcc-13 (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0; clang-18: unavailable; rustc: rustc 1.91.1 (ed61e7d7e 2025-11-07); cargo: cargo 1.91.1 (ea2d97820 2025-10-10); node: v22.23.2; npm: 10.9.8; python3: Python 3.12.3; forge: forge Version: 1.7.1; CARGO_BUILD_JOBS=4; RAYON_NUM_THREADS=4"
started_at = "2026-09-07T12:23:31Z"
outcome = "pass"
evidence = "spec/layerx-beta/evidence/e68e3cf8b7144318a34b475c3843b0a8ba38053e/focused/01-build.log"
note = "Tracked working-tree changes present at runner start; this is development evidence, not immutable release qualification."
"""


def records() -> list[Record]:
    return parse_ledger(EXCERPT, LEDGER)


def built(identifier: str, **fields: str) -> Record:
    values = {
        "task": "1.1",
        "file": "",
        "symbol": "",
        "observed": "",
        "assumption": "",
        "resolution": "",
        "severity": "note",
    }
    values.update(fields)
    return Record(identifier=identifier, path=LEDGER, line=1, **values)


def evaluated(answers: Mapping[str, object]) -> Evaluation:
    body = {
        "model": "typesafe/jev-1.13-20260917",
        "answers": dict(answers),
        "usage": dict(USAGE),
        "id": "gen-dec-1789715517-ledger",
    }
    return parse_evaluation(body, DEFAULT_MODEL, "0" * 64, "1" * 64)


class ParserTests(unittest.TestCase):
    def test_every_observation_block_becomes_a_record(self) -> None:
        parsed = records()
        self.assertEqual([record.identifier for record in parsed], ["2.4.2", "8.4.1", "8.1.1"])
        self.assertEqual([record.line for record in parsed], [1, 9, 18])
        self.assertEqual([record.task for record in parsed], ["2.4", "8.4", "8.1"])
        self.assertEqual([record.severity for record in parsed], ["suspect", "note", "blocker"])
        self.assertEqual(parsed[2].symbol, "lxt721::reference_interface")

    def test_gate_blocks_are_not_observations(self) -> None:
        self.assertIn("[gate.5.1.1]", EXCERPT)
        self.assertNotIn("5.1.1", [record.identifier for record in records()])

    def test_escaped_quotes_are_decoded(self) -> None:
        observed = records()[0].observed
        self.assertTrue(
            observed.startswith(
                'npx eslint --max-warnings=0 "src/**/*.{ts,tsx}" "copy/**/*.ts" in human/apps/web'
            )
        )
        self.assertNotIn("\\", observed)

    def test_resolution_is_optional(self) -> None:
        parsed = records()
        self.assertEqual(parsed[0].resolution, "")
        self.assertTrue(
            parsed[1].resolution.startswith("Resolved on this branch by commit 4ec6f185f")
        )

    def test_anchor_is_the_ledger_file_and_line(self) -> None:
        self.assertEqual(records()[1].anchor(), f"{LEDGER}:9")

    def test_state_carries_only_observed_assumption_and_resolution(self) -> None:
        parsed = records()
        self.assertEqual(set(parsed[0].state()), {"observed", "assumption"})
        self.assertEqual(set(parsed[1].state()), {"observed", "assumption", "resolution"})
        payload = json.dumps(parsed[1].state())
        self.assertNotIn(parsed[1].symbol, payload)
        self.assertNotIn(parsed[1].file, payload)

    def test_the_real_beta_ledger_parses_into_the_severity_vocabulary(self) -> None:
        parsed = parse_ledger((ROOT / LEDGER).read_text(encoding="utf-8"), LEDGER)
        self.assertGreater(len(parsed), 1700)
        self.assertEqual({record.severity for record in parsed}, set(SEVERITY_RUBRIC))
        self.assertTrue(all(record.observed for record in parsed))
        self.assertTrue(all(record.anchor().startswith(f"{LEDGER}:") for record in parsed))

    def test_records_without_observed_text_are_not_judged(self) -> None:
        parsed = load_records(sorted(ROOT.glob("spec/*/qualification.kvx")))
        self.assertTrue(any(not record.observed for record in parsed))
        self.assertLess(len(judgeable(parsed)), len(parsed))
        self.assertTrue(all(record.observed for record in judgeable(parsed)))


class SelectTests(unittest.TestCase):
    def test_zero_or_oversized_sample_keeps_every_record(self) -> None:
        parsed = records()
        self.assertEqual(select(parsed, 0, 1), parsed)
        self.assertEqual(select(parsed, 9, 1), parsed)

    def test_sample_is_seeded_and_keeps_ledger_order(self) -> None:
        parsed = records()
        picked = select(parsed, 2, 7)
        self.assertEqual(len(picked), 2)
        self.assertEqual(picked, select(parsed, 2, 7))
        self.assertEqual(
            [record.line for record in picked], sorted(record.line for record in picked)
        )
        self.assertTrue(set(picked) <= set(parsed))


class CandidatePairTests(unittest.TestCase):
    def test_shingles_are_four_word_windows(self) -> None:
        self.assertEqual(
            shingles("one two three four five"),
            frozenset({("one", "two", "three", "four"), ("two", "three", "four", "five")}),
        )
        self.assertEqual(shingles("one, two! three?"), frozenset())

    def test_same_file_is_a_candidate(self) -> None:
        pairs = candidate_pairs(
            [
                built("a", file="src/one.c", symbol="alpha", observed="the first body"),
                built("b", file="src/one.c", symbol="beta", observed="a different body"),
            ]
        )
        self.assertEqual(pairs, [(0, 1)])

    def test_same_symbol_is_a_candidate(self) -> None:
        pairs = candidate_pairs(
            [
                built("a", file="src/one.c", symbol="alpha", observed="the first body"),
                built("b", file="src/two.c", symbol="alpha", observed="a different body"),
            ]
        )
        self.assertEqual(pairs, [(0, 1)])

    def test_six_shared_shingles_are_a_candidate(self) -> None:
        shared = "alpha beta gamma delta epsilon zeta eta theta iota"
        pairs = candidate_pairs(
            [
                built("a", file="src/one.c", symbol="alpha", observed=f"{shared} left tail"),
                built("b", file="src/two.c", symbol="beta", observed=f"{shared} right tail"),
            ]
        )
        self.assertEqual(pairs, [(0, 1)])

    def test_five_shared_shingles_are_not_a_candidate(self) -> None:
        shared = "alpha beta gamma delta epsilon zeta eta theta"
        pairs = candidate_pairs(
            [
                built("a", file="src/one.c", symbol="alpha", observed=f"{shared} left tail"),
                built("b", file="src/two.c", symbol="beta", observed=f"{shared} right tail"),
            ]
        )
        self.assertEqual(pairs, [])

    def test_max_pairs_caps_the_candidate_list(self) -> None:
        many = [
            built(str(index), file="src/one.c", observed=f"body number {index}")
            for index in range(5)
        ]
        self.assertEqual(len(candidate_pairs(many)), 10)
        self.assertEqual(candidate_pairs(many, 3), [(0, 1), (0, 2), (0, 3)])
        self.assertEqual(candidate_pairs(many, 0), [])

    def test_unrelated_ledger_records_are_not_candidates(self) -> None:
        self.assertEqual(candidate_pairs(records()), [])


class ClusterTests(unittest.TestCase):
    def test_transitive_edges_merge_into_one_cluster(self) -> None:
        self.assertEqual(cluster(6, [(0, 1), (1, 2), (3, 4)]), [[0, 1, 2], [3, 4]])

    def test_edges_in_any_order_reach_the_same_clusters(self) -> None:
        self.assertEqual(cluster(4, [(2, 3), (0, 3), (0, 1)]), [[0, 1, 2, 3]])

    def test_singletons_are_dropped(self) -> None:
        self.assertEqual(cluster(3, []), [])
        self.assertEqual(cluster(3, [(0, 2)]), [[0, 2]])


class QuestionTests(unittest.TestCase):
    def test_severity_question_uses_the_four_level_rubric(self) -> None:
        questions = severity_questions(records()[0])
        self.assertEqual(set(questions), {SEVERITY_QUESTION})
        question = questions[SEVERITY_QUESTION]
        self.assertEqual(question["type"], "choice")
        self.assertEqual(question["criteria"], dict(SEVERITY_RUBRIC))
        self.assertEqual(
            set(SEVERITY_RUBRIC), {"blocker", "suspect", "assumption", "note"}
        )

    def test_closure_question_appears_only_with_a_resolution(self) -> None:
        questions = severity_questions(records()[1])
        self.assertEqual(set(questions), {SEVERITY_QUESTION, CLOSURE_QUESTION})
        self.assertEqual(questions[CLOSURE_QUESTION]["type"], "noul")
        self.assertIn("fully closes", questions[CLOSURE_QUESTION]["instructions"])

    def test_pair_state_sends_only_the_three_record_fields(self) -> None:
        parsed = records()
        state = pair_state(parsed[0], parsed[1])
        self.assertEqual(set(state), {"first", "second"})
        self.assertEqual(set(state["first"]), {"observed", "assumption"})
        self.assertEqual(set(state["second"]), {"observed", "assumption", "resolution"})
        self.assertEqual(set(pair_questions()), {PAIR_QUESTION})


class SeverityFindingTests(unittest.TestCase):
    def test_disagreement_and_open_resolution_both_become_findings(self) -> None:
        subject = records()[1]
        result = severity_findings(
            subject,
            evaluated(
                {
                    SEVERITY_QUESTION: DISAGREEING_SEVERITY,
                    CLOSURE_QUESTION: {"type": "noul", "noul": 0.21},
                }
            ),
        )
        self.assertEqual(
            [finding.question for finding in result], [SEVERITY_QUESTION, CLOSURE_QUESTION]
        )
        severity, closure = result
        self.assertEqual(severity.check, CHECK_NAME)
        self.assertEqual(severity.subject, "8.4.1")
        self.assertEqual(severity.anchor, f"{LEDGER}:9")
        self.assertEqual(severity.answer, "blocker")
        self.assertAlmostEqual(severity.confidence, 0.72)
        self.assertEqual(severity.route, "review")
        self.assertEqual(severity.detail["recorded"], "note")
        self.assertEqual(severity.detail["proposed"], "blocker")
        self.assertAlmostEqual(severity.detail["probabilities"]["blocker"], 0.72)
        self.assertEqual(severity.detail["task"], "8.4")
        self.assertEqual(closure.check, CHECK_NAME)
        self.assertEqual(closure.anchor, f"{LEDGER}:9")
        self.assertAlmostEqual(closure.answer, 0.21)
        self.assertAlmostEqual(closure.confidence, 0.79)
        self.assertEqual(closure.route, "review")
        self.assertAlmostEqual(closure.detail["closes"], 0.21)
        self.assertAlmostEqual(closure.detail["threshold"], 0.5)

    def test_agreeing_severity_and_closed_resolution_emit_nothing(self) -> None:
        result = severity_findings(
            records()[1],
            evaluated(
                {
                    SEVERITY_QUESTION: AGREEING_SEVERITY,
                    CLOSURE_QUESTION: {"type": "noul", "noul": 0.93},
                }
            ),
        )
        self.assertEqual(result, [])

    def test_the_closure_threshold_is_exclusive(self) -> None:
        result = severity_findings(
            records()[1],
            evaluated(
                {
                    SEVERITY_QUESTION: AGREEING_SEVERITY,
                    CLOSURE_QUESTION: {"type": "noul", "noul": 0.5},
                }
            ),
        )
        self.assertEqual(result, [])

    def test_a_record_without_a_resolution_is_judged_on_severity_alone(self) -> None:
        subject = records()[0]
        result = severity_findings(subject, evaluated({SEVERITY_QUESTION: AGREEING_SEVERITY}))
        self.assertEqual([finding.question for finding in result], [SEVERITY_QUESTION])
        self.assertEqual(result[0].detail["recorded"], "suspect")
        self.assertEqual(result[0].answer, "note")
        self.assertEqual(result[0].route, "auto")

    def test_usage_is_tallied_into_the_report(self) -> None:
        report = Report(check=CHECK_NAME, revision="8506f3e17", model=DEFAULT_MODEL)
        tally(report, evaluated({SEVERITY_QUESTION: AGREEING_SEVERITY}))
        tally(report, evaluated({SEVERITY_QUESTION: AGREEING_SEVERITY}))
        self.assertEqual(report.calls, 2)
        self.assertEqual(report.input_tokens, 1024)
        self.assertAlmostEqual(report.cost_usd, 0.000043)


class DuplicateFindingTests(unittest.TestCase):
    def test_a_cluster_becomes_one_finding_carrying_its_members(self) -> None:
        parsed = records()
        result = duplicate_findings(parsed, {(0, 1): 0.91, (1, 2): 0.84, (0, 2): 0.40})
        self.assertEqual(len(result), 1)
        finding = result[0]
        self.assertEqual(finding.check, CHECK_NAME)
        self.assertEqual(finding.question, PAIR_QUESTION)
        self.assertEqual(finding.subject, "2.4.2, 8.4.1, 8.1.1")
        self.assertEqual(finding.anchor, f"{LEDGER}:1")
        self.assertAlmostEqual(finding.answer, 3.0)
        self.assertAlmostEqual(finding.confidence, 0.84)
        self.assertEqual(finding.route, "review")
        self.assertEqual(finding.detail["members"], ["2.4.2", "8.4.1", "8.1.1"])
        self.assertEqual(finding.detail["severities"], ["suspect", "note", "blocker"])
        self.assertEqual(
            finding.detail["anchors"], [f"{LEDGER}:1", f"{LEDGER}:9", f"{LEDGER}:18"]
        )
        self.assertEqual(set(finding.detail["pairs"]), {"2.4.2|8.4.1", "8.4.1|8.1.1"})

    def test_two_clusters_stay_separate(self) -> None:
        parsed = records() + [built("9.9.9", file="src/nine.c", observed="ninth body")]
        result = duplicate_findings(parsed, {(0, 1): 0.95, (2, 3): 0.88})
        self.assertEqual(
            [finding.subject for finding in result], ["2.4.2, 8.4.1", "8.1.1, 9.9.9"]
        )

    def test_pairs_below_the_threshold_do_not_cluster(self) -> None:
        pairs = {(0, 1): PAIR_THRESHOLD - 0.01, (1, 2): 0.1}
        self.assertEqual(duplicate_findings(records(), pairs), [])


class RegistrationTests(unittest.TestCase):
    def test_the_check_is_registered_under_its_report_name(self) -> None:
        self.assertEqual(CHECK_NAME, "ledger")
        self.assertIs(CHECKS[CHECK_NAME], CHECK)
        self.assertTrue(callable(CHECK.add_arguments))
        self.assertTrue(callable(CHECK.run))


class CliDryRunTests(unittest.TestCase):
    def ledger_file(self, directory: str) -> Path:
        path = Path(directory) / "qualification.kvx"
        path.write_text(EXCERPT, encoding="utf-8")
        return path

    def test_dry_run_exits_zero_without_reaching_the_api(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            stderr = io.StringIO()
            with mock.patch.dict(os.environ, {API_KEY_ENV: ""}):
                with contextlib.redirect_stderr(stderr):
                    code = main(
                        [
                            "ledger",
                            "--out",
                            str(out_dir),
                            "--dry-run",
                            "--kvx",
                            str(self.ledger_file(directory)),
                        ]
                    )
            self.assertEqual(code, 0)
            self.assertIn("dry run", stderr.getvalue())
            self.assertIn("were not sent", stderr.getvalue())
            self.assertFalse(out_dir.exists())

    def test_dedup_only_dry_run_makes_no_call_at_all(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            stdout = io.StringIO()
            with mock.patch.dict(os.environ, {API_KEY_ENV: ""}):
                with contextlib.redirect_stdout(stdout):
                    code = main(
                        [
                            "ledger",
                            "--out",
                            str(out_dir),
                            "--dry-run",
                            "--dedup-only",
                            "--kvx",
                            str(self.ledger_file(directory)),
                        ]
                    )
            self.assertEqual(code, 0)
            document = json.loads((out_dir / "ledger.json").read_text(encoding="utf-8"))
            self.assertEqual(document["check"], "ledger")
            self.assertEqual(document["calls"], 0)
            self.assertEqual(document["cost_usd"], 0.0)
            self.assertEqual(document["findings"], [])
            self.assertIn("ledger.md", stdout.getvalue())


class LiveLedgerTests(unittest.TestCase):
    def test_live_severity_call_answers_inside_the_rubric(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        subject = records()[1]
        client = JevClient()
        result = client.evaluate(subject.state(), severity_questions(subject))
        self.assertEqual(set(result.answers), {SEVERITY_QUESTION, CLOSURE_QUESTION})
        severity = result.answers[SEVERITY_QUESTION]
        self.assertEqual(severity.kind, "choice")
        self.assertIn(severity.value, SEVERITY_RUBRIC)
        self.assertAlmostEqual(sum(severity.probabilities.values()), 1.0, places=2)
        closure = result.answers[CLOSURE_QUESTION]
        self.assertEqual(closure.kind, "noul")
        self.assertGreaterEqual(closure.value, 0.0)
        self.assertLessEqual(closure.value, 1.0)
        report = Report(check=CHECK_NAME, revision="live", model=client.model)
        tally(report, result)
        self.assertEqual(report.calls, 1)
        self.assertGreater(report.input_tokens, 0)
        for finding in severity_findings(subject, result):
            self.assertEqual(finding.check, CHECK_NAME)
            self.assertEqual(finding.anchor, f"{LEDGER}:9")
            self.assertIn(finding.question, {SEVERITY_QUESTION, CLOSURE_QUESTION})

    def test_live_pair_call_answers_the_root_cause_noul(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        parsed = records()
        client = JevClient()
        result = client.evaluate(pair_state(parsed[1], parsed[2]), pair_questions())
        self.assertEqual(set(result.answers), {PAIR_QUESTION})
        answer = result.answers[PAIR_QUESTION]
        self.assertEqual(answer.kind, "noul")
        self.assertGreaterEqual(answer.value, 0.0)
        self.assertLessEqual(answer.value, 1.0)


if __name__ == "__main__":
    unittest.main()
