import {
  Role,
  type AgentCard,
  type Message,
  type Part,
} from "@a2a-js/sdk";
import {
  AgentEvent,
  DefaultRequestHandler,
  InMemoryTaskStore,
  ServerCallContext,
  type A2ARequestHandler,
  type AgentExecutor,
  type ExecutionEventBus,
  type RequestContext,
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

export interface LayerXA2AIntegration extends LayerXAgentIntegration {
  readonly agentCard: AgentCard;
  readonly executor: LayerXA2AExecutor;
  readonly requestHandler: A2ARequestHandler;
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

export function createA2AIntegration(options: AgentIntegrationOptions): LayerXA2AIntegration {
  const integration = createAgentIntegration(options);
  const executor = new LayerXA2AExecutor(integration);
  const agentCard = layerXAgentCard(endpoint(required(options.environment, "LAYERX_A2A_URL")));
  const requestHandler = new DefaultRequestHandler(agentCard, new InMemoryTaskStore(), executor);
  return {
    ...integration,
    agentCard,
    executor,
    requestHandler,
    executeToolRequest: async (request) => {
      const message = requestMessage(request);
      const response = await requestHandler.sendMessage(
        { tenant: integration.config.tenant, message, configuration: undefined, metadata: undefined },
        new ServerCallContext({ tenant: integration.config.tenant, requestedVersion: "1.0" }),
      );
      if (!("role" in response)) throw new AgentIntegrationError("service-refused");
      return outcomeMessage(response);
    },
  };
}

export function layerXAgentCard(url: string): AgentCard {
  return {
    name: "LayerX payment agent",
    description: "Receipt-verified LayerX spending, tracking and local receipt verification.",
    supportedInterfaces: [{ url: endpoint(url), protocolBinding: "JSONRPC", tenant: "", protocolVersion: "1.0" }],
    provider: { organization: "Sidiora", url: "https://layerx.network" },
    version: "0.1.0",
    capabilities: { streaming: false, pushNotifications: false, extensions: [], extendedAgentCard: false },
    securitySchemes: {},
    securityRequirements: [],
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
      securityRequirements: [],
    }],
    signatures: [],
  };
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
