import {
  ProductionClient,
  SecretBytes,
  type ReceiptVerification,
} from "@sidiora/layerx-sdk";
import {
  MiddlewareError,
  ReceiptPayloadAuthority,
  SellerMiddleware,
  VerifiedWebhookConsumer,
  encodePaymentPayloadHeader,
  encodePaymentRequiredHeader,
  encodeSettlementHeader,
  verifyPaymentReceipt,
  type PaymentPayload,
  type SellerDecision,
} from "@sidiora/layerx-seller-middleware";
import {
  BuyerMiddleware,
  LayerXPaymentHttpTransport,
  type Journey,
  type MoveQuote,
  type ParsedOffer,
  type PreparedPayment,
} from "@sidiora/layerx-buyer-middleware";
import {
  MerchantError,
  MerchantMiddleware,
  MerchantSettlementWebhooks,
  type CheckoutOpenRequest,
  type MerchantOrder,
  type MerchantOrderStore,
} from "@sidiora/layerx-merchant-middleware";
import {
  ConformanceSequencer,
  buildSignedReceipt,
  buyerPayload,
  encodeBase64,
  fixedBytes,
  offerFixture,
  toHex,
  type OfferFixture,
  type SignedReceipt,
} from "./receipts.js";
import {
  FixedBatchResolver,
  InMemoryDeliveryStore,
  InMemoryFulfillmentRepository,
  Suite,
  assert,
  expectThrows,
} from "./support.js";

const isMiddlewareError = (code: string) => (error: unknown): boolean =>
  error instanceof MiddlewareError && error.code === code;

function buildBuyer(offer: OfferFixture): BuyerMiddleware {
  const transport = new LayerXPaymentHttpTransport({
    baseUrl: "http://127.0.0.1:1/",
    bearerToken: new SecretBytes(new TextEncoder().encode("unused-conformance-token")),
  });
  return new BuyerMiddleware({
    client: new ProductionClient(transport),
    source: "acct:conformance-buyer",
    supported: [{ scheme: offer.requirements.scheme, network: offer.requirements.network }],
    authorizedBatches: new FixedBatchResolver(buildAuthorizedBatchPlaceholder()),
  });
}

function buildAuthorizedBatchPlaceholder(): SignedReceipt["authorizedBatch"] {
  return {
    batchId: fixedBytes(0x01),
    asset: fixedBytes(0x02),
    previousStateRoot: fixedBytes(0x03),
    resultingStateRoot: fixedBytes(0x04),
    sequencerPublicKey: fixedBytes(0x05),
  };
}

function preparedPayment(
  offer: ParsedOffer,
  receipt: SignedReceipt,
  verification: ReceiptVerification,
): PreparedPayment {
  const quote: MoveQuote = {
    quote_id: "quote-conformance",
    description_copy_key: "move.description",
    mechanism: "transfer",
    money: { amount: offer.accepted.amount, currency: offer.accepted.asset },
    fee_estimate: { amount: "0", currency: offer.accepted.asset },
    fee_ceiling: { amount: "0", currency: offer.accepted.asset },
    arrival_estimate: "2026-01-01T00:00:00.000Z",
    expires_at: "2999-01-01T00:00:00.000Z",
  };
  const journey: Journey = {
    journey_id: "journey-conformance",
    kind: "move",
    state: "done",
    state_copy_key: "journey.done",
    evidence: [],
    stages: [],
    started_at: "2026-01-01T00:00:00.000Z",
    updated_at: "2026-01-01T00:00:00.000Z",
  };
  const payload = buyerPayload({ requirements: offer.accepted, paymentRequired: offer.required }, receipt, "buyer-key");
  return {
    offer,
    quote,
    journey,
    payload,
    paymentHeader: encodePaymentPayloadHeader(payload),
    idempotencyKey: "buyer-key",
    canonicalReceipt: receipt.canonicalReceipt,
    authorizedBatch: receipt.authorizedBatch,
    verification,
    receiptDigest: receipt.receiptDigest,
  };
}

class ConformanceMerchantOrders implements MerchantOrderStore {
  readonly #orders = new Map<string, MerchantOrder>();

  public async open(request: CheckoutOpenRequest): Promise<MerchantOrder> {
    const existing = this.#orders.get(request.checkoutKey);
    if (existing !== undefined) {
      if (existing.requestDigest !== request.requestDigest) throw new Error("order-conflict");
      return existing;
    }
    const order: MerchantOrder = {
      orderId: request.checkoutKey,
      checkoutKey: request.checkoutKey,
      requestDigest: request.requestDigest,
      state: "awaiting-payment",
      quote: request.quote,
    };
    this.#orders.set(order.orderId, order);
    return order;
  }

  public async releaseResource(orderId: string): Promise<MerchantOrder> {
    return this.required(orderId);
  }

  public async markPaid(
    orderId: string,
    requestDigest: string,
    receiptDigest: string,
    transaction: string,
  ): Promise<MerchantOrder> {
    const current = this.required(orderId);
    if (current.requestDigest !== requestDigest) throw new Error("order-conflict");
    if (current.state === "paid-verified") {
      if (current.receiptDigest !== receiptDigest || current.transaction !== transaction) {
        throw new Error("order-conflict");
      }
      return current;
    }
    const paid: MerchantOrder = { ...current, state: "paid-verified", receiptDigest, transaction };
    this.#orders.set(orderId, paid);
    return paid;
  }

  public async markRefused(orderId: string, requestDigest: string): Promise<MerchantOrder> {
    const current = this.required(orderId);
    if (current.requestDigest !== requestDigest || current.state === "paid-verified") throw new Error("order-conflict");
    const refused: MerchantOrder = { ...current, state: "refused" };
    this.#orders.set(orderId, refused);
    return refused;
  }

  public async get(orderId: string): Promise<MerchantOrder | undefined> {
    return this.#orders.get(orderId);
  }

  private required(orderId: string): MerchantOrder {
    const order = this.#orders.get(orderId);
    if (order === undefined) throw new Error("order-missing");
    return order;
  }
}

async function releasedDecision(
  offer: OfferFixture,
  payload: PaymentPayload,
  resolver: FixedBatchResolver,
  repository: InMemoryFulfillmentRepository<string>,
): Promise<SellerDecision<string>> {
  const seller = new SellerMiddleware<string>({
    paymentRequired: offer.paymentRequired,
    authority: new ReceiptPayloadAuthority(resolver),
    fulfillments: repository,
  });
  return seller.handle("acct:conformance-seller", encodePaymentPayloadHeader(payload), async () => "the-paid-resource");
}

export async function runScenarios(): Promise<Suite> {
  const suite = new Suite();

  const sequencer = await ConformanceSequencer.generate();
  const foreignSequencer = await ConformanceSequencer.generate();
  const payTo = fixedBytes(0xaa);
  const asset = fixedBytes(0xbb);
  const amount = 250_000n;
  const offer = offerFixture(payTo, asset, amount);
  const facts = {
    asset,
    payTo,
    amount,
    from: fixedBytes(0xcc),
    batchId: fixedBytes(0xd1),
    previousStateRoot: fixedBytes(0xd2),
    resultingStateRoot: fixedBytes(0xd3),
  };
  const receipt = await buildSignedReceipt(sequencer, facts);
  const resolver = new FixedBatchResolver(receipt.authorizedBatch);

  await suite.check("seller: no payment header yields a 402 offer and never releases", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const seller = new SellerMiddleware<string>({
      paymentRequired: offer.paymentRequired,
      authority: new ReceiptPayloadAuthority(resolver),
      fulfillments: repository,
    });
    const decision = await seller.handle("acct:conformance-seller", undefined, async () => "the-paid-resource");
    assert(decision.kind === "payment-required" && decision.status === 402, "expected a 402 payment-required decision");
    assert(repository.releaseCount === 0, "the resource must not be released without payment");
  });

  await suite.check("seller: a valid receipt releases the resource under sequencer-signed evidence", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const payload = buyerPayload(offer, receipt, "seller-happy");
    const decision = await releasedDecision(offer, payload, resolver, repository);
    assert(decision.kind === "released" && decision.status === 200, "expected a released decision");
    if (decision.kind !== "released") {
      return;
    }
    assert(repository.releaseCount === 1, "the resource must be released exactly once");
    assert(decision.resource === "the-paid-resource", "the released resource must be returned");
    assert(decision.settlement.success, "the settlement response must report success");
    assert(decision.settlement.transaction === `lxp:${receipt.receiptDigest}`, "the settlement must reference the receipt digest");
    assert(decision.verification.level === "sequencer-signed", "the verification level must be recorded");
  });

  await suite.check("seller: a tampered receipt cannot be presented as success", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const tampered = receipt.canonicalReceipt.slice();
    const last = tampered.length - 1;
    tampered[last] = (tampered[last] ?? 0) ^ 0x01;
    const payload = buyerPayload(offer, { ...receipt, evidence: { ...receipt.evidence, receipt: encodeBase64(tampered) } }, "seller-tamper");
    await expectThrows(
      () => releasedDecision(offer, payload, resolver, repository),
      isMiddlewareError("verification-failure"),
      "tampered receipt",
    );
    assert(repository.releaseCount === 0, "a tampered receipt must never release the resource");
  });

  await suite.check("seller: a receipt signed by an unauthorised sequencer is rejected", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const forged = await buildSignedReceipt(foreignSequencer, facts);
    const forgedBatch = new FixedBatchResolver({
      ...forged.authorizedBatch,
      sequencerPublicKey: sequencer.publicKey,
    });
    const payload = buyerPayload(offer, forged, "seller-forged");
    await expectThrows(
      () => releasedDecision(offer, payload, forgedBatch, repository),
      isMiddlewareError("verification-failure"),
      "forged sequencer signature",
    );
    assert(repository.releaseCount === 0, "a forged signature must never release the resource");
  });

  await suite.check("seller: a receipt whose amount disagrees with the offer is rejected", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const cheapReceipt = await buildSignedReceipt(sequencer, { ...facts, amount: 1n });
    const cheapResolver = new FixedBatchResolver(cheapReceipt.authorizedBatch);
    const payload = buyerPayload(offer, cheapReceipt, "seller-underpaid");
    await expectThrows(
      () => releasedDecision(offer, payload, cheapResolver, repository),
      isMiddlewareError("verification-failure"),
      "underpaid receipt",
    );
    assert(repository.releaseCount === 0, "an underpaid receipt must never release the resource");
  });

  await suite.check("seller: fulfilment is idempotent under a replayed payment", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const payload = buyerPayload(offer, receipt, "seller-idempotent");
    const first = await releasedDecision(offer, payload, resolver, repository);
    const second = await releasedDecision(offer, payload, resolver, repository);
    assert(first.kind === "released" && second.kind === "released", "both replays must release");
    assert(repository.releaseCount === 1, "the resource must be released only once across replays");
  });

  await suite.check("buyer: parseOffer selects only a supported requirement", async () => {
    const buyer = buildBuyer(offer);
    const parsed = buyer.parseOffer(encodePaymentRequiredHeader(offer.paymentRequired));
    assert(parsed.accepted.scheme === offer.requirements.scheme, "the supported scheme must be selected");
    const unsupported = offerFixture(payTo, asset, amount);
    const other = {
      ...unsupported.paymentRequired,
      accepts: [{ ...unsupported.requirements, network: "layerx:mainnet" }],
    };
    expectThrowsSync(
      () => buyer.parseOffer(encodePaymentRequiredHeader(other)),
      isMiddlewareError("unsupported-payment"),
      "unsupported network",
    );
  });

  await suite.check("buyer: captureSettlement verifies the seller's settlement evidence", async () => {
    const repository = new InMemoryFulfillmentRepository<string>();
    const payload = buyerPayload(offer, receipt, "buyer-capture");
    const decision = await releasedDecision(offer, payload, resolver, repository);
    assert(decision.kind === "released", "seller must release for the capture scenario");
    if (decision.kind !== "released") {
      return;
    }
    const buyer = new BuyerMiddleware({
      client: new ProductionClient(new LayerXPaymentHttpTransport({
        baseUrl: "http://127.0.0.1:1/",
        bearerToken: new SecretBytes(new TextEncoder().encode("unused-conformance-token")),
      })),
      source: "acct:conformance-buyer",
      supported: [{ scheme: offer.requirements.scheme, network: offer.requirements.network }],
      authorizedBatches: resolver,
    });
    const parsed = buyer.parseOffer(encodePaymentRequiredHeader(offer.paymentRequired));
    const prepared = preparedPayment(parsed, receipt, decision.verification);
    const captured = await buyer.captureSettlement(
      decision.headers["PAYMENT-RESPONSE"],
      prepared,
    );
    assert(captured.verification.level === "sequencer-signed", "the buyer must verify the captured receipt");
  });

  await suite.check("buyer: a failed settlement is never reported as paid", async () => {
    const buyer = buildBuyer(offer);
    const parsed = buyer.parseOffer(encodePaymentRequiredHeader(offer.paymentRequired));
    const prepared = preparedPayment(parsed, receipt, await sequencerVerification(receipt, resolver));
    const failure = encodeSettlementHeader({
      success: false,
      errorReason: "insufficient_funds",
      transaction: "",
      network: offer.requirements.network,
    });
    await expectThrows(
      () => buyer.captureSettlement(failure, prepared),
      isMiddlewareError("payment-refused"),
      "failed settlement",
    );
  });

  await suite.check("webhook: a valid signature is processed exactly once and replays are duplicates", async () => {
    const deliveries = new InMemoryDeliveryStore();
    const consumer = new VerifiedWebhookConsumer({
      publicKeys: { "seq-1": sequencer.publicKey },
      deliveries,
    });
    const body = new TextEncoder().encode(JSON.stringify({ event: "settlement", ok: true }));
    const id = "delivery-1";
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const signature = await signWebhook(sequencer, id, timestamp, body);
    let handled = 0;
    const first = await consumer.consume(body, { id, timestamp, keyId: "seq-1", signature }, async () => {
      handled += 1;
    });
    assert(first === "processed", "a valid webhook must be processed");
    assert(handled === 1, "the handler must run exactly once");
    const second = await consumer.consume(body, { id, timestamp, keyId: "seq-1", signature }, async () => {
      handled += 1;
    });
    assert(second === "duplicate", "a replay must be reported as a duplicate");
    assert(handled === 1, "a replay must not re-run the handler");
  });

  await suite.check("webhook: a tampered body cannot be presented as verified", async () => {
    const deliveries = new InMemoryDeliveryStore();
    const consumer = new VerifiedWebhookConsumer({
      publicKeys: { "seq-1": sequencer.publicKey },
      deliveries,
    });
    const body = new TextEncoder().encode(JSON.stringify({ event: "settlement", ok: true }));
    const id = "delivery-2";
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const signature = await signWebhook(sequencer, id, timestamp, body);
    const tampered = new TextEncoder().encode(JSON.stringify({ event: "settlement", ok: false }));
    let handled = 0;
    await expectThrows(
      () => consumer.consume(tampered, { id, timestamp, keyId: "seq-1", signature }, async () => {
        handled += 1;
      }),
      isMiddlewareError("invalid-webhook"),
      "tampered webhook body",
    );
    assert(handled === 0, "a tampered webhook must never run the handler");
  });

  await suite.check("webhook: an unknown key id is rejected", async () => {
    const deliveries = new InMemoryDeliveryStore();
    const consumer = new VerifiedWebhookConsumer({
      publicKeys: { "seq-1": sequencer.publicKey },
      deliveries,
    });
    const body = new TextEncoder().encode(JSON.stringify({ event: "settlement" }));
    const id = "delivery-3";
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const signature = await signWebhook(sequencer, id, timestamp, body);
    await expectThrows(
      () => consumer.consume(body, { id, timestamp, keyId: "unknown", signature }, async () => undefined),
      isMiddlewareError("invalid-webhook"),
      "unknown webhook key",
    );
  });

  await suite.check("merchant: a signed settlement webhook verifies the receipt and pays exactly once", async () => {
    const orders = new ConformanceMerchantOrders();
    const merchant = new MerchantMiddleware({
      catalog: { get: async () => ({
        sku: "merchant-item",
        title: "Merchant conformance item",
        unitAmount: amount.toString(),
        asset: toHex(asset),
        payTo: toHex(payTo),
        scheme: "exact",
        network: "layerx:testnet",
        maxTimeoutSeconds: 120,
      }) },
      orders,
      sellers: {
        create: (paymentRequired) => new SellerMiddleware({
          paymentRequired,
          authority: new ReceiptPayloadAuthority(resolver),
          fulfillments: new InMemoryFulfillmentRepository<MerchantOrder>(),
        }),
      },
      resourceUrl: (checkoutKey) => `https://merchant.example/checkout/${checkoutKey}`,
    });
    const checkout = await merchant.checkout(
      "acct:merchant-conformance",
      "merchant-webhook-order",
      [{ sku: "merchant-item", quantity: 1 }],
    );
    assert(checkout.kind === "payment-required", "checkout must start payment-required");
    const deliveries = new InMemoryDeliveryStore();
    const consumer = new VerifiedWebhookConsumer({
      publicKeys: { "seq-merchant": sequencer.publicKey },
      deliveries,
    });
    const webhooks = new MerchantSettlementWebhooks(
      consumer,
      orders,
      { resolve: async () => ({ canonicalReceipt: receipt.canonicalReceipt, authorizedBatch: receipt.authorizedBatch }) },
    );
    const event = {
      order_id: checkout.order.orderId,
      request_digest: checkout.order.requestDigest,
      receipt_digest: receipt.receiptDigest,
      receipt_ref: "receipt:merchant-conformance",
      transaction: `lxp:${receipt.receiptDigest}`,
      verification: "sequencer-signed",
    };
    const body = new TextEncoder().encode(JSON.stringify(event));
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const signature = await signWebhook(sequencer, "merchant-delivery", timestamp, body);
    const first = await webhooks.consume(body, {
      id: "merchant-delivery",
      timestamp,
      keyId: "seq-merchant",
      signature,
    });
    const paid = await orders.get(checkout.order.orderId);
    assert(first === "processed" && paid?.state === "paid-verified", "verified webhook must pay the order");
    assert(paid?.receiptDigest === receipt.receiptDigest, "paid order must bind the verified receipt digest");
    const second = await webhooks.consume(body, {
      id: "merchant-delivery",
      timestamp,
      keyId: "seq-merchant",
      signature,
    });
    assert(second === "duplicate", "settlement redelivery must be idempotent");
  });

  await suite.check("merchant: a signed claim with mismatched receipt evidence cannot pay an order", async () => {
    const orders = new ConformanceMerchantOrders();
    const merchant = new MerchantMiddleware({
      catalog: { get: async () => ({
        sku: "merchant-item",
        title: "Merchant conformance item",
        unitAmount: amount.toString(),
        asset: toHex(asset),
        payTo: toHex(payTo),
        scheme: "exact",
        network: "layerx:testnet",
        maxTimeoutSeconds: 120,
      }) },
      orders,
      sellers: {
        create: (paymentRequired) => new SellerMiddleware({
          paymentRequired,
          authority: new ReceiptPayloadAuthority(resolver),
          fulfillments: new InMemoryFulfillmentRepository<MerchantOrder>(),
        }),
      },
      resourceUrl: (checkoutKey) => `https://merchant.example/checkout/${checkoutKey}`,
    });
    const checkout = await merchant.checkout(
      "acct:merchant-conformance",
      "merchant-mismatch",
      [{ sku: "merchant-item", quantity: 1 }],
    );
    assert(checkout.kind === "payment-required", "mismatch checkout must start payment-required");
    const order = checkout.order;
    const webhooks = new MerchantSettlementWebhooks(
      new VerifiedWebhookConsumer({
        publicKeys: { "seq-merchant": sequencer.publicKey },
        deliveries: new InMemoryDeliveryStore(),
      }),
      orders,
      { resolve: async () => ({ canonicalReceipt: receipt.canonicalReceipt, authorizedBatch: receipt.authorizedBatch }) },
    );
    const body = new TextEncoder().encode(JSON.stringify({
      order_id: order.orderId,
      request_digest: order.requestDigest,
      receipt_digest: toHex(fixedBytes(0xff)),
      receipt_ref: "receipt:merchant-conformance",
      transaction: "lxp:mismatch",
      verification: "sequencer-signed",
    }));
    const timestamp = Math.floor(Date.now() / 1000).toString();
    const signature = await signWebhook(sequencer, "merchant-mismatch-delivery", timestamp, body);
    await expectThrows(
      () => webhooks.consume(body, {
        id: "merchant-mismatch-delivery",
        timestamp,
        keyId: "seq-merchant",
        signature,
      }),
      (error) => error instanceof MerchantError && error.code === "invalid-webhook",
      "mismatched merchant receipt digest",
    );
    assert((await orders.get(order.orderId))?.state === "awaiting-payment", "mismatched evidence must not alter order state");
  });

  return suite;
}

async function sequencerVerification(receipt: SignedReceipt, resolver: FixedBatchResolver): Promise<ReceiptVerification> {
  const batch = await resolver.resolve();
  return verifyPaymentReceipt(
    { canonicalReceipt: receipt.canonicalReceipt, authorizedBatch: batch },
    {
      scheme: "exact",
      network: "layerx:testnet",
      amount: "250000",
      asset: toHex(fixedBytes(0xbb)),
      payTo: toHex(fixedBytes(0xaa)),
      maxTimeoutSeconds: 120,
    },
  );
}

async function signWebhook(
  sequencer: ConformanceSequencer,
  id: string,
  timestamp: string,
  body: Uint8Array,
): Promise<string> {
  const prefix = new TextEncoder().encode(`${id}.${timestamp}.`);
  const message = new Uint8Array(prefix.length + body.length);
  message.set(prefix);
  message.set(body, prefix.length);
  const signature = await sequencer.sign(message);
  return `v1=${encodeBase64(signature)}`;
}

function expectThrowsSync(body: () => unknown, predicate: (error: unknown) => boolean, description: string): void {
  try {
    body();
  } catch (error) {
    if (!predicate(error)) {
      throw new Error(`${description}: threw the wrong error`);
    }
    return;
  }
  throw new Error(`${description}: expected a throw but the call returned`);
}
