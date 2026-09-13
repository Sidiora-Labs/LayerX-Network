import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import {
  decodeProgramReceiptOutcome,
  ReceiptVerificationError,
  verifyReceipt,
  type AuthorizedReceiptBatch,
} from "../src/verifier.js";
import { ReceiptFailureCode } from "../src/generated/receipt.js";

const read = (path: string): Buffer => readFileSync(new URL(`../../../../../${path}`, import.meta.url));
const hash = (...parts: readonly (string | Uint8Array)[]): Buffer => {
  const value = createHash("sha256");
  for (const part of parts) value.update(part);
  return value.digest();
};

function fields(canonical: Buffer, publicKey: Uint8Array) {
  let offset = 6;
  const bounded = (size?: number) => {
    const length = canonical.readUInt32BE(offset);
    if (size !== undefined) assert.equal(length, size);
    offset += 4;
    const value = canonical.subarray(offset, offset + length);
    assert.equal(value.length, length);
    offset += length;
    return value;
  };
  const activity = bounded(32);
  offset += 8;
  const previousStateRoot = bounded(32), resultingStateRoot = bounded(32);
  bounded(32);
  const result = offset;
  offset += 4;
  const count = canonical.readUInt32BE(offset);
  offset += 4;
  for (let index = 0; index < count; index++) {
    offset += 8;
    bounded(32);
    bounded();
  }
  offset += 16;
  const batchId = bounded(32), moduleId = offset;
  offset += 10;
  const operation = offset++;
  const asset = bounded(32), amount = offset;
  offset += 16;
  bounded(32);
  offset += 40;
  bounded(32);
  offset += 32;
  bounded(32);
  bounded(32);
  bounded(32);
  offset += 8;
  return {
    activity, result, moduleId, operation, amount, extension: offset,
    authority: { batchId, asset, previousStateRoot, resultingStateRoot, sequencerPublicKey: publicKey } satisfies AuthorizedReceiptBatch,
  };
}

const source = "tests/fixtures/asset/daemon-send-supply";
const canonical = read(`${source}/receipt`), publicKey = read(`${source}/sequencer.public`);
const expected = JSON.parse(read(`${source}/expected.json`).toString("utf8")) as Record<string, string>;
const layout = fields(canonical, publicKey), authority = layout.authority;
const verify = (bytes: Uint8Array, trusted = authority) => verifyReceipt(bytes, trusted, { protocolVersion: 3 });
const verified = await verify(canonical), receipt = verified.receipt;
assert.equal(canonical.readUInt16BE(2), 0x5202);
assert.equal(receipt.protocolVersion, 3);
assert.deepEqual([receipt.moduleId, receipt.operation, receipt.resultCode], [1, 5, 0]);
assert.equal(Buffer.from(receipt.activityId).toString("hex"), expected.activity_id);
assert.equal(Buffer.from(receipt.from).toString("hex"), expected.source_account);
assert.equal(Buffer.from(receipt.to).toString("hex"), expected.destination_account);
assert.equal(Buffer.from(receipt.asset).toString("hex"), expected.asset);
assert.equal(receipt.amount, 1n);
assert.equal(receipt.feeCharged, 4n);
assert.equal(receipt.fromSequence, BigInt(expected.source_sequence_before!));
assert.equal(receipt.fromBalanceBefore + receipt.feeCharged, BigInt(expected.source_balance_before!));
assert.equal(receipt.fromBalanceAfter + receipt.amount, receipt.fromBalanceBefore);
assert.equal(receipt.toBalanceBefore, BigInt(expected.destination_balance_before!));
assert.equal(receipt.toBalanceAfter, receipt.toBalanceBefore + receipt.amount);
assert.deepEqual(receipt.totalUnits, [0n, 0n]);
assert(Object.isFrozen(receipt.totalUnits));
assert.equal(receipt.programOutcome, undefined);
assert.deepEqual(Buffer.from(verified.receiptDigest), hash("LXP/v1/receipt\0", canonical.subarray(0, -69), Buffer.of(0)));
await assert.rejects(verifyReceipt(canonical, authority));
for (const field of ["batchId", "asset", "previousStateRoot", "resultingStateRoot", "sequencerPublicKey"] as const) {
  await assert.rejects(verify(canonical, { ...authority, [field]: Buffer.alloc(32) }));
}
for (let size = 0; size < canonical.length; size++) await assert.rejects(verify(canonical.subarray(0, size)));
await assert.rejects(verify(Buffer.concat([canonical, Buffer.of(0)])));
for (const offset of [0, 1, 2, 3, 4, 5, layout.result, layout.moduleId, layout.operation, layout.amount, layout.extension + 15, layout.extension + 31, canonical.length - 1]) {
  const changed = Buffer.from(canonical);
  changed[offset] = changed[offset]! ^ 1;
  await assert.rejects(verify(changed));
}
const equalSupply = Buffer.from(canonical);
equalSupply[layout.extension + 15] = 1;
equalSupply[layout.extension + 31] = 1;
const failsAt = (code: ReceiptFailureCode) => (error: unknown): boolean => error instanceof ReceiptVerificationError && error.check === code;
await assert.rejects(verify(equalSupply), failsAt(ReceiptFailureCode.SequencerSignature));
const downgraded = Buffer.concat([canonical.subarray(0, layout.extension), canonical.subarray(layout.extension + 32)]);
downgraded[3] = 1;
await assert.rejects(verify(downgraded), failsAt(ReceiptFailureCode.SequencerSignature));
function supplyChange(operation: number, before: bigint, after: bigint, amount = 1n): Buffer {
  const bytes = Buffer.from(canonical);
  const u128 = (offset: number, value: bigint) => {
    bytes.writeBigUInt64BE(value >> 64n, offset);
    bytes.writeBigUInt64BE(value & ((1n << 64n) - 1n), offset + 8);
  };
  bytes[layout.operation] = operation;
  u128(layout.extension, before);
  u128(layout.extension + 16, after);
  u128(layout.amount, amount);
  return bytes;
}
for (const operation of [1, 2, 3, 4, 6, 7, 8]) {
  await assert.rejects(verify(supplyChange(operation, 0n, 0n)), failsAt(ReceiptFailureCode.SequencerSignature));
}
for (const [operation, before, after, amount] of [
  [1, 1n, 0n, 1n], [1, 0n, 1n, 1n],
  ...[2, 3, 4, 5, 6, 7, 8].map(operation => [operation, 1n, 2n, 1n] as const),
  [0, 0n, 0n, 1n], [9, 0n, 0n, 1n], [12, 0n, 0n, 1n], [255, 0n, 0n, 1n],
  [10, 0n, 0n, 0n], [11, 0n, 0n, 0n],
  [10, (1n << 128n) - 1n, 0n, 1n], [11, 0n, (1n << 128n) - 1n, 1n],
  [10, 2n, 2n, 1n], [11, 2n, 2n, 1n],
] as const) {
  await assert.rejects(verify(supplyChange(operation, before, after, amount)), failsAt(ReceiptFailureCode.CanonicalEncoding));
}
await assert.rejects(verify(supplyChange(10, 2n, 3n)), failsAt(ReceiptFailureCode.SequencerSignature));
await assert.rejects(verify(supplyChange(11, 2n, 1n)), failsAt(ReceiptFailureCode.SequencerSignature));

const legacy = JSON.parse(read("platform/sdk/conformance/fixtures/receipt-positive-v2.json").toString("utf8")) as {
  canonical_receipt_hex: string;
  authorized_batch: { sequencer_public_key_hex: string };
};
for (const [bytes, units] of [
  [read("tests/fixtures/receipt-supply-v2.bin"), [1_000_000n, 1_000_000n]],
  [Buffer.from(legacy.canonical_receipt_hex, "hex"), undefined],
] as const) {
  const fixtureAuthority = fields(bytes, Buffer.from(legacy.authorized_batch.sequencer_public_key_hex, "hex")).authority;
  const result = await verifyReceipt(bytes, fixtureAuthority);
  assert.equal(result.receipt.amount, 25_000n);
  assert.deepEqual(result.receipt.totalUnits, units);
}

for (const index of [0, 1]) {
  const directory = "tests/fixtures/programs/maintained-multicall";
  const bytes = read(`${directory}/receipt-${index}`);
  const selected = fields(bytes, read(`${directory}/sequencer.public`));
  const result = await verifyReceipt(bytes, selected.authority, { protocolVersion: 3 });
  const actual = result.receipt, outcome = actual.programOutcome;
  assert.equal(bytes.readUInt16BE(2), 0x5201);
  assert.equal(actual.totalUnits, undefined);
  assert.deepEqual([actual.protocolVersion, actual.moduleId, actual.moduleVersion, actual.operation, actual.resultCode], [3, 9, 4, 3, 0]);
  assert.deepEqual(Buffer.from(actual.activityId), hash("LXP/v1/activity-id\0", read(`${directory}/activity-${index}`)));
  assert(outcome);
  assert.equal(outcome.encodingVersion, 4);
  assert.equal(outcome.terminalKind, 1);
  assert.equal(outcome.resultCode, actual.resultCode);
  assert.equal(outcome.cpuFuel, 623n);
  assert.equal(outcome.memoryBytes, 65_536n);
  assert.equal(outcome.feeUnits, 66_160n);
  assert.deepEqual(outcome.feeSchedulePrices, [1n, 1n, 2n, 4n, 1n, 1n, 1n]);
  assert.deepEqual(outcome.transferRoot, actual.transferSetRoot);
  assert.deepEqual(Buffer.from(outcome.appliedLegsDigest), hash(new Uint8Array()));
  const encoded = bytes.subarray(selected.extension, -69);
  assert.deepEqual(decodeProgramReceiptOutcome(encoded, 3), outcome);
  for (let size = 0; size < encoded.length; size++) assert.throws(() => decodeProgramReceiptOutcome(encoded.subarray(0, size), 3));
  assert.throws(() => decodeProgramReceiptOutcome(Buffer.concat([encoded, Buffer.of(0)]), 3));
  for (const protocol of [1, 2, 4]) assert.throws(() => decodeProgramReceiptOutcome(encoded, protocol));
  for (const offset of [selected.result + 3, selected.extension + 8, bytes.length - 70, bytes.length - 1]) {
    const changed = Buffer.from(bytes);
    changed[offset] = changed[offset]! ^ 1;
    await assert.rejects(verifyReceipt(changed, selected.authority, { protocolVersion: 3 }));
  }
}
console.log("native supply SEND and maintained Programs receipts verify without changing signed bytes");
