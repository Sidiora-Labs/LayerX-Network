import {
  PlatformSdkError,
  ProductionClient,
  SecretBytes,
  idempotencyKey,
  type AuthorizedReceiptBatch,
  type ProductionTransport,
  type ReceiptVerification,
  type TransportCall,
} from "@sidiora/layerx-sdk";
import {
  MiddlewareError,
  PAYMENT_REQUIRED_HEADER,
  PAYMENT_RESPONSE_HEADER,
  PAYMENT_SIGNATURE_HEADER,
  X402_VERSION,
  decodePaymentRequiredHeader,
  decodeSettlementHeader,
  encodePaymentPayloadHeader,
  verifyPaymentReceipt,
  type AuthorizedBatchResolver,
  type JsonValue,
  type LayerXReceiptEvidence,
  type PaymentPayload,
  type PaymentRequired,
  type PaymentRequirements,
  type ResourceInfo,
  type SettlementResponse,
} from "@sidiora/layerx-seller-middleware";

const MAX_U128 = 340282366920938463463374607431768211455n;
const MERKLE_LEAF_DOMAIN = new TextEncoder().encode("LXP/v1/merkle-leaf\0");
const MAXIMUM_HTTP_RESPONSE_BYTES = 8 * 1024 * 1024;
const HTTP_REQUEST_TIMEOUT_MS = 30_000;

export interface BuyerSupportedKind {
  readonly scheme: string;
  readonly network: string;
}

export interface BuyerRetryPolicy {
  readonly maximumQuoteAttempts: number;
  readonly maximumCommitAttempts: number;
  readonly maximumJourneyPolls: number;
  readonly baseDelayMs: number;
  readonly wait: (delayMs: number) => Promise<void>;
}

export interface BuyerMiddlewareConfig {
  readonly client: ProductionClient;
  readonly source: string;
  readonly supported: readonly BuyerSupportedKind[];
  readonly authorizedBatches: AuthorizedBatchResolver;
  readonly retry?: Partial<BuyerRetryPolicy>;
  readonly now?: () => number;
  readonly fetch?: typeof globalThis.fetch;
}

export interface ParsedOffer {
  readonly required: PaymentRequired;
  readonly accepted: PaymentRequirements;
}

export interface MoveQuoteRequest {
  readonly source: string;
  readonly destination: string;
  readonly money: { readonly amount: string; readonly currency: string };
}

export interface MoveQuote {
  readonly quote_id: string;
  readonly description_copy_key: string;
  readonly mechanism: "fund" | "allocate" | "return" | "transfer";
  readonly money: { readonly amount: string; readonly currency: string };
  readonly fee_estimate: { readonly amount: string; readonly currency: string };
  readonly fee_ceiling: { readonly amount: string; readonly currency: string };
  readonly arrival_estimate: string;
  readonly expires_at: string;
  readonly irreversibility_copy_key?: string;
}

export type JourneyState =
  | "getting-ready"
  | "sending"
  | "processing"
  | "done"
  | "done-finalised"
  | "still-checking"
  | "refused"
  | "waiting-for-you";

export interface EvidenceReference {
  readonly evidence_id: string;
  readonly class: string;
  readonly verification: string;
}

export interface Journey {
  readonly journey_id: string;
  readonly kind: string;
  readonly state: JourneyState;
  readonly state_copy_key: string;
  readonly evidence: readonly EvidenceReference[];
  readonly stages: readonly {
    readonly stage_id: string;
    readonly copy_key: string;
    readonly state: JourneyState;
    readonly evidence: readonly EvidenceReference[];
  }[];
  readonly started_at: string;
  readonly updated_at: string;
  readonly refusal?: {
    readonly refused_by: string;
    readonly copy_key: string;
    readonly money_left: boolean;
  };
}

export interface EvidenceMaterial {
  readonly evidence_id: string;
  readonly class: string;
  readonly verification: string;
  readonly content_type: string;
  readonly bytes_base64: string;
}

export interface PreparedPayment {
  readonly offer: ParsedOffer;
  readonly quote: MoveQuote;
  readonly journey: Journey;
  readonly payload: PaymentPayload;
  readonly paymentHeader: string;
  readonly idempotencyKey: string;
  readonly canonicalReceipt: Uint8Array;
  readonly authorizedBatch: AuthorizedReceiptBatch;
  readonly verification: ReceiptVerification;
  readonly receiptDigest: string;
}

export type PaymentPreparation =
  | { readonly kind: "ready"; readonly payment: PreparedPayment }
  | { readonly kind: "pending"; readonly journey: Journey }
  | { readonly kind: "unknown"; readonly idempotencyKey: string; readonly journey?: Journey }
  | { readonly kind: "refused"; readonly journey: Journey };

export interface CapturedSettlement {
  readonly response: SettlementResponse;
  readonly verification: ReceiptVerification;
  readonly canonicalReceipt: Uint8Array;
}

export type PaidFetchResult =
  | { readonly kind: "not-payment-required"; readonly response: Response }
  | { readonly kind: "pending"; readonly journey: Journey }
  | { readonly kind: "unknown"; readonly idempotencyKey: string; readonly journey?: Journey }
  | { readonly kind: "refused"; readonly journey: Journey }
  | {
    readonly kind: "paid";
    readonly response: Response;
    readonly payment: PreparedPayment;
    readonly settlement: CapturedSettlement;
  };

export class BuyerMiddleware {
  readonly #client: ProductionClient;
  readonly #source: string;
  readonly #supported: readonly BuyerSupportedKind[];
  readonly #authorizedBatches: AuthorizedBatchResolver;
  readonly #retry: BuyerRetryPolicy;
  readonly #now: () => number;
  readonly #fetch: typeof globalThis.fetch;

  public constructor(config: BuyerMiddlewareConfig) {
    if (config.source.length === 0 || config.source.length > 512 || config.supported.length === 0) {
      throw new MiddlewareError("unsupported-payment");
    }
    const seen = new Set<string>();
    for (const kind of config.supported) {
      const key = `${kind.scheme}\0${kind.network}`;
      if (kind.scheme.length === 0 || kind.network.length === 0 || seen.has(key)) {
        throw new MiddlewareError("unsupported-payment");
      }
      seen.add(key);
    }
    this.#client = config.client;
    this.#source = config.source;
    this.#supported = config.supported;
    this.#authorizedBatches = config.authorizedBatches;
    this.#retry = retryPolicy(config.retry);
    this.#now = config.now ?? Date.now;
    this.#fetch = config.fetch ?? globalThis.fetch;
  }

  public parseOffer(paymentRequiredHeader: string): ParsedOffer {
    const required = decodePaymentRequiredHeader(paymentRequiredHeader);
    const accepted = required.accepts.find((candidate) => this.#supported.some(
      (kind) => kind.scheme === candidate.scheme && kind.network === candidate.network,
    ));
    if (accepted === undefined) {
      throw new MiddlewareError("unsupported-payment");
    }
    return { required, accepted };
  }

  public async prepare(paymentRequiredHeader: string, callerIdempotencyKey: string): Promise<PaymentPreparation> {
    const offer = this.parseOffer(paymentRequiredHeader);
    const mutationKey = idempotencyKey(callerIdempotencyKey);
    const quote = await this.#retrySdk(
      () => this.#client.human<MoveQuoteRequest, unknown>("move.quote", {
        source: this.#source,
        destination: offer.accepted.payTo,
        money: { amount: offer.accepted.amount, currency: offer.accepted.asset },
      }),
      this.#retry.maximumQuoteAttempts,
      false,
    );
    const parsedQuote = parseMoveQuote(quote);
    if (
      parsedQuote.money.amount !== offer.accepted.amount
      || !constantTimeEqualText(parsedQuote.money.currency, offer.accepted.asset)
      || Date.parse(parsedQuote.expires_at) <= this.#now()
    ) {
      throw new MiddlewareError("requirements-mismatch");
    }
    let rawJourney: unknown;
    try {
      rawJourney = await this.#retrySdk(
        () => this.#client.human("move.commit", { quote_id: parsedQuote.quote_id }, { idempotencyKey: mutationKey }),
        this.#retry.maximumCommitAttempts,
        true,
      );
    } catch (error) {
      if (error instanceof PlatformSdkError && error.retry === "unknown-outcome") {
        return { kind: "unknown", idempotencyKey: callerIdempotencyKey };
      }
      throw error;
    }
    let journey = parseJourney(rawJourney);
    for (let poll = 0; poll < this.#retry.maximumJourneyPolls && isProgress(journey.state); poll += 1) {
      await this.#retry.wait(this.#retry.baseDelayMs * Math.min(poll + 1, 10));
      journey = parseJourney(await this.#retrySdk(
        () => this.#client.human("journey.get", { journey_id: journey.journey_id }),
        this.#retry.maximumQuoteAttempts,
        false,
      ));
    }
    if (journey.state === "still-checking") {
      return { kind: "unknown", idempotencyKey: callerIdempotencyKey, journey };
    }
    if (journey.state === "refused") {
      return { kind: "refused", journey };
    }
    if (journey.state !== "done" && journey.state !== "done-finalised") {
      return { kind: "pending", journey };
    }
    const receipt = await this.#findMatchingReceipt(journey, offer.accepted);
    const payload: PaymentPayload = {
      x402Version: X402_VERSION,
      resource: offer.required.resource,
      payload: {
        receipt: encodeBase64(receipt.canonicalReceipt),
        receiptDigest: receipt.receiptDigest,
        verificationLevel: "sequencer-signed",
        idempotencyKey: callerIdempotencyKey,
      },
      accepted: offer.accepted,
      extensions: offer.required.extensions ?? {},
    };
    return {
      kind: "ready",
      payment: {
        offer,
        quote: parsedQuote,
        journey,
        payload,
        paymentHeader: encodePaymentPayloadHeader(payload),
        idempotencyKey: callerIdempotencyKey,
        canonicalReceipt: receipt.canonicalReceipt,
        authorizedBatch: receipt.authorizedBatch,
        verification: receipt.verification,
        receiptDigest: receipt.receiptDigest,
      },
    };
  }

  public async captureSettlement(
    paymentResponseHeader: string,
    payment: PreparedPayment,
  ): Promise<CapturedSettlement> {
    const response = decodeSettlementHeader(paymentResponseHeader);
    if (!response.success) {
      throw new MiddlewareError(response.errorReason === "settlement_pending" ? "payment-pending" : "payment-refused");
    }
    if (
      response.network !== payment.offer.accepted.network
      || response.amount !== payment.offer.accepted.amount
      || response.transaction !== `lxp:${payment.receiptDigest}`
    ) {
      throw new MiddlewareError("verification-failure");
    }
    const layerx = asObject(response.extensions?.["layerx"]);
    const evidence = parseReceiptEvidence(layerx);
    if (!constantTimeEqualText(evidence.receiptDigest, payment.receiptDigest)) {
      throw new MiddlewareError("verification-failure");
    }
    const canonicalReceipt = decodeBase64(evidence.receipt);
    if (!equalBytes(canonicalReceipt, payment.canonicalReceipt)) {
      throw new MiddlewareError("verification-failure");
    }
    const authorizedBatch = await this.#authorizedBatches.resolve(canonicalReceipt);
    const verification = await verifyPaymentReceipt(
      { canonicalReceipt, authorizedBatch },
      payment.offer.accepted,
    );
    return { response, verification, canonicalReceipt };
  }

  public async fetch(
    input: RequestInfo | URL,
    init: RequestInit,
    callerIdempotencyKey: string,
  ): Promise<PaidFetchResult> {
    const initial = await this.#fetch(input, init);
    if (initial.status !== 402) {
      return { kind: "not-payment-required", response: initial };
    }
    const required = initial.headers.get(PAYMENT_REQUIRED_HEADER);
    if (required === null) {
      throw new MiddlewareError("invalid-payment-required");
    }
    await initial.body?.cancel();
    const preparation = await this.prepare(required, callerIdempotencyKey);
    if (preparation.kind !== "ready") {
      return preparation;
    }
    const headers = new Headers(init.headers);
    headers.set(PAYMENT_SIGNATURE_HEADER, preparation.payment.paymentHeader);
    const paid = await this.#fetch(input, { ...init, headers });
    const responseHeader = paid.headers.get(PAYMENT_RESPONSE_HEADER);
    if (responseHeader === null) {
      await paid.body?.cancel();
      throw new MiddlewareError("verification-failure");
    }
    const settlement = await this.captureSettlement(responseHeader, preparation.payment);
    return { kind: "paid", response: paid, payment: preparation.payment, settlement };
  }

  async #findMatchingReceipt(
    journey: Journey,
    requirements: PaymentRequirements,
  ): Promise<{
    readonly canonicalReceipt: Uint8Array;
    readonly authorizedBatch: AuthorizedReceiptBatch;
    readonly verification: ReceiptVerification;
    readonly receiptDigest: string;
  }> {
    const references = uniqueEvidence(journey).filter((reference) =>
      reference.class === "layerx-receipt"
      && (reference.verification === "receipt-verified" || reference.verification === "checkpoint-finalised")
    );
    let matched: {
      readonly canonicalReceipt: Uint8Array;
      readonly authorizedBatch: AuthorizedReceiptBatch;
      readonly verification: ReceiptVerification;
      readonly receiptDigest: string;
    } | undefined;
    for (const reference of references) {
      const material = parseEvidenceMaterial(await this.#retrySdk(
        () => this.#client.human("evidence.get", { evidence_id: reference.evidence_id }),
        this.#retry.maximumQuoteAttempts,
        false,
      ));
      if (
        material.evidence_id !== reference.evidence_id
        || material.class !== "layerx-receipt"
        || material.content_type !== "application/layerx-receipt"
        || (material.verification !== "receipt-verified" && material.verification !== "checkpoint-finalised")
      ) {
        throw new MiddlewareError("verification-failure");
      }
      const canonicalReceipt = decodeBase64(material.bytes_base64);
      const authorizedBatch = await this.#authorizedBatches.resolve(canonicalReceipt);
      let candidate: {
        readonly canonicalReceipt: Uint8Array;
        readonly authorizedBatch: AuthorizedReceiptBatch;
        readonly verification: ReceiptVerification;
        readonly receiptDigest: string;
      };
      try {
        const verification = await verifyPaymentReceipt({ canonicalReceipt, authorizedBatch }, requirements);
        const receiptDigest = toHex(await merkleLeafDigest(canonicalReceipt));
        candidate = { canonicalReceipt, authorizedBatch, verification, receiptDigest };
      } catch (error) {
        if (!(error instanceof MiddlewareError) || error.code !== "verification-failure") {
          throw error;
        }
        continue;
      }
      if (matched !== undefined) {
        throw new MiddlewareError("verification-failure");
      }
      matched = candidate;
    }
    if (matched === undefined) {
      throw new MiddlewareError("verification-failure");
    }
    return matched;
  }

  async #retrySdk<T>(operation: () => Promise<T>, maximumAttempts: number, mutation: boolean): Promise<T> {
    for (let attempt = 1; ; attempt += 1) {
      try {
        return await operation();
      } catch (error) {
        if (!(error instanceof PlatformSdkError) || attempt >= maximumAttempts) {
          throw error;
        }
        if (error.retry === "never" || error.retry === "unknown-outcome") {
          throw error;
        }
        if (mutation && error.retry !== "safe" && error.retry !== "after") {
          throw error;
        }
        const delay = error.retryAfterMs ?? this.#retry.baseDelayMs * attempt;
        await this.#retry.wait(delay);
      }
    }
  }
}

export interface LayerXHttpTransportConfig {
  readonly baseUrl: string;
  readonly bearerToken: SecretBytes;
  readonly fetch?: typeof globalThis.fetch;
}

export class LayerXPaymentHttpTransport implements ProductionTransport {
  readonly #baseUrl: URL;
  readonly #token: SecretBytes;
  readonly #fetch: typeof globalThis.fetch;

  public constructor(config: LayerXHttpTransportConfig) {
    this.#baseUrl = new URL(config.baseUrl.endsWith("/") ? config.baseUrl : `${config.baseUrl}/`);
    const loopback = this.#baseUrl.hostname === "127.0.0.1"
      || this.#baseUrl.hostname === "localhost"
      || this.#baseUrl.hostname === "[::1]";
    if (this.#baseUrl.username.length > 0 || this.#baseUrl.password.length > 0
      || this.#baseUrl.hash.length > 0
      || (this.#baseUrl.protocol !== "https:" && !(this.#baseUrl.protocol === "http:" && loopback))) {
      throw new MiddlewareError("unsupported-payment");
    }
    this.#token = config.bearerToken;
    this.#fetch = config.fetch ?? globalThis.fetch;
  }

  public async call<TRequest, TResponse>(call: TransportCall<TRequest>): Promise<TResponse> {
    if (call.plane !== "human") {
      throw new PlatformSdkError({ code: "unavailable-capability", retry: "never" });
    }
    const route = paymentRoute(call.operation, call.request);
    const headers = new Headers({ accept: "application/json" });
    this.#token.withBytes((bytes) => {
      headers.set("authorization", `Bearer ${new TextDecoder("utf-8", { fatal: true }).decode(bytes)}`);
    });
    if (call.idempotencyKey !== undefined) {
      headers.set("idempotency-key", call.idempotencyKey);
    }
    if (route.body !== undefined) {
      headers.set("content-type", "application/json");
    }
    let response: Response;
    try {
      response = await this.#fetch(new URL(route.path, this.#baseUrl), {
        method: route.method,
        headers,
        ...(route.body === undefined ? {} : { body: JSON.stringify(route.body) }),
        signal: AbortSignal.timeout(HTTP_REQUEST_TIMEOUT_MS),
      });
    } catch {
      throw new PlatformSdkError({ code: "transport-failure", retry: "safe" });
    }
    const body = await readBoundedJson(response);
    if (!response.ok) {
      throw mapHttpError(response);
    }
    const envelope = asObject(body);
    if (envelope["ok"] !== true || envelope["result"] === undefined) {
      throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
    }
    return envelope["result"] as TResponse;
  }
}

async function readBoundedJson(response: Response): Promise<unknown> {
  if (response.body === null) return undefined;
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (let step = await reader.read(); step.done !== true; step = await reader.read()) {
    total += step.value.length;
    if (total > MAXIMUM_HTTP_RESPONSE_BYTES) {
      await reader.cancel();
      throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
    }
    chunks.push(step.value);
  }
  const encoded = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    encoded.set(chunk, offset);
    offset += chunk.length;
  }
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded));
  } catch {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
}

export function platform_mw_buyer(): "sdk-quoted-receipt-verified-x402-buyer" {
  return "sdk-quoted-receipt-verified-x402-buyer";
}

function retryPolicy(value: Partial<BuyerRetryPolicy> | undefined): BuyerRetryPolicy {
  const policy: BuyerRetryPolicy = {
    maximumQuoteAttempts: value?.maximumQuoteAttempts ?? 3,
    maximumCommitAttempts: value?.maximumCommitAttempts ?? 3,
    maximumJourneyPolls: value?.maximumJourneyPolls ?? 20,
    baseDelayMs: value?.baseDelayMs ?? 250,
    wait: value?.wait ?? ((delayMs) => new Promise((resolve) => setTimeout(resolve, delayMs))),
  };
  if (
    !Number.isSafeInteger(policy.maximumQuoteAttempts) || policy.maximumQuoteAttempts <= 0
    || !Number.isSafeInteger(policy.maximumCommitAttempts) || policy.maximumCommitAttempts <= 0
    || !Number.isSafeInteger(policy.maximumJourneyPolls) || policy.maximumJourneyPolls < 0
    || !Number.isSafeInteger(policy.baseDelayMs) || policy.baseDelayMs < 0
  ) {
    throw new MiddlewareError("unsupported-payment");
  }
  return policy;
}

function parseMoveQuote(value: unknown): MoveQuote {
  const object = asObject(value);
  const mechanism = object["mechanism"];
  if (mechanism !== "fund" && mechanism !== "allocate" && mechanism !== "return" && mechanism !== "transfer") {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return {
    quote_id: requiredString(object["quote_id"]),
    description_copy_key: requiredString(object["description_copy_key"]),
    mechanism,
    money: parseMoney(object["money"]),
    fee_estimate: parseMoney(object["fee_estimate"]),
    fee_ceiling: parseMoney(object["fee_ceiling"]),
    arrival_estimate: requiredTimestamp(object["arrival_estimate"]),
    expires_at: requiredTimestamp(object["expires_at"]),
    ...(object["irreversibility_copy_key"] === undefined
      ? {}
      : { irreversibility_copy_key: requiredString(object["irreversibility_copy_key"]) }),
  };
}

function parseJourney(value: unknown): Journey {
  const object = asObject(value);
  const state = parseJourneyState(object["state"]);
  const evidence = parseEvidenceReferences(object["evidence"]);
  if (!Array.isArray(object["stages"])) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  const stages = object["stages"].map((value) => {
    const stage = asObject(value);
    return {
      stage_id: requiredString(stage["stage_id"]),
      copy_key: requiredString(stage["copy_key"]),
      state: parseJourneyState(stage["state"]),
      evidence: parseEvidenceReferences(stage["evidence"]),
    };
  });
  const refusalValue = object["refusal"];
  const refusal = refusalValue === undefined ? undefined : asObject(refusalValue);
  if (state === "refused" && refusal === undefined) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return {
    journey_id: requiredString(object["journey_id"]),
    kind: requiredString(object["kind"]),
    state,
    state_copy_key: requiredString(object["state_copy_key"]),
    evidence,
    stages,
    started_at: requiredTimestamp(object["started_at"]),
    updated_at: requiredTimestamp(object["updated_at"]),
    ...(refusal === undefined ? {} : {
      refusal: {
        refused_by: requiredString(refusal["refused_by"]),
        copy_key: requiredString(refusal["copy_key"]),
        money_left: requiredBoolean(refusal["money_left"]),
      },
    }),
  };
}

function parseEvidenceMaterial(value: unknown): EvidenceMaterial {
  const object = asObject(value);
  return {
    evidence_id: requiredString(object["evidence_id"]),
    class: requiredString(object["class"]),
    verification: requiredString(object["verification"]),
    content_type: requiredString(object["content_type"]),
    bytes_base64: requiredString(object["bytes_base64"]),
  };
}

function parseEvidenceReferences(value: unknown): readonly EvidenceReference[] {
  if (!Array.isArray(value)) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return value.map((item) => {
    const object = asObject(item);
    return {
      evidence_id: requiredString(object["evidence_id"]),
      class: requiredString(object["class"]),
      verification: requiredString(object["verification"]),
    };
  });
}

function uniqueEvidence(journey: Journey): readonly EvidenceReference[] {
  const unique = new Map<string, EvidenceReference>();
  for (const reference of [...journey.evidence, ...journey.stages.flatMap((stage) => stage.evidence)]) {
    const previous = unique.get(reference.evidence_id);
    if (previous !== undefined && (
      previous.class !== reference.class || previous.verification !== reference.verification
    )) {
      throw new MiddlewareError("verification-failure");
    }
    unique.set(reference.evidence_id, reference);
  }
  return [...unique.values()];
}

function parseMoney(value: unknown): { readonly amount: string; readonly currency: string } {
  const object = asObject(value);
  return { amount: requiredAmount(object["amount"]), currency: requiredString(object["currency"]) };
}

function parseJourneyState(value: unknown): JourneyState {
  if (
    value !== "getting-ready" && value !== "sending" && value !== "processing"
    && value !== "done" && value !== "done-finalised" && value !== "still-checking"
    && value !== "refused" && value !== "waiting-for-you"
  ) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return value;
}

function isProgress(state: JourneyState): boolean {
  return state === "getting-ready" || state === "sending" || state === "processing";
}

function parseReceiptEvidence(value: Readonly<Record<string, JsonValue>>): LayerXReceiptEvidence {
  if (
    typeof value["receipt"] !== "string"
    || typeof value["receiptDigest"] !== "string"
    || value["verificationLevel"] !== "sequencer-signed"
  ) {
    throw new MiddlewareError("verification-failure");
  }
  return {
    receipt: value["receipt"],
    receiptDigest: value["receiptDigest"],
    verificationLevel: "sequencer-signed",
  };
}

function paymentRoute(operation: string, request: unknown): { readonly method: string; readonly path: string; readonly body?: unknown } {
  switch (operation) {
    case "move.quote":
      return { method: "POST", path: "v1/moves/quote", body: request };
    case "move.commit":
      return { method: "POST", path: "v1/moves", body: request };
    case "journey.get":
      return { method: "GET", path: `v1/journeys/${encodeURIComponent(requiredString(asObject(request)["journey_id"]))}` };
    case "evidence.get":
      return { method: "GET", path: `v1/evidence/${encodeURIComponent(requiredString(asObject(request)["evidence_id"]))}` };
    default:
      throw new PlatformSdkError({ code: "unavailable-capability", retry: "never" });
  }
}

function mapHttpError(response: Response): PlatformSdkError {
  if (response.status === 429) {
    const seconds = response.headers.get("retry-after");
    const retryAfterMs = seconds !== null && /^[0-9]+$/u.test(seconds) ? Number(seconds) * 1000 : undefined;
    return new PlatformSdkError({
      code: "rate-limit",
      retry: "after",
      ...(retryAfterMs === undefined || !Number.isSafeInteger(retryAfterMs) ? {} : { retryAfterMs }),
    });
  }
  if (response.status === 409) {
    return new PlatformSdkError({ code: "idempotency-conflict", retry: "never" });
  }
  if (response.status === 400 || response.status === 422) {
    return new PlatformSdkError({ code: "invalid-argument", retry: "never" });
  }
  if (response.status === 401 || response.status === 403) {
    return new PlatformSdkError({ code: "capability-refusal", retry: "never" });
  }
  if (response.status >= 500) {
    return new PlatformSdkError({ code: "transport-failure", retry: "safe" });
  }
  return new PlatformSdkError({ code: "internal-fault", retry: "never" });
}

function asObject(value: unknown): Readonly<Record<string, JsonValue>> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return value as Readonly<Record<string, JsonValue>>;
}

function requiredString(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.includes("\0")) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return value;
}

function requiredBoolean(value: unknown): boolean {
  if (typeof value !== "boolean") {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return value;
}

function requiredAmount(value: unknown): string {
  const text = requiredString(value);
  if (!/^(0|[1-9][0-9]*)$/u.test(text) || BigInt(text) > MAX_U128) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return text;
}

function requiredTimestamp(value: unknown): string {
  const text = requiredString(value);
  if (!Number.isFinite(Date.parse(text))) {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  }
  return text;
}

function encodeBase64(value: Uint8Array): string {
  let binary = "";
  for (let offset = 0; offset < value.length; offset += 0x8000) {
    binary += String.fromCharCode(...value.subarray(offset, Math.min(offset + 0x8000, value.length)));
  }
  return btoa(binary);
}

function decodeBase64(value: string): Uint8Array {
  if (value.length === 0 || !/^[A-Za-z0-9+/]*={0,2}$/u.test(value)) {
    throw new MiddlewareError("verification-failure");
  }
  try {
    return Uint8Array.from(atob(value), (character) => character.charCodeAt(0));
  } catch {
    throw new MiddlewareError("verification-failure");
  }
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

function toHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
