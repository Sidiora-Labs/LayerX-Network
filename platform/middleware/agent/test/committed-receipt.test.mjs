import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { verifyCommittedAgentPayment } from "../dist/index.js";
const fixture = JSON.parse(await readFile(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url)));
const bytes = (hex) => Uint8Array.from(Buffer.from(hex, "hex"));
const batch = fixture.authorized_batch;
const evidence = {
  canonicalReceipt: bytes(fixture.canonical_receipt_hex),
  authorizedBatch: {
    batchId: bytes(batch.batch_id_hex), asset: bytes(batch.asset_hex),
    previousStateRoot: bytes(batch.previous_state_root_hex),
    resultingStateRoot: bytes(batch.resulting_state_root_hex),
    sequencerPublicKey: bytes(batch.sequencer_public_key_hex),
  },
};
const request = { amount: fixture.expected.amount, asset: batch.asset_hex, recipient: fixture.expected.to_hex };
const reservation = {
  state: "committed", reservationId: fixture.expected.activity_id_hex,
  requestDigest: fixture.expected.receipt_digest_hex,
  amount: request.amount, asset: request.asset, receiptDigest: fixture.expected.receipt_digest_hex,
};
test("committed reservation returns the same authenticated native outcome without a new submission", async () => {
  const first = await verifyCommittedAgentPayment(evidence, reservation, request);
  const replay = await verifyCommittedAgentPayment(evidence, reservation, request);
  assert.equal(first.kind, "verified");
  assert.deepEqual(replay, first);
  assert.equal(Object.hasOwn(first, "submission"), false);
  assert.equal(first.verification.receipt.amount, BigInt(fixture.expected.amount));
});
test("committed replay refuses substituted receipt, digest, amount, asset and recipient", async () => {
  const corrupted = Uint8Array.from(evidence.canonicalReceipt);
  corrupted[corrupted.length - 1] ^= 1;
  await assert.rejects(verifyCommittedAgentPayment({ ...evidence, canonicalReceipt: corrupted }, reservation, request));
  await assert.rejects(verifyCommittedAgentPayment(evidence, { ...reservation, receiptDigest: "00".repeat(32) }, request));
  for (const changed of [
    { ...request, amount: (BigInt(request.amount) + 1n).toString() },
    { ...request, asset: "00".repeat(32) },
    { ...request, recipient: "00".repeat(32) },
  ]) await assert.rejects(verifyCommittedAgentPayment(evidence, reservation, changed));
});
