import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { rpcCheckpointEvidence, verifyPaymentReceipt, verifyPaymentCommitmentEvidence } from "@sidiora/layerx-seller-middleware";
const read = path => JSON.parse(readFileSync(new URL(path, import.meta.url)));
const f = read("checkpoint.json"), b = read("batch.json"), r = read("../../../sdk/conformance/fixtures/receipt-positive-v2.json");
const hex = v => Uint8Array.from(Buffer.from(v, "hex"));
const a = r.authorized_batch;
const authorizedBatch = { batchId: hex(a.batch_id_hex), asset: hex(a.asset_hex), previousStateRoot: hex(a.previous_state_root_hex), resultingStateRoot: hex(a.resulting_state_root_hex), sequencerPublicKey: hex(a.sequencer_public_key_hex) };
const batch = { networkId: 7, canonicalHeader: hex(b.header), headerSignature: hex(b.signature), authorization: { sequencerId: hex(b.sequencer_id), publicKey: authorizedBatch.sequencerPublicKey, firstBatchNumber: 1n, lastBatchNumber: 1n }, proof: { leafIndex: 0, leafCount: 1, siblings: [] } };
const authority = { canonicalContext: hex(f.operator.context), requiredGuarantors: 1,
  verification: { bondedSet: [{ guarantorId: hex(f.operator.guarantor_id), publicKey: hex(f.operator.public_key), bonded: true }], registeredCheckpointId: hex(f.checkpoint_evidence.checkpoint_id), expectedPaxeerChainId: BigInt(f.operator.chain_id), expectedSettlementContract: hex(f.operator.contract), registeredSettlementReference: hex(f.operator.reference), availabilityObtained: true },
  signatures: { async verifyRecoverableSecp256k1(publicKey, signature, signatureV, signer, digest) {
    const result = spawnSync("python3", ["-c", "import sys, importlib.util, os; spec=importlib.util.spec_from_file_location('signatures',os.path.join(os.environ['PYTHONPATH'],'layerx_fastapi/signatures.py')); module=importlib.util.module_from_spec(spec); spec.loader.exec_module(module); v=module.verify_recoverable_secp256k1; sys.exit(0 if v(bytes.fromhex(sys.argv[1]),bytes.fromhex(sys.argv[2]),int(sys.argv[3]),bytes.fromhex(sys.argv[4]),bytes.fromhex(sys.argv[5])) else 1)", ...[publicKey, signature].map(v => Buffer.from(v).toString("hex")), String(signatureV), ...[signer, digest].map(v => Buffer.from(v).toString("hex"))], { env: { ...process.env, PYTHONPATH: new URL("../../../integrations/fastapi", import.meta.url).pathname } });
    assert.equal(result.stderr.toString(), "");
    assert.ok(result.status === 0 || result.status === 1);
    return result.status === 0;
  } },
};
const offer = { scheme: "exact", network: "layerx:testnet", asset: a.asset_hex, amount: r.expected.amount, payTo: r.expected.to_hex, maxTimeoutSeconds: 30, extra: { layerx: { commitment: "executed", payer: r.expected.from_hex } } };
test("published checkpoint binary verifies finalised receipt with configured authority", async () => {
  const verified = await verifyPaymentReceipt({ canonicalReceipt: hex(r.canonical_receipt_hex), authorizedBatch }, offer);
  await verifyPaymentCommitmentEvidence(verified, authorizedBatch.sequencerPublicKey, "finalised", rpcCheckpointEvidence(f, batch, authority));
  for (const change of [{ requiredGuarantors: 0 }, { requiredGuarantors: 2 }, { canonicalContext: hex("00") }]) assert.throws(() => rpcCheckpointEvidence(f, batch, { ...authority, ...change }));
  for (const change of [{ bondedSet: [] }, { expectedPaxeerChainId: 778n }, { availabilityObtained: false }]) await assert.rejects(verifyPaymentCommitmentEvidence(verified, authorizedBatch.sequencerPublicKey, "finalised", rpcCheckpointEvidence(f, batch, { ...authority, verification: { ...authority.verification, ...change } })));
  for (const field of ["checkpoint", "context", "canonical_header", "checkpoint_id"]) for (const value of ["00", f.checkpoint_evidence[field] + "00"]) assert.throws(() => rpcCheckpointEvidence({ checkpoint_evidence: { ...f.checkpoint_evidence, [field]: value } }, batch, authority));
  const forged = Buffer.from(f.checkpoint_evidence.checkpoint, "hex");
  forged[forged.length - 114] ^= 1;
  await assert.rejects(verifyPaymentCommitmentEvidence(verified, authorizedBatch.sequencerPublicKey, "finalised", rpcCheckpointEvidence({ checkpoint_evidence: { ...f.checkpoint_evidence, checkpoint: forged.toString("hex") } }, batch, authority)));
});
