import assert from "node:assert/strict";
import test from "node:test";

import { csrfTokenFromCookie, singleCurrentSessionId } from "../src/auth/session.ts";
import { requestHasWebOrigin } from "../src/security/origin.ts";

test("web origin binds the configured public endpoint through an internal server URL", () => {
  const request = new Request("http://localhost:3105/api/performance/vitals", {
    headers: { Origin: "https://app.example.com", "Sec-Fetch-Site": "same-origin" },
  });
  assert.equal(requestHasWebOrigin(request, "https://app.example.com"), true);
  assert.equal(requestHasWebOrigin(request, "https://other.example.com"), false);
  assert.equal(requestHasWebOrigin(request, undefined), false);
  assert.equal(requestHasWebOrigin(request, "https://app.example.com/"), false);
  assert.equal(requestHasWebOrigin(request, "https://user@app.example.com"), false);
});

test("web origin refuses forged forwarding headers and noncanonical or cross-site origins", () => {
  for (const origin of [
    "https://other.example.com", "https://app.example.com:8443", "https://app.example.com/",
    "https://user@app.example.com", "https://app.example.com?x=1", "null", "",
  ]) {
    const request = new Request("http://localhost:3105/api/performance/vitals", {
      headers: {
        Origin: origin, "Sec-Fetch-Site": "same-origin",
        Host: "app.example.com", "X-Forwarded-Host": "app.example.com", "X-Forwarded-Proto": "https",
      },
    });
    assert.equal(requestHasWebOrigin(request, "https://app.example.com"), false, origin);
  }
  for (const site of ["cross-site", "same-site", "none", ""]) {
    const request = new Request("http://localhost:3105/api/performance/vitals", {
      headers: { Origin: "https://app.example.com", "Sec-Fetch-Site": site },
    });
    assert.equal(requestHasWebOrigin(request, "https://app.example.com"), false, site);
  }
});

test("unencrypted web origins are limited to an explicitly configured loopback address", () => {
  for (const origin of ["http://127.0.0.1:3105", "http://[::1]:3105"]) {
    const request = new Request("http://localhost:3105/api/performance/vitals", {
      headers: { Origin: origin, "Sec-Fetch-Site": "same-origin" },
    });
    assert.equal(requestHasWebOrigin(request, origin), true);
  }
  for (const origin of ["http://app.example.com", "http://localhost:3105", "ftp://app.example.com"]) {
    const request = new Request("http://localhost:3105/api/performance/vitals", {
      headers: { Origin: origin, "Sec-Fetch-Site": "same-origin" },
    });
    assert.equal(requestHasWebOrigin(request, origin), false);
  }
});

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
