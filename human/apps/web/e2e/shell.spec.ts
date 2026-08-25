import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { NextRequest } from "next/server.js";

import { proxy } from "../src/proxy.ts";
import { appContentSecurityPolicy } from "../src/security/csp.ts";
import {
  SHELL_HINT_COOKIE,
  ShellSelector,
  TABLET_MIN_WIDTH,
} from "../src/shell/selector.ts";
import { selectServerShell } from "../src/shell/server.ts";

test("server selection covers phone, tablet and desktop capability profiles", () => {
  const phone = ShellSelector.server({ viewportWidth: 390, pointer: "coarse" });
  assert.equal(phone.shell, "mobile");
  assert.equal(phone.touchTargets, true);

  const tablet = ShellSelector.server({ viewportWidth: TABLET_MIN_WIDTH, pointer: "coarse" });
  assert.equal(tablet.shell, "desktop");
  assert.equal(tablet.touchTargets, true);

  const desktop = ShellSelector.server({ viewportWidth: 1440, pointer: "fine" });
  assert.equal(desktop.shell, "desktop");
  assert.equal(desktop.touchTargets, false);
});

test("missing viewport hints fail safely to desktop even for a coarse pointer", () => {
  assert.deepEqual(ShellSelector.server({ pointer: "coarse" }), {
    shell: "desktop",
    pointer: "coarse",
    touchTargets: true,
    source: "server",
  });
});

test("server request hints combine viewport width with the stored pointer capability", () => {
  const request = new NextRequest("https://layerx.test/app", {
    headers: {
      cookie: `${SHELL_HINT_COOKIE}=v1.1024.coarse`,
      "sec-ch-viewport-width": "834",
    },
  });
  const selected = selectServerShell(request.headers, request.cookies);
  assert.equal(selected.viewportWidth, 834);
  assert.equal(selected.pointer, "coarse");
  assert.equal(selected.shell, "desktop");
  assert.equal(selected.touchTargets, true);
});

test("a mobile user-agent is never sufficient to choose the mobile shell", () => {
  const request = new NextRequest("https://layerx.test/app", {
    headers: { "user-agent": "Mobile" },
  });
  assert.equal(selectServerShell(request.headers, request.cookies).shell, "desktop");
});

test("client confirmation corrects a server mismatch before paint", () => {
  const server = ShellSelector.server({ viewportWidth: 1440, pointer: "fine" });
  const confirmation = ShellSelector.confirm(server, { viewportWidth: 390, pointer: "coarse" });
  assert.equal(confirmation.correction, "pre-paint");
  assert.equal(confirmation.selection.shell, "mobile");
  assert.equal(confirmation.selection.source, "client");
});

test("resize transitions preserve the tablet desktop boundary", () => {
  const widths = [390, 767, 768, 1024, 1440];
  assert.deepEqual(
    widths.map((viewportWidth) =>
      ShellSelector.client({ viewportWidth, pointer: "coarse" }).shell,
    ),
    ["mobile", "mobile", "desktop", "desktop", "desktop"],
  );
});

test("deep links retain the same path, query and fragment across shells", () => {
  const href = "/app/activity/act_42?view=details#receipt";
  assert.equal(ShellSelector.resolveDeepLink(href, "mobile").href, href);
  assert.equal(ShellSelector.resolveDeepLink(href, "desktop").href, href);
  assert.throws(
    () => ShellSelector.resolveDeepLink("https://example.com/app/activity", "desktop"),
    /authenticated plane/u,
  );
});

test("authenticated responses use a nonce policy with pinned asset sources", () => {
  const policy = appContentSecurityPolicy("abcdefghijklmnop123456", false);
  assert.match(policy, /script-src 'self' 'nonce-abcdefghijklmnop123456' 'strict-dynamic'/u);
  assert.match(policy, /style-src 'self' 'nonce-abcdefghijklmnop123456'/u);
  assert.match(policy, /img-src 'self' data: blob:/u);
  assert.match(policy, /connect-src 'self'/u);
  assert.match(policy, /object-src 'none'/u);
  assert.doesNotMatch(policy, /https?:/u);
  assert.doesNotMatch(policy, /unsafe-inline|unsafe-eval/u);
});

test("the proxy never promotes cookie presence into authenticated identity", () => {
  const response = proxy(
    new NextRequest("https://layerx.test/app/activity/act_42?view=details", {
      headers: { cookie: "__Host-layerx-session=forged" },
    }),
  );
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("x-layerx-authenticated"), null);
  assert.match(response.headers.get("content-security-policy") ?? "", /script-src 'self' 'nonce-/u);
});

test("the public and authenticated plane roots remain separate", async () => {
  const [appLayout, explorerLayout, proxy] = await Promise.all([
    readFile(new URL("../src/app/app/layout.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/app/explorer/layout.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/proxy.ts", import.meta.url), "utf8"),
  ]);
  assert.match(appLayout, /AuthenticatedShell/u);
  assert.match(appLayout, /verifiedWebSession/u);
  assert.doesNotMatch(proxy, /APP_SESSION_COOKIE|x-layerx-authenticated/u);
  assert.match(explorerLayout, /force-static/u);
  assert.match(proxy, /matcher: \["\/app\/:path\*"\]/u);
});
