from __future__ import annotations

import contextlib
import io
import json
import os
import tempfile
import unittest
from pathlib import Path

from tools.jev.checks.failures import (
    CATEGORIES,
    CHECK,
    MERGE_BLOCK_LABELS,
    MESSAGE_LIMIT,
    anchor_for,
    candidate_pairs,
    cluster_findings,
    clusters,
    collect,
    extract_cargo,
    extract_conformance,
    extract_ctest,
    extract_forge,
    extract_fuzz,
    extract_pytest,
    merge_label,
    normalise,
    redact,
    sniff,
)
from tools.jev.cli import main
from tools.jev.client import API_KEY_ENV, Answer, parse_answer


FIXTURES = Path(__file__).resolve().parent / "fixtures" / "failures"
CARGO = FIXTURES / "cargo.log"
CTEST = FIXTURES / "ctest.log"
FORGE = FIXTURES / "forge.log"
PYTEST = FIXTURES / "pytest.log"
CONFORMANCE = FIXTURES / "conformance.json"
FUZZ = FIXTURES / "fuzz.log"
HEX_DUMP = "9f2c4b7d1e08a3560b7c2d9e4f108a3b"


def text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def choice_answer(value: str, confidence: float) -> Answer:
    return parse_answer(
        "category",
        {
            "type": "choice",
            "choice": value,
            "confidence": confidence,
            "probabilities": {value: confidence},
        },
    )


def score_answer(value: float, confidence: float) -> Answer:
    return parse_answer(
        "merge_block_confidence",
        {
            "type": "score",
            "score": value,
            "confidence": confidence,
            "probabilities": {"0": 0.05, "1": 0.15, "2": 0.8},
            "legend": {"0": "unlikely", "1": "possible", "2": "likely"},
        },
    )


class CargoExtractionTests(unittest.TestCase):
    def test_stdout_blocks_yield_name_location_and_message(self) -> None:
        failures = extract_cargo(text(CARGO))
        self.assertEqual(
            [failure.name for failure in failures],
            [
                "delivery::tests::replayed_delivery_is_a_duplicate",
                "delivery::tests::conflicting_digest_is_rejected",
                "signature::tests::rejects_unauthorised_sequencer",
            ],
        )
        self.assertEqual(
            [failure.location for failure in failures],
            [
                "platform/webhooks/src/delivery.rs:214:9",
                "platform/webhooks/src/delivery.rs:271:9",
                "platform/webhooks/src/signature.rs:97:5",
            ],
        )
        self.assertEqual(
            failures[0].message,
            "assertion `left == right` failed: a replayed delivery must release exactly once "
            "left: 2 right: 1",
        )
        self.assertEqual(
            failures[2].message,
            "assertion failed: verify_header(&header, &authority).is_err()",
        )

    def test_backtrace_notes_and_summary_lines_stay_out_of_the_message(self) -> None:
        for failure in extract_cargo(text(CARGO)):
            self.assertNotIn("RUST_BACKTRACE", failure.message)
            self.assertNotIn("test result:", failure.message)
            self.assertNotIn("panicked at", failure.message)


class HarnessExtractionTests(unittest.TestCase):
    def test_assertion_lines_attach_to_the_following_fail_line(self) -> None:
        failures = extract_ctest(text(CTEST))
        self.assertEqual(
            [failure.name for failure in failures],
            [
                "authority.multisig-resolution",
                "authority.timelock-resolution",
                "codec.vector-roundtrip",
            ],
        )
        self.assertEqual(failures[0].location, "tests/test_authority_multisig.c:214")
        self.assertEqual(failures[0].message, "expected=3 produced=4")
        self.assertEqual(failures[1].location, "tests/test_authority_multisig.c:388")
        self.assertEqual(
            failures[1].message,
            "expected=LXP_RESULT_AUTHORITY_TIMELOCK(19) produced=LXP_RESULT_OK(0)",
        )
        self.assertEqual(failures[2].location, "tests/codec/lxp_test_codec_vectors.c:141")

    def test_a_passing_test_does_not_inherit_an_earlier_assertion(self) -> None:
        failures = extract_ctest(
            "tests/ledger/test_send.c:44 expected=1 produced=0\n"
            "PASS ledger.send-debits-the-sender\n"
            "FAIL ledger.send-rejects-overdraft\n"
        )
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0].location, "")
        self.assertEqual(failures[0].message, "")


class ForgeExtractionTests(unittest.TestCase):
    def test_both_fail_syntaxes_carry_reason_and_suite_path(self) -> None:
        failures = extract_forge(text(FORGE))
        self.assertEqual(len(failures), 4)
        self.assertEqual(
            [failure.name for failure in failures[:2]],
            ["testRejectsStaleCheckpointRoot()", "testCheckpointIndexAdvances()"],
        )
        self.assertEqual(failures[0].message, "revert: stale-root")
        self.assertEqual(failures[1].message, "assertion failed: 3 != 4")
        self.assertEqual(
            failures[0].location,
            "interop/contracts/ethereum-mirror/test/MirrorCheckpoint.t.sol",
        )


class PytestExtractionTests(unittest.TestCase):
    def test_short_summary_lines_split_path_name_and_message(self) -> None:
        failures = extract_pytest(text(PYTEST))
        self.assertEqual(
            [failure.name for failure in failures],
            [
                "test_genesis_allocations_match_the_manifest",
                "test_missing_artifact_is_rejected",
            ],
        )
        self.assertEqual(failures[0].location, "tests/test_beta_withdrawal_genesis.py")
        self.assertEqual(failures[1].message, "KeyError: 'receipt'")


class ConformanceExtractionTests(unittest.TestCase):
    def test_only_failed_check_results_become_records(self) -> None:
        failures = extract_conformance(text(CONFORMANCE))
        self.assertEqual(
            [failure.name for failure in failures],
            [
                "seller: a receipt signed by an unauthorised sequencer is rejected",
                "webhook: a valid signature is processed exactly once and replays are duplicates",
            ],
        )
        self.assertEqual(failures[1].message, "fulfillment-conflict")
        self.assertEqual(failures[0].location, "")

    def test_a_results_object_and_json_lines_are_both_accepted(self) -> None:
        document = json.dumps({"results": [{"name": "a", "ok": False, "detail": "boom"}]})
        self.assertEqual(extract_conformance(document)[0].message, "boom")
        lines = '{"name": "a", "ok": true}\n{"name": "b", "ok": false, "detail": "boom"}\n'
        self.assertEqual([failure.name for failure in extract_conformance(lines)], ["b"])


class FuzzExtractionTests(unittest.TestCase):
    def test_divergence_and_sanitizer_reports_are_both_records(self) -> None:
        failures = extract_fuzz(text(FUZZ))
        self.assertEqual(
            [failure.name for failure in failures],
            ["non-deterministic Execution outcome", "AddressSanitizer: heap-buffer-overflow"],
        )
        self.assertEqual(
            failures[0].location, "programs/fuzz/corpus/execution/seed-0007.hex"
        )
        self.assertEqual(failures[1].location, "src/codec/lxp_codec_primitives.c:118:12")
        self.assertIn("heap-buffer-overflow on address", failures[1].message)


class SniffTests(unittest.TestCase):
    def test_every_fixture_is_recognised_without_a_format_flag(self) -> None:
        self.assertEqual(sniff(text(CARGO)), "cargo")
        self.assertEqual(sniff(text(CTEST)), "ctest")
        self.assertEqual(sniff(text(FORGE)), "forge")
        self.assertEqual(sniff(text(PYTEST)), "pytest")
        self.assertEqual(sniff(text(CONFORMANCE)), "conformance")
        self.assertEqual(sniff(text(FUZZ)), "fuzz")


class RedactionTests(unittest.TestCase):
    def test_hex_dumps_and_addresses_never_survive(self) -> None:
        self.assertEqual(
            redact(f"expected={HEX_DUMP} produced=00000000000000000000000000000000"),
            "expected=<hex> produced=<hex>",
        )
        self.assertEqual(
            redact("heap-buffer-overflow on address 0x60300000ef18 at pc 0x0000004f21ba"),
            "heap-buffer-overflow on address <hex> at pc <hex>",
        )
        self.assertEqual(redact("left: 2\n right: 1"), "left: 2 right: 1")

    def test_messages_are_truncated_before_they_leave_the_process(self) -> None:
        self.assertEqual(len(redact("boundary " * 200)), MESSAGE_LIMIT)

    def test_collect_redacts_and_truncates_every_record(self) -> None:
        records = collect([CTEST], "auto")
        self.assertEqual(records[2].message, "expected=<hex> produced=<hex>")
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "long.log"
            log.write_text(
                "FAILED tests/test_long.py::test_wide - AssertionError: " + "x" * 900 + "\n",
                encoding="utf-8",
            )
            oversized = collect([log], "pytest")
        self.assertEqual(len(oversized[0].message), MESSAGE_LIMIT)


class NormalisationTests(unittest.TestCase):
    def test_numbers_hex_and_paths_collapse(self) -> None:
        self.assertEqual(
            normalise("expected=3 produced=4 at platform/webhooks/src/delivery.rs:214:9"),
            "expected=<n> produced=<n> at <path>",
        )
        self.assertEqual(normalise(f"digest {HEX_DUMP} mismatch"), "digest <hex> mismatch")

    def test_two_failures_differing_only_in_numbers_normalise_equal(self) -> None:
        records = collect([CARGO], "auto")
        self.assertNotEqual(records[0].message, records[1].message)
        self.assertEqual(normalise(records[0].message), normalise(records[1].message))
        self.assertNotEqual(normalise(records[0].message), normalise(records[2].message))


class CollectTests(unittest.TestCase):
    def test_records_carry_identifier_format_and_source(self) -> None:
        records = collect([CARGO], "auto")
        self.assertEqual(
            [record.identifier for record in records],
            ["cargo-001", "cargo-002", "cargo-003"],
        )
        self.assertEqual({record.format for record in records}, {"cargo"})
        self.assertEqual({record.source for record in records}, {str(CARGO)})

    def test_a_repeated_forge_failure_is_collected_once(self) -> None:
        self.assertEqual(len(extract_forge(text(FORGE))), 4)
        records = collect([FORGE], "auto")
        self.assertEqual(
            [record.name for record in records],
            ["testRejectsStaleCheckpointRoot()", "testCheckpointIndexAdvances()"],
        )

    def test_an_explicit_format_overrides_the_sniffer(self) -> None:
        self.assertEqual(collect([CARGO], "pytest"), [])

    def test_several_logs_share_one_identifier_sequence(self) -> None:
        records = collect([CONFORMANCE, FUZZ], "auto")
        self.assertEqual(
            [record.identifier for record in records],
            ["conformance-001", "conformance-002", "fuzz-003", "fuzz-004"],
        )


class AnchorTests(unittest.TestCase):
    def test_a_file_and_line_location_is_the_anchor(self) -> None:
        records = collect([CARGO], "auto")
        self.assertEqual(anchor_for(records[0]), "platform/webhooks/src/delivery.rs:214:9")

    def test_a_location_without_a_line_falls_back_to_the_record_id(self) -> None:
        records = collect([PYTEST], "auto")
        self.assertEqual(anchor_for(records[0]), "pytest-001")
        self.assertEqual(anchor_for(collect([CONFORMANCE], "auto")[0]), "conformance-001")


class CandidatePairTests(unittest.TestCase):
    def test_a_shared_file_or_a_shared_normalised_message_is_a_candidate(self) -> None:
        self.assertEqual(candidate_pairs(collect([CARGO], "auto")), [("cargo-001", "cargo-002")])
        self.assertEqual(candidate_pairs(collect([CTEST], "auto")), [("ctest-001", "ctest-002")])

    def test_unrelated_failures_are_never_paired(self) -> None:
        self.assertEqual(candidate_pairs(collect([CONFORMANCE], "auto")), [])
        self.assertEqual(candidate_pairs(collect([FUZZ], "auto")), [])


class ClusterTests(unittest.TestCase):
    def test_a_link_at_the_threshold_merges_and_a_weak_link_does_not(self) -> None:
        records = collect([CARGO], "auto")
        self.assertEqual(
            clusters(records, {("cargo-001", "cargo-002"): 0.8}),
            [["cargo-001", "cargo-002"], ["cargo-003"]],
        )
        self.assertEqual(
            clusters(records, {("cargo-001", "cargo-002"): 0.79}),
            [["cargo-001"], ["cargo-002"], ["cargo-003"]],
        )

    def test_clustering_is_transitive(self) -> None:
        records = collect([CTEST], "auto")
        self.assertEqual(
            clusters(
                records,
                {("ctest-001", "ctest-002"): 0.91, ("ctest-002", "ctest-003"): 0.86},
            ),
            [["ctest-001", "ctest-002", "ctest-003"]],
        )

    def test_no_links_leaves_every_failure_on_its_own(self) -> None:
        records = collect([CARGO], "auto")
        self.assertEqual(
            clusters(records, {}), [["cargo-001"], ["cargo-002"], ["cargo-003"]]
        )


class FindingTests(unittest.TestCase):
    def test_a_cluster_reports_the_representative_with_its_members(self) -> None:
        records = collect([CARGO], "auto")
        index = {record.identifier: record for record in records}
        answers = {
            "category::cargo-001": choice_answer("logic_regression", 0.93),
            "merge_block_confidence::cargo-001": score_answer(1.8, 0.77),
        }
        links = {("cargo-001", "cargo-002"): 0.91}
        findings = cluster_findings(index, ["cargo-001", "cargo-002"], answers, links)
        self.assertEqual(
            [finding.question for finding in findings],
            ["category", "merge_block_confidence"],
        )
        category, blocking = findings
        self.assertEqual(category.check, CHECK)
        self.assertEqual(category.subject, "delivery::tests::replayed_delivery_is_a_duplicate")
        self.assertEqual(category.anchor, "platform/webhooks/src/delivery.rs:214:9")
        self.assertEqual(category.answer, "logic_regression")
        self.assertEqual(category.route, "auto")
        self.assertEqual(category.detail["members"], ["cargo-001", "cargo-002"])
        self.assertEqual(category.detail["size"], 2)
        self.assertEqual(category.detail["log"], str(CARGO))
        self.assertEqual(category.detail["links"], {"cargo-001~cargo-002": 0.91})
        self.assertEqual(
            category.detail["names"],
            [
                "delivery::tests::replayed_delivery_is_a_duplicate",
                "delivery::tests::conflicting_digest_is_rejected",
            ],
        )
        self.assertEqual(blocking.answer, "likely")
        self.assertEqual(blocking.confidence, 0.77)
        self.assertEqual(blocking.route, "review")
        self.assertEqual(blocking.detail["score"], 1.8)

    def test_a_missing_answer_drops_only_its_own_finding(self) -> None:
        records = collect([PYTEST], "auto")
        index = {record.identifier: record for record in records}
        answers = {"category::pytest-001": choice_answer("fixture_or_vector_drift", 0.44)}
        findings = cluster_findings(index, ["pytest-001"], answers, {})
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].anchor, "pytest-001")
        self.assertEqual(findings[0].route, "escalate")
        self.assertEqual(findings[0].detail["members"], ["pytest-001"])

    def test_a_representative_with_an_anchor_leads_its_cluster(self) -> None:
        records = collect([FUZZ], "auto")
        index = {record.identifier: record for record in records}
        answers = {"category::fuzz-002": choice_answer("arithmetic_or_bound", 0.88)}
        findings = cluster_findings(index, ["fuzz-001", "fuzz-002"], answers, {})
        self.assertEqual(findings[0].anchor, "src/codec/lxp_codec_primitives.c:118:12")
        self.assertEqual(findings[0].subject, "AddressSanitizer: heap-buffer-overflow")

    def test_score_values_map_onto_the_declared_levels(self) -> None:
        self.assertEqual(merge_label(score_answer(0.1, 0.9)), "unlikely")
        self.assertEqual(merge_label(score_answer(1.2, 0.9)), "possible")
        self.assertEqual(merge_label(score_answer(1.6, 0.9)), "likely")


class DryRunTests(unittest.TestCase):
    def test_dry_run_exits_zero_and_sends_nothing(self) -> None:
        stderr = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            with contextlib.redirect_stderr(stderr):
                code = main(
                    [
                        "failures",
                        "--log-file",
                        str(CTEST),
                        "--out",
                        str(out_dir),
                        "--dry-run",
                    ]
                )
            self.assertEqual(code, 0)
            self.assertFalse(out_dir.exists())
        self.assertIn("dry run", stderr.getvalue())

    def test_an_empty_log_produces_a_report_without_findings(self) -> None:
        stdout = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "clean.log"
            log.write_text(
                "running 3 tests\ntest result: ok. 3 passed; 0 failed\n", encoding="utf-8"
            )
            out_dir = Path(directory) / "reports"
            with contextlib.redirect_stdout(stdout):
                code = main(
                    ["failures", "--log-file", str(log), "--out", str(out_dir), "--dry-run"]
                )
            self.assertEqual(code, 0)
            document = json.loads((out_dir / "failures.json").read_text(encoding="utf-8"))
        self.assertEqual(document["check"], "failures")
        self.assertEqual(document["findings"], [])
        self.assertEqual(document["calls"], 0)


class LiveFailureTriageTests(unittest.TestCase):
    def test_live_triage_clusters_two_failures_from_one_file(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        stdout = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            with contextlib.redirect_stdout(stdout):
                code = main(["failures", "--log-file", str(CTEST), "--out", str(out_dir)])
            self.assertEqual(code, 0)
            raw = (out_dir / "failures.json").read_text(encoding="utf-8")
            markdown = (out_dir / "failures.md").read_text(encoding="utf-8")
        document = json.loads(raw)
        self.assertEqual(document["check"], "failures")
        self.assertEqual(document["calls"], 2)
        self.assertGreater(document["input_tokens"], 0)
        self.assertNotIn(HEX_DUMP, raw)
        self.assertNotIn(HEX_DUMP, markdown)
        findings = document["findings"]
        self.assertTrue(findings)
        subjects = {
            "authority.multisig-resolution",
            "authority.timelock-resolution",
            "codec.vector-roundtrip",
        }
        for finding in findings:
            self.assertEqual(finding["check"], "failures")
            self.assertIn(finding["subject"], subjects)
            self.assertIn(finding["route"], ("auto", "review", "escalate"))
            self.assertTrue(finding["anchor"])
            self.assertTrue(finding["detail"]["members"])
            if finding["question"] == "category":
                self.assertIn(finding["answer"], CATEGORIES)
            else:
                self.assertEqual(finding["question"], "merge_block_confidence")
                self.assertIn(finding["answer"], MERGE_BLOCK_LABELS)


if __name__ == "__main__":
    unittest.main()
