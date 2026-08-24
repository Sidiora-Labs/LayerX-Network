import { AgentIntegrationError } from "./config.js";
import type {
  ChatCompletionFunctionTool,
  ChatCompletionMessageFunctionToolCall,
  ChatCompletionToolMessageParam,
} from "openai/resources/chat/completions";
import {
  createAgentIntegration,
  type AgentIntegrationOptions,
  type LayerXAgentIntegration,
} from "./integration.js";
import { refusalCode, renderOutcome, type ToolDefinition } from "./tools.js";

export type OpenAiFunctionTool = ChatCompletionFunctionTool;

export type OpenAiToolCall = Omit<ChatCompletionMessageFunctionToolCall, "type"> & {
  readonly type?: "function";
};

export type OpenAiToolMessage = ChatCompletionToolMessageParam;

export interface LayerXOpenAiIntegration extends LayerXAgentIntegration {
  readonly openAiTools: readonly OpenAiFunctionTool[];
  handleToolCall(call: OpenAiToolCall): Promise<OpenAiToolMessage>;
  handleToolCalls(calls: readonly OpenAiToolCall[]): Promise<readonly OpenAiToolMessage[]>;
}

export function openAiTool(definition: ToolDefinition): OpenAiFunctionTool {
  return {
    type: "function",
    function: {
      name: definition.name,
      description: definition.description,
      parameters: definition.inputSchema,
    },
  };
}

export function openAiTools(definitions: readonly ToolDefinition[]): readonly OpenAiFunctionTool[] {
  return definitions.map(openAiTool);
}

export function createOpenAiIntegration(options: AgentIntegrationOptions): LayerXOpenAiIntegration {
  const integration = createAgentIntegration(options);
  const handleToolCall = async (call: OpenAiToolCall): Promise<OpenAiToolMessage> => {
    let input: unknown;
    try {
      input = parseArguments(call.function.arguments);
    } catch (error) {
      return {
        role: "tool",
        tool_call_id: call.id,
        content: JSON.stringify({ ok: false, tool: call.function.name, code: refusalCode(error) }),
      };
    }
    const outcome = await integration.tools.execute(call.function.name, input);
    return {
      role: "tool",
      tool_call_id: call.id,
      content: JSON.stringify(renderOutcome(outcome)),
    };
  };
  return {
    ...integration,
    openAiTools: openAiTools(integration.tools.definitions),
    handleToolCall,
    handleToolCalls: async (calls) => {
      const messages: OpenAiToolMessage[] = [];
      for (const call of calls) {
        messages.push(await handleToolCall(call));
      }
      return messages;
    },
  };
}

function parseArguments(value: string): unknown {
  if (value.length === 0) {
    return {};
  }
  if (value.length > 4 * 1024 * 1024) {
    throw new AgentIntegrationError("invalid-tool-input");
  }
  try {
    return JSON.parse(value);
  } catch {
    throw new AgentIntegrationError("invalid-tool-input");
  }
}
