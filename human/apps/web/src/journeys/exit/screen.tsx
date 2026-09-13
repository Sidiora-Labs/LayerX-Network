"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { humanApi } from "../../api/index.ts";
import { DesktopConfirmation, MobileConfirmation } from "../../kit/confirm";
import { DesktopPrimaryAction, MobilePrimaryAction } from "../../kit/patterns";
import { InlineNotice, ScreenCard, StateFrame } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { StatusPill } from "../../kit/money";
import { ErrorSurface, errorPresentation, LoadingSurface, StillCheckingSurface } from "../../states";
import { presentedJourneyState, statusKeyForState } from "../custody/evidence.ts";
import { browserWalletBridge, windowWalletProvider } from "../custody/handoff.ts";
import { isJourneyOutcomeUnknown, mutationOutcomeIsUnknown } from "../custody/recovery.ts";
import {
  CompleteView,
  JourneyTechnicalDetails,
  JourneyTimelineView,
  RefusalView,
  useCustodyShell,
  WalletPanelView,
} from "../custody/timeline";
import { EXIT_FINAL_STAGE, ExitController, exitPlan } from "./model.ts";

const REFRESH_INTERVAL_MS = 5_000;

export function Exit() {
  const shell = useCustodyShell();
  const router = useRouter();
  const controller = useMemo(
    () => new ExitController({ api: humanApi(), bridge: browserWalletBridge(windowWalletProvider) }),
    [],
  );
  const [typedConfirmation, setTypedConfirmation] = useState("");
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [degraded, setDegraded] = useState(false);
  const [checked, setChecked] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<unknown>(undefined);
  const [, setVersion] = useState(0);
  const sync = useCallback(() => {
    setVersion((version) => version + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void controller
      .checkEligibility()
      .then(() => {
        if (!cancelled) {
          setChecked(true);
          sync();
        }
      })
      .catch(() => {
        if (!cancelled) {
          setDegraded(true);
          setChecked(true);
          sync();
        }
      });
    return () => {
      cancelled = true;
    };
  }, [controller, sync]);

  const journey = controller.journey;
  const presented = journey === undefined
    ? undefined
    : presentedJourneyState(journey, EXIT_FINAL_STAGE);
  const checkingUnknown = controller.outcomeUnknown || presented === "still-checking";
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
      void controller
        .refresh()
        .then(sync)
        .catch(() => undefined);
    }, REFRESH_INTERVAL_MS);
    return () => {
      clearInterval(timer);
    };
  }, [active, controller, sync]);

  const lookupUnknown = useCallback(async (): Promise<"pending" | "resolved"> => {
    try {
      const outcome = await controller.recoverUnknown();
      sync();
      return outcome.resolved &&
        presentedJourneyState(outcome.journey, EXIT_FINAL_STAGE) !== "still-checking"
        ? "resolved"
        : "pending";
    } catch (error) {
      if (controller.unknownRecoveryMode === "start" && !mutationOutcomeIsUnknown(error)) {
        setFailure(error);
        return "resolved";
      }
      return "pending";
    }
  }, [controller, sync]);

  if (failure !== undefined) {
    return (
      <ErrorSurface
        error={errorPresentation(failure)}
        route="/app/settings/exit"
        platform={shell}
        onRetry={() => {
          setFailure(undefined);
        }}
      />
    );
  }

  const plan = exitPlan({
    shell,
    typedConfirmation,
    degraded,
    ...(controller.eligibility === undefined ? {} : { eligibility: controller.eligibility }),
    ...(journey === undefined ? {} : { journey }),
    walletPhase: controller.walletPhase,
  });
  const PrimaryAction = shell === "mobile" ? MobilePrimaryAction : DesktopPrimaryAction;
  const Confirmation = shell === "mobile" ? MobileConfirmation : DesktopConfirmation;

  const runStart = async () => {
    setBusy(true);
    try {
      await controller.start(typedConfirmation);
      setConfirmOpen(false);
    } catch (error) {
      if (!isJourneyOutcomeUnknown(error)) {
        setFailure(error);
      }
    } finally {
      setBusy(false);
      sync();
    }
  };

  const runOpenWallet = async () => {
    setBusy(true);
    try {
      await controller.openWallet();
    } catch (error) {
      if (!isJourneyOutcomeUnknown(error)) {
        setFailure(error);
      }
    } finally {
      setBusy(false);
      sync();
    }
  };

  if (checkingUnknown && journey === undefined) {
    return (
      <ScreenCard
        landmark="section"
        title={copyEntry(plan.titleKey).message}
        description={copyEntry(plan.summaryKey).message}
        dataApplication="exit"
      >
        <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={sync} />
      </ScreenCard>
    );
  }

  if (!checked && plan.phase === "checking") {
    return <LoadingSurface />;
  }

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry(plan.titleKey).message}
      description={copyEntry(plan.summaryKey).message}
      dataApplication="exit"
    >
      {plan.degradedKey === undefined ? null : (
        <InlineNotice tone="warning" role="status">
          {copyEntry(plan.degradedKey).message}
        </InlineNotice>
      )}
      {plan.phase === "unavailable" && plan.unavailable !== undefined ? (
        <StateFrame
          title={copyEntry(plan.titleKey).message}
          description={copyEntry(plan.unavailable.bodyKey).message}
          role="status"
        >
          {plan.unavailable.withdrawInsteadPath === undefined ||
          plan.unavailable.withdrawInsteadKey === undefined ? null : (
            <KitButton
              variant="primary"
              onClick={() => {
                router.push(plan.unavailable?.withdrawInsteadPath ?? "/app/withdraw");
              }}
            >
              {copyEntry(plan.unavailable.withdrawInsteadKey).message}
            </KitButton>
          )}
        </StateFrame>
      ) : null}
      {plan.phase === "confirm" && plan.confirmation !== undefined ? (
        <>
          <InlineNotice tone="warning" role="status">
            {copyEntry(plan.confirmation.consequenceKey).message}
          </InlineNotice>
          <PrimaryAction
            onClick={() => {
              setConfirmOpen(true);
            }}
          >
            {copyEntry(plan.confirmation.actionKey).message}
          </PrimaryAction>
          <Confirmation
            open={confirmOpen}
            onOpenChange={setConfirmOpen}
            kind="irreversible"
            title={copyEntry(plan.titleKey).message}
            consequence={copyEntry(plan.confirmation.consequenceKey).message}
            confirmLabel={copyEntry(plan.confirmation.actionKey).message}
            loading={busy}
            typedConfirmation={{
              expectedValue: plan.confirmation.expectedValue,
              value: typedConfirmation,
              onValueChange: setTypedConfirmation,
            }}
            onConfirm={() => {
              void runStart();
            }}
          />
        </>
      ) : null}
      {plan.phase === "journey" && journey !== undefined ? (
        <StatusPill status={statusKeyForState(presentedJourneyState(journey, EXIT_FINAL_STAGE))} />
      ) : null}
      {!checkingUnknown ? null : (
        <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={sync} />
      )}
      {plan.timeline === undefined ? null : <JourneyTimelineView rows={plan.timeline} />}
      {journey === undefined ? null : <JourneyTechnicalDetails journey={journey} />}
      {plan.wallet === undefined ? null : (
        <WalletPanelView
          panel={plan.wallet}
          busy={busy}
          onOpen={() => {
            void runOpenWallet();
          }}
        />
      )}
      {plan.refusal === undefined ? null : (
        <RefusalView
          refusal={plan.refusal}
          onNavigate={(path) => {
            router.push(path);
          }}
        />
      )}
      {plan.complete === undefined ? null : (
        <CompleteView titleKey={plan.complete.titleKey} bodyKey={plan.complete.bodyKey} />
      )}
    </ScreenCard>
  );
}

export const ExitScreen = Exit;
