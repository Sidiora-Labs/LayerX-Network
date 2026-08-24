import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import type { Tool, ToolResultBlockParam, ToolUseBlockParam } from "@anthropic-ai/sdk/resources/messages";
import { renderOutcome, type ToolDefinition } from "./tools.js";

export type AnthropicTool = Tool;

export type AnthropicToolUseBlock = ToolUseBlockParam;

export type AnthropicToolResultBlock = ToolResultBlockParam & {
  readonly content: string;
  readonly is_error: boolean;
};

export interface LayerXAnthropicIntegration extends LayerXAgentIntegration {
  readonly anthropicTools: readonly AnthropicTool[];
  handleToolUse(block: AnthropicToolUseBlock): Promise<AnthropicToolResultBlock>;
  handleToolUseBlocks(content: readonly unknown[]): Promise<readonly AnthropicToolResultBlock[]>;
}

export function anthropicTool(definition: ToolDefinition): AnthropicTool {
  return {
    name: definition.name,
    description: definition.description,
    input_schema: definition.inputSchema,
  };
}

export function anthropicTools(definitions: readonly ToolDefinition[]): readonly AnthropicTool[] {
  return definitions.map(anthropicTool);
}

export function isToolUseBlock(value: unknown): value is AnthropicToolUseBlock {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const block = value as Record<string, unknown>;
  return block["type"] === "tool_use"
    && typeof block["id"] === "string"
    && typeof block["name"] === "string"
    && "input" in block;
}

export function createAnthropicIntegration(options: AgentIntegrationOptions): LayerXAnthropicIntegration {
  const integration = createAgentIntegration(options);
  const handleToolUse = async (block: AnthropicToolUseBlock): Promise<AnthropicToolResultBlock> => {
    const outcome = await integration.tools.execute(block.name, block.input ?? {});
    return {
      type: "tool_result",
      tool_use_id: block.id,
      content: JSON.stringify(renderOutcome(outcome)),
      is_error: !outcome.ok,
    };
  };
  return {
    ...integration,
    anthropicTools: anthropicTools(integration.tools.definitions),
    handleToolUse,
    handleToolUseBlocks: async (content) => {
      const results: AnthropicToolResultBlock[] = [];
      for (const block of content) {
        if (isToolUseBlock(block)) {
          results.push(await handleToolUse(block));
        }
      }
      return results;
    },
  };
}
