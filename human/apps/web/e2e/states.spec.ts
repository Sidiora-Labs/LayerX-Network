import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { SupportReportRepository } from "../src/server/support-reports.ts";
import { CURRENT_ROUTES, CURRENT_SHELLS, REQUIRED_SCREEN_STATES, humanStateMatrix } from "../src/states/matrix.ts";
import { OfflineActionQueue } from "../src/states/queue.ts";
import { redactErrorReport } from "../src/states/report.ts";
import { parseSupportReportRequest } from "../src/states/report-schema.ts";

test("every current route and shell registers the complete state matrix", () => {
  const matrix = humanStateMatrix();
  matrix.assertComplete(CURRENT_ROUTES, CURRENT_SHELLS);
  for (const target of matrix.enumerate()) {
    assert.deepEqual(Object.keys(target.states).sort(), [...REQUIRED_SCREEN_STATES].sort());
    assert.equal(target.states["still-checking"]().duplicateActionsLocked, true);
  }
});

test("offline queue deduplicates safe actions and refuses money actions", async () => {
  const queue = new OfflineActionQueue();
  let readRuns = 0;
  let moneyRuns = 0;
  const read = { kind: "read", queueKey: "activity.refresh", run: async () => { readRuns += 1; } } as const;
  const money = { kind: "money", queueKey: "move.123", run: async () => { moneyRuns += 1; } } as const;

  assert.equal(queue.enqueue(read), "queued");
  assert.equal(queue.enqueue(read), "duplicate");
  assert.equal(queue.enqueue(money), "money-rejected");
  await Promise.all([queue.flush(), queue.flush()]);
  assert.equal(readRuns, 1);
  assert.equal(moneyRuns, 0);
  assert.equal(queue.size(), 0);
});

test("error reports require consent and retain only the non-sensitive schema", () => {
  assert.throws(() => redactErrorReport({
    consented: false,
    traceId: "trc_ui_123",
    machineCode: "UI_UNHANDLED",
    route: "/app",
    shell: "desktop",
  }), /consent/u);

  const report = redactErrorReport({
    consented: true,
    traceId: "trc_ui_123",
    machineCode: "UI_UNHANDLED",
    route: "/app",
    shell: "desktop",
  });
  assert.deepEqual(Object.keys(report).sort(), ["machineCode", "route", "shell", "traceId"]);
  assert.equal(report.traceId, "trc_ui_123");
  assert.throws(() => parseSupportReportRequest({ ...report, enteredValue: "private" }), /fields/u);
});

test("support reports are idempotent and retained independently for each verified principal", async () => {
  const directory = await mkdtemp(join(tmpdir(), "layerx-support-reports-"));
  const repository = new SupportReportRepository(directory, 2);
  const firstPrincipal = "verified-principal-0001";
  const secondPrincipal = "verified-principal-0002";
  const first = parseSupportReportRequest({
    traceId: "trc_ui_first",
    machineCode: "UI_UNHANDLED",
    route: "/app",
    shell: "desktop",
  });
  try {
    assert.equal((await repository.save(firstPrincipal, first)).created, true);
    assert.equal((await repository.save(firstPrincipal, first)).created, false);
    await repository.save(firstPrincipal, parseSupportReportRequest({
      traceId: "trc_ui_second",
      machineCode: "UI_UNHANDLED",
      route: "/app",
      shell: "mobile",
    }));
    await repository.save(secondPrincipal, parseSupportReportRequest({
      traceId: "trc_ui_other_first",
      machineCode: "UI_UNHANDLED",
      route: "/app",
      shell: "desktop",
    }));
    await repository.save(secondPrincipal, parseSupportReportRequest({
      traceId: "trc_ui_other_second",
      machineCode: "UI_UNHANDLED",
      route: "/app",
      shell: "mobile",
    }));
    await repository.save(firstPrincipal, parseSupportReportRequest({
      traceId: "trc_ui_third",
      machineCode: "UI_UNHANDLED",
      route: "/explorer",
      shell: "desktop",
    }));
    assert.equal(await repository.findByTrace("trc_ui_first", firstPrincipal), undefined);
    assert.equal((await repository.findByTrace("trc_ui_second", firstPrincipal))?.shell, "mobile");
    assert.equal((await repository.findByTrace("trc_ui_third", firstPrincipal))?.route, "/explorer");
    assert.equal((await repository.findByTrace("trc_ui_other_first", secondPrincipal))?.route, "/app");
    assert.equal((await repository.findByTrace("trc_ui_other_second", secondPrincipal))?.shell, "mobile");
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
