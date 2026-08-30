import * as http from "node:http";
import * as https from "node:https";

import type { Operation as AgentOperation } from "./generated/client.js";
import {
  PlatformSdkError,
  SecretBytes,
  type ProductionTransport,
  type SdkErrorCode,
  type TransportCall,
} from "./production.js";

const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const MAX_REQUEST_BYTES = 4 * 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 30_000;
const HEX32 = /^[0-9a-f]{64}$/u;
const KEY_ID = /^[A-Za-z0-9_-]{1,64}$/u;

interface AgentRoute {
  readonly method: "GET" | "POST";
  readonly path: string;
  readonly pathField?: "program_id" | "idempotency_key" | "activity_id";
}

const PROGRAM_ROUTES: Readonly<Partial<Record<AgentOperation, AgentRoute>>> = Object.freeze({
  "program.discover": Object.freeze({ method: "GET", path: "/v1/programs/registry/{program_id}", pathField: "program_id" }),
  "program.interface": Object.freeze({ method: "GET", path: "/v1/programs/registry/{program_id}/interface", pathField: "program_id" }),
  "program.simulate": Object.freeze({ method: "POST", path: "/v1/programs/simulate" }),
  "program.call": Object.freeze({ method: "POST", path: "/v1/programs/call" }),
  "program.receipt": Object.freeze({ method: "GET", path: "/v1/programs/receipts/by-idempotency/{idempotency_key}", pathField: "idempotency_key" }),
  "program.activity": Object.freeze({ method: "GET", path: "/v1/programs/activities/{activity_id}", pathField: "activity_id" }),
});

const ERROR_CLASS: Readonly<Record<string, SdkErrorCode>> = Object.freeze({
  TransportFailure: "transport-failure",
  Deadline: "deadline",
  ProtocolIncompatibility: "protocol-incompatibility",
  UnavailableCapability: "unavailable-capability",
  CoreRejection: "core-rejection",
  VerificationFailure: "verification-failure",
  PolicyRefusal: "policy-refusal",
  CapabilityRefusal: "capability-refusal",
  BudgetRefusal: "budget-refusal",
  RateLimit: "rate-limit",
  IdempotencyConflict: "idempotency-conflict",
  InternalFault: "internal-fault",
});

export class LayerXKeyCredential {
  public constructor(
    private readonly keyId: string,
    private readonly secret: SecretBytes,
  ) {
    if (!KEY_ID.test(keyId)) throw invalidArgument();
  }

  public use<T>(consumer: (authorization: string) => T): T {
    return this.secret.withBytes((bytes) => {
      let value: string;
      try { value = new TextDecoder("utf-8", { fatal: true }).decode(bytes); }
      catch { throw invalidArgument(); }
      if (!/^lxp_live_[0-9a-f]{64}$/u.test(value)) throw invalidArgument();
      return consumer(`LayerX-Key ${this.keyId}:${value}`);
    });
  }

  public toString(): string { return "[REDACTED]"; }
  public toJSON(): string { return "[REDACTED]"; }
}

export interface AgentHttpTransportOptions {
  readonly endpoint: URL | string;
  readonly credential?: LayerXKeyCredential;
  readonly timeoutMs?: number;
  readonly maximumResponseBytes?: number;
}

/** Exact HTTP transport for the six schema-routed Programs operations. */
export class AgentHttpTransport implements ProductionTransport {
  readonly #endpoint: URL;
  readonly #credential: LayerXKeyCredential | undefined;
  readonly #timeoutMs: number;
  readonly #maximumResponseBytes: number;

  public constructor(options: AgentHttpTransportOptions) {
    this.#endpoint = validateEndpoint(options.endpoint);
    this.#credential = options.credential;
    this.#timeoutMs = exactPositive(options.timeoutMs ?? DEFAULT_TIMEOUT_MS);
    this.#maximumResponseBytes = exactPositive(options.maximumResponseBytes ?? MAX_RESPONSE_BYTES);
    if (this.#maximumResponseBytes > MAX_RESPONSE_BYTES) throw invalidArgument();
  }

  public async call<TRequest, TResponse>(call: TransportCall<TRequest>): Promise<TResponse> {
    if (call.plane !== "agent") throw unavailableCapability();
    const route = PROGRAM_ROUTES[call.operation as AgentOperation];
    if (route === undefined) throw unavailableCapability();
    const request = record(call.request);
    const path = route.pathField === undefined
      ? route.path
      : route.path.replace(`{${route.pathField}}`, encodeURIComponent(hex32Field(request, route.pathField)));
    if (call.operation === "program.call") {
      if (call.idempotencyKey === undefined || !HEX32.test(call.idempotencyKey)) throw invalidArgument();
    } else if (call.idempotencyKey !== undefined) {
      throw invalidArgument();
    }
    requireRequestedVerification(call.operation as AgentOperation, request);
    requireExactRequest(call.operation as AgentOperation, request);
    let body: Buffer;
    try { body = Buffer.from(JSON.stringify(request), "utf8"); }
    catch { throw invalidArgument(); }
    if (body.length > MAX_REQUEST_BYTES) throw invalidArgument();
    const endpoint = routeEndpoint(this.#endpoint, path);
    const headers: http.OutgoingHttpHeaders = {
      Accept: "application/json",
      "Content-Type": "application/json",
      "Content-Length": body.length,
      "User-Agent": "layerx-typescript/0.1.0",
    };
    if (call.idempotencyKey !== undefined) headers["Idempotency-Key"] = call.idempotencyKey;
    if (this.#credential !== undefined) {
      this.#credential.use((authorization) => { headers.Authorization = authorization; });
    }
    return await this.dispatch<TResponse>(endpoint, route.method, headers, body, call.operation as AgentOperation);
  }

  private dispatch<TResponse>(
    endpoint: URL,
    method: "GET" | "POST",
    headers: http.OutgoingHttpHeaders,
    body: Buffer,
    operation: AgentOperation,
  ): Promise<TResponse> {
    return new Promise<TResponse>((resolve, reject) => {
      const driver = endpoint.protocol === "https:" ? https : http;
      let settled = false;
      const request = driver.request(endpoint, { method, headers, timeout: this.#timeoutMs }, (response) => {
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
            if (response.headers["content-type"] !== "application/json") throw decodeFailure();
            const value = decodeEnvelope(response.statusCode ?? 0, Buffer.concat(chunks), operation);
            finish(resolve, value as TResponse);
          } catch (error) {
            finish(reject, error);
          }
        });
        response.on("error", () => finish(reject, transportFailure(operation)));
      });
      const finish = <T>(callback: (value: T) => void, value: T): void => {
        if (settled) return;
        settled = true;
        callback(value);
      };
      request.on("timeout", () => request.destroy());
      request.on("error", () => finish(reject, transportFailure(operation)));
      request.end(body);
    });
  }
}

function record(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw invalidArgument();
  return value as Readonly<Record<string, unknown>>;
}

function hex32Field(value: Readonly<Record<string, unknown>>, field: string): string {
  const candidate = value[field];
  if (typeof candidate !== "string" || !HEX32.test(candidate)) throw invalidArgument();
  return candidate;
}

function requireRequestedVerification(operation: AgentOperation, request: Readonly<Record<string, unknown>>): void {
  if (operation === "program.discover" || operation === "program.interface"
    || operation === "program.receipt" || operation === "program.activity") {
    if (request.requested_verification_level !== "sequencer-signed") throw invalidArgument();
  }
}

function requireExactRequest(operation: AgentOperation, request: Readonly<Record<string, unknown>>): void {
  const fields: Readonly<Partial<Record<AgentOperation, readonly string[]>>> = {
    "program.discover": ["program_id", "requested_verification_level"],
    "program.interface": ["program_id", "requested_verification_level"],
    "program.simulate": ["program_id", "calldata", "budget", "capabilities", "signed_activity"],
    "program.call": ["program_id", "calldata", "budget", "capabilities", "signed_activity"],
    "program.receipt": ["idempotency_key", "expected_activity_id", "requested_verification_level"],
    "program.activity": ["activity_id", "requested_verification_level"],
  };
  const expected = fields[operation];
  if (expected === undefined || Object.keys(request).length !== expected.length || expected.some((field) => !(field in request))) throw invalidArgument();
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

function decodeEnvelope(status: number, encoded: Buffer, operation: AgentOperation): unknown {
  let envelope: Readonly<Record<string, unknown>>;
  try { envelope = record(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(encoded)) as unknown); }
  catch { throw decodeFailure(); }
  if ("class" in envelope) {
    exactKeys(envelope, ["class", "protocol_result_code", "retriability", "reason", "request_id"]);
    throw serviceError(status, envelope);
  }
  exactKeys(envelope, ["request_id", "value", "verification_status"]);
  const requestId = envelope.request_id;
  if (status < 200 || status >= 300 || !validRequestId(requestId) || !("value" in envelope)) {
    throw decodeFailure(typeof requestId === "string" ? requestId : undefined);
  }
  if (!acceptedProgramVerification(operation, envelope.value, envelope.verification_status)) throw new PlatformSdkError({ code: "verification-failure", retry: "never", requestId });
  return envelope.value;
}

function acceptedProgramVerification(operation: AgentOperation, result: unknown, value: unknown): boolean {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return false;
  const status = value as Readonly<Record<string, unknown>>;
  const resultState = result !== null && typeof result === "object" && !Array.isArray(result)
    ? (result as Readonly<Record<string, unknown>>).state : undefined;
  if (operation === "program.discover" || operation === "program.interface") {
    return exactUnverified(status, "server_side_receipt_verification_only");
  }
  if ((operation === "program.call" || operation === "program.receipt" || operation === "program.activity")
    && (resultState === "unknown" || resultState === "pending")) {
    return exactUnverified(status, "receipt_pending");
  }
  return Object.keys(status).length === 2 && status.state === "Achieved" && status.level === "SequencerSigned";
}

function exactUnverified(value: Readonly<Record<string, unknown>>, reason: string): boolean {
  return Object.keys(value).length === 4 && value.state === "Unverified" && value.requested === "SequencerSigned"
    && value.achieved === "Unverified" && value.reason === reason;
}

function serviceError(status: number, error: Readonly<Record<string, unknown>>): PlatformSdkError {
  const requestId = validRequestId(error.request_id) ? error.request_id : undefined;
  const code = typeof error.class === "string" ? ERROR_CLASS[error.class] : undefined;
  const retriability = error.retriability;
  const reason = error.reason;
  const protocol = error.protocol_result_code;
  if (status >= 200 && status < 300 || requestId === undefined || code === undefined
    || (retriability !== "Terminal" && retriability !== "Retriable")
    || typeof reason !== "string" || !/^[a-z0-9_.]+$/u.test(reason)
    || (protocol !== null && (!Number.isSafeInteger(protocol) || typeof protocol !== "number"))) {
    throw decodeFailure(requestId);
  }
  return new PlatformSdkError({
    code,
    retry: retriability === "Retriable" ? "safe" : "never",
    requestId,
    ...(protocol === null ? {} : { protocolResultCode: protocol as number }),
  });
}

function exactKeys(value: Readonly<Record<string, unknown>>, required: readonly string[]): void {
  if (Object.keys(value).length !== required.length || required.some((key) => !(key in value))) throw decodeFailure();
}

function validRequestId(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 128 && /^[\x21-\x7e]+$/u.test(value);
}

function transportFailure(operation: AgentOperation): PlatformSdkError {
  return new PlatformSdkError({
    code: operation === "program.call" ? "unknown-outcome" : "transport-failure",
    retry: operation === "program.call" ? "unknown-outcome" : "safe",
  });
}

function invalidArgument(): PlatformSdkError {
  return new PlatformSdkError({ code: "invalid-argument", retry: "never" });
}

function unavailableCapability(): PlatformSdkError {
  return new PlatformSdkError({ code: "unavailable-capability", retry: "never" });
}

function decodeFailure(requestId?: string): PlatformSdkError {
  return new PlatformSdkError({ code: "decode-failure", retry: "never", ...(requestId === undefined ? {} : { requestId }) });
}
