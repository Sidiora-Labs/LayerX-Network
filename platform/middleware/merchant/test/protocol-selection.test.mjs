import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { DEFAULT_PROTOCOL_VERSION, STATE_COMMITMENT_PROTOCOL_VERSION } from "@sidiora/layerx-sdk";
import {
  MiddlewareError,
  ReceiptPayloadAuthority,
  SellerMiddleware,
  VerifiedWebhookConsumer,
} from "@sidiora/layerx-seller-middleware";
import { MerchantError, MerchantMiddleware, MerchantSettlementWebhooks } from "../dist/index.js";

const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");

const bytes = (hex) => Uint8Array.from(Buffer.from(hex, "hex"));
const toHex = (value) => Buffer.from(value).toString("hex");

async function receiptLeafDigest(canonicalReceipt) {
  const input = new Uint8Array(MERKLE_LEAF_DOMAIN.length + canonicalReceipt.length);
  input.set(MERKLE_LEAF_DOMAIN);
  input.set(canonicalReceipt, MERKLE_LEAF_DOMAIN.length);
  return toHex(new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", input)));
}

async function loadFixture(name) {
  const fixture = JSON.parse(await readFile(new URL(`../../../sdk/conformance/fixtures/${name}`, import.meta.url), "utf8"));
  const batch = fixture.authorized_batch;
  const canonicalReceipt = bytes(fixture.canonical_receipt_hex);
  return {
    protocolVersion: fixture.expected.protocol_version,
    amount: fixture.expected.amount,
    asset: batch.asset_hex,
    payTo: fixture.expected.to_hex,
    receiptDigest: await receiptLeafDigest(canonicalReceipt),
    evidence: {
      canonicalReceipt,
      authorizedBatch: {
        batchId: bytes(batch.batch_id_hex),
        asset: bytes(batch.asset_hex),
        previousStateRoot: bytes(batch.previous_state_root_hex),
        resultingStateRoot: bytes(batch.resulting_state_root_hex),
        sequencerPublicKey: bytes(batch.sequencer_public_key_hex),
      },
    },
  };
}

const stateCommitment = await loadFixture("receipt-positive-v3.json");
const legacyDefault = await loadFixture("receipt-positive-v2.json");

class InMemoryOrders {
  #orders = new Map();

  async open(request) {
    const existing = this.#orders.get(request.checkoutKey);
    if (existing !== undefined) {
      if (existing.requestDigest !== request.requestDigest) throw new Error("order-conflict");
      return existing;
    }
    const order = {
      orderId: request.checkoutKey,
      checkoutKey: request.checkoutKey,
      requestDigest: request.requestDigest,
      state: "awaiting-payment",
      quote: request.quote,
    };
    this.#orders.set(order.orderId, order);
    return order;
  }

  async releaseResource(orderId) {
    return this.#required(orderId);
  }

  async markPaid(orderId, requestDigest, receiptDigest, transaction) {
    const current = this.#required(orderId);
    if (current.requestDigest !== requestDigest) throw new Error("order-conflict");
    if (current.state === "paid-verified") {
      if (current.receiptDigest !== receiptDigest || current.transaction !== transaction) {
        throw new Error("order-conflict");
      }
      return current;
    }
    const paid = { ...current, state: "paid-verified", receiptDigest, transaction };
    this.#orders.set(orderId, paid);
    return paid;
  }

  async markRefused(orderId, requestDigest) {
    const current = this.#required(orderId);
    if (current.requestDigest !== requestDigest || current.state === "paid-verified") throw new Error("order-conflict");
    const refused = { ...current, state: "refused" };
    this.#orders.set(orderId, refused);
    return refused;
  }

  async get(orderId) {
    return this.#orders.get(orderId);
  }

  #required(orderId) {
    const order = this.#orders.get(orderId);
    if (order === undefined) throw new Error("order-missing");
    return order;
  }
}

class InMemoryDeliveries {
  #records = new Map();

  async claim(value) {
    const existing = this.#records.get(value.deliveryId);
    if (existing === undefined) {
      this.#records.set(value.deliveryId, { digest: value.payloadDigest, done: false });
      return "claimed";
    }
    if (existing.digest !== value.payloadDigest) return "conflict";
    return existing.done ? "completed" : "processing";
  }

  async complete(deliveryId, payloadDigest) {
    const record = this.#records.get(deliveryId);
    if (record !== undefined && record.digest === payloadDigest) record.done = true;
  }

  async release(deliveryId, payloadDigest) {
    const record = this.#records.get(deliveryId);
    if (record !== undefined && record.digest === payloadDigest && !record.done) {
      this.#records.delete(deliveryId);
    }
  }
}

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

const merchantConfig = (fixture, protocolVersion, orders) => ({
  catalog: {
    get: async () => ({
      sku: "market-report",
      title: "Receipt-backed market report",
      unitAmount: fixture.amount,
      asset: fixture.asset,
      payTo: fixture.payTo,
      scheme: "exact",
      network: "layerx:beta",
      maxTimeoutSeconds: 120,
    }),
  },
  orders,
  sellers: {
    create: (paymentRequired, declared) => new SellerMiddleware({
      paymentRequired,
      protocolVersion: declared,
      authority: new ReceiptPayloadAuthority({
        resolve: async () => fixture.evidence.authorizedBatch,
      }),
      fulfillments: new InMemoryFulfillments(),
    }),
  },
  protocolVersion,
  resourceUrl: (checkoutKey) => `https://merchant.example/orders/${checkoutKey}`,
});

async function webhookKey() {
  const pair = await globalThis.crypto.subtle.generateKey({ name: "Ed25519" }, true, ["sign", "verify"]);
  const raw = new Uint8Array(await globalThis.crypto.subtle.exportKey("raw", pair.publicKey));
  return { privateKey: pair.privateKey, publicKey: raw };
}

async function signedDelivery(key, id, body) {
  const timestamp = Math.floor(Date.now() / 1000).toString();
  const prefix = new TextEncoder().encode(`${id}.${timestamp}.`);
  const message = new Uint8Array(prefix.length + body.length);
  message.set(prefix);
  message.set(body, prefix.length);
  const signature = new Uint8Array(await globalThis.crypto.subtle.sign("Ed25519", key.privateKey, message));
  return { id, timestamp, keyId: "seq-merchant", signature: `v1=${Buffer.from(signature).toString("base64")}` };
}

async function openedOrder(fixture, protocolVersion, checkoutKey) {
  const orders = new InMemoryOrders();
  const merchant = new MerchantMiddleware(merchantConfig(fixture, protocolVersion, orders));
  assert.equal(merchant.protocolVersion, protocolVersion);
  const checkout = await merchant.checkout("acct:merchant-protocol", checkoutKey, [{ sku: "market-report", quantity: 1 }]);
  assert.equal(checkout.kind, "payment-required");
  return { orders, order: checkout.order };
}

test("a settlement webhook verifies its receipt at the declared protocol version", async () => {
  assert.equal(stateCommitment.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  const { orders, order } = await openedOrder(stateCommitment, STATE_COMMITMENT_PROTOCOL_VERSION, "merchant-protocol3");
  const key = await webhookKey();
  const webhooks = new MerchantSettlementWebhooks({
    verifier: new VerifiedWebhookConsumer({
      publicKeys: { "seq-merchant": key.publicKey },
      deliveries: new InMemoryDeliveries(),
    }),
    orders,
    receipts: { resolve: async () => stateCommitment.evidence },
    protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION,
  });
  assert.equal(webhooks.protocolVersion, STATE_COMMITMENT_PROTOCOL_VERSION);
  const body = new TextEncoder().encode(JSON.stringify({
    order_id: order.orderId,
    request_digest: order.requestDigest,
    receipt_digest: stateCommitment.receiptDigest,
    receipt_ref: "receipt:merchant-protocol3",
    transaction: `lxp:${stateCommitment.receiptDigest}`,
    verification: "sequencer-signed",
  }));
  const result = await webhooks.consume(body, await signedDelivery(key, "merchant-delivery-3", body));
  assert.equal(result, "processed");
  const paid = await orders.get(order.orderId);
  assert.equal(paid.state, "paid-verified");
  assert.equal(paid.receiptDigest, stateCommitment.receiptDigest);
});

test("a settlement webhook carrying another protocol version's receipt is refused", async () => {
  assert.equal(legacyDefault.protocolVersion, DEFAULT_PROTOCOL_VERSION);
  const { orders, order } = await openedOrder(stateCommitment, STATE_COMMITMENT_PROTOCOL_VERSION, "merchant-protocol3-other");
  const key = await webhookKey();
  const webhooks = new MerchantSettlementWebhooks({
    verifier: new VerifiedWebhookConsumer({
      publicKeys: { "seq-merchant": key.publicKey },
      deliveries: new InMemoryDeliveries(),
    }),
    orders,
    receipts: { resolve: async () => legacyDefault.evidence },
    protocolVersion: STATE_COMMITMENT_PROTOCOL_VERSION,
  });
  const body = new TextEncoder().encode(JSON.stringify({
    order_id: order.orderId,
    request_digest: order.requestDigest,
    receipt_digest: legacyDefault.receiptDigest,
    receipt_ref: "receipt:merchant-protocol2",
    transaction: `lxp:${legacyDefault.receiptDigest}`,
    verification: "sequencer-signed",
  }));
  await assert.rejects(
    webhooks.consume(body, await signedDelivery(key, "merchant-delivery-2", body)),
    (error) => error instanceof MiddlewareError && error.code === "verification-failure",
  );
  assert.equal((await orders.get(order.orderId)).state, "awaiting-payment");
});

test("a merchant configuration without a usable protocol version is refused by name", async () => {
  const key = await webhookKey();
  const webhookConfig = (protocolVersion) => ({
    verifier: new VerifiedWebhookConsumer({
      publicKeys: { "seq-merchant": key.publicKey },
      deliveries: new InMemoryDeliveries(),
    }),
    orders: new InMemoryOrders(),
    receipts: { resolve: async () => stateCommitment.evidence },
    protocolVersion,
  });
  for (const declared of [undefined, null, 0, 1, 4, "3", 3.5]) {
    assert.throws(
      () => new MerchantMiddleware(merchantConfig(stateCommitment, declared, new InMemoryOrders())),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
      `declared protocol version ${String(declared)} must be refused by name`,
    );
    assert.throws(
      () => new MerchantSettlementWebhooks(webhookConfig(declared)),
      (error) => error instanceof MiddlewareError && error.code === "missing-protocol-version",
      `declared webhook protocol version ${String(declared)} must be refused by name`,
    );
  }
});

test("a seller that does not declare the merchant's protocol version cannot take a checkout", async () => {
  const orders = new InMemoryOrders();
  const config = merchantConfig(stateCommitment, STATE_COMMITMENT_PROTOCOL_VERSION, orders);
  const merchant = new MerchantMiddleware({
    ...config,
    sellers: {
      create: (paymentRequired) => config.sellers.create(paymentRequired, DEFAULT_PROTOCOL_VERSION),
    },
  });
  await assert.rejects(
    merchant.checkout("acct:merchant-protocol", "merchant-protocol3-drift", [{ sku: "market-report", quantity: 1 }]),
    (error) => error instanceof MerchantError && error.code === "protocol-version-mismatch",
  );
});
