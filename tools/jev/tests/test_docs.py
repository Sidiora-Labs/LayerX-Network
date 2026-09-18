from __future__ import annotations

import contextlib
import io
import os
import tempfile
import unittest
from pathlib import Path

from tools.jev.checks import CHECKS
from tools.jev.checks.docs import (
    CHECK_NAME,
    DRIFT_KIND,
    DRIFT_KINDS,
    EXTRA_SECTION,
    FAITHFUL,
    MISSING_SECTION,
    PAIRING_QUESTION,
    PREAMBLE_TITLE,
    QUESTIONS,
    SAME_CLAIMS,
    Pair,
    add_arguments,
    pair_findings,
    pair_sections,
    relative,
    sanitize,
    split_sections,
    unpaired_findings,
)
from tools.jev.cli import build_parser, main
from tools.jev.client import API_KEY_ENV, DEFAULT_MODEL, Evaluation, JevClient, parse_evaluation


ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / "README.md"
GERMAN = ROOT / "docs" / "readme" / "README.de.md"
ENGLISH_TITLES = [
    PREAMBLE_TITLE,
    "What LayerX Network is",
    "Try the testnet",
    "Build from source",
    "Repository layout",
    "Documentation",
    "SDKs and integrations",
    "Contributing",
    "Security",
    "License",
]
GERMAN_TITLES = [
    PREAMBLE_TITLE,
    "Was LayerX Network ist",
    "Testnet ausprobieren",
    "Aus dem Quellcode bauen",
    "Repository-Aufbau",
    "Dokumentation",
    "SDKs und Integrationen",
    "Mitwirken",
    "Sicherheit",
    "Lizenz",
]
FENCED = "\n".join(
    [
        "Intro prose.",
        "",
        "## Real heading",
        "",
        "```sh",
        "# not a heading",
        "make build",
        "```",
        "",
        "Trailing prose.",
        "",
        "## Second heading",
        "",
        "Body.",
    ]
)


def truncated_german() -> str:
    return "\n".join(GERMAN.read_text(encoding="utf-8").splitlines()[:112])


def extended_german() -> str:
    body = GERMAN.read_text(encoding="utf-8").rstrip("\n")
    return body + "\n\n## Zusatzabschnitt\n\nDieser Abschnitt steht nur auf Deutsch.\n"


def evaluation_for(same: float, drift: str, confidence: float) -> Evaluation:
    others = (1.0 - confidence) / float(len(DRIFT_KINDS) - 1)
    document = {
        "id": "gen-dec-docs",
        "model": "typesafe/jev-1.13-20260917",
        "answers": {
            SAME_CLAIMS: {"type": "noul", "noul": same},
            DRIFT_KIND: {
                "type": "choice",
                "choice": drift,
                "confidence": confidence,
                "probabilities": {
                    kind: confidence if kind == drift else others for kind in DRIFT_KINDS
                },
            },
        },
        "usage": {"input_tokens": 512, "output_tokens": 24, "cost": 0.0000215},
    }
    return parse_evaluation(document, DEFAULT_MODEL, "a" * 64, "b" * 64)


class SectionSplitterTests(unittest.TestCase):
    def test_real_readme_splits_into_a_preamble_and_every_heading(self) -> None:
        sections = split_sections(SOURCE)
        self.assertEqual([section.title for section in sections], ENGLISH_TITLES)
        self.assertEqual([section.index for section in sections], list(range(len(sections))))
        self.assertEqual([section.level for section in sections], [0] + [2] * 9)
        self.assertEqual(sections[0].line, 1)
        lines = SOURCE.read_text(encoding="utf-8").splitlines()
        for section in sections[1:]:
            self.assertEqual(lines[section.line - 1], f"## {section.title}")

    def test_bodies_hold_the_content_below_each_heading(self) -> None:
        sections = {section.title: section for section in split_sections(SOURCE)}
        self.assertIn("Apache License, Version 2.0", sections["License"].body)
        self.assertIn("layerx key create quickstart", sections["Try the testnet"].body)
        self.assertIn("`rust-toolchain.toml`", sections["Build from source"].body)
        self.assertIn("<h1 align=\"center\">LayerX Network</h1>", sections[PREAMBLE_TITLE].body)
        for section in sections.values():
            self.assertFalse(section.body.startswith("#"))
            self.assertNotIn("\n## ", section.body)

    def test_headings_inside_code_fences_are_not_sections(self) -> None:
        sections = split_sections(SOURCE, FENCED)
        self.assertEqual(
            [section.title for section in sections],
            [PREAMBLE_TITLE, "Real heading", "Second heading"],
        )
        self.assertEqual([section.line for section in sections], [1, 3, 12])
        self.assertIn("# not a heading", sections[1].body)
        self.assertEqual(sanitize(sections[1].body), "Trailing prose.")


class SanitizeTests(unittest.TestCase):
    def test_preamble_loses_images_badges_and_urls(self) -> None:
        preamble = sanitize(split_sections(SOURCE)[0].body)
        self.assertNotIn("https://", preamble)
        self.assertNotIn("<img", preamble)
        self.assertNotIn("![", preamble)
        self.assertNotIn("shields.io", preamble)
        self.assertIn("LayerX Network", preamble)
        self.assertIn("Deutsch", preamble)
        self.assertIn("deterministic execution and accounting network", preamble)

    def test_code_fences_are_dropped_and_prose_survives(self) -> None:
        sections = {section.title: section for section in split_sections(SOURCE)}
        testnet = sanitize(sections["Try the testnet"].body)
        self.assertNotIn("layerx key create quickstart", testnet)
        self.assertNotIn("curl", testnet)
        self.assertNotIn("```", testnet)
        self.assertIn("The cluster path is", testnet)
        self.assertIn("`docs/wiki/Quickstart.md`", testnet)
        self.assertNotIn("(docs/wiki/Quickstart.md)", testnet)

    def test_tables_survive_without_link_targets(self) -> None:
        sections = {section.title: section for section in split_sections(SOURCE)}
        documentation = sanitize(sections["Documentation"].body)
        self.assertIn("`docs/wiki/Home.md`", documentation)
        self.assertNotIn("(docs/wiki/Home.md)", documentation)
        self.assertNotIn("\n\n\n", documentation)


class PairingTests(unittest.TestCase):
    def test_real_german_readme_pairs_section_for_section(self) -> None:
        source = split_sections(SOURCE)
        translation = split_sections(GERMAN)
        self.assertEqual([section.title for section in translation], GERMAN_TITLES)
        pairs, unpaired_source, unpaired_translation = pair_sections(source, translation)
        self.assertEqual(len(pairs), len(ENGLISH_TITLES))
        self.assertEqual(unpaired_source, [])
        self.assertEqual(unpaired_translation, [])
        self.assertEqual(
            [(pair.source.title, pair.translation.title) for pair in pairs],
            list(zip(ENGLISH_TITLES, GERMAN_TITLES)),
        )
        self.assertEqual([pair.index for pair in pairs], list(range(len(pairs))))
        for pair in pairs:
            self.assertEqual(pair.index, pair.source.index)
            self.assertEqual(pair.index, pair.translation.index)

    def test_dropped_translation_sections_are_code_level_findings(self) -> None:
        source = split_sections(SOURCE)
        translation = split_sections(GERMAN, truncated_german())
        pairs, unpaired_source, unpaired_translation = pair_sections(source, translation)
        self.assertEqual(len(pairs), len(translation))
        self.assertEqual(unpaired_translation, [])
        self.assertEqual(
            [section.title for section in unpaired_source],
            ENGLISH_TITLES[len(translation) :],
        )
        findings = unpaired_findings(unpaired_source, unpaired_translation, GERMAN)
        self.assertEqual(len(findings), len(unpaired_source))
        source_lines = SOURCE.read_text(encoding="utf-8").splitlines()
        for finding, section in zip(findings, unpaired_source):
            self.assertEqual(finding.check, CHECK_NAME)
            self.assertEqual(finding.question, PAIRING_QUESTION)
            self.assertEqual(finding.answer, MISSING_SECTION)
            self.assertEqual(finding.confidence, 1.0)
            self.assertEqual(finding.route, "auto")
            self.assertEqual(finding.anchor, f"README.md:{section.line}")
            self.assertEqual(source_lines[section.line - 1], f"## {section.title}")
            self.assertEqual(finding.subject, f"docs/readme/README.de.md {section.title}")
            self.assertEqual(finding.detail["index"], section.index)
            self.assertEqual(finding.detail["translation"], "docs/readme/README.de.md")

    def test_added_translation_sections_are_code_level_findings(self) -> None:
        text = extended_german()
        source = split_sections(SOURCE)
        translation = split_sections(GERMAN, text)
        pairs, unpaired_source, unpaired_translation = pair_sections(source, translation)
        self.assertEqual(len(pairs), len(source))
        self.assertEqual(unpaired_source, [])
        self.assertEqual([section.title for section in unpaired_translation], ["Zusatzabschnitt"])
        findings = unpaired_findings(unpaired_source, unpaired_translation, GERMAN)
        self.assertEqual(len(findings), 1)
        extra = findings[0]
        section = unpaired_translation[0]
        self.assertEqual(extra.answer, EXTRA_SECTION)
        self.assertEqual(extra.confidence, 1.0)
        self.assertEqual(extra.route, "auto")
        self.assertEqual(extra.anchor, f"docs/readme/README.de.md:{section.line}")
        self.assertEqual(text.splitlines()[section.line - 1], "## Zusatzabschnitt")
        self.assertEqual(extra.subject, "docs/readme/README.de.md Zusatzabschnitt")
        self.assertEqual(extra.detail["translation_title"], "Zusatzabschnitt")


class QuestionTests(unittest.TestCase):
    def test_questions_ask_one_noul_and_one_drift_choice(self) -> None:
        self.assertEqual(set(QUESTIONS), {SAME_CLAIMS, DRIFT_KIND})
        self.assertEqual(QUESTIONS[SAME_CLAIMS]["type"], "noul")
        self.assertEqual(
            QUESTIONS[SAME_CLAIMS]["instructions"],
            "the translated section states the same claims as the source "
            "with nothing added or dropped",
        )
        self.assertEqual(QUESTIONS[DRIFT_KIND]["type"], "choice")
        self.assertEqual(
            set(QUESTIONS[DRIFT_KIND]["criteria"]),
            {FAITHFUL, "missing_content", "extra_content", "stale_reference", "mistranslated"},
        )
        for description in QUESTIONS[DRIFT_KIND]["criteria"].values():
            self.assertTrue(description.strip())

    def test_check_registers_itself_with_readme_defaults(self) -> None:
        self.assertIn(CHECK_NAME, CHECKS)
        parser = build_parser(CHECK_NAME)
        add_arguments(parser)
        args = parser.parse_args(["--out", "reports"])
        self.assertEqual(args.source, "README.md")
        self.assertEqual(args.translations, "docs/readme/README.*.md")
        chosen = parser.parse_args(["--out", "reports", "--source", "docs/readme/README.fr.md"])
        self.assertEqual(chosen.source, "docs/readme/README.fr.md")


class PairFindingTests(unittest.TestCase):
    def pair(self) -> Pair:
        source = split_sections(SOURCE)
        translation = split_sections(GERMAN)
        return pair_sections(source, translation)[0][5]

    def test_drift_answers_become_routed_findings(self) -> None:
        pair = self.pair()
        findings = pair_findings(pair, evaluation_for(0.12, "missing_content", 0.93))
        self.assertEqual([finding.question for finding in findings], [SAME_CLAIMS, DRIFT_KIND])
        claims, drift = findings
        self.assertAlmostEqual(claims.answer, 0.12)
        self.assertAlmostEqual(claims.confidence, 0.88)
        self.assertEqual(claims.route, "review")
        self.assertEqual(claims.detail[DRIFT_KIND], "missing_content")
        self.assertEqual(drift.answer, "missing_content")
        self.assertAlmostEqual(drift.confidence, 0.93)
        self.assertEqual(drift.route, "auto")
        self.assertEqual(drift.check, CHECK_NAME)
        self.assertEqual(drift.subject, f"docs/readme/README.de.md {pair.translation.title}")
        self.assertEqual(drift.anchor, f"docs/readme/README.de.md:{pair.translation.line}")
        self.assertEqual(drift.detail["source_anchor"], f"README.md:{pair.source.line}")
        self.assertEqual(drift.detail["index"], pair.index)
        self.assertAlmostEqual(drift.detail["probabilities"]["missing_content"], 0.93)

    def test_low_confidence_drift_escalates(self) -> None:
        findings = pair_findings(self.pair(), evaluation_for(0.44, "stale_reference", 0.31))
        self.assertEqual([finding.route for finding in findings], ["review", "escalate"])
        self.assertEqual(findings[1].answer, "stale_reference")
        self.assertAlmostEqual(findings[0].confidence, 0.56)

    def test_faithful_answers_produce_no_findings(self) -> None:
        self.assertEqual(pair_findings(self.pair(), evaluation_for(0.97, FAITHFUL, 0.95)), [])

    def test_disagreement_still_reports_the_low_probability_claim(self) -> None:
        findings = pair_findings(self.pair(), evaluation_for(0.2, FAITHFUL, 0.6))
        self.assertEqual([finding.question for finding in findings], [SAME_CLAIMS])
        self.assertEqual(findings[0].detail[DRIFT_KIND], FAITHFUL)


class CliTests(unittest.TestCase):
    def test_dry_run_exits_zero_and_writes_nothing(self) -> None:
        stderr = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            out_dir = Path(directory) / "reports"
            with contextlib.redirect_stderr(stderr):
                code = main([CHECK_NAME, "--out", str(out_dir), "--dry-run"])
            self.assertEqual(code, 0)
            self.assertFalse(out_dir.exists())
        self.assertIn("dry run", stderr.getvalue())
        self.assertIn("2 questions", stderr.getvalue())

    def test_dry_run_of_a_single_translation_exits_zero(self) -> None:
        stderr = io.StringIO()
        with tempfile.TemporaryDirectory() as directory:
            with contextlib.redirect_stderr(stderr):
                code = main(
                    [
                        CHECK_NAME,
                        "--out",
                        directory,
                        "--source",
                        "README.md",
                        "--translations",
                        "docs/readme/README.de.md",
                        "--dry-run",
                    ]
                )
            self.assertEqual(code, 0)
            self.assertEqual(os.listdir(directory), [])

    def test_relative_paths_are_reported_against_the_repository_root(self) -> None:
        self.assertEqual(relative(SOURCE), "README.md")
        self.assertEqual(relative(GERMAN), "docs/readme/README.de.md")


class LiveDocsTests(unittest.TestCase):
    def test_live_pair_evaluation_answers_both_questions(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        pairs = pair_sections(split_sections(SOURCE), split_sections(GERMAN))[0]
        pair = next(pair for pair in pairs if pair.source.title == "Security")
        state = {
            "section": pair.source.title,
            "source": sanitize(pair.source.body),
            "translation": sanitize(pair.translation.body),
        }
        evaluation = JevClient().evaluate(state, QUESTIONS)
        self.assertEqual(set(evaluation.answers), {SAME_CLAIMS, DRIFT_KIND})
        same = evaluation.answers[SAME_CLAIMS]
        self.assertEqual(same.kind, "noul")
        self.assertGreaterEqual(float(same.value), 0.0)
        self.assertLessEqual(float(same.value), 1.0)
        drift = evaluation.answers[DRIFT_KIND]
        self.assertEqual(drift.kind, "choice")
        self.assertIn(drift.value, DRIFT_KINDS)
        self.assertAlmostEqual(sum(drift.probabilities.values()), 1.0, places=2)
        self.assertGreater(evaluation.usage["input_tokens"], 0.0)
        for finding in pair_findings(pair, evaluation):
            self.assertEqual(finding.check, CHECK_NAME)
            self.assertEqual(finding.anchor, f"docs/readme/README.de.md:{pair.translation.line}")


if __name__ == "__main__":
    unittest.main()
