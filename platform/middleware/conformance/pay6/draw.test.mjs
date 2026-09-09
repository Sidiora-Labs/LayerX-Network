import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { test } from "node:test";
import { decodeReceive } from "../../../../agent/sdk/typescript/dist/src/x402/receive.js";
import { bindReceiveActivity } from "../../../../agent/sdk/typescript/dist/src/x402/activity.js";
import { PreparedGrantDraws } from "../../../../agent/sdk/typescript/dist/src/x402/draw.js";
import { PaymentRpc, PaymentRpcError } from "../../seller/dist/index.js";
const fixture = readFileSync(new URL("draw.hex", import.meta.url), "utf8").trim().split("\n");
const canonical = Uint8Array.from(Buffer.from(fixture[0], "hex"));
const receive = Uint8Array.from(Buffer.from(readFileSync(new URL("receive.hex", import.meta.url), "utf8").split("\n")[0], "hex"));
const key = "04" + "00".repeat(31);
test("native signed envelope binds receive payload, identity network and idempotency", () => {
  assert.equal(bindReceiveActivity(canonical, receive, "did:lxp:pay6-receiver", 7, key), fixture[1]);
  for (let length = 0; length < canonical.length; length++) assert.throws(() => bindReceiveActivity(canonical.slice(0, length), receive, "did:lxp:pay6-receiver", 7, key));
  for (const [actor, network, id] of [["wrong", 7, key], ["did:lxp:pay6-receiver", 8, key], ["did:lxp:pay6-receiver", 7, "00".repeat(32)]]) assert.throws(() => bindReceiveActivity(canonical, receive, actor, network, id));
});
test("persistent draw registrations preserve signed bytes and reject changed request ownership", async () => {
  const directory = mkdtempSync(new URL("../../../../qual-logs/pay6/draw-", import.meta.url).pathname);
  const rpc = new PaymentRpc("http://127.0.0.1:1/rpc");
  const f = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url))).authorized_batch;
  const b = v => Uint8Array.from(Buffer.from(v, "hex"));
  const authority = async () => ({ batchId: b(f.batch_id_hex), asset: b(f.asset_hex), previousStateRoot: b(f.previous_state_root_hex), resultingStateRoot: b(f.resulting_state_root_hex), sequencerPublicKey: b(f.sequencer_public_key_hex) });
  const store = new PreparedGrantDraws(join(directory, "draws.sqlite"), "did:lxp:pay6-receiver", 7, rpc, authority);
  try {
    store.register("payer", "ab".repeat(32), canonical, receive, key, "subscription:period:1");
    store.register("payer", "ab".repeat(32), canonical, receive, key, "subscription:period:1");
    const r = decodeReceive(receive);
    const request = { principal: "payer", requestDigest: "ab".repeat(32), receive: Buffer.from(receive).toString("hex"), idempotencyKey: key,
      requirements: { scheme: "subscription", asset: r.asset, amount: r.amount, payTo: r.to, extra: { layerx: { commitment: "executed", payer: r.from, purposeHash: r.payer_grant.purpose_hash, windowSeconds: "3600" } } } };
    await assert.rejects(store.execute(request));
    await assert.rejects(store.execute(request));
    assert.throws(() => store.register("other", "ab".repeat(32), canonical, receive, key, "subscription:period:1"));
    assert.throws(() => store.register("payer", "cd".repeat(32), canonical, receive, key, "subscription:period:1"));
  } finally { store.close(); rmSync(directory, { recursive: true }); }
});

test("draw execution maps only typed protocol pending and surfaces HTTP, parse, and other RPC errors", async () => {
  let mode = "pending";
  const server = createServer(async (incoming, response) => {
    const chunks = [];
    for await (const chunk of incoming) chunks.push(chunk);
    const id = JSON.parse(Buffer.concat(chunks).toString("utf8")).id;
    if (mode === "http") {
      response.writeHead(503);
      response.end();
      return;
    }
    response.writeHead(200, { "content-type": "application/json" });
    if (mode === "parse") {
      response.end("{");
      return;
    }
    const error = mode === "pending"
      ? { code: -32001, message: "Requested commitment unavailable", data: { state: "pending" } }
      : mode === "invalid-pending"
        ? { code: -32001, message: "Requested commitment unavailable", data: { state: "completed" } }
        : { code: -32603, message: "Internal error", data: { state: "pending" } };
    response.end(JSON.stringify({ jsonrpc: "2.0", id, error }));
  });
  await new Promise((resolve, reject) => server.listen(0, "127.0.0.1", (error) => error ? reject(error) : resolve()));
  const address = server.address();
  assert.ok(address !== null && typeof address === "object");
  const receiptFixture = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url))).authorized_batch;
  const bytes = value => Uint8Array.from(Buffer.from(value, "hex"));
  const authority = async () => ({ batchId: bytes(receiptFixture.batch_id_hex), asset: bytes(receiptFixture.asset_hex),
    previousStateRoot: bytes(receiptFixture.previous_state_root_hex), resultingStateRoot: bytes(receiptFixture.resulting_state_root_hex),
    sequencerPublicKey: bytes(receiptFixture.sequencer_public_key_hex) });
  const decoded = decodeReceive(receive);
  const request = { principal: "payer", requestDigest: "ab".repeat(32), receive: Buffer.from(receive).toString("hex"), idempotencyKey: key,
    requirements: { scheme: "subscription", asset: decoded.asset, amount: decoded.amount, payTo: decoded.to,
      extra: { layerx: { commitment: "executed", payer: decoded.from, purposeHash: decoded.payer_grant.purpose_hash, windowSeconds: "3600" } } } };
  try {
    for (const selected of ["pending", "invalid-pending", "rpc", "http", "parse"]) {
      mode = selected;
      const directory = mkdtempSync(new URL(`../../../../qual-logs/pay6/draw-${selected}-`, import.meta.url).pathname);
      const store = new PreparedGrantDraws(join(directory, "draws.sqlite"), "did:lxp:pay6-receiver", 7,
        new PaymentRpc(`http://127.0.0.1:${address.port}/rpc`), authority);
      try {
        store.register("payer", "ab".repeat(32), canonical, receive, key, "subscription:period:1");
        if (selected === "pending") {
          assert.deepEqual(await store.execute(request), { kind: "pending" });
        } else if (selected === "rpc" || selected === "invalid-pending") {
          const code = selected === "rpc" ? -32603 : -32001;
          await assert.rejects(store.execute(request), error => error instanceof PaymentRpcError && error.code === code);
        } else {
          await assert.rejects(store.execute(request), error => !(error instanceof PaymentRpcError));
        }
      } finally {
        store.close();
        rmSync(directory, { recursive: true });
      }
    }
  } finally {
    await new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  }
});
