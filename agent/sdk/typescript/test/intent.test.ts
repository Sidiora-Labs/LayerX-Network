import assert from "node:assert/strict";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import * as http from "node:http";

import {
  HumanIntentClient,
  HumanIntentError,
  decodeHumanEnvelope,
  decodeIntentPlan,
  decodeIntentSubmission,
  encodePlanIntentRequest,
  encodeSubmitPlanRequest,
  idempotencyKey,
  protocolAmount,
  PlatformSdkError,
  type PlanIntentRequest,
  type SubmitPlanRequest,
} from "../src/index.js";

interface IntentPlanFixture {
  readonly name: string;
  readonly provenance: string;
  readonly plan_request: Record<string, unknown>;
  readonly plan_result: Record<string, unknown>;
  readonly plan_trace: string;
}

const fixture = JSON.parse(readFileSync(
  new URL("../../../../../platform/sdk/conformance/fixtures/intent-plan-v1.json", import.meta.url),
  "utf8",
)) as IntentPlanFixture;
assert.equal(fixture.name, "intent-plan-v1");
assert.equal(typeof fixture.provenance, "string");

const REFUSED_ASSET = "ee".repeat(32);
const SUBMIT_KEY = idempotencyKey("b1946ac92492d2347c6235b4d2611184");
const SUBMIT_TRACE = "trc_01j2gxq4dpc2d3e4f5g6h7j8k9";
const REFUSAL_TRACE = "trc_01j2gxq3cnb1c2d3e4f5g6h7j8";

const planRequest: PlanIntentRequest = {
  source: { kind: "paxeer-wallet", account: null },
  destination: { kind: "agent", account: "agent:did:layerx:bob:main" },
  assetId: "99".repeat(32),
  money: { amount: protocolAmount("500000"), currency: "LXP" },
  constraints: {
    deadline: "2026-08-18T09:35:00Z",
    maxFee: { amount: protocolAmount("250"), currency: "LXP" },
    allowTopUp: false,
  },
};

const submitRequest: SubmitPlanRequest = {
  planDigest: "4b".repeat(32),
  signedDigest: "7c".repeat(32),
  bindings: [
    {
      legIndex: 0,
      actionKey: "a1".repeat(32),
      actor: "did:layerx:alice",
      authority: "custody-key",
      relationship: "self",
      accountSequence: 7n,
      notBefore: 1755500000n,
      notAfter: 1755503600n,
      feeLimit: { amount: protocolAmount("125"), currency: "LXP" },
    },
    {
      legIndex: 1,
      actionKey: "b2".repeat(32),
      actor: "did:layerx:alice",
      authority: "agent-authority",
      relationship: "self",
      accountSequence: 8n,
      notBefore: 1755500000n,
      notAfter: 1755503600n,
      feeLimit: { amount: protocolAmount("125"), currency: "LXP" },
    },
  ],
};

assert.deepEqual(encodePlanIntentRequest(planRequest), fixture.plan_request);
assert.deepEqual(encodeSubmitPlanRequest(submitRequest), {
  plan_digest: "4b".repeat(32),
  signed_digest: "7c".repeat(32),
  bindings: [
    { leg_index: 0, action_key: "a1".repeat(32), actor: "did:layerx:alice", authority: "custody-key", relationship: "self", account_sequence: 7, not_before: 1755500000, not_after: 1755503600, fee_limit: { amount: "125", currency: "LXP" } },
    { leg_index: 1, action_key: "b2".repeat(32), actor: "did:layerx:alice", authority: "agent-authority", relationship: "self", account_sequence: 8, not_before: 1755500000, not_after: 1755503600, fee_limit: { amount: "125", currency: "LXP" } },
  ],
});

assert.throws(() => encodePlanIntentRequest({ ...planRequest, source: { kind: "paxeer-wallet", account: "agent:did:layerx:alice:main" } }));
assert.throws(() => encodePlanIntentRequest({ ...planRequest, destination: { kind: "agent", account: null } }));
assert.throws(() => encodePlanIntentRequest({ ...planRequest, assetId: "AB".repeat(32) }));
assert.throws(() => encodePlanIntentRequest({ ...planRequest, constraints: { ...planRequest.constraints, deadline: "2026-08-18 09:35:00" } }));
assert.throws(() => encodeSubmitPlanRequest({ ...submitRequest, bindings: [] }));
assert.throws(() => encodeSubmitPlanRequest({ ...submitRequest, bindings: [submitRequest.bindings[0]!, submitRequest.bindings[0]!] }));
assert.throws(() => encodeSubmitPlanRequest({ ...submitRequest, bindings: [{ ...submitRequest.bindings[0]!, notAfter: -1n }] }));

const decodedPlan = decodeIntentPlan(fixture.plan_result);
assert.equal(decodedPlan.planDigest, "4b".repeat(32));
assert.equal(decodedPlan.journeyKind, "deposit");
assert.equal(decodedPlan.totalFee.amount, 9n);
assert.equal(decodedPlan.legs.length, 2);
assert.equal(decodedPlan.legs[0]!.domain, "paxeer");
assert.equal(decodedPlan.legs[0]!.source.account, null);
assert.equal(decodedPlan.legs[1]!.destination.account, "agent:did:layerx:bob:main");
assert.equal(decodedPlan.legs[1]!.money.amount, 500000n);
assert.equal(decodedPlan.legs[1]!.fee.amount, 8n);
assert.equal(decodedPlan.signingRequirements[1]!.authority, "agent-authority");

const unordered = { ...fixture.plan_result, legs: [...(fixture.plan_result.legs as unknown[])].reverse() };
assert.throws(() => decodeIntentPlan(unordered));
assert.throws(() => decodeIntentPlan({ ...fixture.plan_result, total_fee: { amount: 9, currency: "LXP" } }));
assert.throws(() => decodeIntentPlan({ ...fixture.plan_result, extra: 1 }));
assert.throws(() => decodeIntentSubmission({ journey_id: "01j2gx3fam9kq4vte8n5w6y7z8", plan_digest: "4b".repeat(32), state: "processing", state_copy_key: "status.processing" }));

const planEnvelope = { ok: true, result: fixture.plan_result, trace: fixture.plan_trace };
assert.deepEqual(decodeHumanEnvelope(200, Buffer.from(JSON.stringify(planEnvelope), "utf8")), fixture.plan_result);
assert.throws(() => decodeHumanEnvelope(500, Buffer.from(JSON.stringify(planEnvelope), "utf8")),
  (error: unknown) => error instanceof PlatformSdkError && error.code === "decode-failure");
assert.throws(() => decodeHumanEnvelope(200, Buffer.from(JSON.stringify({ ok: true, result: {}, trace: "01j2" }), "utf8")));
assert.throws(() => decodeHumanEnvelope(429, Buffer.from(JSON.stringify({
  ok: false,
  error: { code: "rate-limited", copy_key: "error.rate-limited", retry: "retriable-after" },
  trace: REFUSAL_TRACE,
}), "utf8")));
assert.throws(() => decodeHumanEnvelope(429, Buffer.from(JSON.stringify({
  ok: false,
  error: { code: "rate-limited", copy_key: "error.rate-limited", retry: "retriable-after", retry_after_ms: 2500 },
  trace: REFUSAL_TRACE,
}), "utf8")), (error: unknown) => error instanceof HumanIntentError && error.code === "rate-limit"
  && error.retry === "after" && error.retryAfterMs === 2500 && error.field === null);

const observed: { readonly path: string; readonly idempotency: string | undefined; readonly body: unknown }[] = [];
const server = http.createServer((request, response) => {
  const chunks: Buffer[] = [];
  request.on("data", (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
  request.on("end", () => {
    const path = request.url ?? "";
    const idempotency = request.headers["idempotency-key"];
    const body = JSON.parse(Buffer.concat(chunks).toString("utf8")) as Record<string, unknown>;
    observed.push({ path, idempotency: typeof idempotency === "string" ? idempotency : undefined, body });
    assert.equal(request.method, "POST");
    assert.equal(request.headers["content-type"], "application/json");
    const refused = body.asset_id === REFUSED_ASSET;
    const [status, envelope] = path === "/v1/intents/plan"
      ? refused
        ? [422, { ok: false, error: { code: "refused-by-protocol", copy_key: "error.intent.no-route", retry: "final" }, trace: REFUSAL_TRACE }] as const
        : [200, { ok: true, result: fixture.plan_result, trace: fixture.plan_trace }] as const
      : [200, {
        ok: true,
        result: {
          journey_id: "jrn_01j2gx3fam9kq4vte8n5w6y7z8",
          plan_digest: "4b".repeat(32),
          state: "processing",
          state_copy_key: "status.processing",
        },
        trace: SUBMIT_TRACE,
      }] as const;
    const encoded = Buffer.from(JSON.stringify(envelope), "utf8");
    response.writeHead(status, { "Content-Type": "application/json", "Content-Length": encoded.length });
    response.end(encoded);
  });
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const address = server.address();
assert(address !== null && typeof address === "object", "intent listener missing");
try {
  const client = new HumanIntentClient({ endpoint: `http://127.0.0.1:${address.port}` });

  const plan = await client.planIntent(planRequest);
  assert.equal(plan.planDigest, "4b".repeat(32));
  assert.equal(plan.legs.length, 2);
  assert.equal(plan.totalFee.amount, 9n);
  assert.equal(observed[0]!.path, "/v1/intents/plan");
  assert.equal(observed[0]!.idempotency, undefined);
  assert.deepEqual(observed[0]!.body, fixture.plan_request);

  const submission = await client.submitPlan(submitRequest, SUBMIT_KEY);
  assert.equal(submission.journeyId, "jrn_01j2gx3fam9kq4vte8n5w6y7z8");
  assert.equal(submission.planDigest, "4b".repeat(32));
  assert.equal(submission.state, "processing");
  assert.equal(submission.stateCopyKey, "status.processing");
  assert.equal(observed[1]!.path, "/v1/intents/submit");
  assert.equal(observed[1]!.idempotency, "b1946ac92492d2347c6235b4d2611184");
  assert.deepEqual(observed[1]!.body, encodeSubmitPlanRequest(submitRequest));

  await assert.rejects(
    client.planIntent({ ...planRequest, assetId: REFUSED_ASSET }),
    (error: unknown) => error instanceof HumanIntentError
      && error.humanCode === "refused-by-protocol"
      && error.code === "core-rejection"
      && error.copyKey === "error.intent.no-route"
      && error.retriability === "final"
      && error.retry === "never"
      && error.status === 422
      && error.trace === REFUSAL_TRACE
      && error.retryAfterMs === undefined,
  );
  assert.equal(observed.length, 3);
} finally {
  server.close();
  await once(server, "close");
}

assert.throws(() => new HumanIntentClient({ endpoint: "http://example.com" }));
assert.throws(() => new HumanIntentClient({ endpoint: "https://user:secret@example.com" }));
assert.throws(() => new HumanIntentClient({ endpoint: "https://example.com/?trace=1" }));
assert.throws(() => new HumanIntentClient({ endpoint: "https://example.com", timeoutMs: 0 }));
