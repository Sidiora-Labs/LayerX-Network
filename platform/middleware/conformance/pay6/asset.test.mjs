import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { encodeAccountOpen, encodeGrantRevoke, encodeAssetSupply, encodeRegister, nativeAssetId } from "@sidiora/layerx-sdk";

const hex = (bytes) => Buffer.from(bytes).toString("hex");
const actorId = (actor) => {
  const bytes = Buffer.from(actor, "utf8");
  const length = Buffer.alloc(2);
  length.writeUInt16BE(bytes.length);
  return createHash("sha256").update(Buffer.concat([Buffer.from("LXP/v1/did-id\0"), length, bytes])).digest("hex");
};

test("asset payload version, field order and unsigned big-endian bounds", () => {
  assert.equal(hex(encodeAccountOpen("ab".repeat(32))), "0001" + "ab".repeat(32));
  assert.equal(hex(encodeGrantRevoke("ab".repeat(32), "18446744073709551615")), "0001" + "ab".repeat(32) + "ff".repeat(8));
  assert.equal(hex(encodeAssetSupply("ab".repeat(32), "cd".repeat(32), "340282366920938463463374607431768211455")), "0001" + "ab".repeat(32) + "cd".repeat(32) + "ff".repeat(16));
  for (const amount of ["0", "01", "-1", "340282366920938463463374607431768211456", 1, true]) assert.throws(() => encodeAssetSupply("ab".repeat(32), "cd".repeat(32), amount));
  for (const asset of ["AB".repeat(32), "ab".repeat(31), "0x" + "ab".repeat(32)]) assert.throws(() => encodeAccountOpen(asset));
  assert.throws(() => encodeGrantRevoke("ab".repeat(32), "18446744073709551616"));
});

test("kind-1 register derives asset id and matches the signer vector", () => {
  const issuer = actorId("did:layerx:alice");
  const salt = "02".repeat(32);
  const wire = encodeRegister({
    issuer_did_id32: issuer, salt, symbol: "TOK", name: "Token €", decimals: 18,
    supply_cap: "1000000", issuer_kind: 1, custody_ref: "",
  });
  const fixture = readFileSync(new URL("../../../../agent/crates/layerx-crypto/tests/fixtures/payments/1-1.hex", import.meta.url), "utf8").trim();
  assert.equal(hex(wire), fixture);
  assert.equal(hex(wire.slice(2, 34)), nativeAssetId(issuer, salt));
  assert.equal(hex(encodeRegister({
    issuer_did_id32: issuer, salt, symbol: "TOK", name: "Token €", decimals: 18,
    supply_cap: "1000000", issuer_kind: 1, custody_ref: "", asset_id: nativeAssetId(issuer, salt),
  })), fixture);
});

test("register encoder refuses malformed kind, custody, utf-8 and cap values", () => {
  const valid = {
    issuer_did_id32: "ab".repeat(32), salt: "cd".repeat(32), symbol: "TOK", name: "Token",
    decimals: 6, supply_cap: "0", issuer_kind: 1, custody_ref: "",
  };
  for (const change of [
    { symbol: "" }, { symbol: "X".repeat(17) }, { symbol: "T€K" }, { name: "" }, { name: "n".repeat(33) },
    { decimals: 39 }, { issuer_kind: 3 }, { custody_ref: "aa" }, { asset_id: "00".repeat(32) },
    { supply_cap: "01" },
  ]) assert.throws(() => encodeRegister({ ...valid, ...change }));
  const custody = encodeRegister({ ...valid, issuer_kind: 2, asset_id: "11".repeat(32), custody_ref: "aa".repeat(128) });
  assert.equal(hex(custody.slice(2, 34)), "11".repeat(32));
  assert.throws(() => encodeRegister({ ...valid, issuer_kind: 2 }));
});
