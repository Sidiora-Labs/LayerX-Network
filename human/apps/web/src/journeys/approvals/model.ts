import { copyEntry, human_copy_catalog } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import type {
  ActivityEntry,
  ActivityPage,
  ApiError,
  ApprovalDecision,
  ApprovalDetail,
  ApprovalState,
  ApprovalSummary,
  ErrorCode,
  JourneyState,
  Money,
  OperationDigest,
  StepUpEvidence,
  Timestamp,
  VerificationLevel,
  VerifiedMoney,
} from "../../api/index.ts";
import { formatExplicitCurrencyAmount } from "../../kit/a11y.ts";
import { protocolAmount, type ProtocolAmount, type StatusKey } from "../../kit/model.ts";

export const APPROVALS_ROUTE = "/app/approvals";
export const MONEY_LOCALE = "en-US";

const DIGEST_PREFIX = "opd_";
const EVIDENCE_PREFIX = "evd_";
const CHALLENGE_PREFIX = "chg_";

export function approvalRoute(approvalId: string): string {
  return `${APPROVALS_ROUTE}/${encodeURIComponent(approvalId)}`;
}

export function activityRoute(entryId: string): string {
  return `/app/activity/${encodeURIComponent(entryId)}`;
}

export function protocolMoney(money: Money): Readonly<{ value: ProtocolAmount; currency: string }> {
  if (
    money.amount > BigInt(Number.MAX_SAFE_INTEGER)
    || money.amount < BigInt(Number.MIN_SAFE_INTEGER)
  ) {
    throw new RangeError("Money amounts must stay within the safe integer range");
  }
  const value = Number(money.amount);
  return Object.freeze({ value: protocolAmount(value), currency: money.currency });
}

export function moneyLabel(money: Money): string {
  const amount = protocolMoney(money);
  return formatExplicitCurrencyAmount(amount.value, {
    currency: amount.currency,
    locale: MONEY_LOCALE,
    decimals: 0,
    signed: false,
  });
}

export interface ExpiryCountdown {
  readonly expired: boolean;
  readonly label: string;
}

export function expiryCountdown(expiresAt: Timestamp, at: Date): ExpiryCountdown {
  const remaining = Date.parse(expiresAt) - at.getTime();
  if (Number.isNaN(remaining) || remaining <= 0) {
    return Object.freeze({ expired: true, label: copyEntry("approval.state.expired").message });
  }
  if (remaining < 60_000) {
    return Object.freeze({ expired: false, label: copyEntry("approval.expiry.imminent").message });
  }
  return Object.freeze({
    expired: false,
    label: formatCopy("approval.expiry.remaining", { minutes: Math.floor(remaining / 60_000) }),
  });
}

export function requestedAtLabel(createdAt: Timestamp): string {
  return new Intl.DateTimeFormat(MONEY_LOCALE, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(createdAt),
  );
}

const VERIFICATION_COPY: Readonly<Record<VerificationLevel, string>> = Object.freeze({
  unverified: "verification.unverified",
  "receipt-verified": "verification.receipt_verified",
  "checkpoint-finalised": "verification.checkpoint_finalised",
  "paxeer-finalised": "verification.paxeer_finalised",
});

export function verificationLabel(level: VerificationLevel): string {
  return copyEntry(VERIFICATION_COPY[level]).message;
}

export function verifiedMoneyLabel(budget: VerifiedMoney): string {
  return moneyLabel(budget.money);
}

export type ApprovalTone = "destructive" | "neutral" | "success" | "warning";

const STATE_TONE: Readonly<Record<ApprovalState, ApprovalTone>> = Object.freeze({
  pending: "warning",
  approved: "success",
  rejected: "destructive",
  expired: "neutral",
  defective: "destructive",
});

export function approvalStateCopyKey(state: ApprovalState): string {
  return `approval.state.${state}`;
}

export interface ApprovalStatePresentation {
  readonly label: string;
  readonly tone: ApprovalTone;
}

export function approvalStatePresentation(
  state: ApprovalState,
  stateCopyKey: string = approvalStateCopyKey(state),
): ApprovalStatePresentation {
  const entry = human_copy_catalog().get(stateCopyKey) ?? copyEntry(approvalStateCopyKey(state));
  return Object.freeze({ label: entry.message, tone: STATE_TONE[state] });
}

export function catalogMessage(copyKey: string, fallbackKey: string): string {
  return (human_copy_catalog().get(copyKey) ?? copyEntry(fallbackKey)).message;
}

export interface ApprovalInboxItem {
  readonly id: string;
  readonly agentName: string;
  readonly counterparty: string;
  readonly amountLabel: string;
  readonly reason: string;
  readonly countdown: ExpiryCountdown;
  readonly state: ApprovalState;
  readonly statePresentation: ApprovalStatePresentation;
  readonly budgetAfterLabel: string;
  readonly budgetVerification: string;
  readonly href: string;
}

export function approvalInboxItem(summary: ApprovalSummary, at: Date): ApprovalInboxItem {
  const countdown = expiryCountdown(summary.expires_at, at);
  const state = summary.state === "pending" && countdown.expired ? "expired" : summary.state;
  return Object.freeze({
    id: summary.approval_id,
    agentName: summary.agent_name,
    counterparty: summary.counterparty,
    amountLabel: moneyLabel(summary.amount),
    reason: catalogMessage(summary.reason_copy_key, "approval.detail.held_reason"),
    countdown,
    state,
    statePresentation: approvalStatePresentation(state),
    budgetAfterLabel: verifiedMoneyLabel(summary.budget_remaining_after),
    budgetVerification: verificationLabel(summary.budget_remaining_after.verification),
    href: approvalRoute(summary.approval_id),
  });
}

export function pendingApprovalCount(summaries: readonly ApprovalSummary[]): number {
  return summaries.filter((summary) => summary.state === "pending").length;
}

export function approveConsequence(detail: ApprovalDetail): string {
  return formatCopy("approval.approve.consequence", {
    amount: moneyLabel(detail.facts.amount),
    counterparty: detail.facts.counterparty,
    fee: moneyLabel(detail.facts.fees),
  });
}

export function heldDigest(detail: ApprovalDetail): OperationDigest | undefined {
  const hold = detail.evidence.find((reference) => reference.class === "approval-hold");
  if (hold === undefined || !hold.evidence_id.startsWith(EVIDENCE_PREFIX)) {
    return undefined;
  }
  const opaqueId = hold.evidence_id.slice(EVIDENCE_PREFIX.length);
  return /^[0-9a-z]{26}$/u.test(opaqueId) ? `${DIGEST_PREFIX}${opaqueId}` : undefined;
}

export function stepUpEvidenceReference(evidence: StepUpEvidence): string {
  if (!evidence.challenge_id.startsWith(CHALLENGE_PREFIX)) {
    throw new TypeError("Step-up evidence must reference its ceremony challenge");
  }
  const opaqueId = evidence.challenge_id.slice(CHALLENGE_PREFIX.length);
  if (!/^[0-9a-z]{26}$/u.test(opaqueId)) {
    throw new TypeError("Step-up evidence must reference its ceremony challenge");
  }
  return `${EVIDENCE_PREFIX}${opaqueId}`;
}

export function canApprove(detail: ApprovalDetail, at: Date): boolean {
  return (
    detail.state === "pending" &&
    !expiryCountdown(detail.facts.expires_at, at).expired &&
    heldDigest(detail) !== undefined
  );
}

export type ApprovalOutcome =
  | Readonly<{ kind: "already-decided"; message: string }>
  | Readonly<{ kind: "converged"; detail: ApprovalDetail; message: string }>
  | Readonly<{ kind: "decided"; decision: ApprovalDecision; message: string }>
  | Readonly<{ kind: "defective"; message: string }>
  | Readonly<{ kind: "expired"; message: string }>
  | Readonly<{ kind: "still-checking"; message: string }>
  | Readonly<{ kind: "step-up-required"; message: string }>;

export function decidedOutcome(decision: ApprovalDecision): ApprovalOutcome {
  if (
    (decision.state !== "approved" && decision.state !== "rejected")
    || (decision.state === "rejected" && decision.money_moved)
  ) {
    throw new TypeError("The approval decision returned an invalid terminal outcome");
  }
  return Object.freeze({
    kind: "decided",
    decision,
    message: catalogMessage(decision.moved_copy_key, approvalStateCopyKey(decision.state)),
  });
}

export function defectiveOutcome(): ApprovalOutcome {
  return Object.freeze({
    kind: "defective",
    message: copyEntry("error.approval.hold-defective").message,
  });
}

export function convergedOutcome(detail: ApprovalDetail): ApprovalOutcome {
  if (detail.state === "expired") {
    return Object.freeze({
      kind: "expired",
      message: copyEntry("error.approval.hold-expired").message,
    });
  }
  if (detail.state === "defective") {
    return defectiveOutcome();
  }
  if (detail.state === "approved" || detail.state === "rejected") {
    return Object.freeze({
      kind: "converged",
      detail,
      message: catalogMessage(
        detail.state === "approved"
          ? "approval.approve.released"
          : "approval.reject.nothing-moved",
        detail.state_copy_key,
      ),
    });
  }
  return Object.freeze({
    kind: "still-checking",
    message: copyEntry("state.still_checking.body").message,
  });
}

type FailureOutcomePresentation = Readonly<{
  kind: "already-decided" | "defective" | "expired" | "step-up-required";
  fallbackKey: string;
}>;

const FAILURE_OUTCOMES: Readonly<Partial<Record<ErrorCode, FailureOutcomePresentation>>> = Object.freeze({
  "already-decided": Object.freeze({
    kind: "already-decided",
    fallbackKey: "error.approval.already-decided",
  }),
  "hold-expired": Object.freeze({
    kind: "expired",
    fallbackKey: "error.approval.hold-expired",
  }),
  "hold-defective": Object.freeze({
    kind: "defective",
    fallbackKey: "error.approval.hold-defective",
  }),
  "step-up-required": Object.freeze({
    kind: "step-up-required",
    fallbackKey: "error.step-up.required",
  }),
});

export function failureOutcome(detail: ApiError): ApprovalOutcome | undefined {
  const presentation = FAILURE_OUTCOMES[detail.code];
  if (presentation === undefined) {
    return undefined;
  }
  return Object.freeze({
    kind: presentation.kind,
    message: catalogMessage(detail.copy_key, presentation.fallbackKey),
  });
}

export function decisionKey(): string {
  return crypto.randomUUID().replaceAll("-", "");
}

export function releasedActivity(page: ActivityPage, approvalId: string): ActivityEntry | undefined {
  const entries = page.groups
    .flatMap((group) => group.entries)
    .filter((entry) => entry.approval_id === approvalId);
  return entries.find((entry) => entry.kind !== "approval") ?? entries[0];
}

const JOURNEY_STATUS: Readonly<Record<JourneyState, StatusKey>> = Object.freeze({
  "getting-ready": "getting_ready",
  sending: "sending",
  processing: "processing",
  done: "done",
  "done-finalised": "done_finalised",
  "still-checking": "still_checking",
  refused: "refused",
  "waiting-for-you": "waiting_for_you",
});

export function journeyStatus(state: JourneyState): StatusKey {
  return JOURNEY_STATUS[state];
}
