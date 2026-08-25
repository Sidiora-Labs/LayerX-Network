import {
  BuyerMiddleware,
  LayerXPaymentHttpTransport,
  type BuyerSupportedKind,
} from "@sidiora/layerx-buyer-middleware";
import {
  ProductionClient,
  SecretBytes,
  type AuthorizedReceiptBatch,
} from "@sidiora/layerx-sdk";
import {
  MiddlewareError,
  PAYMENT_REQUIRED_HEADER,
  PAYMENT_RESPONSE_HEADER,
  PAYMENT_SIGNATURE_HEADER,
  ReceiptPayloadAuthority,
  SellerMiddleware,
  VerifiedWebhookConsumer,
  X402_VERSION,
  type AuthorizedBatchResolver,
  type FulfillmentRepository,
  type JsonValue,
  type MiddlewareErrorCode,
  type PaymentRequired,
  type PaymentRequirements,
  type SellerDecision,
  type SellerPaymentAuthority,
  type WebhookClaimResult,
  type WebhookDeliveryClaim,
  type WebhookDeliveryStore,
  type WebhookRequestHeaders,
} from "@sidiora/layerx-seller-middleware";

export {
  MiddlewareError,
  PAYMENT_REQUIRED_HEADER,
  PAYMENT_RESPONSE_HEADER,
  PAYMENT_SIGNATURE_HEADER,
  ReceiptPayloadAuthority,
  SellerMiddleware,
  VerifiedWebhookConsumer,
  X402_VERSION,
  verifyPaymentReceipt,
} from "@sidiora/layerx-seller-middleware";
export type {
  AuthorizedBatchResolver,
  FulfillmentRepository,
  JsonValue,
  MiddlewareErrorCode,
  PaymentPayload,
  PaymentRequired,
  PaymentRequirements,
  SellerDecision,
  SellerPaymentAuthority,
  SellerSettlementOutcome,
  SellerSettlementRequest,
  SettlementResponse,
  StoredFulfillment,
  WebhookClaimResult,
  WebhookConsumeResult,
  WebhookDeliveryClaim,
  WebhookDeliveryStore,
  WebhookRequestHeaders,
} from "@sidiora/layerx-seller-middleware";
export { BuyerMiddleware, LayerXPaymentHttpTransport } from "@sidiora/layerx-buyer-middleware";
export type { BuyerSupportedKind, PaidFetchResult, PreparedPayment } from "@sidiora/layerx-buyer-middleware";
export { ProductionClient, SecretBytes } from "@sidiora/layerx-sdk";
export type { AuthorizedReceiptBatch, ReceiptVerification } from "@sidiora/layerx-sdk";
export * from "./scan.js";

export const WEBHOOK_ID_HEADER = "layerx-webhook-id" as const;
export const WEBHOOK_TIMESTAMP_HEADER = "layerx-webhook-timestamp" as const;
export const WEBHOOK_KEY_HEADER = "layerx-webhook-key-id" as const;
export const WEBHOOK_SIGNATURE_HEADER = "layerx-webhook-signature" as const;

export const DECLARED_KEYS = [
  "LAYERX_PRINCIPAL",
  "LAYERX_PROTECTED_PATH",
  "LAYERX_RESOURCE_URL",
  "LAYERX_RESOURCE_DESCRIPTION",
  "LAYERX_RESOURCE_MIME_TYPE",
  "LAYERX_RESOURCE_SERVICE_NAME",
  "LAYERX_X402_SCHEME",
  "LAYERX_X402_NETWORK",
  "LAYERX_PRICE",
  "LAYERX_ASSET",
  "LAYERX_PAY_TO",
  "LAYERX_PAYMENT_TIMEOUT_SECONDS",
  "LAYERX_AUTHORIZED_BATCH_JSON",
  "LAYERX_WEBHOOK_PATH",
  "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON",
  "LAYERX_WEBHOOK_MAX_AGE_MS",
  "LAYERX_WEBHOOK_LEASE_MS",
  "LAYERX_HUMAN_URL",
  "LAYERX_SOURCE",
  "LAYERX_TOKEN",
] as const;

export type DeclaredKey = (typeof DECLARED_KEYS)[number];

export const SECRET_DECLARED_KEYS: readonly DeclaredKey[] = ["LAYERX_TOKEN"];

const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");
const MAXIMUM_WEBHOOK_BYTES = 1_048_576;

export type LayerXIntegrationErrorCode =
  | "missing-declared-key"
  | "invalid-declared-key"
  | "client-runtime-refused"
  | "published-secret"
  | "unverifiable-body"
  | "receipt-not-backed";

export class LayerXIntegrationError extends Error {
  public constructor(public readonly code: LayerXIntegrationErrorCode) {
    super(code);
    this.name = "LayerXIntegrationError";
  }
}

export type Environment = Readonly<Record<string, string | undefined>>;

export type RouteHandler = (request: Request) => Promise<Response>;

export interface LayerXResourceRoute {
  readonly GET: RouteHandler;
  readonly POST: RouteHandler;
}

export interface LayerXWebhookRoute {
  readonly POST: RouteHandler;
}

export interface LayerXWebhookSettings {
  readonly path: string;
  readonly publicKeys: Readonly<Record<string, Uint8Array>>;
  readonly maximumAgeMs: number;
  readonly leaseMs: number;
}

export interface LayerXBuyerSettings {
  readonly humanUrl: string;
  readonly source: string;
  readonly supported: readonly BuyerSupportedKind[];
}

export interface LayerXDeclaredConfig {
  readonly principal: string;
  readonly protectedPath: string;
  readonly paymentRequired: PaymentRequired;
  readonly requirements: PaymentRequirements;
  readonly authorizedBatch: AuthorizedReceiptBatch;
  readonly webhook: LayerXWebhookSettings;
  readonly buyer?: LayerXBuyerSettings;
}

export interface LayerXResource {
  readonly contentType: string;
  readonly body: string;
}

export interface LayerXResourceHandler {
  release(request: Request): Promise<LayerXResource>;
}

export interface LayerXWebhookHandlerConsumer {
  handle(event: Readonly<Record<string, JsonValue>>, deliveryId: string): Promise<void>;
}

export interface LayerXMountOptions {
  readonly environment: Environment;
  readonly resources: LayerXResourceHandler;
  readonly fulfillments: FulfillmentRepository<LayerXResource>;
  readonly deliveries: WebhookDeliveryStore;
  readonly events: LayerXWebhookHandlerConsumer;
  readonly authorizedBatches?: AuthorizedBatchResolver;
  readonly authority?: SellerPaymentAuthority;
  readonly now?: () => number;
  readonly fetch?: typeof globalThis.fetch;
}

export interface LayerXNextMount {
  readonly config: LayerXDeclaredConfig;
  readonly seller: SellerMiddleware<LayerXResource>;
  readonly webhooks: VerifiedWebhookConsumer;
  readonly resource: LayerXResourceRoute;
  readonly webhook: LayerXWebhookRoute;
  readonly buyer?: BuyerMiddleware;
  destroy(): void;
}

export function platform_int_next(): "receipt-gated-x402-next" {
  return "receipt-gated-x402-next";
}

export function assertServerRuntime(): void {
  const scope = globalThis as { readonly window?: unknown; readonly document?: unknown };
  if (scope.window !== undefined || scope.document !== undefined) {
    throw new LayerXIntegrationError("client-runtime-refused");
  }
}

export function assertNoPublishedSecrets(environment: Environment): void {
  const secrets = SECRET_DECLARED_KEYS
    .map((key) => environment[key])
    .filter((value): value is string => value !== undefined && value.length > 0);
  for (const [name, value] of Object.entries(environment)) {
    if (!isPublishedName(name)) {
      continue;
    }
    if (looksLikeKeyMaterial(name)) {
      throw new LayerXIntegrationError("published-secret");
    }
    if (value !== undefined && secrets.some((secret) => secret === value)) {
      throw new LayerXIntegrationError("published-secret");
    }
  }
}

export function readDeclaredConfig(environment: Environment): LayerXDeclaredConfig {
  assertServerRuntime();
  assertNoPublishedSecrets(environment);
  const scheme = required(environment, "LAYERX_X402_SCHEME");
  const network = required(environment, "LAYERX_X402_NETWORK");
  const requirements: PaymentRequirements = {
    scheme,
    network,
    amount: required(environment, "LAYERX_PRICE"),
    asset: required(environment, "LAYERX_ASSET"),
    payTo: required(environment, "LAYERX_PAY_TO"),
    maxTimeoutSeconds: positiveInteger(required(environment, "LAYERX_PAYMENT_TIMEOUT_SECONDS")),
  };
  const description = optional(environment, "LAYERX_RESOURCE_DESCRIPTION");
  const mimeType = optional(environment, "LAYERX_RESOURCE_MIME_TYPE");
  const serviceName = optional(environment, "LAYERX_RESOURCE_SERVICE_NAME");
  const paymentRequired: PaymentRequired = {
    x402Version: X402_VERSION,
    resource: {
      url: required(environment, "LAYERX_RESOURCE_URL"),
      ...(description === undefined ? {} : { description }),
      ...(mimeType === undefined ? {} : { mimeType }),
      ...(serviceName === undefined ? {} : { serviceName }),
    },
    accepts: [requirements],
  };
  const humanUrl = optional(environment, "LAYERX_HUMAN_URL");
  const source = optional(environment, "LAYERX_SOURCE");
  const buyer: LayerXBuyerSettings | undefined = humanUrl === undefined || source === undefined
    ? undefined
    : { humanUrl, source, supported: [{ scheme, network }] };
  return {
    principal: required(environment, "LAYERX_PRINCIPAL"),
    protectedPath: routePath(required(environment, "LAYERX_PROTECTED_PATH")),
    paymentRequired,
    requirements,
    authorizedBatch: parseAuthorizedBatch(required(environment, "LAYERX_AUTHORIZED_BATCH_JSON")),
    webhook: {
      path: routePath(required(environment, "LAYERX_WEBHOOK_PATH")),
      publicKeys: parseWebhookKeys(required(environment, "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON")),
      maximumAgeMs: positiveInteger(optional(environment, "LAYERX_WEBHOOK_MAX_AGE_MS") ?? "300000"),
      leaseMs: positiveInteger(optional(environment, "LAYERX_WEBHOOK_LEASE_MS") ?? "60000"),
    },
    ...(buyer === undefined ? {} : { buyer }),
  };
}

export function staticAuthorizedBatches(batch: AuthorizedReceiptBatch): AuthorizedBatchResolver {
  return { resolve: () => Promise.resolve(batch) };
}

export class SingleProcessWebhookDeliveryStore implements WebhookDeliveryStore {
  readonly #entries = new Map<string, { payloadDigest: string; leaseUntilMs: number; completed: boolean }>();
  readonly #now: () => number;

  public constructor(now?: () => number) {
    this.#now = now ?? Date.now;
  }

  public claim(value: WebhookDeliveryClaim): Promise<WebhookClaimResult> {
    const existing = this.#entries.get(value.deliveryId);
    if (existing === undefined) {
      this.#entries.set(value.deliveryId, {
        payloadDigest: value.payloadDigest,
        leaseUntilMs: value.leaseUntilMs,
        completed: false,
      });
      return Promise.resolve("claimed");
    }
    if (existing.payloadDigest !== value.payloadDigest) {
      return Promise.resolve("conflict");
    }
    if (existing.completed) {
      return Promise.resolve("completed");
    }
    if (existing.leaseUntilMs > this.#now()) {
      return Promise.resolve("processing");
    }
    existing.leaseUntilMs = value.leaseUntilMs;
    return Promise.resolve("claimed");
  }

  public complete(deliveryId: string, payloadDigest: string): Promise<void> {
    const existing = this.#entries.get(deliveryId);
    if (existing === undefined || existing.payloadDigest !== payloadDigest) {
      throw new MiddlewareError("webhook-replay");
    }
    existing.completed = true;
    existing.leaseUntilMs = 0;
    return Promise.resolve();
  }

  public release(deliveryId: string, payloadDigest: string): Promise<void> {
    const existing = this.#entries.get(deliveryId);
    if (existing !== undefined && existing.payloadDigest === payloadDigest && !existing.completed) {
      this.#entries.delete(deliveryId);
    }
    return Promise.resolve();
  }
}

export function mountLayerX(options: LayerXMountOptions): LayerXNextMount {
  const config = readDeclaredConfig(options.environment);
  const authorizedBatches = options.authorizedBatches ?? staticAuthorizedBatches(config.authorizedBatch);
  const seller = new SellerMiddleware<LayerXResource>({
    paymentRequired: config.paymentRequired,
    authority: options.authority ?? new ReceiptPayloadAuthority(authorizedBatches),
    fulfillments: options.fulfillments,
  });
  const webhooks = new VerifiedWebhookConsumer({
    publicKeys: config.webhook.publicKeys,
    deliveries: options.deliveries,
    maximumAgeMs: config.webhook.maximumAgeMs,
    leaseMs: config.webhook.leaseMs,
    ...(options.now === undefined ? {} : { now: options.now }),
  });
  const token = optional(options.environment, "LAYERX_TOKEN");
  const secret = config.buyer === undefined || token === undefined
    ? undefined
    : new SecretBytes(new TextEncoder().encode(token));
  const buyer = config.buyer === undefined || secret === undefined
    ? undefined
    : new BuyerMiddleware({
      client: new ProductionClient(new LayerXPaymentHttpTransport({
        baseUrl: config.buyer.humanUrl,
        bearerToken: secret,
        ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
      })),
      source: config.buyer.source,
      supported: config.buyer.supported,
      authorizedBatches,
      ...(options.now === undefined ? {} : { now: options.now }),
      ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
    });
  const runtime: LayerXSellerRuntime = {
    seller,
    requirements: config.requirements,
    principal: config.principal,
    resources: options.resources,
  };
  const handler = createResourceHandler(runtime);
  return {
    config,
    seller,
    webhooks,
    resource: { GET: handler, POST: handler },
    webhook: { POST: createWebhookHandler(webhooks, options.events) },
    ...(buyer === undefined ? {} : { buyer }),
    destroy: () => {
      secret?.destroy();
    },
  };
}

export function createLayerXResourceRoute(options: LayerXMountOptions): LayerXResourceRoute {
  return mountLayerX(options).resource;
}

export function createLayerXWebhookRoute(options: LayerXMountOptions): LayerXWebhookRoute {
  return mountLayerX(options).webhook;
}

export interface LayerXSellerRuntime {
  readonly seller: SellerMiddleware<LayerXResource>;
  readonly requirements: PaymentRequirements;
  readonly principal: string;
  readonly resources: LayerXResourceHandler;
}

export async function assertReceiptBacked(
  decision: Extract<SellerDecision<LayerXResource>, { readonly kind: "released" }>,
  requirements: PaymentRequirements,
): Promise<void> {
  const evidenceDigest = layerXEvidenceDigest(decision.settlement.extensions);
  const receiptDigest = toHex(await merkleLeafDigest(decision.verification.canonicalBytes));
  if (
    decision.verification.level !== "sequencer-signed"
    || decision.verification.receipt.resultCode !== 0
    || !decision.settlement.success
    || decision.settlement.network !== requirements.network
    || decision.settlement.amount !== requirements.amount
    || decision.settlement.transaction !== `lxp:${receiptDigest}`
    || !constantTimeEqualText(evidenceDigest, receiptDigest)
    || decision.verification.receipt.amount !== BigInt(requirements.amount)
    || !equalBytes(decision.verification.receipt.asset, parseHex32(requirements.asset))
    || !equalBytes(decision.verification.receipt.to, parseHex32(requirements.payTo))
  ) {
    throw new LayerXIntegrationError("receipt-not-backed");
  }
}

function createResourceHandler(runtime: LayerXSellerRuntime): RouteHandler {
  return async (request) => {
    let decision: SellerDecision<LayerXResource>;
    try {
      decision = await runtime.seller.handle(
        runtime.principal,
        request.headers.get(PAYMENT_SIGNATURE_HEADER) ?? undefined,
        () => runtime.resources.release(request),
      );
    } catch (error) {
      if (error instanceof MiddlewareError) {
        return jsonResponse(paymentErrorStatus(error.code), { error: error.code });
      }
      throw error;
    }
    if (decision.kind === "payment-required") {
      return new Response(JSON.stringify(decision.body), {
        status: decision.status,
        headers: new Headers({
          "content-type": "application/json",
          [PAYMENT_REQUIRED_HEADER]: decision.headers[PAYMENT_REQUIRED_HEADER],
        }),
      });
    }
    if (decision.kind === "pending") {
      return new Response(null, { status: decision.status, headers: new Headers({ "retry-after": "1" }) });
    }
    if (decision.kind === "refused") {
      return new Response(null, {
        status: decision.status,
        headers: new Headers({ [PAYMENT_RESPONSE_HEADER]: decision.headers[PAYMENT_RESPONSE_HEADER] }),
      });
    }
    await assertReceiptBacked(decision, runtime.requirements);
    return new Response(decision.resource.body, {
      status: decision.status,
      headers: new Headers({
        "content-type": decision.resource.contentType,
        [PAYMENT_RESPONSE_HEADER]: decision.headers[PAYMENT_RESPONSE_HEADER],
      }),
    });
  };
}

function createWebhookHandler(
  consumer: VerifiedWebhookConsumer,
  events: LayerXWebhookHandlerConsumer,
): RouteHandler {
  return async (request) => {
    const headers = webhookHeaders(request.headers);
    const rawBody = await readRawBody(request);
    try {
      const outcome = await consumer.consume(rawBody, headers, (event, deliveryId) => events.handle(event, deliveryId));
      if (outcome === "processed") {
        return new Response(null, { status: 204 });
      }
      if (outcome === "duplicate") {
        return jsonResponse(200, { outcome });
      }
      return new Response(JSON.stringify({ outcome }), {
        status: 409,
        headers: new Headers({ "content-type": "application/json", "retry-after": "1" }),
      });
    } catch (error) {
      if (error instanceof MiddlewareError && error.code === "invalid-webhook") {
        return jsonResponse(401, { error: error.code });
      }
      if (error instanceof MiddlewareError && error.code === "webhook-replay") {
        return jsonResponse(409, { error: error.code });
      }
      throw error;
    }
  };
}

function webhookHeaders(headers: Headers): WebhookRequestHeaders {
  const id = headers.get(WEBHOOK_ID_HEADER);
  const timestamp = headers.get(WEBHOOK_TIMESTAMP_HEADER);
  const keyId = headers.get(WEBHOOK_KEY_HEADER);
  const signature = headers.get(WEBHOOK_SIGNATURE_HEADER);
  if (id === null || timestamp === null || keyId === null || signature === null) {
    throw new MiddlewareError("invalid-webhook");
  }
  return { id, timestamp, keyId, signature };
}

async function readRawBody(request: Request): Promise<Uint8Array> {
  if (request.body === null) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (let step = await reader.read(); step.done !== true; step = await reader.read()) {
    total += step.value.length;
    if (total > MAXIMUM_WEBHOOK_BYTES) {
      await reader.cancel();
      throw new MiddlewareError("invalid-webhook");
    }
    chunks.push(step.value);
  }
  const body = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.length;
  }
  return body;
}

function jsonResponse(status: number, body: Readonly<Record<string, string>>): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: new Headers({ "content-type": "application/json" }),
  });
}

function paymentErrorStatus(code: MiddlewareErrorCode): number {
  if (code === "payment-pending") {
    return 202;
  }
  if (code === "fulfillment-conflict") {
    return 409;
  }
  return 402;
}

function layerXEvidenceDigest(extensions: Readonly<Record<string, JsonValue>> | undefined): string {
  const layerx = extensions?.["layerx"];
  if (typeof layerx !== "object" || layerx === null || Array.isArray(layerx)) {
    throw new LayerXIntegrationError("receipt-not-backed");
  }
  const digest = (layerx as Readonly<Record<string, JsonValue>>)["receiptDigest"];
  if (typeof digest !== "string" || !/^[0-9a-f]{64}$/u.test(digest)) {
    throw new LayerXIntegrationError("receipt-not-backed");
  }
  return digest;
}

function required(environment: Environment, key: DeclaredKey): string {
  const value = environment[key];
  if (value === undefined || value.length === 0) {
    throw new LayerXIntegrationError("missing-declared-key");
  }
  return value;
}

function optional(environment: Environment, key: DeclaredKey): string | undefined {
  const value = environment[key];
  return value === undefined || value.length === 0 ? undefined : value;
}

function positiveInteger(value: string): number {
  if (!/^[1-9][0-9]*$/u.test(value)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return parsed;
}

function routePath(value: string): string {
  if (!value.startsWith("/") || value.length > 512 || /[\s?#]/u.test(value)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return value;
}

function parseAuthorizedBatch(value: string): AuthorizedReceiptBatch {
  const parsed = parseJsonObject(value);
  return {
    batchId: parseHex32(requiredText(parsed["batchId"])),
    asset: parseHex32(requiredText(parsed["asset"])),
    previousStateRoot: parseHex32(requiredText(parsed["previousStateRoot"])),
    resultingStateRoot: parseHex32(requiredText(parsed["resultingStateRoot"])),
    sequencerPublicKey: parseHex32(requiredText(parsed["sequencerPublicKey"])),
  };
}

function parseWebhookKeys(value: string): Readonly<Record<string, Uint8Array>> {
  const parsed = parseJsonObject(value);
  const entries = Object.entries(parsed);
  if (entries.length === 0 || entries.length > 32) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return Object.fromEntries(entries.map(([keyId, encoded]) => {
    if (!/^[A-Za-z0-9._-]{1,64}$/u.test(keyId)) {
      throw new LayerXIntegrationError("invalid-declared-key");
    }
    const key = decodeBase64(requiredText(encoded));
    if (key.length !== 32) {
      throw new LayerXIntegrationError("invalid-declared-key");
    }
    return [keyId, key];
  }));
}

function parseJsonObject(value: string): Readonly<Record<string, JsonValue>> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return parsed as Readonly<Record<string, JsonValue>>;
}

function requiredText(value: JsonValue | undefined): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return value;
}

function isPublishedName(name: string): boolean {
  return name.startsWith("NEXT_PUBLIC_")
    || name.startsWith("PUBLIC_")
    || name.startsWith("VITE_")
    || name.startsWith("REACT_APP_")
    || name.startsWith("EXPO_PUBLIC_");
}

function looksLikeKeyMaterial(name: string): boolean {
  return /(^|_)(TOKEN|SECRET|PRIVATE|CREDENTIAL|PASSWORD|SIGNING_KEY|API_KEY)(_|$)/u.test(name);
}

function parseHex32(value: string): Uint8Array {
  const digits = value.startsWith("0x") ? value.slice(2) : value;
  if (!/^[0-9a-fA-F]{64}$/u.test(digits)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  return Uint8Array.from({ length: 32 }, (_, index) => Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16));
}

function decodeBase64(value: string): Uint8Array {
  if (value.length === 0 || !/^[A-Za-z0-9+/]*={0,2}$/u.test(value)) {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
  try {
    return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
  } catch {
    throw new LayerXIntegrationError("invalid-declared-key");
  }
}

function toHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function constantTimeEqualText(left: string, right: string): boolean {
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

async function merkleLeafDigest(canonicalReceipt: Uint8Array): Promise<Uint8Array> {
  const digest = await globalThis.crypto.subtle.digest(
    "SHA-256",
    arrayBuffer(concatenate(MERKLE_LEAF_DOMAIN, canonicalReceipt)),
  );
  return new Uint8Array(digest);
}
