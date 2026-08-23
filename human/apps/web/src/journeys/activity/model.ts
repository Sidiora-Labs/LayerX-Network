import { copyEntry, human_copy_catalog } from "../../../copy/catalog.ts";
import { formatCopy } from "../../../copy/format.ts";
import {
  HumanApiDecodeError,
  HumanApiError,
  activityEntryKindVariants,
  encodeActivityFilter,
  type Agent,
  type ActivityEntry,
  type ActivityEntryDetail,
  type ActivityEntryKind,
  type ActivityFilter,
  type ActivityGroup,
  type ActivityPage,
  type EvidenceClass,
  type EvidenceRef,
  type ExportArtefact,
  type HumanApiClient,
  type JourneyStage,
  type JourneyState,
  type Money,
  type MoneyDirection,
  type VerificationLevel,
} from "../../api/index.ts";
import { protocolAmount, statusPresentation, type ProtocolAmount, type StatusKey } from "../../kit/model.ts";

export const STATUS_KEY_BY_STATE: Readonly<Record<JourneyState, StatusKey>> = Object.freeze({
  "getting-ready": "getting_ready",
  sending: "sending",
  processing: "processing",
  done: "done",
  "done-finalised": "done_finalised",
  "still-checking": "still_checking",
  refused: "refused",
  "waiting-for-you": "waiting_for_you",
});

const KIND_TOKENS: Readonly<Record<ActivityEntryKind, string>> = Object.freeze({
  deposit: "deposit",
  withdrawal: "withdrawal",
  movement: "move",
  "agent-action": "agent",
  approval: "approval",
  "security-event": "security",
});

const KIND_LABEL_KEYS: Readonly<Record<ActivityEntryKind, string>> = Object.freeze({
  deposit: "activity.kind.deposit",
  withdrawal: "activity.kind.withdrawal",
  movement: "activity.kind.movement",
  "agent-action": "activity.kind.agent_action",
  approval: "activity.kind.approval",
  "security-event": "activity.kind.security_event",
});

const VERIFICATION_LABEL_KEYS: Readonly<Record<VerificationLevel, string>> = Object.freeze({
  unverified: "activity.verification.unverified",
  "receipt-verified": "activity.verification.receipt_verified",
  "checkpoint-finalised": "activity.verification.checkpoint_finalised",
  "paxeer-finalised": "activity.verification.paxeer_finalised",
});

const VERIFICATION_ORDER: readonly VerificationLevel[] = Object.freeze([
  "unverified",
  "receipt-verified",
  "checkpoint-finalised",
  "paxeer-finalised",
]);

export function entryStatusKey(state: JourneyState): StatusKey {
  return STATUS_KEY_BY_STATE[state];
}

export function entryStatusLabel(state: JourneyState): string {
  return statusPresentation(entryStatusKey(state)).label;
}

export function entryUnresolved(state: JourneyState): boolean {
  return state === "still-checking";
}

export function stillCheckingLockReason(): string {
  return copyEntry("state.still_checking.locked").message;
}

export function kindLabel(kind: ActivityEntryKind): string {
  return copyEntry(KIND_LABEL_KEYS[kind]).message;
}

export function plainSentence(
  entry: Readonly<Pick<ActivityEntry, "kind" | "state" | "summary_copy_key">>,
): string {
  const supplied = human_copy_catalog().get(entry.summary_copy_key);
  if (supplied !== undefined) {
    return supplied.message;
  }
  if (entry.state === "refused") {
    throw new Error("A refused activity is missing its money-disposition sentence");
  }
  const token = KIND_TOKENS[entry.kind];
  const suffix = entry.state === "done" || entry.state === "done-finalised" ? "summary" : "progress";
  return copyEntry(`activity.${token}.${suffix}`).message;
}

export interface TimelineStageView {
  readonly id: string;
  readonly label: string;
  readonly status: StatusKey;
  readonly evidence: readonly EvidenceRef[];
}

export function stageView(stage: JourneyStage): TimelineStageView {
  const catalogued = human_copy_catalog().get(stage.copy_key);
  return Object.freeze({
    id: stage.stage_id,
    label: catalogued === undefined ? entryStatusLabel(stage.state) : catalogued.message,
    status: entryStatusKey(stage.state),
    evidence: stage.evidence,
  });
}

export function toKitDirection(direction: MoneyDirection | undefined): "inbound" | "outbound" | "other" {
  if (direction === "in") {
    return "inbound";
  }
  if (direction === "out") {
    return "outbound";
  }
  return "other";
}

export function signedBaseUnits(
  money: Money | undefined,
  direction: MoneyDirection | undefined,
): ProtocolAmount {
  if (money === undefined) {
    return protocolAmount(0);
  }
  const value = Number(money.amount);
  if (!Number.isSafeInteger(value) || BigInt(value) !== money.amount) {
    throw new RangeError("Activity money exceeds the safe display range");
  }
  const magnitude = protocolAmount(value);
  return direction === "out" ? protocolAmount(magnitude * -1) : magnitude;
}

export function unsignedBaseUnits(money: Money): ProtocolAmount {
  return signedBaseUnits(money, "in");
}

export function monthLabel(month: string): string {
  if (!/^\d{4}-(0[1-9]|1[0-2])$/u.test(month)) {
    throw new RangeError("Activity month bands must use YYYY-MM");
  }
  const [year, monthNumber] = month.split("-");
  const date = new Date(Date.UTC(Number(year), Number(monthNumber) - 1, 1));
  return date.toLocaleDateString("en-US", { month: "long", year: "numeric", timeZone: "UTC" });
}

function datePart(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError("Activity timestamps must be RFC 3339 date-times");
  }
  const day = String(date.getUTCDate()).padStart(2, "0");
  const month = date.toLocaleDateString("en-US", { month: "short", timeZone: "UTC" });
  const year = String(date.getUTCFullYear());
  return `${day} ${month} ${year}`;
}

export function formatEntryDate(timestamp: string): string {
  return datePart(timestamp);
}

export function formatEntryTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError("Activity timestamps must be RFC 3339 date-times");
  }
  const hours = String(date.getUTCHours()).padStart(2, "0");
  const minutes = String(date.getUTCMinutes()).padStart(2, "0");
  return `${datePart(timestamp)}, ${hours}:${minutes} UTC`;
}

export function explorerPath(reference: EvidenceRef): string | undefined {
  if (reference.class === "layerx-receipt") {
    return `/explorer/receipts/${reference.evidence_id}`;
  }
  if (reference.class === "checkpoint-proof") {
    return `/explorer/checkpoints/${reference.evidence_id}`;
  }
  return undefined;
}

export function verificationLabel(level: VerificationLevel): string {
  return copyEntry(VERIFICATION_LABEL_KEYS[level]).message;
}

export function evidenceClassLabel(evidenceClass: EvidenceClass): string {
  return copyEntry(`activity.evidence.${evidenceClass}`).message;
}

export function entryVerification(evidence: readonly EvidenceRef[]): VerificationLevel {
  let best = 0;
  for (const reference of evidence) {
    const rank = VERIFICATION_ORDER.indexOf(reference.verification);
    if (rank > best) {
      best = rank;
    }
  }
  return VERIFICATION_ORDER[best] ?? "unverified";
}

export interface FeedRowView {
  id: string;
  title: string;
  subtitle: string;
  amount: ProtocolAmount;
  currency?: string;
  status: string;
  statusKey: StatusKey;
  date: Date;
}

export interface FeedGroupView {
  id: string;
  label: string;
  subtotalIn: ProtocolAmount;
  subtotalOut: ProtocolAmount;
  currency: string;
  items: FeedRowView[];
}

function rowView(entry: ActivityEntry): FeedRowView {
  const statusKey = entryStatusKey(entry.state);
  const row: FeedRowView = {
    id: entry.entry_id,
    title: kindLabel(entry.kind),
    subtitle: plainSentence(entry),
    amount: signedBaseUnits(entry.money, entry.direction),
    status: statusPresentation(statusKey).label,
    statusKey,
    date: activityDate(entry.occurred_at),
  };
  if (entry.money !== undefined) {
    row.currency = entry.money.currency;
  }
  return row;
}

function groupView(group: ActivityGroup): FeedGroupView {
  if (group.subtotal_in.currency !== group.subtotal_out.currency) {
    throw new RangeError("Activity group subtotals must share one currency");
  }
  return {
    id: group.month,
    label: monthLabel(group.month),
    subtotalIn: unsignedBaseUnits(group.subtotal_in),
    subtotalOut: unsignedBaseUnits(group.subtotal_out),
    currency: group.subtotal_in.currency,
    items: group.entries.map(rowView),
  };
}

export function activityDate(timestamp: string): Date {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    throw new RangeError("Activity timestamps must be RFC 3339 date-times");
  }
  return date;
}

export function feedGroups(page: ActivityPage): FeedGroupView[] {
  return page.groups.map(groupView);
}

export function feedIsEmpty(page: ActivityPage): boolean {
  return page.groups.every((group) => group.entries.length === 0);
}

export function filterEchoLines(
  filter: ActivityFilter,
  agentNames: ReadonlyMap<string, string> = new Map(),
): readonly string[] {
  const lines: string[] = [];
  if (filter.kinds !== undefined && filter.kinds.length > 0) {
    const labels = filter.kinds.map((kind) => kindLabel(kind).toLocaleLowerCase("en-US"));
    const list = new Intl.ListFormat("en-US", { style: "long", type: "conjunction" }).format(labels);
    lines.push(formatCopy("activity.feed.echo.kinds", { kinds: list }));
  }
  if (filter.agent_id !== undefined) {
    const agent = agentNames.get(filter.agent_id) ?? filter.agent_id;
    lines.push(formatCopy("activity.feed.echo.agent", { agent }));
  }
  if (filter.from !== undefined && filter.to !== undefined) {
    lines.push(formatCopy("activity.feed.echo.range", {
      from: formatEntryDate(filter.from),
      to: formatEntryDate(filter.to),
    }));
  }
  if (lines.length === 0) {
    lines.push(copyEntry("activity.feed.echo.all").message);
  }
  return lines;
}

export interface FeedFilterOption {
  value: string;
  label: string;
}

export interface FeedFilterDef {
  id: string;
  label: string;
  type: "options" | "date-range";
  options?: FeedFilterOption[];
}

export interface FeedDateRange {
  from: Date;
  to?: Date;
}

export type FeedFilterValues = Readonly<Record<string, string | FeedDateRange | undefined>>;

export function emptyFilterValues(): FeedFilterValues {
  return Object.freeze({ kind: "all", agent: "all", date: undefined });
}

export function feedFilterDefs(agents: readonly FeedFilterOption[]): FeedFilterDef[] {
  const kindOptions: FeedFilterOption[] = [
    { value: "all", label: copyEntry("activity.filter.all_types").message },
    ...activityEntryKindVariants.map((kind) => ({ value: kind, label: kindLabel(kind) })),
  ];
  const agentOptions: FeedFilterOption[] = [
    { value: "all", label: copyEntry("activity.filter.all_agents").message },
    ...agents,
  ];
  return [
    { id: "kind", label: copyEntry("activity.filter.kind").message, type: "options", options: kindOptions },
    { id: "agent", label: copyEntry("activity.filter.agent").message, type: "options", options: agentOptions },
    { id: "date", label: copyEntry("activity.filter.date").message, type: "date-range" },
  ];
}

function isEntryKind(value: string): value is ActivityEntryKind {
  return (activityEntryKindVariants as readonly string[]).includes(value);
}

export function toWireFilter(values: FeedFilterValues): ActivityFilter {
  const filter: ActivityFilter = {};
  const kind = values["kind"];
  if (typeof kind === "string" && kind !== "all" && isEntryKind(kind)) {
    filter.kinds = [kind];
  }
  const agent = values["agent"];
  if (typeof agent === "string" && agent !== "all" && agent.length > 0) {
    filter.agent_id = agent;
  }
  const range = values["date"];
  if (typeof range === "object") {
    const last = range.to ?? range.from;
    const from = new Date(Date.UTC(range.from.getFullYear(), range.from.getMonth(), range.from.getDate()));
    const to = new Date(Date.UTC(last.getFullYear(), last.getMonth(), last.getDate() + 1));
    filter.from = from.toISOString();
    filter.to = to.toISOString();
  }
  return filter;
}

export function sameFilterValues(first: FeedFilterValues, second: FeedFilterValues): boolean {
  const firstWire = JSON.stringify(encodeActivityFilter(toWireFilter(first)));
  const secondWire = JSON.stringify(encodeActivityFilter(toWireFilter(second)));
  return firstWire === secondWire;
}

export function mergePages(current: ActivityPage, incoming: ActivityPage): ActivityPage {
  const groups: ActivityGroup[] = current.groups.map((group) => ({
    month: group.month,
    subtotal_in: group.subtotal_in,
    subtotal_out: group.subtotal_out,
    entries: [...group.entries],
  }));
  const byMonth = new Map(groups.map((group) => [group.month, group]));
  const seen = new Set<string>();
  for (const group of groups) {
    for (const entry of group.entries) {
      seen.add(entry.entry_id);
    }
  }
  for (const group of incoming.groups) {
    const fresh = group.entries.filter((entry) => !seen.has(entry.entry_id));
    for (const entry of fresh) {
      seen.add(entry.entry_id);
    }
    const existing = byMonth.get(group.month);
    if (existing === undefined) {
      const appended: ActivityGroup = {
        month: group.month,
        subtotal_in: group.subtotal_in,
        subtotal_out: group.subtotal_out,
        entries: fresh,
      };
      groups.push(appended);
      byMonth.set(group.month, appended);
    } else {
      existing.subtotal_in = group.subtotal_in;
      existing.subtotal_out = group.subtotal_out;
      existing.entries.push(...fresh);
    }
  }
  return { groups, next_cursor: incoming.next_cursor, filter: incoming.filter };
}

export function detailUnresolved(detail: Readonly<Pick<ActivityEntryDetail, "state">>): boolean {
  return entryUnresolved(detail.state);
}

const RECEIPT_LEVELS: readonly VerificationLevel[] = Object.freeze([
  "receipt-verified",
  "checkpoint-finalised",
  "paxeer-finalised",
]);

export function receiptBacked(reference: EvidenceRef): boolean {
  return reference.class === "layerx-receipt" && RECEIPT_LEVELS.includes(reference.verification);
}

export function validatedDetail(detail: ActivityEntryDetail): ActivityEntryDetail {
  const completion = detail.state === "done" || detail.state === "done-finalised";
  const hasReceipt = detail.evidence.some(receiptBacked)
    || detail.stages.some((stage) => stage.evidence.some(receiptBacked));
  if (completion && !hasReceipt) {
    throw new Error("A completed activity is missing a verified receipt");
  }
  if ((detail.money !== undefined || detail.fees !== undefined) && !hasReceipt) {
    throw new Error("Activity money facts are missing their verified receipt");
  }
  signedBaseUnits(detail.money, detail.direction);
  if (detail.fees !== undefined) {
    unsignedBaseUnits(detail.fees);
  }
  activityDate(detail.occurred_at);
  return detail;
}

export function agentFilterOptions(
  agents: readonly Pick<Agent, "agent_id" | "name">[],
): readonly FeedFilterOption[] {
  return agents
    .map((agent) => ({ value: agent.agent_id, label: agent.name }))
    .sort((left, right) => left.label.localeCompare(right.label, "en-US"));
}

export async function loadActivity(
  client: HumanApiClient,
  filter: ActivityFilter = {},
): Promise<Readonly<{ page: ActivityPage; agents: readonly Agent[] }>> {
  const [page, agentPage] = await Promise.all([
    client.activityQuery({ filter, page_limit: 50 }),
    client.agentList(),
  ]);
  return { page, agents: agentPage.agents };
}

export type ActivityFailure = Readonly<{
  kind: "offline" | "service";
  message: string;
  code: string;
  trace?: string;
  retriable: boolean;
}>;

export function activityFailure(error: unknown): ActivityFailure {
  if (error instanceof HumanApiError) {
    const catalogued = human_copy_catalog().get(error.detail.copy_key);
    return {
      kind: "service",
      message: catalogued?.message ?? copyEntry("state.error.body").message,
      code: error.detail.code,
      trace: error.trace,
      retriable: error.detail.retry === "retriable" || error.detail.retry === "retriable-after",
    };
  }
  if (error instanceof HumanApiDecodeError || error instanceof RangeError || error instanceof Error) {
    if (error instanceof TypeError) {
      return {
        kind: "offline",
        message: copyEntry("state.offline.body").message,
        code: "connection-unavailable",
        retriable: true,
      };
    }
    return {
      kind: "service",
      message: copyEntry("state.error.body").message,
      code: error instanceof HumanApiDecodeError ? "invalid-service-response" : "activity-presentation-invalid",
      retriable: true,
    };
  }
  return {
    kind: "service",
    message: copyEntry("state.error.body").message,
    code: "activity-unavailable",
    retriable: true,
  };
}

export function newExportKey(): string {
  return globalThis.crypto.randomUUID();
}

export function safeExportArtefact(artefact: ExportArtefact): ExportArtefact {
  if (!/^\/v1\/activity\/exports\/exp_[A-Za-z0-9_-]{8,128}\/download$/u.test(artefact.download_path)) {
    throw new Error("The export service returned an unsafe download path");
  }
  return artefact;
}

export function allEntryIds(page: ActivityPage): readonly string[] {
  return page.groups.flatMap((group) => group.entries.map((entry) => entry.entry_id));
}
