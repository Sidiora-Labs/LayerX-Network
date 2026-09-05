import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { parseProgramExecutionDocument, ProgramTrustContext, verifyProgramReceipt } from "../src/programs.js";
import { decodeAndVerifyProgramTerminal } from "../src/program-wire.js";
import { verifyReceiptOutcome } from "../src/verifier.js";

const fixture = JSON.parse(readFileSync(new URL("../../../../../platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json", import.meta.url), "utf8")) as {
  canonical_receipt_hex: string; program_id_hex: string; receipt_digest_hex: string; terminal_payload_hex: string; call_graph_hex: string;
  execution_document: Readonly<Record<string, unknown>>;
  authorized_batch: { batch_id_hex: string; asset_hex: string; previous_state_root_hex: string; resulting_state_root_hex: string; sequencer_public_key_hex: string };
};
const source = fixture.authorized_batch;
const authority = {
  batchId: Buffer.from(source.batch_id_hex, "hex"), asset: Buffer.from(source.asset_hex, "hex"),
  previousStateRoot: Buffer.from(source.previous_state_root_hex, "hex"), resultingStateRoot: Buffer.from(source.resulting_state_root_hex, "hex"),
  sequencerPublicKey: Buffer.from(source.sequencer_public_key_hex, "hex"),
};
const verified = await verifyReceiptOutcome(Buffer.from(fixture.canonical_receipt_hex, "hex"), authority, { protocolVersion: 3 });
const receipt = verified.receipt; assert.equal(receipt.protocolVersion, 3); assert.equal(receipt.moduleVersion, 4); assert.equal(receipt.operation, 3);
assert(receipt.programOutcome);
const terminal = await decodeAndVerifyProgramTerminal(Buffer.from(fixture.terminal_payload_hex, "hex"), Buffer.from(fixture.call_graph_hex, "hex"), fixture.program_id_hex, receipt.programOutcome, 3);
const document = fixture.execution_document;
assert.deepEqual(document.usage, terminal.usage);
assert.deepEqual(document.outcome, terminal.outcome);
const parsed = parseProgramExecutionDocument(document);
assert.equal(parsed.module_version, 4);
const trust = new ProgramTrustContext(authority.sequencerPublicKey, () => 0n, 300_000n, 3);
const execution = await verifyProgramReceipt(parsed, authority, trust);
assert.equal(Buffer.from(execution.verification.receiptDigest).toString("hex"), fixture.receipt_digest_hex);
await assert.rejects(verifyProgramReceipt(parseProgramExecutionDocument({ ...document, module_version: 3 }), authority, trust));
await assert.rejects(verifyProgramReceipt(parsed, authority, new ProgramTrustContext(authority.sequencerPublicKey, () => 0n, 300_000n, 2)));
for (const module_version of [0, 5, true]) assert.throws(() => parseProgramExecutionDocument({ ...document, module_version }));
const corrupted = Buffer.from(fixture.canonical_receipt_hex, "hex"); corrupted[corrupted.length - 1] = corrupted[corrupted.length - 1]! ^ 1;
await assert.rejects(verifyProgramReceipt(parseProgramExecutionDocument({ ...document, receipt: corrupted.toString("hex") }), authority, trust));
