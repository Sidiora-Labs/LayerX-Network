import { defineConfig } from "@playwright/test";

import { SHELL_PROFILES, human_test_harness } from "./e2e/harness.ts";

const harness = human_test_harness(process.env);

export default defineConfig({
  testDir: "./e2e",
  testMatch: [
    "browser/**/*.spec.ts",
    "approvals.spec.ts",
    "explorer.spec.ts",
    "move.spec.ts",
    "onboarding.spec.ts",
    "support.spec.ts",
  ],
  outputDir: harness.traceDirectory,
  fullyParallel: false,
  forbidOnly: true,
  retries: process.env.CI === "true" ? 2 : 0,
  reporter: [["line"], ["html", { open: "never", outputFolder: "test-results/report" }]],
  snapshotPathTemplate: "{testDir}/__screenshots__/{projectName}/{arg}{ext}",
  ...(harness.localProduction
    ? {
        webServer: {
          command: "npm run start -- --hostname 127.0.0.1 --port 3105",
          url: harness.baseUrl,
          reuseExistingServer: false,
          timeout: 120_000,
        },
      }
    : {}),
  use: {
    baseURL: harness.baseUrl,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    ...Object.values(SHELL_PROFILES).map((profile) => ({
      name: profile.projectName,
      use: {
        viewport: profile.viewport,
        hasTouch: profile.hasTouch,
        isMobile: profile.isMobile,
      },
    })),
    {
      name: "touch-tablet-shell",
      testMatch: "browser/a11y.spec.ts",
      use: {
        viewport: { width: 1024, height: 1366 },
        hasTouch: true,
        isMobile: true,
      },
    },
  ],
});
