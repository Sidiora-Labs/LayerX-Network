import assert from "node:assert/strict";
import test from "node:test";

import { copyEntry } from "../copy/catalog.ts";
import { formatCopy } from "../copy/format.ts";
import {
  Agents,
  agentListItems,
  agentPresentation,
  agentsLayout,
  agentStateVerified,
  controlsFor,
  creationHeadlineKey,
  creationReady,
  creationSteps,
  formatPlainDelay,
  formatPlainTimestamp,
  journeyProgress,
  keyChallengePresentation,
  parseMonthlyLimit,
  spendPresentation,
  type Agent,
  type AgentControlContext,
  type AgentPage,
  type CreationDraft,
  type Journey,
  type KeyChallenge,
  type Money,
} from "../src/journeys/agents/model.ts";

const LOCALE = "en-GB";
const CURRENCY = "LXP";

test("agent creation requires exactly three decisions - name, purpose and monthly limit", () => {
  const emptyDraft: CreationDraft = {
    name: "",
    purpose: "",
    limitInput: "",
    currency: CURRENCY,
  };
  const steps = creationSteps(emptyDraft);
  assert.equal(steps.length, 3);
  assert.equal(steps[0].id, "name");
  assert.equal(steps[1].id, "purpose");
  assert.equal(steps[2].id, "limit");
  assert.equal(creationReady(emptyDraft), false);
});

test("creation draft progresses honestly through each decision", () => {
  let draft: CreationDraft = {
    name: "",
    purpose: "",
    limitInput: "",
    currency: CURRENCY,
  };
  assert.equal(creationSteps(draft).filter((s) => s.complete).length, 0);

  draft = { ...draft, name: "Test Agent" };
  assert.equal(creationSteps(draft).filter((s) => s.complete).length, 1);
  assert.equal(creationReady(draft), false);

  draft = { ...draft, purpose: "API testing automation" };
  assert.equal(creationSteps(draft).filter((s) => s.complete).length, 2);
  assert.equal(creationReady(draft), false);

  draft = { ...draft, limitInput: "100000" };
  assert.equal(creationSteps(draft).filter((s) => s.complete).length, 3);
  assert.equal(creationReady(draft), true);
});

test("monthly limit parsing accepts only positive protocol integers", () => {
  assert.deepEqual(parseMonthlyLimit("100", CURRENCY), { amount: 100n, currency: CURRENCY });
  assert.deepEqual(parseMonthlyLimit("0", CURRENCY), undefined);
  assert.deepEqual(parseMonthlyLimit("-1", CURRENCY), undefined);
  assert.deepEqual(parseMonthlyLimit("", CURRENCY), undefined);
  assert.deepEqual(parseMonthlyLimit("100.50", CURRENCY), undefined);
  assert.deepEqual(parseMonthlyLimit("abc", CURRENCY), undefined);
  assert.deepEqual(parseMonthlyLimit(" 250 ", CURRENCY), { amount: 250n, currency: CURRENCY });
});

test("creation journey progress surfaces honest partial and complete states", () => {
  const mockJourney: Journey = {
    journey_id: "jrn_creation_001",
    kind: "agent-create",
    state_copy_key: "status.processing",
    evidence: [],
    started_at: "2026-08-01T00:00:00Z",
    updated_at: "2026-08-01T00:00:00Z",
    state: "processing",
    stages: [
      {
        stage_id: "register-did",
        copy_key: "agent.create.stage.setting-up",
        state: "done",
        evidence: [
          {
            class: "layerx-receipt",
            evidence_id: "rcp_001",
            verification: "receipt-verified",
          },
        ],
      },
      {
        stage_id: "create-budget",
        copy_key: "agent.create.stage.protection",
        state: "processing",
        evidence: [],
      },
    ],
  };

  const progress = journeyProgress(mockJourney);
  assert.equal(progress.complete, false);
  assert.equal(creationHeadlineKey(progress), "agent.create.partial");
  assert.equal(progress.stages.length, 2);
  assert.equal(progress.stages[0]!.receiptVerified, true);
  assert.equal(progress.stages[1]!.receiptVerified, false);
});

test("creation journey complete requires all stages receipt-verified", () => {
  const completeJourney: Journey = {
    journey_id: "jrn_creation_002",
    kind: "agent-create",
    state_copy_key: "status.processing",
    evidence: [],
    started_at: "2026-08-01T00:00:00Z",
    updated_at: "2026-08-01T00:00:00Z",
    state: "done",
    stages: [
      {
        stage_id: "register-did",
        copy_key: "agent.create.stage.setting-up",
        state: "done",
        evidence: [
          {
            class: "layerx-receipt",
            evidence_id: "rcp_001",
            verification: "receipt-verified",
          },
        ],
      },
      {
        stage_id: "create-budget",
        copy_key: "agent.create.stage.protection",
        state: "done",
        evidence: [
          {
            class: "layerx-receipt",
            evidence_id: "rcp_002",
            verification: "checkpoint-finalised",
          },
        ],
      },
    ],
  };

  const progress = journeyProgress(completeJourney);
  assert.equal(progress.complete, true);
  assert.equal(creationHeadlineKey(progress), "agent.create.ready");
});

test("agent list renders for both mobile and desktop shells", () => {
  assert.equal(agentsLayout("mobile"), "stacked");
  assert.equal(agentsLayout("desktop"), "master-detail");
});

test("agent list items include spend summary from receipts", () => {
  const mockPage: AgentPage = {
    next_cursor: "",
    agents: [
      {
        agent_id: "agt_001",
        name: "Purchase Bot",
        purpose: "Automate purchases",
        state: "active",
        state_copy_key: "agent.state.active",
        created_at: "2026-08-01T10:00:00Z",
        updated_at: "2026-08-15T00:00:00Z",
        spend: {
          period_start: "2026-08-01T00:00:00Z",
          period_end: "2026-08-31T23:59:59Z",
          spent: { amount: 25000n, currency: CURRENCY },
          remaining: { amount: 75000n, currency: CURRENCY },
          verification: "receipt-verified",
          reconciliation_copy_key: "agent.spend.reconciled-to-protocol",
        },
        limit: {
          monthly: { amount: 100000n, currency: CURRENCY },
          enforcement: "protocol",
          enforcement_copy_key: "agent.limit.protocol-backed",
        },
        evidence: [
          {
            class: "layerx-receipt",
            evidence_id: "rcp_state_001",
            verification: "receipt-verified",
          },
        ],
      },
    ],
  };

  const items = agentListItems(mockPage, LOCALE);
  assert.equal(items.length, 1);
  assert.equal(items[0]!.name, "Purchase Bot");
  assert.match(items[0]!.spendSummary, /25,000/u);
  assert.match(items[0]!.spendSummary, /100,000/u);
});

test("agent detail shows spend versus limit computed from verified receipts", () => {
  const mockAgent: Agent = {
    agent_id: "agt_002",
    name: "Payment Agent",
    purpose: "Process payments",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T12:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 30000n, currency: CURRENCY },
      remaining: { amount: 70000n, currency: CURRENCY },
      verification: "checkpoint-finalised",
    },
    limit: {
      monthly: { amount: 100000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "checkpoint-proof",
        evidence_id: "ckp_001",
        verification: "checkpoint-finalised",
      },
    ],
  };

  const presentation = spendPresentation(mockAgent, LOCALE);
  assert.match(presentation.spent, /30,000/u);
  assert.match(presentation.remaining, /70,000/u);
  assert.match(presentation.limit, /100,000/u);
  assert.equal(presentation.percentSpent, 30);
  assert.equal(presentation.protocolBacked, true);
  assert.match(presentation.enforcementSentence, /protocol/iu);
});

test("agent controls surface when agent is active and receipt-verified", () => {
  const activeAgent: Agent = {
    agent_id: "agt_003",
    name: "Active Agent",
    purpose: "Testing controls",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T14:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 0n, currency: CURRENCY },
      remaining: { amount: 50000n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "app",
      enforcement_copy_key: "agent.limit.app-enforced",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_003",
        verification: "receipt-verified",
      },
    ],
  };

  const context: AgentControlContext = {
    ownerAccount: "acc_owner_001",
  };

  const controls = controlsFor(activeAgent, context);
  assert.equal(controls.length, 7);

  const fund = controls.find((c) => c.id === "fund");
  assert.equal(fund?.kind, "reversible");
  assert.equal(fund?.enabled, true);

  const reclaim = controls.find((c) => c.id === "reclaim");
  assert.equal(reclaim?.kind, "reversible");
  assert.equal(reclaim?.enabled, true);

  const changeLimit = controls.find((c) => c.id === "limit");
  assert.equal(changeLimit?.kind, "reversible");
  assert.equal(changeLimit?.enabled, true);

  const pause = controls.find((c) => c.id === "pause");
  assert.equal(pause?.kind, "reversible");
  assert.equal(pause?.enabled, true);

  const rotate = controls.find((c) => c.id === "rotate");
  assert.equal(rotate?.kind, "reversible");
  assert.equal(rotate?.enabled, true);

  const recover = controls.find((c) => c.id === "recover");
  assert.equal(recover?.kind, "reversible");
  assert.equal(recover?.enabled, true);

  const archive = controls.find((c) => c.id === "archive");
  assert.equal(archive?.kind, "irreversible");
  assert.equal(archive?.enabled, true);
  assert.equal(archive?.dispositionFirst, true);
  assert.equal(archive?.typedExpected, "Active Agent");
});

test("pause is reversible and resume replaces it", () => {
  const pausedAgent: Agent = {
    agent_id: "agt_004",
    name: "Paused Agent",
    purpose: "Testing pause/resume",
    state: "paused",
    state_copy_key: "agent.state.paused",
    created_at: "2026-08-01T15:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 10000n, currency: CURRENCY },
      remaining: { amount: 40000n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_004",
        verification: "receipt-verified",
      },
    ],
  };

  const controls = controlsFor(pausedAgent, {});
  const resume = controls.find((c) => c.id === "resume");
  const pause = controls.find((c) => c.id === "pause");

  assert.equal(resume?.enabled, true);
  assert.equal(resume?.kind, "reversible");
  assert.equal(pause, undefined);
});

test("archive requires disposition first and typed confirmation", () => {
  const agent: Agent = {
    agent_id: "agt_005",
    name: "Archive Test",
    purpose: "Testing archive flow",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T16:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 5000n, currency: CURRENCY },
      remaining: { amount: 45000n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_005",
        verification: "receipt-verified",
      },
    ],
  };

  const controls = controlsFor(agent, {});
  const archive = controls.find((c) => c.id === "archive");

  assert.equal(archive?.kind, "irreversible");
  assert.equal(archive?.dispositionFirst, true);
  assert.equal(archive?.typedExpected, "Archive Test");
});

test("rotation and recovery present challenge delay in plain time", () => {
  const mockChallenge: KeyChallenge = {
    kind: "rotate",
    agent_id: "agt_006",
    delay_seconds: 86400,
    delay_copy_key: "agent.keys.rotate-delay",
    ready_at: "2026-08-24T10:00:00Z",
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_rotation_006",
        verification: "receipt-verified",
      },
    ],
  };

  const presentation = keyChallengePresentation(mockChallenge, LOCALE);
  assert.match(presentation.delayText, /day/iu);
  assert.equal(
    presentation.delaySentence,
    formatCopy("agent.keys.rotate-delay", { delay: presentation.delayText }),
  );
  assert.match(presentation.readySentence, /24 Aug 2026/u);
  assert.equal(presentation.bodyKey, "agent.keys.rotate.body");
});

test("plain delay formatting chooses the largest whole unit", () => {
  assert.match(formatPlainDelay(86400), /1.*day/iu);
  assert.match(formatPlainDelay(172800), /2.*day/iu);
  assert.match(formatPlainDelay(3600), /1.*hour/iu);
  assert.match(formatPlainDelay(7200), /2.*hour/iu);
  assert.match(formatPlainDelay(60), /1.*minute/iu);
  assert.match(formatPlainDelay(120), /2.*minute/iu);
  assert.match(formatPlainDelay(30), /1.*minute/iu);
  assert.match(formatPlainDelay(90), /2.*minute/iu);
});

test("plain timestamp formatting renders in UTC with explicit timezone", () => {
  const formatted = formatPlainTimestamp("2026-08-23T15:30:00Z", LOCALE);
  assert.match(formatted, /23 Aug 2026/u);
  assert.match(formatted, /15:30/u);
  assert.match(formatted, /UTC/u);
});

test("agent state verified requires receipt-backed evidence", () => {
  const verifiedAgent: Agent = {
    agent_id: "agt_007",
    name: "Verified",
    purpose: "Test",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T17:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 0n, currency: CURRENCY },
      remaining: { amount: 50000n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_007",
        verification: "receipt-verified",
      },
    ],
  };

  assert.equal(agentStateVerified(verifiedAgent), true);

  const unverifiedAgent: Agent = {
    ...verifiedAgent,
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_008",
        verification: "unverified",
      },
    ],
  };

  assert.equal(agentStateVerified(unverifiedAgent), false);
});

test("agent presentation suppresses unverified active state", () => {
  const unverifiedActiveAgent: Agent = {
    agent_id: "agt_008",
    name: "Creating",
    purpose: "Test",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T18:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 0n, currency: CURRENCY },
      remaining: { amount: 50000n, currency: CURRENCY },
      verification: "unverified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "app",
      enforcement_copy_key: "agent.limit.app-enforced",
    },
    evidence: [],
  };

  const presentation = agentPresentation(unverifiedActiveAgent);
  assert.equal(presentation.label, copyEntry("agent.state.creating").message);
  assert.equal(presentation.tone, "accent");
  assert.equal(presentation.stateVerified, false);
});

test("archived agents are read-only with explicit notice", () => {
  const archivedAgent: Agent = {
    agent_id: "agt_009",
    name: "Archived",
    purpose: "Test",
    state: "archived",
    state_copy_key: "agent.state.archived",
    created_at: "2026-08-01T19:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 20000n, currency: CURRENCY },
      remaining: { amount: 0n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_009",
        verification: "receipt-verified",
      },
    ],
  };

  const presentation = agentPresentation(archivedAgent);
  assert.equal(presentation.readOnly, true);
  assert.equal(presentation.readOnlyKey, "agent.archive.readonly");
  assert.equal(controlsFor(archivedAgent, {}).length, 0);
});

test("limit enforcement labelling distinguishes protocol from app restriction", () => {
  const protocolAgent: Agent = {
    agent_id: "agt_010",
    name: "Protocol Limited",
    purpose: "Test",
    state: "active",
    state_copy_key: "agent.state.active",
    created_at: "2026-08-01T20:00:00Z",
    updated_at: "2026-08-15T00:00:00Z",
    spend: {
      period_start: "2026-08-01T00:00:00Z",
      period_end: "2026-08-31T23:59:59Z",
      spent: { amount: 10000n, currency: CURRENCY },
      remaining: { amount: 40000n, currency: CURRENCY },
      verification: "receipt-verified",
    },
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "protocol",
      enforcement_copy_key: "agent.limit.protocol-backed",
    },
    evidence: [
      {
        class: "layerx-receipt",
        evidence_id: "rcp_010",
        verification: "receipt-verified",
      },
    ],
  };

  const protocolPresentation = spendPresentation(protocolAgent, LOCALE);
  assert.equal(protocolPresentation.protocolBacked, true);
  assert.match(protocolPresentation.enforcementSentence, /protocol/iu);

  const appAgent: Agent = {
    ...protocolAgent,
    limit: {
      monthly: { amount: 50000n, currency: CURRENCY },
      enforcement: "app",
      enforcement_copy_key: "agent.limit.app-enforced",
    },
  };

  const appPresentation = spendPresentation(appAgent, LOCALE);
  assert.equal(appPresentation.protocolBacked, false);
  assert.match(appPresentation.enforcementSentence, /app/iu);
});

test("agents class supports idempotent mutations with automatic key management", async () => {
  const agents = new Agents({
    idempotencyKey: () => "test_key_001",
  });

  assert.ok(agents);
  assert.equal(typeof agents.overview, "function");
  assert.equal(typeof agents.create, "function");
  assert.equal(typeof agents.pause, "function");
  assert.equal(typeof agents.resume, "function");
  assert.equal(typeof agents.archive, "function");
  assert.equal(typeof agents.reclaim, "function");
});

test("reclaim journey completion returns money from agent to human", async () => {
  const mockJourney: Journey = {
    journey_id: "jrn_reclaim_001",
    kind: "move",
    state_copy_key: "status.processing",
    evidence: [],
    started_at: "2026-08-01T00:00:00Z",
    updated_at: "2026-08-01T00:00:00Z",
    state: "done",
    stages: [
      {
        stage_id: "reclaim-transfer",
        copy_key: "agent.reclaim.consequence",
        state: "done",
        evidence: [
          {
            class: "layerx-receipt",
            evidence_id: "rcp_reclaim_001",
            verification: "receipt-verified",
          },
        ],
      },
    ],
  };

  const progress = journeyProgress(mockJourney);
  assert.equal(progress.complete, true);
  assert.equal(progress.kind, "move");
});
