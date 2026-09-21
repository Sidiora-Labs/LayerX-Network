import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { DEFAULT_PROTOCOL_VERSION, STATE_COMMITMENT_PROTOCOL_VERSION } from "@sidiora/layerx-sdk";
import {
  MiddlewareError,
  ReceiptPayloadAuthority,
  SellerMiddleware,
  requireProtocolVersion,
  verifyPaymentReceipt,
} from "../dist/index.js";

const bytes = (hex) => Uint8Array.from(Buffer.from(hex, "hex"));

async function loadFixture(name) {
  const fixture = JSON.parse(await readFile(new URL(`../../../sdk/conformance/fixtures/${name}`, import.meta.url), "utf8"));
  const batch = fixture.authorized_batch;
  return {
    protocolVersion: fixture.expected.protocol_version,
    evidence: {
      canonicalReceipt: bytes(fixture.canonical_receipt_hex),
      authorizedBatch: {
        batchId: bytes(batch.batch_id_hex),
        asset: bytes(batch.asset_hex),
        previousStateRoot: bytes(batch.previous_state_root_hex),
        resultingStateRoot: bytes(batch.resulting_state_root_hex),
        sequencerPublicKey: bytes(batch.sequencer_public_key_hex),
      },
    },
    requirements: {
      scheme: "exact",
      network: "layerx:beta",
      amount: fixture.expected.amount,
      asset: batch.asset_hex,
      payTo: fixture.expected.to_hex,
      maxTimeoutSeconds: 120,
    },
  };
}

const stateCommitment = await loadFixture("receipt-positive-v3.json");
const legacyDefault = await loadFixture("receipt-positive-v2.json");

class InMemoryFulfillments {
  #entries = new Map();

  async fulfill(proposed, release) {
    const existing = this.#entries.get(proposed.idempotencyKey);
    if (existing !== undefined) return existing;
    const stored = { ...proposed, resource: await release() };
    this.#entries.set(proposed.idempotencyKey, stored);
    return stored;
  }
}

const paymentRequired = (requirements) => ({
  x402Version: 2,
  resource: { url: "https://seller.example/paid" },
  accepts: [requirements],
});

const sellerConfig = (protocolVersion) => ({
  paymentRequired: paymentRequired(stateCommitment.requirements),
  protocolVersion,
  authority: new ReceiptPayloadAuthority({
    resolve: () => Promise.resolve(stateCommitment.evidence.authorizedBatch),
  }),
  fulfillments: new InMemoryFulfillments(),
});

test("the declared protocol version decides which receipts a seller accepts", async () => {
  assert.equal(stateCommitment.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  assert.equal(legacyDefault.protocolVersion, DEFAULT_PROTOCOL_VERSION);

  const verified = await verifyPaymentReceipt(
    stateCommitment.evidence,
    stateCommitment.requirements,
    undefined,
    { protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION },
  );
  assert.equal(verified.level, "sequencer-signed");
  assert.equal(verified.receipt.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);

  const legacy = await verifyPaymentReceipt(
    legacyDefault.evidence,
    legacyDefault.requirements,
    undefined,
    { protocolVersion: DEFAULT_PROTOCOL_VERSION },
  );
  assert.equal(legacy.receipt.protocolVersion, DEFAULT_PROTOCOL_VERSION);
});

test("a receipt of another protocol version is refused under the declared selection", async () => {
  await assert.rejects(
    verifyPaymentReceipt(stateCommitment.evidence, stateCommitment.requirements, undefined, {
      protocolVersion: DEFAULT_PROTOCOL_VERSION,
    }),
    (error) => error instanceof MiddlewareError && error.code === "verification-failure",
  );
  await assert.rejects(
    verifyPaymentReceipt(legacyDefault.evidence, legacyDefault.requirements, undefined, {
      protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION,
    }),
    (error) => error instanceof MiddlewareError && error.code === "verification-failure",
  );
});

test("a seller configuration without a usable protocol version is refused by name", () => {
  for (const declared of [undefined, null, 0, 1, 4, "3", 3.5]) {
    assert.throws(
      () => new SellerMiddleware(sellerConfig(declared)),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
      `declared protocol version ${String(declared)} must be refused by name`,
    );
    assert.throws(
      () => requireProtocolVersion(declared),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
    );
  }
  const seller = new SellerMiddleware(sellerConfig(STATE_COMMITMENT_PROTOCOL_VERSION));
  assert.equal(seller.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  assert.equal(new SellerMiddleware(sellerConfig(DEFAULT_PROTOCOL_VERSION)).protocolVersion, DEFAULT_PROTOCOL_VERSION);
});
