"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import { humanApi, type Agent, type HumanApiClient, type Journey, type MoveQuote } from "../../api/index.ts";
import { CopyableIdentifier, LabelValue, StatusPill } from "../../kit/money";
import { DesktopDetail, DesktopWizard, MobileDetail, MobileWizard, type WizardProps } from "../../kit/patterns";
import { InlineNotice, ScreenCard, StateFrame } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { KitList, KitListItem, KitOptionList, KitTextField } from "../../kit/display";
import { statusPresentation } from "../../kit/model";
import { useShellSelection } from "../../shell/app-shell";
import { PrivateFigure } from "../../settings/privacy";
import { LoadingSurface, OfflineSurface, StillCheckingSurface } from "../../states/surfaces";
import {
  HOME_ROUTE,
  MOVE_CURRENCY,
  OTHER_ACCOUNT_OPTION,
  clearPendingMove,
  commitMove,
  destinationOptions,
  moveFailure,
  moveResult,
  moveReview,
  moveSteps,
  moveSummary,
  newMoveAttemptKey,
  parseMoveAmount,
  pendingMoveAttempt,
  persistPendingMove,
  quoteExpired,
  requestMoveQuote,
  stillCheckingLookup,
  storedAccount,
  type PendingMoveAttempt,
  type MoveFailure,
} from "./model.ts";

type QuoteState =
  | Readonly<{ kind: "idle" }>
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "quoted"; quote: MoveQuote; attemptKey: string; source: string }>
  | Readonly<{ kind: "failed"; failure: MoveFailure }>;

type MovePhase =
  | Readonly<{ kind: "wizard" }>
  | Readonly<{ kind: "committing" }>
  | Readonly<{ kind: "uncertain"; message: string; attempt: PendingMoveAttempt }>
  | Readonly<{ kind: "result"; journey: Journey }>
  | Readonly<{ kind: "failed"; failure: MoveFailure }>;

type AgentLoad =
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "loaded"; agents: readonly Agent[] }>
  | Readonly<{ kind: "offline" }>
  | Readonly<{ kind: "error" }>;

function FailureView({
  failure,
  onRetry,
}: Readonly<{ failure: MoveFailure; onRetry: () => void }>) {
  if (failure.kind === "offline") {
    return <OfflineSurface onRetry={onRetry} />;
  }
  if (failure.kind === "refused") {
    return (
      <StateFrame
        tone="danger"
        role="alert"
        title={statusPresentation("refused").label}
        description={failure.message}
      >
        <p className="text-sm">{failure.moneyLeftMessage}</p>
      </StateFrame>
    );
  }
  return (
    <StateFrame tone="danger" role="alert" title={copyEntry("state.error").message} description={failure.message}>
      <div className="flex flex-wrap gap-3">
        <KitButton variant="primary" onClick={onRetry}>{copyEntry("action.retry").message}</KitButton>
      </div>
    </StateFrame>
  );
}

function ReviewPane({
  quote,
  begin,
}: Readonly<{ quote: QuoteState; begin: () => void }>) {
  useEffect(() => {
    if (quote.kind === "idle") {
      begin();
    }
  }, [quote.kind, begin]);

  if (quote.kind === "idle" || quote.kind === "loading") {
    return <LoadingSurface rows={2} />;
  }
  if (quote.kind === "failed") {
    return <FailureView failure={quote.failure} onRetry={begin} />;
  }
  const review = moveReview(quote.quote);
  return (
    <div className="flex flex-col gap-4">
      <p className="font-semibold text-foreground">{review.headline}</p>
      <LabelValue
        label={copyEntry("move.summary.amount").message}
        value={<PrivateFigure>{review.amount}</PrivateFigure>}
      />
      <p className="text-sm text-foreground-secondary">
        <PrivateFigure>{review.fee}</PrivateFigure>
      </p>
      <p className="text-sm text-foreground-secondary">{review.arrival}</p>
      <p className="text-sm text-foreground-secondary">{review.expires}</p>
      <LabelValue
        label={copyEntry("move.summary.route").message}
        value={`${review.route} — ${copyEntry("move.route.automatic").message}`}
      />
      {review.irreversibility === undefined ? null : (
        <InlineNotice tone="warning" role="status">{review.irreversibility}</InlineNotice>
      )}
    </div>
  );
}

function MoveResultView({
  client,
  journey,
  onJourney,
  onAgain,
  onHome,
  onChangePath,
}: Readonly<{
  client: HumanApiClient;
  journey: Journey;
  onJourney: (journey: Journey) => void;
  onAgain: () => void;
  onHome: () => void;
  onChangePath: (path: string) => void;
}>) {
  const shell = useShellSelection().shell;
  const Detail = shell === "mobile" ? MobileDetail : DesktopDetail;
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const [refreshFailure, setRefreshFailure] = useState<"offline" | "error" | undefined>(undefined);
  const result = moveResult(journey);
  const journeyId = journey.journey_id;
  const lookup = useMemo(
    () => stillCheckingLookup(client, journeyId, onJourney),
    [client, journeyId, onJourney],
  );
  const refresh = useCallback(async () => {
    try {
      const latest = await client.journeyGet(journeyId);
      setRefreshFailure(undefined);
      onJourney(latest);
    } catch (error) {
      setRefreshFailure(moveFailure(error).kind === "offline" ? "offline" : "error");
    }
  }, [client, journeyId, onJourney]);

  useEffect(() => {
    if (result.kind !== "in-progress" && result.kind !== "waiting") {
      return;
    }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      await refresh();
      if (!cancelled) {
        timer = setTimeout(() => { void check(); }, 3_000);
      }
    };
    timer = setTimeout(() => { void check(); }, 3_000);
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }, [journey.updated_at, refresh, result.kind]);

  if (result.kind === "still-checking") {
    return (
      <StillCheckingSurface lookupOutcome={lookup} onResolved={() => undefined}>
        <KitList>
          {result.stages.map((stage) => (
            <KitListItem
              key={stage.id}
              title={stage.label}
              trailing={<StatusPill status={stage.status} />}
            />
          ))}
        </KitList>
      </StillCheckingSurface>
    );
  }
  if (result.kind === "refused") {
    return (
      <StateFrame
        tone="danger"
        role="alert"
        title={statusPresentation(result.status).label}
        description={result.message}
      >
        <KitList>
          {result.stages.map((stage) => (
            <KitListItem
              key={stage.id}
              title={stage.label}
              trailing={<StatusPill status={stage.status} />}
            />
          ))}
        </KitList>
        <p className="text-sm">{result.moneyLeftMessage}</p>
        <div className="flex flex-wrap gap-3">
          {result.changePath === undefined ? null : (
            <KitButton
              variant="secondary"
              onClick={() => {
                if (result.changePath !== undefined) {
                  onChangePath(result.changePath);
                }
              }}
            >
              {copyEntry("move.result.change").message}
            </KitButton>
          )}
          <KitButton variant="primary" onClick={onAgain}>{copyEntry("move.result.start_again").message}</KitButton>
          <KitButton variant="secondary" onClick={onHome}>{copyEntry("move.result.home").message}</KitButton>
        </div>
      </StateFrame>
    );
  }

  const stages = (
    <KitList>
      {result.stages.map((stage) => (
        <KitListItem key={stage.id} title={stage.label} trailing={<StatusPill status={stage.status} />} />
      ))}
    </KitList>
  );

  if (result.kind === "done") {
    return (
      <StateFrame
        tone="success"
        role="status"
        title={statusPresentation(result.status).label}
        description={result.receiptMessage}
      >
        {stages}
        <Detail
          open={technicalOpen}
          onOpenChange={setTechnicalOpen}
          title={copyEntry("error.technical.title").message}
          summary={copyEntry("error.technical.title").message}
          mobileVariant="sheet"
          desktopVariant="inline"
        >
          <div className="flex flex-col gap-3">
            {result.receipts.map((receipt, index) => {
              const receiptNumber = index + 1;
              return (
                <CopyableIdentifier
                  key={receipt.evidence_id}
                  label={formatCopy("move.result.receipt_reference", { number: receiptNumber })}
                  value={receipt.evidence_id}
                />
              );
            })}
          </div>
        </Detail>
        <div className="flex flex-wrap gap-3">
          <KitButton variant="primary" onClick={onAgain}>{copyEntry("move.result.start_again").message}</KitButton>
          <KitButton variant="secondary" onClick={onHome}>{copyEntry("move.result.home").message}</KitButton>
        </div>
      </StateFrame>
    );
  }

  return (
    <StateFrame role="status" busy title={statusPresentation(result.status).label}>
      {stages}
      {refreshFailure === undefined ? null : (
        <InlineNotice tone="warning" role="status">
          {copyEntry(
            refreshFailure === "offline"
              ? "move.result.updates_offline"
              : "move.result.updates_unavailable",
          ).message}
        </InlineNotice>
      )}
      <div className="flex flex-wrap gap-3">
        <KitButton variant="secondary" onClick={() => { void refresh(); }}>
          {copyEntry("action.retry").message}
        </KitButton>
        <KitButton variant="secondary" onClick={onHome}>{copyEntry("move.result.home").message}</KitButton>
      </div>
    </StateFrame>
  );
}

export function MoveMoney() {
  const router = useRouter();
  const shell = useShellSelection().shell;
  const client = useMemo(() => humanApi(), []);
  const [agentLoad, setAgentLoad] = useState<AgentLoad>({ kind: "loading" });
  const [destinationChoice, setDestinationChoice] = useState("");
  const [otherAccount, setOtherAccount] = useState("");
  const [amountText, setAmountText] = useState("");
  const [quote, setQuote] = useState<QuoteState>({ kind: "idle" });
  const [notice, setNotice] = useState<string | undefined>(undefined);
  const [phase, setPhase] = useState<MovePhase>({ kind: "wizard" });
  const agentRequest = useRef(0);
  const quoteRequest = useRef(0);

  const loadAgents = useCallback(async () => {
    const request = ++agentRequest.current;
    setAgentLoad({ kind: "loading" });
    try {
      const page = await client.agentList();
      if (request === agentRequest.current) {
        setAgentLoad({ kind: "loaded", agents: page.agents });
      }
    } catch (error) {
      if (request === agentRequest.current) {
        setAgentLoad({ kind: moveFailure(error).kind === "offline" ? "offline" : "error" });
      }
    }
  }, [client]);

  useEffect(() => {
    void loadAgents();
    const pending = pendingMoveAttempt(
      window.localStorage,
      storedAccount(window.localStorage),
    );
    if (pending !== undefined) {
      setPhase({
        kind: "uncertain",
        message: copyEntry("move.result.checking.body").message,
        attempt: pending,
      });
    }
    return () => {
      agentRequest.current += 1;
      quoteRequest.current += 1;
    };
  }, [loadAgents]);

  const agents = agentLoad.kind === "loaded" ? agentLoad.agents : [];
  const amount = parseMoveAmount(amountText);
  const destination =
    destinationChoice === OTHER_ACCOUNT_OPTION
      ? otherAccount.trim().length > 0
        ? otherAccount.trim()
        : undefined
      : destinationChoice.length > 0
        ? destinationChoice
        : undefined;
  const destinationLabel =
    destinationChoice === OTHER_ACCOUNT_OPTION
      ? destination
      : agents.find((agent) => agent.agent_id === destinationChoice)?.name;

  useEffect(() => {
    quoteRequest.current += 1;
    setQuote({ kind: "idle" });
  }, [destination, amountText]);

  const beginQuote = useCallback(() => {
    const source = storedAccount(window.localStorage);
    if (source === undefined || destination === undefined || amount === undefined) {
      setQuote({
        kind: "failed",
        failure: {
          kind: "error",
          message: copyEntry(
            source === undefined ? "move.source.unavailable" : "state.error.body",
          ).message,
        },
      });
      return;
    }
    const request = ++quoteRequest.current;
    setQuote({ kind: "loading" });
    void requestMoveQuote(client, { source, destination, amount }).then((outcome) => {
      if (request === quoteRequest.current) {
        setQuote(
          outcome.kind === "quoted"
            ? {
                kind: "quoted",
                quote: outcome.quote,
                attemptKey: newMoveAttemptKey(),
                source,
              }
            : { kind: "failed", failure: outcome.failure },
        );
      }
    });
  }, [client, destination, amount]);

  const complete = useCallback((recovering = false) => {
    const attempt = recovering && phase.kind === "uncertain"
      ? phase.attempt
      : quote.kind === "quoted"
        ? {
            source: quote.source,
            quoteId: quote.quote.quote_id,
            attemptKey: quote.attemptKey,
          }
        : undefined;
    if (attempt === undefined) {
      return;
    }
    if (!recovering && quote.kind === "quoted" && quoteExpired(quote.quote)) {
      setQuote({ kind: "idle" });
      setNotice(copyEntry("error.move.quote-expired").message);
      setPhase({ kind: "wizard" });
      return;
    }
    persistPendingMove(window.localStorage, attempt);
    setPhase({ kind: "committing" });
    void commitMove(client, attempt.quoteId, attempt.attemptKey).then((outcome) => {
      if (outcome.kind === "journey") {
        clearPendingMove(window.localStorage);
        setPhase({ kind: "result", journey: outcome.journey });
        return;
      }
      if (outcome.kind === "uncertain") {
        setPhase({ kind: "uncertain", message: outcome.message, attempt });
        return;
      }
      clearPendingMove(window.localStorage);
      if (outcome.failure.kind === "quote-expired") {
        setQuote({ kind: "idle" });
        setNotice(outcome.failure.message);
        setPhase({ kind: "wizard" });
        return;
      }
      setPhase({ kind: "failed", failure: outcome.failure });
    });
  }, [client, phase, quote]);

  const reset = useCallback(() => {
    quoteRequest.current += 1;
    clearPendingMove(window.localStorage);
    setDestinationChoice("");
    setOtherAccount("");
    setAmountText("");
    setQuote({ kind: "idle" });
    setNotice(undefined);
    setPhase({ kind: "wizard" });
  }, []);

  const goHome = useCallback(() => {
    router.push(HOME_ROUTE);
  }, [router]);

  const acceptJourney = useCallback((journey: Journey) => {
    setPhase({ kind: "result", journey });
  }, []);

  const changePath = useCallback((path: string) => {
    router.push(path);
  }, [router]);

  const stepCopy = moveSteps();
  const steps: WizardProps["steps"] = [
    {
      id: stepCopy[0].id,
      label: stepCopy[0].label,
      title: stepCopy[0].title,
      ...(stepCopy[0].description === undefined ? {} : { description: stepCopy[0].description }),
      render: () => (
        <div className="flex flex-col gap-4">
          {agentLoad.kind === "loading" ? (
            <InlineNotice tone="neutral" role="status">
              {copyEntry("move.destination.loading").message}
            </InlineNotice>
          ) : agentLoad.kind === "loaded" ? null : (
            <InlineNotice tone="warning" role="status">
              <span>
                {copyEntry(
                  agentLoad.kind === "offline"
                    ? "move.destination.offline"
                    : "move.destination.unavailable",
                ).message}
              </span>
              <KitButton variant="secondary" size="sm" onClick={() => { void loadAgents(); }}>
                {copyEntry("action.retry").message}
              </KitButton>
            </InlineNotice>
          )}
          <KitOptionList
            aria-label={stepCopy[0].title}
            items={[...destinationOptions(agents)]}
            value={destinationChoice}
            onValueChange={setDestinationChoice}
          />
          {destinationChoice === OTHER_ACCOUNT_OPTION ? (
            <KitTextField
              label={copyEntry("move.destination.other.hint").message}
              value={otherAccount}
              onChange={(event) => {
                setOtherAccount(event.target.value);
              }}
            />
          ) : null}
        </div>
      ),
      canContinue: () => destination !== undefined,
    },
    {
      id: stepCopy[1].id,
      label: stepCopy[1].label,
      title: stepCopy[1].title,
      render: () => (
        <KitTextField
          label={formatCopy("move.amount.hint", { currency: MOVE_CURRENCY })}
          inputMode="numeric"
          value={amountText}
          onChange={(event) => {
            setAmountText(event.target.value);
          }}
        />
      ),
      canContinue: () => amount !== undefined,
    },
    {
      id: stepCopy[2].id,
      label: stepCopy[2].label,
      title: stepCopy[2].title,
      render: () => <ReviewPane quote={quote} begin={beginQuote} />,
      canContinue: () => quote.kind === "quoted" && !quoteExpired(quote.quote),
    },
  ];

  const summary = moveSummary(
    {
      ...(destinationLabel === undefined ? {} : { destinationLabel }),
      ...(amount === undefined ? {} : { amount }),
      currency: MOVE_CURRENCY,
    },
    quote.kind === "quoted" ? quote.quote : undefined,
  ).map((item) => ({
    label: item.label,
    value: item.privateFigure ? <PrivateFigure>{item.value}</PrivateFigure> : item.value,
  }));

  const WizardFor = shell === "mobile" ? MobileWizard : DesktopWizard;

  return (
    <ScreenCard landmark="section" title={copyEntry("move.title").message}>
      {phase.kind === "committing" ? (
        <LoadingSurface rows={2} />
      ) : phase.kind === "uncertain" ? (
        <StateFrame
          tone="warning"
          role="status"
          title={copyEntry("move.result.checking").message}
          description={phase.message}
        >
          <KitButton disabled disabledReason={copyEntry("state.still_checking.locked").message}>
            {copyEntry("action.send_again").message}
          </KitButton>
          <div className="flex flex-wrap gap-3">
            <KitButton variant="primary" onClick={() => { complete(true); }}>
              {copyEntry("move.result.check").message}
            </KitButton>
            <KitButton variant="secondary" onClick={goHome}>
              {copyEntry("move.result.home").message}
            </KitButton>
          </div>
        </StateFrame>
      ) : phase.kind === "result" ? (
        <MoveResultView
          client={client}
          journey={phase.journey}
          onJourney={acceptJourney}
          onAgain={reset}
          onHome={goHome}
          onChangePath={changePath}
        />
      ) : phase.kind === "failed" ? (
        <div className="flex flex-col gap-4">
          <FailureView failure={phase.failure} onRetry={() => { complete(false); }} />
          <div className="flex flex-wrap gap-3">
            <KitButton variant="primary" onClick={reset}>{copyEntry("move.result.start_again").message}</KitButton>
            <KitButton variant="secondary" onClick={goHome}>{copyEntry("move.result.home").message}</KitButton>
          </div>
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          {notice === undefined ? null : (
            <InlineNotice tone="warning" role="alert">{notice}</InlineNotice>
          )}
          <WizardFor
            steps={steps}
            summary={[...summary]}
            onComplete={() => { complete(false); }}
            onCancel={goHome}
            completeLabel={copyEntry("home.actions.move").message}
            summaryTitle={copyEntry("move.review.what").message}
          />
        </div>
      )}
    </ScreenCard>
  );
}
