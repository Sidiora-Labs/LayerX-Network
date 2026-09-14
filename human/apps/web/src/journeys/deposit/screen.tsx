"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { humanApi } from "../../api/index.ts";
import type { WalletBinding } from "../../api/index.ts";
import { DesktopPrimaryAction, MobilePrimaryAction } from "../../kit/pattern-primary-action";
import { InlineNotice, ScreenCard } from "../../kit/surface";
import { LabelValue, StatusPill } from "../../kit/money";
import { TextField } from "../../kit/field";
import { ErrorSurface, errorPresentation, StillCheckingSurface } from "../../states";
import { PrivateFigure } from "../../settings/privacy";
import { presentedJourneyState, statusKeyForState } from "../custody/evidence.ts";
import { browserWalletBridge, windowWalletProvider } from "../custody/handoff.ts";
import { CUSTODY_CURRENCY, validatePositiveAmount } from "../custody/model.ts";
import { isJourneyOutcomeUnknown, mutationOutcomeIsUnknown } from "../custody/recovery.ts";
import type { CustodyTiming } from "../custody/time.ts";
import {
  CompleteView,
  DelayNotice,
  JourneyTechnicalDetails,
  JourneyTimelineView,
  RefusalView,
  SafeToCloseNotice,
  useCustodyShell,
  WalletPanelView,
} from "../custody/timeline";
import { DEPOSIT_FINAL_STAGE, DepositController, depositPlan } from "./model.ts";

const REFRESH_INTERVAL_MS = 5_000;

export function Deposit({ timing }: Readonly<{ timing: CustodyTiming }>) {
  const shell = useCustodyShell();
  const router = useRouter();
  const controller = useMemo(
    () => new DepositController({ api: humanApi(), bridge: browserWalletBridge(windowWalletProvider) }),
    [],
  );
  const [amountInput, setAmountInput] = useState("");
  const [binding, setBinding] = useState<WalletBinding | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<unknown>(undefined);
  const [, setVersion] = useState(0);
  const sync = useCallback(() => {
    setVersion((version) => version + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void humanApi()
      .bindingStatus()
      .then((status) => {
        if (!cancelled) {
          setBinding(status);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const journey = controller.journey;
  const presented = journey === undefined
    ? undefined
    : presentedJourneyState(journey, DEPOSIT_FINAL_STAGE);
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
        presentedJourneyState(outcome.journey, DEPOSIT_FINAL_STAGE) !== "still-checking"
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
        route="/app/deposit"
        platform={shell}
        onRetry={() => {
          setFailure(undefined);
        }}
      />
    );
  }

  const plan = depositPlan({
    shell,
    timing,
    nowMs: Date.now(),
    amountInput,
    ...(binding === undefined ? {} : { binding }),
    ...(journey === undefined ? {} : { journey }),
    walletPhase: controller.walletPhase,
  });
  const PrimaryAction = shell === "mobile" ? MobilePrimaryAction : DesktopPrimaryAction;

  const runStart = async () => {
    const amount = validatePositiveAmount(amountInput);
    if (amount === undefined) {
      return;
    }
    setBusy(true);
    try {
      await controller.start(amount, CUSTODY_CURRENCY);
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
        dataApplication="deposit"
      >
        {amountInput.length === 0 ? null : (
          <LabelValue
            label={copyEntry(plan.amount.labelKey).message}
            value={<PrivateFigure>{amountInput} {CUSTODY_CURRENCY}</PrivateFigure>}
          />
        )}
        <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={sync} />
      </ScreenCard>
    );
  }

  if (plan.phase === "form") {
    return (
      <ScreenCard
        landmark="section"
        title={copyEntry(plan.titleKey).message}
        description={copyEntry(plan.summaryKey).message}
        dataApplication="deposit"
      >
        {plan.bindingFolded ? (
          <InlineNotice tone="neutral" role="status">
            {copyEntry("deposit.wallet.linking").message}
          </InlineNotice>
        ) : (
          <LabelValue
            label={copyEntry("deposit.wallet.label").message}
            value={binding?.address ?? ""}
          />
        )}
        <TextField
          label={copyEntry(plan.amount.labelKey).message}
          value={plan.amount.value}
          inputMode="numeric"
          autoComplete="off"
          onChange={(event) => {
            setAmountInput(event.target.value);
          }}
          {...(plan.amount.errorKey === undefined
            ? {}
            : { errorMessage: copyEntry(plan.amount.errorKey).message })}
        />
        <PrimaryAction
          onClick={() => {
            void runStart();
          }}
          loading={busy}
          {...(plan.primaryAction.disabled
            ? {
                disabled: true as const,
                disabledReason: copyEntry(plan.primaryAction.disabledReasonKey ?? "confirmation.incomplete").message,
              }
            : {})}
        >
          {copyEntry(plan.primaryAction.labelKey).message}
        </PrimaryAction>
      </ScreenCard>
    );
  }

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry(plan.titleKey).message}
      dataApplication="deposit"
    >
      {journey === undefined ? null : (
        <StatusPill status={statusKeyForState(presentedJourneyState(journey, DEPOSIT_FINAL_STAGE))} />
      )}
      {amountInput.length === 0 ? null : (
        <LabelValue
          label={copyEntry(plan.amount.labelKey).message}
          value={<PrivateFigure>{amountInput} {CUSTODY_CURRENCY}</PrivateFigure>}
        />
      )}
      {!checkingUnknown ? null : (
        <StillCheckingSurface lookupOutcome={lookupUnknown} onResolved={sync} />
      )}
      {plan.delayed === undefined ? null : <DelayNotice duration={plan.delayed.duration} />}
      {plan.safeToCloseKey === undefined ? null : <SafeToCloseNotice messageKey={plan.safeToCloseKey} />}
      {plan.pendingHonestyKey === undefined ? null : (
        <p className="text-sm text-foreground-secondary">{copyEntry(plan.pendingHonestyKey).message}</p>
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

export const DepositScreen = Deposit;
