import { jsonSchema, tool, type Tool, type ToolSet } from "ai";
import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import { renderOutcome, type AgentToolExecutor, type ToolDefinition, type ToolJsonObject } from "./tools.js";

export type VercelAiTool = Tool<unknown, ToolJsonObject> & {
  readonly parameters: ToolJsonObject;
  execute(input: unknown): PromiseLike<ToolJsonObject>;
};

export type VercelAiToolSet = ToolSet & Readonly<Record<string, VercelAiTool>>;

export interface LayerXVercelAiIntegration extends LayerXAgentIntegration {
  readonly vercelAiTools: VercelAiToolSet;
}

export function vercelAiTool(executor: AgentToolExecutor, definition: ToolDefinition): VercelAiTool {
  const official = tool({
    type: "function",
    description: definition.description,
    inputSchema: jsonSchema(definition.inputSchema),
    execute: async (input) => renderOutcome(await executor.execute(definition.name, input ?? {})),
  });
  return Object.assign(official, { parameters: definition.inputSchema });
}

export function vercelAiToolSet(executor: AgentToolExecutor): VercelAiToolSet {
  const tools: Record<string, VercelAiTool> = {};
  for (const definition of executor.definitions) {
    tools[definition.name] = vercelAiTool(executor, definition);
  }
  return tools;
}

export function createVercelAiIntegration(options: AgentIntegrationOptions): LayerXVercelAiIntegration {
  const integration = createAgentIntegration(options);
  return {
    ...integration,
    vercelAiTools: vercelAiToolSet(integration.tools),
  };
}
