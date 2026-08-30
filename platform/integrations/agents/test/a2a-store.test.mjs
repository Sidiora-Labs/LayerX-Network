import { strict as assert } from "node:assert";
import { chmodSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { Role, TaskState } from "@a2a-js/sdk";
import {
  DefaultExecutionEventBus,
  RequestContext,
  ServerCallContext,
} from "@a2a-js/sdk/server";
import { createA2AIntegration, FileA2ATaskStore } from "../dist/a2a.js";

const workspace = mkdtempSync(join(tmpdir(), "layerx-a2a-store-"));
const databasePath = join(workspace, "tasks.sqlite3");
const context = callContext("tenant-a", "actor-a");
const otherOwner = callContext("tenant-a", "actor-b");
const store = new FileA2ATaskStore(databasePath, 2);

try {
  const base = task("task-1", TaskState.TASK_STATE_SUBMITTED, "2026-08-30T00:00:00.000Z");
  await store.save(base, context);
  assert.equal(base.metadata?.["layerx.task-store.revision"], "1");
  const loadedBase = await store.load(base.id, context);
  assert.equal(loadedBase?.id, base.id);
  assert.equal(await store.load(base.id, otherOwner), undefined);
  assert.ok(loadedBase);
  const concurrentStore = new FileA2ATaskStore(databasePath, 2);
  const concurrentSnapshot = await concurrentStore.load(base.id, context);
  assert.ok(concurrentSnapshot);

  const unspecified = await store.list({
    contextId: "",
    status: TaskState.TASK_STATE_UNSPECIFIED,
    pageSize: 50,
    pageToken: "",
    statusTimestampAfter: undefined,
    includeArtifacts: false,
    historyLength: undefined,
  }, context);
  assert.equal(unspecified.tasks.length, 0);

  const advanced = structuredClone(loadedBase);
  advanced.status = {
    state: TaskState.TASK_STATE_WORKING,
    message: undefined,
    timestamp: "2026-08-30T00:00:01.000Z",
  };
  advanced.history.push(message("message-2"));
  await store.save(advanced, context);
  assert.equal(advanced.metadata?.["layerx.task-store.revision"], "2");
  await assert.rejects(store.save(base, context), /service-refused/u);
  try {
    const conflicting = structuredClone(concurrentSnapshot);
    conflicting.status = {
      state: TaskState.TASK_STATE_WORKING,
      message: undefined,
      timestamp: "2026-08-30T00:00:02.000Z",
    };
    conflicting.history.push(message("message-3"));
    await assert.rejects(concurrentStore.save(conflicting, context), /service-refused/u);
  } finally {
    concurrentStore.close();
  }

  const artifactTask = await store.load(base.id, context);
  assert.ok(artifactTask);
  artifactTask.artifacts = [artifact("artifact-1", "first")];
  await store.save(artifactTask, context);
  const replacementTask = await store.load(base.id, context);
  assert.ok(replacementTask);
  replacementTask.artifacts = [artifact("artifact-1", "replacement")];
  await store.save(replacementTask, context);
  assert.equal(
    (await store.load(base.id, context))?.artifacts[0]?.parts[0]?.content?.value,
    "replacement",
  );

  const corruptPath = join(workspace, "corrupt.sqlite3");
  const corruptStore = new FileA2ATaskStore(corruptPath);
  const corruptTask = task("corrupt-task", TaskState.TASK_STATE_SUBMITTED, "2026-08-30T00:00:00.000Z");
  await corruptStore.save(corruptTask, context);
  const corruptor = new DatabaseSync(corruptPath);
  try {
    const row = corruptor.prepare(
      "SELECT task_json FROM layerx_a2a_tasks WHERE task_id = ?",
    ).get(corruptTask.id);
    assert.equal(typeof row?.task_json, "string");
    const wrongId = JSON.parse(row.task_json);
    wrongId.id = "other-task";
    corruptor.prepare("UPDATE layerx_a2a_tasks SET task_json = ? WHERE task_id = ?")
      .run(JSON.stringify(wrongId), corruptTask.id);
    await assert.rejects(corruptStore.load(corruptTask.id, context), /service-refused/u);
    await assert.rejects(corruptStore.list({
      contextId: "",
      status: undefined,
      pageSize: 50,
      pageToken: "",
      statusTimestampAfter: undefined,
      includeArtifacts: false,
      historyLength: undefined,
    }, context), /service-refused/u);
    wrongId.id = corruptTask.id;
    wrongId.metadata["layerx.task-store.revision"] = "999";
    corruptor.prepare("UPDATE layerx_a2a_tasks SET task_json = ? WHERE task_id = ?")
      .run(JSON.stringify(wrongId), corruptTask.id);
    await assert.rejects(corruptStore.load(corruptTask.id, context), /service-refused/u);
  } finally {
    corruptor.close();
    corruptStore.close();
  }

  chmodSync(workspace, 0o755);
  assert.throws(() => new FileA2ATaskStore(join(workspace, "insecure.sqlite3")), /service-refused/u);
  chmodSync(workspace, 0o700);

  const foreignPath = join(workspace, "foreign.sqlite3");
  const foreign = new DatabaseSync(foreignPath);
  foreign.exec("CREATE TABLE sentinel (value TEXT) STRICT");
  foreign.close();
  chmodSync(foreignPath, 0o600);
  assert.throws(() => new FileA2ATaskStore(foreignPath), /service-refused/u);
  const unchanged = new DatabaseSync(foreignPath);
  try {
    assert.equal(unchanged.prepare("PRAGMA journal_mode").get().journal_mode, "delete");
    assert.equal(unchanged.prepare(
      "SELECT COUNT(*) AS count FROM sqlite_schema WHERE type = 'table' AND name = 'sentinel'",
    ).get().count, 1);
  } finally {
    unchanged.close();
  }

  const triggeredPath = join(workspace, "triggered.sqlite3");
  const triggerSeed = new FileA2ATaskStore(triggeredPath);
  triggerSeed.close();
  const triggerDatabase = new DatabaseSync(triggeredPath);
  triggerDatabase.exec(`CREATE TRIGGER discard_tasks BEFORE INSERT ON layerx_a2a_tasks
    BEGIN SELECT RAISE(IGNORE); END`);
  triggerDatabase.close();
  assert.throws(() => new FileA2ATaskStore(triggeredPath), /service-refused/u);

  const identityStore = new FileA2ATaskStore(join(workspace, "identity.sqlite3"));
  const integration = createA2AIntegration({
    environment: environment(workspace),
    durableTaskStore: identityStore,
    authentication: {
      schemeName: "layerxBearer",
      securityScheme: {
        scheme: {
          $case: "httpAuthSecurityScheme",
          value: { description: "test", scheme: "Bearer", bearerFormat: "opaque" },
        },
      },
      authenticate: async (value) => value,
    },
  });
  try {
    for (const arguments_ of [
      { tenant: "tenant-b" },
      { actor: "actor-b" },
      { authority: "authority-b" },
    ]) {
      const boundRequest = new RequestContext({
        tenant: "tenant-a",
        message: toolMessage(arguments_),
        configuration: undefined,
        metadata: undefined,
      }, "task-1", "context-1", context);
      await assert.rejects(
        integration.executor.execute(boundRequest, new DefaultExecutionEventBus()),
        /service-refused/u,
      );
    }
    const foreignContext = callContext("tenant-a", "actor-b");
    const requestMessage = message("message-foreign");
    const request = new RequestContext({
      tenant: "tenant-a",
      message: requestMessage,
      configuration: undefined,
      metadata: undefined,
    }, "task-1", "context-1", foreignContext);
    await assert.rejects(
      integration.executor.execute(request, new DefaultExecutionEventBus()),
      /service-refused/u,
    );
  } finally {
    integration.destroy();
    identityStore.close();
  }
} finally {
  store.close();
  rmSync(workspace, { recursive: true, force: true });
}

function callContext(tenant, actor) {
  return new ServerCallContext({
    tenant,
    requestedVersion: "1.0",
    user: { isAuthenticated: true, userName: actor },
  });
}

function task(id, state, timestamp) {
  return {
    id,
    contextId: "context-1",
    status: { state, message: undefined, timestamp },
    artifacts: [],
    history: [message("message-1")],
    metadata: undefined,
  };
}

function message(messageId) {
  return {
    messageId,
    contextId: "context-1",
    taskId: "task-1",
    role: Role.ROLE_USER,
    parts: [],
    metadata: undefined,
    extensions: [],
    referenceTaskIds: [],
  };
}

function artifact(artifactId, text) {
  return {
    artifactId,
    name: "result",
    description: "result",
    parts: [{
      content: { $case: "text", value: text },
      metadata: undefined,
      filename: "",
      mediaType: "text/plain",
    }],
    metadata: undefined,
    extensions: [],
  };
}

function toolMessage(arguments_) {
  return {
    ...message("message-tool"),
    parts: [{
      content: { $case: "data", value: { tool: "layerx_spend", arguments: arguments_ } },
      metadata: undefined,
      filename: "",
      mediaType: "application/json",
    }],
  };
}

function environment(path) {
  const endpoint = "http://127.0.0.1:1";
  return {
    LAYERX_AGENT_RPC_URL: endpoint,
    LAYERX_BUDGET_SERVICE_URL: endpoint,
    LAYERX_SIGNER_SERVICE_URL: endpoint,
    LAYERX_RECEIPT_SERVICE_URL: endpoint,
    LAYERX_TENANT: "tenant-a",
    LAYERX_ACTOR: "actor-a",
    LAYERX_AUTHORITY: "authority-a",
    LAYERX_FEE_LIMIT: "1000",
    LAYERX_WEBHOOK_PUBLIC_KEYS_JSON: JSON.stringify({ test: "00".repeat(32) }),
    LAYERX_WEBHOOK_DELIVERY_STORE_PATH: join(path, "webhooks.json"),
    LAYERX_A2A_URL: "http://127.0.0.1:3000",
    LAYERX_TOKEN: "test-token",
  };
}
