"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { humanApi } from "../../api/index.ts";
import type { Journey } from "../../api/index.ts";
import { ScreenCard } from "../../kit/surface";
import { StatusPill } from "../../kit/money";
import { ErrorSurface, errorPresentation, LoadingSurface, StillCheckingSurface } from "../../states";
import { DEPOSIT_FINAL_STAGE, DepositController, depositPlan } from "../deposit/model.ts";
import { EXIT_FINAL_STAGE, ExitController, exitPlan } from "../exit/model.ts";
import {
  WITHDRAW_FINAL_STAGE,
  WithdrawController,
  withdrawPlan,
} from "../withdraw/model.ts";
import { ChallengeHoldView, SettlementNotice } from "../withdraw/screen";
import { presentedJourneyState, statusKeyForState } from "./evidence.ts";
import { browserWalletBridge, windowWalletProvider } from "./handoff.ts";
import { journeyTimeline } from "./model.ts";
import { isJourneyOutcomeUnknown } from "./recovery.ts";
import type { CustodyTiming } from "./time.ts";
import {
  CompleteView,
  DelayNotice,
  JourneyTechnicalDetails,
  JourneyTimelineView,
  RefusalView,
  SafeToCloseNotice,
  useCustodyShell,
  WalletPanelView,
} from "./timeline";

const REFRESH_INTERVAL_MS = 5_000;

type CustodyController = DepositController | ExitController | WithdrawController;

function controllerFor(journey: Journey): CustodyController | undefined {
  const options = { api: humanApi(), bridge: browserWalletBridge(windowWalletProvider) };
  switch (journey.kind) {
    case "deposit": {
      const controller = new DepositController(options);
      controller.adopt(journey);
      return controller;
    }
    case "withdraw": {
      const controller = new WithdrawController(options);
      controller.adopt(journey);
      return controller;
    }
    case "exit": {
      const controller = new ExitController(options);
      controller.adopt(journey);
      return controller;
    }
    case "move":
    case "onboarding":
    case "wallet-binding":
    case "agent-create":
    case "agent-fund":
    case "agent-pause":
    case "agent-retire":
      return undefined;
  }
}

const JOURNEY_TITLE_KEYS = Object.freeze({
  deposit: "deposit.title",
  withdraw: "withdraw.title",
  exit: "exit.title",
} as const);

function finalStageFor(journey: Journey): string {
  switch (journey.kind) {
    case "deposit":
      return DEPOSIT_FINAL_STAGE;
    case "withdraw":
      return WITHDRAW_FINAL_STAGE;
    case "exit":
      return EXIT_FINAL_STAGE;
    case "move":
    case "onboarding":
    case "wallet-binding":
    case "agent-create":
    case "agent-fund":
    case "agent-pause":
    case "agent-retire":
      return "";
  }
}

export function JourneyScreen({
  journeyId,
  timing,
}: Readonly<{ journeyId: string; timing: CustodyTiming }>) {
  const shell = useCustodyShell();
  const router = useRouter();
  const api = useMemo(() => humanApi(), []);
  const controllerRef = useRef<CustodyController | undefined>(undefined);
  const [journey, setJourney] = useState<Journey | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<unknown>(undefined);
  const [retryToken, setRetryToken] = useState(0);

  useEffect(() => {
    let cancelled = false;
    void api
      .journeyGet(journeyId)
      .then((loaded) => {
        if (cancelled) {
          return;
        }
        controllerRef.current = controllerFor(loaded);
        setJourney(loaded);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setFailure(error);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [api, journeyId, retryToken]);

  const presented = journey === undefined
    ? undefined
    : presentedJourneyState(journey, finalStageFor(journey));
  const checkingUnknown = controllerRef.current?.outcomeUnknown === true || presented === "still-checking";
  const active =
    journey !== undefined &&
    !checkingUnknown &&
    journey.refusal === undefined &&
    journey.state !== "done" &&
    journey.state !== "done-finalised" &&
    journey.wallet_request === undefined;
  useEffect(() => {
    if (!active) {
      return;
    }
    const timer = setInterval(() => {
      void api
        .journeyGet(journeyId)
        .then((loaded) => {
          controllerRef.current?.adopt(loaded);
          setJourney(loaded);
        })
        .catch(() => undefined);
    }, REFRESH_INTERVAL_MS);
    return () => {
      clearInterval(timer);
    };
  }, [active, api, journeyId]);

  const lookupUnknown = useCallback(async (): Promise<"pending" | "resolved"> => {
    try {
      const controller = controllerRef.current;
      const outcome = controller?.outcomeUnknown === true
        ? await controller.recoverUnknown()
        : { journey: await api.journeyGet(journeyId), resolved: true };
      controller?.adopt(outcome.journey);
      setJourney(outcome.journey);
      return outcome.resolved &&
        presentedJourneyState(outcome.journey, finalStageFor(outcome.journey)) !== "still-checking"
        ? "resolved"
        : "pending";
    } catch {
      return "pending";
    }
  }, [api, journeyId]);
  const unknownResolved = useCallback(() => undefined, []);

  if (failure !== undefined) {
    return (
      <ErrorSurface
        error={errorPresentation(failure)}
        route="/app/journeys"
        platform={shell}
        onRetry={() => {
          setFailure(undefined);
          setRetryToken((token) => token + 1);
        }}
      />
    );
  }
  if (journey === undefined) {
    return <LoadingSurface />;
  }

  const controller = controllerRef.current;
  const walletPhase = controller?.walletPhase ?? "idle";
  const runOpenWallet = async () => {
    if (controller === undefined) {
      return;
    }
    setBusy(true);
    try {
      const updated =
        controller instanceof WithdrawController
          ? await controller.openWalletToClaim()
          : await controller.openWallet();
      if (updated !== undefined) {
        setJourney(updated);
      }
    } catch (error) {
      if (!isJourneyOutcomeUnknown(error)) {
        setFailure(error);
      }
    } finally {
      setBusy(false);
      setJourney(controller.journey);
    }
  };

  const custodyKind =
    journey.kind === "deposit" || journey.kind === "withdraw" || journey.kind === "exit";
  const titleKey = custodyKind
    ? JOURNEY_TITLE_KEYS[journey.kind as keyof typeof JOURNEY_TITLE_KEYS]
    : "journey.timeline.title";
  const status = statusKeyForState(
    custodyKind ? presentedJourneyState(journey, finalStageFor(journey)) : journey.state,
  );
  const nowMs = Date.now();
  const navigate = (path: string) => {
    router.push(path);
  };

  if (journey.kind === "deposit") {
    const plan = depositPlan({ shell, timing, nowMs, amountInput: "", journey, walletPhase });
    return (
      <ScreenCard landmark="section" title={copyEntry(titleKey).message} dataApplication="deposit">
        <StatusPill status={status} />
        {!checkingUnknown ? null : (
          <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={unknownResolved} />
        )}
        {plan.delayed === undefined ? null : <DelayNotice duration={plan.delayed.duration} />}
        {plan.safeToCloseKey === undefined ? null : <SafeToCloseNotice messageKey={plan.safeToCloseKey} />}
        {plan.timeline === undefined ? null : <JourneyTimelineView rows={plan.timeline} />}
        <JourneyTechnicalDetails journey={journey} />
        {plan.wallet === undefined ? null : (
          <WalletPanelView
            panel={plan.wallet}
            busy={busy}
            onOpen={() => {
              void runOpenWallet();
            }}
          />
        )}
        {plan.refusal === undefined ? null : <RefusalView refusal={plan.refusal} onNavigate={navigate} />}
        {plan.complete === undefined ? null : (
          <CompleteView titleKey={plan.complete.titleKey} bodyKey={plan.complete.bodyKey} />
        )}
      </ScreenCard>
    );
  }

  if (journey.kind === "withdraw") {
    const plan = withdrawPlan({
      shell,
      timing,
      nowMs,
      amountInput: "",
      destinationInput: "",
      journey,
      walletPhase,
    });
    return (
      <ScreenCard landmark="section" title={copyEntry(titleKey).message} dataApplication="withdraw">
        <StatusPill status={status} />
        {!checkingUnknown ? null : (
          <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={unknownResolved} />
        )}
        <SettlementNotice settlement={plan.settlement} />
        {plan.hold === undefined ? null : <ChallengeHoldView hold={plan.hold} />}
        {plan.timeline === undefined ? null : <JourneyTimelineView rows={plan.timeline} />}
        <JourneyTechnicalDetails journey={journey} />
        {plan.wallet === undefined ? null : (
          <WalletPanelView
            panel={plan.wallet}
            busy={busy}
            onOpen={() => {
              void runOpenWallet();
            }}
          />
        )}
        {plan.refusal === undefined ? null : <RefusalView refusal={plan.refusal} onNavigate={navigate} />}
        {plan.complete === undefined ? null : (
          <CompleteView titleKey={plan.complete.titleKey} bodyKey={plan.complete.bodyKey} />
        )}
      </ScreenCard>
    );
  }

  if (journey.kind === "exit") {
    const plan = exitPlan({
      shell,
      typedConfirmation: "",
      degraded: false,
      journey,
      walletPhase,
    });
    return (
      <ScreenCard landmark="section" title={copyEntry(titleKey).message} dataApplication="exit">
        <StatusPill status={status} />
        {!checkingUnknown ? null : (
          <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={unknownResolved} />
        )}
        {plan.timeline === undefined ? null : <JourneyTimelineView rows={plan.timeline} />}
        <JourneyTechnicalDetails journey={journey} />
        {plan.wallet === undefined ? null : (
          <WalletPanelView
            panel={plan.wallet}
            busy={busy}
            onOpen={() => {
              void runOpenWallet();
            }}
          />
        )}
        {plan.refusal === undefined ? null : <RefusalView refusal={plan.refusal} onNavigate={navigate} />}
        {plan.complete === undefined ? null : (
          <CompleteView titleKey={plan.complete.titleKey} bodyKey={plan.complete.bodyKey} />
        )}
      </ScreenCard>
    );
  }

  return (
    <ScreenCard landmark="section" title={copyEntry(titleKey).message} dataApplication="journey">
      <StatusPill status={status} />
      {!checkingUnknown ? null : (
        <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={unknownResolved} />
      )}
      <JourneyTimelineView rows={journeyTimeline(journey)} />
      <JourneyTechnicalDetails journey={journey} />
    </ScreenCard>
  );
}
