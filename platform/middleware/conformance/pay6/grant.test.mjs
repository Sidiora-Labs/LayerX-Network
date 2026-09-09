import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { validateGrantDraw, decodeReceive } from "@sidiora/layerx-sdk";
import { encodePaymentRequiredHeader } from "@sidiora/layerx-seller-middleware";
const wire = Uint8Array.from(Buffer.from(readFileSync(new URL("receive.hex", import.meta.url), "utf8").split("\n")[0], "hex"));
const r = decodeReceive(wire);
const offer = { scheme: "subscription", network: "layerx:testnet", asset: r.asset, amount: r.amount, payTo: r.to, maxTimeoutSeconds: 30, extra: { layerx: { commitment: "executed", purposeHash: r.payer_grant.purpose_hash, payer: r.from, windowSeconds: "3600" } } };
test("native signed receive matches subscription offer without authorizing fulfillment", () => {
  assert.deepEqual(validateGrantDraw(wire, offer, r.idempotency_key, 7, 0n), r);
  for (const change of [{ amount: "1" }, { asset: "ab".repeat(32) }, { payTo: "ab".repeat(32) }, { scheme: "metered" }]) assert.throws(() => validateGrantDraw(wire, { ...offer, ...change }, r.idempotency_key, 7, 0n));
  assert.throws(() => validateGrantDraw(wire, offer, "00".repeat(32), 7, 0n));
  assert.throws(() => validateGrantDraw(wire, offer, r.idempotency_key, 8, 0n));
  assert.throws(() => validateGrantDraw(wire, offer, r.idempotency_key, 7, BigInt(r.payer_grant.expiration)));
});
test("offer parsing requires explicit grant terms and rejects unknown schemes", () => {
  const header = value => encodePaymentRequiredHeader({ x402Version: 2, resource: { url: "https://example.com/paid" }, accepts: [value] });
  header(offer);
  for (const change of [
    { scheme: "unknown" },
    { extra: {} },
    { extra: { layerx: { ...offer.extra.layerx, windowSeconds: "0" } } },
    { extra: { layerx: { commitment: "executed", payer: r.from, windowSeconds: "3600" } } },
    { extra: { layerx: { purposeHash: r.payer_grant.purpose_hash, payer: r.from, windowSeconds: "3600" } } },
    { extra: { layerx: { commitment: "executed", purposeHash: r.payer_grant.purpose_hash, windowSeconds: "3600" } } },
    { extra: { layerx: { ...offer.extra.layerx, payer: "00".repeat(32) } } },
    { extra: { layerx: { ...offer.extra.layerx, purposeHash: "00".repeat(32) } } },
  ]) assert.throws(() => header({ ...offer, ...change }));
});

test("buyer grant header preserves the selected terms and native idempotency key", async () => {
  const { grantPaymentHeader } = await import("../../buyer/dist/index.js");
  const required = { x402Version: 2, resource: { url: "https://example.com/paid" }, accepts: [offer] };
  const payload = JSON.parse(Buffer.from(grantPaymentHeader(required, offer, Buffer.from(wire).toString("hex")), "base64"));
  assert.deepEqual(payload.accepted, offer);
  assert.equal(payload.payload.idempotencyKey, r.idempotency_key);
  assert.throws(() => grantPaymentHeader(required, { ...offer, amount: "1" }, Buffer.from(wire).toString("hex")));
});
