import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { parseProgramExecutionDocument, ProgramTrustContext, verifyProgramReceipt } from "../src/programs.js";
import { decodeAndVerifyProgramTerminal } from "../src/program-wire.js";

const root = new URL("../../../../../tests/fixtures/programs/emulator-response-code/", import.meta.url);
const fixture = (name: string): Buffer => Buffer.from(readFileSync(new URL(name, root), "utf8").trim(), "hex");
const document = parseProgramExecutionDocument(JSON.parse(readFileSync(new URL("execution.json", root), "utf8")));
const canonical = fixture("signed-activity.hex"), pin = fixture("sequencer-public.hex");
const authority = {
  batchId: Buffer.from(document.authority.batch_id, "hex"), asset: Buffer.from(document.authority.asset, "hex"),
  previousStateRoot: Buffer.from(document.authority.previous_state_root, "hex"),
  resultingStateRoot: Buffer.from(document.authority.resulting_state_root, "hex"), sequencerPublicKey: pin,
};
const trust = new ProgramTrustContext(pin, () => 0n, 300_000n, 3);
const verified = await verifyProgramReceipt(document, authority, trust, canonical);
assert.equal(verified.verification.receipt.resultCode, 0);
const outcome = verified.verification.receipt.programOutcome;
assert(outcome);
assert.equal(outcome.resultCode, 0);
assert.equal(document.outcome.kind, "completed");
assert.equal(document.outcome.kind === "completed" && document.outcome.code, 7);
await assert.rejects(verifyProgramReceipt({ ...document, outcome: { ...document.outcome, code: 8 } }, authority, trust, canonical), /program terminal document binding/);
const terminal = fixture("terminal.hex"), graph = fixture("call-graph.hex");
await assert.rejects(decodeAndVerifyProgramTerminal(terminal, graph, document.program_id, { ...outcome, resultCode: 7 }, 3), /candidate response code/);
for (let offset = 0; offset < terminal.length; offset++) {
  const altered = Buffer.from(terminal); altered[offset]! ^= 1;
  await assert.rejects(decodeAndVerifyProgramTerminal(altered, graph, document.program_id, outcome, 3), /program terminal root/);
}
const signature = fixture("receipt.hex"); signature[signature.length - 1]! ^= 1;
await assert.rejects(verifyProgramReceipt({ ...document, receipt: signature.toString("hex") }, authority, trust, canonical));
console.log("successful guest response code 7 is authenticated by the original signed terminal root");
