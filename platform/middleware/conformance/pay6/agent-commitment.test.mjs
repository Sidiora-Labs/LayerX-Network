import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { verifyAgentPayment } from "@sidiora/layerx-agent-middleware";
const f = JSON.parse(readFileSync(new URL("../../../sdk/conformance/fixtures/receipt-positive-v2.json", import.meta.url)));
const b = v => Uint8Array.from(Buffer.from(v, "hex"));
const a = f.authorized_batch;
const evidence = { canonicalReceipt: b(f.canonical_receipt_hex), authorizedBatch: { batchId: b(a.batch_id_hex), asset: b(a.asset_hex), previousStateRoot: b(a.previous_state_root_hex), resultingStateRoot: b(a.resulting_state_root_hex), sequencerPublicKey: b(a.sequencer_public_key_hex) } };
const request = { tenant: "tenant", actor: "actor", authority: "authority", accountSequence: "1", timestampBound: "1", idempotencyKey: "request", feeLimit: "1", payloadBase64: "AA==", payloadHash: "00".repeat(32), asset: a.asset_hex, amount: f.expected.amount, recipient: f.expected.to_hex };
test("agent receipt verification enforces selected commitment before budget commit", async () => {
  await verifyAgentPayment(evidence, request);
  await verifyAgentPayment(evidence, { ...request, commitment: { network: "layerx:testnet", level: "executed" } });
  for (const level of ["batched", "finalised", "acknowledged"]) await assert.rejects(verifyAgentPayment(evidence, { ...request, commitment: { network: "layerx:testnet", level } }));
  await assert.rejects(verifyAgentPayment(evidence, { ...request, amount: "1" }));
});
