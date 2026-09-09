import assert from "node:assert/strict";
import { test } from "node:test";
import { encodeAccountOpen, encodeGrantRevoke, encodeAssetSupply } from "../../../../agent/sdk/typescript/dist/src/x402/receive.js";
const hex = (bytes) => Buffer.from(bytes).toString("hex");
test("asset payload version, field order and unsigned big-endian bounds", () => {
  assert.equal(hex(encodeAccountOpen("ab".repeat(32))), "0001" + "ab".repeat(32));
  assert.equal(hex(encodeGrantRevoke("ab".repeat(32), "18446744073709551615")), "0001" + "ab".repeat(32) + "ff".repeat(8));
  assert.equal(hex(encodeAssetSupply("ab".repeat(32), "cd".repeat(32), "340282366920938463463374607431768211455")), "0001" + "ab".repeat(32) + "cd".repeat(32) + "ff".repeat(16));
  for (const amount of ["0", "01", "-1", "340282366920938463463374607431768211456", 1, true]) assert.throws(() => encodeAssetSupply("ab".repeat(32), "cd".repeat(32), amount));
  for (const asset of ["AB".repeat(32), "ab".repeat(31), "0x" + "ab".repeat(32)]) assert.throws(() => encodeAccountOpen(asset));
  assert.throws(() => encodeGrantRevoke("ab".repeat(32), "18446744073709551616"));
});
