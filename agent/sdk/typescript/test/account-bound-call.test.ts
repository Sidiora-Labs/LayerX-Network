import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { parseProgramExecutionDocument, ProgramTrustContext, verifyProgramReceipt } from "../src/programs.js";
import { decodeAndVerifyProgramTerminal } from "../src/program-wire.js";

const fixtureRoot = new URL("../../../../../tests/fixtures/programs/account-bound-call/", import.meta.url);
const document = parseProgramExecutionDocument(JSON.parse(readFileSync(new URL("execution.json", fixtureRoot), "utf8")));
const pin = Buffer.from(readFileSync(new URL("sequencer-key.hex", fixtureRoot), "utf8").trim(), "hex");
const canonical = Buffer.from(readFileSync(new URL("signed-activity.hex", fixtureRoot), "utf8").trim(), "hex");
const authority = {
  batchId: Buffer.from(document.authority.batch_id, "hex"), asset: Buffer.from(document.authority.asset, "hex"),
  previousStateRoot: Buffer.from(document.authority.previous_state_root, "hex"),
  resultingStateRoot: Buffer.from(document.authority.resulting_state_root, "hex"), sequencerPublicKey: pin,
};
const trust = new ProgramTrustContext(pin, () => 0n, 300_000n, 3);
const verified = await verifyProgramReceipt(document, authority, trust);
assert.equal(verified.transferVerification, "reconstructed");
assert.equal(document.result_code, 0);
assert.equal(document.activity_id, createHash("sha256").update("LXP/v1/activity-id\0").update(canonical).digest("hex"));
const outcome = verified.verification.receipt.programOutcome;
assert(outcome);
const payload = Buffer.from(document.terminal_payload, "hex"), graph = Buffer.from(document.call_graph, "hex");
await assert.rejects(decodeAndVerifyProgramTerminal(payload, graph, document.program_id, { ...outcome, resultCode: 1 }, 3), /candidate response code/);
const changedReceipt = Buffer.from(document.receipt, "hex"); changedReceipt[changedReceipt.length - 1]! ^= 1;
await assert.rejects(verifyProgramReceipt({ ...document, receipt: changedReceipt.toString("hex") }, authority, trust));

const appliedDomain = Buffer.from("LXP/programs/terminal-applied-legs/v1\0");
const authorityDomain = Buffer.from("LXP/program-execution-with-transfer-authority/v2\0");
const boundDomain = Buffer.from("LayerX/programs/402LXP/account-bound-set/v1\0");
assert(payload.subarray(0, appliedDomain.length).equals(appliedDomain));
const detailLength = payload.readUInt32BE(appliedDomain.length);
const detail = payload.subarray(appliedDomain.length + 4, appliedDomain.length + 4 + detailLength);
assert(detail.subarray(0, authorityDomain.length).equals(authorityDomain));
const executionLength = detail.readUInt32BE(authorityDomain.length);
const authorizationOffset = authorityDomain.length + 4 + executionLength;
const authorizationLength = detail.readUInt32BE(authorizationOffset);
const authorization = detail.subarray(authorizationOffset + 4, authorizationOffset + 4 + authorizationLength);
assert(authorization.subarray(0, boundDomain.length).equals(boundDomain));
const originalLength = authorization.readUInt32BE(boundDomain.length);
const original = authorization.subarray(boundDomain.length + 4, boundDomain.length + 4 + originalLength);
const names = authorization.subarray(boundDomain.length + 4 + originalLength);
const nameLength = names.readUInt16BE(0);
const name = names.subarray(2, 2 + nameLength);
assert.equal(names.length, nameLength + 2);
assert(name.toString("ascii").endsWith(":main"));
const u32 = (length: number): Buffer => { const bytes = Buffer.alloc(4); bytes.writeUInt32BE(length); return bytes; };
const named = (value: Buffer): Buffer => { const size = Buffer.alloc(2); size.writeUInt16BE(value.length); return Buffer.concat([size, value]); };
const wrapped = (inner: Buffer, suffix: Buffer): Buffer => Buffer.concat([boundDomain, u32(inner.length), inner, suffix]);
const mutate = (value: Buffer): Buffer => {
  const nextDetail = Buffer.concat([detail.subarray(0, authorizationOffset), u32(value.length), value, detail.subarray(authorizationOffset + 4 + authorizationLength)]);
  return Buffer.concat([appliedDomain, u32(nextDetail.length), nextDetail, payload.subarray(appliedDomain.length + 4 + detailLength)]);
};
const wrongOwner = Buffer.from(name); wrongOwner[6] = wrongOwner[6] === 97 ? 98 : 97;
const invalidNames = [Buffer.alloc(0), Buffer.alloc(513, 97), Buffer.from(name.toString().toUpperCase()), wrongOwner,
  Buffer.from(name.toString().replace(":main", ":asset:" + "00".repeat(32))), Buffer.from("agent::main")];
const malformed = invalidNames.map((value) => wrapped(original, named(value)));
const legacy = Buffer.from(original);
const transferDomain = Buffer.from("LayerX/programs/402LXP/transfer-set/v2\0");
assert(legacy.subarray(0, transferDomain.length).equals(transferDomain));
legacy[transferDomain.length - 2] = 49;
malformed.push(wrapped(legacy, names));
malformed.push(wrapped(authorization, names), wrapped(original, Buffer.concat([names, Buffer.of(0)])), wrapped(original, names.subarray(0, -1)));
for (const value of malformed) {
  const terminal = mutate(value);
  await assert.rejects(decodeAndVerifyProgramTerminal(terminal, graph, document.program_id,
    { ...outcome, terminalPayloadRoot: createHash("sha256").update(terminal).digest() }, 3));
}
console.log("native account-bound Programs receipt and malformed authority checks passed");
