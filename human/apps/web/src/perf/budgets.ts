export const WEB_VITAL_NAMES = ["LCP", "INP", "CLS"] as const;
export type WebVitalName = (typeof WEB_VITAL_NAMES)[number];

export const PERFORMANCE_ROUTES = ["root", "explorer", "app"] as const;
export type PerformanceRoute = (typeof PERFORMANCE_ROUTES)[number];

export const WEB_VITAL_BUDGETS = Object.freeze({
  LCP: Object.freeze({ limit: 2_500, scale: 1_000 }),
  INP: Object.freeze({ limit: 200, scale: 1_000 }),
  CLS: Object.freeze({ limit: 0.1, scale: 1_000_000 }),
} satisfies Readonly<Record<WebVitalName, Readonly<{ limit: number; scale: number }>>>);

const ROOT_ROUTE_SCRIPT_BUDGET_BYTES = 900 * 1_024;
const EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES = 900 * 1_024;
const APP_ROUTE_SCRIPT_BUDGET_BYTES = 1_100 * 1_024;

export const ROUTE_SCRIPT_BUDGETS = Object.freeze({
  "/": ROOT_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/accounts/[accountId]": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/batches": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/batches/[batchNumber]": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/checkpoints": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/checkpoints/[checkpointId]": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/receipts/[receiptId]": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/explorer/verify": EXPLORER_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/activity": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/activity/[entryId]": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/agents": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/agents/[agentId]": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/agents/new": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/approvals": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/approvals/[approvalId]": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/deposit": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/journeys/[journeyId]": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/journeys/[journeyId]/claim": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/move": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/move/withdraw": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/notifications": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/security/devices": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/security/recovery": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/settings": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/settings/devices": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/settings/exit": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/settings/security": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/settings/wallet": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/support": APP_ROUTE_SCRIPT_BUDGET_BYTES,
  "/app/withdraw": APP_ROUTE_SCRIPT_BUDGET_BYTES,
} as const satisfies Readonly<Record<string, number>>);

export type BudgetedApplicationRoute = keyof typeof ROUTE_SCRIPT_BUDGETS;

export const THREE_G_PROFILE = Object.freeze({
  latencyMs: 150,
  downloadBytesPerSecond: 1_600_000 / 8,
  uploadBytesPerSecond: 750_000 / 8,
});

export const PERFORMANCE_SAMPLE_COUNT = 4;
export const RUM_SERIES_CAPACITY = 256;

const PERFORMANCE_BUDGETS = Object.freeze({
  webVitals: WEB_VITAL_BUDGETS,
  routeScripts: ROUTE_SCRIPT_BUDGETS,
  threeG: THREE_G_PROFILE,
  samples: PERFORMANCE_SAMPLE_COUNT,
});

export function perf() {
  return PERFORMANCE_BUDGETS;
}

export function isWebVitalName(value: string): value is WebVitalName {
  return WEB_VITAL_NAMES.some((name) => name === value);
}

export function isPerformanceRoute(value: string): value is PerformanceRoute {
  return PERFORMANCE_ROUTES.some((route) => route === value);
}

export function classifyPerformanceRoute(pathname: string): PerformanceRoute | undefined {
  if (pathname === "/") {
    return "root";
  }
  if (pathname === "/explorer" || pathname.startsWith("/explorer/")) {
    return "explorer";
  }
  if (pathname === "/app" || pathname.startsWith("/app/")) {
    return "app";
  }
  return undefined;
}

export function normalizeWebVital(name: WebVitalName, observed: number): number | undefined {
  if (!Number.isFinite(observed) || observed < 0) {
    return undefined;
  }
  const normalized = Math.round(observed * WEB_VITAL_BUDGETS[name].scale);
  const maximum = name === "CLS" ? 10_000_000 : 120_000_000;
  return Number.isSafeInteger(normalized) && normalized <= maximum ? normalized : undefined;
}

export function normalizedBudget(name: WebVitalName): number {
  const budget = WEB_VITAL_BUDGETS[name];
  return Math.round(budget.limit * budget.scale);
}

export function percentile75(values: readonly number[]): number {
  if (values.length === 0) {
    throw new RangeError("A percentile requires at least one observation");
  }
  const ordered = [...values].sort((left, right) => left - right);
  const index = Math.max(0, Math.ceil(ordered.length * 0.75) - 1);
  const observed = ordered[index];
  if (observed === undefined) {
    throw new RangeError("The percentile observation is unavailable");
  }
  return observed;
}
