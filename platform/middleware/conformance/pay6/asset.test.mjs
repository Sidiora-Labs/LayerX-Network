import assert from "node:assert/strict";
import { test } from "node:test";
import { deriveNativeAssetId, encodeAccountOpen, encodeAssetRegister, encodeGrantRevoke, encodeAssetSupply } from "../../../../agent/sdk/typescript/dist/src/x402/receive.js";
const hex = (bytes) => Buffer.from(bytes).toString("hex");
const issuer = "1eff10b60dad92693680cd2a6ebf032e48aa1ef8f019e2f51a0362df67755d18";
const salt = "02".repeat(32);
const native = { salt, symbol: "TOK", name: "Token €", decimals: 18, supply_cap: "1000000", issuer_kind: 1, custody_ref: new Uint8Array() };
const asset = "073db0b2bcee1c62c538a3c25482055597708ec6d79708710c5a94c63b2f8aba";
test("asset payload version, field order and unsigned big-endian bounds", () => {
  assert.equal(hex(encodeAccountOpen("ab".repeat(32))), "0001" + "ab".repeat(32));
  assert.equal(hex(encodeGrantRevoke("ab".repeat(32), "18446744073709551615")), "0001" + "ab".repeat(32) + "ff".repeat(8));
  assert.equal(hex(encodeAssetSupply("ab".repeat(32), "cd".repeat(32), "340282366920938463463374607431768211455")), "0001" + "ab".repeat(32) + "cd".repeat(32) + "ff".repeat(16));
  for (const amount of ["0", "01", "-1", "340282366920938463463374607431768211456", 1, true]) assert.throws(() => encodeAssetSupply("ab".repeat(32), "cd".repeat(32), amount));
  for (const asset of ["AB".repeat(32), "ab".repeat(31), "0x" + "ab".repeat(32)]) assert.throws(() => encodeAccountOpen(asset));
  assert.throws(() => encodeGrantRevoke("ab".repeat(32), "18446744073709551616"));
});

test("asset registration derives native ids and matches the version-1 native wire layout", () => {
  assert.equal(deriveNativeAssetId(issuer, salt), asset);
  assert.equal(hex(encodeAssetRegister(issuer, native)), "0001" + asset + salt
    + "03" + Buffer.from("TOK").toString("hex") + "09" + Buffer.from("Token €").toString("hex")
    + "12" + "000000000000000000000000000f4240" + "0100");
  assert.equal(hex(encodeAssetRegister(issuer, { ...native, issuer_kind: 2, asset_id: "ab".repeat(32), custody_ref: Uint8Array.of(1, 2) })),
    "0001" + "ab".repeat(32) + salt + "03" + Buffer.from("TOK").toString("hex")
    + "09" + Buffer.from("Token €").toString("hex") + "12" + "000000000000000000000000000f4240" + "02020102");
});

test("asset registration refuses noncanonical identities, metadata, limits and custody", () => {
  for (const registration of [
    { ...native, asset_id: "ab".repeat(32) },
    { ...native, symbol: "" }, { ...native, symbol: "A".repeat(17) }, { ...native, symbol: "€" },
    { ...native, name: "" }, { ...native, name: "€".repeat(11) }, { ...native, name: "\ud800" },
    { ...native, decimals: -1 }, { ...native, decimals: 39 }, { ...native, decimals: true },
    { ...native, supply_cap: "01" }, { ...native, supply_cap: "340282366920938463463374607431768211456" },
    { ...native, issuer_kind: 0 }, { ...native, issuer_kind: 3 },
    { ...native, custody_ref: Uint8Array.of(1) },
    { ...native, issuer_kind: 2 },
    { ...native, issuer_kind: 2, asset_id: "ab".repeat(32), custody_ref: new Uint8Array(129) },
  ]) assert.throws(() => encodeAssetRegister(issuer, registration));
  for (const identity of ["ab".repeat(31), "AB".repeat(32), "00".repeat(33)]) {
    assert.throws(() => encodeAssetRegister(identity, native));
  }
});
