from __future__ import annotations

import json
import threading
import unittest
from dataclasses import replace
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from layerx_sdk import (
    HumanIntentClient,
    HumanIntentError,
    IdempotencyKey,
    IntentConstraints,
    IntentEndpointRef,
    IntentLegBinding,
    IntentMoney,
    PlanIntentRequest,
    PlatformSdkError,
    SdkErrorCode,
    SubmitPlanRequest,
    decode_human_envelope,
    decode_intent_plan,
    decode_intent_submission,
    encode_plan_intent_request,
    encode_submit_plan_request,
)

FIXTURE = json.loads(
    (Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures/intent-plan-v1.json").read_text()
)
REFUSED_ASSET = "ee" * 32
SUBMIT_KEY = "b1946ac92492d2347c6235b4d2611184"
SUBMIT_TRACE = "trc_01j2gxq4dpc2d3e4f5g6h7j8k9"
REFUSAL_TRACE = "trc_01j2gxq3cnb1c2d3e4f5g6h7j8"
SUBMISSION = {
    "journey_id": "jrn_01j2gx3fam9kq4vte8n5w6y7z8",
    "plan_digest": "4b" * 32,
    "state": "processing",
    "state_copy_key": "status.processing",
}

PLAN_REQUEST = PlanIntentRequest(
    IntentEndpointRef("paxeer-wallet", None),
    IntentEndpointRef("agent", "agent:did:layerx:bob:main"),
    "99" * 32,
    IntentMoney(500000, "LXP"),
    IntentConstraints("2026-08-18T09:35:00Z", IntentMoney(250, "LXP"), False),
)
SUBMIT_REQUEST = SubmitPlanRequest(
    "4b" * 32,
    "7c" * 32,
    (
        IntentLegBinding(0, "a1" * 32, "did:layerx:alice", "custody-key", "self", 7, 1755500000, 1755503600, IntentMoney(125, "LXP")),
        IntentLegBinding(1, "b2" * 32, "did:layerx:alice", "agent-authority", "self", 8, 1755500000, 1755503600, IntentMoney(125, "LXP")),
    ),
)


def intent_handler(observed: list[dict[str, object]]) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            length = int(self.headers["Content-Length"])
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            observed.append({
                "path": self.path,
                "idempotency": self.headers["Idempotency-Key"],
                "content_type": self.headers["Content-Type"],
                "body": body,
            })
            if self.path == "/v1/intents/plan":
                if body.get("asset_id") == REFUSED_ASSET:
                    status = 422
                    envelope: dict[str, object] = {
                        "ok": False,
                        "error": {"code": "refused-by-protocol", "copy_key": "error.intent.no-route", "retry": "final"},
                        "trace": REFUSAL_TRACE,
                    }
                else:
                    status = 200
                    envelope = {"ok": True, "result": FIXTURE["plan_result"], "trace": FIXTURE["plan_trace"]}
            else:
                status = 200
                envelope = {"ok": True, "result": SUBMISSION, "trace": SUBMIT_TRACE}
            encoded = json.dumps(envelope, separators=(",", ":")).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, _format: str, *args: object) -> None:
            del args

    return Handler


class HumanIntentBindingTest(unittest.TestCase):
    def test_plan_request_encoding_matches_shared_fixture(self) -> None:
        self.assertEqual(FIXTURE["name"], "intent-plan-v1")
        self.assertIsInstance(FIXTURE["provenance"], str)
        self.assertEqual(encode_plan_intent_request(PLAN_REQUEST), FIXTURE["plan_request"])
        self.assertNotIn("account", FIXTURE["plan_request"]["source"])
        self.assertEqual(encode_submit_plan_request(SUBMIT_REQUEST), {
            "plan_digest": "4b" * 32,
            "signed_digest": "7c" * 32,
            "bindings": [
                {
                    "leg_index": 0,
                    "action_key": "a1" * 32,
                    "actor": "did:layerx:alice",
                    "authority": "custody-key",
                    "relationship": "self",
                    "account_sequence": 7,
                    "not_before": 1755500000,
                    "not_after": 1755503600,
                    "fee_limit": {"amount": "125", "currency": "LXP"},
                },
                {
                    "leg_index": 1,
                    "action_key": "b2" * 32,
                    "actor": "did:layerx:alice",
                    "authority": "agent-authority",
                    "relationship": "self",
                    "account_sequence": 8,
                    "not_before": 1755500000,
                    "not_after": 1755503600,
                    "fee_limit": {"amount": "125", "currency": "LXP"},
                },
            ],
        })

    def test_encoding_refuses_malformed_intent_arguments(self) -> None:
        for request in (
            replace(PLAN_REQUEST, source=IntentEndpointRef("paxeer-wallet", "agent:did:layerx:alice:main")),
            replace(PLAN_REQUEST, destination=IntentEndpointRef("agent", None)),
            replace(PLAN_REQUEST, asset_id="AB" * 32),
            replace(PLAN_REQUEST, money=IntentMoney(-1, "LXP")),
            replace(PLAN_REQUEST, constraints=IntentConstraints("2026-08-18 09:35:00", IntentMoney(250, "LXP"), False)),
        ):
            with self.assertRaises(PlatformSdkError):
                encode_plan_intent_request(request)
        for submission in (
            replace(SUBMIT_REQUEST, bindings=()),
            replace(SUBMIT_REQUEST, bindings=(SUBMIT_REQUEST.bindings[0], SUBMIT_REQUEST.bindings[0])),
            replace(SUBMIT_REQUEST, bindings=(replace(SUBMIT_REQUEST.bindings[0], not_after=-1),)),
            replace(SUBMIT_REQUEST, plan_digest="4b" * 31),
        ):
            with self.assertRaises(PlatformSdkError):
                encode_submit_plan_request(submission)

    def test_plan_result_decoding_binds_every_leg(self) -> None:
        plan = decode_intent_plan(FIXTURE["plan_result"])
        self.assertEqual(plan.plan_digest, "4b" * 32)
        self.assertEqual(plan.journey_kind, "deposit")
        self.assertEqual(plan.total_fee, IntentMoney(9, "LXP"))
        self.assertEqual(len(plan.legs), 2)
        self.assertEqual(plan.legs[0].domain, "paxeer")
        self.assertIsNone(plan.legs[0].source.account)
        self.assertEqual(plan.legs[1].destination.account, "agent:did:layerx:bob:main")
        self.assertEqual(plan.legs[1].money.amount, 500000)
        self.assertEqual(plan.legs[1].fee.amount, 8)
        self.assertEqual(plan.signing_requirements[1].authority, "agent-authority")
        unordered = {**FIXTURE["plan_result"], "legs": list(reversed(FIXTURE["plan_result"]["legs"]))}
        with self.assertRaises(PlatformSdkError):
            decode_intent_plan(unordered)
        with self.assertRaises(PlatformSdkError):
            decode_intent_plan({**FIXTURE["plan_result"], "total_fee": {"amount": 9, "currency": "LXP"}})
        with self.assertRaises(PlatformSdkError):
            decode_intent_plan({**FIXTURE["plan_result"], "extra": 1})
        with self.assertRaises(PlatformSdkError):
            decode_intent_submission({**SUBMISSION, "journey_id": "01j2gx3fam9kq4vte8n5w6y7z8"})
        self.assertEqual(decode_intent_submission(SUBMISSION).journey_id, "jrn_01j2gx3fam9kq4vte8n5w6y7z8")

    def test_failure_envelope_decodes_to_typed_error(self) -> None:
        retriable = json.dumps({
            "ok": False,
            "error": {"code": "rate-limited", "copy_key": "error.rate-limited", "retry": "retriable-after", "retry_after_ms": 2500},
            "trace": REFUSAL_TRACE,
        }).encode("utf-8")
        with self.assertRaises(HumanIntentError) as caught:
            decode_human_envelope(429, retriable)
        self.assertEqual(caught.exception.code, SdkErrorCode.RATE_LIMIT)
        self.assertEqual(caught.exception.retry, "after")
        self.assertEqual(caught.exception.retry_after_ms, 2500)
        self.assertEqual(caught.exception.trace, REFUSAL_TRACE)
        self.assertIsNone(caught.exception.field)
        missing_delay = json.dumps({
            "ok": False,
            "error": {"code": "rate-limited", "copy_key": "error.rate-limited", "retry": "retriable-after"},
            "trace": REFUSAL_TRACE,
        }).encode("utf-8")
        with self.assertRaises(PlatformSdkError) as decode_failure:
            decode_human_envelope(429, missing_delay)
        self.assertEqual(decode_failure.exception.code, SdkErrorCode.DECODE_FAILURE)
        success = json.dumps({"ok": True, "result": FIXTURE["plan_result"], "trace": FIXTURE["plan_trace"]}).encode("utf-8")
        self.assertEqual(decode_human_envelope(200, success), FIXTURE["plan_result"])
        with self.assertRaises(PlatformSdkError):
            decode_human_envelope(500, success)
        with self.assertRaises(PlatformSdkError):
            decode_human_envelope(200, json.dumps({"ok": True, "result": {}, "trace": "01j2"}).encode("utf-8"))

    def test_loopback_plan_and_submit_carry_the_idempotency_key(self) -> None:
        observed: list[dict[str, object]] = []
        server = ThreadingHTTPServer(("127.0.0.1", 0), intent_handler(observed))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            client = HumanIntentClient(f"http://127.0.0.1:{server.server_port}")
            plan = client.plan_intent(PLAN_REQUEST)
            self.assertEqual(plan.plan_digest, "4b" * 32)
            self.assertEqual(len(plan.legs), 2)
            self.assertEqual(plan.total_fee.amount, 9)
            self.assertEqual(observed[0]["path"], "/v1/intents/plan")
            self.assertIsNone(observed[0]["idempotency"])
            self.assertEqual(observed[0]["content_type"], "application/json")
            self.assertEqual(observed[0]["body"], FIXTURE["plan_request"])

            submission = client.submit_plan(SUBMIT_REQUEST, IdempotencyKey(SUBMIT_KEY))
            self.assertEqual(submission.journey_id, "jrn_01j2gx3fam9kq4vte8n5w6y7z8")
            self.assertEqual(submission.plan_digest, "4b" * 32)
            self.assertEqual(submission.state, "processing")
            self.assertEqual(submission.state_copy_key, "status.processing")
            self.assertEqual(observed[1]["path"], "/v1/intents/submit")
            self.assertEqual(observed[1]["idempotency"], SUBMIT_KEY)
            self.assertEqual(observed[1]["body"], encode_submit_plan_request(SUBMIT_REQUEST))

            with self.assertRaises(HumanIntentError) as caught:
                client.plan_intent(replace(PLAN_REQUEST, asset_id=REFUSED_ASSET))
            self.assertEqual(caught.exception.human_code, "refused-by-protocol")
            self.assertEqual(caught.exception.code, SdkErrorCode.CORE_REJECTION)
            self.assertEqual(caught.exception.copy_key, "error.intent.no-route")
            self.assertEqual(caught.exception.retriability, "final")
            self.assertEqual(caught.exception.retry, "never")
            self.assertEqual(caught.exception.status, 422)
            self.assertEqual(caught.exception.trace, REFUSAL_TRACE)
            self.assertIsNone(caught.exception.retry_after_ms)
            self.assertEqual(len(observed), 3)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_client_refuses_unsafe_endpoints(self) -> None:
        for endpoint in (
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com/?trace=1",
            "ftp://example.com",
        ):
            with self.assertRaises(PlatformSdkError):
                HumanIntentClient(endpoint)
        with self.assertRaises(PlatformSdkError):
            HumanIntentClient("https://example.com", timeout=0)


if __name__ == "__main__":
    unittest.main()
