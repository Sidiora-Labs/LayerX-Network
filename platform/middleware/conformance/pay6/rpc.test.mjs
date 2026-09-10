import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { verifyRpcPayment, verifyPaymentReceipt, rpcBatchEvidence, PaymentRpc } from "../../seller/dist/index.js";
const f = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url)));
const b = v => Uint8Array.from(Buffer.from(v, "hex"));
const a = f.authorized_batch;
const authorized = { batchId: b(a.batch_id_hex), asset: b(a.asset_hex), previousStateRoot: b(a.previous_state_root_hex), resultingStateRoot: b(a.resulting_state_root_hex), sequencerPublicKey: b(a.sequencer_public_key_hex) };
const offer = { scheme: "exact", network: "layerx:testnet", amount: f.expected.amount, asset: a.asset_hex, payTo: f.expected.to_hex, maxTimeoutSeconds: 30 };
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

test("RPC batched result is verified against caller-configured sequencer authority", async () => {
  const verified = await verifyPaymentReceipt({ canonicalReceipt: b(f.canonical_receipt_hex), authorizedBatch: authorized }, offer);
  const activity = Buffer.from(verified.receipt.activityId).toString("hex");
  const payer = Buffer.from(verified.receipt.from).toString("hex");
  const batch = JSON.parse(readFileSync(new URL("batch.json", import.meta.url)));
  const result = { activity_id: activity, receipt: f.canonical_receipt_hex, commitment: "batched", batch_evidence: {
    kind: "receipt", activity_id: activity, canonical_value: f.canonical_receipt_hex,
    proof: { leaf_index: 0, leaf_count: 1, siblings: [] },
    signed_header: { canonical_header: batch.header, signature: batch.signature,
      sequencer_id: batch.sequencer_id, public_key: a.sequencer_public_key_hex },
  } };
  const authorization = { sequencerId: b(batch.sequencer_id), publicKey: authorized.sequencerPublicKey,
    firstBatchNumber: 1n, lastBatchNumber: 1n };
  const commitments = { async resolve(receipt, network, commitment) {
    assert.equal(network, "layerx:testnet"); assert.equal(commitment, "batched");
    return rpcBatchEvidence(result, activity, receipt, { networkId: 7, authorization });
  } };
  const batched = { ...offer, extra: { layerx: { commitment: "batched", payer } } };
  assert.equal((await verifyRpcPayment(result, activity, payer, batched, authorized, commitments)).kind, "verified");
  await assert.rejects(verifyRpcPayment({ ...result, state: "acknowledged" }, activity, payer, batched, authorized, commitments));
  const forged = { ...result, batch_evidence: { ...result.batch_evidence,
    signed_header: { ...result.batch_evidence.signed_header, public_key: "00".repeat(32) } } };
  await assert.rejects(verifyRpcPayment(forged, activity, payer, batched, authorized,
    { async resolve(receipt) { return rpcBatchEvidence(forged, activity, receipt, { networkId: 7, authorization }); } }));
});
