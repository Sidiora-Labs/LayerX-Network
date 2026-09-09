import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { decodeReceive, encodeReceive, encodeGrant, grantAuthorizationMessage, receiveAuthorizationMessage } from "../../../../agent/sdk/typescript/dist/src/x402/receive.js";

const fixture = readFileSync(new URL("receive.hex", import.meta.url), "utf8").trim().split("\n");
const wire = Uint8Array.from(Buffer.from(fixture[0], "hex"));
const receive = decodeReceive(wire);
const hex = (bytes) => Buffer.from(bytes).toString("hex");

test("native canonical grant, receive and signing preimages", () => {
  assert.equal(hex(encodeReceive(receive)), fixture[0]);
  assert.equal(hex(grantAuthorizationMessage(receive.payer_grant)), fixture[1]);
  assert.equal(hex(receiveAuthorizationMessage(receive)), fixture[2]);
  assert.equal(hex(encodeGrant(receive.payer_grant)), hex(wire.slice(387)));
  assert.equal(receive.amount, ((1n << 64n) + 25n).toString());
  assert.equal(receive.receiver_sequence, ((1n << 64n) - 1n).toString());
});

test("all truncations, trailing data, tags and noncanonical booleans refused", () => {
  for (let length = 0; length < wire.length; length++) assert.throws(() => decodeReceive(wire.slice(0, length)));
  assert.throws(() => decodeReceive(new Uint8Array([...wire, 0])));
  for (const offset of [0, 1, 2, 3, 547, 596]) {
    const invalid = wire.slice();
    invalid[offset] = [547, 596].includes(offset) ? 2 : invalid[offset] ^ 255;
    assert.throws(() => decodeReceive(invalid));
  }
});

test("integer bounds, types and exact fields", () => {
  for (const amount of ["-1", "01", "1.0", "", "١", (1n << 128n).toString(), 1, true]) assert.throws(() => encodeReceive({ ...receive, amount }));
  for (const receiver_sequence of [(1n << 64n).toString(), "-1", "01", 1]) assert.throws(() => encodeReceive({ ...receive, receiver_sequence }));
  for (const recurring of [0, 1, "true", null]) assert.throws(() => encodeReceive({ ...receive, payer_grant: { ...receive.payer_grant, recurring } }));
  for (const asset of ["AB".repeat(32), "0x" + "00".repeat(32), "00".repeat(31), "gg".repeat(32)]) assert.throws(() => encodeReceive({ ...receive, asset }));
  assert.throws(() => encodeReceive({ ...receive, unknown: 1 }));
  const { asset, ...missing } = receive;
  assert.throws(() => encodeReceive(missing));
  const maximum = { ...receive, amount: ((1n << 128n) - 1n).toString() };
  assert.deepEqual(decodeReceive(encodeReceive(maximum)), maximum);
});
