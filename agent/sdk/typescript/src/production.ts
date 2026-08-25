import type { Operation as AgentOperation } from "./generated/client.js";

export const AGENT_OPERATIONS = [
  "agent.register",
  "approval.approve",
  "approval.get",
  "approval.list",
  "approval.reject",
  "availability.fetch",
  "budget.create",
  "budget.fund",
  "budget.list",
  "budget.reconciliation",
  "budget.revoke",
  "capability.attenuate",
  "capability.create",
  "capability.list",
  "capability.revoke",
  "export.offline",
  "prepare",
  "project",
  "read.account",
  "read.balance",
  "read.batch",
  "read.checkpoint",
  "read.history",
  "read.module_state",
  "read.proof_bundle",
  "session.close",
  "session.list",
  "session.open",
  "session.refresh",
  "sign",
  "submit",
  "subscription.acknowledge",
  "subscription.create",
  "subscription.delete",
  "subscription.health",
  "subscription.list",
  "subscription.pause",
  "subscription.resume",
  "track",
  "wait",
] as const satisfies readonly AgentOperation[];

export const HUMAN_OPERATIONS = [
  "account.create",
  "activity.entry",
  "activity.export.evidence",
  "activity.export.statement",
  "activity.query",
  "agent.archive",
  "agent.create",
  "agent.get",
  "agent.limit",
  "agent.list",
  "agent.pause",
  "agent.reclaim",
  "agent.recover",
  "agent.resume",
  "agent.rotate",
  "approval.approve",
  "approval.get",
  "approval.list",
  "approval.reject",
  "binding.rebind",
  "binding.statement",
  "binding.status",
  "binding.submit",
  "deposit.confirm",
  "deposit.start",
  "evidence.get",
  "exit.eligibility",
  "exit.start",
  "journey.get",
  "journey.list",
  "move.commit",
  "move.quote",
  "notification.list",
  "notification.preferences.get",
  "notification.preferences.set",
  "notification.read",
  "onboarding.resume",
  "onboarding.status",
  "passkey.assert.begin",
  "passkey.assert.finish",
  "passkey.register.begin",
  "passkey.register.finish",
  "profile.get",
  "profile.update",
  "session.list",
  "session.open",
  "session.refresh",
  "session.revoke",
  "session.revoke-all",
  "stepup.begin",
  "stepup.finish",
  "stream.next",
  "stream.open",
  "version",
  "withdraw.claim",
  "withdraw.start",
] as const;

export type HumanOperation = (typeof HUMAN_OPERATIONS)[number];
export type PlatformPlane = "agent" | "human";
export type RetryClass = "never" | "safe" | "after" | "unknown-outcome";

export const SDK_ERROR_CODES = [
  "invalid-argument",
  "idempotency-required",
  "transport-failure",
  "deadline",
  "protocol-incompatibility",
  "unavailable-capability",
  "core-rejection",
  "verification-failure",
  "policy-refusal",
  "capability-refusal",
  "budget-refusal",
  "rate-limit",
  "idempotency-conflict",
  "decode-failure",
  "unknown-outcome",
  "internal-fault",
] as const;

export type SdkErrorCode = (typeof SDK_ERROR_CODES)[number];

const SAFE_MESSAGES: Readonly<Record<SdkErrorCode, string>> = Object.freeze({
  "invalid-argument": "The SDK rejected an invalid argument.",
  "idempotency-required": "This operation requires an idempotency key.",
  "transport-failure": "The request could not reach the service.",
  deadline: "The request deadline elapsed.",
  "protocol-incompatibility": "The service protocol is not compatible with this SDK.",
  "unavailable-capability": "The requested operation is unavailable.",
  "core-rejection": "The protocol refused the request.",
  "verification-failure": "Local verification failed.",
  "policy-refusal": "Policy refused the request.",
  "capability-refusal": "The caller does not have the required authority.",
  "budget-refusal": "The configured budget refused the request.",
  "rate-limit": "The request rate limit was reached.",
  "idempotency-conflict": "The idempotency key belongs to a different request.",
  "decode-failure": "The service response did not match the contract.",
  "unknown-outcome": "The request outcome is unknown and must be resolved before retrying.",
  "internal-fault": "The service could not complete the request.",
});

export interface SafeErrorDetails {
  readonly code: SdkErrorCode;
  readonly retry: RetryClass;
  readonly requestId?: string;
  readonly protocolResultCode?: number;
  readonly retryAfterMs?: number;
}

export class PlatformSdkError extends Error {
  public readonly code: SdkErrorCode;
  public readonly retry: RetryClass;
  public readonly requestId: string | undefined;
  public readonly protocolResultCode: number | undefined;
  public readonly retryAfterMs: number | undefined;

  public constructor(details: SafeErrorDetails) {
    super(SAFE_MESSAGES[details.code]);
    this.name = "PlatformSdkError";
    this.code = details.code;
    this.retry = details.retry;
    this.requestId = details.requestId;
    this.protocolResultCode = details.protocolResultCode;
    this.retryAfterMs = details.retryAfterMs;
  }

  public toJSON(): SafeErrorDetails {
    return {
      code: this.code,
      retry: this.retry,
      ...(this.requestId === undefined ? {} : { requestId: this.requestId }),
      ...(this.protocolResultCode === undefined ? {} : { protocolResultCode: this.protocolResultCode }),
      ...(this.retryAfterMs === undefined ? {} : { retryAfterMs: this.retryAfterMs }),
    };
  }
}

declare const idempotencyKeyBrand: unique symbol;
export type IdempotencyKey = string & { readonly [idempotencyKeyBrand]: true };

export function idempotencyKey(value: string): IdempotencyKey {
  if (value.length === 0 || value.length > 255 || value.includes("\0")) {
    throw new PlatformSdkError({ code: "invalid-argument", retry: "never" });
  }
  return value as IdempotencyKey;
}

declare const protocolAmountBrand: unique symbol;
export type ProtocolAmount = bigint & { readonly [protocolAmountBrand]: true };

export function protocolAmount(value: bigint | string): ProtocolAmount {
  const parsed = typeof value === "bigint"
    ? value
    : /^(0|[1-9][0-9]*)$/u.test(value)
      ? BigInt(value)
      : -1n;
  if (parsed < 0n || parsed > 340282366920938463463374607431768211455n) {
    throw new PlatformSdkError({ code: "invalid-argument", retry: "never" });
  }
  return parsed as ProtocolAmount;
}

export class SecretBytes {
  readonly #bytes: Uint8Array;
  #destroyed = false;

  public constructor(bytes: Uint8Array) {
    if (bytes.length === 0) {
      throw new PlatformSdkError({ code: "invalid-argument", retry: "never" });
    }
    this.#bytes = bytes.slice();
  }

  public withBytes<T>(consumer: (bytes: Uint8Array) => T): T {
    if (this.#destroyed) {
      throw new PlatformSdkError({ code: "invalid-argument", retry: "never" });
    }
    const scoped = this.#bytes.slice();
    try {
      const result = consumer(scoped);
      if (result instanceof Promise) {
        return result.finally(() => scoped.fill(0)) as T;
      }
      scoped.fill(0);
      return result;
    } catch (error) {
      scoped.fill(0);
      throw error;
    }
  }

  public destroy(): void {
    this.#bytes.fill(0);
    this.#destroyed = true;
  }

  public toString(): string {
    return "[REDACTED]";
  }

  public toJSON(): string {
    return "[REDACTED]";
  }
}

export interface TransportCall<TRequest> {
  readonly plane: PlatformPlane;
  readonly operation: AgentOperation | HumanOperation;
  readonly request: TRequest;
  readonly idempotencyKey?: IdempotencyKey;
}

export interface ProductionTransport {
  call<TRequest, TResponse>(call: TransportCall<TRequest>): Promise<TResponse>;
}

export interface CallOptions {
  readonly idempotencyKey?: IdempotencyKey;
}

export interface SdkTelemetryEvent {
  readonly plane: PlatformPlane;
  readonly operation: AgentOperation | HumanOperation;
  readonly outcome: "completed" | "refused";
  readonly code?: SdkErrorCode;
}

export type SdkTelemetry = (event: SdkTelemetryEvent) => void;

const AGENT_IDEMPOTENT = new Set<AgentOperation>([
  "agent.register",
  "approval.approve",
  "approval.reject",
  "budget.create",
  "budget.fund",
  "budget.revoke",
  "capability.attenuate",
  "capability.create",
  "capability.revoke",
  "prepare",
  "session.close",
  "session.open",
  "session.refresh",
  "sign",
  "submit",
  "subscription.acknowledge",
  "subscription.create",
  "subscription.delete",
  "subscription.pause",
  "subscription.resume",
]);

const HUMAN_IDEMPOTENT = new Set<HumanOperation>([
  "account.create",
  "activity.export.evidence",
  "activity.export.statement",
  "agent.archive",
  "agent.create",
  "agent.limit",
  "agent.pause",
  "agent.reclaim",
  "agent.recover",
  "agent.resume",
  "agent.rotate",
  "approval.approve",
  "approval.reject",
  "binding.rebind",
  "binding.submit",
  "deposit.start",
  "exit.start",
  "move.commit",
  "withdraw.start",
]);

function requiresIdempotency(plane: PlatformPlane, operation: AgentOperation | HumanOperation): boolean {
  return plane === "agent"
    ? AGENT_IDEMPOTENT.has(operation as AgentOperation)
    : HUMAN_IDEMPOTENT.has(operation as HumanOperation);
}

export class ProductionClient {
  public constructor(
    private readonly transport: ProductionTransport,
    private readonly telemetry?: SdkTelemetry,
  ) {}

  public agent<TRequest, TResponse>(
    operation: AgentOperation,
    request: TRequest,
    options: CallOptions = {},
  ): Promise<TResponse> {
    return this.execute("agent", operation, request, options);
  }

  public human<TRequest, TResponse>(
    operation: HumanOperation,
    request: TRequest,
    options: CallOptions = {},
  ): Promise<TResponse> {
    return this.execute("human", operation, request, options);
  }

  private async execute<TRequest, TResponse>(
    plane: PlatformPlane,
    operation: AgentOperation | HumanOperation,
    request: TRequest,
    options: CallOptions,
  ): Promise<TResponse> {
    if (requiresIdempotency(plane, operation) && options.idempotencyKey === undefined) {
      throw new PlatformSdkError({ code: "idempotency-required", retry: "never" });
    }
    try {
      const response = await this.transport.call<TRequest, TResponse>({
        plane,
        operation,
        request,
        ...(options.idempotencyKey === undefined ? {} : { idempotencyKey: options.idempotencyKey }),
      });
      this.telemetry?.({ plane, operation, outcome: "completed" });
      return response;
    } catch (error) {
      const safe = error instanceof PlatformSdkError
        ? error
        : new PlatformSdkError({ code: "transport-failure", retry: "safe" });
      this.telemetry?.({ plane, operation, outcome: "refused", code: safe.code });
      throw safe;
    }
  }
}

const PACKAGE_METADATA = Object.freeze({
  name: "@sidiora/layerx-sdk",
  version: "0.1.0",
  agentOperations: AGENT_OPERATIONS.length,
  humanOperations: HUMAN_OPERATIONS.length,
});

export function platform_sdk_typescript(): typeof PACKAGE_METADATA {
  return PACKAGE_METADATA;
}
