import { AgentMiddleware } from "@sidiora/layerx-agent-middleware";
import { ProductionClient, type SecretBytes } from "@sidiora/layerx-sdk";
import type { WebhookDeliveryStore } from "@sidiora/layerx-seller-middleware";
import {
  readDeclaredConfig,
  readServiceToken,
  type AgentDeclaredConfig,
  type Environment,
} from "./config.js";
import {
  LayerXAgentTransport,
  LayerXBudgetLedger,
  LayerXReceiptResolver,
  LayerXRemoteSigner,
  LayerXServiceEndpoint,
} from "./services.js";
import { AgentToolExecutor } from "./tools.js";
import { AgentWebhookGateway } from "./webhooks.js";

export const AGENT_FRAMEWORKS = ["mcp", "a2a", "openai", "anthropic", "langchain", "vercel-ai"] as const;

export type AgentFramework = (typeof AGENT_FRAMEWORKS)[number];

export interface AgentIntegrationOptions {
  readonly environment: Environment;
  readonly deliveries?: WebhookDeliveryStore;
  readonly now?: () => number;
  readonly fetch?: typeof globalThis.fetch;
  readonly wait?: (milliseconds: number) => Promise<void>;
}

export interface LayerXAgentIntegration {
  readonly config: AgentDeclaredConfig;
  readonly client: ProductionClient;
  readonly middleware: AgentMiddleware;
  readonly tools: AgentToolExecutor;
  readonly webhooks: AgentWebhookGateway;
  destroy(): void;
}

export function createAgentIntegration(options: AgentIntegrationOptions): LayerXAgentIntegration {
  const config = readDeclaredConfig(options.environment);
  const token: SecretBytes = readServiceToken(options.environment);
  const endpoint = (url: string): LayerXServiceEndpoint => new LayerXServiceEndpoint({
    url,
    token,
    timeoutMs: config.requestTimeoutMs,
    ...(options.fetch === undefined ? {} : { fetch: options.fetch }),
  });
  const client = new ProductionClient(new LayerXAgentTransport(endpoint(config.agentRpcUrl)));
  const receipts = new LayerXReceiptResolver(endpoint(config.receiptServiceUrl));
  const middleware = new AgentMiddleware({
    client,
    budgets: new LayerXBudgetLedger(endpoint(config.budgetServiceUrl)),
    signer: new LayerXRemoteSigner(endpoint(config.signerServiceUrl)),
    receipts,
    maximumTrackPolls: config.maximumTrackPolls,
    ...(options.wait === undefined ? {} : { wait: options.wait }),
  });
  const webhooks = new AgentWebhookGateway({
    webhook: config.webhook,
    deliveryStorePath: config.webhookDeliveryStorePath,
    ...(options.deliveries === undefined ? {} : { deliveries: options.deliveries }),
    ...(options.now === undefined ? {} : { now: options.now }),
  });
  return {
    config,
    client,
    middleware,
    tools: new AgentToolExecutor({ middleware, client, receipts, config }),
    webhooks,
    destroy: () => {
      token.destroy();
    },
  };
}

export function platform_int_agent_frameworks(): "receipt-verified-agent-framework-integrations" {
  return "receipt-verified-agent-framework-integrations";
}
