import {
  AgentMiddlewareError,
  type AgentMiddleware,
  type AgentReceiptResolver,
  type AgentSpendRequest,
  type AgentSpendResult,
} from "@sidiora/layerx-agent-middleware";
import {
  PlatformSdkError,
  verifyReceipt,
  type ProductionClient,
} from "@sidiora/layerx-sdk";
import { AgentIntegrationError, toHex, type AgentDeclaredConfig } from "./config.js";

export type ToolJson =
  | string
  | number
  | boolean
  | null
  | readonly ToolJson[]
  | { readonly [key: string]: ToolJson };

export type ToolJsonObject = { readonly [key: string]: ToolJson };

export interface ToolDefinition {
  readonly name: string;
  readonly title: string;
  readonly description: string;
  readonly inputSchema: ToolJsonObject & { readonly type: "object" };
}

export type ToolOutcome =
  | { readonly ok: true; readonly tool: string; readonly result: ToolJsonObject }
  | { readonly ok: false; readonly tool: string; readonly code: string };

const HEX32_PATTERN = "^[0-9a-f]{64}$";
const AMOUNT_PATTERN = "^(0|[1-9][0-9]*)$";

export const SPEND_TOOL: ToolDefinition = {
  name: "layerx_spend",
  title: "Spend through LayerX",
  description:
    "Reserve budget, prepare, sign, submit and locally verify a LayerX payment. "
    + "Returns verified receipt evidence, an approval hold, honest pending/unknown state, or a typed refusal.",
  inputSchema: {
    type: "object",
    additionalProperties: false,
    required: ["asset", "amount", "recipient", "payloadBase64", "payloadHash", "accountSequence", "timestampBound", "idempotencyKey"],
    properties: {
      asset: { type: "string", pattern: HEX32_PATTERN, description: "Asset identifier as 64 lowercase hex characters." },
      amount: { type: "string", pattern: AMOUNT_PATTERN, description: "Amount in protocol base units, integer only." },
      recipient: { type: "string", pattern: HEX32_PATTERN, description: "Recipient account as 64 lowercase hex characters." },
      payloadBase64: { type: "string", minLength: 1, maxLength: 1_398_104, description: "Canonical activity payload, base64." },
      payloadHash: { type: "string", pattern: HEX32_PATTERN, description: "SHA-256 of the canonical payload." },
      accountSequence: { type: "string", pattern: AMOUNT_PATTERN, description: "Expected account sequence." },
      timestampBound: { type: "string", pattern: AMOUNT_PATTERN, description: "Upper timestamp bound in milliseconds." },
      idempotencyKey: { type: "string", minLength: 1, maxLength: 255, description: "Replay-safe key for this spend." },
      feeLimit: { type: "string", pattern: AMOUNT_PATTERN, description: "Optional fee ceiling; defaults to the declared limit." },
      tenant: { type: "string", minLength: 1, maxLength: 512, description: "Optional tenant override." },
      actor: { type: "string", minLength: 1, maxLength: 512, description: "Optional actor override." },
      authority: { type: "string", minLength: 1, maxLength: 512, description: "Optional authority override." },
    },
  },
};

export const TRACK_TOOL: ToolDefinition = {
  name: "layerx_track",
  title: "Track a LayerX submission",
  description: "Read the current state of a previously submitted LayerX activity without changing anything.",
  inputSchema: {
    type: "object",
    additionalProperties: false,
    required: ["submissionRef"],
    properties: {
      submissionRef: { type: "string", minLength: 1, maxLength: 512, description: "Submission reference returned by a spend." },
    },
  },
};

export const VERIFY_RECEIPT_TOOL: ToolDefinition = {
  name: "layerx_verify_receipt",
  title: "Verify a LayerX receipt locally",
  description:
    "Resolve a receipt reference and verify its sequencer signature, balances and settlement target on this machine. "
    + "A receipt that does not verify is refused, never reported as settled.",
  inputSchema: {
    type: "object",
    additionalProperties: false,
    required: ["receiptRef", "asset", "amount", "recipient"],
    properties: {
      receiptRef: { type: "string", minLength: 1, maxLength: 512, description: "Receipt reference to resolve." },
      asset: { type: "string", pattern: HEX32_PATTERN, description: "Expected asset identifier." },
      amount: { type: "string", pattern: AMOUNT_PATTERN, description: "Expected amount in protocol base units." },
      recipient: { type: "string", pattern: HEX32_PATTERN, description: "Expected recipient account." },
    },
  },
};

export const LAYERX_TOOLS: readonly ToolDefinition[] = [
  SPEND_TOOL,
  TRACK_TOOL,
  VERIFY_RECEIPT_TOOL,
];

export interface AgentToolExecutorConfig {
  readonly middleware: AgentMiddleware;
  readonly client: ProductionClient;
  readonly receipts: AgentReceiptResolver;
  readonly config: AgentDeclaredConfig;
}

export class AgentToolExecutor {
  readonly #middleware: AgentMiddleware;
  readonly #client: ProductionClient;
  readonly #receipts: AgentReceiptResolver;
  readonly #config: AgentDeclaredConfig;

  public constructor(config: AgentToolExecutorConfig) {
    this.#middleware = config.middleware;
    this.#client = config.client;
    this.#receipts = config.receipts;
    this.#config = config.config;
  }

  public get definitions(): readonly ToolDefinition[] {
    return LAYERX_TOOLS;
  }

  public async execute(name: string, input: unknown): Promise<ToolOutcome> {
    try {
      if (name === SPEND_TOOL.name) {
        return { ok: true, tool: name, result: describeSpend(await this.#middleware.spend(this.#spendRequest(input))) };
      }
      if (name === TRACK_TOOL.name) {
        return { ok: true, tool: name, result: await this.#track(input) };
      }
      if (name === VERIFY_RECEIPT_TOOL.name) {
        return { ok: true, tool: name, result: await this.#verify(input) };
      }
      throw new AgentIntegrationError("unknown-tool");
    } catch (error) {
      return { ok: false, tool: name, code: refusalCode(error) };
    }
  }

  #spendRequest(input: unknown): AgentSpendRequest {
    const object = asObject(input);
    return {
      tenant: optionalText(object, "tenant", 512) ?? this.#config.tenant,
      actor: optionalText(object, "actor", 512) ?? this.#config.actor,
      authority: optionalText(object, "authority", 512) ?? this.#config.authority,
      accountSequence: canonicalInteger(object, "accountSequence"),
      timestampBound: canonicalInteger(object, "timestampBound"),
      idempotencyKey: text(object, "idempotencyKey", 255),
      feeLimit: optionalCanonicalInteger(object, "feeLimit") ?? this.#config.feeLimit,
      payloadBase64: text(object, "payloadBase64", 1_398_104),
      payloadHash: hex32(object, "payloadHash"),
      asset: hex32(object, "asset"),
      amount: canonicalInteger(object, "amount"),
      recipient: hex32(object, "recipient"),
    };
  }

  async #track(input: unknown): Promise<ToolJsonObject> {
    const object = asObject(input);
    const submissionRef = text(object, "submissionRef", 512);
    const response = await this.#client.agent<{ readonly submission_ref: string }, unknown>(
      "track",
      { submission_ref: submissionRef },
    );
    const submission = asObject(response);
    const output: Record<string, ToolJson> = {
      submissionRef,
      state: jsonValue(submission["state"]),
    };
    if (submission["verification_level"] !== undefined) {
      output["verificationLevel"] = jsonValue(submission["verification_level"]);
    }
    return output;
  }

  async #verify(input: unknown): Promise<ToolJsonObject> {
    const object = asObject(input);
    const receiptRef = text(object, "receiptRef", 512);
    const asset = hex32(object, "asset");
    const amount = canonicalInteger(object, "amount");
    const recipient = hex32(object, "recipient");
    const evidence = await this.#receipts.resolve(receiptRef);
    const verification = await verifyReceipt(evidence.canonicalReceipt, evidence.authorizedBatch);
    if (
      verification.receipt.amount !== BigInt(amount)
      || toHex(verification.receipt.asset) !== asset
      || toHex(verification.receipt.to) !== recipient
    ) {
      throw new AgentMiddlewareError("verification-failure");
    }
    return {
      receiptRef,
      level: verification.level,
      receiptDigest: toHex(verification.receiptDigest),
      resultCode: verification.receipt.resultCode,
      amount,
      asset,
      recipient,
    };
  }
}

export function renderOutcome(outcome: ToolOutcome): ToolJsonObject {
  return outcome.ok
    ? { ok: true, tool: outcome.tool, result: outcome.result }
    : { ok: false, tool: outcome.tool, code: outcome.code };
}

export function describeSpend(result: AgentSpendResult): ToolJsonObject {
  if (result.kind === "verified") {
    return {
      kind: result.kind,
      submissionRef: result.submission.submission_ref,
      receiptDigest: toHex(result.verification.receiptDigest),
      level: result.verification.level,
      reservationState: result.reservation.state,
    };
  }
  if (result.kind === "approval-hold") {
    return {
      kind: result.kind,
      approvalId: result.approval.approvalId,
      canonicalBytesDigest: result.approval.canonicalBytesDigest,
      reservationState: result.reservation.state,
    };
  }
  if (result.kind === "pending") {
    return {
      kind: result.kind,
      submissionRef: result.submission.submission_ref,
      reservationState: result.reservation.state,
    };
  }
  if (result.kind === "unknown") {
    const output: Record<string, ToolJson> = {
      kind: result.kind,
      reservationState: result.reservation.state,
    };
    if (result.submission !== undefined) {
      output["submissionRef"] = result.submission.submission_ref;
    }
    return output;
  }
  if (result.kind === "refused") {
    return {
      kind: result.kind,
      code: result.code,
      retry: result.retry,
      ...(result.retryAfterMs === undefined ? {} : { retryAfterMs: result.retryAfterMs }),
      ...(result.protocolResultCode === undefined ? {} : { protocolResultCode: result.protocolResultCode }),
      ...(result.submissionState === undefined ? {} : { submissionState: result.submissionState }),
      reservationState: result.reservation.state,
    };
  }
  return { kind: result.kind, code: result.code, retry: result.retry, available: result.available };
}

export function refusalCode(error: unknown): string {
  if (error instanceof PlatformSdkError) {
    return error.code;
  }
  if (error instanceof AgentMiddlewareError) {
    return error.code;
  }
  if (error instanceof AgentIntegrationError) {
    return error.code;
  }
  return "transport-failure";
}

function jsonValue(value: unknown): ToolJson {
  if (value === null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => jsonValue(entry));
  }
  if (typeof value === "object") {
    const output: Record<string, ToolJson> = {};
    for (const [name, entry] of Object.entries(value as Record<string, unknown>)) {
      output[name] = jsonValue(entry);
    }
    return output;
  }
  throw new AgentIntegrationError("invalid-tool-input");
}

function asObject(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return value as Readonly<Record<string, unknown>>;
}

function text(object: Readonly<Record<string, unknown>>, name: string, maximum: number): string {
  const value = object[name];
  if (typeof value !== "string" || value.length === 0 || value.length > maximum || value.includes("\0")) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return value;
}

function optionalText(
  object: Readonly<Record<string, unknown>>,
  name: string,
  maximum: number,
): string | undefined {
  return object[name] === undefined ? undefined : text(object, name, maximum);
}

function hex32(object: Readonly<Record<string, unknown>>, name: string): string {
  const value = object[name];
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return value;
}

function canonicalInteger(object: Readonly<Record<string, unknown>>, name: string): string {
  const value = object[name];
  if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/u.test(value) || value.length > 39) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return value;
}

function optionalCanonicalInteger(
  object: Readonly<Record<string, unknown>>,
  name: string,
): string | undefined {
  return object[name] === undefined ? undefined : canonicalInteger(object, name);
}
