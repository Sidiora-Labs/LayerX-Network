import { defineConfig } from "@playwright/test";

import baseConfig from "./playwright.config";

const performanceConfig = { ...baseConfig };
delete performanceConfig.webServer;

export default defineConfig(performanceConfig, {
  testDir: "./e2e",
  testMatch: "perf.spec.ts",
});
