import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash, createPublicKey, verify } from "node:crypto";
import { verifyProgramLifecycleReceipt } from "../src/verifier.js";
import { encodeProgramMutationBody, decodeProgramBoundaryError, decodeEnvelope } from "../src/agent-http.js";
import { bindSignedProgramLifecycle } from "../src/program-wire.js";
import { verifyLifecycleRecovery, resolveLifecycleResponse, resolveLifecycleFailure } from "../src/programs.js";
import { decodeNativeProgramDeploy, decodeNativeProgramUpgrade, decodeNativeProgramWindDown,
  encodeNativeProgramDeploy, encodeNativeProgramUpgrade, encodeNativeProgramWindDown, NativeProgramLifecycleRequest } from "../src/program-lifecycle.js";

for (const name of ["deploy", "upgrade", "wind-down-route", "wind-down-deprecate", "wind-down-tombstone", "wind-down-exit"]) {
  const fixture = JSON.parse(readFileSync(new URL(`../../../../../platform/sdk/conformance/fixtures/native-program-${name}-v3.json`, import.meta.url), "utf8")) as Record<string, string>;
  const payload = Buffer.from(fixture.payload_hex!, "hex"), signed = Buffer.from(fixture.signed_activity_hex!, "hex");
  assert.deepEqual(encodeProgramMutationBody(fixture.signed_activity_hex), signed);
  assert.throws(() => encodeProgramMutationBody({ activity: fixture.signed_activity_hex }));
  assert.throws(() => encodeProgramMutationBody(fixture.signed_activity_hex!.toUpperCase()));
  const unsigned = Buffer.from(signed.subarray(0, -69)); unsigned[4] = 11;
  const preimage = createHash("sha256").update(Buffer.from("LXP/v1/signature-preimage\0")).update(unsigned).digest();
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), Buffer.from(fixture.public_key_hex!, "hex")]), format: "der", type: "spki" });
  assert.equal(verify(null, preimage, publicKey, signed.subarray(-64)), true);
  const decode = name === "deploy" ? (bytes: Uint8Array) => encodeNativeProgramDeploy(decodeNativeProgramDeploy(bytes))
    : name === "upgrade" ? (bytes: Uint8Array) => encodeNativeProgramUpgrade(decodeNativeProgramUpgrade(bytes))
    : (bytes: Uint8Array) => encodeNativeProgramWindDown(decodeNativeProgramWindDown(bytes));
  assert.deepEqual(Buffer.from(decode(payload)), payload);
  const request = name === "deploy" ? NativeProgramLifecycleRequest.deploy(decodeNativeProgramDeploy(payload), signed)
    : name === "upgrade" ? NativeProgramLifecycleRequest.upgrade(decodeNativeProgramUpgrade(payload), signed)
    : NativeProgramLifecycleRequest.windDown(decodeNativeProgramWindDown(payload), signed);
  assert.equal((await request.bind(fixture.idempotency_key_hex)).activityId, fixture.activity_id_hex);
  const ordinal = name === "deploy" ? 1 : name === "upgrade" ? 2 : 7;
  const binding = await request.bind();
  const expectedUnknown = { state: "unknown", activity_id: fixture.activity_id_hex, idempotency_key: fixture.idempotency_key_hex, retained_signed_activity: fixture.signed_activity_hex };
  const receiptFixture = JSON.parse(readFileSync(new URL("../../../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v3.json", import.meta.url), "utf8"));
  const damagedReceipt = Buffer.from(receiptFixture.canonical_receipt_hex, "hex");
  damagedReceipt[damagedReceipt.length - 1] = damagedReceipt[damagedReceipt.length - 1]! ^ 1;
  for (const response of [
    { state: "executed", activity_id: fixture.activity_id_hex, receipt: damagedReceipt.toString("hex"), terminal_payload: "", call_graph: "" },
    { state: "refused", activity_id: fixture.activity_id_hex, receipt: "00", terminal_payload: "", call_graph: "" },
    { state: "unknown", activity_id: "00".repeat(32), retry: "after", retry_after_seconds: 2 },
  ]) {
    const value = decodeEnvelope(200, Buffer.from(JSON.stringify({ result: response })), "program.deploy");
    assert.deepEqual(await resolveLifecycleResponse(value, binding, Buffer.from(receiptFixture.authorized_batch.sequencer_public_key_hex, "hex")), expectedUnknown);
  }
  for (const encoded of [Buffer.from('{"result":'), Buffer.from(JSON.stringify({ result: {}, extra: true }))]) {
    let caught: unknown;
    try { decodeEnvelope(200, encoded, "program.deploy"); } catch (error) { caught = error; }
    assert.notEqual(caught, undefined);
    assert.deepEqual(resolveLifecycleFailure(caught, binding), expectedUnknown);
  }
  const refusalError = decodeProgramBoundaryError(400, { code: "invalid_program_payload", retry: "never" });
  assert.throws(() => resolveLifecycleFailure(refusalError, binding), (error) => error === refusalError);
  const localError = new TypeError("invalid signed payload");
  assert.throws(() => resolveLifecycleFailure(localError, binding), (error) => error === localError);
  const hashOffset = signed.length - 69 - payload.length - 5 - 32;
  const badHash = Buffer.from(signed); badHash[hashOffset] = badHash[hashOffset]! ^ 1;
  await assert.rejects(bindSignedProgramLifecycle(badHash, payload, ordinal));
  if (name === "deploy") {
    for (const size of [524_288, 524_289]) {
      const original = decodeNativeProgramDeploy(payload);
      const largeWasm = Buffer.concat([original.wasm, Buffer.alloc(size - payload.length)]);
      const largePayload = Buffer.from(encodeNativeProgramDeploy({ ...original, wasm: largeWasm, newHash: createHash("sha256").update(largeWasm).digest() }));
      assert.equal(largePayload.length, size);
      const length = Buffer.alloc(4); length.writeUInt32BE(size);
      const mutated = Buffer.concat([signed.subarray(0, hashOffset), createHash("sha256").update(Buffer.from("LXP/v1/payload-hash\0")).update(largePayload).digest(), Buffer.from([11]), length, largePayload, signed.subarray(-69)]);
      if (size === 524_288) assert.equal((await bindSignedProgramLifecycle(mutated, undefined, ordinal)).idempotencyKey, fixture.idempotency_key_hex);
      else await assert.rejects(bindSignedProgramLifecycle(mutated, undefined, ordinal));
    }
  }
  assert.equal((await bindSignedProgramLifecycle(signed, undefined, ordinal, fixture.idempotency_key_hex)).activityId, fixture.activity_id_hex);
  await assert.rejects(bindSignedProgramLifecycle(signed, undefined, ordinal, "00".repeat(32)));
  await assert.rejects(bindSignedProgramLifecycle(signed, undefined, ordinal === 1 ? 2 : 1, fixture.idempotency_key_hex));
  const beforeMutation = Buffer.from(signed); signed[0] = signed[0]! ^ 1;
  assert.equal((await request.bind()).activityId, fixture.activity_id_hex);
  signed.set(beforeMutation);
  const mutable = Buffer.from(signed), pending = bindSignedProgramLifecycle(mutable, undefined, ordinal);
  mutable.fill(0);
  assert.equal((await pending).activityId, fixture.activity_id_hex);
  await assert.rejects(request.bind("00".repeat(32)));
  for (let length = 0; length < payload.length; length++) assert.throws(() => decode(payload.subarray(0, length)));
  assert.throws(() => decode(Buffer.concat([payload, Buffer.from([0])])));
  if (name === "deploy" || name === "upgrade") {
    for (const offset of [35, 68]) { const changed = Buffer.from(payload); changed[offset] = changed[offset]! ^ 1; assert.throws(() => decode(changed)); }
  }
  const changed = Buffer.from(signed); changed[1] = 2;
  const wrong = name === "deploy" ? NativeProgramLifecycleRequest.deploy(decodeNativeProgramDeploy(payload), changed)
    : name === "upgrade" ? NativeProgramLifecycleRequest.upgrade(decodeNativeProgramUpgrade(payload), changed)
    : NativeProgramLifecycleRequest.windDown(decodeNativeProgramWindDown(payload), changed);
  await assert.rejects(wrong.bind());
}
const wasm = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]), programId = Buffer.alloc(32, 1), newHash = createHash("sha256").update(wasm).digest();
const refusal = decodeProgramBoundaryError(400, { code: "invalid_program_payload", retry: "never" });
assert.equal(refusal.boundaryCode, "invalid_program_payload");
assert.equal(refusal.retry, "never");
assert.equal(decodeProgramBoundaryError(503, { code: "node_unavailable", retry: "after", retry_after_seconds: 2 }).retryAfterMs, 2000);
assert.throws(() => decodeProgramBoundaryError(200, { code: "invalid_program_payload", retry: "never" }));
assert.throws(() => decodeProgramBoundaryError(400, { code: "invalid_program_payload", retry: "never", extra: 0 }));
assert.throws(() => decodeProgramBoundaryError(503, { code: "node_unavailable", retry: "after", retry_after_seconds: true }));
const callReceipt = JSON.parse(readFileSync(new URL("../../../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v3.json", import.meta.url), "utf8")) as { canonical_receipt_hex: string; authorized_batch: { sequencer_public_key_hex: string } };
await assert.rejects(verifyProgramLifecycleReceipt(Buffer.from(callReceipt.canonical_receipt_hex, "hex"), Buffer.alloc(32, 0x41), Buffer.from(callReceipt.authorized_batch.sequencer_public_key_hex, "hex")));
const recovery = { activity_id: "41".repeat(32), receipt: callReceipt.canonical_receipt_hex };
const sequencer = Buffer.from(callReceipt.authorized_batch.sequencer_public_key_hex, "hex");
await assert.rejects(verifyLifecycleRecovery(recovery, "42".repeat(32), sequencer));
await assert.rejects(verifyLifecycleRecovery({ ...recovery, program_id: "11".repeat(32) }, recovery.activity_id, sequencer));
await assert.rejects(verifyLifecycleRecovery(recovery, recovery.activity_id, sequencer));
assert.throws(() => encodeNativeProgramDeploy({ programId, guestAbi: 2, policy: 0, authority: programId, newHash, wasm }));
assert.throws(() => encodeNativeProgramDeploy({ programId, guestAbi: 2, policy: 0, authority: Buffer.alloc(32), newHash, wasm, interface: Buffer.alloc(953) }));
assert.throws(() => encodeNativeProgramUpgrade({ programId, guestAbi: 2, oldHash: newHash, newHash, wasm, migrationHook: Buffer.alloc(0), clearInterface: true }));
assert.throws(() => encodeNativeProgramWindDown({ programId, operation: "route", account: programId, asset: programId, destination: programId, seed: Buffer.alloc(129) }));
