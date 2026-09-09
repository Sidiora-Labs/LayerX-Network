import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { decodeReceive, bindReceiveActivity, PreparedGrantDraws } from "@sidiora/layerx-sdk";
import { PaymentRpc } from "@sidiora/layerx-seller-middleware";
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
      requirements: { scheme: "subscription", asset: r.asset, amount: r.amount, payTo: r.to, extra: { layerx: { commitment: "executed", purposeHash: r.payer_grant.purpose_hash, payer: r.from, windowSeconds: "3600" } } } };
    assert.deepEqual(await store.execute(request), { kind: "pending" });
    assert.deepEqual(await store.execute(request), { kind: "pending" });
    assert.throws(() => store.register("other", "ab".repeat(32), canonical, receive, key, "subscription:period:1"));
    assert.throws(() => store.register("payer", "cd".repeat(32), canonical, receive, key, "subscription:period:1"));
  } finally { store.close(); rmSync(directory, { recursive: true }); }
});
