import assert from "node:assert/strict";
import test from "node:test";

import { csrfTokenFromCookie, singleCurrentSessionId } from "../src/auth/session.ts";

test("session identity requires exactly one server-verified current session", () => {
  assert.equal(singleCurrentSessionId([]), undefined);
  assert.equal(singleCurrentSessionId([{ session_id: "forged-cookie", current: false }]), undefined);
  assert.equal(singleCurrentSessionId([
    { session_id: "session-a", current: true },
    { session_id: "session-b", current: true },
  ]), undefined);
  assert.equal(singleCurrentSessionId([{ session_id: "session-a", current: true }]), "session-a");
});

test("csrf extraction is exact and refuses absent or empty tokens", () => {
  assert.equal(csrfTokenFromCookie("__Host-layerx-session=opaque"), undefined);
  assert.equal(csrfTokenFromCookie("__Host-layerx_csrf="), undefined);
  assert.equal(
    csrfTokenFromCookie("a=b; __Host-layerx_csrf=csrf-0123456789abcdef; c=d"),
    "csrf-0123456789abcdef",
  );
});
