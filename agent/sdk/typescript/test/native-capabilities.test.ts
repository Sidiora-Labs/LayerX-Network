import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { decodeNativeCapabilitySet, encodeNativeCapabilitySet, narrowNativeCapabilitySet, deriveNativeProgramAccount, type NativeCapability } from "../src/native-capabilities.js";

type LogicalGrant = Readonly<Record<string, string | number>>;
function logical(grants: readonly LogicalGrant[]): readonly NativeCapability[] {
  const bytes = (grant: LogicalGrant, name: string) => { const value = grant[name]; assert.equal(typeof value, "string"); assert.match(String(value), /^(?:[0-9a-f]{2})*$/u); return Buffer.from(String(value), "hex"); };
  return grants.map((grant): NativeCapability => {
    switch (grant.kind) {
      case "StorageRead": return { kind: "storage_read" };
      case "StorageWrite": return { kind: "storage_write" };
      case "EmitEvent": return { kind: "emit_event" };
      case "Call": return { kind: "call", program: bytes(grant, "program") };
      case "Transfer402": return { kind: "transfer402", asset: bytes(grant, "asset"), to: bytes(grant, "to"), maximumAmount: BigInt(grant.maximum_amount!) };
      case "ProgramSpend": return { kind: "program_spend", ownerProgram: bytes(grant, "owner_program"), seed: bytes(grant, "seed"), sourceAccount: bytes(grant, "source_account"), asset: bytes(grant, "asset"), to: bytes(grant, "to"), maximumAmount: BigInt(grant.maximum_amount!) };
      case "ReceiptRead": return { kind: "receipt_read", receiptDigest: bytes(grant, "receipt_digest") };
      case "BalanceView": return { kind: "balance_view", account: bytes(grant, "account"), asset: bytes(grant, "asset"), receiptDigest: bytes(grant, "receipt_digest") };
      case "SharedStorageRead": return { kind: "shared_storage_read" };
      case "SharedStorageWrite": return { kind: "shared_storage_write" };
      default: throw new TypeError("unknown fixture grant");
    }
  });
}
const fixture = JSON.parse(readFileSync(new URL("../../../../../platform/sdk/conformance/fixtures/native-program-capabilities-v2.json", import.meta.url), "utf8")) as {
  capabilities: LogicalGrant[]; canonical_hex: string; narrowed_capabilities: LogicalGrant[]; narrowed_hex: string; equal_narrowing_accepted: boolean;
  escalation_cases: { parent: string; capabilities: LogicalGrant[]; canonical_hex: string; accepted: boolean }[];
};
const all = logical(fixture.capabilities), narrowed = logical(fixture.narrowed_capabilities);
assert.deepEqual(fixture.capabilities.map((grant) => grant.tag), [1,2,3,4,5,9,6,10,7,8]);
assert.equal(Buffer.from(await encodeNativeCapabilitySet(all)).toString("hex"), fixture.canonical_hex);
assert.equal(Buffer.from(await encodeNativeCapabilitySet(narrowed)).toString("hex"), fixture.narrowed_hex);
assert.equal(Buffer.from(await encodeNativeCapabilitySet(await narrowNativeCapabilitySet(all, narrowed))).toString("hex"), fixture.narrowed_hex);
assert.equal(fixture.equal_narrowing_accepted, true);
assert.equal(Buffer.from(await encodeNativeCapabilitySet(await narrowNativeCapabilitySet(all, all))).toString("hex"), fixture.canonical_hex);
assert.equal(fixture.escalation_cases.length, 3);
for (const entry of fixture.escalation_cases) {
  assert.equal(entry.parent, "narrowed"); assert.equal(entry.accepted, false);
  const child = logical(entry.capabilities);
  assert.equal(Buffer.from(await encodeNativeCapabilitySet(child)).toString("hex"), entry.canonical_hex);
  await assert.rejects(narrowNativeCapabilitySet(narrowed, child));
}
const encoded = Buffer.from(fixture.canonical_hex, "hex");
assert.equal(Buffer.from(await encodeNativeCapabilitySet(await decodeNativeCapabilitySet(encoded))).toString("hex"), fixture.canonical_hex);
for (let length = 0; length < encoded.length; length++) await assert.rejects(decodeNativeCapabilitySet(encoded.subarray(0, length)));
await assert.rejects(decodeNativeCapabilitySet(Buffer.concat([encoded, Buffer.from([0])])));
const unknown = Buffer.from(encoded); unknown[2] = 11; await assert.rejects(decodeNativeCapabilitySet(unknown));
const reordered = Buffer.from(encoded); reordered[2] = 2; reordered[3] = 1; await assert.rejects(decodeNativeCapabilitySet(reordered));
await assert.rejects(encodeNativeCapabilitySet([...all, all[0]!]));
const spend = all.find((grant) => grant.kind === "program_spend"); assert(spend?.kind === "program_spend");
const maximum: NativeCapability[] = [];
for (let index = 0; index < 238; index++) {
  const seed = new Uint8Array(128); new DataView(seed.buffer).setUint16(0, index);
  maximum.push({ ...spend, seed, sourceAccount: await deriveNativeProgramAccount(spend.ownerProgram, seed) });
}
assert.equal((await encodeNativeCapabilitySet(maximum)).length, 65_452);
await assert.rejects(encodeNativeCapabilitySet([...maximum, { kind: "call", program: new Uint8Array(32).fill(1) }]));
const view = all.find((grant) => grant.kind === "balance_view"); assert(view?.kind === "balance_view");
const views = Array.from({ length: 32 }, (_, index) => ({ ...view, account: new Uint8Array(32).fill(index + 1) }));
assert.equal((await decodeNativeCapabilitySet(await encodeNativeCapabilitySet(views))).length, 32);
await assert.rejects(encodeNativeCapabilitySet([...views, { ...view, account: new Uint8Array(32).fill(33) }]));
for (const grant of all) {
  if (grant.kind === "program_spend") {
    const sourceAccount = new Uint8Array(grant.sourceAccount); sourceAccount[0] = sourceAccount[0]! ^ 1;
    await assert.rejects(encodeNativeCapabilitySet([{ ...grant, sourceAccount }]));
    await assert.rejects(encodeNativeCapabilitySet([{ ...grant, seed: new Uint8Array(129) }]));
  }
  if (grant.kind === "transfer402") await assert.rejects(encodeNativeCapabilitySet([{ ...grant, maximumAmount: 0n }]));
  if (grant.kind === "balance_view") await assert.rejects(encodeNativeCapabilitySet([{ ...grant, receiptDigest: new Uint8Array(32) }]));
}
