import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { paymentCommitment, verifyPaymentReceipt, verifyPaymentCommitmentEvidence } from "../../seller/dist/index.js";

const fixture = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url), "utf8"));
const b = (value) => Uint8Array.from(Buffer.from(value, "hex"));
const batch = fixture.authorized_batch;
const evidence = {
  canonicalReceipt: b(fixture.canonical_receipt_hex),
  authorizedBatch: {
    batchId: b(batch.batch_id_hex), asset: b(batch.asset_hex),
    previousStateRoot: b(batch.previous_state_root_hex), resultingStateRoot: b(batch.resulting_state_root_hex),
    sequencerPublicKey: b(batch.sequencer_public_key_hex),
  },
};
const offer = {
  scheme: "exact", network: "layerx:testnet", asset: batch.asset_hex,
  amount: fixture.expected.amount, payTo: fixture.expected.to_hex, maxTimeoutSeconds: 30,
};

test("native executed receipt and amount, asset, recipient binding", async () => {
  const payer = Buffer.from((await verifyPaymentReceipt(evidence, offer)).receipt.from).toString("hex");
  await verifyPaymentReceipt(evidence, { ...offer, extra: { layerx: { commitment: "executed", payer } } });
  for (const change of [{ amount: "25001" }, { asset: "01".repeat(32) }, { payTo: "01".repeat(32) }]) {
    await assert.rejects(verifyPaymentReceipt(evidence, { ...offer, ...change }));
  }
  await assert.rejects(verifyPaymentReceipt(evidence, { ...offer, extra: { layerx: { commitment: "executed", payer: "01".repeat(32) } } }));
  const corrupt = evidence.canonicalReceipt.slice();
  corrupt[corrupt.length - 1] ^= 1;
  await assert.rejects(verifyPaymentReceipt({ ...evidence, canonicalReceipt: corrupt }, offer));
});

test("explicit commitment cannot downgrade when evidence is missing", async () => {
  for (const commitment of ["batched", "finalised", "acknowledged", null, 1]) {
    await assert.rejects(verifyPaymentReceipt(evidence, { ...offer, extra: { layerx: { commitment } } }));
  }
  assert.equal(paymentCommitment(), "executed");
  for (const value of [null, [], {}, "executed"]) assert.throws(() => paymentCommitment({ layerx: value }));
});

test("signed batch inclusion is bound to receipt, network, sequence and key", async () => {
  const verified = await verifyPaymentReceipt(evidence, offer);
  const batchFixture = JSON.parse(readFileSync(new URL("batch.json", import.meta.url), "utf8"));
  const proof = {
    networkId: 7,
    canonicalHeader: b(batchFixture.header), headerSignature: b(batchFixture.signature),
    authorization: { sequencerId: b(batchFixture.sequencer_id), publicKey: evidence.authorizedBatch.sequencerPublicKey, firstBatchNumber: 1n, lastBatchNumber: 1n },
    proof: { leafIndex: 0, leafCount: 1, siblings: [] },
  };
  await verifyPaymentCommitmentEvidence(verified, evidence.authorizedBatch.sequencerPublicKey, "batched", proof);
  await assert.rejects(verifyPaymentCommitmentEvidence(verified, evidence.authorizedBatch.sequencerPublicKey, "finalised", proof));
  for (const change of [{ networkId: 8 }, { headerSignature: new Uint8Array(64) }, { proof: { leafIndex: 1, leafCount: 1, siblings: [] } }]) {
    await assert.rejects(verifyPaymentCommitmentEvidence(verified, evidence.authorizedBatch.sequencerPublicKey, "batched", { ...proof, ...change }));
  }
  await assert.rejects(verifyPaymentCommitmentEvidence(verified, new Uint8Array(32), "batched", proof));
  await assert.rejects(verifyPaymentCommitmentEvidence({ ...verified, receipt: { ...verified.receipt, globalSequence: 2n } }, evidence.authorizedBatch.sequencerPublicKey, "batched", proof));
});
