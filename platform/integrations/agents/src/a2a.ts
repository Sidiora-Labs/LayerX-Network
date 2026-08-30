import {
  Role,
  Task,
  TaskState,
  type AgentCard,
  type ListTasksRequest,
  type ListTasksResponse,
  type Message,
  type Part,
  type SecurityScheme,
} from "@a2a-js/sdk";
import {
  AgentEvent,
  DefaultRequestHandler,
  ServerCallContext,
  type A2ARequestHandler,
  type AgentExecutor,
  type ExecutionEventBus,
  type RequestContext,
  type TaskStore,
} from "@a2a-js/sdk/server";
import { RequestMalformedError, UnsupportedOperationError } from "@a2a-js/sdk/errors";
import { chmodSync, existsSync, lstatSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { AgentIntegrationError, endpoint, required } from "./config.js";
import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import { renderOutcome, type ToolJsonObject, type ToolOutcome } from "./tools.js";

export interface A2AToolRequest {
  readonly tool: string;
  readonly arguments: unknown;
}

export interface A2AAuthenticationBoundary {
  readonly schemeName: string;
  readonly securityScheme: SecurityScheme;
  readonly requiredScopes?: readonly string[];
  readonly authenticate: (context: ServerCallContext) => Promise<ServerCallContext>;
}

export interface LayerXA2AOptions extends AgentIntegrationOptions {
  readonly durableTaskStore: TaskStore;
  readonly authentication: A2AAuthenticationBoundary;
}

export interface LayerXA2AIntegration extends LayerXAgentIntegration {
  readonly agentCard: AgentCard;
  readonly executor: LayerXA2AExecutor;
  readonly requestHandler: A2ARequestHandler;
  /** Executes inside the owning process and does not cross the authenticated network mount. */
  executeToolRequest(request: A2AToolRequest): Promise<ToolOutcome>;
}

export class FileA2ATaskStore implements TaskStore {
  readonly #path: string;
  readonly #capacity: number;
  readonly #database: DatabaseSync;

  public constructor(path = ".layerx/a2a-tasks-v1.sqlite3", capacity = 65_536) {
    if (path.length === 0 || path.length > 4_096 || path.includes("\0")
        || !Number.isSafeInteger(capacity) || capacity < 1) {
      throw new AgentIntegrationError("invalid-declared-key");
    }
    this.#path = resolve(path);
    this.#capacity = capacity;
    let database: DatabaseSync | undefined;
    try {
      const parent = dirname(this.#path);
      mkdirSync(parent, { recursive: true, mode: 0o700 });
      const parentMetadata = lstatSync(parent);
      if (!parentMetadata.isDirectory() || parentMetadata.isSymbolicLink()
          || (parentMetadata.mode & 0o077) !== 0) {
        throw new AgentIntegrationError("service-refused");
      }
      if (existsSync(this.#path)) {
        const metadata = lstatSync(this.#path);
        if (!metadata.isFile() || metadata.isSymbolicLink() || (metadata.mode & 0o077) !== 0) {
          throw new AgentIntegrationError("service-refused");
        }
      }
      database = new DatabaseSync(this.#path, {
        timeout: 5_000,
        enableForeignKeyConstraints: true,
        enableDoubleQuotedStringLiterals: false,
        allowExtension: false,
      });
      initializeTaskDatabase(database);
      secureTaskDatabaseFiles(this.#path);
      this.#database = database;
    } catch (error) {
      database?.close();
      if (error instanceof AgentIntegrationError) throw error;
      throw new AgentIntegrationError("service-refused");
    }
  }

  public async save(task: Task, context: ServerCallContext): Promise<void> {
    const scope = taskScope(context);
    const stored = normalizedTask(task);
    const originalMetadata = task.metadata;
    try {
      this.#transaction(() => {
        let expectedEncoded: string;
        let expectedRevision: number;
        const existingRow = this.#database.prepare(
          "SELECT task_json, revision FROM layerx_a2a_tasks WHERE tenant = ? AND owner = ? AND task_id = ?",
        ).get(scope.tenant, scope.owner, stored.id);
        if (existingRow === undefined) {
          if (taskRevision(stored) !== undefined) throw new AgentIntegrationError("service-refused");
          const count = this.#database.prepare("SELECT COUNT(*) AS count FROM layerx_a2a_tasks").get();
          if (count === undefined || typeof count["count"] !== "number" || count["count"] >= this.#capacity) {
            throw new AgentIntegrationError("service-refused");
          }
          const versioned = withTaskRevision(stored, 1);
          expectedEncoded = encodeStoredTask(versioned);
          expectedRevision = 1;
          const result = this.#database.prepare(`INSERT INTO layerx_a2a_tasks (tenant, owner, task_id, task_json, revision)
            VALUES (?, ?, ?, ?, ?)`
          ).run(scope.tenant, scope.owner, stored.id, expectedEncoded, expectedRevision);
          if (result.changes !== 1) throw new AgentIntegrationError("service-refused");
        } else {
          const existing = existingRow["task_json"];
          const revision = existingRow["revision"];
          if (typeof existing !== "string" || typeof revision !== "number"
              || !Number.isSafeInteger(revision) || revision < 1 || revision >= Number.MAX_SAFE_INTEGER) {
            throw new AgentIntegrationError("service-refused");
          }
          const current = decodeStoredTask(existing);
          if (taskRevision(current) !== revision || taskRevision(stored) !== revision
              || current.id !== stored.id || current.contextId !== stored.contextId) {
            throw new AgentIntegrationError("service-refused");
          }
          if (isTerminalState(current.status?.state)
              && encodeCanonical(current.status) !== encodeCanonical(stored.status)) {
            throw new AgentIntegrationError("service-refused");
          }
          const nextRevision = revision + 1;
          const versioned = withTaskRevision(stored, nextRevision);
          expectedEncoded = encodeStoredTask(versioned);
          expectedRevision = nextRevision;
          const result = this.#database.prepare(`UPDATE layerx_a2a_tasks SET task_json = ?, revision = ?
            WHERE tenant = ? AND owner = ? AND task_id = ? AND revision = ?`
          ).run(
            expectedEncoded,
            nextRevision,
            scope.tenant,
            scope.owner,
            stored.id,
            revision,
          );
          if (result.changes !== 1) throw new AgentIntegrationError("service-refused");
        }
        const persisted = this.#database.prepare(
          "SELECT task_json, revision FROM layerx_a2a_tasks WHERE tenant = ? AND owner = ? AND task_id = ?",
        ).get(scope.tenant, scope.owner, stored.id);
        if (persisted?.["task_json"] !== expectedEncoded || persisted["revision"] !== expectedRevision) {
          throw new AgentIntegrationError("service-refused");
        }
        secureTaskDatabaseFiles(this.#path);
        setTaskRevision(task, expectedRevision);
      });
    } catch (error) {
      try { task.metadata = originalMetadata; } catch { /* the write already failed closed */ }
      throw error;
    }
  }

  public async load(taskId: string, context: ServerCallContext): Promise<Task | undefined> {
    requireTaskIdentifier(taskId);
    const scope = taskScope(context);
    try {
      const row = this.#database.prepare(
        "SELECT task_id, task_json, revision FROM layerx_a2a_tasks WHERE tenant = ? AND owner = ? AND task_id = ?",
      ).get(scope.tenant, scope.owner, taskId);
      if (row === undefined) return undefined;
      return decodeTaskRow(row, taskId);
    } catch (error) {
      if (error instanceof AgentIntegrationError) throw error;
      throw new AgentIntegrationError("service-refused");
    }
  }

  public async list(params: ListTasksRequest, context: ServerCallContext): Promise<ListTasksResponse> {
    const scope = taskScope(context);
    const pageSize = params.pageSize ?? 50;
    if (!Number.isSafeInteger(pageSize) || pageSize < 1 || pageSize > 100) {
      throw new RequestMalformedError("Invalid page size.");
    }
    let tasks: Task[];
    try {
      tasks = this.#database.prepare(
        "SELECT task_id, task_json, revision FROM layerx_a2a_tasks WHERE tenant = ? AND owner = ?",
      ).all(scope.tenant, scope.owner).map((row) => decodeTaskRow(row));
    } catch (error) {
      if (error instanceof AgentIntegrationError) throw error;
      throw new AgentIntegrationError("service-refused");
    }
    if (params.contextId.length > 0) {
      tasks = tasks.filter((task) => task.contextId === params.contextId);
    }
    if (params.status !== undefined) {
      tasks = tasks.filter((task) => task.status?.state === params.status);
    }
    if (params.statusTimestampAfter !== undefined && params.statusTimestampAfter.length > 0) {
      const after = new Date(params.statusTimestampAfter).getTime();
      if (!Number.isFinite(after)) throw new RequestMalformedError("Invalid status timestamp.");
      tasks = tasks.filter((task) => {
        const timestamp = task.status?.timestamp;
        return timestamp !== undefined && new Date(timestamp).getTime() > after;
      });
    }
    tasks.sort((left, right) => {
      const leftTime = taskTimestamp(left) ?? Number.NEGATIVE_INFINITY;
      const rightTime = taskTimestamp(right) ?? Number.NEGATIVE_INFINITY;
      return rightTime === leftTime ? right.id.localeCompare(left.id) : rightTime - leftTime;
    });
    const totalSize = tasks.length;
    if (params.pageToken.length > 0) {
      const cursor = decodeTaskCursor(params.pageToken);
      const index = tasks.findIndex((task) =>
        (task.status?.timestamp ?? "") === cursor.timestamp && task.id === cursor.id);
      tasks = index === -1 ? [] : tasks.slice(index + 1);
    }
    const page = tasks.slice(0, pageSize).map((task) => {
      const copy = structuredClone(task);
      if (params.includeArtifacts !== true) copy.artifacts = [];
      if (params.historyLength !== undefined) {
        if (!Number.isSafeInteger(params.historyLength) || params.historyLength < 0) {
          throw new RequestMalformedError("Invalid history length.");
        }
        copy.history = params.historyLength === 0 ? [] : copy.history.slice(-params.historyLength);
      }
      return copy;
    });
    const last = page.at(-1);
    return {
      tasks: page,
      nextPageToken: last !== undefined && tasks.length > page.length
        ? encodeTaskCursor(last.status?.timestamp ?? "", last.id)
        : "",
      pageSize,
      totalSize,
    };
  }

  public close(): void {
    this.#database.close();
  }

  #transaction(body: () => void): void {
    let begun = false;
    try {
      this.#database.exec("BEGIN IMMEDIATE");
      begun = true;
      body();
      this.#database.exec("COMMIT");
      begun = false;
    } catch (error) {
      if (begun) {
        try { this.#database.exec("ROLLBACK"); } catch { /* the database already failed closed */ }
      }
      if (error instanceof AgentIntegrationError || error instanceof RequestMalformedError) throw error;
      throw new AgentIntegrationError("service-refused");
    }
  }
}

export class LayerXA2AExecutor implements AgentExecutor {
  readonly #integration: LayerXAgentIntegration;

  public constructor(integration: LayerXAgentIntegration) {
    this.#integration = integration;
  }

  public execute = async (requestContext: RequestContext, eventBus: ExecutionEventBus): Promise<void> => {
    const scope = taskScope(requestContext.context);
    if (scope.tenant !== this.#integration.config.tenant || scope.owner !== this.#integration.config.actor) {
      throw new AgentIntegrationError("service-refused");
    }
    const request = bindToolRequest(
      toolRequest(requestContext.userMessage.parts),
      this.#integration.config,
    );
    const outcome = await this.#integration.tools.execute(request.tool, request.arguments);
    const response: Message = {
      messageId: globalThis.crypto.randomUUID(),
      contextId: requestContext.contextId,
      taskId: requestContext.taskId,
      role: Role.ROLE_AGENT,
      parts: [dataPart(renderOutcome(outcome))],
      metadata: undefined,
      extensions: [],
      referenceTaskIds: [],
    };
    eventBus.publish(AgentEvent.message(response));
    eventBus.finished();
  };

  public cancelTask = async (_taskId: string, eventBus: ExecutionEventBus): Promise<void> => {
    eventBus.finished();
    throw new UnsupportedOperationError();
  };
}

export function createA2AIntegration(options: LayerXA2AOptions): LayerXA2AIntegration {
  const taskStore = requireTaskStore(options.durableTaskStore);
  const authentication = requireAuthentication(options.authentication);
  const integration = createAgentIntegration(options);
  const executor = new LayerXA2AExecutor(integration);
  const agentCard = layerXAgentCard(
    endpoint(required(options.environment, "LAYERX_A2A_URL")), authentication);
  const coreHandler = new DefaultRequestHandler(agentCard, taskStore, executor);
  const requestHandler = authenticatedRequestHandler(coreHandler, authentication.authenticate);
  return {
    ...integration,
    agentCard,
    executor,
    requestHandler,
    executeToolRequest: async (request) => {
      const message = requestMessage(request);
      const response = await coreHandler.sendMessage(
        { tenant: integration.config.tenant, message, configuration: undefined, metadata: undefined },
        new ServerCallContext({
          tenant: integration.config.tenant,
          requestedVersion: "1.0",
          user: { isAuthenticated: true, userName: integration.config.actor },
        }),
      );
      if (!("role" in response)) throw new AgentIntegrationError("service-refused");
      return outcomeMessage(response);
    },
  };
}

export function layerXAgentCard(url: string, authentication: A2AAuthenticationBoundary): AgentCard {
  const requirement = {
    schemes: {
      [authentication.schemeName]: { list: [...(authentication.requiredScopes ?? [])] },
    },
  };
  return {
    name: "LayerX payment agent",
    description: "Receipt-verified LayerX spending, tracking and local receipt verification.",
    supportedInterfaces: [{ url: endpoint(url), protocolBinding: "JSONRPC", tenant: "", protocolVersion: "1.0" }],
    provider: { organization: "Sidiora", url: "https://layerx.network" },
    version: "0.1.0",
    capabilities: { streaming: false, pushNotifications: false, extensions: [], extendedAgentCard: false },
    securitySchemes: { [authentication.schemeName]: structuredClone(authentication.securityScheme) },
    securityRequirements: [requirement],
    defaultInputModes: ["application/json"],
    defaultOutputModes: ["application/json"],
    skills: [{
      id: "layerx-payments",
      name: "LayerX payments",
      description: "Spend with budget enforcement and local receipt verification, track submissions and verify receipts.",
      tags: ["payments", "receipts", "layerx"],
      examples: ["Call layerx_spend with a canonical activity and idempotency key."],
      inputModes: ["application/json"],
      outputModes: ["application/json"],
      securityRequirements: [requirement],
    }],
    signatures: [],
  };
}

function requireTaskStore(value: TaskStore): TaskStore {
  if (value === undefined || value === null
      || typeof value.save !== "function" || typeof value.load !== "function"
      || typeof value.list !== "function") {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return value;
}

function requireAuthentication(value: A2AAuthenticationBoundary): A2AAuthenticationBoundary {
  if (value === undefined || value === null
      || !/^[A-Za-z][A-Za-z0-9._-]{0,63}$/u.test(value.schemeName)
      || value.securityScheme === undefined || value.securityScheme.scheme === undefined
      || typeof value.authenticate !== "function") {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  const scopes = value.requiredScopes ?? [];
  if (scopes.length > 32 || scopes.some((scope) =>
    !/^[\x21-\x7e]{1,128}$/u.test(scope))) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  let encoded: string | undefined;
  try {
    encoded = JSON.stringify(value.securityScheme);
  } catch {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  if (encoded === undefined || encoded.length < 2 || encoded.length > 16 * 1024) {
    throw new AgentIntegrationError("invalid-declared-key");
  }
  return {
    schemeName: value.schemeName,
    securityScheme: structuredClone(value.securityScheme),
    requiredScopes: [...scopes],
    authenticate: value.authenticate,
  };
}

function authenticatedRequestHandler(
  delegate: A2ARequestHandler,
  verify: (context: ServerCallContext) => Promise<ServerCallContext>,
): A2ARequestHandler {
  const authenticated = async (context: ServerCallContext): Promise<ServerCallContext> => {
    const verified = await verify(context);
    if (verified === undefined || verified === null) {
      throw new AgentIntegrationError("service-refused");
    }
    const user = verified.user;
    if (user?.isAuthenticated !== true || user.userName.length === 0
        || user.userName.length > 512 || /[\u0000-\u001f\u007f]/u.test(user.userName)
        || verified.tenant !== context.tenant
        || verified.requestedVersion !== context.requestedVersion) {
      throw new AgentIntegrationError("service-refused");
    }
    return verified;
  };
  const handler: A2ARequestHandler = {
    getAgentCard: () => delegate.getAgentCard(),
    getAuthenticatedExtendedAgentCard: async (params, context) =>
      delegate.getAuthenticatedExtendedAgentCard(params, await authenticated(context)),
    sendMessage: async (params, context) =>
      delegate.sendMessage(params, await authenticated(context)),
    sendMessageStream: async function* (params, context) {
      yield* delegate.sendMessageStream(params, await authenticated(context));
    },
    getTask: async (params, context) =>
      delegate.getTask(params, await authenticated(context)),
    listTasks: async (params, context) =>
      delegate.listTasks(params, await authenticated(context)),
    cancelTask: async (params, context) =>
      delegate.cancelTask(params, await authenticated(context)),
    createTaskPushNotificationConfig: async (params, context) =>
      delegate.createTaskPushNotificationConfig(params, await authenticated(context)),
    getTaskPushNotificationConfig: async (params, context) =>
      delegate.getTaskPushNotificationConfig(params, await authenticated(context)),
    listTaskPushNotificationConfigs: async (params, context) =>
      delegate.listTaskPushNotificationConfigs(params, await authenticated(context)),
    deleteTaskPushNotificationConfig: async (params, context) =>
      delegate.deleteTaskPushNotificationConfig(params, await authenticated(context)),
    resubscribe: async function* (params, context) {
      yield* delegate.resubscribe(params, await authenticated(context));
    },
  };
  return handler;
}

function toolRequest(parts: readonly Part[]): A2AToolRequest {
  if (parts.length !== 1) throw new AgentIntegrationError("invalid-tool-input");
  const part = parts[0];
  if (part === undefined || part.content === undefined) throw new AgentIntegrationError("invalid-tool-input");
  let value: unknown;
  if (part.content.$case === "data") {
    value = part.content.value;
  } else if (part.content.$case === "text") {
    if (part.content.value.length > 4 * 1024 * 1024) throw new AgentIntegrationError("invalid-tool-input");
    try {
      value = JSON.parse(part.content.value);
    } catch {
      throw new AgentIntegrationError("invalid-tool-input");
    }
  } else {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  const request = value as Readonly<Record<string, unknown>>;
  if (typeof request["tool"] !== "string" || !("arguments" in request)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return { tool: request["tool"], arguments: request["arguments"] };
}

function dataPart(value: ToolJsonObject): Part {
  return {
    content: { $case: "data", value },
    metadata: undefined,
    filename: "",
    mediaType: "application/json",
  };
}

function requestMessage(request: A2AToolRequest): Message {
  let text: string;
  try {
    text = JSON.stringify({ tool: request.tool, arguments: request.arguments });
  } catch {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  if (text.length > 4 * 1024 * 1024) throw new AgentIntegrationError("invalid-tool-input");
  return {
    messageId: globalThis.crypto.randomUUID(),
    contextId: "",
    taskId: "",
    role: Role.ROLE_USER,
    parts: [{ content: { $case: "text", value: text }, metadata: undefined, filename: "", mediaType: "application/json" }],
    metadata: undefined,
    extensions: [],
    referenceTaskIds: [],
  };
}

function outcomeMessage(message: Message): ToolOutcome {
  if (message.role !== Role.ROLE_AGENT || message.parts.length !== 1) {
    throw new AgentIntegrationError("service-refused");
  }
  const part = message.parts[0];
  if (part === undefined || part.content?.$case !== "data") {
    throw new AgentIntegrationError("service-refused");
  }
  const value = part.content.value;
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new AgentIntegrationError("service-refused");
  }
  const outcome = value as Readonly<Record<string, unknown>>;
  if (outcome["ok"] === true && typeof outcome["tool"] === "string"
      && outcome["result"] !== null && typeof outcome["result"] === "object"
      && !Array.isArray(outcome["result"])) {
    return { ok: true, tool: outcome["tool"], result: outcome["result"] as ToolJsonObject };
  }
  if (outcome["ok"] === false && typeof outcome["tool"] === "string" && typeof outcome["code"] === "string") {
    return { ok: false, tool: outcome["tool"], code: outcome["code"] };
  }
  throw new AgentIntegrationError("service-refused");
}

function taskScope(context: ServerCallContext): { tenant: string; owner: string } {
  const tenant = context.tenant;
  const user = context.user;
  if (tenant === undefined || tenant.length === 0 || tenant.length > 512 || tenant.includes("\0")
      || user?.isAuthenticated !== true || user.userName.length === 0 || user.userName.length > 512
      || /[\u0000-\u001f\u007f]/u.test(user.userName)) {
    throw new AgentIntegrationError("service-refused");
  }
  return { tenant, owner: user.userName };
}

function bindToolRequest(
  request: A2AToolRequest,
  config: LayerXAgentIntegration["config"],
): A2AToolRequest {
  if (request.tool !== "layerx_spend") return request;
  if (request.arguments === null || typeof request.arguments !== "object" || Array.isArray(request.arguments)) {
    return request;
  }
  const input = request.arguments as Readonly<Record<string, unknown>>;
  const bindings = [
    ["tenant", config.tenant],
    ["actor", config.actor],
    ["authority", config.authority],
  ] as const;
  for (const [name, expected] of bindings) {
    if (input[name] !== undefined && input[name] !== expected) {
      throw new AgentIntegrationError("service-refused");
    }
  }
  return {
    tool: request.tool,
    arguments: {
      ...input,
      tenant: config.tenant,
      actor: config.actor,
      authority: config.authority,
    },
  };
}

function normalizedTask(task: Task): Task {
  requireTaskIdentifier(task.id);
  if (task.contextId.length === 0 || task.contextId.length > 512 || task.contextId.includes("\0")) {
    throw new AgentIntegrationError("service-refused");
  }
  let encoded: string;
  try {
    encoded = JSON.stringify(Task.toJSON(task));
  } catch {
    throw new AgentIntegrationError("service-refused");
  }
  if (Buffer.byteLength(encoded, "utf8") > 8 * 1024 * 1024) {
    throw new AgentIntegrationError("service-refused");
  }
  const normalized = Task.fromJSON(JSON.parse(encoded) as unknown);
  requireSupportedTask(normalized);
  return normalized;
}

function requireTaskIdentifier(value: string): void {
  if (value.length === 0 || value.length > 512 || value.includes("\0")) {
    throw new AgentIntegrationError("service-refused");
  }
}

function encodeTaskCursor(timestamp: string, id: string): string {
  return Buffer.from(`${timestamp}|${id}`, "utf8").toString("base64");
}

function decodeTaskCursor(value: string): { timestamp: string; id: string } {
  if (value.length > 2_048 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    throw new RequestMalformedError("Invalid page token.");
  }
  const decoded = Buffer.from(value, "base64").toString("utf8");
  const separator = decoded.indexOf("|");
  if (separator === -1) throw new RequestMalformedError("Invalid page token.");
  const timestamp = decoded.slice(0, separator);
  const id = decoded.slice(separator + 1);
  requireTaskIdentifier(id);
  return { timestamp, id };
}

const A2A_DATABASE_APPLICATION_ID = 0x4c584132;
const A2A_DATABASE_SCHEMA_VERSION = 1;
const A2A_DATABASE_MAXIMUM_BYTES = 64 * 1024 * 1024;
const A2A_TASK_MAXIMUM_BYTES = 8 * 1024 * 1024;
const A2A_TASK_TABLE_SQL = `CREATE TABLE layerx_a2a_tasks (
  tenant TEXT NOT NULL,
  owner TEXT NOT NULL,
  task_id TEXT NOT NULL,
  task_json TEXT NOT NULL CHECK (length(CAST(task_json AS BLOB)) <= ${A2A_TASK_MAXIMUM_BYTES}),
  revision INTEGER NOT NULL CHECK (revision >= 1 AND revision <= ${Number.MAX_SAFE_INTEGER}),
  PRIMARY KEY (tenant, owner, task_id)
) STRICT, WITHOUT ROWID`;

function initializeTaskDatabase(database: DatabaseSync): void {
  const applicationId = pragmaNumber(database, "application_id");
  const version = pragmaNumber(database, "user_version");
  const objects = database.prepare(
    "SELECT type, name FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
  ).all();
  if (applicationId === 0 && version === 0 && objects.length === 0) {
    try {
      database.exec(`
        PRAGMA page_size = 4096;
        BEGIN IMMEDIATE;
        ${A2A_TASK_TABLE_SQL};
        PRAGMA application_id = ${A2A_DATABASE_APPLICATION_ID};
        PRAGMA user_version = ${A2A_DATABASE_SCHEMA_VERSION};
        COMMIT;
      `);
    } catch (error) {
      if (database.isTransaction) {
        try { database.exec("ROLLBACK"); } catch { /* initialization already failed closed */ }
      }
      throw error;
    }
  } else if (applicationId !== A2A_DATABASE_APPLICATION_ID
      || version !== A2A_DATABASE_SCHEMA_VERSION
      || objects.length !== 1
      || objects[0]?.["type"] !== "table" || objects[0]?.["name"] !== "layerx_a2a_tasks") {
    throw new AgentIntegrationError("service-refused");
  }
  const schema = database.prepare(
    "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'layerx_a2a_tasks'",
  ).get();
  if (schema === undefined || schema["sql"] !== A2A_TASK_TABLE_SQL) {
    throw new AgentIntegrationError("service-refused");
  }
  database.exec(`
    PRAGMA trusted_schema = OFF;
    PRAGMA foreign_keys = ON;
    PRAGMA journal_mode = WAL;
    PRAGMA synchronous = FULL;
    PRAGMA temp_store = MEMORY;
    PRAGMA wal_autocheckpoint = 100;
    PRAGMA journal_size_limit = 8388608;
  `);
  const pageSize = pragmaNumber(database, "page_size");
  if (pageSize < 512 || pageSize > 65_536 || (pageSize & (pageSize - 1)) !== 0) {
    throw new AgentIntegrationError("service-refused");
  }
  const maximumPages = Math.floor(A2A_DATABASE_MAXIMUM_BYTES / pageSize);
  database.exec(`PRAGMA max_page_count = ${maximumPages}`);
  if (pragmaNumber(database, "max_page_count") !== maximumPages) {
    throw new AgentIntegrationError("service-refused");
  }
  const quickCheck = database.prepare("PRAGMA quick_check").all();
  if (quickCheck.length !== 1 || quickCheck[0]?.["quick_check"] !== "ok") {
    throw new AgentIntegrationError("service-refused");
  }
}

function pragmaNumber(database: DatabaseSync, name: string): number {
  const row = database.prepare(`PRAGMA ${name}`).get();
  const value = row?.[name];
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new AgentIntegrationError("service-refused");
  }
  return value;
}

function secureTaskDatabaseFiles(path: string): void {
  for (const candidate of [path, `${path}-wal`, `${path}-shm`]) {
    if (!existsSync(candidate)) continue;
    const metadata = lstatSync(candidate);
    if (!metadata.isFile() || metadata.isSymbolicLink()) {
      throw new AgentIntegrationError("service-refused");
    }
    chmodSync(candidate, 0o600);
  }
}

function encodeStoredTask(task: Task): string {
  const encoded = JSON.stringify(Task.toJSON(task));
  if (Buffer.byteLength(encoded, "utf8") > A2A_TASK_MAXIMUM_BYTES) {
    throw new AgentIntegrationError("service-refused");
  }
  return encoded;
}

function decodeStoredTask(encoded: string): Task {
  if (Buffer.byteLength(encoded, "utf8") > A2A_TASK_MAXIMUM_BYTES) {
    throw new AgentIntegrationError("service-refused");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(encoded);
  } catch {
    throw new AgentIntegrationError("service-refused");
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new AgentIntegrationError("service-refused");
  }
  const task = normalizedTask(Task.fromJSON(parsed));
  if (encodeStoredTask(task) !== encoded) {
    throw new AgentIntegrationError("service-refused");
  }
  return task;
}

function decodeTaskRow(row: Readonly<Record<string, unknown>>, expectedId?: string): Task {
  const storedId = row["task_id"];
  const encoded = row["task_json"];
  const revision = row["revision"];
  if (typeof storedId !== "string" || typeof encoded !== "string"
      || typeof revision !== "number" || !Number.isSafeInteger(revision) || revision < 1
      || (expectedId !== undefined && storedId !== expectedId)) {
    throw new AgentIntegrationError("service-refused");
  }
  const task = decodeStoredTask(encoded);
  if (task.id !== storedId || taskRevision(task) !== revision) {
    throw new AgentIntegrationError("service-refused");
  }
  return task;
}

function requireSupportedTask(task: Task): void {
  if (task.status !== undefined
      && (!Number.isInteger(task.status.state)
        || task.status.state < TaskState.TASK_STATE_UNSPECIFIED
        || task.status.state > TaskState.TASK_STATE_AUTH_REQUIRED)) {
    throw new AgentIntegrationError("service-refused");
  }
  taskTimestamp(task);
  const messageIds = new Set<string>();
  for (const message of task.history) {
    if (message.messageId.length === 0 || message.messageId.length > 512
        || message.messageId.includes("\0") || messageIds.has(message.messageId)) {
      throw new AgentIntegrationError("service-refused");
    }
    messageIds.add(message.messageId);
  }
  const artifactIds = new Set<string>();
  for (const artifact of task.artifacts) {
    if (artifact.artifactId.length === 0 || artifact.artifactId.length > 512
        || artifact.artifactId.includes("\0") || artifactIds.has(artifact.artifactId)) {
      throw new AgentIntegrationError("service-refused");
    }
    artifactIds.add(artifact.artifactId);
  }
}

const A2A_TASK_REVISION_KEY = "layerx.task-store.revision";

function taskRevision(task: Task): number | undefined {
  const value = task.metadata?.[A2A_TASK_REVISION_KEY];
  if (value === undefined) return undefined;
  if (typeof value !== "string" || !/^[1-9][0-9]{0,15}$/u.test(value)) {
    throw new AgentIntegrationError("service-refused");
  }
  const revision = Number(value);
  if (!Number.isSafeInteger(revision) || revision < 1) {
    throw new AgentIntegrationError("service-refused");
  }
  return revision;
}

function withTaskRevision(task: Task, revision: number): Task {
  if (!Number.isSafeInteger(revision) || revision < 1) {
    throw new AgentIntegrationError("service-refused");
  }
  const versioned = structuredClone(task);
  versioned.metadata = {
    ...(versioned.metadata ?? {}),
    [A2A_TASK_REVISION_KEY]: revision.toString(),
  };
  return normalizedTask(versioned);
}

function setTaskRevision(task: Task, revision: number): void {
  if (!Number.isSafeInteger(revision) || revision < 1) {
    throw new AgentIntegrationError("service-refused");
  }
  task.metadata = {
    ...(task.metadata ?? {}),
    [A2A_TASK_REVISION_KEY]: revision.toString(),
  };
  if (taskRevision(task) !== revision) throw new AgentIntegrationError("service-refused");
}

function encodeCanonical(value: unknown): string {
  return JSON.stringify(value);
}

function taskTimestamp(task: Task): number | undefined {
  const timestamp = task.status?.timestamp;
  if (timestamp === undefined) return undefined;
  const value = new Date(timestamp).getTime();
  if (!Number.isFinite(value)) throw new AgentIntegrationError("service-refused");
  return value;
}

function isTerminalState(state: TaskState | undefined): boolean {
  return state === TaskState.TASK_STATE_COMPLETED
    || state === TaskState.TASK_STATE_FAILED
    || state === TaskState.TASK_STATE_CANCELED
    || state === TaskState.TASK_STATE_REJECTED;
}
