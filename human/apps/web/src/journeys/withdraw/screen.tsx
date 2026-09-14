"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import { humanApi } from "../../api/index.ts";
import { DesktopWizard, MobileWizard } from "../../kit/pattern-wizard";
import { InlineNotice, ScreenCard, StateFrame } from "../../kit/surface";
import { LabelValue, StatusPill } from "../../kit/money";
import { TextField } from "../../kit/field";
import { ErrorSurface, errorPresentation, StillCheckingSurface } from "../../states";
import { PrivateFigure } from "../../settings/privacy";
import { presentedJourneyState, statusKeyForState } from "../custody/evidence.ts";
import { browserWalletBridge, windowWalletProvider } from "../custody/handoff.ts";
import { CUSTODY_CURRENCY, validateDestinationAddress, validatePositiveAmount } from "../custody/model.ts";
import { isJourneyOutcomeUnknown, mutationOutcomeIsUnknown } from "../custody/recovery.ts";
import type { CustodyTiming } from "../custody/time.ts";
import {
  CompleteView,
  JourneyTechnicalDetails,
  JourneyTimelineView,
  RefusalView,
  useCustodyShell,
  WalletPanelView,
} from "../custody/timeline";
import {
  WITHDRAW_FINAL_STAGE,
  WithdrawController,
  withdrawPlan,
  type ChallengeHoldPresentation,
  type SettlementPresentation,
} from "./model.ts";

const REFRESH_INTERVAL_MS = 5_000;

export function SettlementNotice({ settlement }: Readonly<{ settlement: SettlementPresentation }>) {
  return (
    <InlineNotice tone="neutral" role="status">
      {settlement.duration === undefined
        ? copyEntry(settlement.bodyKey).message
        : formatCopy(settlement.bodyKey, { duration: settlement.duration })}
    </InlineNotice>
  );
}

export function ChallengeHoldView({ hold }: Readonly<{ hold: ChallengeHoldPresentation }>) {
  return (
    <StateFrame
      title={copyEntry(hold.titleKey).message}
      description={copyEntry(hold.bodyKey).message}
      tone={hold.cancelledKey === undefined ? "warning" : "danger"}
      role={hold.cancelledKey === undefined ? "status" : "alert"}
    >
      {hold.expectation === undefined ? null : (
        <p className="text-sm">
          {formatCopy(hold.expectation.bodyKey, { duration: hold.expectation.duration })}
        </p>
      )}
      {hold.cancelledKey === undefined ? null : (
        <p className="text-sm font-semibold">{copyEntry(hold.cancelledKey).message}</p>
      )}
    </StateFrame>
  );
}

export function Withdraw({ timing }: Readonly<{ timing: CustodyTiming }>) {
  const shell = useCustodyShell();
  const router = useRouter();
  const controller = useMemo(
    () => new WithdrawController({ api: humanApi(), bridge: browserWalletBridge(windowWalletProvider) }),
    [],
  );
  const [amountInput, setAmountInput] = useState("");
  const [destinationInput, setDestinationInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<unknown>(undefined);
  const [, setVersion] = useState(0);
  const sync = useCallback(() => {
    setVersion((version) => version + 1);
  }, []);

  const journey = controller.journey;
  const presented = journey === undefined
    ? undefined
    : presentedJourneyState(journey, WITHDRAW_FINAL_STAGE);
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
        presentedJourneyState(outcome.journey, WITHDRAW_FINAL_STAGE) !== "still-checking"
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
        route="/app/withdraw"
        platform={shell}
        onRetry={() => {
          setFailure(undefined);
        }}
      />
    );
  }

  const plan = withdrawPlan({
    shell,
    timing,
    nowMs: Date.now(),
    amountInput,
    destinationInput,
    ...(journey === undefined ? {} : { journey }),
    walletPhase: controller.walletPhase,
  });

  const runCommit = async () => {
    const amount = validatePositiveAmount(amountInput);
    const destination = validateDestinationAddress(destinationInput);
    if (amount === undefined || destination === undefined) {
      return;
    }
    setBusy(true);
    try {
      await controller.commit(amount, CUSTODY_CURRENCY, destination);
    } catch (error) {
      if (!isJourneyOutcomeUnknown(error)) {
        setFailure(error);
      }
    } finally {
      setBusy(false);
      sync();
    }
  };

  const runClaim = async () => {
    setBusy(true);
    try {
      await controller.openWalletToClaim();
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
        dataApplication="withdraw"
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
    const Wizard = shell === "mobile" ? MobileWizard : DesktopWizard;
    return (
      <ScreenCard
        landmark="section"
        title={copyEntry(plan.titleKey).message}
        description={copyEntry(plan.summaryKey).message}
        dataApplication="withdraw"
      >
        <Wizard
          steps={[
            {
              id: "amount",
              label: copyEntry(plan.amount.labelKey).message,
              title: copyEntry(plan.amount.labelKey).message,
              canContinue: () => validatePositiveAmount(amountInput) !== undefined,
              render: () => (
                <TextField
                  label={copyEntry(plan.amount.labelKey).message}
                  value={amountInput}
                  inputMode="numeric"
                  autoComplete="off"
                  onChange={(event) => {
                    setAmountInput(event.target.value);
                  }}
                  {...(plan.amount.errorKey === undefined
                    ? {}
                    : { errorMessage: copyEntry(plan.amount.errorKey).message })}
                />
              ),
            },
            {
              id: "destination",
              label: copyEntry(plan.destination.labelKey).message,
              title: copyEntry(plan.destination.labelKey).message,
              canContinue: () => validateDestinationAddress(destinationInput) !== undefined,
              render: () => (
                <TextField
                  label={copyEntry(plan.destination.labelKey).message}
                  value={destinationInput}
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(event) => {
                    setDestinationInput(event.target.value);
                  }}
                  {...(plan.destination.errorKey === undefined
                    ? {}
                    : { errorMessage: copyEntry(plan.destination.errorKey).message })}
                />
              ),
            },
            {
              id: "review",
              label: copyEntry(plan.review.titleKey).message,
              title: copyEntry(plan.review.titleKey).message,
              description: copyEntry(plan.review.irreversibleKey).message,
              canContinue: () => plan.review.ready && !busy,
              render: () => (
                <div className="flex flex-col gap-3">
                  <InlineNotice tone="warning" role="status">
                    {copyEntry(plan.review.irreversibleKey).message}
                  </InlineNotice>
                  <SettlementNotice settlement={plan.settlement} />
                </div>
              ),
            },
          ]}
          summary={plan.summaryItems.map((item) => ({
            label: copyEntry(item.labelKey).message,
            value: item.labelKey === "withdraw.amount.label"
              ? <PrivateFigure>{item.value} {CUSTODY_CURRENCY}</PrivateFigure>
              : item.value,
          }))}
          completeLabel={copyEntry(plan.review.commitKey).message}
          onComplete={() => {
            void runCommit();
          }}
        />
      </ScreenCard>
    );
  }

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry(plan.titleKey).message}
      dataApplication="withdraw"
    >
      {journey === undefined ? null : (
        <StatusPill status={statusKeyForState(presentedJourneyState(journey, WITHDRAW_FINAL_STAGE))} />
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
      <SettlementNotice settlement={plan.settlement} />
      {plan.hold === undefined ? null : <ChallengeHoldView hold={plan.hold} />}
      {plan.timeline === undefined ? null : <JourneyTimelineView rows={plan.timeline} />}
      {journey === undefined ? null : <JourneyTechnicalDetails journey={journey} />}
      {plan.claim === undefined ? null : (
        <StateFrame
          title={copyEntry(plan.claim.titleKey).message}
          description={copyEntry(plan.claim.bodyKey).message}
          role="status"
        />
      )}
      {plan.wallet === undefined ? null : (
        <WalletPanelView
          panel={plan.wallet}
          busy={busy}
          onOpen={() => {
            void runClaim();
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

export const WithdrawScreen = Withdraw;
