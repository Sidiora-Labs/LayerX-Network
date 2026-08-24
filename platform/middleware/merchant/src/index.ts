import {
  MiddlewareError,
  SellerMiddleware,
  VerifiedWebhookConsumer,
  X402_VERSION,
  verifyPaymentReceipt,
  type JsonValue,
  type PaymentRequired,
  type SellerDecision,
  type WebhookConsumeResult,
  type WebhookRequestHeaders,
} from "@sidiora/layerx-seller-middleware";
import type { ReceiptVerification } from "@sidiora/layerx-sdk";

const MAX_U128 = 340282366920938463463374607431768211455n;
const MAX_LINES = 256;
const MAX_QUANTITY = 1_000_000;

export interface CatalogItem {
  readonly sku: string;
  readonly title: string;
  readonly unitAmount: string;
  readonly asset: string;
  readonly payTo: string;
  readonly scheme: string;
  readonly network: string;
  readonly maxTimeoutSeconds: number;
}

export interface CatalogProvider {
  get(sku: string): Promise<CatalogItem | undefined>;
}

export interface CartLine {
  readonly sku: string;
  readonly quantity: number;
}

export interface QuotedLine extends CartLine {
  readonly title: string;
  readonly unitAmount: string;
  readonly lineAmount: string;
}

export interface MerchantQuote {
  readonly lines: readonly QuotedLine[];
  readonly totalAmount: string;
  readonly asset: string;
  readonly payTo: string;
  readonly scheme: string;
  readonly network: string;
  readonly maxTimeoutSeconds: number;
  readonly paymentRequired: PaymentRequired;
}

export type MerchantOrderState = "awaiting-payment" | "paid-verified" | "refused";

export interface MerchantOrder {
  readonly orderId: string;
  readonly checkoutKey: string;
  readonly requestDigest: string;
  readonly state: MerchantOrderState;
  readonly quote: MerchantQuote;
  readonly receiptDigest?: string;
  readonly transaction?: string;
}

export interface CheckoutOpenRequest {
  readonly checkoutKey: string;
  readonly requestDigest: string;
  readonly quote: MerchantQuote;
}

export interface MerchantOrderStore {
  open(request: CheckoutOpenRequest): Promise<MerchantOrder>;
  releaseResource(orderId: string): Promise<MerchantOrder>;
  markPaid(
    orderId: string,
    requestDigest: string,
    receiptDigest: string,
    transaction: string,
  ): Promise<MerchantOrder>;
  markRefused(orderId: string, requestDigest: string): Promise<MerchantOrder>;
  get(orderId: string): Promise<MerchantOrder | undefined>;
}

export interface MerchantSellerFactory {
  create(paymentRequired: PaymentRequired): SellerMiddleware<MerchantOrder>;
}

export interface MerchantMiddlewareConfig {
  readonly catalog: CatalogProvider;
  readonly orders: MerchantOrderStore;
  readonly sellers: MerchantSellerFactory;
  readonly resourceUrl: (checkoutKey: string) => string;
}

export type MerchantCheckoutResult =
  | { readonly kind: "payment-required"; readonly order: MerchantOrder; readonly decision: Extract<SellerDecision<MerchantOrder>, { readonly kind: "payment-required" }> }
  | { readonly kind: "pending"; readonly order: MerchantOrder }
  | { readonly kind: "refused"; readonly order: MerchantOrder }
  | {
    readonly kind: "paid";
    readonly order: MerchantOrder;
    readonly verification: ReceiptVerification;
    readonly settlement: Extract<SellerDecision<MerchantOrder>, { readonly kind: "released" }>["settlement"];
  };

export class MerchantMiddleware {
  readonly #catalog: CatalogProvider;
  readonly #orders: MerchantOrderStore;
  readonly #sellers: MerchantSellerFactory;
  readonly #resourceUrl: (checkoutKey: string) => string;

  public constructor(config: MerchantMiddlewareConfig) {
    this.#catalog = config.catalog;
    this.#orders = config.orders;
    this.#sellers = config.sellers;
    this.#resourceUrl = config.resourceUrl;
  }

  public async quote(checkoutKey: string, lines: readonly CartLine[]): Promise<MerchantQuote> {
    requireIdentifier(checkoutKey, 255);
    if (lines.length === 0 || lines.length > MAX_LINES) {
      throw new MerchantError("invalid-cart");
    }
    const quoted: QuotedLine[] = [];
    const seen = new Set<string>();
    let total = 0n;
    let first: CatalogItem | undefined;
    for (const line of lines) {
      requireIdentifier(line.sku, 128);
      if (!Number.isSafeInteger(line.quantity) || line.quantity <= 0 || line.quantity > MAX_QUANTITY || seen.has(line.sku)) {
        throw new MerchantError("invalid-cart");
      }
      seen.add(line.sku);
      const item = await this.#catalog.get(line.sku);
      if (item === undefined) {
        throw new MerchantError("catalog-item-missing");
      }
      validateCatalogItem(item);
      if (item.sku !== line.sku) {
        throw new MerchantError("catalog-item-missing");
      }
      if (first === undefined) {
        first = item;
      } else if (
        item.asset !== first.asset
        || item.payTo !== first.payTo
        || item.scheme !== first.scheme
        || item.network !== first.network
      ) {
        throw new MerchantError("mixed-payment-facts");
      }
      const unit = parseAmount(item.unitAmount);
      const lineAmount = checkedMultiply(unit, BigInt(line.quantity));
      total = checkedAdd(total, lineAmount);
      quoted.push({
        sku: line.sku,
        quantity: line.quantity,
        title: item.title,
        unitAmount: unit.toString(),
        lineAmount: lineAmount.toString(),
      });
    }
    if (first === undefined || total === 0n) {
      throw new MerchantError("invalid-cart");
    }
    const resourceUrl = this.#resourceUrl(checkoutKey);
    requireUrl(resourceUrl);
    const totalAmount = total.toString();
    const paymentRequired: PaymentRequired = {
      x402Version: X402_VERSION,
      resource: {
        url: resourceUrl,
        description: `Checkout ${checkoutKey}`,
        serviceName: "LayerX merchant checkout",
        tags: ["checkout"],
      },
      accepts: [{
        scheme: first.scheme,
        network: first.network,
        amount: totalAmount,
        asset: first.asset,
        payTo: first.payTo,
        maxTimeoutSeconds: first.maxTimeoutSeconds,
      }],
      extensions: {},
    };
    return {
      lines: quoted,
      totalAmount,
      asset: first.asset,
      payTo: first.payTo,
      scheme: first.scheme,
      network: first.network,
      maxTimeoutSeconds: first.maxTimeoutSeconds,
      paymentRequired,
    };
  }

  public async checkout(
    principal: string,
    checkoutKey: string,
    lines: readonly CartLine[],
    paymentHeader?: string,
  ): Promise<MerchantCheckoutResult> {
    requireText(principal, 512);
    const quote = await this.quote(checkoutKey, lines);
    const requestDigest = await digestQuote(quote);
    const opened = await this.#orders.open({ checkoutKey, requestDigest, quote });
    requireMatchingOrder(opened, checkoutKey, requestDigest, quote);
    if (paymentHeader === undefined && opened.state === "paid-verified") {
      return { kind: "pending", order: opened };
    }
    if (paymentHeader === undefined && opened.state === "refused") {
      return { kind: "refused", order: opened };
    }
    const seller = this.#sellers.create(quote.paymentRequired);
    const decision = await seller.handle(
      principal,
      paymentHeader,
      () => this.#orders.releaseResource(opened.orderId),
    );
    if (decision.kind === "payment-required") {
      return { kind: "payment-required", order: opened, decision };
    }
    if (decision.kind === "pending") {
      return { kind: "pending", order: opened };
    }
    if (decision.kind === "refused") {
      const refused = await this.#orders.markRefused(opened.orderId, requestDigest);
      requireMatchingOrder(refused, checkoutKey, requestDigest, quote);
      if (refused.state !== "refused") {
        throw new MerchantError("order-conflict");
      }
      return { kind: "refused", order: refused };
    }
    const receiptDigest = layerXReceiptDigest(decision.settlement.extensions);
    const paid = await this.#orders.markPaid(
      opened.orderId,
      requestDigest,
      receiptDigest,
      decision.settlement.transaction,
    );
    requireMatchingOrder(paid, checkoutKey, requestDigest, quote);
    if (
      paid.state !== "paid-verified"
      || paid.receiptDigest !== receiptDigest
      || paid.transaction !== decision.settlement.transaction
    ) {
      throw new MerchantError("order-conflict");
    }
    return {
      kind: "paid",
      order: paid,
      verification: decision.verification,
      settlement: decision.settlement,
    };
  }
}

export interface SettlementWebhookEvent {
  readonly order_id: string;
  readonly request_digest: string;
  readonly receipt_digest: string;
  readonly receipt_ref: string;
  readonly transaction: string;
  readonly verification: "sequencer-signed" | "batch-included" | "state-proven" | "checkpoint-finalised" | "settlement-anchored";
}

export interface MerchantReceiptEvidence {
  readonly canonicalReceipt: Uint8Array;
  readonly authorizedBatch: {
    readonly batchId: Uint8Array;
    readonly asset: Uint8Array;
    readonly previousStateRoot: Uint8Array;
    readonly resultingStateRoot: Uint8Array;
    readonly sequencerPublicKey: Uint8Array;
  };
}

export interface MerchantReceiptResolver {
  resolve(receiptRef: string): Promise<MerchantReceiptEvidence>;
}

export class MerchantSettlementWebhooks {
  public constructor(
    private readonly verifier: VerifiedWebhookConsumer,
    private readonly orders: MerchantOrderStore,
    private readonly receipts: MerchantReceiptResolver,
  ) {}

  public consume(rawBody: Uint8Array, headers: WebhookRequestHeaders): Promise<WebhookConsumeResult> {
    return this.verifier.consume(rawBody, headers, async (value) => {
      const event = parseSettlementWebhook(value);
      const current = await this.orders.get(event.order_id);
      if (current === undefined || current.requestDigest !== event.request_digest) {
        throw new MerchantError("order-conflict");
      }
      requireMatchingOrder(current, current.checkoutKey, event.request_digest, current.quote);
      if (await digestQuote(current.quote) !== current.requestDigest) {
        throw new MerchantError("order-conflict");
      }
      const evidence = await this.receipts.resolve(event.receipt_ref);
      const verification = await verifyPaymentReceipt(evidence, current.quote.paymentRequired.accepts[0]!);
      if (verificationRank(event.verification) > verificationRank(verification.level)) {
        throw new MerchantError("invalid-webhook");
      }
      const receiptDigest = toHex(verification.receiptDigest);
      if (!constantTimeHex(receiptDigest, event.receipt_digest)) {
        throw new MerchantError("invalid-webhook");
      }
      const paid = await this.orders.markPaid(
        current.orderId,
        current.requestDigest,
        event.receipt_digest,
        event.transaction,
      );
      requireMatchingOrder(paid, current.checkoutKey, current.requestDigest, current.quote);
      if (
        paid.orderId !== current.orderId
        || paid.state !== "paid-verified"
        || paid.receiptDigest !== event.receipt_digest
        || paid.transaction !== event.transaction
      ) {
        throw new MerchantError("order-conflict");
      }
    });
  }
}

export type MerchantErrorCode =
  | "invalid-cart"
  | "catalog-item-missing"
  | "mixed-payment-facts"
  | "amount-overflow"
  | "order-conflict"
  | "invalid-webhook";

export class MerchantError extends Error {
  public constructor(public readonly code: MerchantErrorCode) {
    super(code);
    this.name = "MerchantError";
  }
}

export function platform_mw_merchant(): "receipt-backed-merchant-checkout" {
  return "receipt-backed-merchant-checkout";
}

function validateCatalogItem(item: CatalogItem): void {
  requireIdentifier(item.sku, 128);
  requireText(item.title, 256);
  parseAmount(item.unitAmount);
  requireText(item.asset, 256);
  requireText(item.payTo, 256);
  requireIdentifier(item.scheme, 32);
  requireNetwork(item.network);
  if (!Number.isSafeInteger(item.maxTimeoutSeconds) || item.maxTimeoutSeconds <= 0) {
    throw new MerchantError("invalid-cart");
  }
}

function requireMatchingOrder(
  order: MerchantOrder,
  key: string,
  digest: string,
  quote: MerchantQuote,
): void {
  if (
    order === null
    || typeof order !== "object"
    || Array.isArray(order)
    || typeof order.orderId !== "string"
    || typeof order.checkoutKey !== "string"
    || typeof order.requestDigest !== "string"
    || order.quote === null
    || typeof order.quote !== "object"
    || Array.isArray(order.quote)
    || (order.state !== "awaiting-payment" && order.state !== "paid-verified" && order.state !== "refused")
    || order.orderId.length === 0
    || order.orderId.length > 255
    || order.orderId.includes("\0")
    || order.checkoutKey.length === 0
    || order.checkoutKey.length > 255
    || order.checkoutKey.includes("\0")
    || !/^[0-9a-f]{64}$/u.test(order.requestDigest)
    || order.checkoutKey !== key
    || order.requestDigest !== digest
    || !sameQuote(order.quote, quote)
  ) {
    throw new MerchantError("order-conflict");
  }
  if (order.state === "paid-verified") {
    if (
      order.receiptDigest === undefined
      || !/^[0-9a-f]{64}$/u.test(order.receiptDigest)
      || order.transaction === undefined
      || order.transaction.length === 0
      || order.transaction.length > 512
      || order.transaction.includes("\0")
    ) {
      throw new MerchantError("order-conflict");
    }
  } else if (order.receiptDigest !== undefined || order.transaction !== undefined) {
    throw new MerchantError("order-conflict");
  }
}

function sameQuote(left: MerchantQuote, right: MerchantQuote): boolean {
  try {
    return JSON.stringify(left) === JSON.stringify(right);
  } catch {
    return false;
  }
}

function parseSettlementWebhook(value: Readonly<Record<string, JsonValue>>): SettlementWebhookEvent {
  const allowed = new Set(["order_id", "request_digest", "receipt_digest", "receipt_ref", "transaction", "verification"]);
  if (Object.keys(value).some((key) => !allowed.has(key))) {
    throw new MerchantError("invalid-webhook");
  }
  const verification = value["verification"];
  const levels = new Set(["sequencer-signed", "batch-included", "state-proven", "checkpoint-finalised", "settlement-anchored"]);
  if (typeof verification !== "string" || !levels.has(verification)) {
    throw new MerchantError("invalid-webhook");
  }
  const event = {
    order_id: requiredField(value, "order_id", 255),
    request_digest: requiredHex(value, "request_digest"),
    receipt_digest: requiredHex(value, "receipt_digest"),
    receipt_ref: requiredField(value, "receipt_ref", 512),
    transaction: requiredField(value, "transaction", 512),
    verification,
  };
  return event as SettlementWebhookEvent;
}

function verificationRank(
  level: SettlementWebhookEvent["verification"] | ReceiptVerification["level"],
): number {
  switch (level) {
    case "sequencer-signed": return 1;
    case "batch-included": return 2;
    case "state-proven": return 3;
    case "checkpoint-finalised": return 4;
    case "settlement-anchored": return 5;
  }
}

function constantTimeHex(actual: string, expected: string): boolean {
  if (!/^[0-9a-f]{64}$/u.test(actual) || !/^[0-9a-f]{64}$/u.test(expected)) return false;
  let difference = 0;
  for (let index = 0; index < actual.length; index += 1) {
    difference |= actual.charCodeAt(index) ^ expected.charCodeAt(index);
  }
  return difference === 0;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function layerXReceiptDigest(extensions: Readonly<Record<string, JsonValue>> | undefined): string {
  const layerx = extensions?.["layerx"];
  if (layerx === null || typeof layerx !== "object" || Array.isArray(layerx)) {
    throw new MiddlewareError("verification-failure");
  }
  const digest = layerx["receiptDigest"];
  if (typeof digest !== "string" || !/^[0-9a-f]{64}$/u.test(digest)) {
    throw new MiddlewareError("verification-failure");
  }
  return digest;
}

async function digestQuote(quote: MerchantQuote): Promise<string> {
  const canonical = JSON.stringify({
    lines: quote.lines,
    totalAmount: quote.totalAmount,
    asset: quote.asset,
    payTo: quote.payTo,
    scheme: quote.scheme,
    network: quote.network,
    maxTimeoutSeconds: quote.maxTimeoutSeconds,
    paymentRequired: quote.paymentRequired,
  });
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonical)));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function parseAmount(value: string): bigint {
  if (!/^(0|[1-9][0-9]*)$/u.test(value)) {
    throw new MerchantError("invalid-cart");
  }
  const amount = BigInt(value);
  if (amount > MAX_U128) {
    throw new MerchantError("amount-overflow");
  }
  return amount;
}

function checkedMultiply(left: bigint, right: bigint): bigint {
  const result = left * right;
  if (result > MAX_U128) throw new MerchantError("amount-overflow");
  return result;
}

function checkedAdd(left: bigint, right: bigint): bigint {
  const result = left + right;
  if (result > MAX_U128) throw new MerchantError("amount-overflow");
  return result;
}

function requiredField(value: Readonly<Record<string, JsonValue>>, name: string, maximum: number): string {
  const field = value[name];
  if (typeof field !== "string") throw new MerchantError("invalid-webhook");
  requireText(field, maximum);
  return field;
}

function requiredHex(value: Readonly<Record<string, JsonValue>>, name: string): string {
  const field = requiredField(value, name, 64);
  if (!/^[0-9a-f]{64}$/u.test(field)) throw new MerchantError("invalid-webhook");
  return field;
}

function requireIdentifier(value: string, maximum: number): void {
  if (value.length === 0 || value.length > maximum || !/^[A-Za-z0-9._-]+$/u.test(value)) {
    throw new MerchantError("invalid-cart");
  }
}

function requireText(value: string, maximum: number): void {
  if (value.length === 0 || value.length > maximum || value.includes("\0")) {
    throw new MerchantError("invalid-cart");
  }
}

function requireNetwork(value: string): void {
  const parts = value.split(":");
  if (parts.length !== 2) throw new MerchantError("invalid-cart");
  requireIdentifier(parts[0] ?? "", 32);
  requireIdentifier(parts[1] ?? "", 64);
}

function requireUrl(value: string): void {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    throw new MerchantError("invalid-cart");
  }
  if ((parsed.protocol !== "https:" && parsed.hostname !== "127.0.0.1" && parsed.hostname !== "localhost") || value.length > 2048) {
    throw new MerchantError("invalid-cart");
  }
}
