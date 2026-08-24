#!/usr/bin/env node
import { AgentIntegrationError } from "./config.js";
import { createMcpIntegration } from "./mcp.js";

interface NodeProcess {
  readonly env: Readonly<Record<string, string | undefined>>;
  readonly stderr: { write(chunk: string): boolean };
  exitCode: number;
  once(signal: "SIGINT" | "SIGTERM", listener: () => void): void;
}

function runtime(): NodeProcess {
  const scope = globalThis as { readonly process?: NodeProcess };
  if (scope.process === undefined) throw new AgentIntegrationError("client-runtime-refused");
  return scope.process;
}

function describeFailure(error: unknown): string {
  if (error instanceof AgentIntegrationError) return error.code;
  return error instanceof Error ? error.name : "unknown-failure";
}

async function main(): Promise<void> {
  const host = runtime();
  const integration = createMcpIntegration({ environment: host.env });
  const shutdown = (): void => {
    void integration.closeMcp().finally(() => integration.destroy());
  };
  host.once("SIGINT", shutdown);
  host.once("SIGTERM", shutdown);
  await integration.connectStdio();
}

main().catch((error: unknown) => {
  const host = runtime();
  host.stderr.write(`layerx-mcp-server: ${describeFailure(error)}\n`);
  host.exitCode = 1;
});
