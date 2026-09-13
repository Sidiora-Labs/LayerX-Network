import { copyEntry } from "../../copy/runtime.ts";

export const REQUIRED_SCREEN_STATES = [
  "loading",
  "empty",
  "error",
  "offline",
  "degraded",
  "still-checking",
] as const;

export const CURRENT_ROUTES = [
  "/",
  "/app",
  "/app/move",
  "/app/activity",
  "/app/activity/[entryId]",
  "/app/settings",
  "/app/support",
  "/app/deposit",
  "/app/withdraw",
  "/app/journeys/[journeyId]",
  "/app/journeys/[journeyId]/claim",
  "/app/settings/exit",
  "/app/approvals",
  "/app/approvals/[approvalId]",
  "/app/notifications",
  "/explorer",
  "/explorer/checkpoints",
  "/explorer/checkpoints/[checkpointId]",
  "/explorer/batches",
  "/explorer/batches/[batchNumber]",
  "/explorer/receipts/[receiptId]",
  "/explorer/accounts/[accountId]",
  "/explorer/verify",
  "/app/agents",
  "/app/agents/new",
  "/app/agents/[agentId]",
] as const;
export const CURRENT_SHELLS = ["authenticated"] as const;

export type ScreenState = (typeof REQUIRED_SCREEN_STATES)[number];
export type HumanShell = "mobile" | "desktop";
export type StateTargetKind = "route" | "shell";

export interface StatePresentation {
  readonly titleKey: string;
  readonly descriptionKey: string;
  readonly actionKeys: readonly string[];
  readonly duplicateActionsLocked: boolean;
}

export interface ScreenStateEntry {
  readonly id: string;
  readonly kind: StateTargetKind;
  readonly route: string;
  readonly shells: readonly HumanShell[];
  readonly states: Readonly<Record<ScreenState, () => StatePresentation>>;
}

const presentation = (
  titleKey: string,
  descriptionKey: string,
  actionKeys: readonly string[] = [],
  duplicateActionsLocked = false,
): (() => StatePresentation) =>
  () => Object.freeze({
    titleKey,
    descriptionKey,
    actionKeys: Object.freeze(actionKeys),
    duplicateActionsLocked,
  });

const completeStates = (): ScreenStateEntry["states"] => Object.freeze({
  loading: presentation("state.loading", "state.loading.body"),
  empty: presentation("state.empty", "state.empty.body"),
  error: presentation(
    "state.error",
    "state.error.body",
    ["action.retry", "action.reload", "action.report"],
  ),
  offline: presentation("state.offline", "state.offline.body", ["action.retry", "action.report"]),
  degraded: presentation(
    "state.degraded",
    "state.degraded.age",
    ["action.reload", "action.report"],
  ),
  "still-checking": presentation(
    "status.still_checking",
    "state.still_checking.body",
    [],
    true,
  ),
});

const BOTH_SHELLS: readonly HumanShell[] = Object.freeze(["mobile", "desktop"]);

const currentTargets: readonly ScreenStateEntry[] = Object.freeze([
  Object.freeze({ id: "route.root", kind: "route", route: "/", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app", kind: "route", route: "/app", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.move", kind: "route", route: "/app/move", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.activity", kind: "route", route: "/app/activity", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.activity.detail", kind: "route", route: "/app/activity/[entryId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.settings", kind: "route", route: "/app/settings", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.support", kind: "route", route: "/app/support", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.deposit", kind: "route", route: "/app/deposit", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.withdraw", kind: "route", route: "/app/withdraw", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.journeys.detail", kind: "route", route: "/app/journeys/[journeyId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.journeys.claim", kind: "route", route: "/app/journeys/[journeyId]/claim", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.settings.exit", kind: "route", route: "/app/settings/exit", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.approvals", kind: "route", route: "/app/approvals", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.approval", kind: "route", route: "/app/approvals/[approvalId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.notifications", kind: "route", route: "/app/notifications", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer", kind: "route", route: "/explorer", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.checkpoints", kind: "route", route: "/explorer/checkpoints", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.checkpoint", kind: "route", route: "/explorer/checkpoints/[checkpointId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.batches", kind: "route", route: "/explorer/batches", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.batch", kind: "route", route: "/explorer/batches/[batchNumber]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.receipt", kind: "route", route: "/explorer/receipts/[receiptId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.account", kind: "route", route: "/explorer/accounts/[accountId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.explorer.verify", kind: "route", route: "/explorer/verify", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.agents", kind: "route", route: "/app/agents", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.agents.new", kind: "route", route: "/app/agents/new", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "route.app.agents.detail", kind: "route", route: "/app/agents/[agentId]", shells: BOTH_SHELLS, states: completeStates() }),
  Object.freeze({ id: "shell.authenticated", kind: "shell", route: "/app", shells: BOTH_SHELLS, states: completeStates() }),
]);

export class StateMatrix {
  readonly #entries: ReadonlyMap<string, ScreenStateEntry>;

  constructor(entries: readonly ScreenStateEntry[]) {
    this.#entries = new Map(entries.map((entry) => [entry.id, entry]));
    if (this.#entries.size !== entries.length) {
      throw new Error("state-matrix target identifiers must be unique");
    }
  }

  enumerate(): readonly ScreenStateEntry[] {
    return Object.freeze([...this.#entries.values()]);
  }

  assertComplete(
    routes: readonly string[] = CURRENT_ROUTES,
    shells: readonly string[] = CURRENT_SHELLS,
  ): void {
    const registeredRoutes = new Set<string>();
    const registeredShells = new Set<string>();

    for (const entry of this.#entries.values()) {
      if (entry.kind === "route") {
        registeredRoutes.add(entry.route);
      } else {
        registeredShells.add(entry.id.replace(/^shell\./u, ""));
      }
      if (
        entry.shells.length !== 2
        || !entry.shells.includes("mobile")
        || !entry.shells.includes("desktop")
      ) {
        throw new Error(`${entry.id} must declare both shells`);
      }
      for (const state of REQUIRED_SCREEN_STATES) {
        const renderer = entry.states[state];
        if (typeof renderer !== "function") {
          throw new Error(`${entry.id} is missing ${state}`);
        }
        const rendered = renderer();
        copyEntry(rendered.titleKey);
        copyEntry(rendered.descriptionKey);
        for (const actionKey of rendered.actionKeys) {
          copyEntry(actionKey);
        }
        if (state === "still-checking" && !rendered.duplicateActionsLocked) {
          throw new Error(`${entry.id} must lock duplicate actions while still checking`);
        }
      }
    }

    for (const route of routes) {
      if (!registeredRoutes.has(route)) {
        throw new Error(`${route} has no state-matrix registration`);
      }
    }
    for (const shell of shells) {
      if (!registeredShells.has(shell)) {
        throw new Error(`${shell} has no state-matrix registration`);
      }
    }
  }
}

export function humanStateMatrix(): StateMatrix {
  return new StateMatrix(currentTargets);
}

export function human_state_matrix_registry(): StateMatrix {
  return humanStateMatrix();
}
