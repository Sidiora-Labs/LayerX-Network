import { expect, test, type Page } from "@playwright/test";
import { readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  PERFORMANCE_SAMPLE_COUNT,
  percentile75,
  ROUTE_SCRIPT_BUDGETS,
  THREE_G_PROFILE,
  WEB_VITAL_BUDGETS,
} from "../src/perf/budgets";

const WEB_ROOT = fileURLToPath(new URL("../", import.meta.url));
const NEXT_ROOT = path.join(WEB_ROOT, ".next");
const APP_ROOT = path.join(WEB_ROOT, "src/app");

interface LabMetrics {
  LCP: number;
  INP: number;
  CLS: number;
}

type InteractionTiming = PerformanceEventTiming & Readonly<{ interactionId: number }>;
type EventObserverInit = PerformanceObserverInit & Readonly<{ durationThreshold: number }>;

declare global {
  interface Window {
    __layerxLabMetrics: LabMetrics;
  }
}

function object(value: unknown, label: string): Readonly<Record<string, unknown>> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new TypeError(`${label} is not an object`);
  }
  return value as Readonly<Record<string, unknown>>;
}

function strings(value: unknown, label: string): readonly string[] {
  if (!Array.isArray(value)) {
    throw new TypeError(`${label} is not a string array`);
  }
  const entries: readonly unknown[] = value;
  if (!entries.every((entry): entry is string => typeof entry === "string")) {
    throw new TypeError(`${label} is not a string array`);
  }
  return entries;
}

async function routeScriptBytes(route: string): Promise<number> {
  const build = object(
    JSON.parse(await readFile(path.join(NEXT_ROOT, "build-manifest.json"), "utf8")) as unknown,
    "build manifest",
  );
  const shared = [
    ...strings(build.polyfillFiles, "polyfill files"),
    ...strings(build.rootMainFiles, "root main files"),
  ];
  const routePath = route === "/" ? "" : `${route.slice(1)}/`;
  const references = await readFile(
    path.join(NEXT_ROOT, `server/app/${routePath}page_client-reference-manifest.js`),
    "utf8",
  );
  const routeChunks = [...references.matchAll(/static\/chunks\/[^"'\\]+\.js/gu)].map(
    (match) => match[0],
  );
  const chunks = new Set([...shared, ...routeChunks]);
  let bytes = 0;
  for (const chunk of chunks) {
    bytes += (await stat(path.join(NEXT_ROOT, chunk))).size;
  }
  return bytes;
}

async function applicationPageRoutes(
  directory = APP_ROOT,
  segments: readonly string[] = [],
): Promise<readonly string[]> {
  const routes: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      routes.push(
        ...(await applicationPageRoutes(path.join(directory, entry.name), [
          ...segments,
          entry.name,
        ])),
      );
    } else if (entry.isFile() && entry.name === "page.tsx") {
      routes.push(segments.length === 0 ? "/" : `/${segments.join("/")}`);
    }
  }
  return routes.sort();
}

async function installMetricObservers(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const metrics: LabMetrics = { LCP: 0, INP: 0, CLS: 0 };
    window.__layerxLabMetrics = metrics;
    if (PerformanceObserver.supportedEntryTypes.includes("largest-contentful-paint")) {
      new PerformanceObserver((list) => {
        const last = list.getEntries().at(-1);
        if (last !== undefined) {
          metrics.LCP = last.startTime;
        }
      }).observe({ type: "largest-contentful-paint", buffered: true });
    }
    if (PerformanceObserver.supportedEntryTypes.includes("layout-shift")) {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          const shift = entry as PerformanceEntry & { hadRecentInput?: boolean; value?: number };
          if (shift.hadRecentInput === false && shift.value !== undefined) {
            metrics.CLS += shift.value;
          }
        }
      }).observe({ type: "layout-shift", buffered: true });
    }
    if (PerformanceObserver.supportedEntryTypes.includes("event")) {
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) {
          const interaction = entry as InteractionTiming;
          if (interaction.interactionId > 0) {
            metrics.INP = Math.max(metrics.INP, interaction.duration);
          }
        }
      }).observe({ type: "event", buffered: true, durationThreshold: 16 } as EventObserverInit);
    }
  });
}

async function labMetrics(page: Page, route: string): Promise<LabMetrics> {
  await page.goto(route, { waitUntil: "networkidle" });
  await page.waitForTimeout(250);
  return page.evaluate(() => ({ ...window.__layerxLabMetrics }));
}

async function measuredInteraction(page: Page): Promise<number> {
  await page.goto("/", { waitUntil: "networkidle" });
  await page.getByRole("button", { name: "Open explorer" }).click({ noWaitAfter: true });
  await page.waitForURL("**/explorer");
  await page.waitForFunction(() => window.__layerxLabMetrics.INP > 0);
  return page.evaluate(() => window.__layerxLabMetrics.INP);
}

test("production routes stay split and within their declared script budgets", async () => {
  expect(Object.keys(ROUTE_SCRIPT_BUDGETS).sort()).toEqual(await applicationPageRoutes());
  for (const [route, budget] of Object.entries(ROUTE_SCRIPT_BUDGETS)) {
    expect(await routeScriptBytes(route), `${route} raw script bytes`).toBeLessThanOrEqual(budget);
  }
});

test("representative pages meet p75 paint interaction and layout budgets", async ({ page }) => {
  await installMetricObservers(page);
  await page.setExtraHTTPHeaders({ "x-layerx-authenticated": "1" });
  for (const route of ["/", "/explorer", "/app"] as const) {
    const largestPaints: number[] = [];
    const layoutShifts: number[] = [];
    for (let sample = 0; sample < PERFORMANCE_SAMPLE_COUNT; sample += 1) {
      const metrics = await labMetrics(page, route);
      expect(metrics.LCP).toBeGreaterThan(0);
      largestPaints.push(metrics.LCP);
      layoutShifts.push(metrics.CLS);
    }
    expect(percentile75(largestPaints), `${route} p75 LCP`).toBeLessThanOrEqual(
      WEB_VITAL_BUDGETS.LCP.limit,
    );
    expect(percentile75(layoutShifts), `${route} p75 CLS`).toBeLessThanOrEqual(
      WEB_VITAL_BUDGETS.CLS.limit,
    );
  }
  const interactions: number[] = [];
  for (let sample = 0; sample < PERFORMANCE_SAMPLE_COUNT; sample += 1) {
    interactions.push(await measuredInteraction(page));
  }
  expect(percentile75(interactions)).toBeLessThanOrEqual(WEB_VITAL_BUDGETS.INP.limit);
});

test("redacted RUM, cache controls, and 3G journey progress use the production server", async ({
  context,
  page,
  request,
}) => {
  const rumHeaders = {
    Origin: "http://127.0.0.1:3105",
    "Sec-Fetch-Site": "same-origin",
  } as const;
  const accepted = await request.post("/api/performance/vitals", {
    headers: rumHeaders,
    data: { version: 1, route: "app", metric: "CLS", observed: 50_000 },
  });
  expect(accepted.status()).toBe(202);
  expect((await accepted.json()) as unknown).toMatchObject({
    accepted: true,
    durable: true,
    sink: {
      mode: "durable-file",
      configured: true,
      durable: true,
      delivery: "healthy",
      reason: "delivery-confirmed",
    },
  });
  const refused = await request.post("/api/performance/vitals", {
    headers: rumHeaders,
    data: {
      version: 1,
      route: "explorer",
      metric: "LCP",
      observed: 1_500_000,
      account: "alice",
    },
  });
  expect(refused.status()).toBe(400);
  const snapshot = await request.get("/api/performance/vitals");
  const rum = (await snapshot.json()) as {
    version: number;
    sink: Readonly<Record<string, unknown>>;
    aggregates: Array<{
      route: string;
      metric: string;
      samples: number;
      percentile75: number;
      withinBudget: boolean;
    }>;
  };
  expect(rum).toMatchObject({
    version: 1,
    sink: {
      mode: "durable-file",
      configured: true,
      durable: true,
      delivery: "healthy",
      reason: "delivery-confirmed",
    },
  });
  const cls = rum.aggregates.find(
    (aggregate) => aggregate.route === "app" && aggregate.metric === "CLS",
  );
  expect(cls).toBeDefined();
  expect(cls?.samples).toBeGreaterThanOrEqual(1);
  expect(cls?.percentile75).toBe(50_000);
  expect(cls?.withinBudget).toBe(true);
  const explorer = await request.get("/explorer");
  expect(explorer.headers()["cache-control"]).toContain("s-maxage=60");

  await page.goto("/", { waitUntil: "networkidle" });
  const session = await context.newCDPSession(page);
  await session.send("Network.enable");
  await session.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: THREE_G_PROFILE.latencyMs,
    downloadThroughput: THREE_G_PROFILE.downloadBytesPerSecond,
    uploadThroughput: THREE_G_PROFILE.uploadBytesPerSecond,
    connectionType: "cellular3g",
  });
  await page.getByRole("button", { name: "Open explorer" }).click({ noWaitAfter: true });
  await expect(page.locator('[data-honest-progress="explorer"]')).toBeVisible();
  await expect(page.getByRole("heading", { name: "LayerX Explorer" })).toBeVisible({
    timeout: 30_000,
  });

  await context.setExtraHTTPHeaders({ "x-layerx-authenticated": "1" });
  const journeyNavigation = page.goto("/app/move", { waitUntil: "networkidle" });
  await expect(page.locator('[data-honest-progress="app"]')).toBeVisible();
  await journeyNavigation;
  await expect(page.getByRole("heading", { name: "Move money" })).toBeVisible({
    timeout: 30_000,
  });
  await session.send("Network.disable");
});
