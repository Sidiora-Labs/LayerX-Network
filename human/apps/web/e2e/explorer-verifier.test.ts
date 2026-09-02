import assert from "node:assert/strict";
import { createServer } from "node:http";
import { once } from "node:events";
import test from "node:test";

import {
  EXPLORER_VERIFIER_REFUSED_STATUSES,
  explorerVerifierFailure,
  explorerVerifierRetryAfter,
} from "../src/explorer/verifier-state.ts";

const NOW = Date.UTC(2026, 8, 2, 12, 0, 0);

test("429 maps to an overloaded state that preserves Retry-After", () => {
  assert.deepEqual(explorerVerifierFailure(429, "1", NOW), {
    kind: "overloaded",
    retryAfter: { kind: "known", seconds: 1 },
  });
  assert.deepEqual(explorerVerifierFailure(429, "30", NOW), {
    kind: "overloaded",
    retryAfter: { kind: "known", seconds: 30 },
  });
  assert.deepEqual(explorerVerifierFailure(429, "Wed, 02 Sep 2026 12:00:45 GMT", NOW), {
    kind: "overloaded",
    retryAfter: { kind: "known", seconds: 45 },
  });
  assert.deepEqual(explorerVerifierFailure(429, "Wed, 02 Sep 2026 11:59:00 GMT", NOW), {
    kind: "overloaded",
    retryAfter: { kind: "known", seconds: 0 },
  });
  assert.deepEqual(explorerVerifierFailure(429, null, NOW), {
    kind: "overloaded",
    retryAfter: { kind: "unknown" },
  });
  assert.deepEqual(explorerVerifierFailure(429, "soon", NOW), {
    kind: "overloaded",
    retryAfter: { kind: "unknown" },
  });
  assert.deepEqual(explorerVerifierRetryAfter(" 7 ", NOW), { kind: "known", seconds: 7 });
  assert.deepEqual(explorerVerifierRetryAfter("99999999999999999999", NOW), { kind: "unknown" });
});

test("401, 409, 503 and other 5xx never render as refused", () => {
  assert.deepEqual(explorerVerifierFailure(401, null, NOW), { kind: "unavailable" });
  assert.deepEqual(explorerVerifierFailure(409, null, NOW), { kind: "divergent" });
  assert.deepEqual(explorerVerifierFailure(503, null, NOW), { kind: "unavailable" });
  assert.deepEqual(explorerVerifierFailure(503, "5", NOW), { kind: "unavailable" });
  for (const status of [500, 501, 502, 504, 507, 599]) {
    assert.deepEqual(explorerVerifierFailure(status, null, NOW), { kind: "unavailable" }, `status ${String(status)}`);
  }
});

test("only the verify route's own refusals render as refused", () => {
  assert.deepEqual([...EXPLORER_VERIFIER_REFUSED_STATUSES], [400, 413, 422]);
  for (let status = 100; status < 600; status += 1) {
    const state = explorerVerifierFailure(status, "1", NOW);
    assert.equal(
      state.kind === "refused",
      EXPLORER_VERIFIER_REFUSED_STATUSES.includes(status),
      `status ${String(status)} mapped to ${state.kind}`,
    );
    assert.equal(state.kind === "overloaded", status === 429, `status ${String(status)} mapped to ${state.kind}`);
    assert.equal(state.kind === "divergent", status === 409, `status ${String(status)} mapped to ${state.kind}`);
  }
});

test("a live 429 response carries its Retry-After into the overloaded state", async () => {
  const server = createServer((_request, response) => {
    response.writeHead(429, { "Cache-Control": "no-store", "Content-Type": "application/json", "Retry-After": "1" });
    response.end(JSON.stringify({ status: "overloaded" }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  assert.ok(address !== null && typeof address === "object");
  try {
    const response = await fetch(`http://127.0.0.1:${String(address.port)}/api/explorer/verify`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Accept: "application/json" },
      body: JSON.stringify({ kind: "receipt", evidence: "rcpt_example" }),
    });
    assert.equal(response.ok, false);
    assert.equal(response.status, 429);
    assert.equal(response.headers.get("Retry-After"), "1");
    assert.deepEqual(explorerVerifierFailure(response.status, response.headers.get("Retry-After"), NOW), {
      kind: "overloaded",
      retryAfter: { kind: "known", seconds: 1 },
    });
  } finally {
    server.close();
    await once(server, "close");
  }
});
