import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { verifyRpcPayment, verifyPaymentReceipt, PaymentRpc } from "@sidiora/layerx-seller-middleware";
const f = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url)));
const b = v => Uint8Array.from(Buffer.from(v, "hex"));
const a = f.authorized_batch;
const authorized = { batchId: b(a.batch_id_hex), asset: b(a.asset_hex), previousStateRoot: b(a.previous_state_root_hex), resultingStateRoot: b(a.resulting_state_root_hex), sequencerPublicKey: b(a.sequencer_public_key_hex) };
const offer = { scheme: "exact", network: "layerx:testnet", amount: f.expected.amount, asset: a.asset_hex, payTo: f.expected.to_hex, maxTimeoutSeconds: 30, extra: { layerx: { commitment: "executed", payer: f.expected.from_hex } } };
test("RPC receipt response binds actual signed activity and payer; pending never succeeds", async () => {
  const verified = await verifyPaymentReceipt({ canonicalReceipt: b(f.canonical_receipt_hex), authorizedBatch: authorized }, offer);
  const activity = Buffer.from(verified.receipt.activityId).toString("hex");
  const payer = Buffer.from(verified.receipt.from).toString("hex");
  const result = { activity_id: activity, receipt: f.canonical_receipt_hex };
  assert.equal((await verifyRpcPayment(result, activity, payer, offer, authorized)).kind, "verified");
  assert.equal((await verifyRpcPayment({ activity_id: activity, state: "pending" }, activity, payer, offer, authorized)).kind, "pending");
  for (const change of [{ activity_id: "ab".repeat(32) }, { receipt: "00" }, { state: "acknowledged" }, { commitment: "batched" }]) await assert.rejects(verifyRpcPayment({ ...result, ...change }, activity, payer, offer, authorized));
  await assert.rejects(verifyRpcPayment(result, activity, "ab".repeat(32), offer, authorized));
  await assert.rejects(verifyRpcPayment({ activity_id: activity }, activity, payer, offer, authorized));
});
test("RPC endpoint and submission bounds refuse before network access", async () => {
  for (const endpoint of ["http://example.com/rpc", "https://user:password@example.com/rpc", "https://example.com/rpc?token=1", "https://example.com/"]) assert.throws(() => new PaymentRpc(endpoint));
  const rpc = new PaymentRpc("http://127.0.0.1:1/rpc");
  await assert.rejects(rpc.send("00", "acknowledged"));
  await assert.rejects(rpc.send("0x00", "executed"));
});
