import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import {
  decodeProgramReceiptOutcome,
  ReceiptFailureCode,
  ReceiptVerificationError,
  verifyReceipt,
  type AuthorizedReceiptBatch,
  type ReceiptVerification,
} from "../src/index.js";

const PROGRAM_OUTCOME_V3 = "505247330100000000000100010000000700000001000000000000000b000000000000000c000000000000000d000000000000000e00000001000000000000000f0000000000000000000000000000000000000000000000000000000000000000000000000000000100000000000000020000000000000003000000000000000400000000000000050000000000000006000000000000000700000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000020000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000010000000201111111111111111111111111111111111111111111111111111111111111111000000202222222222222222222222222222222222222222222222222222222222222222000000200000000000000000000000000000000000000000000000000000000000000000";

function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

interface ReceiptFixture {
  readonly canonical_receipt_hex: string;
  readonly authorized_batch: {
    readonly batch_id_hex: string;
    readonly asset_hex: string;
    readonly previous_state_root_hex: string;
    readonly resulting_state_root_hex: string;
    readonly sequencer_public_key_hex: string;
  };
  readonly expected: {
    readonly level: string;
    readonly result_code: number;
    readonly protocol_version: number;
    readonly operation: number;
    readonly module_id: number;
    readonly global_sequence: number;
    readonly timestamp_ms: number;
    readonly amount: string;
    readonly fee_charged: string;
    readonly from_balance_before: string;
    readonly from_balance_after: string;
    readonly to_balance_before: string;
    readonly to_balance_after: string;
    readonly activity_id_hex: string;
    readonly from_hex: string;
    readonly to_hex: string;
    readonly receipt_digest_hex: string;
  };
}

interface ReceiptRefusalFixture {
  readonly authorized_batch: ReceiptFixture["authorized_batch"];
  readonly vectors: readonly {
    readonly name: string;
    readonly expected_check: string;
    readonly canonical_receipt_hex: string;
  }[];
}

function hexBytes(value: string): Uint8Array {
  assert(value.length % 2 === 0, "odd hex length");
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    const parsed = Number.parseInt(value.slice(2 * index, 2 * index + 2), 16);
    assert(Number.isInteger(parsed), "invalid hex byte");
    bytes[index] = parsed;
  }
  return bytes;
}

function hexEqual(actual: Uint8Array, expectedHex: string, field: string): void {
  const expected = hexBytes(expectedHex);
  assert(actual.length === expected.length, `${field} length diverged`);
  for (let index = 0; index < actual.length; index += 1) {
    assert(actual[index] === expected[index], `${field} diverged at byte ${index}`);
  }
}

function loadFixture(name = "receipt-positive-v1.json"): ReceiptFixture {
  const path = fileURLToPath(
    new URL(
      `../../../../../platform/sdk/conformance/fixtures/${name}`,
      import.meta.url,
    ),
  );
  return JSON.parse(readFileSync(path, "utf8")) as ReceiptFixture;
}

function authorizedBatch(fixture: ReceiptFixture): AuthorizedReceiptBatch {
  return {
    batchId: hexBytes(fixture.authorized_batch.batch_id_hex),
    asset: hexBytes(fixture.authorized_batch.asset_hex),
    previousStateRoot: hexBytes(fixture.authorized_batch.previous_state_root_hex),
    resultingStateRoot: hexBytes(fixture.authorized_batch.resulting_state_root_hex),
    sequencerPublicKey: hexBytes(fixture.authorized_batch.sequencer_public_key_hex),
  };
}

export async function verifyReceiptFixture(): Promise<void> {
  const outcome = decodeProgramReceiptOutcome(hexBytes(PROGRAM_OUTCOME_V3), 1);
  assert(outcome.encodingVersion === 3, "program outcome encoding version diverged");
  assert(outcome.abiVersion === 1, "program outcome ABI diverged");
  assert(outcome.feeUnits === 16n, "program outcome fee units diverged");
  hexEqual(outcome.callGraphRoot, "11".repeat(32), "program outcome call graph root");
  hexEqual(outcome.terminalPayloadRoot, "22".repeat(32), "program outcome terminal payload root");
  const fixture = loadFixture();
  const canonical = hexBytes(fixture.canonical_receipt_hex);
  const verified: ReceiptVerification = await verifyReceipt(canonical, authorizedBatch(fixture));
  assert(verified.level === fixture.expected.level, "verification level diverged");
  const receipt = verified.receipt;
  assert(receipt.resultCode === fixture.expected.result_code, "result code diverged");
  assert(receipt.protocolVersion === fixture.expected.protocol_version, "protocol version diverged");
  assert(receipt.operation === fixture.expected.operation, "operation diverged");
  assert(receipt.moduleId === fixture.expected.module_id, "module diverged");
  assert(receipt.globalSequence === BigInt(fixture.expected.global_sequence), "global sequence diverged");
  assert(receipt.timestamp === BigInt(fixture.expected.timestamp_ms), "timestamp diverged");
  assert(receipt.amount === BigInt(fixture.expected.amount), "amount diverged");
  assert(receipt.feeCharged === BigInt(fixture.expected.fee_charged), "fee diverged");
  assert(receipt.fromBalanceBefore === BigInt(fixture.expected.from_balance_before), "from balance before diverged");
  assert(receipt.fromBalanceAfter === BigInt(fixture.expected.from_balance_after), "from balance after diverged");
  assert(receipt.toBalanceBefore === BigInt(fixture.expected.to_balance_before), "to balance before diverged");
  assert(receipt.toBalanceAfter === BigInt(fixture.expected.to_balance_after), "to balance after diverged");
  hexEqual(receipt.activityId, fixture.expected.activity_id_hex, "activity id");
  hexEqual(receipt.from, fixture.expected.from_hex, "from account");
  hexEqual(receipt.to, fixture.expected.to_hex, "to account");
  hexEqual(receipt.batchId, fixture.authorized_batch.batch_id_hex, "batch id");
  hexEqual(receipt.asset, fixture.authorized_batch.asset_hex, "asset");
  hexEqual(receipt.previousStateRoot, fixture.authorized_batch.previous_state_root_hex, "previous state root");
  hexEqual(receipt.resultingStateRoot, fixture.authorized_batch.resulting_state_root_hex, "resulting state root");
  hexEqual(verified.canonicalBytes, fixture.canonical_receipt_hex, "canonical bytes");
  hexEqual(verified.receiptDigest, fixture.expected.receipt_digest_hex, "receipt digest");

  const mutated = new Uint8Array(canonical);
  const lastIndex = mutated.length - 1;
  mutated[lastIndex] = (mutated[lastIndex] ?? 0) ^ 0x01;
  let refused = false;
  try {
    await verifyReceipt(mutated, authorizedBatch(fixture));
  } catch {
    refused = true;
  }
  assert(refused, "mutated receipt verified; a flipped signature byte must fail");

  const programs = loadFixture("receipt-programs-positive-v1.json");
  const verifiedPrograms = await verifyReceipt(
    hexBytes(programs.canonical_receipt_hex),
    authorizedBatch(programs),
  );
  const embedded = verifiedPrograms.receipt.programOutcome;
  assert(embedded !== undefined, "Programs receipt lost its optional outcome");
  assert(embedded.encodingVersion === 3, "embedded outcome version diverged");
  assert(embedded.runtimeVersion === 1, "embedded runtime version diverged");
  assert(embedded.abiVersion === 1, "embedded ABI version diverged");
  assert(embedded.feeUnits === 16n, "embedded fee units diverged");

  const refusals = loadFixture("receipt-refusals-v1.json") as unknown as ReceiptRefusalFixture;
  for (const vector of refusals.vectors) {
    try {
      await verifyReceipt(
        hexBytes(vector.canonical_receipt_hex),
        authorizedBatch(refusals as unknown as ReceiptFixture),
      );
      throw new Error(`${vector.name} verified`);
    } catch (error) {
      assert(error instanceof ReceiptVerificationError, `${vector.name} returned an untyped failure`);
      assert(error.check === (vector.expected_check as ReceiptFailureCode), `${vector.name} taxonomy diverged`);
    }
  }
}

await verifyReceiptFixture();
