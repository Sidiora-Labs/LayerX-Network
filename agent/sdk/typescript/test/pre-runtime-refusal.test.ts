import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { parseProgramExecutionDocument, ProgramTrustContext, verifyProgramReceipt } from "../src/programs.js";
import { bindRetainedProgramCall, decodeAndVerifyProgramTerminal } from "../src/program-wire.js";
import type { ProgramReceiptOutcome, ProtocolReceipt } from "../src/verifier.js";

const root = new URL("../../../../../tests/fixtures/programs/pre-runtime-refusal/", import.meta.url);
const fixture = (name: string): Buffer => Buffer.from(readFileSync(new URL(name, root), "utf8").trim(), "hex");
const document = parseProgramExecutionDocument(JSON.parse(readFileSync(new URL("execution.json", root), "utf8")));
const canonical = fixture("signed-activity.hex"), pin = fixture("sequencer-public.hex");
const payload = fixture("terminal.hex"), graph = fixture("call-graph.hex");
const hash = (value: Uint8Array): Buffer => createHash("sha256").update(value).digest();
const authority = {
  batchId: Buffer.from(document.authority.batch_id, "hex"), asset: Buffer.from(document.authority.asset, "hex"),
  previousStateRoot: Buffer.from(document.authority.previous_state_root, "hex"),
  resultingStateRoot: Buffer.from(document.authority.resulting_state_root, "hex"), sequencerPublicKey: pin,
};
const trust = new ProgramTrustContext(pin, () => 0n, 300_000n, 3);
const verified = await verifyProgramReceipt(document, authority, trust, canonical);
const protocol = verified.verification.receipt, outcome = protocol.programOutcome;
assert(outcome);
assert.equal(outcome.resultCode, -3);
assert.equal(verified.transferVerification, "reconstructed");
assert.deepEqual(document.outcome, { kind: "refused", failure: { kind: "guest_refused", code: -3 } });
assert.equal(payload.length, 165);
const { retained_signed_activity: _retained, ...withoutRetained } = document;
await assert.rejects(verifyProgramReceipt(withoutRetained, authority, trust), /signed activity binding/);
await verifyProgramReceipt(withoutRetained, authority, trust, canonical);
const changedActivity = Buffer.from(canonical); changedActivity[changedActivity.length - 1]! ^= 1;
await assert.rejects(verifyProgramReceipt(document, authority, trust, changedActivity), /retained program activity mismatch/);
await assert.rejects(verifyProgramReceipt({ ...document, retained_signed_activity: changedActivity.toString("hex") }, authority, trust), /signed activity mismatch/);
const changedReceipt = fixture("receipt.hex"); changedReceipt[changedReceipt.length - 1]! ^= 1;
await assert.rejects(verifyProgramReceipt({ ...document, receipt: changedReceipt.toString("hex") }, authority, trust));
await assert.rejects(verifyProgramReceipt({ ...document, program_id: "01".repeat(32) }, authority, trust), /native refusal payload binding/);
await assert.rejects(verifyProgramReceipt({ ...document, idempotency_key: "01".repeat(32) }, authority, trust), /retained program call metadata mismatch/);

const historical = Buffer.from(canonical);
historical.writeUInt16BE(2, 0); historical.writeUInt16BE(2, 6);
const historicalId = createHash("sha256").update("LXP/v1/activity-id\0").update(historical).digest("hex");
const historicalBinding = await bindRetainedProgramCall(historical, historicalId, document.program_id, 2);
assert.equal(historicalBinding.guestAbi, 2);
assert.equal(historicalBinding.idempotencyKey, document.idempotency_key);
await assert.rejects(verifyProgramReceipt(document, authority, trust, historical));

const decode = (terminal: Uint8Array, metadata: ProgramReceiptOutcome = outcome,
  enclosing: ProtocolReceipt = protocol, callGraph: Uint8Array = graph) => {
  const bound = { ...metadata, terminalPayloadRoot: hash(terminal), callGraphRoot: hash(callGraph) };
  return decodeAndVerifyProgramTerminal(terminal, callGraph, document.program_id, bound,
    3, { protocol: { ...enclosing, programOutcome: bound }, signedActivity: canonical });
};
await assert.rejects(decodeAndVerifyProgramTerminal(payload, graph, document.program_id, { ...outcome, feeUnits: 1n },
  3, { protocol, signedActivity: canonical }), /native refusal receipt binding/);
for (let length = 0; length < payload.length; length++) {
  await assert.rejects(decode(payload.subarray(0, length)));
}
for (let offset = 0; offset < payload.length; offset++) {
  const changed = Buffer.from(payload); changed[offset]! ^= 1;
  await assert.rejects(decode(changed));
}
await assert.rejects(decode(Buffer.concat([payload, Buffer.of(0)])));
const size = Buffer.alloc(4); size.writeUInt32BE(payload.length);
await assert.rejects(decode(Buffer.concat([Buffer.from("LXP/programs/terminal-applied-legs/v1\0"), size, payload, Buffer.alloc(4)])));
for (const change of [
  { resultCode: 0 }, { runtimeVersion: 2 }, { abiVersion: 1 }, { terminalKind: 1 as const },
  { encodingVersion: 3 as const }, { memoryBytes: 1n }, { storageReadBytes: 1n },
  { outputValues: 1 }, { outputBytes: 1n }, { transferRoot: Buffer.alloc(32, 1) },
  { appliedLegsDigest: Buffer.alloc(32) }, { occupancyAssetId: Buffer.alloc(32, 1) },
  { occupancyEvidenceDigest: Buffer.alloc(32, 1) }, { occupancyTransferRoot: Buffer.alloc(32, 1) },
  { occupancyByteBatches: 1n }, { occupancyFeeUnits: 1n },
]) await assert.rejects(decode(payload, { ...outcome, ...change }));
for (const change of [
  { activityId: Buffer.alloc(32, 1) }, { resultCode: -4 }, { moduleVersion: 3 },
  { parameterVersion: 2 }, { moduleId: 1 }, { operation: 4 }, { protocolVersion: 2 },
]) await assert.rejects(decode(payload, outcome, { ...protocol, ...change }));
await assert.rejects(decode(payload, outcome, protocol, Buffer.concat([graph, Buffer.of(0)])));
console.log("native admission refusal verifies against the original CALL and rejects altered commitments");
