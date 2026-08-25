import type { JsonValue } from "@sidiora/layerx-seller-middleware";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
  type CallToolResult,
  type Tool as McpTool,
} from "@modelcontextprotocol/sdk/types.js";
import { constants } from "node:fs";
import { chmod, mkdir, open, rename, unlink } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { AgentIntegrationError } from "./config.js";
import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import type { ToolDefinition, ToolJson, ToolJsonObject } from "./tools.js";
import type { AgentWebhookResponse, WebhookHeaderSource } from "./webhooks.js";

export const MCP_PROTOCOL_VERSION = "2025-06-18" as const;

export const SUPPORTED_MCP_PROTOCOL_VERSIONS: readonly string[] = [
  "2025-06-18",
  "2025-03-26",
  "2024-11-05",
];

export const MAXIMUM_MCP_LINE_BYTES = 8 * 1024 * 1024;

export const MAXIMUM_BUFFERED_EVENTS = 256;

export const EVENTS_TOOL: ToolDefinition = {
  name: "layerx_events",
  title: "Read verified LayerX events",
  description:
    "Return LayerX webhook events delivered to this server that passed Ed25519 signature verification and "
    + "replay protection. Deliveries that fail verification are rejected and never reach this queue.",
  inputSchema: {
    type: "object",
    additionalProperties: false,
    required: [],
    properties: {
      limit: {
        type: "integer",
        minimum: 1,
        maximum: MAXIMUM_BUFFERED_EVENTS,
        description: "Maximum number of verified events to drain; defaults to all buffered events.",
      },
    },
  },
};

export type JsonRpcId = string | number | null;

export interface JsonRpcSuccess {
  readonly jsonrpc: "2.0";
  readonly id: JsonRpcId;
  readonly result: ToolJsonObject;
}

export interface JsonRpcFailure {
  readonly jsonrpc: "2.0";
  readonly id: JsonRpcId;
  readonly error: {
    readonly code: number;
    readonly message: string;
  };
}

export type JsonRpcResponse = JsonRpcSuccess | JsonRpcFailure;

export const PARSE_ERROR = -32700;
export const INVALID_REQUEST = -32600;
export const METHOD_NOT_FOUND = -32601;
export const INVALID_PARAMS = -32602;
export const INTERNAL_ERROR = -32603;

export interface McpStreamTransport {
  readonly input: AsyncIterable<Uint8Array | string>;
  write(line: string): void | Promise<void>;
}

export interface LayerXMcpServerConfig {
  readonly integration: LayerXAgentIntegration;
  readonly name?: string;
  readonly version?: string;
  readonly bufferedEvents?: number;
  readonly eventQueue?: VerifiedEventBuffer;
}

export interface VerifiedEventBuffer {
  push(deliveryId: string, event: Readonly<Record<string, JsonValue>>): Promise<void>;
  drain(limit: number): Promise<readonly { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]>;
  count(): Promise<number>;
}

export class VerifiedEventQueue implements VerifiedEventBuffer {
  readonly #capacity: number;
  readonly #entries: { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[] = [];

  public constructor(capacity: number = MAXIMUM_BUFFERED_EVENTS) {
    if (!Number.isSafeInteger(capacity) || capacity < 1) {
      throw new AgentIntegrationError("invalid-declared-key");
    }
    this.#capacity = capacity;
  }

  public get size(): number {
    return this.#entries.length;
  }

  public push(deliveryId: string, event: Readonly<Record<string, JsonValue>>): Promise<void> {
    const existing = this.#entries.find((entry) => entry.deliveryId === deliveryId);
    if (existing !== undefined) {
      if (JSON.stringify(existing.event) !== JSON.stringify(event)) {
        return Promise.reject(new AgentIntegrationError("service-refused"));
      }
      return Promise.resolve();
    }
    this.#entries.push({ deliveryId, event });
    if (this.#entries.length > this.#capacity) {
      this.#entries.pop();
      return Promise.reject(new AgentIntegrationError("service-refused"));
    }
    return Promise.resolve();
  }

  public drain(limit: number): Promise<readonly { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]> {
    return Promise.resolve(this.#entries.splice(0, Math.max(0, Math.min(limit, this.#entries.length))));
  }

  public count(): Promise<number> {
    return Promise.resolve(this.#entries.length);
  }
}

interface VerifiedEventLedger {
  readonly version: 1;
  readonly entries: readonly { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[];
}

export class FileVerifiedEventQueue implements VerifiedEventBuffer {
  readonly #path: string;
  readonly #lockPath: string;
  readonly #capacity: number;

  public constructor(path: string, capacity = MAXIMUM_BUFFERED_EVENTS) {
    if (path.length === 0 || path.length > 4_096 || path.includes("\0")
        || !Number.isSafeInteger(capacity) || capacity < 1) {
      throw new AgentIntegrationError("invalid-declared-key");
    }
    this.#path = resolve(path);
    this.#lockPath = `${this.#path}.lock`;
    this.#capacity = capacity;
  }

  public push(deliveryId: string, event: Readonly<Record<string, JsonValue>>): Promise<void> {
    if (deliveryId.length === 0 || deliveryId.length > 255 || deliveryId.includes("\0")) {
      throw new AgentIntegrationError("service-refused");
    }
    return this.#mutate((entries) => {
      const existing = entries.find((entry) => entry.deliveryId === deliveryId);
      if (existing !== undefined) {
        if (JSON.stringify(existing.event) !== JSON.stringify(event)) {
          throw new AgentIntegrationError("service-refused");
        }
        return;
      }
      if (entries.length >= this.#capacity) throw new AgentIntegrationError("service-refused");
      entries.push({ deliveryId, event });
    });
  }

  public drain(limit: number): Promise<readonly { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]> {
    if (!Number.isSafeInteger(limit) || limit < 0) throw new AgentIntegrationError("invalid-tool-input");
    return this.#mutate((entries) => entries.splice(0, Math.min(limit, entries.length)));
  }

  public count(): Promise<number> {
    return this.#withLock(async () => (await this.#read()).length);
  }

  async #mutate<Result>(
    body: (entries: { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]) => Result,
  ): Promise<Result> {
    return this.#withLock(async () => {
      const entries = await this.#read();
      const result = body(entries);
      await this.#write(entries);
      return result;
    });
  }

  async #withLock<Result>(body: () => Promise<Result>): Promise<Result> {
    await mkdir(dirname(this.#path), { recursive: true, mode: 0o700 });
    await chmod(dirname(this.#path), 0o700);
    const lock = await acquireMcpFileLock(this.#lockPath);
    try {
      return await body();
    } catch (error) {
      if (error instanceof AgentIntegrationError) throw error;
      throw new AgentIntegrationError("service-refused");
    } finally {
      await lock.close();
      try {
        await unlink(this.#lockPath);
      } catch (error) {
        if (!isNodeFileError(error, "ENOENT")) throw new AgentIntegrationError("service-refused");
      }
    }
  }

  async #read(): Promise<{ deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]> {
    let encoded: Uint8Array;
    try {
      encoded = await readBoundedRegularFile(this.#path, 64 * 1024 * 1024);
    } catch (error) {
      if (isNodeFileError(error, "ENOENT")) return [];
      throw new AgentIntegrationError("service-refused");
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(new TextDecoder().decode(encoded));
    } catch {
      throw new AgentIntegrationError("service-refused");
    }
    if (!isVerifiedEventLedger(parsed)) throw new AgentIntegrationError("service-refused");
    if (parsed.entries.length > this.#capacity) throw new AgentIntegrationError("service-refused");
    return parsed.entries.map((entry) => ({ deliveryId: entry.deliveryId, event: entry.event }));
  }

  async #write(entries: readonly { deliveryId: string; event: Readonly<Record<string, JsonValue>> }[]): Promise<void> {
    const temporary = `${this.#path}.${globalThis.crypto.randomUUID()}.tmp`;
    const output = await open(temporary, "wx", 0o600);
    try {
      await output.writeFile(JSON.stringify({ version: 1, entries } satisfies VerifiedEventLedger), "utf8");
      await output.sync();
    } finally {
      await output.close();
    }
    try {
      await rename(temporary, this.#path);
    } catch (error) {
      await unlink(temporary).catch(() => undefined);
      throw error;
    }
    const directory = await open(dirname(this.#path), "r");
    try {
      await directory.sync();
    } finally {
      await directory.close();
    }
  }
}

export class LayerXMcpServer {
  readonly #integration: LayerXAgentIntegration;
  readonly #queue: VerifiedEventBuffer;
  readonly #name: string;
  readonly #version: string;
  #initialized = false;

  public constructor(config: LayerXMcpServerConfig) {
    this.#integration = config.integration;
    this.#queue = config.eventQueue ?? new FileVerifiedEventQueue(
      `${config.integration.config.webhookDeliveryStorePath}.mcp-events.json`,
      config.bufferedEvents ?? MAXIMUM_BUFFERED_EVENTS,
    );
    this.#name = config.name ?? "layerx";
    this.#version = config.version ?? "0.1.0";
  }

  public get integration(): LayerXAgentIntegration {
    return this.#integration;
  }

  public get events(): VerifiedEventBuffer {
    return this.#queue;
  }

  public get tools(): readonly ToolDefinition[] {
    return [...this.#integration.tools.definitions, EVENTS_TOOL];
  }

  public deliver(rawBody: Uint8Array, headers: WebhookHeaderSource): Promise<AgentWebhookResponse> {
    return this.#integration.webhooks.respond(rawBody, headers, {
      handle: async (event, deliveryId) => {
        await this.#queue.push(deliveryId, event);
      },
    });
  }

  public async handle(message: unknown): Promise<JsonRpcResponse | undefined> {
    if (!isObject(message) || message["jsonrpc"] !== "2.0" || typeof message["method"] !== "string") {
      return failure(identifier(message), INVALID_REQUEST, "invalid request");
    }
    const method = message["method"];
    const id = identifier(message);
    if (id === undefined) {
      if (method === "notifications/initialized") {
        this.#initialized = true;
      }
      return undefined;
    }
    try {
      return success(id, await this.#dispatch(method, message["params"]));
    } catch (error) {
      if (error instanceof McpMethodError) {
        return failure(id, error.rpcCode, error.message);
      }
      if (error instanceof AgentIntegrationError) {
        return failure(id, INVALID_PARAMS, error.code);
      }
      return failure(id, INTERNAL_ERROR, "internal error");
    }
  }

  public async serve(transport: McpStreamTransport): Promise<void> {
    const decoder = new TextDecoder();
    let buffer = "";
    for await (const chunk of transport.input) {
      buffer += typeof chunk === "string" ? chunk : decoder.decode(chunk, { stream: true });
      let newline = buffer.indexOf("\n");
      while (newline >= 0) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        await this.#serveLine(line, transport);
        newline = buffer.indexOf("\n");
      }
      if (buffer.length > MAXIMUM_MCP_LINE_BYTES) {
        await transport.write(`${JSON.stringify(failure(null, PARSE_ERROR, "message too large"))}\n`);
        buffer = "";
      }
    }
    const trailing = buffer.trim();
    if (trailing.length > 0) {
      await this.#serveLine(trailing, transport);
    }
  }

  async #serveLine(line: string, transport: McpStreamTransport): Promise<void> {
    const text = line.trim();
    if (text.length === 0) {
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(text);
    } catch {
      await transport.write(`${JSON.stringify(failure(null, PARSE_ERROR, "parse error"))}\n`);
      return;
    }
    if (Array.isArray(parsed)) {
      const responses: JsonRpcResponse[] = [];
      for (const entry of parsed) {
        const response = await this.handle(entry);
        if (response !== undefined) {
          responses.push(response);
        }
      }
      if (responses.length > 0) {
        await transport.write(`${JSON.stringify(responses)}\n`);
      }
      return;
    }
    const response = await this.handle(parsed);
    if (response !== undefined) {
      await transport.write(`${JSON.stringify(response)}\n`);
    }
  }

  async #dispatch(method: string, params: unknown): Promise<ToolJsonObject> {
    if (method === "initialize") {
      return this.#initialize(params);
    }
    if (method === "ping") {
      return {};
    }
    if (method === "tools/list") {
      return { tools: this.tools.map(describeTool) };
    }
    if (method === "tools/call") {
      return this.#call(params);
    }
    if (method === "layerx/webhook") {
      return this.#webhook(params);
    }
    throw new McpMethodError(METHOD_NOT_FOUND, `unknown method ${method}`);
  }

  #initialize(params: unknown): ToolJsonObject {
    const requested = isObject(params) ? params["protocolVersion"] : undefined;
    const version = typeof requested === "string" && SUPPORTED_MCP_PROTOCOL_VERSIONS.includes(requested)
      ? requested
      : MCP_PROTOCOL_VERSION;
    return {
      protocolVersion: version,
      capabilities: { tools: { listChanged: false } },
      serverInfo: { name: this.#name, version: this.#version },
      instructions:
        "Every LayerX spend returns only after its protocol receipt has been verified locally. "
        + "Treat a refusal as final and never retry a spend with a fresh idempotency key.",
    };
  }

  async #call(params: unknown): Promise<ToolJsonObject> {
    if (!isObject(params) || typeof params["name"] !== "string") {
      throw new McpMethodError(INVALID_PARAMS, "tools/call requires a tool name");
    }
    const name = params["name"];
    const input = params["arguments"] ?? {};
    if (name === EVENTS_TOOL.name) {
      return content(await this.#drain(input), false);
    }
    const outcome = await this.#integration.tools.execute(name, input);
    if (outcome.ok) {
      return content({ tool: outcome.tool, result: outcome.result }, false);
    }
    return content({ tool: outcome.tool, code: outcome.code }, true);
  }

  async #webhook(params: unknown): Promise<ToolJsonObject> {
    if (!isObject(params) || typeof params["body"] !== "string") {
      throw new McpMethodError(INVALID_PARAMS, "layerx/webhook requires a base64 body");
    }
    const headers = params["headers"];
    if (!isObject(headers)) {
      throw new McpMethodError(INVALID_PARAMS, "layerx/webhook requires delivery headers");
    }
    const normalized: Record<string, string> = {};
    for (const [name, value] of Object.entries(headers)) {
      if (typeof value !== "string") {
        throw new McpMethodError(INVALID_PARAMS, "delivery headers must be strings");
      }
      normalized[name.toLowerCase()] = value;
    }
    const response = await this.deliver(decodeBase64(params["body"]), normalized);
    return { status: response.status, body: response.body };
  }

  async #drain(input: unknown): Promise<ToolJsonObject> {
    const requested = isObject(input) ? input["limit"] : undefined;
    if (requested !== undefined && (typeof requested !== "number" || !Number.isSafeInteger(requested) || requested < 1)) {
      throw new McpMethodError(INVALID_PARAMS, "limit must be a positive integer");
    }
    const drained = await this.#queue.drain(requested === undefined ? MAXIMUM_BUFFERED_EVENTS : requested);
    const events: ToolJson[] = drained.map((entry) => ({ deliveryId: entry.deliveryId, event: entry.event }));
    return {
      tool: EVENTS_TOOL.name,
      result: { events, remaining: await this.#queue.count() },
    };
  }

  public get initialized(): boolean {
    return this.#initialized;
  }

  public drainVerifiedEvents(input: unknown): Promise<ToolJsonObject> {
    return this.#drain(input);
  }
}

export interface LayerXMcpIntegration extends LayerXAgentIntegration {
  readonly server: LayerXMcpServer;
  readonly officialServer: LayerXMcpSdkServer;
  connectStdio(): Promise<void>;
  callToolEmbedded(name: string, input: ToolJsonObject): Promise<CallToolResult>;
  closeMcp(): Promise<void>;
}

export class LayerXMcpSdkServer {
  readonly #legacy: LayerXMcpServer;
  readonly #server: Server;

  public constructor(legacy: LayerXMcpServer, name = "layerx", version = "0.1.0") {
    this.#legacy = legacy;
    this.#server = new Server(
      { name, version },
      {
        capabilities: { tools: { listChanged: false } },
        instructions:
          "LayerX spend tools reserve budget and report settlement only after local receipt verification. "
          + "Refusals are typed and a spend must never be retried with a fresh idempotency key.",
      },
    );
    this.#server.setRequestHandler(ListToolsRequestSchema, () => ({
      tools: this.#legacy.tools.map(sdkDescribeTool),
    }));
    this.#server.setRequestHandler(CallToolRequestSchema, async (request) => {
      const name = request.params.name;
      const input = request.params.arguments ?? {};
      if (name === EVENTS_TOOL.name) {
        return sdkContent(await this.#legacy.drainVerifiedEvents(input), false);
      }
      const outcome = await this.#legacy.integration.tools.execute(name, input);
      return outcome.ok
        ? sdkContent({ tool: outcome.tool, result: outcome.result }, false)
        : sdkContent({ tool: outcome.tool, code: outcome.code }, true);
    });
  }

  public connect(transport: Transport): Promise<void> {
    return this.#server.connect(transport);
  }

  public close(): Promise<void> {
    return this.#server.close();
  }

  public get protocolServer(): Server {
    return this.#server;
  }
}

export function createMcpIntegration(options: AgentIntegrationOptions): LayerXMcpIntegration {
  const integration = createAgentIntegration(options);
  const server = new LayerXMcpServer({ integration });
  const officialServer = new LayerXMcpSdkServer(server);
  let client: Client | undefined;
  let connection: Promise<void> | undefined;
  let connectionMode: "embedded" | "stdio" | undefined;
  const connectEmbedded = (): Promise<void> => {
    if (connectionMode === "stdio") throw new AgentIntegrationError("service-refused");
    if (connection !== undefined) return connection;
    connectionMode = "embedded";
    client = new Client({ name: "layerx-embedded-client", version: "0.1.0" });
    const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
    connection = Promise.all([
      officialServer.connect(serverTransport),
      client.connect(clientTransport),
    ]).then(() => undefined);
    return connection;
  };
  return {
    ...integration,
    server,
    officialServer,
    connectStdio: () => {
      if (connectionMode !== undefined) throw new AgentIntegrationError("service-refused");
      connectionMode = "stdio";
      connection = officialServer.connect(new StdioServerTransport());
      return connection;
    },
    callToolEmbedded: async (name, input) => {
      await connectEmbedded();
      if (client === undefined) throw new AgentIntegrationError("service-refused");
      return client.callTool({ name, arguments: input });
    },
    closeMcp: async () => {
      if (client !== undefined) await client.close();
      await officialServer.close();
    },
  };
}

class McpMethodError extends Error {
  public readonly rpcCode: number;

  public constructor(rpcCode: number, message: string) {
    super(message);
    this.name = "McpMethodError";
    this.rpcCode = rpcCode;
  }
}

function describeTool(definition: ToolDefinition): ToolJsonObject {
  return {
    name: definition.name,
    title: definition.title,
    description: definition.description,
    inputSchema: definition.inputSchema,
  };
}

function sdkDescribeTool(definition: ToolDefinition): McpTool {
  return {
    name: definition.name,
    title: definition.title,
    description: definition.description,
    inputSchema: definition.inputSchema,
  };
}

function content(structured: ToolJsonObject, isError: boolean): ToolJsonObject {
  return {
    content: [{ type: "text", text: JSON.stringify(structured) }],
    structuredContent: structured,
    isError,
  };
}

function sdkContent(structured: ToolJsonObject, isError: boolean): CallToolResult {
  return {
    content: [{ type: "text", text: JSON.stringify(structured) }],
    structuredContent: structured,
    isError,
  };
}

function success(id: JsonRpcId, result: ToolJsonObject): JsonRpcSuccess {
  return { jsonrpc: "2.0", id, result };
}

function failure(id: JsonRpcId | undefined, code: number, message: string): JsonRpcFailure {
  return { jsonrpc: "2.0", id: id ?? null, error: { code, message } };
}

function identifier(message: unknown): JsonRpcId | undefined {
  if (!isObject(message)) {
    return null;
  }
  const id = message["id"];
  if (typeof id === "string" || (typeof id === "number" && Number.isFinite(id)) || id === null) {
    return id;
  }
  return undefined;
}

function decodeBase64(value: string): Uint8Array {
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u.test(value)) {
    throw new AgentIntegrationError("unverifiable-body");
  }
  const binary = globalThis.atob(value);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

function isObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

async function acquireMcpFileLock(path: string): Promise<Awaited<ReturnType<typeof open>>> {
  for (let attempt = 0; attempt < 200; attempt += 1) {
    try {
      return await open(path, "wx", 0o600);
    } catch (error) {
      if (!isNodeFileError(error, "EEXIST")) throw new AgentIntegrationError("service-refused");
      await new Promise<void>((resolveWait) => setTimeout(resolveWait, Math.min(10 + attempt * 5, 250)));
    }
  }
  throw new AgentIntegrationError("service-refused");
}

async function readBoundedRegularFile(path: string, maximum: number): Promise<Uint8Array> {
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const metadata = await handle.stat();
    if (!metadata.isFile() || metadata.size > maximum) throw new AgentIntegrationError("service-refused");
    const chunks: Uint8Array[] = [];
    let total = 0;
    for (;;) {
      const chunk = new Uint8Array(Math.min(64 * 1024, maximum + 1 - total));
      const { bytesRead } = await handle.read(chunk, 0, chunk.length, null);
      if (bytesRead === 0) break;
      total += bytesRead;
      if (total > maximum) throw new AgentIntegrationError("service-refused");
      chunks.push(chunk.subarray(0, bytesRead));
    }
    const encoded = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      encoded.set(chunk, offset);
      offset += chunk.length;
    }
    return encoded;
  } finally {
    await handle.close();
  }
}

function isNodeFileError(error: unknown, code: string): boolean {
  return error instanceof Error
    && "code" in error
    && (error as Error & { readonly code: unknown }).code === code;
}

function isVerifiedEventLedger(value: unknown): value is VerifiedEventLedger {
  if (!isObject(value) || value["version"] !== 1 || !Array.isArray(value["entries"])) return false;
  const deliveryIds = new Set<string>();
  for (const untrusted of value["entries"]) {
    if (!isObject(untrusted)) return false;
    const deliveryId = untrusted["deliveryId"];
    const event = untrusted["event"];
    if (typeof deliveryId !== "string" || deliveryId.length === 0 || deliveryId.length > 255
        || deliveryId.includes("\0") || deliveryIds.has(deliveryId) || !isJsonObject(event)) return false;
    deliveryIds.add(deliveryId);
  }
  return true;
}

function isJsonObject(value: unknown): value is Readonly<Record<string, JsonValue>> {
  if (!isObject(value)) return false;
  return Object.values(value).every(isJsonValue);
}

function isJsonValue(value: unknown): value is JsonValue {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value)) return value.every(isJsonValue);
  return isJsonObject(value);
}
