import { SecretBytes } from "@sidiora/layerx-sdk";

export const DECLARED_KEYS = [
  "LAYERX_AGENT_RPC_URL",
  "LAYERX_BUDGET_SERVICE_URL",
  "LAYERX_SIGNER_SERVICE_URL",
  "LAYERX_RECEIPT_SERVICE_URL",
  "LAYERX_TENANT",
  "LAYERX_ACTOR",
  "LAYERX_AUTHORITY",
  "LAYERX_FEE_LIMIT",
  "LAYERX_MAX_TRACK_POLLS",
  "LAYERX_REQUEST_TIMEOUT_MS",
  "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON",
  "LAYERX_WEBHOOK_MAX_AGE_MS",
  "LAYERX_WEBHOOK_LEASE_MS",
  "LAYERX_WEBHOOK_DELIVERY_STORE_PATH",
  "LAYERX_WEBHOOK_LISTEN_HOST",
  "LAYERX_WEBHOOK_LISTEN_PORT",
  "LAYERX_A2A_URL",
  "LAYERX_TOKEN",
] as const;

export type DeclaredKey = (typeof DECLARED_KEYS)[number];

export type Environment = Readonly<Record<string, string | undefined>>;

export type AgentIntegrationErrorCode =
  | "missing-declared-key"
  | "invalid-declared-key"
  | "client-runtime-refused"
  | "unknown-tool"
  | "invalid-tool-input"
  | "duplicate-header"
  | "unverifiable-body"
  | "protocol-violation"
  | "service-refused";

export class AgentIntegrationError extends Error {
  public constructor(public readonly code: AgentIntegrationErrorCode) {
    super(code);
    this.name = "AgentIntegrationError";
  }
}

export interface AgentWebhookSettings {
  readonly publicKeys: Readonly<Record<string, Uint8Array>>;
  readonly maximumAgeMs: number;
  readonly leaseMs: number;
}

export interface WebhookListener {
  readonly host: string;
  readonly port: number;
}

export interface AgentDeclaredConfig {
  readonly agentRpcUrl: string;
  readonly budgetServiceUrl: string;
  readonly signerServiceUrl: string;
  readonly receiptServiceUrl: string;
  readonly tenant: string;
  readonly actor: string;
  readonly authority: string;
  readonly feeLimit: string;
  readonly maximumTrackPolls: number;
  readonly requestTimeoutMs: number;
  readonly webhook: AgentWebhookSettings;
  readonly webhookDeliveryStorePath: string;
}

export function assertServerRuntime(): void {
  const scope = globalThis as { readonly window?: unknown; readonly document?: unknown };
  if (scope.window !== undefined || scope.document !== undefined) {
    throw new AgentIntegrationError("client-runtime-refused");
  }
}

export function readDeclaredConfig(environment: Environment): AgentDeclaredConfig {
  assertServerRuntime();
  return {
    agentRpcUrl: endpoint(required(environment, "LAYERX_AGENT_RPC_URL")),
    budgetServiceUrl: endpoint(required(environment, "LAYERX_BUDGET_SERVICE_URL")),
    signerServiceUrl: endpoint(required(environment, "LAYERX_SIGNER_SERVICE_URL")),
    receiptServiceUrl: endpoint(required(environment, "LAYERX_RECEIPT_SERVICE_URL")),
    tenant: bounded(required(environment, "LAYERX_TENANT"), 512),
    actor: bounded(required(environment, "LAYERX_ACTOR"), 512),
    authority: bounded(required(environment, "LAYERX_AUTHORITY"), 512),
    feeLimit: canonicalInteger(required(environment, "LAYERX_FEE_LIMIT")),
    maximumTrackPolls: boundedInteger(optional(environment, "LAYERX_MAX_TRACK_POLLS") ?? "20", 0, 1_000),
    requestTimeoutMs: boundedInteger(optional(environment, "LAYERX_REQUEST_TIMEOUT_MS") ?? "30000", 1_000, 300_000),
    webhook: {
      publicKeys: parseWebhookKeys(required(environment, "LAYERX_WEBHOOK_PUBLIC_KEYS_JSON")),
      maximumAgeMs: boundedInteger(optional(environment, "LAYERX_WEBHOOK_MAX_AGE_MS") ?? "300000", 1, 86_400_000),
      leaseMs: boundedInteger(optional(environment, "LAYERX_WEBHOOK_LEASE_MS") ?? "60000", 1, 86_400_000),
    },
    webhookDeliveryStorePath: filesystemPath(
      optional(environment, "LAYERX_WEBHOOK_DELIVERY_STORE_PATH") ?? ".layerx/webhook-deliveries-v1.json",
    ),
  };
}

export function readWebhookListener(environment: Environment): WebhookListener | undefined {
  const port = optional(environment, "LAYERX_WEBHOOK_LISTEN_PORT");
  if (port === undefined) {
    return undefined;
  }
  return {
    host: bounded(optional(environment, "LAYERX_WEBHOOK_LISTEN_HOST") ?? "127.0.0.1", 255),
    port: boundedInteger(port, 1, 65_535),
  };
}

export function readServiceToken(environment: Environment): SecretBytes {
  assertServerRuntime();
  return new SecretBytes(new TextEncoder().encode(required(environment, "LAYERX_TOKEN")));
}

export function required(environment: Environment, key: DeclaredKey): string {
  const value = environment[key];
  if (value === undefined || value.length === 0) {
    throw new AgentIntegrationError("missing-declared-key");
  }
  return value;
}

export function optional(environment: Environment, key: DeclaredKey): string | undefined {
  const value = environment[key];
  return value === undefined || value.length === 0 ? undefined : value;
}

export function endpoint(value: string): string {
  let url: URL;
  try {
    url = new URL(value);
  } catch {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  const loopback = url.hostname === "127.0.0.1" || url.hostname === "localhost" || url.hostname === "[::1]";
  if (url.username.length > 0 || url.password.length > 0 || url.hash.length > 0) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  if (url.protocol !== "https:" && !(url.protocol === "http:" && loopback)) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return url.toString();
}

export function bounded(value: string, maximum: number): string {
  if (value.length === 0 || value.length > maximum || value.includes("\0")) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return value;
}

export function canonicalInteger(value: string): string {
  if (!/^(0|[1-9][0-9]*)$/u.test(value) || value.length > 39) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return value;
}

export function filesystemPath(value: string): string {
  if (value.length === 0 || value.length > 4_096 || value.includes("\0")) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return value;
}

export function boundedInteger(value: string, minimum: number, maximum: number): number {
  if (!/^(0|[1-9][0-9]*)$/u.test(value) || value.length > 12) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed < minimum || parsed > maximum) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return parsed;
}

export function parseHex32(value: string): Uint8Array {
  if (!/^[0-9a-f]{64}$/u.test(value)) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return Uint8Array.from({ length: 32 }, (_unused, index) =>
    Number.parseInt(value.slice(index * 2, index * 2 + 2), 16));
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function parseWebhookKeys(value: string): Readonly<Record<string, Uint8Array>> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  const keys: Record<string, Uint8Array> = {};
  for (const [name, encoded] of Object.entries(parsed as Record<string, unknown>)) {
    if (typeof encoded !== "string" || !/^[A-Za-z0-9._-]{1,64}$/u.test(name)) {
      throw new AgentIntegrationError("invalid-declared-key");
    }
    keys[name] = parseHex32(encoded);
  }
  if (Object.keys(keys).length === 0) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return Object.freeze(keys);
}
