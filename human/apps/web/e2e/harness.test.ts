import assert from "node:assert/strict";
import test from "node:test";

import { SHELL_PROFILES, human_test_harness } from "./harness.ts";

test("the browser harness refuses anything except an explicitly real service", () => {
  assert.throws(() => human_test_harness({}), /HUMAN_E2E_REAL_STACK/);
  assert.throws(
    () => human_test_harness({ HUMAN_E2E_REAL_STACK: "0", HUMAN_E2E_BASE_URL: "http://127.0.0.1:3000" }),
    /substitutes are not accepted/,
  );
});

test("the real-stack harness exposes canonical mobile and desktop profiles", () => {
  const harness = human_test_harness({
    HUMAN_E2E_REAL_STACK: "1",
    HUMAN_E2E_BASE_URL: "http://127.0.0.1:3000",
  });
  assert.equal(harness.realStack, true);
  assert.equal(SHELL_PROFILES.mobile.viewport.width, 390);
  assert.equal(SHELL_PROFILES.desktop.viewport.width, 1440);
  assert.equal(SHELL_PROFILES.mobile.hasTouch, true);
  assert.equal(SHELL_PROFILES.desktop.hasTouch, false);
});

test("local production requires the real HTTPS origin and explicit trust material", () => {
  const environment = {
    HUMAN_E2E_REAL_STACK: "1",
    HUMAN_E2E_LOCAL_PRODUCTION: "1",
    HUMAN_E2E_BASE_URL: "https://app.paxeer.network",
    HUMAN_E2E_TLS_CONFIG: "/qualification/tls.json",
    HUMAN_E2E_BROWSER_HOME: "/qualification/browser-home",
  };
  const harness = human_test_harness(environment);
  assert.equal(harness.baseUrl, "https://app.paxeer.network/");
  assert.equal(harness.browserHome, environment.HUMAN_E2E_BROWSER_HOME);
  for (const url of ["http://127.0.0.1:3105", "https://app.paxeer.network:3105",
    "https://app.paxeer.network/path", "https://app.paxeer.network?query",
    "https://app.paxeer.network#fragment"]) {
    assert.throws(() => human_test_harness({ ...environment, HUMAN_E2E_BASE_URL: url }), /HTTPS application origin/);
  }
  assert.throws(() => human_test_harness({ ...environment, HUMAN_E2E_TLS_CONFIG: undefined }), /HUMAN_E2E_TLS_CONFIG/);
  assert.throws(() => human_test_harness({ ...environment, HUMAN_E2E_BROWSER_HOME: undefined }), /HUMAN_E2E_BROWSER_HOME/);
});
