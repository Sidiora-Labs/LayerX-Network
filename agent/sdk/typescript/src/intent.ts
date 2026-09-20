import * as http from "node:http";
import * as https from "node:https";

import { LayerXKeyCredential } from "./agent-http.js";
import {
  PlatformSdkError,
  protocolAmount,
  type IdempotencyKey,
  type ProtocolAmount,
  type RetryClass,
  type SdkErrorCode,
} from "./production.js";

const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const MAX_REQUEST_BYTES = 1024 * 1024;
const MAX_LEGS = 16;
const DEFAULT_TIMEOUT_MS = 30_000;
const HEX32 = /^[0-9a-f]{64}$/u;
const HEX = /^(?:[0-9a-f]{2})+$/u;
const ACCOUNT_NAME = /^[a-z0-9][a-z0-9:._-]{0,254}$/u;
const ACTOR = /^did:[a-z0-9]{1,32}:[a-z0-9:._-]{1,214}$/u;
const AUTHORITY = /^[a-z][a-z0-9-]{0,63}$/u;
const CURRENCY = /^[A-Z][A-Z0-9]{1,15}$/u;
const DEADLINE = /^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]{1,9})?Z$/u;
const COPY_KEY = /^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)*$/u;
const TRACE_ID = /^trc_[0-9a-z]{1,64}$/u;
const JOURNEY_ID = /^jrn_[0-9a-z]{1,64}$/u;
const LOWER_TOKEN = /^[a-z][a-z0-9-]{0,63}$/u;
const HEADER_VALUE = /^[\x21-\x7e]{1,255}$/u;
const MAX_U64 = 18446744073709551615n;

export const INTENT_ENDPOINT_KINDS = ["paxeer-wallet", "human", "agent", "agent-budget"] as const;
export type IntentEndpointKind = (typeof INTENT_ENDPOINT_KINDS)[number];

export const INTENT_DOMAINS = ["paxeer", "layerx"] as const;
export type IntentDomain = (typeof INTENT_DOMAINS)[number];

export const HUMAN_ERROR_CODES = [
  "unauthenticated",
  "session-expired",
  "step-up-required",
  "forbidden",
  "not-found",
  "invalid-request",
  "conflict",
  "rate-limited",
  "cursor-expired",
  "unavailable",
  "upstream-degraded",
  "challenge-expired",
  "refused-by-policy",
  "refused-by-budget",
  "refused-by-capability",
  "refused-by-protocol",
  "refused-by-limit",
  "quote-expired",
  "wallet-not-bound",
  "exit-unavailable",
  "already-decided",
  "hold-expired",
  "hold-defective",
  "archive-needs-disposition",
  "confirmation-mismatch",
  "not-suppressible",
  "support-unavailable",
  "support-conversation-unknown",
  "support-message-unknown",
] as const;
export type HumanErrorCode = (typeof HUMAN_ERROR_CODES)[number];

export const HUMAN_RETRIABILITIES = ["retriable", "retriable-after", "structural", "final"] as const;
export type HumanRetriability = (typeof HUMAN_RETRIABILITIES)[number];

const HUMAN_ERROR_CLASS: Readonly<Record<HumanErrorCode, SdkErrorCode>> = Object.freeze({
  unauthenticated: "capability-refusal",
  "session-expired": "capability-refusal",
  "step-up-required": "capability-refusal",
  forbidden: "capability-refusal",
  "not-found": "core-rejection",
  "invalid-request": "invalid-argument",
  conflict: "idempotency-conflict",
  "rate-limited": "rate-limit",
  "cursor-expired": "invalid-argument",
  unavailable: "unavailable-capability",
  "upstream-degraded": "unavailable-capability",
  "challenge-expired": "deadline",
  "refused-by-policy": "policy-refusal",
  "refused-by-budget": "budget-refusal",
  "refused-by-capability": "capability-refusal",
  "refused-by-protocol": "core-rejection",
  "refused-by-limit": "policy-refusal",
  "quote-expired": "deadline",
  "wallet-not-bound": "core-rejection",
  "exit-unavailable": "unavailable-capability",
  "already-decided": "core-rejection",
  "hold-expired": "deadline",
  "hold-defective": "core-rejection",
  "archive-needs-disposition": "core-rejection",
  "confirmation-mismatch": "core-rejection",
  "not-suppressible": "policy-refusal",
  "support-unavailable": "unavailable-capability",
  "support-conversation-unknown": "core-rejection",
  "support-message-unknown": "core-rejection",
});

const HUMAN_RETRY: Readonly<Record<HumanRetriability, RetryClass>> = Object.freeze({
  retriable: "safe",
  "retriable-after": "after",
  structural: "never",
  final: "never",
});

interface IntentRoute {
  readonly method: "POST";
  readonly path: string;
  readonly idempotent: boolean;
}

const INTENT_ROUTES = Object.freeze({
  "intent.plan": Object.freeze({ method: "POST", path: "/v1/intents/plan", idempotent: false }),
  "intent.submit": Object.freeze({ method: "POST", path: "/v1/intents/submit", idempotent: true }),
} as const satisfies Readonly<Record<string, IntentRoute>>);

export type IntentOperation = keyof typeof INTENT_ROUTES;

export interface IntentMoney {
  readonly amount: ProtocolAmount;
  readonly currency: string;
}

export interface IntentEndpointRef {
  readonly kind: IntentEndpointKind;
  readonly account: string | null;
}

export interface IntentConstraints {
  readonly deadline: string;
  readonly maxFee: IntentMoney;
  readonly allowTopUp: boolean;
}

export interface PlanIntentRequest {
  readonly source: IntentEndpointRef;
  readonly destination: IntentEndpointRef;
  readonly assetId: string;
  readonly money: IntentMoney;
  readonly constraints: IntentConstraints;
}

export interface IntentLeg {
  readonly index: number;
  readonly mechanism: string;
  readonly domain: IntentDomain;
  readonly source: IntentEndpointRef;
  readonly destination: IntentEndpointRef;
  readonly money: IntentMoney;
  readonly fee: IntentMoney;
}

export interface IntentSigningRequirement {
  readonly legIndex: number;
  readonly actionKey: string;
  readonly signingContext: string;
  readonly authority: string;
}

export interface IntentPlan {
  readonly planDigest: string;
  readonly journeyKind: string;
  readonly totalFee: IntentMoney;
  readonly legs: readonly IntentLeg[];
  readonly signingRequirements: readonly IntentSigningRequirement[];
}

export interface IntentLegBinding {
  readonly legIndex: number;
  readonly actionKey: string;
  readonly actor: string;
  readonly authority: string;
  readonly relationship: string;
  readonly accountSequence: bigint;
  readonly notBefore: bigint;
  readonly notAfter: bigint;
  readonly feeLimit: IntentMoney;
}

export interface SubmitPlanRequest {
  readonly planDigest: string;
  readonly signedDigest: string;
  readonly bindings: readonly IntentLegBinding[];
}

export interface IntentSubmission {
  readonly journeyId: string;
  readonly planDigest: string;
  readonly state: string;
  readonly stateCopyKey: string;
}

export interface HumanIntentErrorDetails {
  readonly status: number;
  readonly humanCode: HumanErrorCode;
  readonly copyKey: string;
  readonly retriability: HumanRetriability;
  readonly trace: string;
  readonly field: string | null;
  readonly retryAfterMs: number | null;
}

/** A typed refusal carried by the human-plane response envelope. */
export class HumanIntentError extends PlatformSdkError {
  public readonly status: number;
  public readonly humanCode: HumanErrorCode;
  public readonly copyKey: string;
  public readonly retriability: HumanRetriability;
  public readonly trace: string;
  public readonly field: string | null;

  public constructor(details: HumanIntentErrorDetails) {
    super({
      code: HUMAN_ERROR_CLASS[details.humanCode],
      retry: HUMAN_RETRY[details.retriability],
      ...(details.retryAfterMs === null ? {} : { retryAfterMs: details.retryAfterMs }),
    });
    this.status = details.status;
    this.humanCode = details.humanCode;
    this.copyKey = details.copyKey;
    this.retriability = details.retriability;
    this.trace = details.trace;
    this.field = details.field;
  }
}

export interface HumanIntentClientOptions {
  readonly endpoint: URL | string;
  readonly credential?: LayerXKeyCredential;
  readonly timeoutMs?: number;
  readonly maximumResponseBytes?: number;
}

/** Exact HTTP client for the two human-plane intent operations. */
export class HumanIntentClient {
  readonly #endpoint: URL;
  readonly #credential: LayerXKeyCredential | undefined;
  readonly #timeoutMs: number;
  readonly #maximumResponseBytes: number;

  public constructor(options: HumanIntentClientOptions) {
    this.#endpoint = validateEndpoint(options.endpoint);
    this.#credential = options.credential;
    this.#timeoutMs = exactPositive(options.timeoutMs ?? DEFAULT_TIMEOUT_MS);
    this.#maximumResponseBytes = exactPositive(options.maximumResponseBytes ?? MAX_RESPONSE_BYTES);
    if (this.#maximumResponseBytes > MAX_RESPONSE_BYTES) throw invalidArgument();
  }

  public async planIntent(request: PlanIntentRequest): Promise<IntentPlan> {
    return decodeIntentPlan(await this.dispatch("intent.plan", encodePlanIntentRequest(request), undefined));
  }

  public async submitPlan(request: SubmitPlanRequest, key: IdempotencyKey): Promise<IntentSubmission> {
    return decodeIntentSubmission(await this.dispatch("intent.submit", encodeSubmitPlanRequest(request), key));
  }

  private async dispatch(
    operation: IntentOperation,
    document: Readonly<Record<string, unknown>>,
    key: IdempotencyKey | undefined,
  ): Promise<unknown> {
    const route = INTENT_ROUTES[operation];
    if (route.idempotent === (key === undefined)) throw invalidArgument();
    if (key !== undefined && !HEADER_VALUE.test(key)) throw invalidArgument();
    let body: Buffer;
    try { body = Buffer.from(JSON.stringify(document), "utf8"); }
    catch { throw invalidArgument(); }
    if (body.length > MAX_REQUEST_BYTES) throw invalidArgument();
    const headers: http.OutgoingHttpHeaders = {
      Accept: "application/json",
      "Content-Type": "application/json",
      "Content-Length": body.length,
      "User-Agent": "layerx-typescript/0.1.0",
    };
    if (key !== undefined) headers["Idempotency-Key"] = key;
    if (this.#credential !== undefined) {
      this.#credential.use((authorization) => { headers.Authorization = authorization; });
    }
    return await this.send(routeEndpoint(this.#endpoint, route.path), headers, body, route.idempotent);
  }

  private send(
    endpoint: URL,
    headers: http.OutgoingHttpHeaders,
    body: Buffer,
    mutation: boolean,
  ): Promise<unknown> {
    return new Promise<unknown>((resolve, reject) => {
      const driver = endpoint.protocol === "https:" ? https : http;
      let settled = false;
      const finish = <T>(callback: (value: T) => void, value: T): void => {
        if (settled) return;
        settled = true;
        callback(value);
      };
      const request = driver.request(endpoint, { method: "POST", headers, timeout: this.#timeoutMs }, (response) => {
        const chunks: Buffer[] = [];
        let received = 0;
        response.on("data", (chunk: Buffer) => {
          received += chunk.length;
          if (received > this.#maximumResponseBytes) {
            response.destroy();
            finish(reject, decodeFailure());
            return;
          }
          chunks.push(Buffer.from(chunk));
        });
        response.on("end", () => {
          if (settled) return;
          try {
            if (response.headers["content-type"]?.split(";")[0]?.trim() !== "application/json") throw decodeFailure();
            finish(resolve, decodeHumanEnvelope(response.statusCode ?? 0, Buffer.concat(chunks)));
          } catch (error) {
            finish(reject, error);
          }
        });
        response.on("error", () => finish(reject, transportFailure(mutation)));
      });
      request.on("timeout", () => request.destroy());
      request.on("error", () => finish(reject, transportFailure(mutation)));
      request.end(body);
    });
  }
}

export function encodePlanIntentRequest(request: PlanIntentRequest): Readonly<Record<string, unknown>> {
  return {
    source: encodeEndpoint(request.source),
    destination: encodeEndpoint(request.destination),
    asset_id: hex32(request.assetId),
    money: encodeMoney(request.money),
    constraints: {
      deadline: deadline(request.constraints.deadline),
      max_fee: encodeMoney(request.constraints.maxFee),
      allow_top_up: boolean(request.constraints.allowTopUp),
    },
  };
}

export function encodeSubmitPlanRequest(request: SubmitPlanRequest): Readonly<Record<string, unknown>> {
  if (request.bindings.length === 0 || request.bindings.length > MAX_LEGS) throw invalidArgument();
  const seen = new Set<number>();
  const bindings = request.bindings.map((binding) => {
    const legIndex = legPosition(binding.legIndex);
    if (seen.has(legIndex)) throw invalidArgument();
    seen.add(legIndex);
    return {
      leg_index: legIndex,
      action_key: hex32(binding.actionKey),
      actor: actor(binding.actor),
      authority: token(binding.authority, AUTHORITY),
      relationship: token(binding.relationship, AUTHORITY),
      account_sequence: unsigned64(binding.accountSequence),
      not_before: unsigned64(binding.notBefore),
      not_after: unsigned64(binding.notAfter),
      fee_limit: encodeMoney(binding.feeLimit),
    };
  });
  return {
    plan_digest: hex32(request.planDigest),
    signed_digest: hex32(request.signedDigest),
    bindings,
  };
}

export function decodeHumanEnvelope(status: number, encoded: Buffer): unknown {
  let envelope: Readonly<Record<string, unknown>>;
  try { envelope = record(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded)) as unknown); }
  catch { throw decodeFailure(); }
  const trace = envelope.trace;
  if (typeof trace !== "string" || !TRACE_ID.test(trace)) throw decodeFailure();
  if (envelope.ok === true) {
    exactKeys(envelope, ["ok", "result", "trace"]);
    if (status < 200 || status >= 300) throw decodeFailure();
    return envelope.result;
  }
  if (envelope.ok !== false) throw decodeFailure();
  exactKeys(envelope, ["ok", "error", "trace"]);
  if (status < 400 || status >= 600) throw decodeFailure();
  throw decodeHumanError(status, envelope.error, trace);
}

export function decodeHumanError(status: number, value: unknown, trace: string): HumanIntentError {
  const error = record(value);
  for (const key of Object.keys(error)) {
    if (!["code", "copy_key", "retry", "retry_after_ms", "field"].includes(key)) throw decodeFailure();
  }
  const code = error.code;
  const copyKey = error.copy_key;
  const retry = error.retry;
  if (typeof code !== "string" || !(HUMAN_ERROR_CODES as readonly string[]).includes(code)
    || typeof copyKey !== "string" || copyKey.length > 128 || !COPY_KEY.test(copyKey)
    || typeof retry !== "string" || !(HUMAN_RETRIABILITIES as readonly string[]).includes(retry)) {
    throw decodeFailure();
  }
  const retryAfter = "retry_after_ms" in error ? error.retry_after_ms : null;
  if (retryAfter !== null && (typeof retryAfter !== "number" || !Number.isSafeInteger(retryAfter) || retryAfter <= 0)) throw decodeFailure();
  const field = "field" in error ? error.field : null;
  if (field !== null && (typeof field !== "string" || field.length === 0 || field.length > 128)) throw decodeFailure();
  if ((retry === "retriable-after") !== (retryAfter !== null)) throw decodeFailure();
  return new HumanIntentError({
    status,
    humanCode: code as HumanErrorCode,
    copyKey,
    retriability: retry as HumanRetriability,
    trace,
    field,
    retryAfterMs: retryAfter,
  });
}

export function decodeIntentPlan(value: unknown): IntentPlan {
  const plan = record(value);
  exactKeys(plan, ["plan_digest", "journey_kind", "total_fee", "legs", "signing_requirements"]);
  if (!Array.isArray(plan.legs) || plan.legs.length === 0 || plan.legs.length > MAX_LEGS) throw decodeFailure();
  const legs = plan.legs.map((leg, position) => decodeIntentLeg(leg, position));
  if (!Array.isArray(plan.signing_requirements) || plan.signing_requirements.length > MAX_LEGS) throw decodeFailure();
  const seen = new Set<number>();
  const signingRequirements = plan.signing_requirements.map((requirement) => {
    const decoded = decodeSigningRequirement(requirement);
    if (decoded.legIndex >= legs.length || seen.has(decoded.legIndex)) throw decodeFailure();
    seen.add(decoded.legIndex);
    return decoded;
  });
  return Object.freeze({
    planDigest: decodedHex32(plan.plan_digest),
    journeyKind: decodedToken(plan.journey_kind),
    totalFee: decodeMoney(plan.total_fee),
    legs: Object.freeze(legs),
    signingRequirements: Object.freeze(signingRequirements),
  });
}

export function decodeIntentSubmission(value: unknown): IntentSubmission {
  const submission = record(value);
  exactKeys(submission, ["journey_id", "plan_digest", "state", "state_copy_key"]);
  const journeyId = submission.journey_id;
  const state = submission.state;
  const stateCopyKey = submission.state_copy_key;
  if (typeof journeyId !== "string" || !JOURNEY_ID.test(journeyId)
    || typeof state !== "string" || !LOWER_TOKEN.test(state)
    || typeof stateCopyKey !== "string" || stateCopyKey.length > 128 || !COPY_KEY.test(stateCopyKey)) {
    throw decodeFailure();
  }
  return Object.freeze({
    journeyId,
    planDigest: decodedHex32(submission.plan_digest),
    state,
    stateCopyKey,
  });
}

function decodeIntentLeg(value: unknown, position: number): IntentLeg {
  const leg = record(value);
  exactKeys(leg, ["index", "mechanism", "domain", "source", "destination", "money", "fee"]);
  if (leg.index !== position) throw decodeFailure();
  const domain = leg.domain;
  if (typeof domain !== "string" || !(INTENT_DOMAINS as readonly string[]).includes(domain)) throw decodeFailure();
  return Object.freeze({
    index: position,
    mechanism: decodedToken(leg.mechanism),
    domain: domain as IntentDomain,
    source: decodeEndpoint(leg.source),
    destination: decodeEndpoint(leg.destination),
    money: decodeMoney(leg.money),
    fee: decodeMoney(leg.fee),
  });
}

function decodeSigningRequirement(value: unknown): IntentSigningRequirement {
  const requirement = record(value);
  exactKeys(requirement, ["leg_index", "action_key", "signing_context", "authority"]);
  const context = requirement.signing_context;
  const authority = requirement.authority;
  if (typeof context !== "string" || context.length === 0 || context.length > 2048 || !HEX.test(context)
    || typeof authority !== "string" || !AUTHORITY.test(authority)) {
    throw decodeFailure();
  }
  const legIndex = requirement.leg_index;
  if (typeof legIndex !== "number" || !Number.isSafeInteger(legIndex) || legIndex < 0 || legIndex >= MAX_LEGS) throw decodeFailure();
  return Object.freeze({
    legIndex,
    actionKey: decodedHex32(requirement.action_key),
    signingContext: context,
    authority,
  });
}

function decodeEndpoint(value: unknown): IntentEndpointRef {
  const endpoint = record(value);
  const kind = endpoint.kind;
  if (typeof kind !== "string" || !(INTENT_ENDPOINT_KINDS as readonly string[]).includes(kind)) throw decodeFailure();
  if (kind === "paxeer-wallet") {
    exactKeys(endpoint, ["kind"]);
    return Object.freeze({ kind: kind as IntentEndpointKind, account: null });
  }
  exactKeys(endpoint, ["kind", "account"]);
  const account = endpoint.account;
  if (typeof account !== "string" || !ACCOUNT_NAME.test(account)) throw decodeFailure();
  return Object.freeze({ kind: kind as IntentEndpointKind, account });
}

function decodeMoney(value: unknown): IntentMoney {
  const money = record(value);
  exactKeys(money, ["amount", "currency"]);
  const amount = money.amount;
  const currency = money.currency;
  if (typeof amount !== "string" || !/^(0|[1-9][0-9]*)$/u.test(amount)
    || typeof currency !== "string" || !CURRENCY.test(currency)) {
    throw decodeFailure();
  }
  let parsed: ProtocolAmount;
  try { parsed = protocolAmount(amount); }
  catch { throw decodeFailure(); }
  return Object.freeze({ amount: parsed, currency });
}

function encodeEndpoint(endpoint: IntentEndpointRef): Readonly<Record<string, unknown>> {
  if (!(INTENT_ENDPOINT_KINDS as readonly string[]).includes(endpoint.kind)) throw invalidArgument();
  if (endpoint.kind === "paxeer-wallet") {
    if (endpoint.account !== null) throw invalidArgument();
    return { kind: endpoint.kind };
  }
  if (typeof endpoint.account !== "string" || !ACCOUNT_NAME.test(endpoint.account)) throw invalidArgument();
  return { kind: endpoint.kind, account: endpoint.account };
}

function encodeMoney(money: IntentMoney): Readonly<Record<string, unknown>> {
  if (typeof money.currency !== "string" || !CURRENCY.test(money.currency)) throw invalidArgument();
  let amount: ProtocolAmount;
  try { amount = protocolAmount(money.amount); }
  catch { throw invalidArgument(); }
  return { amount: amount.toString(), currency: money.currency };
}

function record(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw decodeFailure();
  return value as Readonly<Record<string, unknown>>;
}

function exactKeys(value: Readonly<Record<string, unknown>>, required: readonly string[]): void {
  if (Object.keys(value).length !== required.length || required.some((key) => !(key in value))) throw decodeFailure();
}

function decodedHex32(value: unknown): string {
  if (typeof value !== "string" || !HEX32.test(value)) throw decodeFailure();
  return value;
}

function decodedToken(value: unknown): string {
  if (typeof value !== "string" || !LOWER_TOKEN.test(value)) throw decodeFailure();
  return value;
}

function hex32(value: string): string {
  if (typeof value !== "string" || !HEX32.test(value)) throw invalidArgument();
  return value;
}

function token(value: string, pattern: RegExp): string {
  if (typeof value !== "string" || !pattern.test(value)) throw invalidArgument();
  return value;
}

function actor(value: string): string {
  if (typeof value !== "string" || !ACTOR.test(value)) throw invalidArgument();
  return value;
}

function deadline(value: string): string {
  if (typeof value !== "string" || !DEADLINE.test(value)) throw invalidArgument();
  return value;
}

function boolean(value: boolean): boolean {
  if (typeof value !== "boolean") throw invalidArgument();
  return value;
}

function legPosition(value: number): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0 || value >= MAX_LEGS) throw invalidArgument();
  return value;
}

function unsigned64(value: bigint): number {
  if (typeof value !== "bigint" || value < 0n || value > MAX_U64) throw invalidArgument();
  if (value > BigInt(Number.MAX_SAFE_INTEGER)) throw invalidArgument();
  return Number(value);
}

function validateEndpoint(value: URL | string): URL {
  let endpoint: URL;
  try { endpoint = new URL(value); } catch { throw invalidArgument(); }
  if ((endpoint.protocol !== "https:" && endpoint.protocol !== "http:")
    || endpoint.username !== "" || endpoint.password !== "" || endpoint.search !== "" || endpoint.hash !== "") {
    throw invalidArgument();
  }
  if (endpoint.protocol === "http:" && !isLoopback(endpoint.hostname)) throw invalidArgument();
  return endpoint;
}

function isLoopback(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return host === "localhost" || host === "::1" || host === "[::1]" || /^127(?:\.[0-9]{1,3}){3}$/u.test(host);
}

function routeEndpoint(base: URL, path: string): URL {
  const endpoint = new URL(base.toString());
  endpoint.pathname = `${endpoint.pathname.replace(/\/+$/u, "")}${path}`;
  return endpoint;
}

function exactPositive(value: number): number {
  if (!Number.isSafeInteger(value) || value <= 0) throw invalidArgument();
  return value;
}

function transportFailure(mutation: boolean): PlatformSdkError {
  return new PlatformSdkError({
    code: mutation ? "unknown-outcome" : "transport-failure",
    retry: mutation ? "unknown-outcome" : "safe",
  });
}

function invalidArgument(): PlatformSdkError {
  return new PlatformSdkError({ code: "invalid-argument", retry: "never" });
}

function decodeFailure(): PlatformSdkError {
  return new PlatformSdkError({ code: "decode-failure", retry: "never" });
}
