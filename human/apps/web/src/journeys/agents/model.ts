import { copyEntry, human_copy_catalog } from "../../../copy/catalog.ts";
import { formatCopy } from "../../../copy/format.ts";
import {
  HumanApiError,
  humanApi,
  type Agent,
  type AgentPage,
  type EvidenceRef,
  type HumanApiClient,
  type Journey,
  type JourneyKind,
  type JourneyStage,
  type JourneyState,
  type KeyChallenge,
  type LimitEnforcement,
  type Money,
  type MoveQuote,
  type VerificationLevel,
} from "../../api/index.ts";
import { type ConfirmationKind, type StatusKey } from "../../kit/model.ts";

export type {
  Agent,
  AgentPage,
  Journey,
  KeyChallenge,
  Money,
} from "../../api/index.ts";

export type AgentsShell = "mobile" | "desktop";
export type AgentsLayout = "stacked" | "master-detail";

export const AGENT_CURRENCY = "LXP";
export const AGENT_LOCALE = "en-GB";

export function agentsLayout(shell: AgentsShell): AgentsLayout {
  return shell === "mobile" ? "stacked" : "master-detail";
}

export const CREATION_DECISIONS = 3;

export interface CreationDraft {
  readonly name: string;
  readonly purpose: string;
  readonly limitInput: string;
  readonly currency: string;
}

export interface CreationStep {
  readonly id: "name" | "purpose" | "limit";
  readonly labelKey: string;
  readonly helpKey: string;
  readonly complete: boolean;
}

export function parseMonthlyLimit(input: string, currency: string): Money | undefined {
  const trimmed = input.trim();
  if (!/^\d{1,18}$/u.test(trimmed)) {
    return undefined;
  }
  const amount = BigInt(trimmed);
  return amount > 0n ? { amount, currency } : undefined;
}

export function creationSteps(draft: CreationDraft): readonly [CreationStep, CreationStep, CreationStep] {
  return [
    {
      id: "name",
      labelKey: "agent.create.name.label",
      helpKey: "agent.create.name.help",
      complete: draft.name.trim().length > 0,
    },
    {
      id: "purpose",
      labelKey: "agent.create.purpose.label",
      helpKey: "agent.create.purpose.help",
      complete: draft.purpose.trim().length > 0,
    },
    {
      id: "limit",
      labelKey: "agent.create.limit.label",
      helpKey: "agent.create.limit.help",
      complete: parseMonthlyLimit(draft.limitInput, draft.currency) !== undefined,
    },
  ];
}

export function creationReady(draft: CreationDraft): boolean {
  return creationSteps(draft).every((step) => step.complete);
}

const JOURNEY_STATUS: Readonly<Record<JourneyState, StatusKey>> = {
  "getting-ready": "getting_ready",
  sending: "sending",
  processing: "processing",
  done: "done",
  "done-finalised": "done_finalised",
  "still-checking": "still_checking",
  refused: "refused",
  "waiting-for-you": "waiting_for_you",
};

export function journeyStatusKey(state: JourneyState): StatusKey {
  return JOURNEY_STATUS[state];
}

export function catalogSentence(copyKey: string): string {
  return copyEntry(copyKey).message;
}

const RECEIPT_LEVELS: readonly VerificationLevel[] = [
  "receipt-verified",
  "checkpoint-finalised",
  "paxeer-finalised",
];

const VERIFICATION_COPY_KEYS: Readonly<Record<VerificationLevel, string>> = {
  unverified: "verification.unverified",
  "receipt-verified": "verification.receipt_verified",
  "checkpoint-finalised": "verification.checkpoint_finalised",
  "paxeer-finalised": "verification.paxeer_finalised",
};

function receiptBacked(reference: EvidenceRef): boolean {
  return (
    reference.class === "layerx-receipt"
    || reference.class === "checkpoint-proof"
    || reference.class === "paxeer-finality"
  ) && RECEIPT_LEVELS.includes(reference.verification);
}

export function evidenceReceiptVerified(evidence: readonly EvidenceRef[]): boolean {
  return evidence.some(receiptBacked);
}

export function stageReceiptVerified(stage: JourneyStage): boolean {
  if (stage.state !== "done" && stage.state !== "done-finalised") {
    return false;
  }
  return stage.evidence.some(receiptBacked);
}

export interface StageView {
  readonly stageId: string;
  readonly sentence: string;
  readonly statusKey: StatusKey;
  readonly receiptVerified: boolean;
}

export interface JourneyProgress {
  readonly journeyId: string;
  readonly kind: JourneyKind;
  readonly statusKey: StatusKey;
  readonly stages: readonly StageView[];
  readonly complete: boolean;
  readonly refusalSentence?: string;
}

export function journeyProgress(journey: Journey): JourneyProgress {
  const stages = journey.stages.map((stage) => ({
    stageId: stage.stage_id,
    sentence: catalogSentence(stage.copy_key),
    statusKey: journeyStatusKey(stage.state),
    receiptVerified: stageReceiptVerified(stage),
  }));
  const settled = journey.state === "done" || journey.state === "done-finalised";
  const progress: {
    journeyId: string;
    kind: JourneyKind;
    statusKey: StatusKey;
    stages: readonly StageView[];
    complete: boolean;
    refusalSentence?: string;
  } = {
    journeyId: journey.journey_id,
    kind: journey.kind,
    statusKey: journeyStatusKey(journey.state),
    stages,
    complete: settled && stages.length > 0 && stages.every((stage) => stage.receiptVerified),
  };
  if (journey.refusal !== undefined) {
    progress.refusalSentence = catalogSentence(journey.refusal.copy_key);
  }
  return progress;
}

export function creationHeadlineKey(progress: JourneyProgress): "agent.create.ready" | "agent.create.partial" {
  return progress.complete ? "agent.create.ready" : "agent.create.partial";
}

export type AgentTone = "neutral" | "accent" | "success" | "warning" | "destructive";

const AGENT_TONES: Readonly<Record<Agent["state"], AgentTone>> = {
  creating: "accent",
  active: "success",
  paused: "warning",
  archiving: "warning",
  archived: "neutral",
};

const AGENT_STATE_COPY_KEYS: Readonly<Record<Agent["state"], string>> = {
  creating: "agent.state.creating",
  active: "agent.state.active",
  paused: "agent.state.paused",
  archiving: "agent.state.archiving",
  archived: "agent.state.archived",
};

export interface AgentPresentation {
  readonly label: string;
  readonly tone: AgentTone;
  readonly readOnly: boolean;
  readonly stateVerified: boolean;
  readonly readOnlyKey?: "agent.archive.readonly";
}

export function agentStateVerified(agent: Agent): boolean {
  return evidenceReceiptVerified(agent.evidence);
}

export function agentPresentation(agent: Agent): AgentPresentation {
  const expectedCopyKey = AGENT_STATE_COPY_KEYS[agent.state];
  if (agent.state_copy_key !== expectedCopyKey) {
    throw new Error("Agent state copy did not match the declared lifecycle state");
  }
  const readOnly = agent.state === "archived" || agent.state === "archiving";
  const stateVerified = agentStateVerified(agent);
  const suppressUnverifiedActive = agent.state === "active" && !stateVerified;
  const presentation: {
    label: string;
    tone: AgentTone;
    readOnly: boolean;
    stateVerified: boolean;
    readOnlyKey?: "agent.archive.readonly";
  } = {
    label: suppressUnverifiedActive
      ? copyEntry("agent.state.creating").message
      : copyEntry(expectedCopyKey).message,
    tone: suppressUnverifiedActive ? "accent" : AGENT_TONES[agent.state],
    readOnly,
    stateVerified,
  };
  if (readOnly) {
    presentation.readOnlyKey = "agent.archive.readonly";
  }
  return presentation;
}

export function formatMoney(money: Money, locale: string): string {
  if (!/^[A-Z]{3}$/u.test(money.currency)) {
    throw new Error("Currency must be an explicit uppercase three-letter code");
  }
  const [canonicalLocale] = Intl.getCanonicalLocales(locale);
  if (canonicalLocale === undefined) {
    throw new Error("A locale is required for amount formatting");
  }
  return new Intl.NumberFormat(canonicalLocale, {
    style: "currency",
    currency: money.currency,
    currencyDisplay: "code",
    signDisplay: "auto",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(money.amount);
}

export interface SpendPresentation {
  readonly spent: string;
  readonly remaining: string;
  readonly limit: string;
  readonly summary: string;
  readonly percentSpent: number;
  readonly enforcement: LimitEnforcement;
  readonly protocolBacked: boolean;
  readonly enforcementSentence: string;
  readonly verificationSentence: string;
  readonly reconciliationSentence?: string;
}

export function spendPresentation(agent: Agent, locale: string): SpendPresentation {
  const spent = formatMoney(agent.spend.spent, locale);
  const limit = formatMoney(agent.limit.monthly, locale);
  const monthly = agent.limit.monthly.amount;
  const percent = monthly > 0n ? Number((agent.spend.spent.amount * 100n) / monthly) : 0;
  const enforcementCopyKey = agent.limit.enforcement === "protocol"
    ? "agent.limit.protocol-backed"
    : "agent.limit.app-enforced";
  if (agent.limit.enforcement_copy_key !== enforcementCopyKey) {
    throw new Error("Spend-limit copy did not match its declared enforcement authority");
  }
  if (
    agent.spend.reconciliation_copy_key !== undefined
    && agent.spend.reconciliation_copy_key !== "agent.spend.reconciled-to-protocol"
  ) {
    throw new Error("Spend reconciliation copy did not match the declared reconciliation state");
  }
  const presentation: {
    spent: string;
    remaining: string;
    limit: string;
    summary: string;
    percentSpent: number;
    enforcement: LimitEnforcement;
    protocolBacked: boolean;
    enforcementSentence: string;
    verificationSentence: string;
    reconciliationSentence?: string;
  } = {
    spent,
    remaining: formatMoney(agent.spend.remaining, locale),
    limit,
    summary: formatCopy("agent.spend.of_limit", { spent, limit }),
    percentSpent: Math.min(100, Math.max(0, percent)),
    enforcement: agent.limit.enforcement,
    protocolBacked: agent.limit.enforcement === "protocol",
    enforcementSentence: copyEntry(enforcementCopyKey).message,
    verificationSentence: catalogSentence(VERIFICATION_COPY_KEYS[agent.spend.verification]),
  };
  if (agent.spend.reconciliation_copy_key !== undefined) {
    presentation.reconciliationSentence = catalogSentence(agent.spend.reconciliation_copy_key);
  }
  return presentation;
}

export type AgentControlId =
  | "fund"
  | "reclaim"
  | "limit"
  | "pause"
  | "resume"
  | "rotate"
  | "recover"
  | "archive";

export interface AgentControl {
  readonly id: AgentControlId;
  readonly labelKey: string;
  readonly kind: ConfirmationKind;
  readonly consequenceKey: string;
  readonly enabled: boolean;
  readonly disabledReasonKey?: string;
  readonly typedExpected?: string;
  readonly dispositionFirst?: boolean;
}

export interface AgentControlContext {
  readonly ownerAccount?: string;
}

export function controlsFor(agent: Agent, context: AgentControlContext = {}): readonly AgentControl[] {
  if ((agent.state !== "active" && agent.state !== "paused") || !agentStateVerified(agent)) {
    return [];
  }
  const fund: {
    id: AgentControlId;
    labelKey: string;
    kind: ConfirmationKind;
    consequenceKey: string;
    enabled: boolean;
    disabledReasonKey?: string;
  } = {
    id: "fund",
    labelKey: "agent.control.fund",
    kind: "reversible",
    consequenceKey: "agent.fund.consequence",
    enabled: context.ownerAccount !== undefined,
  };
  if (context.ownerAccount === undefined) {
    fund.disabledReasonKey = "agent.fund.unavailable";
  }
  return [
    fund,
    {
      id: "reclaim",
      labelKey: "agent.control.reclaim",
      kind: "reversible",
      consequenceKey: "agent.reclaim.consequence",
      enabled: true,
    },
    {
      id: "limit",
      labelKey: "agent.control.limit",
      kind: "reversible",
      consequenceKey: "agent.limit.consequence",
      enabled: true,
    },
    agent.state === "paused"
      ? {
          id: "resume",
          labelKey: "agent.control.resume",
          kind: "reversible",
          consequenceKey: "agent.resume.consequence",
          enabled: true,
        }
      : {
          id: "pause",
          labelKey: "agent.control.pause",
          kind: "reversible",
          consequenceKey: "agent.pause.consequence",
          enabled: true,
        },
    {
      id: "rotate",
      labelKey: "agent.control.rotate",
      kind: "reversible",
      consequenceKey: "agent.keys.rotate.body",
      enabled: true,
    },
    {
      id: "recover",
      labelKey: "agent.control.recover",
      kind: "reversible",
      consequenceKey: "agent.keys.recover.body",
      enabled: true,
    },
    {
      id: "archive",
      labelKey: "agent.control.archive",
      kind: "irreversible",
      consequenceKey: "agent.archive.consequence",
      enabled: true,
      typedExpected: agent.name,
      dispositionFirst: true,
    },
  ];
}

export function formatPlainDelay(seconds: number): string {
  if (seconds >= 86400 && seconds % 86400 === 0) {
    return formatCopy("agent.keys.delay.days", { count: seconds / 86400 });
  }
  if (seconds >= 3600 && seconds % 3600 === 0) {
    return formatCopy("agent.keys.delay.hours", { count: seconds / 3600 });
  }
  return formatCopy("agent.keys.delay.minutes", { count: Math.max(1, Math.ceil(seconds / 60)) });
}

export function formatPlainTimestamp(timestamp: string, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    day: "2-digit",
    month: "short",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: "UTC",
    timeZoneName: "short",
  }).format(new Date(timestamp));
}

export interface KeyChallengePresentation {
  readonly delayText: string;
  readonly delaySentence: string;
  readonly readySentence: string;
  readonly startedSentence: string;
  readonly bodyKey: "agent.keys.rotate.body" | "agent.keys.recover.body";
}

export function keyChallengePresentation(challenge: KeyChallenge, locale: string): KeyChallengePresentation {
  if (!evidenceReceiptVerified(challenge.evidence)) {
    throw new Error(copyEntry("error.agent.challenge-unverified").message);
  }
  const expectedDelayCopyKey = challenge.kind === "rotate"
    ? "agent.keys.rotate-delay"
    : "agent.keys.recover-delay";
  if (challenge.delay_copy_key !== expectedDelayCopyKey) {
    throw new Error("Key challenge copy did not match its declared operation");
  }
  const delayText = formatPlainDelay(challenge.delay_seconds);
  const delaySentence = formatCopy(expectedDelayCopyKey, { delay: delayText });
  return {
    delayText,
    delaySentence,
    readySentence: formatCopy("agent.keys.ready_at", {
      readyAt: formatPlainTimestamp(challenge.ready_at, locale),
    }),
    startedSentence: copyEntry("agent.keys.started").message,
    bodyKey: challenge.kind === "rotate" ? "agent.keys.rotate.body" : "agent.keys.recover.body",
  };
}

export interface QuotePresentation {
  readonly description: string;
  readonly amount: string;
  readonly feeSentence: string;
  readonly arrivalSentence: string;
}

export function quotePresentation(quote: MoveQuote, locale: string): QuotePresentation {
  return {
    description: catalogSentence(quote.description_copy_key),
    amount: formatMoney(quote.money, locale),
    feeSentence: formatCopy("agent.quote.fee", { fee: formatMoney(quote.fee_ceiling, locale) }),
    arrivalSentence: formatCopy("agent.quote.arrives", {
      arrival: formatPlainTimestamp(quote.arrival_estimate, locale),
    }),
  };
}

export function apiErrorSentence(error: unknown): string {
  if (error instanceof HumanApiError) {
    const entry = human_copy_catalog().get(error.detail.copy_key);
    if (entry !== undefined) {
      return entry.message;
    }
  }
  return copyEntry("state.error.body").message;
}

export function apiErrorCode(error: unknown): string | undefined {
  return error instanceof HumanApiError ? error.detail.code : undefined;
}

export function mutationOutcomeUnknown(error: unknown): boolean {
  return !(error instanceof HumanApiError);
}

export interface AgentListItemView {
  readonly agentId: string;
  readonly name: string;
  readonly stateLabel: string;
  readonly tone: AgentTone;
  readonly spendSummary: string;
  readonly verificationSentence: string;
  readonly readOnly: boolean;
}

export function agentListItems(page: AgentPage, locale: string): readonly AgentListItemView[] {
  return page.agents.map((agent) => {
    const presentation = agentPresentation(agent);
    const spend = spendPresentation(agent, locale);
    return {
      agentId: agent.agent_id,
      name: agent.name,
      stateLabel: presentation.label,
      tone: presentation.tone,
      spendSummary: spend.summary,
      verificationSentence: presentation.stateVerified
        ? spend.verificationSentence
        : copyEntry("agent.state.unverified").message,
      readOnly: presentation.readOnly,
    };
  });
}

function newIdempotencyKey(): string {
  return crypto.randomUUID().replaceAll("-", "");
}

function mutationScope(operation: string, ...parts: readonly string[]): string {
  return JSON.stringify([operation, ...parts]);
}

export interface AgentsOptions {
  readonly client?: HumanApiClient;
  readonly idempotencyKey?: () => string;
}

export class Agents {
  readonly #client: HumanApiClient;
  readonly #idempotencyKey: () => string;
  readonly #pendingKeys = new Map<string, string>();

  constructor(options: AgentsOptions = {}) {
    this.#client = options.client ?? humanApi();
    this.#idempotencyKey = options.idempotencyKey ?? newIdempotencyKey;
  }

  overview(): Promise<AgentPage> {
    return this.#client.agentList();
  }

  agent(agentId: string): Promise<Agent> {
    return this.#client.agentGet(agentId);
  }

  journey(journeyId: string): Promise<Journey> {
    return this.#client.journeyGet(journeyId);
  }

  create(draft: CreationDraft): Promise<Journey> {
    const monthlyLimit = parseMonthlyLimit(draft.limitInput, draft.currency);
    if (monthlyLimit === undefined) {
      return Promise.reject(new Error(copyEntry("error.agent.limit-invalid").message));
    }
    const request = {
      name: draft.name.trim(),
      purpose: draft.purpose.trim(),
      monthly_limit: monthlyLimit,
    };
    return this.#mutate(
      mutationScope(
        "create",
        request.name,
        request.purpose,
        monthlyLimit.currency,
        monthlyLimit.amount.toString(10),
      ),
      (key) => this.#client.agentCreate(request, key),
    );
  }

  pause(agentId: string): Promise<Agent> {
    return this.#mutate(mutationScope("pause", agentId), (key) => this.#client.agentPause(agentId, key));
  }

  resume(agentId: string): Promise<Agent> {
    return this.#mutate(mutationScope("resume", agentId), (key) => this.#client.agentResume(agentId, key));
  }

  changeLimit(agentId: string, monthly: Money): Promise<Agent> {
    return this.#mutate(
      mutationScope("limit", agentId, monthly.currency, monthly.amount.toString(10)),
      (key) => this.#client.agentLimit(agentId, { monthly_limit: monthly }, key),
    );
  }

  reclaim(agentId: string, money: Money): Promise<Journey> {
    return this.#mutate(
      mutationScope("reclaim", agentId, money.currency, money.amount.toString(10)),
      (key) => this.#client.agentReclaim(agentId, { money }, key),
    );
  }

  fundQuote(ownerAccount: string, agentId: string, money: Money): Promise<MoveQuote> {
    return this.#client.moveQuote({ source: ownerAccount, destination: agentId, money });
  }

  fundCommit(quoteId: string): Promise<Journey> {
    return this.#mutate(
      mutationScope("fund", quoteId),
      (key) => this.#client.moveCommit({ quote_id: quoteId }, key),
    );
  }

  archive(agentId: string, confirmName: string): Promise<Journey> {
    return this.#mutate(
      mutationScope("archive", agentId, confirmName),
      (key) => this.#client.agentArchive(agentId, { confirm_name: confirmName }, key),
    );
  }

  rotate(agentId: string): Promise<KeyChallenge> {
    return this.#mutate(mutationScope("rotate", agentId), (key) => this.#client.agentRotate(agentId, key));
  }

  recover(agentId: string): Promise<KeyChallenge> {
    return this.#mutate(mutationScope("recover", agentId), (key) => this.#client.agentRecover(agentId, key));
  }

  async #mutate<T>(scope: string, run: (idempotencyKey: string) => Promise<T>): Promise<T> {
    const key = this.#pendingKeys.get(scope) ?? this.#idempotencyKey();
    this.#pendingKeys.set(scope, key);
    try {
      const result = await run(key);
      this.#pendingKeys.delete(scope);
      return result;
    } catch (error) {
      if (
        error instanceof HumanApiError
        && (error.detail.retry === "final" || error.detail.retry === "structural")
      ) {
        this.#pendingKeys.delete(scope);
      }
      throw error;
    }
  }
}
