import { copyEntry } from "../../../copy/runtime.ts";
import type { ExitEligibility, HumanApiClient, Journey } from "../../api/index.ts";
import { custodyCopyKey } from "../custody/copy.ts";
import { exitComplete, presentedJourneyState } from "../custody/evidence.ts";
import { WalletHandOff, type PaxeerWalletBridge, type WalletHandOffPhase } from "../custody/handoff.ts";
import {
  custodyApplicationPath,
  journeyTimeline,
  newIdempotencyKey,
  refusalPresentation,
  walletPanel,
  type CustodyShell,
  type RandomBytes,
  type RefusalPresentation,
  type TimelineRow,
  type WalletPanelPlan,
} from "../custody/model.ts";
import {
  JourneyOutcomeUnknownError,
  mutationOutcomeIsUnknown,
  type UnknownOutcomeRecovery,
} from "../custody/recovery.ts";

export const EXIT_CONFIRMATION_PHRASE = copyEntry("exit.confirmation.phrase").message;
export const EXIT_FINAL_STAGE = "exit.stage.confirming-on-paxeer";

export class ExitConfirmationError extends Error {
  constructor() {
    super("The exit confirmation phrase does not match");
    this.name = "ExitConfirmationError";
  }
}

export function exitConfirmationReady(typed: string): boolean {
  return typed === EXIT_CONFIRMATION_PHRASE;
}

export interface ExitPlanInput {
  readonly shell: CustodyShell;
  readonly typedConfirmation: string;
  readonly degraded: boolean;
  readonly eligibility?: ExitEligibility;
  readonly journey?: Journey;
  readonly walletPhase: WalletHandOffPhase;
}

export interface ExitPlan {
  readonly shell: CustodyShell;
  readonly titleKey: "exit.title";
  readonly summaryKey: "exit.summary";
  readonly phase: "checking" | "unavailable" | "confirm" | "journey";
  readonly degradedKey?: "exit.degraded";
  readonly unavailable?: Readonly<{
    bodyKey: string;
    withdrawInsteadPath?: string;
    withdrawInsteadKey?: "exit.withdraw_instead";
  }>;
  readonly confirmation?: Readonly<{
    kind: "irreversible";
    expectedValue: string;
    consequenceKey: "exit.irreversible";
    actionKey: "exit.start";
    ready: boolean;
  }>;
  readonly timeline?: readonly TimelineRow[];
  readonly wallet?: WalletPanelPlan;
  readonly refusal?: RefusalPresentation;
  readonly complete?: Readonly<{ titleKey: "exit.complete"; bodyKey: "exit.complete.body" }>;
  readonly duplicateLocked: boolean;
}

export function exitPlan(input: ExitPlanInput): ExitPlan {
  const base = {
    shell: input.shell,
    titleKey: "exit.title" as const,
    summaryKey: "exit.summary" as const,
    ...(input.degraded ? { degradedKey: "exit.degraded" as const } : {}),
  };
  if (input.journey !== undefined) {
    const journey = input.journey;
    const presented = presentedJourneyState(journey, EXIT_FINAL_STAGE);
    const complete = exitComplete(journey);
    const wallet =
      journey.wallet_request === undefined || journey.refusal !== undefined || presented === "still-checking"
        ? undefined
        : walletPanel(journey.wallet_request, input.walletPhase, "exit.sign.exit-claim");
    return Object.freeze({
      ...base,
      phase: "journey",
      timeline: journeyTimeline(journey),
      ...(wallet === undefined ? {} : { wallet }),
      ...(journey.refusal === undefined ? {} : { refusal: refusalPresentation(journey.refusal) }),
      ...(complete
        ? { complete: Object.freeze({ titleKey: "exit.complete" as const, bodyKey: "exit.complete.body" as const }) }
        : {}),
      duplicateLocked: presented === "still-checking",
    });
  }
  if (input.eligibility === undefined && !input.degraded) {
    return Object.freeze({ ...base, phase: "checking", duplicateLocked: false });
  }
  if (input.eligibility !== undefined && !input.eligibility.eligible) {
    const withdrawInsteadPath = input.eligibility.withdraw_instead_path === undefined
      ? undefined
      : custodyApplicationPath(input.eligibility.withdraw_instead_path);
    return Object.freeze({
      ...base,
      phase: "unavailable",
      unavailable: Object.freeze({
        bodyKey: custodyCopyKey(input.eligibility.copy_key, "exit.unavailable.network-operating-normally"),
        ...(withdrawInsteadPath === undefined
          ? {}
          : {
              withdrawInsteadPath,
              withdrawInsteadKey: "exit.withdraw_instead" as const,
            }),
      }),
      duplicateLocked: false,
    });
  }
  return Object.freeze({
    ...base,
    phase: "confirm",
    confirmation: Object.freeze({
      kind: "irreversible" as const,
      expectedValue: EXIT_CONFIRMATION_PHRASE,
      consequenceKey: "exit.irreversible" as const,
      actionKey: "exit.start" as const,
      ready: exitConfirmationReady(input.typedConfirmation),
    }),
    duplicateLocked: false,
  });
}

export interface ExitControllerOptions {
  readonly api: HumanApiClient;
  readonly bridge: PaxeerWalletBridge;
  readonly randomBytes?: RandomBytes;
}

export class ExitController {
  readonly #api: HumanApiClient;
  readonly #handOff: WalletHandOff;
  readonly #randomBytes: RandomBytes | undefined;
  #idempotencyKey: string | undefined;
  #eligibility: ExitEligibility | undefined;
  #journey: Journey | undefined;
  #unknownRecovery: UnknownOutcomeRecovery | undefined;
  #unknownRecoveryMode: "start" | "handoff" | undefined;

  constructor(options: ExitControllerOptions) {
    this.#api = options.api;
    this.#handOff = new WalletHandOff(options.bridge);
    this.#randomBytes = options.randomBytes;
  }

  get eligibility(): ExitEligibility | undefined {
    return this.#eligibility;
  }

  get journey(): Journey | undefined {
    return this.#journey;
  }

  get walletPhase(): WalletHandOffPhase {
    return this.#handOff.phase;
  }

  get idempotencyKey(): string | undefined {
    return this.#idempotencyKey;
  }

  get outcomeUnknown(): boolean {
    return this.#unknownRecovery !== undefined;
  }

  get unknownRecoveryMode(): "start" | "handoff" | undefined {
    return this.#unknownRecoveryMode;
  }

  walletOpens(stageId: string): number {
    return this.#handOff.opens(stageId);
  }

  cancelWallet(): void {
    this.#handOff.cancel();
  }

  async checkEligibility(): Promise<ExitEligibility> {
    this.#eligibility = await this.#api.exitEligibility();
    return this.#eligibility;
  }

  async start(typedConfirmation: string): Promise<Journey> {
    if (!exitConfirmationReady(typedConfirmation)) {
      throw new ExitConfirmationError();
    }
    this.#idempotencyKey ??=
      this.#randomBytes === undefined ? newIdempotencyKey() : newIdempotencyKey(this.#randomBytes);
    const request = { confirmation: typedConfirmation } as const;
    const start = async () => this.#api.exitStart(request, this.#idempotencyKey as string);
    try {
      this.#journey = await start();
      this.#unknownRecovery = undefined;
      this.#unknownRecoveryMode = undefined;
      return this.#journey;
    } catch (error) {
      if (!mutationOutcomeIsUnknown(error)) {
        throw error;
      }
      this.#unknownRecovery = async () => {
        const journey = await start();
        this.#journey = journey;
        return { journey, resolved: true };
      };
      this.#unknownRecoveryMode = "start";
      throw new JourneyOutcomeUnknownError(error);
    }
  }

  async openWallet(): Promise<Journey | undefined> {
    const journey = this.#journey;
    const request = journey?.wallet_request;
    if (journey === undefined || request === undefined) {
      throw new Error("No signing moment is pending for this exit");
    }
    const outcome = await this.#handOff.open(request);
    if (outcome.outcome !== "approved") {
      return undefined;
    }
    try {
      this.#journey = await this.#api.journeyGet(journey.journey_id);
      this.#unknownRecovery = undefined;
      this.#unknownRecoveryMode = undefined;
      return this.#journey;
    } catch (error) {
      const signedStage = request.stage_id;
      this.#unknownRecovery = async () => {
        const updated = await this.#api.journeyGet(journey.journey_id);
        this.#journey = updated;
        return {
          journey: updated,
          resolved:
            updated.state !== "still-checking" && updated.wallet_request?.stage_id !== signedStage,
        };
      };
      this.#unknownRecoveryMode = "handoff";
      throw new JourneyOutcomeUnknownError(error);
    }
  }

  async recoverUnknown(): Promise<Readonly<{ journey: Journey; resolved: boolean }>> {
    const recovery = this.#unknownRecovery;
    if (recovery === undefined) {
      const journey = await this.refresh();
      return { journey, resolved: journey.state !== "still-checking" };
    }
    const outcome = await recovery();
    if (outcome.resolved) {
      this.#unknownRecovery = undefined;
      this.#unknownRecoveryMode = undefined;
    }
    return outcome;
  }

  async refresh(): Promise<Journey> {
    const journey = this.#journey;
    if (journey === undefined) {
      throw new Error("No exit journey has been started");
    }
    this.#journey = await this.#api.journeyGet(journey.journey_id);
    return this.#journey;
  }

  adopt(journey: Journey): void {
    this.#journey = journey;
  }
}
