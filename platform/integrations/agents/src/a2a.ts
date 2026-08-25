import {
  Role,
  type AgentCard,
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
import { UnsupportedOperationError } from "@a2a-js/sdk/errors";
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

export class LayerXA2AExecutor implements AgentExecutor {
  readonly #integration: LayerXAgentIntegration;

  public constructor(integration: LayerXAgentIntegration) {
    this.#integration = integration;
  }

  public execute = async (requestContext: RequestContext, eventBus: ExecutionEventBus): Promise<void> => {
    const request = toolRequest(requestContext.userMessage.parts);
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
          user: { isAuthenticated: true, userName: "layerx-in-process" },
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
