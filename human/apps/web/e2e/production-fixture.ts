import { spawn } from "node:child_process";
import { writeSync } from "node:fs";
import { mkdtemp, open } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { test as base } from "@playwright/test";

import { human_test_harness } from "./harness.ts";
import { establishPublicSession } from "./public-session.ts";

const WEB_ROOT = fileURLToPath(new URL("../", import.meta.url));

export const test = base.extend<{
  productionServer: void;
  authenticatedSession: Awaited<ReturnType<typeof establishPublicSession>>;
}>({
  authenticatedSession: async ({ context }, use) => {
    const session = await establishPublicSession(context, human_test_harness(process.env).baseUrl);
    await use(session);
  },
  productionServer: [async ({}, use, testInfo) => {
    const harness = human_test_harness(process.env);
    if (!harness.localProduction) {
      await use();
      return;
    }
    const directory = await mkdtemp(path.join(WEB_ROOT, ".next", "rum-test-"));
    const logPath = path.join(directory, "server.log");
    const log = await open(logPath, "wx", 0o600);
    const child = spawn(process.execPath, [
      path.join(WEB_ROOT, "node_modules/next/dist/bin/next"),
      "start", "--hostname", "127.0.0.1", "--port", "3105",
    ], {
      cwd: WEB_ROOT,
      env: { ...process.env, LAYERX_RUM_STORAGE_DIRECTORY: path.join(directory, "records") },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const exited = new Promise<void>((resolve) => { child.once("close", () => { resolve(); }); });
    let readyTimer: ReturnType<typeof setTimeout> | undefined;
    try {
      await new Promise<void>((resolve, reject) => {
        let output = "";
        child.stdout.on("data", (chunk: Buffer) => {
          writeSync(log.fd, chunk);
          output = (output + chunk.toString("utf8")).slice(-4096);
          if (/Ready in /u.test(output)) resolve();
        });
        child.stderr.on("data", (chunk: Buffer) => { writeSync(log.fd, chunk); });
        child.once("error", reject);
        child.once("exit", (code, signal) => {
          reject(new Error(`Production server exited before readiness: ${String(code)} ${String(signal)}`));
        });
        readyTimer = setTimeout(() => {
          reject(new Error("Production server did not become ready"));
        }, 15_000);
      });
      clearTimeout(readyTimer);
      await use();
    } finally {
      clearTimeout(readyTimer);
      if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
      const killTimer = setTimeout(() => { child.kill("SIGKILL"); }, 5_000);
      await exited;
      clearTimeout(killTimer);
      await log.close();
      await testInfo.attach("production-server", { path: logPath, contentType: "text/plain" });
    }
  }, { auto: true }],
});
