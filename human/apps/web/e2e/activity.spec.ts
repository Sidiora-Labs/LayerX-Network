import assert from "node:assert/strict";
import test from "node:test";

import type { ActivityEntry, ActivityEntryDetail, ActivityGroup, ActivityPage, ExportArtefact } from "../src/api/generated/index.ts";
import { humanApi } from "../src/api/index.ts";
import {
  activityFailure,
  agentFilterOptions,
  detailUnresolved,
  emptyFilterValues,
  entryStatusKey,
  entryVerification,
  evidenceClassLabel,
  explorerPath,
  feedFilterDefs,
  feedGroups,
  filterEchoLines,
  formatEntryDate,
  formatEntryTimestamp,
  kindLabel,
  loadActivity,
  mergePages,
  monthLabel,
  newExportKey,
  plainSentence,
  receiptBacked,
  safeExportArtefact,
  sameFilterValues,
  signedBaseUnits,
  stageView,
  stillCheckingLockReason,
  toKitDirection,
  toWireFilter,
  unsignedBaseUnits,
  validatedDetail,
  verificationLabel,
  type ActivityFailure,
  type FeedDateRange,
  type FeedFilterValues,
} from "../src/journeys/activity/model.ts";

const mockActivityPage: ActivityPage = Object.freeze({
  groups: [
    {
      month: "2026-08",
      subtotal_in: { amount: 50000n, currency: "USD" },
      subtotal_out: { amount: 25000n, currency: "USD" },
      entries: [
        {
          entry_id: "ent_deposit_001",
          kind: "deposit",
          state: "done",
          summary_copy_key: "activity.deposit.summary",
          state_copy_key: "status.done",
          occurred_at: "2026-08-15T10:30:00Z",
          money: { amount: 50000n, currency: "USD" },
          direction: "in" as const,
        },
        {
          entry_id: "ent_withdrawal_001",
          kind: "withdrawal",
          state: "processing",
          summary_copy_key: "activity.withdrawal.progress",
          state_copy_key: "status.processing",
          occurred_at: "2026-08-10T14:20:00Z",
          money: { amount: 25000n, currency: "USD" },
          direction: "out" as const,
        },
      ],
    },
    {
      month: "2026-07",
      subtotal_in: { amount: 100000n, currency: "USD" },
      subtotal_out: { amount: 0n, currency: "USD" },
      entries: [
        {
          entry_id: "ent_deposit_002",
          kind: "deposit",
          state: "done-finalised",
          summary_copy_key: "activity.deposit.summary",
          state_copy_key: "status.done_finalised",
          occurred_at: "2026-07-25T09:15:00Z",
          money: { amount: 100000n, currency: "USD" },
          direction: "in" as const,
        },
      ],
    },
  ],
  next_cursor: "cursor_next_page",
  filter: {},
});

const mockActivityDetail: ActivityEntryDetail = Object.freeze({
  entry_id: "ent_deposit_001",
  kind: "deposit",
  state: "done",
  summary_copy_key: "activity.deposit.summary",
  state_copy_key: "status.done",
  occurred_at: "2026-08-15T10:30:00Z",
  money: { amount: 50000n, currency: "USD" },
  direction: "in" as const,
  fees: { amount: 250n, currency: "USD" },
  evidence: [
    {
      class: "layerx-receipt",
      evidence_id: "rcpt_001",
      verification: "checkpoint-finalised",
    },
  ],
  stages: [
    {
      stage_id: "stage_pending",
      copy_key: "status.processing",
      state: "processing",
      evidence: [],
    },
    {
      stage_id: "stage_complete",
      copy_key: "status.done",
      state: "done",
      evidence: [
        {
          class: "checkpoint-proof",
          evidence_id: "chkpt_001",
          verification: "checkpoint-finalised",
        },
      ],
    },
  ],
});

const mockUnresolvedDetail: ActivityEntryDetail = Object.freeze({
  entry_id: "ent_still_checking_001",
  kind: "withdrawal",
  state: "still-checking",
  summary_copy_key: "activity.withdrawal.progress",
  state_copy_key: "status.still_checking",
  occurred_at: "2026-08-20T16:45:00Z",
  money: undefined,
  direction: undefined,
  evidence: [],
  stages: [],
});

test("feedGroups transforms ActivityPage groups into FeedGroupView", () => {
  const groups = feedGroups(mockActivityPage);
  assert.equal(groups.length, 2);
  assert.equal(groups[0].id, "2026-08");
  assert.equal(groups[0].label, "August 2026");
  assert.equal(groups[0].items.length, 2);
  assert.equal(groups[0].items[0].id, "ent_deposit_001");
  assert.equal(groups[0].items[0].title, kindLabel("deposit"));
  assert.equal(groups[0].items[0].currency, "USD");
  assert.equal(groups[1].id, "2026-07");
  assert.equal(groups[1].label, "July 2026");
  assert.equal(groups[1].items.length, 1);
});

test("feedGroups calculates signed amounts with correct direction", () => {
  const groups = feedGroups(mockActivityPage);
  const depositAmount = signedBaseUnits(mockActivityPage.groups[0].entries[0].money, "in");
  const withdrawalAmount = signedBaseUnits(mockActivityPage.groups[0].entries[1].money, "out");
  assert.equal(depositAmount, 50000);
  assert.equal(withdrawalAmount, -25000);
});

test("monthLabel formats YYYY-MM as readable month and year", () => {
  assert.equal(monthLabel("2026-08"), "August 2026");
  assert.equal(monthLabel("2026-01"), "January 2026");
  assert.equal(monthLabel("2026-12"), "December 2026");
  assert.throws(() => monthLabel("2026-13"), /YYYY-MM/u);
  assert.throws(() => monthLabel("2026-8"), /YYYY-MM/u);
});

test("formatEntryDate formats RFC 3339 timestamps as date only", () => {
  assert.equal(formatEntryDate("2026-08-15T10:30:00Z"), "15 Aug 2026");
  assert.equal(formatEntryDate("2026-01-01T00:00:00Z"), "01 Jan 2026");
});

test("formatEntryTimestamp formats RFC 3339 timestamps with time in UTC", () => {
  assert.equal(formatEntryTimestamp("2026-08-15T10:30:00Z"), "15 Aug 2026, 10:30 UTC");
  assert.equal(formatEntryTimestamp("2026-01-01T23:59:00Z"), "01 Jan 2026, 23:59 UTC");
});

test("entryStatusKey maps journey states to status keys", () => {
  assert.equal(entryStatusKey("done"), "done");
  assert.equal(entryStatusKey("still-checking"), "still_checking");
  assert.equal(entryStatusKey("processing"), "processing");
  assert.equal(entryStatusKey("refused"), "refused");
});

test("kindLabel returns localized label for activity entry kinds", () => {
  const labels = ["deposit", "withdrawal", "movement", "agent-action", "approval", "security-event"] as const;
  for (const kind of labels) {
    const label = kindLabel(kind);
    assert.equal(typeof label, "string");
    assert.ok(label.length > 0);
  }
});

test("plainSentence returns summary copy for activity entries", () => {
  const entry = mockActivityPage.groups[0].entries[0];
  const sentence = plainSentence(entry);
  assert.equal(typeof sentence, "string");
  assert.ok(sentence.length > 0);
});

test("toKitDirection maps MoneyDirection to kit direction strings", () => {
  assert.equal(toKitDirection("in"), "inbound");
  assert.equal(toKitDirection("out"), "outbound");
  assert.equal(toKitDirection(undefined), "other");
});

test("signedBaseUnits converts Money to signed protocol amounts", () => {
  const inbound = signedBaseUnits({ amount: 10000n, currency: "USD" }, "in");
  const outbound = signedBaseUnits({ amount: 10000n, currency: "USD" }, "out");
  assert.equal(inbound, 10000);
  assert.equal(outbound, -10000);
});

test("unsignedBaseUnits converts Money to unsigned protocol amounts", () => {
  const amount = unsignedBaseUnits({ amount: 10000n, currency: "USD" });
  assert.equal(amount, 10000);
});

test("signedBaseUnits throws on amounts exceeding safe integer range", () => {
  const huge = { amount: BigInt(Number.MAX_SAFE_INTEGER) + 1n, currency: "USD" };
  assert.throws(() => signedBaseUnits(huge, "in"), /safe display range/u);
});

test("emptyFilterValues returns default filter values", () => {
  const empty = emptyFilterValues();
  assert.equal(empty.kind, "all");
  assert.equal(empty.agent, "all");
  assert.equal(empty.date, undefined);
});

test("toWireFilter converts FeedFilterValues to ActivityFilter", () => {
  const values: FeedFilterValues = {
    kind: "deposit",
    agent: "agent_123",
    date: { from: new Date("2026-08-01"), to: new Date("2026-08-31") },
  };
  const wire = toWireFilter(values);
  assert.deepEqual(wire.kinds, ["deposit"]);
  assert.equal(wire.agent_id, "agent_123");
  assert.equal(wire.from, "2026-08-01T00:00:00.000Z");
  assert.equal(wire.to, "2026-09-01T00:00:00.000Z");
});

test("toWireFilter handles all-types and all-agents filters", () => {
  const values: FeedFilterValues = { kind: "all", agent: "all", date: undefined };
  const wire = toWireFilter(values);
  assert.equal(wire.kinds, undefined);
  assert.equal(wire.agent_id, undefined);
  assert.equal(wire.from, undefined);
  assert.equal(wire.to, undefined);
});

test("toWireFilter handles date range with same from and to", () => {
  const values: FeedFilterValues = {
    kind: "all",
    agent: "all",
    date: { from: new Date("2026-08-15"), to: undefined },
  };
  const wire = toWireFilter(values);
  assert.equal(wire.from, "2026-08-15T00:00:00.000Z");
  assert.equal(wire.to, "2026-08-16T00:00:00.000Z");
});

test("sameFilterValues compares filter values by wire encoding", () => {
  const first: FeedFilterValues = { kind: "deposit", agent: "all", date: undefined };
  const second: FeedFilterValues = { kind: "deposit", agent: "all", date: undefined };
  const third: FeedFilterValues = { kind: "withdrawal", agent: "all", date: undefined };
  assert.equal(sameFilterValues(first, second), true);
  assert.equal(sameFilterValues(first, third), false);
});

test("feedFilterDefs builds filter definition list with options", () => {
  const agents = [
    { value: "agent_1", label: "Agent One" },
    { value: "agent_2", label: "Agent Two" },
  ];
  const defs = feedFilterDefs(agents);
  assert.equal(defs.length, 3);
  assert.equal(defs[0].id, "kind");
  assert.equal(defs[0].type, "options");
  assert.equal(defs[1].id, "agent");
  assert.equal(defs[1].type, "options");
  assert.equal(defs[2].id, "date");
  assert.equal(defs[2].type, "date-range");
  assert.ok(defs[1].options !== undefined);
  assert.equal(defs[1].options.length, 3);
});

test("filterEchoLines generates human-readable filter descriptions", () => {
  const names = new Map([["agent_1", "Agent One"]]);
  const emptyEcho = filterEchoLines({}, names);
  assert.equal(emptyEcho.length, 1);
  assert.ok(emptyEcho[0].includes("All"));

  const kindEcho = filterEchoLines({ kinds: ["deposit", "withdrawal"] }, names);
  assert.equal(kindEcho.length, 1);

  const agentEcho = filterEchoLines({ agent_id: "agent_1" }, names);
  assert.equal(agentEcho.length, 1);
  assert.ok(agentEcho[0].includes("Agent One"));

  const rangeEcho = filterEchoLines({ from: "2026-08-01T00:00:00Z", to: "2026-08-31T23:59:59Z" }, names);
  assert.equal(rangeEcho.length, 1);
});

test("agentFilterOptions sorts agents by label", () => {
  const agents = [
    { agent_id: "agent_2", name: "Zebra Agent" },
    { agent_id: "agent_1", name: "Alpha Agent" },
    { agent_id: "agent_3", name: "Beta Agent" },
  ];
  const options = agentFilterOptions(agents);
  assert.equal(options.length, 3);
  assert.equal(options[0].label, "Alpha Agent");
  assert.equal(options[1].label, "Beta Agent");
  assert.equal(options[2].label, "Zebra Agent");
});

test("mergePages deduplicates entries and updates cursors", () => {
  const current: ActivityPage = {
    groups: [
      {
        month: "2026-08",
        subtotal_in: { amount: 50000n, currency: "USD" },
        subtotal_out: { amount: 25000n, currency: "USD" },
        entries: [mockActivityPage.groups[0].entries[0]],
      },
    ],
    next_cursor: "cursor_1",
    filter: {},
  };
  const incoming: ActivityPage = {
    groups: [
      {
        month: "2026-08",
        subtotal_in: { amount: 75000n, currency: "USD" },
        subtotal_out: { amount: 25000n, currency: "USD" },
        entries: [mockActivityPage.groups[0].entries[0], mockActivityPage.groups[0].entries[1]],
      },
    ],
    next_cursor: "cursor_2",
    filter: {},
  };
  const merged = mergePages(current, incoming);
  assert.equal(merged.next_cursor, "cursor_2");
  assert.equal(merged.groups[0].entries.length, 2);
  assert.equal(merged.groups[0].subtotal_in.amount, 75000n);
});

test("detailUnresolved identifies still-checking entries", () => {
  assert.equal(detailUnresolved(mockActivityDetail), false);
  assert.equal(detailUnresolved(mockUnresolvedDetail), true);
});

test("stillCheckingLockReason returns the lock message", () => {
  const reason = stillCheckingLockReason();
  assert.equal(typeof reason, "string");
  assert.ok(reason.length > 0);
});

test("validatedDetail enforces receipt requirements for completed entries", () => {
  const valid = validatedDetail(mockActivityDetail);
  assert.equal(valid.entry_id, mockActivityDetail.entry_id);

  const incomplete: ActivityEntryDetail = {
    ...mockActivityDetail,
    state: "done",
    evidence: [],
    stages: [],
  };
  assert.throws(() => validatedDetail(incomplete), /verified receipt/u);
});

test("validatedDetail enforces receipt backing for money facts", () => {
  const withoutReceipt: ActivityEntryDetail = {
    ...mockActivityDetail,
    state: "processing",
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcpt_001",
        verification: "unverified",
      },
    ],
  };
  assert.throws(() => validatedDetail(withoutReceipt), /verified receipt/u);
});

test("receiptBacked identifies receipt-verified evidence", () => {
  assert.equal(
    receiptBacked({
      class: "layerx-receipt",
      evidence_id: "rcpt_001",
      verification: "checkpoint-finalised",
    }),
    true,
  );
  assert.equal(
    receiptBacked({
      class: "layerx-receipt",
      evidence_id: "rcpt_001",
      verification: "unverified",
    }),
    false,
  );
  assert.equal(
    receiptBacked({
      class: "checkpoint-proof",
      evidence_id: "chkpt_001",
      verification: "checkpoint-finalised",
    }),
    false,
  );
});

test("stageView transforms JourneyStage into TimelineStageView", () => {
  const stage = mockActivityDetail.stages[1];
  const view = stageView(stage);
  assert.equal(view.id, "stage_complete");
  assert.equal(view.status, "done");
  assert.equal(view.evidence.length, 1);
});

test("explorerPath builds explorer URLs for receipts and checkpoints", () => {
  assert.equal(
    explorerPath({
      class: "layerx-receipt",
      evidence_id: "rcpt_001",
      verification: "checkpoint-finalised",
    }),
    "/explorer/receipts/rcpt_001",
  );
  assert.equal(
    explorerPath({
      class: "checkpoint-proof",
      evidence_id: "chkpt_001",
      verification: "checkpoint-finalised",
    }),
    "/explorer/checkpoints/chkpt_001",
  );
  assert.equal(
    explorerPath({
      class: "paxeer-batch-proof",
      evidence_id: "batch_001",
      verification: "paxeer-finalised",
    }),
    undefined,
  );
});

test("verificationLabel returns localized label for verification levels", () => {
  const levels = ["unverified", "receipt-verified", "checkpoint-finalised", "paxeer-finalised"] as const;
  for (const level of levels) {
    const label = verificationLabel(level);
    assert.equal(typeof label, "string");
    assert.ok(label.length > 0);
  }
});

test("evidenceClassLabel returns localized label for evidence classes", () => {
  const classes = ["layerx-receipt", "checkpoint-proof", "paxeer-batch-proof"] as const;
  for (const evidenceClass of classes) {
    const label = evidenceClassLabel(evidenceClass);
    assert.equal(typeof label, "string");
    assert.ok(label.length > 0);
  }
});

test("entryVerification returns the highest verification level from evidence", () => {
  const evidence = [
    { class: "layerx-receipt" as const, evidence_id: "rcpt_1", verification: "unverified" as const },
    {
      class: "checkpoint-proof" as const,
      evidence_id: "chkpt_1",
      verification: "checkpoint-finalised" as const,
    },
    { class: "layerx-receipt" as const, evidence_id: "rcpt_2", verification: "receipt-verified" as const },
  ];
  assert.equal(entryVerification(evidence), "checkpoint-finalised");
  assert.equal(entryVerification([]), "unverified");
});

test("activityFailure transforms HumanApiError into ActivityFailure", () => {
  const apiError = {
    detail: {
      code: "ACTIVITY_UNAVAILABLE",
      copy_key: "error.activity_unavailable",
      retry: "retriable",
    },
    trace: "trc_001",
  };
  const failure = activityFailure({
    ...apiError,
    name: "HumanApiError",
    message: "Activity unavailable",
  });
  assert.equal(failure.kind, "service");
  assert.equal(failure.code, "ACTIVITY_UNAVAILABLE");
  assert.equal(failure.trace, "trc_001");
  assert.equal(failure.retriable, true);
});

test("activityFailure identifies offline errors from TypeError", () => {
  const networkError = new TypeError("Failed to fetch");
  const failure = activityFailure(networkError);
  assert.equal(failure.kind, "offline");
  assert.equal(failure.code, "connection-unavailable");
  assert.equal(failure.retriable, true);
});

test("newExportKey generates valid UUIDs", () => {
  const key1 = newExportKey();
  const key2 = newExportKey();
  assert.notEqual(key1, key2);
  assert.ok(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(key1));
});

test("safeExportArtefact validates export download paths", () => {
  const validArtefact: ExportArtefact = {
    kind: "statement",
    download_path: "/v1/activity/exports/exp_abcdefgh/download",
    evidence: [],
  };
  assert.equal(safeExportArtefact(validArtefact).download_path, validArtefact.download_path);

  const invalidArtefact: ExportArtefact = {
    kind: "statement",
    download_path: "/invalid/path",
    evidence: [],
  };
  assert.throws(() => safeExportArtefact(invalidArtefact), /unsafe download path/u);
});

test("safeExportArtefact prevents path traversal in download paths", () => {
  const traversalArtefact: ExportArtefact = {
    kind: "statement",
    download_path: "/v1/activity/exports/../../../etc/passwd",
    evidence: [],
  };
  assert.throws(() => safeExportArtefact(traversalArtefact), /unsafe download path/u);
});
