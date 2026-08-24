import { AgentIntegrationError } from "./config.js";
import { DynamicStructuredTool } from "@langchain/core/tools";
import type { JSONSchema } from "@langchain/core/utils/json_schema";
import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import { renderOutcome, type AgentToolExecutor, type ToolDefinition } from "./tools.js";

export type LangChainToolSpec = DynamicStructuredTool<JSONSchema>;

export interface LayerXLangChainIntegration extends LayerXAgentIntegration {
  readonly langChainTools: readonly LangChainToolSpec[];
  langChainTool(name: string): LangChainToolSpec;
}

export function langChainToolSpec(executor: AgentToolExecutor, definition: ToolDefinition): LangChainToolSpec {
  const run = async (input: unknown): Promise<string> => {
    const outcome = await executor.execute(definition.name, input ?? {});
    return JSON.stringify(renderOutcome(outcome));
  };
  return new DynamicStructuredTool<JSONSchema>({
    name: definition.name,
    description: definition.description,
    schema: langChainSchema(definition.inputSchema),
    returnDirect: false,
    func: run,
  });
}

function langChainSchema(value: Readonly<Record<string, unknown>>): JSONSchema {
  const type = value["type"];
  const description = optionalString(value, "description");
  if (type === "object") {
    requireOnly(value, ["type", "description", "properties", "required", "additionalProperties"]);
    const untrustedProperties = value["properties"];
    if (untrustedProperties === null || typeof untrustedProperties !== "object"
        || Array.isArray(untrustedProperties)) throw new AgentIntegrationError("invalid-tool-input");
    const properties: Record<string, JSONSchema> = {};
    for (const [name, property] of Object.entries(untrustedProperties)) {
      if (property === null || typeof property !== "object" || Array.isArray(property)) {
        throw new AgentIntegrationError("invalid-tool-input");
      }
      properties[name] = langChainSchema(property as Readonly<Record<string, unknown>>);
    }
    const required = optionalStringArray(value, "required");
    const additionalProperties = value["additionalProperties"];
    if (additionalProperties !== undefined && typeof additionalProperties !== "boolean") {
      throw new AgentIntegrationError("invalid-tool-input");
    }
    return {
      type: "object",
      properties,
      ...(description === undefined ? {} : { description }),
      ...(required === undefined ? {} : { required }),
      ...(additionalProperties === undefined ? {} : { additionalProperties }),
    };
  }
  if (type === "string") {
    requireOnly(value, ["type", "description", "pattern", "minLength", "maxLength"]);
    const pattern = optionalString(value, "pattern");
    const minLength = optionalNonnegativeInteger(value, "minLength");
    const maxLength = optionalNonnegativeInteger(value, "maxLength");
    return {
      type: "string",
      ...(description === undefined ? {} : { description }),
      ...(pattern === undefined ? {} : { pattern }),
      ...(minLength === undefined ? {} : { minLength }),
      ...(maxLength === undefined ? {} : { maxLength }),
    };
  }
  if (type === "integer" || type === "number") {
    requireOnly(value, ["type", "description", "minimum", "maximum", "multipleOf"]);
    const minimum = optionalFiniteNumber(value, "minimum");
    const maximum = optionalFiniteNumber(value, "maximum");
    const multipleOf = optionalFiniteNumber(value, "multipleOf");
    return {
      type,
      ...(description === undefined ? {} : { description }),
      ...(minimum === undefined ? {} : { minimum }),
      ...(maximum === undefined ? {} : { maximum }),
      ...(multipleOf === undefined ? {} : { multipleOf }),
    };
  }
  if (type === "boolean") {
    requireOnly(value, ["type", "description"]);
    return { type: "boolean", ...(description === undefined ? {} : { description }) };
  }
  throw new AgentIntegrationError("invalid-tool-input");
}

function optionalString(value: Readonly<Record<string, unknown>>, name: string): string | undefined {
  const field = value[name];
  if (field === undefined) return undefined;
  if (typeof field !== "string") throw new AgentIntegrationError("invalid-tool-input");
  return field;
}

function optionalStringArray(
  value: Readonly<Record<string, unknown>>,
  name: string,
): string[] | undefined {
  const field = value[name];
  if (field === undefined) return undefined;
  if (!Array.isArray(field) || !field.every((entry) => typeof entry === "string")) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return [...field];
}

function optionalFiniteNumber(value: Readonly<Record<string, unknown>>, name: string): number | undefined {
  const field = value[name];
  if (field === undefined) return undefined;
  if (typeof field !== "number" || !Number.isFinite(field)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return field;
}

function optionalNonnegativeInteger(value: Readonly<Record<string, unknown>>, name: string): number | undefined {
  const field = optionalFiniteNumber(value, name);
  if (field !== undefined && (!Number.isSafeInteger(field) || field < 0)) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  return field;
}

function requireOnly(value: Readonly<Record<string, unknown>>, allowed: readonly string[]): void {
  const names = new Set(allowed);
  if (Object.keys(value).some((name) => !names.has(name))) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
}

export function createLangChainIntegration(options: AgentIntegrationOptions): LayerXLangChainIntegration {
  const integration = createAgentIntegration(options);
  const tools = integration.tools.definitions.map((definition) => langChainToolSpec(integration.tools, definition));
  return {
    ...integration,
    langChainTools: tools,
    langChainTool: (name) => {
      const tool = tools.find((candidate) => candidate.name === name);
      if (tool === undefined) {
        throw new AgentIntegrationError("unknown-tool");
      }
      return tool;
    },
  };
}
