import {
  PlatformSdkError,
  ProductionClient,
  SDK_ERROR_CODES,
  idempotencyKey,
  protocolAmount,
  verifyReceipt,
  type AuthorizedReceiptBatch,
  type ReceiptVerification,
  type RetryClass,
  type SdkErrorCode,
} from "@sidiora/layerx-sdk";

const POST_SUBMIT_UNCERTAIN_CODES: ReadonlySet<SdkErrorCode> = new Set([
  "transport-failure",
  "deadline",
  "decode-failure",
  "internal-fault",
]);

export interface AgentSpendRequest {
  readonly tenant: string;
  readonly actor: string;
  readonly authority: string;
  readonly accountSequence: string;
  readonly timestampBound: string;
  readonly idempotencyKey: string;
  readonly feeLimit: string;
  readonly payloadBase64: string;
  readonly payloadHash: string;
  readonly asset: string;
  readonly amount: string;
  readonly recipient: string;
}

interface BudgetReservationBase {
  readonly reservationId: string;
  readonly requestDigest: string;
  readonly amount: string;
  readonly asset: string;
}

export type ReservedBudgetReservation = BudgetReservationBase & {
  readonly state: "reserved";
};

export type HeldBudgetReservation = BudgetReservationBase & {
  readonly state: "held";
  readonly approvalId: string;
  readonly canonicalBytesDigest: string;
};

export type CommittedBudgetReservation = BudgetReservationBase & {
  readonly state: "committed";
  readonly receiptDigest: string;
};

export type ReleasedBudgetReservation = BudgetReservationBase & {
  readonly state: "released";
  readonly refusal: AgentRefusal;
};

export type BudgetReservation =
  | ReservedBudgetReservation
  | HeldBudgetReservation
  | CommittedBudgetReservation
  | ReleasedBudgetReservation;

export type BudgetReserveResult =
  | { readonly kind: "reserved"; readonly reservation: BudgetReservation }
  | { readonly kind: "exhausted"; readonly available: string }
  | { readonly kind: "conflict" };

export interface BudgetTransition {
  readonly reservationId: string;
  readonly requestDigest: string;
  readonly amount: string;
  readonly asset: string;
}

export interface BudgetHoldTransition extends BudgetTransition {
  readonly approvalId: string;
  readonly canonicalBytesDigest: string;
}

export interface BudgetCommitTransition extends BudgetTransition {
  readonly receiptDigest: string;
}

export interface BudgetReleaseTransition extends BudgetTransition {
  readonly refusal: AgentRefusal;
}

export interface AgentBudgetLedger {
  reserve(request: {
    readonly tenant: string;
    readonly idempotencyKey: string;
    readonly requestDigest: string;
    readonly amount: string;
    readonly asset: string;
  }): Promise<BudgetReserveResult>;
  hold(transition: BudgetHoldTransition): Promise<HeldBudgetReservation>;
  commit(transition: BudgetCommitTransition): Promise<CommittedBudgetReservation>;
  release(transition: BudgetReleaseTransition): Promise<ReleasedBudgetReservation>;
}

export interface PreparedActivity {
  readonly preparation_ref: string;
  readonly unsigned_canonical_bytes: string;
  readonly signing_preimage: string;
  readonly disclosure: Readonly<Record<string, unknown>>;
  readonly expiry: string;
}

export interface AgentSigner {
  sign(prepared: PreparedActivity): Promise<string>;
}

export interface Submission {
  readonly submission_ref: string;
  readonly state: string | Readonly<Record<string, unknown>>;
  readonly evidence?: readonly unknown[];
  readonly verification_level?: number;
  readonly transitions?: readonly unknown[];
}

export interface AgentReceiptEvidence {
  readonly canonicalReceipt: Uint8Array;
  readonly authorizedBatch: AuthorizedReceiptBatch;
}

export interface AgentReceiptResolver {
  resolve(receiptRef: string): Promise<AgentReceiptEvidence>;
}

export interface ApprovalHold {
  readonly approvalId: string;
  readonly state: "Held";
  readonly canonicalBytesDigest: string;
  readonly enforcement: "daemon_enforced";
}

export type AgentRefusalRetry = Exclude<RetryClass, "unknown-outcome">;

export interface AgentRefusal {
  readonly code: SdkErrorCode;
  readonly retry: AgentRefusalRetry;
  readonly retryAfterMs?: number;
  readonly protocolResultCode?: number;
  readonly submissionState?: "Failed" | "Expired";
}

export interface AgentMiddlewareConfig {
  readonly client: ProductionClient;
  readonly budgets: AgentBudgetLedger;
  readonly signer: AgentSigner;
  readonly receipts: AgentReceiptResolver;
  readonly maximumTrackPolls?: number;
  readonly wait?: (milliseconds: number) => Promise<void>;
}

export type AgentSpendResult =
  | {
    readonly kind: "verified";
    readonly submission: Submission;
    readonly verification: ReceiptVerification;
    readonly reservation: CommittedBudgetReservation;
  }
  | { readonly kind: "approval-hold"; readonly approval: ApprovalHold; readonly reservation: HeldBudgetReservation }
  | { readonly kind: "pending"; readonly submission: Submission; readonly reservation: BudgetReservation }
  | { readonly kind: "unknown"; readonly reservation: BudgetReservation; readonly submission?: Submission }
  | ({ readonly kind: "refused"; readonly reservation: BudgetReservation } & AgentRefusal)
  | { readonly kind: "budget-refused"; readonly code: "budget-refusal"; readonly retry: "never"; readonly available: string };

export class AgentMiddleware {
  readonly #client: ProductionClient;
  readonly #budgets: AgentBudgetLedger;
  readonly #signer: AgentSigner;
  readonly #receipts: AgentReceiptResolver;
  readonly #maximumTrackPolls: number;
  readonly #wait: (milliseconds: number) => Promise<void>;

  public constructor(config: AgentMiddlewareConfig) {
    this.#client = config.client;
    this.#budgets = config.budgets;
    this.#signer = config.signer;
    this.#receipts = config.receipts;
    this.#maximumTrackPolls = config.maximumTrackPolls ?? 20;
    this.#wait = config.wait ?? ((milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)));
    if (!Number.isSafeInteger(this.#maximumTrackPolls) || this.#maximumTrackPolls < 0 || this.#maximumTrackPolls > 1_000) {
      throw new AgentMiddlewareError("invalid-request");
    }
  }

  public async spend(request: AgentSpendRequest): Promise<AgentSpendResult> {
    validateSpend(request);
    if (!await payloadHashMatches(request.payloadBase64, request.payloadHash)) {
      throw new AgentMiddlewareError("invalid-request");
    }
    const amount = protocolAmount(request.amount).toString();
    const mutationKey = idempotencyKey(request.idempotencyKey);
    const requestDigest = await digestSpend(request);
    const reserved = await this.#budgets.reserve({
      tenant: request.tenant,
      idempotencyKey: request.idempotencyKey,
      requestDigest,
      amount,
      asset: request.asset,
    });
    if (reserved.kind === "exhausted") {
      protocolAmount(reserved.available);
      return { kind: "budget-refused", code: "budget-refusal", retry: "never", available: reserved.available };
    }
    if (reserved.kind === "conflict") {
      throw new AgentMiddlewareError("idempotency-conflict");
    }
    const budget = { requestDigest, amount, asset: request.asset };
    const reservation = validateBudgetReservation(reserved.reservation, budget);
    if (reservation.state === "held") {
      return {
        kind: "approval-hold",
        approval: {
          approvalId: reservation.approvalId,
          state: "Held",
          canonicalBytesDigest: reservation.canonicalBytesDigest,
          enforcement: "daemon_enforced",
        },
        reservation,
      };
    }
    if (reservation.state === "released") {
      return { kind: "refused", reservation, ...reservation.refusal };
    }
    let prepared: PreparedActivity;
    try {
      prepared = parsePrepared(await this.#client.agent("prepare", {
        actor: request.actor,
        authority: request.authority,
        account_sequence: request.accountSequence,
        timestamp_bound: request.timestampBound,
        idempotency_key: request.idempotencyKey,
        fee_limit: request.feeLimit,
        payload: request.payloadBase64,
        payload_hash: request.payloadHash,
      }, { idempotencyKey: mutationKey }));
    } catch (error) {
      return this.#sdkFailure(error, reservation, budget, false);
    }
    let signature: string;
    try {
      signature = await this.#signer.sign(prepared);
    } catch (error) {
      return this.#sdkFailure(error, reservation, budget, false);
    }
    if (signature.length === 0 || signature.length > 16_384 || signature.includes("\0")) {
      const refusal: AgentRefusal = { code: "decode-failure", retry: "never" };
      try {
        const released = await this.#release(reservation, budget, refusal);
        return { kind: "refused", reservation: released, ...refusal };
      } catch {
        return { kind: "unknown", reservation };
      }
    }
    let submission: Submission;
    try {
      submission = parseSubmission(await this.#client.agent("submit", {
        preparation_ref: prepared.preparation_ref,
        signature,
      }, { idempotencyKey: mutationKey }));
    } catch (error) {
      if (error instanceof PlatformSdkError && (error.code === "policy-refusal" || error.code === "budget-refusal")) {
        let approval: ApprovalHold | undefined;
        try {
          approval = await this.#approvalHold(request.tenant, prepared);
        } catch {
          return { kind: "unknown", reservation };
        }
        if (approval !== undefined) {
          let held: BudgetReservation;
          try {
            held = validateBudgetReservation(
              await this.#budgets.hold({
                reservationId: reservation.reservationId,
                requestDigest,
                amount,
                asset: request.asset,
                approvalId: approval.approvalId,
                canonicalBytesDigest: approval.canonicalBytesDigest,
              }),
              budget,
              reservation.reservationId,
            );
          } catch {
            return { kind: "unknown", reservation };
          }
          if (
            held.state !== "held"
            || held.approvalId !== approval.approvalId
            || held.canonicalBytesDigest !== approval.canonicalBytesDigest
          ) {
            throw new AgentMiddlewareError("budget-conflict");
          }
          return { kind: "approval-hold", approval, reservation: held };
        }
      }
      return this.#sdkFailure(error, reservation, budget, true);
    }
    let state: ReturnType<typeof submissionState>;
    try {
      state = submissionState(submission);
    } catch {
      return { kind: "unknown", reservation, submission };
    }
    for (let poll = 0; poll < this.#maximumTrackPolls && state === "Pending"; poll += 1) {
      await this.#wait(Math.min((poll + 1) * 250, 2_500));
      try {
        submission = parseSubmission(await this.#client.agent("track", {
          submission_ref: submission.submission_ref,
        }));
        state = submissionState(submission);
      } catch (error) {
        if (error instanceof PlatformSdkError && error.retry === "unknown-outcome") {
          return { kind: "unknown", reservation, submission };
        }
        if (error instanceof PlatformSdkError) {
          return { kind: "pending", submission, reservation };
        }
        return { kind: "unknown", reservation, submission };
      }
    }
    if (state === "Unknown") {
      return { kind: "unknown", reservation, submission };
    }
    if (state === "Pending") {
      if (reservation.state === "committed") {
        return { kind: "unknown", reservation, submission };
      }
      return { kind: "pending", submission, reservation };
    }
    if (state === "Failed" || state === "Expired") {
      const refusal: AgentRefusal = {
        code: "core-rejection",
        retry: "never",
        submissionState: state,
      };
      try {
        const released = await this.#release(reservation, budget, refusal);
        return { kind: "refused", reservation: released, ...refusal };
      } catch {
        return { kind: "unknown", reservation, submission };
      }
    }
    const receiptRef = executedReceiptRef(submission);
    if (receiptRef === undefined) {
      if (reservation.state === "committed") {
        return { kind: "unknown", reservation, submission };
      }
      return { kind: "pending", submission, reservation };
    }
    let evidence: AgentReceiptEvidence;
    try {
      evidence = await this.#receipts.resolve(receiptRef);
    } catch {
      if (reservation.state === "committed") {
        return { kind: "unknown", reservation, submission };
      }
      return { kind: "pending", submission, reservation };
    }
    let verification: ReceiptVerification;
    try {
      verification = await verifyReceipt(evidence.canonicalReceipt, evidence.authorizedBatch);
    } catch {
      return { kind: "unknown", reservation, submission };
    }
    if (
      verification.receipt.amount !== BigInt(amount)
      || !constantTimeHex(verification.receipt.asset, request.asset)
      || !constantTimeHex(verification.receipt.to, request.recipient)
    ) {
      return { kind: "unknown", reservation, submission };
    }
    const receiptDigest = toHex(verification.receiptDigest);
    let committed: BudgetReservation;
    try {
      committed = validateBudgetReservation(
        await this.#budgets.commit({
          reservationId: reservation.reservationId,
          requestDigest,
          amount,
          asset: request.asset,
          receiptDigest,
        }),
        budget,
        reservation.reservationId,
      );
    } catch {
      return { kind: "unknown", reservation, submission };
    }
    if (committed.state !== "committed" || committed.receiptDigest !== receiptDigest) {
      throw new AgentMiddlewareError("budget-conflict");
    }
    return { kind: "verified", submission, verification, reservation: committed };
  }

  async #sdkFailure(
    error: unknown,
    reservation: BudgetReservation,
    budget: BudgetFacts,
    mayHaveExecuted: boolean,
  ): Promise<AgentSpendResult> {
    if (!(error instanceof PlatformSdkError)) {
      return { kind: "unknown", reservation };
    }
    if (
      error.retry === "unknown-outcome"
      || error.code === "unknown-outcome"
      || (mayHaveExecuted && POST_SUBMIT_UNCERTAIN_CODES.has(error.code))
    ) {
      return { kind: "unknown", reservation };
    }
    let refusal: AgentRefusal;
    try {
      refusal = sdkRefusal(error);
    } catch {
      return { kind: "unknown", reservation };
    }
    if (refusal.retry === "safe" || refusal.retry === "after") {
      return { kind: "refused", reservation, ...refusal };
    }
    try {
      const released = await this.#release(reservation, budget, refusal);
      return { kind: "refused", reservation: released, ...refusal };
    } catch {
      return { kind: "unknown", reservation };
    }
  }

  async #release(
    reservation: BudgetReservation,
    budget: BudgetFacts,
    refusal: AgentRefusal,
  ): Promise<ReleasedBudgetReservation> {
    if (reservation.state === "released") {
      if (!sameRefusal(reservation.refusal, refusal)) {
        throw new AgentMiddlewareError("budget-conflict");
      }
      return reservation;
    }
    if (reservation.state === "committed") {
      throw new AgentMiddlewareError("budget-conflict");
    }
    const released = validateBudgetReservation(
      await this.#budgets.release({
        reservationId: reservation.reservationId,
        requestDigest: budget.requestDigest,
        amount: budget.amount,
        asset: budget.asset,
        refusal,
      }),
      budget,
      reservation.reservationId,
    );
    if (released.state !== "released" || !sameRefusal(released.refusal, refusal)) {
      throw new AgentMiddlewareError("budget-conflict");
    }
    return released;
  }

  async #approvalHold(tenant: string, prepared: PreparedActivity): Promise<ApprovalHold | undefined> {
    const response = await this.#client.agent<
      { readonly tenant: string; readonly cursor: null; readonly page_limit: number },
      unknown
    >("approval.list", { tenant, cursor: null, page_limit: 100 });
    if (response === null || typeof response !== "object" || Array.isArray(response)) return undefined;
    const approvals = (response as Record<string, unknown>)["approvals"];
    if (!Array.isArray(approvals)) return undefined;
    const digest = disclosureDigest(prepared);
    for (const candidate of approvals) {
      if (candidate === null || typeof candidate !== "object" || Array.isArray(candidate)) continue;
      const object = candidate as Record<string, unknown>;
      if (
        object["state"] === "Held"
        && normalizeDigest(object["canonical_bytes_digest"]) === digest
        && typeof object["approval_id"] === "string"
      ) {
        return {
          approvalId: object["approval_id"],
          state: "Held",
          canonicalBytesDigest: digest,
          enforcement: "daemon_enforced",
        };
      }
    }
    return undefined;
  }
}

export type AgentMiddlewareErrorCode =
  | "invalid-request"
  | "invalid-signature"
  | "idempotency-conflict"
  | "budget-conflict"
  | "verification-failure"
  | "decode-failure";

export class AgentMiddlewareError extends Error {
  public constructor(public readonly code: AgentMiddlewareErrorCode) {
    super(code);
    this.name = "AgentMiddlewareError";
  }
}

export function platform_mw_agent(): "budget-aware-receipt-verified-agent" {
  return "budget-aware-receipt-verified-agent";
}

interface BudgetFacts {
  readonly requestDigest: string;
  readonly amount: string;
  readonly asset: string;
}

function validateBudgetReservation(
  reservation: BudgetReservation,
  expected: BudgetFacts,
  reservationId?: string,
): BudgetReservation {
  if (
    reservation === null
    || typeof reservation !== "object"
    || Array.isArray(reservation)
    || typeof reservation.reservationId !== "string"
    || typeof reservation.requestDigest !== "string"
    || typeof reservation.amount !== "string"
    || typeof reservation.asset !== "string"
    || !(new Set(["reserved", "held", "committed", "released"])).has((reservation as { readonly state: string }).state)
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
  if (
    reservation.reservationId.length === 0
    || reservation.reservationId.length > 512
    || reservation.reservationId.includes("\0")
    || (reservationId !== undefined && reservation.reservationId !== reservationId)
    || reservation.requestDigest !== expected.requestDigest
    || reservation.amount !== expected.amount
    || reservation.asset !== expected.asset
    || !/^[0-9a-f]{64}$/u.test(reservation.requestDigest)
    || !/^[0-9a-f]{64}$/u.test(reservation.asset)
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
  try {
    protocolAmount(reservation.amount);
  } catch {
    throw new AgentMiddlewareError("budget-conflict");
  }
  if (reservation.state === "held") {
    if (
      typeof reservation.approvalId !== "string"
      || typeof reservation.canonicalBytesDigest !== "string"
      || reservation.approvalId.length === 0
      || reservation.approvalId.length > 512
      || reservation.approvalId.includes("\0")
      || !/^[0-9a-f]{64}$/u.test(reservation.canonicalBytesDigest)
    ) {
      throw new AgentMiddlewareError("budget-conflict");
    }
  } else if (reservation.state === "committed") {
    if (typeof reservation.receiptDigest !== "string" || !/^[0-9a-f]{64}$/u.test(reservation.receiptDigest)) {
      throw new AgentMiddlewareError("budget-conflict");
    }
  } else if (reservation.state === "released") {
    validateRefusal(reservation.refusal);
  }
  return reservation;
}

function sdkRefusal(error: PlatformSdkError): AgentRefusal {
  if (error.retry === "unknown-outcome" || error.code === "unknown-outcome") {
    throw new AgentMiddlewareError("budget-conflict");
  }
  const refusal: AgentRefusal = {
    code: error.code,
    retry: error.retry,
    ...(error.retryAfterMs === undefined ? {} : { retryAfterMs: error.retryAfterMs }),
    ...(error.protocolResultCode === undefined ? {} : { protocolResultCode: error.protocolResultCode }),
  };
  validateRefusal(refusal);
  return refusal;
}

function validateRefusal(refusal: AgentRefusal): void {
  if (
    refusal === null
    || typeof refusal !== "object"
    || Array.isArray(refusal)
    || !(SDK_ERROR_CODES as readonly string[]).includes(refusal.code)
    || refusal.code === "unknown-outcome"
    || !(new Set(["never", "safe", "after"])).has((refusal as { readonly retry: string }).retry)
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
  if (
    (refusal.retry === "after" && refusal.retryAfterMs === undefined)
    || (refusal.retry !== "after" && refusal.retryAfterMs !== undefined)
    || (refusal.retryAfterMs !== undefined
      && (!Number.isSafeInteger(refusal.retryAfterMs) || refusal.retryAfterMs < 0))
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
  if (
    refusal.protocolResultCode !== undefined
    && (!Number.isSafeInteger(refusal.protocolResultCode)
      || refusal.protocolResultCode < -2_147_483_648
      || refusal.protocolResultCode > 2_147_483_647)
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
  if (
    refusal.submissionState !== undefined
    && refusal.submissionState !== "Failed"
    && refusal.submissionState !== "Expired"
  ) {
    throw new AgentMiddlewareError("budget-conflict");
  }
}

function sameRefusal(left: AgentRefusal, right: AgentRefusal): boolean {
  return left.code === right.code
    && left.retry === right.retry
    && left.retryAfterMs === right.retryAfterMs
    && left.protocolResultCode === right.protocolResultCode
    && left.submissionState === right.submissionState;
}

function validateSpend(request: AgentSpendRequest): void {
  for (const value of [request.tenant, request.actor, request.authority, request.idempotencyKey]) {
    if (value.length === 0 || value.length > 512 || value.includes("\0")) {
      throw new AgentMiddlewareError("invalid-request");
    }
  }
  protocolAmount(request.amount);
  protocolAmount(request.feeLimit);
  if (!/^(0|[1-9][0-9]*)$/u.test(request.accountSequence)
    || !/^(0|[1-9][0-9]*)$/u.test(request.timestampBound)
    || !/^[0-9a-f]{64}$/u.test(request.payloadHash)
    || !/^[0-9a-f]{64}$/u.test(request.asset)
    || !/^[0-9a-f]{64}$/u.test(request.recipient)
    || request.payloadBase64.length === 0
    || request.payloadBase64.length > 1_398_104
    || !isCanonicalBase64(request.payloadBase64)) {
    throw new AgentMiddlewareError("invalid-request");
  }
}

function parsePrepared(value: unknown): PreparedActivity {
  const object = record(value);
  const prepared = {
    preparation_ref: text(object["preparation_ref"], 512),
    unsigned_canonical_bytes: text(object["unsigned_canonical_bytes"], 1_398_104),
    signing_preimage: text(object["signing_preimage"], 1_398_104),
    disclosure: record(object["disclosure"]),
    expiry: text(object["expiry"], 64),
  };
  return prepared;
}

function parseSubmission(value: unknown): Submission {
  const object = record(value);
  return {
    submission_ref: text(object["submission_ref"], 512),
    state: typeof object["state"] === "string" ? object["state"] : record(object["state"]),
    ...(Array.isArray(object["evidence"]) ? { evidence: object["evidence"] } : {}),
    ...(typeof object["verification_level"] === "number" ? { verification_level: object["verification_level"] } : {}),
    ...(Array.isArray(object["transitions"]) ? { transitions: object["transitions"] } : {}),
  };
}

function submissionState(submission: Submission): "Pending" | "Unknown" | "Executed" | "Failed" | "Expired" {
  const state = submission.state;
  if (typeof state === "string") {
    if (["Prepared", "Signed", "Queued", "Submitted", "Acknowledged"].includes(state)) return "Pending";
    if (state === "Unknown" || state === "Executed" || state === "Failed" || state === "Expired") return state;
  } else {
    for (const candidate of ["Unknown", "Executed", "Failed", "Expired", "Pending"] as const) {
      if (candidate in state) return candidate;
      if (state["kind"] === candidate) return candidate;
    }
  }
  throw new AgentMiddlewareError("decode-failure");
}

function executedReceiptRef(submission: Submission): string | undefined {
  if (typeof submission.state === "object") {
    const executed = submission.state["Executed"];
    if (executed !== null && typeof executed === "object" && !Array.isArray(executed)) {
      const receipt = (executed as Record<string, unknown>)["receiptRef"];
      if (typeof receipt === "string") return receipt;
    }
    if (submission.state["kind"] === "Executed" && typeof submission.state["receiptRef"] === "string") {
      return submission.state["receiptRef"];
    }
  }
  for (const evidence of submission.evidence ?? []) {
    if (evidence !== null && typeof evidence === "object" && !Array.isArray(evidence)) {
      const object = evidence as Record<string, unknown>;
      if (object["class"] === "layerx-receipt" && typeof object["reference"] === "string") {
        return object["reference"];
      }
    }
  }
  return undefined;
}

function disclosureDigest(prepared: PreparedActivity): string {
  const digest = prepared.disclosure["canonical_digest"] ?? prepared.disclosure["canonicalDigest"];
  const normalized = normalizeDigest(digest);
  if (normalized === undefined) {
    throw new AgentMiddlewareError("decode-failure");
  }
  return normalized;
}

async function digestSpend(request: AgentSpendRequest): Promise<string> {
  const canonical = JSON.stringify({
    tenant: request.tenant,
    actor: request.actor,
    authority: request.authority,
    accountSequence: request.accountSequence,
    timestampBound: request.timestampBound,
    idempotencyKey: request.idempotencyKey,
    feeLimit: request.feeLimit,
    payloadHash: request.payloadHash,
    asset: request.asset,
    amount: request.amount,
    recipient: request.recipient,
  });
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonical)));
  return toHex(digest);
}

async function payloadHashMatches(payloadBase64: string, expected: string): Promise<boolean> {
  const binary = globalThis.atob(payloadBase64);
  const payload = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  const digest = new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", payload));
  return constantTimeHex(digest, expected);
}

function constantTimeHex(actual: Uint8Array, expected: string): boolean {
  if (!/^[0-9a-f]{64}$/u.test(expected) || actual.length !== 32) return false;
  let difference = 0;
  for (let index = 0; index < actual.length; index += 1) {
    const expectedByte = Number.parseInt(expected.slice(index * 2, index * 2 + 2), 16);
    difference |= (actual[index] ?? 0) ^ expectedByte;
  }
  return difference === 0;
}

function record(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new AgentMiddlewareError("decode-failure");
  }
  return value as Readonly<Record<string, unknown>>;
}

function normalizeDigest(value: unknown): string | undefined {
  if (typeof value !== "string") return undefined;
  const digest = value.startsWith("sha256:") ? value.slice(7) : value;
  return /^[0-9a-f]{64}$/u.test(digest) ? digest : undefined;
}

function isCanonicalBase64(value: string): boolean {
  if (value.length % 4 !== 0 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    return false;
  }
  return true;
}

function text(value: unknown, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum || value.includes("\0")) {
    throw new AgentMiddlewareError("decode-failure");
  }
  return value;
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}
