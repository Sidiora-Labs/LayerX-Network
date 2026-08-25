import {
  PlatformSdkError,
  verifyReceipt,
  type AuthorizedReceiptBatch,
  type ReceiptVerification,
} from "@sidiora/layerx-sdk";

export const X402_VERSION = 2 as const;
export const PAYMENT_REQUIRED_HEADER = "PAYMENT-REQUIRED" as const;
export const PAYMENT_SIGNATURE_HEADER = "PAYMENT-SIGNATURE" as const;
export const PAYMENT_RESPONSE_HEADER = "PAYMENT-RESPONSE" as const;

const MAX_U128 = 340282366920938463463374607431768211455n;
const MAX_HEADER_BYTES = 64 * 1024;
const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");
const PAYMENT_KEY_DOMAIN = new TextEncoder().encode("LayerX/middleware/x402/idempotency\0");

export type JsonValue = null | boolean | number | string | JsonValue[] | { readonly [key: string]: JsonValue };

export interface ResourceInfo {
  readonly url: string;
  readonly description?: string;
  readonly mimeType?: string;
  readonly serviceName?: string;
  readonly tags?: readonly string[];
  readonly iconUrl?: string;
}

export interface X402Extension {
  readonly info: JsonValue;
  readonly schema: JsonValue;
}

export interface PaymentRequirements {
  readonly scheme: string;
  readonly network: string;
  readonly amount: string;
  readonly asset: string;
  readonly payTo: string;
  readonly maxTimeoutSeconds: number;
  readonly extra?: JsonValue;
}

export interface PaymentRequired {
  readonly x402Version: typeof X402_VERSION;
  readonly error?: string;
  readonly resource: ResourceInfo;
  readonly accepts: readonly PaymentRequirements[];
  readonly extensions?: Readonly<Record<string, X402Extension>>;
}

export interface PaymentPayload {
  readonly x402Version: typeof X402_VERSION;
  readonly resource?: ResourceInfo;
  readonly payload: Readonly<Record<string, JsonValue>>;
  readonly accepted: PaymentRequirements;
  readonly extensions?: Readonly<Record<string, X402Extension>>;
}

export interface LayerXReceiptEvidence {
  readonly receipt: string;
  readonly receiptDigest: string;
  readonly verificationLevel: "sequencer-signed";
}

export interface SettlementResponse {
  readonly success: boolean;
  readonly errorReason?: string;
  readonly payer?: string;
  readonly transaction: string;
  readonly network: string;
  readonly amount?: string;
  readonly extensions?: Readonly<Record<string, JsonValue>>;
}

export type MiddlewareErrorCode =
  | "invalid-payment-required"
  | "invalid-payment-payload"
  | "requirements-mismatch"
  | "unsupported-payment"
  | "payment-pending"
  | "payment-refused"
  | "verification-failure"
  | "fulfillment-conflict"
  | "invalid-webhook"
  | "webhook-replay";

export class MiddlewareError extends Error {
  public constructor(public readonly code: MiddlewareErrorCode) {
    super(code);
    this.name = "MiddlewareError";
  }
}

export interface AuthorizedBatchResolver {
  resolve(canonicalReceipt: Uint8Array): Promise<AuthorizedReceiptBatch>;
}

export interface SellerSettlementRequest {
  readonly principal: string;
  readonly payload: PaymentPayload;
  readonly requirements: PaymentRequirements;
  readonly idempotencyKey: string;
  readonly requestDigest: string;
}

export type SellerSettlementOutcome =
  | { readonly kind: "pending" }
  | { readonly kind: "refused"; readonly reason: string }
  | {
    readonly kind: "settled";
    readonly canonicalReceipt: Uint8Array;
    readonly authorizedBatch: AuthorizedReceiptBatch;
  };

export interface SellerPaymentAuthority {
  settle(request: SellerSettlementRequest): Promise<SellerSettlementOutcome>;
}

export interface StoredFulfillment<T> {
  readonly idempotencyKey: string;
  readonly requestDigest: string;
  readonly canonicalReceipt: Uint8Array;
  readonly authorizedBatch: AuthorizedReceiptBatch;
  readonly resource: T;
}

export interface FulfillmentRepository<T> {
  fulfill(
    proposed: Omit<StoredFulfillment<T>, "resource">,
    release: () => Promise<T>,
  ): Promise<StoredFulfillment<T>>;
}

export interface SellerMiddlewareConfig<T> {
  readonly paymentRequired: PaymentRequired;
  readonly authority: SellerPaymentAuthority;
  readonly fulfillments: FulfillmentRepository<T>;
}

export type SellerDecision<T> =
  | {
    readonly kind: "payment-required";
    readonly status: 402;
    readonly headers: Readonly<Record<typeof PAYMENT_REQUIRED_HEADER, string>>;
    readonly body: PaymentRequired;
  }
  | { readonly kind: "pending"; readonly status: 202 }
  | {
    readonly kind: "refused";
    readonly status: 402;
    readonly headers: Readonly<Record<typeof PAYMENT_RESPONSE_HEADER, string>>;
    readonly settlement: SettlementResponse;
  }
  | {
    readonly kind: "released";
    readonly status: 200;
    readonly headers: Readonly<Record<typeof PAYMENT_RESPONSE_HEADER, string>>;
    readonly settlement: SettlementResponse;
    readonly verification: ReceiptVerification;
    readonly resource: T;
  };

export class ReceiptPayloadAuthority implements SellerPaymentAuthority {
  public constructor(private readonly authorizedBatches: AuthorizedBatchResolver) {}

  public async settle(request: SellerSettlementRequest): Promise<SellerSettlementOutcome> {
    const evidence = parseLayerXEvidence(request.payload.payload);
    const canonicalReceipt = decodeBase64(evidence.receipt);
    const digest = await merkleLeafDigest(canonicalReceipt);
    if (!constantTimeEqualHex(evidence.receiptDigest, toHex(digest))) {
      throw new MiddlewareError("verification-failure");
    }
    const authorizedBatch = await this.authorizedBatches.resolve(canonicalReceipt);
    return { kind: "settled", canonicalReceipt, authorizedBatch };
  }
}

export class SellerMiddleware<T> {
  readonly #required: PaymentRequired;
  readonly #authority: SellerPaymentAuthority;
  readonly #fulfillments: FulfillmentRepository<T>;

  public constructor(config: SellerMiddlewareConfig<T>) {
    this.#required = validatePaymentRequired(config.paymentRequired);
    this.#authority = config.authority;
    this.#fulfillments = config.fulfillments;
  }

  public paymentRequired(): Extract<SellerDecision<T>, { readonly kind: "payment-required" }> {
    return {
      kind: "payment-required",
      status: 402,
      headers: { [PAYMENT_REQUIRED_HEADER]: encodePaymentRequiredHeader(this.#required) },
      body: this.#required,
    };
  }

  public async handle(
    principal: string,
    paymentHeader: string | undefined,
    release: () => Promise<T>,
  ): Promise<SellerDecision<T>> {
    if (paymentHeader === undefined) {
      return this.paymentRequired();
    }
    if (!boundedText(principal, 512)) {
      throw new MiddlewareError("invalid-payment-payload");
    }
    const payload = decodePaymentPayloadHeader(paymentHeader);
    const requirements = matchRequirements(this.#required, payload);
    const canonical = canonicalJson(payload);
    const requestDigestBytes = await sha256(new TextEncoder().encode(canonical));
    const requestDigest = toHex(requestDigestBytes);
    const idempotencyKey = toHex(await sha256(
      PAYMENT_KEY_DOMAIN,
      new TextEncoder().encode(principal),
      requestDigestBytes,
    ));
    const outcome = await this.#authority.settle({
      principal,
      payload,
      requirements,
      idempotencyKey,
      requestDigest,
    });
    if (outcome.kind === "pending") {
      return { kind: "pending", status: 202 };
    }
    if (outcome.kind === "refused") {
      return refusalDecision(requirements, outcome.reason);
    }
    const proposed = {
      idempotencyKey,
      requestDigest,
      canonicalReceipt: outcome.canonicalReceipt,
      authorizedBatch: outcome.authorizedBatch,
    };
    const verification = await verifyPaymentReceipt(proposed, requirements);
    const stored = await this.#fulfillments.fulfill(proposed, release);
    if (stored.idempotencyKey !== idempotencyKey || stored.requestDigest !== requestDigest) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    const storedVerification = await verifyPaymentReceipt(stored, requirements);
    if (!equalBytes(verification.receiptDigest, storedVerification.receiptDigest)) {
      throw new MiddlewareError("fulfillment-conflict");
    }
    const receiptDigest = toHex(await merkleLeafDigest(stored.canonicalReceipt));
    const evidence: LayerXReceiptEvidence = {
      receipt: encodeBase64(stored.canonicalReceipt),
      receiptDigest,
      verificationLevel: "sequencer-signed",
    };
    const settlement: SettlementResponse = {
      success: true,
      payer: toHex(storedVerification.receipt.from),
      transaction: `lxp:${receiptDigest}`,
      network: requirements.network,
      amount: requirements.amount,
      extensions: { layerx: evidence as unknown as JsonValue },
    };
    return {
      kind: "released",
      status: 200,
      headers: { [PAYMENT_RESPONSE_HEADER]: encodeSettlementHeader(settlement) },
      settlement,
      verification: storedVerification,
      resource: stored.resource,
    };
  }
}

export function platform_mw_seller(): "receipt-gated-x402-seller" {
  return "receipt-gated-x402-seller";
}

export function encodePaymentRequiredHeader(value: PaymentRequired): string {
  return encodeHeader(validatePaymentRequired(value));
}

export function decodePaymentRequiredHeader(value: string): PaymentRequired {
  return parsePaymentRequired(decodeHeader(value));
}

export function encodePaymentPayloadHeader(value: PaymentPayload): string {
  return encodeHeader(validatePaymentPayload(value));
}

export function decodePaymentPayloadHeader(value: string): PaymentPayload {
  return parsePaymentPayload(decodeHeader(value));
}

export function encodeSettlementHeader(value: SettlementResponse): string {
  return encodeHeader(validateSettlement(value));
}

export function decodeSettlementHeader(value: string): SettlementResponse {
  return parseSettlement(decodeHeader(value));
}

export async function verifyPaymentReceipt(
  evidence: Pick<StoredFulfillment<unknown>, "canonicalReceipt" | "authorizedBatch">,
  requirements: PaymentRequirements,
): Promise<ReceiptVerification> {
  let verified: ReceiptVerification;
  try {
    verified = await verifyReceipt(evidence.canonicalReceipt, evidence.authorizedBatch);
  } catch (error) {
    if (error instanceof PlatformSdkError) {
      throw new MiddlewareError("verification-failure");
    }
    throw error;
  }
  if (
    verified.receipt.amount !== BigInt(requirements.amount)
    || !equalBytes(verified.receipt.asset, parseHex32(requirements.asset))
    || !equalBytes(verified.receipt.to, parseHex32(requirements.payTo))
  ) {
    throw new MiddlewareError("verification-failure");
  }
  return verified;
}

export interface WebhookRequestHeaders {
  readonly id: string;
  readonly timestamp: string;
  readonly keyId: string;
  readonly signature: string;
}

export interface WebhookDeliveryClaim {
  readonly deliveryId: string;
  readonly payloadDigest: string;
  readonly leaseUntilMs: number;
}

export type WebhookClaimResult = "claimed" | "processing" | "completed" | "conflict";

export interface WebhookDeliveryStore {
  claim(value: WebhookDeliveryClaim): Promise<WebhookClaimResult>;
  complete(deliveryId: string, payloadDigest: string): Promise<void>;
  release(deliveryId: string, payloadDigest: string): Promise<void>;
}

export type WebhookConsumeResult = "processed" | "duplicate" | "processing";

export interface VerifiedWebhookConsumerConfig {
  readonly publicKeys: Readonly<Record<string, Uint8Array>>;
  readonly deliveries: WebhookDeliveryStore;
  readonly maximumAgeMs?: number;
  readonly leaseMs?: number;
  readonly now?: () => number;
}

export class VerifiedWebhookConsumer {
  readonly #keys: Readonly<Record<string, Uint8Array>>;
  readonly #deliveries: WebhookDeliveryStore;
  readonly #maximumAgeMs: number;
  readonly #leaseMs: number;
  readonly #now: () => number;

  public constructor(config: VerifiedWebhookConsumerConfig) {
    if (Object.keys(config.publicKeys).length === 0) {
      throw new MiddlewareError("invalid-webhook");
    }
    this.#keys = Object.freeze(Object.fromEntries(
      Object.entries(config.publicKeys).map(([keyId, publicKey]) => {
        if (!identifier(keyId, 64) || publicKey.length !== 32) {
          throw new MiddlewareError("invalid-webhook");
        }
        return [keyId, publicKey.slice()];
      }),
    ));
    this.#deliveries = config.deliveries;
    this.#maximumAgeMs = config.maximumAgeMs ?? 5 * 60 * 1000;
    this.#leaseMs = config.leaseMs ?? 60 * 1000;
    this.#now = config.now ?? Date.now;
    if (this.#maximumAgeMs <= 0 || this.#leaseMs <= 0) {
      throw new MiddlewareError("invalid-webhook");
    }
  }

  public async consume(
    rawBody: Uint8Array,
    headers: WebhookRequestHeaders,
    handle: (event: Readonly<Record<string, JsonValue>>, deliveryId: string) => Promise<void>,
  ): Promise<WebhookConsumeResult> {
    const now = this.#now();
    const timestampSeconds = parseCanonicalInteger(headers.timestamp);
    const timestampMs = Number(timestampSeconds) * 1000;
    if (
      !boundedText(headers.id, 255)
      || !identifier(headers.keyId, 64)
      || !Number.isSafeInteger(timestampMs)
      || timestampMs > now + 30_000
      || now - timestampMs > this.#maximumAgeMs
    ) {
      throw new MiddlewareError("invalid-webhook");
    }
    const publicKey = this.#keys[headers.keyId];
    if (publicKey === undefined || publicKey.length !== 32) {
      throw new MiddlewareError("invalid-webhook");
    }
    const signature = parseWebhookSignature(headers.signature);
    const prefix = new TextEncoder().encode(`${headers.id}.${headers.timestamp}.`);
    let verified = false;
    try {
      const key = await globalThis.crypto.subtle.importKey("raw", arrayBuffer(publicKey), "Ed25519", false, ["verify"]);
      verified = await globalThis.crypto.subtle.verify(
        "Ed25519",
        key,
        arrayBuffer(signature),
        arrayBuffer(concatenate(prefix, rawBody)),
      );
    } catch {
      verified = false;
    }
    if (!verified) {
      throw new MiddlewareError("invalid-webhook");
    }
    const payloadDigest = toHex(await sha256(rawBody));
    const claim = await this.#deliveries.claim({
      deliveryId: headers.id,
      payloadDigest,
      leaseUntilMs: now + this.#leaseMs,
    });
    if (claim === "conflict") {
      throw new MiddlewareError("webhook-replay");
    }
    if (claim === "completed") {
      return "duplicate";
    }
    if (claim === "processing") {
      return "processing";
    }
    let event: Readonly<Record<string, JsonValue>>;
    try {
      event = asObject(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(rawBody)), "invalid-webhook");
      await handle(event, headers.id);
      await this.#deliveries.complete(headers.id, payloadDigest);
    } catch (error) {
      await this.#deliveries.release(headers.id, payloadDigest);
      throw error;
    }
    return "processed";
  }
}

function validatePaymentRequired(value: PaymentRequired): PaymentRequired {
  return parsePaymentRequired(value as unknown);
}

function validatePaymentPayload(value: PaymentPayload): PaymentPayload {
  return parsePaymentPayload(value as unknown);
}

function validateSettlement(value: SettlementResponse): SettlementResponse {
  return parseSettlement(value as unknown);
}

function parsePaymentRequired(value: unknown): PaymentRequired {
  const object = asObject(value, "invalid-payment-required");
  exactKeys(object, ["x402Version", "resource", "accepts"], ["error", "extensions"], "invalid-payment-required");
  if (object["x402Version"] !== X402_VERSION) {
    throw new MiddlewareError("invalid-payment-required");
  }
  const accepts = asArray(object["accepts"], "invalid-payment-required").map(parseRequirements);
  if (accepts.length === 0 || accepts.length > 32) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return {
    x402Version: X402_VERSION,
    resource: parseResource(object["resource"]),
    accepts,
    ...(object["error"] === undefined ? {} : { error: asBoundedString(object["error"], 512, "invalid-payment-required") }),
    ...(object["extensions"] === undefined ? {} : { extensions: parseExtensions(object["extensions"]) }),
  };
}

function parsePaymentPayload(value: unknown): PaymentPayload {
  const object = asObject(value, "invalid-payment-payload");
  exactKeys(object, ["x402Version", "payload", "accepted"], ["resource", "extensions"], "invalid-payment-payload");
  if (object["x402Version"] !== X402_VERSION) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  return {
    x402Version: X402_VERSION,
    payload: asObject(object["payload"], "invalid-payment-payload"),
    accepted: parseRequirements(object["accepted"]),
    ...(object["resource"] === undefined ? {} : { resource: parseResource(object["resource"]) }),
    ...(object["extensions"] === undefined ? {} : { extensions: parseExtensions(object["extensions"]) }),
  };
}

function parseSettlement(value: unknown): SettlementResponse {
  const object = asObject(value, "invalid-payment-payload");
  exactKeys(
    object,
    ["success", "transaction", "network"],
    ["errorReason", "payer", "amount", "extensions"],
    "invalid-payment-payload",
  );
  if (typeof object["success"] !== "boolean") {
    throw new MiddlewareError("invalid-payment-payload");
  }
  const transaction = asString(object["transaction"], "invalid-payment-payload");
  const errorReason = object["errorReason"] === undefined
    ? undefined
    : asBoundedString(object["errorReason"], 512, "invalid-payment-payload");
  if (object["success"] ? transaction.length === 0 || errorReason !== undefined : errorReason === undefined) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  if (!object["success"] && errorReason !== "settlement_pending" && transaction.length !== 0) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  if (!object["success"] && errorReason === "settlement_pending" && transaction.length === 0) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  return {
    success: object["success"],
    transaction,
    network: parseNetwork(object["network"]),
    ...(errorReason === undefined ? {} : { errorReason }),
    ...(object["payer"] === undefined ? {} : { payer: asBoundedString(object["payer"], 256, "invalid-payment-payload") }),
    ...(object["amount"] === undefined ? {} : { amount: parseAmount(object["amount"]) }),
    ...(object["extensions"] === undefined ? {} : { extensions: asJsonRecord(object["extensions"], "invalid-payment-payload") }),
  };
}

function parseResource(value: unknown): ResourceInfo {
  const object = asObject(value, "invalid-payment-required");
  exactKeys(object, ["url"], ["description", "mimeType", "serviceName", "tags", "iconUrl"], "invalid-payment-required");
  const url = parseUrl(object["url"]);
  const tags = object["tags"] === undefined
    ? undefined
    : asArray(object["tags"], "invalid-payment-required").map((tag) => asPrintableString(tag, 32));
  if (tags !== undefined && tags.length > 5) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return {
    url,
    ...(object["description"] === undefined ? {} : { description: asBoundedString(object["description"], 512, "invalid-payment-required") }),
    ...(object["mimeType"] === undefined ? {} : { mimeType: asBoundedString(object["mimeType"], 32, "invalid-payment-required") }),
    ...(object["serviceName"] === undefined ? {} : { serviceName: asPrintableString(object["serviceName"], 32) }),
    ...(tags === undefined ? {} : { tags }),
    ...(object["iconUrl"] === undefined ? {} : { iconUrl: parseUrl(object["iconUrl"]) }),
  };
}

function parseRequirements(value: unknown): PaymentRequirements {
  const object = asObject(value, "invalid-payment-required");
  exactKeys(
    object,
    ["scheme", "network", "amount", "asset", "payTo", "maxTimeoutSeconds"],
    ["extra"],
    "invalid-payment-required",
  );
  const scheme = asIdentifier(object["scheme"], 32, "invalid-payment-required");
  const maxTimeoutSeconds = object["maxTimeoutSeconds"];
  if (!Number.isSafeInteger(maxTimeoutSeconds) || (maxTimeoutSeconds as number) <= 0 || (maxTimeoutSeconds as number) > 0xffff_ffff) {
    throw new MiddlewareError("invalid-payment-required");
  }
  const asset = asBoundedString(object["asset"], 256, "invalid-payment-required");
  const payTo = asBoundedString(object["payTo"], 256, "invalid-payment-required");
  parseHex32(asset);
  parseHex32(payTo);
  return {
    scheme,
    network: parseNetwork(object["network"]),
    amount: parseAmount(object["amount"]),
    asset,
    payTo,
    maxTimeoutSeconds: maxTimeoutSeconds as number,
    ...(object["extra"] === undefined ? {} : { extra: asJson(object["extra"], "invalid-payment-required") }),
  };
}

function parseExtensions(value: unknown): Readonly<Record<string, X402Extension>> {
  const object = asObject(value, "invalid-payment-required");
  const entries = Object.entries(object);
  if (entries.length > 32) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return Object.fromEntries(entries.map(([name, extension]) => {
    if (!identifier(name, 32)) {
      throw new MiddlewareError("invalid-payment-required");
    }
    const body = asObject(extension, "invalid-payment-required");
    exactKeys(body, ["info", "schema"], [], "invalid-payment-required");
    return [name, { info: asJson(body["info"], "invalid-payment-required"), schema: asJson(body["schema"], "invalid-payment-required") }];
  }));
}

function matchRequirements(required: PaymentRequired, payload: PaymentPayload): PaymentRequirements {
  const match = required.accepts.find((candidate) => canonicalJson(candidate) === canonicalJson(payload.accepted));
  if (match === undefined) {
    throw new MiddlewareError("requirements-mismatch");
  }
  for (const [name, extension] of Object.entries(required.extensions ?? {})) {
    const actual = payload.extensions?.[name];
    if (actual === undefined || canonicalJson(actual) !== canonicalJson(extension)) {
      throw new MiddlewareError("requirements-mismatch");
    }
  }
  return match;
}

function refusalDecision<T>(
  requirements: PaymentRequirements,
  reason: string,
): Extract<SellerDecision<T>, { readonly kind: "refused" }> {
  const safeReason = /^[a-z][a-z0-9_]{0,63}$/u.test(reason) ? reason : "payment_refused";
  const settlement: SettlementResponse = {
    success: false,
    errorReason: safeReason,
    transaction: "",
    network: requirements.network,
  };
  return {
    kind: "refused",
    status: 402,
    headers: { [PAYMENT_RESPONSE_HEADER]: encodeSettlementHeader(settlement) },
    settlement,
  };
}

function parseLayerXEvidence(value: Readonly<Record<string, JsonValue>>): LayerXReceiptEvidence {
  const object = asObject(value, "invalid-payment-payload");
  exactKeys(object, ["receipt", "receiptDigest", "verificationLevel"], ["idempotencyKey"], "invalid-payment-payload");
  if (object["verificationLevel"] !== "sequencer-signed") {
    throw new MiddlewareError("verification-failure");
  }
  const receiptDigest = asString(object["receiptDigest"], "invalid-payment-payload");
  parseHex32(receiptDigest);
  return {
    receipt: asBoundedString(object["receipt"], MAX_HEADER_BYTES, "invalid-payment-payload"),
    receiptDigest,
    verificationLevel: "sequencer-signed",
  };
}

function encodeHeader(value: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  if (bytes.length > MAX_HEADER_BYTES) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  return encodeBase64(bytes);
}

function decodeHeader(value: string): unknown {
  const bytes = decodeBase64(value);
  if (bytes.length > MAX_HEADER_BYTES) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
  } catch {
    throw new MiddlewareError("invalid-payment-payload");
  }
}

function encodeBase64(value: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < value.length; offset += 0x8000) {
    binary += String.fromCharCode(...value.subarray(offset, Math.min(offset + 0x8000, value.length)));
  }
  return btoa(binary);
}

function decodeBase64(value: string): Uint8Array {
  if (value.length === 0 || value.length > MAX_HEADER_BYTES * 2 || !/^[A-Za-z0-9+/]*={0,2}$/u.test(value)) {
    throw new MiddlewareError("invalid-payment-payload");
  }
  try {
    const binary = atob(value);
    return Uint8Array.from(binary, (character) => character.charCodeAt(0));
  } catch {
    throw new MiddlewareError("invalid-payment-payload");
  }
}

function parseWebhookSignature(value: string): Uint8Array {
  const encoded = value.startsWith("v1=") ? value.slice(3) : "";
  const signature = decodeBase64(encoded);
  if (signature.length !== 64) {
    throw new MiddlewareError("invalid-webhook");
  }
  return signature;
}

function canonicalJson(value: unknown): string {
  if (value === null || typeof value === "boolean" || typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new MiddlewareError("invalid-payment-payload");
    }
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(",")}]`;
  }
  if (typeof value === "object" && value !== null) {
    return `{${Object.entries(value as Record<string, unknown>)
      .filter(([, item]) => item !== undefined)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, item]) => `${JSON.stringify(key)}:${canonicalJson(item)}`)
      .join(",")}}`;
  }
  throw new MiddlewareError("invalid-payment-payload");
}

function asJson(value: unknown, code: MiddlewareErrorCode): JsonValue {
  canonicalJson(value);
  return value as JsonValue;
}

function asJsonRecord(value: unknown, code: MiddlewareErrorCode): Readonly<Record<string, JsonValue>> {
  const object = asObject(value, code);
  for (const item of Object.values(object)) {
    asJson(item, code);
  }
  return object;
}

function asObject(value: unknown, code: MiddlewareErrorCode): Readonly<Record<string, JsonValue>> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new MiddlewareError(code);
  }
  return value as Readonly<Record<string, JsonValue>>;
}

function asArray(value: unknown, code: MiddlewareErrorCode): readonly unknown[] {
  if (!Array.isArray(value)) {
    throw new MiddlewareError(code);
  }
  return value;
}

function exactKeys(
  value: Readonly<Record<string, JsonValue>>,
  required: readonly string[],
  optional: readonly string[],
  code: MiddlewareErrorCode,
): void {
  const allowed = new Set([...required, ...optional]);
  if (required.some((key) => value[key] === undefined) || Object.keys(value).some((key) => !allowed.has(key))) {
    throw new MiddlewareError(code);
  }
}

function asString(value: unknown, code: MiddlewareErrorCode): string {
  if (typeof value !== "string") {
    throw new MiddlewareError(code);
  }
  return value;
}

function asBoundedString(value: unknown, limit: number, code: MiddlewareErrorCode): string {
  const text = asString(value, code);
  if (!boundedText(text, limit)) {
    throw new MiddlewareError(code);
  }
  return text;
}

function asPrintableString(value: unknown, limit: number): string {
  const text = asBoundedString(value, limit, "invalid-payment-required");
  if (!/^[\x20-\x7e]+$/u.test(text)) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return text;
}

function asIdentifier(value: unknown, limit: number, code: MiddlewareErrorCode): string {
  const text = asString(value, code);
  if (!identifier(text, limit)) {
    throw new MiddlewareError(code);
  }
  return text;
}

function parseNetwork(value: unknown): string {
  const text = asString(value, "invalid-payment-required");
  const parts = text.split(":");
  if (parts.length !== 2 || parts[0] !== "layerx" || !identifier(parts[1] ?? "", 64)) {
    throw new MiddlewareError("unsupported-payment");
  }
  return text;
}

function parseUrl(value: unknown): string {
  const text = asBoundedString(value, 2048, "invalid-payment-required");
  let url: URL;
  try {
    url = new URL(text);
  } catch {
    throw new MiddlewareError("invalid-payment-required");
  }
  if ((url.protocol !== "https:" && url.protocol !== "http:") || /[\r\n\0]/u.test(text)) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return text;
}

function parseAmount(value: unknown): string {
  const text = asString(value, "invalid-payment-required");
  if (!/^(0|[1-9][0-9]*)$/u.test(text)) {
    throw new MiddlewareError("invalid-payment-required");
  }
  const amount = BigInt(text);
  if (amount <= 0n || amount > MAX_U128) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return text;
}

function parseCanonicalInteger(value: string): bigint {
  if (!/^(0|[1-9][0-9]*)$/u.test(value)) {
    throw new MiddlewareError("invalid-webhook");
  }
  return BigInt(value);
}

function boundedText(value: string, limit: number): boolean {
  return value.length > 0 && value.length <= limit && !value.includes("\0");
}

function identifier(value: string, limit: number): boolean {
  return boundedText(value, limit) && /^[A-Za-z0-9._-]+$/u.test(value);
}

function parseHex32(value: string): Uint8Array {
  const digits = value.startsWith("0x") ? value.slice(2) : value;
  if (!/^[0-9a-fA-F]{64}$/u.test(digits)) {
    throw new MiddlewareError("invalid-payment-required");
  }
  return Uint8Array.from({ length: 32 }, (_, index) => Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16));
}

function toHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function constantTimeEqualHex(left: string, right: string): boolean {
  if (left.length !== right.length) {
    return false;
  }
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= left.charCodeAt(index) ^ right.charCodeAt(index);
  }
  return difference === 0;
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) {
    return false;
  }
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) {
    difference |= (left[index] ?? 0) ^ (right[index] ?? 0);
  }
  return difference === 0;
}

function arrayBuffer(value: Uint8Array): ArrayBuffer {
  const result = new ArrayBuffer(value.length);
  new Uint8Array(result).set(value);
  return result;
}

function concatenate(...values: readonly Uint8Array[]): Uint8Array {
  const result = new Uint8Array(values.reduce((total, value) => total + value.length, 0));
  let offset = 0;
  for (const value of values) {
    result.set(value, offset);
    offset += value.length;
  }
  return result;
}

async function sha256(...values: readonly Uint8Array[]): Promise<Uint8Array> {
  return new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", arrayBuffer(concatenate(...values))));
}

async function merkleLeafDigest(canonicalReceipt: Uint8Array): Promise<Uint8Array> {
  return sha256(MERKLE_LEAF_DOMAIN, canonicalReceipt);
}
