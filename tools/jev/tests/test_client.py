from __future__ import annotations

import hashlib
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.jev.client import (
    API_KEY_ENV,
    DEFAULT_ENDPOINT,
    DEFAULT_MODEL,
    ENDPOINT_ENV,
    MAX_CHOICE_OPTIONS,
    MODEL_ENV,
    DryRunClient,
    JevClient,
    JevConfigError,
    JevDryRun,
    JevRequestError,
    batched,
    canonical_request,
    choice,
    noul,
    parse_evaluation,
    score,
)


FIXTURE = Path(__file__).resolve().parent / "fixtures" / "decision_response.json"
PINNED_DIGEST = "da74618a332dad0e9f438f85f74f5f9de7b0d9ef053d8a5bf2fc0ac98d2149b7"
DEPARTMENTS = {
    "sales": "Wants to buy something",
    "billing": "Payment, invoice or card problem",
    "technical": "Software or device problem",
}


def fixture_document() -> dict[str, object]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


class ParseEvaluationTests(unittest.TestCase):
    def test_captured_response_yields_all_three_answer_kinds(self) -> None:
        raw = FIXTURE.read_bytes()
        evaluation = parse_evaluation(
            fixture_document(),
            DEFAULT_MODEL,
            PINNED_DIGEST,
            hashlib.sha256(raw).hexdigest(),
        )
        self.assertEqual(evaluation.model, "typesafe/jev-1.13-20260917")
        self.assertEqual(evaluation.requested_model, DEFAULT_MODEL)
        self.assertEqual(evaluation.request_id, "gen-dec-1789715517-F1dv7b9hjaZx7efS8tmu")
        self.assertEqual(evaluation.request_sha256, PINNED_DIGEST)
        self.assertEqual(evaluation.response_sha256, hashlib.sha256(raw).hexdigest())
        self.assertEqual(evaluation.usage["input_tokens"], 427.0)
        self.assertEqual(evaluation.usage["output_tokens"], 73.0)
        self.assertAlmostEqual(evaluation.usage["cost"], 0.000017934)
        self.assertEqual(set(evaluation.answers), {"is_urgent", "department", "frustration"})

        urgent = evaluation.answers["is_urgent"]
        self.assertEqual(urgent.kind, "noul")
        self.assertEqual(urgent.value, 0.95)
        self.assertAlmostEqual(urgent.confidence, 0.95)
        self.assertAlmostEqual(urgent.probabilities["true"], 0.95)
        self.assertAlmostEqual(urgent.probabilities["false"], 0.05)
        self.assertEqual(urgent.legend, {})

        department = evaluation.answers["department"]
        self.assertEqual(department.kind, "choice")
        self.assertEqual(department.value, "billing")
        self.assertEqual(department.confidence, 0.82)
        self.assertEqual(
            department.probabilities, {"sales": 0.0, "billing": 0.88, "technical": 0.12}
        )

        frustration = evaluation.answers["frustration"]
        self.assertEqual(frustration.kind, "score")
        self.assertEqual(frustration.value, 1.06)
        self.assertEqual(frustration.confidence, 0.91)
        self.assertEqual(
            frustration.legend, {"0": "Calm", "1": "Frustrated", "2": "Very angry"}
        )
        self.assertEqual(frustration.probabilities, {"0": 0.0, "1": 0.94, "2": 0.06})

    def test_noul_confidence_measures_distance_from_undecided(self) -> None:
        document = fixture_document()
        document["answers"]["is_urgent"]["noul"] = 0.05
        evaluation = parse_evaluation(document, DEFAULT_MODEL, PINNED_DIGEST, PINNED_DIGEST)
        urgent = evaluation.answers["is_urgent"]
        self.assertEqual(urgent.value, 0.05)
        self.assertAlmostEqual(urgent.confidence, 0.95)
        self.assertAlmostEqual(urgent.probabilities["false"], 0.95)

    def test_unsupported_answer_type_is_a_request_error(self) -> None:
        document = fixture_document()
        document["answers"]["is_urgent"]["type"] = "mystery"
        with self.assertRaises(JevRequestError) as raised:
            parse_evaluation(document, DEFAULT_MODEL, PINNED_DIGEST, PINNED_DIGEST)
        self.assertEqual(raised.exception.status, 0)
        self.assertIn("is_urgent", raised.exception.body_excerpt)


class QuestionBuilderTests(unittest.TestCase):
    def test_builders_emit_the_wire_shape(self) -> None:
        self.assertEqual(
            noul("The customer needs help immediately."),
            {"type": "noul", "instructions": "The customer needs help immediately."},
        )
        self.assertEqual(
            noul("Urgent.", {"true": "Money is blocked", "false": "Can wait"}),
            {
                "type": "noul",
                "instructions": "Urgent.",
                "criteria": {"true": "Money is blocked", "false": "Can wait"},
            },
        )
        self.assertEqual(
            choice("Which team should handle this message?", DEPARTMENTS),
            {
                "type": "choice",
                "instructions": "Which team should handle this message?",
                "criteria": DEPARTMENTS,
            },
        )
        self.assertEqual(
            score("Rate the frustration.", ["Calm", "Frustrated", "Very angry"]),
            {
                "type": "score",
                "instructions": "Rate the frustration.",
                "criteria": ["Calm", "Frustrated", "Very angry"],
            },
        )

    def test_choice_rejects_bad_cardinality(self) -> None:
        with self.assertRaises(ValueError):
            choice("Pick one.", {"only": "the single option"})
        with self.assertRaises(ValueError):
            choice(
                "Pick one.",
                {f"option-{index}": "described" for index in range(MAX_CHOICE_OPTIONS + 1)},
            )
        widest = choice(
            "Pick one.",
            {f"option-{index}": "described" for index in range(MAX_CHOICE_OPTIONS)},
        )
        self.assertEqual(len(widest["criteria"]), MAX_CHOICE_OPTIONS)

    def test_score_requires_at_least_two_levels(self) -> None:
        with self.assertRaises(ValueError):
            score("Rate it.", ["Calm"])
        with self.assertRaises(ValueError):
            score("Rate it.", [])


class CanonicalRequestTests(unittest.TestCase):
    def test_digest_is_stable_across_insertion_order(self) -> None:
        first = canonical_request(
            DEFAULT_MODEL,
            {"message": "card declined", "account": "acct-1"},
            {
                "is_urgent": noul("Urgent."),
                "department": choice("Which team?", DEPARTMENTS),
            },
        )
        second = canonical_request(
            DEFAULT_MODEL,
            {"account": "acct-1", "message": "card declined"},
            {
                "department": choice("Which team?", dict(reversed(list(DEPARTMENTS.items())))),
                "is_urgent": noul("Urgent."),
            },
        )
        self.assertEqual(first, second)
        self.assertEqual(first[1], hashlib.sha256(first[0]).hexdigest())
        self.assertEqual(json.loads(first[0].decode("utf-8"))["model"], DEFAULT_MODEL)

    def test_digest_matches_the_pinned_canonical_form(self) -> None:
        payload, digest = canonical_request(DEFAULT_MODEL, "ok", {"q": noul("Fine.")})
        self.assertEqual(
            payload.decode("utf-8"),
            '{"model":"typesafe/jev-1.13","questions":{"q":'
            '{"instructions":"Fine.","type":"noul"}},"state":"ok"}',
        )
        self.assertEqual(digest, PINNED_DIGEST)


class ClientConfigurationTests(unittest.TestCase):
    def test_environment_supplies_endpoint_model_and_key(self) -> None:
        environment = {
            ENDPOINT_ENV: "https://gateway.example/decisions",
            MODEL_ENV: "typesafe/jev-1.13-20260917",
            API_KEY_ENV: "test-key",
        }
        with mock.patch.dict(os.environ, environment, clear=True):
            client = JevClient()
            self.assertEqual(client.endpoint, "https://gateway.example/decisions")
            self.assertEqual(client.model, "typesafe/jev-1.13-20260917")
            self.assertEqual(client.resolved_api_key(), "test-key")
        with mock.patch.dict(os.environ, {}, clear=True):
            fallback = JevClient()
            self.assertEqual(fallback.endpoint, DEFAULT_ENDPOINT)
            self.assertEqual(fallback.model, DEFAULT_MODEL)

    def test_missing_api_key_fails_at_call_time(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=True):
            client = JevClient()
            with self.assertRaises(JevConfigError):
                client.evaluate({"message": "card declined"}, {"is_urgent": noul("Urgent.")})

    def test_dry_run_client_sends_nothing(self) -> None:
        with mock.patch.dict(os.environ, {API_KEY_ENV: "test-key"}, clear=True):
            client = DryRunClient()
            with self.assertRaises(JevDryRun) as raised:
                client.evaluate("ok", {"q": noul("Fine.")})
        self.assertIn(PINNED_DIGEST, str(raised.exception))


class BatchedTests(unittest.TestCase):
    def test_batches_cover_every_item(self) -> None:
        self.assertEqual(list(batched(range(7), 3)), [[0, 1, 2], [3, 4, 5], [6]])
        self.assertEqual(list(batched([], 3)), [])
        self.assertEqual(list(batched("abc", 1)), [["a"], ["b"], ["c"]])

    def test_batch_size_must_be_positive(self) -> None:
        with self.assertRaises(ValueError):
            list(batched(range(3), 0))


class LiveEvaluationTests(unittest.TestCase):
    def test_live_roundtrip(self) -> None:
        if not os.environ.get(API_KEY_ENV):
            self.skipTest(f"{API_KEY_ENV} is not set")
        state = {"message": "My card was declined three times and nobody answers the phone."}
        questions = {
            "is_urgent": noul("The customer needs help immediately."),
            "department": choice("Which team should handle this message?", DEPARTMENTS),
        }
        with tempfile.TemporaryDirectory() as directory:
            log_path = Path(directory) / "calls" / "jev.log"
            client = JevClient(log_path=log_path)
            evaluation = client.evaluate(state, questions)
            self.assertEqual(
                evaluation.request_sha256, canonical_request(client.model, state, questions)[1]
            )
            self.assertEqual(evaluation.requested_model, client.model)
            self.assertTrue(evaluation.model.startswith(client.model))
            self.assertTrue(evaluation.request_id)
            self.assertEqual(set(evaluation.answers), {"is_urgent", "department"})
            urgent = evaluation.answers["is_urgent"]
            self.assertEqual(urgent.kind, "noul")
            self.assertIsInstance(urgent.value, float)
            self.assertGreaterEqual(urgent.confidence, 0.5)
            self.assertLessEqual(urgent.confidence, 1.0)
            department = evaluation.answers["department"]
            self.assertEqual(department.kind, "choice")
            self.assertIn(department.value, DEPARTMENTS)
            self.assertAlmostEqual(sum(department.probabilities.values()), 1.0, places=2)
            self.assertGreater(evaluation.usage["input_tokens"], 0.0)
            lines = log_path.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(lines), 1)
            entry = json.loads(lines[0])
            self.assertEqual(
                set(entry),
                {
                    "ts",
                    "requested_model",
                    "model",
                    "request_sha256",
                    "response_sha256",
                    "usage",
                    "request_id",
                },
            )
            self.assertEqual(entry["request_sha256"], evaluation.request_sha256)
            self.assertEqual(entry["response_sha256"], evaluation.response_sha256)
            self.assertEqual(entry["request_id"], evaluation.request_id)
            self.assertNotIn("declined", lines[0])


if __name__ == "__main__":
    unittest.main()
