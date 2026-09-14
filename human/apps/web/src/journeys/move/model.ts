import { copyEntry } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import {
  HumanApiError,
  type Agent,
  type EvidenceRef,
  type HumanApiClient,
  type Journey,
  type JourneyState,
  type Money,
  type MoveQuote,
} from "../../api/index.ts";
import { ACTIVE_ACCOUNT_STORAGE_KEY } from "../../auth/session.ts";
import { formatExplicitCurrencyAmount } from "../../kit/a11y.ts";
import {
  protocolAmount,
  statusKeyFromCopyKey,
  type ProtocolAmount,
  type StatusKey,
} from "../../kit/model.ts";

export const HOME_ROUTE = "/app";
export const MOVE_ROUTE = "/app/move";
export const MOVE_DECISIONS = 3;
export const MOVE_STEP_IDS = ["who", "how-much", "review"] as const;
export const MOVE_CURRENCY = "LXP";
export const AMOUNT_LOCALE = "en";
export const OTHER_ACCOUNT_OPTION = "other-account";
export const ACCOUNT_STORAGE_KEY = ACTIVE_ACCOUNT_STORAGE_KEY;
export const PENDING_MOVE_STORAGE_KEY = "layerx.move.pending.v1";

const REFUSAL_CODES = [
  "refused-by-policy",
  "refused-by-budget",
  "refused-by-capability",
  "refused-by-protocol",
  "refused-by-limit",
] as const;

const RECEIPT_VERIFICATIONS = [
  "receipt-verified",
  "checkpoint-finalised",
  "paxeer-finalised",
] as const;

function copyMessageOr(key: string, fallbackKey: string): string {
  try {
    return copyEntry(key).message;
  } catch {
    return copyEntry(fallbackKey).message;
  }
}

export function storedAccount(storage: Pick<Storage, "getItem">): string | undefined {
  let value: string | null;
  try {
    value = storage.getItem(ACCOUNT_STORAGE_KEY);
  } catch {
    return undefined;
  }
  if (value === null || value.trim().length === 0) {
    return undefined;
  }
  return value.trim();
}

export interface PendingMoveAttempt {
  readonly source: string;
  readonly quoteId: string;
  readonly attemptKey: string;
}

function validPendingMove(value: unknown): value is PendingMoveAttempt {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const record = value as Readonly<Record<string, unknown>>;
  return (
    typeof record.source === "string" &&
    record.source.length > 0 &&
    record.source.length <= 256 &&
    typeof record.quoteId === "string" &&
    /^qte_[A-Za-z0-9_-]{8,128}$/u.test(record.quoteId) &&
    typeof record.attemptKey === "string" &&
    /^[a-f0-9]{32}$/u.test(record.attemptKey)
  );
}

export function pendingMoveAttempt(
  storage: Pick<Storage, "getItem" | "removeItem">,
  source: string | undefined,
): PendingMoveAttempt | undefined {
  try {
    const encoded = storage.getItem(PENDING_MOVE_STORAGE_KEY);
    if (encoded === null) {
      return undefined;
    }
    const attempt: unknown = JSON.parse(encoded);
    if (!validPendingMove(attempt) || attempt.source !== source) {
      clearPendingMove(storage);
      return undefined;
    }
    return attempt;
  } catch {
    clearPendingMove(storage);
    return undefined;
  }
}

export function persistPendingMove(
  storage: Pick<Storage, "setItem">,
  attempt: PendingMoveAttempt,
): void {
  try {
    storage.setItem(PENDING_MOVE_STORAGE_KEY, JSON.stringify(attempt));
  } catch {}
}

export function clearPendingMove(storage: Pick<Storage, "removeItem">): void {
  try {
    storage.removeItem(PENDING_MOVE_STORAGE_KEY);
  } catch {}
}

export function parseMoveAmount(input: string): bigint | undefined {
  const trimmed = input.trim();
  if (!/^\d{1,38}$/u.test(trimmed)) {
    return undefined;
  }
  const amount = BigInt(trimmed);
  return amount > 0n ? amount : undefined;
}

export function moneyAmountNumber(money: Money): ProtocolAmount {
  const value = Number(money.amount);
  if (!Number.isSafeInteger(value) || BigInt(value) !== money.amount) {
    throw new RangeError("Money amounts beyond the safe display range cannot be rendered");
  }
  return protocolAmount(value);
}

export function moneyWords(money: Money, locale: string = AMOUNT_LOCALE): string {
  return formatExplicitCurrencyAmount(moneyAmountNumber(money), {
    currency: money.currency,
    locale,
    decimals: 0,
    signed: false,
  });
}

export function timestampWords(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError("Timestamps must be RFC 3339 date-times");
  }
  const parts = new Intl.DateTimeFormat("en-GB", {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone: "UTC",
    timeZoneName: "short",
  }).formatToParts(date);
  const part = (type: Intl.DateTimeFormatPart["type"]): string =>
    parts.find((candidate) => candidate.type === type)?.value ?? "";
  return `${part("day")} ${part("month")} ${part("year")}, ${part("hour")}:${part("minute")} ${part("timeZoneName")}`;
}

export interface MoveStepCopy {
  readonly id: (typeof MOVE_STEP_IDS)[number];
  readonly label: string;
  readonly title: string;
  readonly description?: string;
}

export function moveSteps(): readonly [MoveStepCopy, MoveStepCopy, MoveStepCopy] {
  return [
    {
      id: "who",
      label: copyEntry("move.step.who.label").message,
      title: copyEntry("move.step.who.title").message,
      description: copyEntry("move.step.who.description").message,
    },
    {
      id: "how-much",
      label: copyEntry("move.step.amount.label").message,
      title: copyEntry("move.step.amount.title").message,
    },
    {
      id: "review",
      label: copyEntry("move.step.review.label").message,
      title: copyEntry("move.step.review.title").message,
    },
  ];
}

export interface MoveDestinationOption {
  readonly value: string;
  readonly label: string;
  readonly description: string;
}

export function destinationOptions(agents: readonly Agent[]): readonly MoveDestinationOption[] {
  return [
    ...agents
      .filter((agent) => agent.state === "active")
      .map((agent) => ({ value: agent.agent_id, label: agent.name, description: agent.purpose })),
    {
      value: OTHER_ACCOUNT_OPTION,
      label: copyEntry("move.destination.other").message,
      description: copyEntry("move.destination.other.hint").message,
    },
  ];
}

export type MoveFailure =
  | Readonly<{ kind: "refused"; code: string; message: string; moneyLeftMessage: string; trace: string }>
  | Readonly<{ kind: "quote-expired"; message: string }>
  | Readonly<{ kind: "error"; message: string }>
  | Readonly<{ kind: "offline"; message: string }>;

export function moveFailure(error: unknown): MoveFailure {
  if (error instanceof HumanApiError) {
    if (error.detail.code === "quote-expired") {
      return {
        kind: "quote-expired",
        message: copyMessageOr(error.detail.copy_key, "error.move.quote-expired"),
      };
    }
    if ((REFUSAL_CODES as readonly string[]).includes(error.detail.code)) {
      return {
        kind: "refused",
        code: error.detail.code,
        message: copyMessageOr(error.detail.copy_key, "status.refused"),
        moneyLeftMessage: copyEntry("move.result.money_stayed").message,
        trace: error.trace,
      };
    }
    return { kind: "error", message: copyEntry("state.error.body").message };
  }
  if (error instanceof TypeError) {
    return { kind: "offline", message: copyEntry("state.offline.body").message };
  }
  return { kind: "error", message: copyEntry("state.error.body").message };
}

export interface MoveQuoteInput {
  readonly source: string;
  readonly destination: string;
  readonly amount: bigint;
  readonly currency?: string;
}

export type MoveQuoteOutcome =
  | Readonly<{ kind: "quoted"; quote: MoveQuote }>
  | Readonly<{ kind: "failed"; failure: MoveFailure }>;

export async function requestMoveQuote(
  client: HumanApiClient,
  input: MoveQuoteInput,
): Promise<MoveQuoteOutcome> {
  try {
    const quote = await client.moveQuote({
      source: input.source,
      destination: input.destination,
      money: { amount: input.amount, currency: input.currency ?? MOVE_CURRENCY },
    });
    return { kind: "quoted", quote };
  } catch (error) {
    return { kind: "failed", failure: moveFailure(error) };
  }
}

export interface MoveReview {
  readonly headline: string;
  readonly amount: string;
  readonly feeEstimate: string;
  readonly feeCeiling: string;
  readonly fee: string;
  readonly arrival: string;
  readonly expires: string;
  readonly route: string;
  readonly irreversibility?: string;
}

export function moveReview(quote: MoveQuote): MoveReview {
  const feeEstimate = moneyWords(quote.fee_estimate);
  const feeCeiling = moneyWords(quote.fee_ceiling);
  const review: {
    headline: string;
    amount: string;
    feeEstimate: string;
    feeCeiling: string;
    fee: string;
    arrival: string;
    expires: string;
    route: string;
    irreversibility?: string;
  } = {
    headline: copyMessageOr(quote.description_copy_key, "move.title"),
    amount: moneyWords(quote.money),
    feeEstimate,
    feeCeiling,
    fee: formatCopy("move.review.fee", { estimate: feeEstimate, ceiling: feeCeiling }),
    arrival: formatCopy("move.review.arrival", { arrival: timestampWords(quote.arrival_estimate) }),
    expires: formatCopy("move.review.expires", { expires: timestampWords(quote.expires_at) }),
    route: copyEntry(`move.mechanism.${quote.mechanism}`).message,
  };
  if (quote.irreversibility_copy_key !== undefined) {
    review.irreversibility = copyMessageOr(
      quote.irreversibility_copy_key,
      "move.irreversible.send-to-account",
    );
  }
  return review;
}

export function quoteExpired(quote: MoveQuote, now: Date = new Date()): boolean {
  const expiresAt = new Date(quote.expires_at).getTime();
  return !Number.isFinite(expiresAt) || expiresAt <= now.getTime();
}

export function newMoveAttemptKey(): string {
  return globalThis.crypto.randomUUID().replaceAll("-", "");
}

export type MoveCommitOutcome =
  | Readonly<{ kind: "journey"; journey: Journey }>
  | Readonly<{ kind: "uncertain"; message: string }>
  | Readonly<{ kind: "failed"; failure: MoveFailure }>;

export async function commitMove(
  client: HumanApiClient,
  quoteId: string,
  attemptKey: string,
): Promise<MoveCommitOutcome> {
  try {
    const journey = await client.moveCommit({ quote_id: quoteId }, attemptKey);
    return { kind: "journey", journey };
  } catch (error) {
    if (!(error instanceof HumanApiError)) {
      return { kind: "uncertain", message: copyEntry("move.result.checking.body").message };
    }
    const failure = moveFailure(error);
    if (failure.kind === "quote-expired" || failure.kind === "refused") {
      return { kind: "failed", failure };
    }
    return { kind: "uncertain", message: copyEntry("move.result.checking.body").message };
  }
}

export function verifiedOutcomeEvidence(item: EvidenceRef): boolean {
  if (item.class === "layerx-receipt") {
    return (RECEIPT_VERIFICATIONS as readonly string[]).includes(item.verification);
  }
  if (item.class === "paxeer-finality") {
    return item.verification === "paxeer-finalised";
  }
  return false;
}

export function receiptBacked(journey: Journey): boolean {
  return (
    journey.stages.length > 0 &&
    journey.stages.every(
      (stage) =>
        (stage.state === "done" || stage.state === "done-finalised") &&
        stage.evidence.some(verifiedOutcomeEvidence),
    )
  );
}

export function receiptEvidence(journey: Journey): readonly EvidenceRef[] {
  const receipts = [...journey.evidence, ...journey.stages.flatMap((stage) => stage.evidence)]
    .filter(verifiedOutcomeEvidence);
  return [...new Map(receipts.map((receipt) => [receipt.evidence_id, receipt])).values()];
}

export interface MoveStagePresentation {
  readonly id: string;
  readonly label: string;
  readonly status: StatusKey;
}

export function moveStages(journey: Journey): readonly MoveStagePresentation[] {
  return journey.stages.map((stage) => ({
    id: stage.stage_id,
    label: copyMessageOr(stage.copy_key, "move.title"),
    status:
      (stage.state === "done" || stage.state === "done-finalised") &&
      !stage.evidence.some(verifiedOutcomeEvidence)
        ? "processing"
        : statusKeyFromCopyKey(stage.state),
  }));
}

export type MoveResult =
  | Readonly<{
      kind: "done";
      status: StatusKey;
      receiptMessage: string;
      receipts: readonly EvidenceRef[];
      stages: readonly MoveStagePresentation[];
      journeyId: string;
    }>
  | Readonly<{
      kind: "in-progress";
      status: StatusKey;
      stages: readonly MoveStagePresentation[];
      journeyId: string;
    }>
  | Readonly<{
      kind: "still-checking";
      status: StatusKey;
      locked: true;
      lockedReason: string;
      stages: readonly MoveStagePresentation[];
      journeyId: string;
    }>
  | Readonly<{
      kind: "refused";
      status: StatusKey;
      message: string;
      moneyLeftMessage: string;
      changePath?: string;
      stages: readonly MoveStagePresentation[];
      journeyId: string;
    }>
  | Readonly<{
      kind: "waiting";
      status: StatusKey;
      stages: readonly MoveStagePresentation[];
      journeyId: string;
    }>;

export function moveResult(journey: Journey): MoveResult {
  const journeyId = journey.journey_id;
  const stages = moveStages(journey);
  const state: JourneyState = journey.state;
  switch (state) {
    case "done":
    case "done-finalised": {
      if (!receiptBacked(journey)) {
        return { kind: "in-progress", status: "processing", stages, journeyId };
      }
      return {
        kind: "done",
        status: statusKeyFromCopyKey(state),
        receiptMessage: copyEntry("move.result.receipt").message,
        receipts: receiptEvidence(journey),
        stages,
        journeyId,
      };
    }
    case "still-checking":
      return {
        kind: "still-checking",
        status: "still_checking",
        locked: true,
        lockedReason: copyEntry("state.still_checking.locked").message,
        stages,
        journeyId,
      };
    case "refused": {
      const refusal = journey.refusal;
      return {
        kind: "refused",
        status: "refused",
        message:
          refusal === undefined
            ? copyEntry("status.refused").message
            : copyMessageOr(refusal.copy_key, "status.refused"),
        moneyLeftMessage: copyEntry(
          refusal !== undefined && refusal.money_left ? "move.result.money_left" : "move.result.money_stayed",
        ).message,
        ...(refusal?.change_path === undefined ? {} : { changePath: refusal.change_path }),
        stages,
        journeyId,
      };
    }
    case "waiting-for-you":
      return { kind: "waiting", status: "waiting_for_you", stages, journeyId };
    case "getting-ready":
    case "sending":
    case "processing":
      return { kind: "in-progress", status: statusKeyFromCopyKey(state), stages, journeyId };
  }
}

export function stillCheckingLookup(
  client: HumanApiClient,
  journeyId: string,
  onJourney: (journey: Journey) => void,
): () => Promise<"pending" | "resolved"> {
  return async () => {
    const journey = await client.journeyGet(journeyId);
    if (journey.state === "still-checking") {
      return "pending";
    }
    onJourney(journey);
    return "resolved";
  };
}

export interface MoveSummaryDraft {
  readonly destinationLabel?: string;
  readonly amount?: bigint;
  readonly currency: string;
}

export interface MoveSummaryItem {
  readonly label: string;
  readonly value: string;
  readonly privateFigure: boolean;
}

export function moveSummary(draft: MoveSummaryDraft, quote?: MoveQuote): readonly MoveSummaryItem[] {
  const items: MoveSummaryItem[] = [];
  if (draft.destinationLabel !== undefined) {
    items.push({
      label: copyEntry("move.summary.to").message,
      value: draft.destinationLabel,
      privateFigure: false,
    });
  }
  if (draft.amount !== undefined) {
    items.push({
      label: copyEntry("move.summary.amount").message,
      value: moneyWords({ amount: draft.amount, currency: draft.currency }),
      privateFigure: true,
    });
  }
  if (quote !== undefined) {
    const review = moveReview(quote);
    items.push({
      label: copyEntry("move.summary.fee").message,
      value: review.fee,
      privateFigure: true,
    });
    items.push({
      label: copyEntry("move.summary.route").message,
      value: copyEntry("move.route.automatic").message,
      privateFigure: false,
    });
  }
  return items;
}
