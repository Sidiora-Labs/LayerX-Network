"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/catalog.ts";
import {
  DesktopWizard,
  InlineNotice,
  KitButton,
  KitTextField,
  MobileWizard,
  ScreenCard,
  StatusPill,
} from "../../kit";
import { PrivateFigure } from "../../settings";
import { StillCheckingSurface } from "../../states";
import {
  AGENT_CURRENCY,
  AGENT_LOCALE,
  Agents,
  apiErrorSentence,
  creationHeadlineKey,
  creationReady,
  creationSteps,
  formatMoney,
  journeyProgress,
  mutationOutcomeUnknown,
  parseMonthlyLimit,
  type AgentsShell,
  type CreationDraft,
  type JourneyProgress,
} from "./model.ts";
import { JourneyStages } from "./progress.tsx";
import { useAgentsShell } from "./shell.ts";

function DraftField({
  labelKey,
  helpKey,
  value,
  numeric,
  onChange,
}: Readonly<{
  labelKey: string;
  helpKey: string;
  value: string;
  numeric?: boolean;
  onChange: (value: string) => void;
}>) {
  return (
    <div className="flex flex-col gap-2">
      <KitTextField
        label={copyEntry(labelKey).message}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
        autoComplete="off"
        spellCheck={false}
        {...(numeric === true ? { inputMode: "numeric" as const } : {})}
      />
      <p className="text-sm text-muted-foreground">{copyEntry(helpKey).message}</p>
    </div>
  );
}

function CreationProgressCard({
  progress,
  onBack,
  onRefresh,
  refreshing,
  errorSentence,
}: Readonly<{
  progress: JourneyProgress;
  onBack: () => void;
  onRefresh: () => void;
  refreshing: boolean;
  errorSentence?: string;
}>) {
  return (
    <ScreenCard
      landmark="section"
      title={copyEntry("agent.create.progress.title").message}
      description={copyEntry("agent.create.progress.body").message}
    >
      <div className="flex items-center gap-3">
        <StatusPill status={progress.statusKey} />
        <p className="text-sm text-foreground-secondary">
          {copyEntry(creationHeadlineKey(progress)).message}
        </p>
      </div>
      <JourneyStages progress={progress} />
      <p className="text-sm text-muted-foreground">
        {copyEntry("agent.create.safe_to_close").message}
      </p>
      {errorSentence === undefined ? null : (
        <InlineNotice tone="danger" role="alert">
          {errorSentence}
        </InlineNotice>
      )}
      <div className="flex flex-wrap gap-2">
        {progress.complete || progress.refusalSentence !== undefined ? null : (
          <KitButton
            variant="primary"
            loading={refreshing}
            onClick={onRefresh}
          >
            {copyEntry("agent.create.check").message}
          </KitButton>
        )}
        <KitButton variant="secondary" onClick={onBack}>
          {copyEntry("action.back_to_agents").message}
        </KitButton>
      </div>
    </ScreenCard>
  );
}

export function AgentCreateJourney({ shell: initialShell }: Readonly<{ shell: AgentsShell }>) {
  const router = useRouter();
  const shell = useAgentsShell(initialShell);
  const agents = useMemo(() => new Agents(), []);
  const [draft, setDraft] = useState<CreationDraft>({
    name: "",
    purpose: "",
    limitInput: "",
    currency: AGENT_CURRENCY,
  });
  const [progress, setProgress] = useState<JourneyProgress | undefined>(undefined);
  const [submitting, setSubmitting] = useState(false);
  const [errorSentence, setErrorSentence] = useState<string | undefined>(undefined);
  const [outcomeUnknown, setOutcomeUnknown] = useState(false);

  const submit = async () => {
    setSubmitting(true);
    setErrorSentence(undefined);
    try {
      setProgress(journeyProgress(await agents.create(draft)));
    } catch (error) {
      if (mutationOutcomeUnknown(error)) {
        setOutcomeUnknown(true);
        setErrorSentence(copyEntry("state.still_checking.body").message);
      } else {
        setErrorSentence(apiErrorSentence(error));
      }
    } finally {
      setSubmitting(false);
    }
  };

  useEffect(() => {
    if (progress === undefined || progress.complete || progress.refusalSentence !== undefined) {
      return;
    }
    let cancelled = false;
    let checking = false;
    const timer = setInterval(() => {
      if (checking) {
        return;
      }
      checking = true;
      void agents.journey(progress.journeyId)
        .then((journey) => {
          if (!cancelled) {
            setProgress(journeyProgress(journey));
            setErrorSentence(undefined);
          }
        })
        .catch((error: unknown) => {
          if (!cancelled) {
            setErrorSentence(apiErrorSentence(error));
          }
        })
        .finally(() => {
          checking = false;
        });
    }, 1_500);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, [agents, progress?.complete, progress?.journeyId, progress?.refusalSentence]);

  const refreshProgress = async () => {
    if (progress === undefined || submitting) {
      return;
    }
    setSubmitting(true);
    setErrorSentence(undefined);
    try {
      setProgress(journeyProgress(await agents.journey(progress.journeyId)));
    } catch (error) {
      setErrorSentence(apiErrorSentence(error));
    } finally {
      setSubmitting(false);
    }
  };

  const lookupUnknownOutcome = useCallback(async (): Promise<"pending" | "resolved"> => {
    try {
      setProgress(journeyProgress(await agents.create(draft)));
      setOutcomeUnknown(false);
      setErrorSentence(undefined);
      return "resolved";
    } catch (error) {
      if (!mutationOutcomeUnknown(error)) {
        setOutcomeUnknown(false);
        setErrorSentence(apiErrorSentence(error));
        return "resolved";
      }
      return "pending";
    }
  }, [agents, draft]);

  const unknownOutcomeResolved = useCallback(() => {
    setOutcomeUnknown(false);
  }, []);

  if (outcomeUnknown) {
    return (
      <StillCheckingSurface
        lookupOutcome={lookupUnknownOutcome}
        onResolved={unknownOutcomeResolved}
      >
        <p className="text-sm text-muted-foreground">
          {copyEntry("agent.create.find_in_list").message}
        </p>
        <div className="flex">
          <KitButton
            variant="secondary"
            onClick={() => {
              router.push("/app/agents");
            }}
          >
            {copyEntry("action.back_to_agents").message}
          </KitButton>
        </div>
      </StillCheckingSurface>
    );
  }

  if (progress !== undefined) {
    return (
      <CreationProgressCard
        progress={progress}
        onBack={() => {
          router.push("/app/agents");
        }}
        onRefresh={() => {
          void refreshProgress();
        }}
        refreshing={submitting}
        {...(errorSentence === undefined ? {} : { errorSentence })}
      />
    );
  }

  const steps = creationSteps(draft);
  const limitMoney = parseMonthlyLimit(draft.limitInput, draft.currency);
  const Wizard = shell === "mobile" ? MobileWizard : DesktopWizard;
  const wizardSteps = [
    {
      id: steps[0].id,
      label: copyEntry(steps[0].labelKey).message,
      title: copyEntry(steps[0].labelKey).message,
      description: copyEntry(steps[0].helpKey).message,
      canContinue: () => steps[0].complete,
      render: () => (
        <DraftField
          labelKey={steps[0].labelKey}
          helpKey={steps[0].helpKey}
          value={draft.name}
          onChange={(value) => {
            setDraft((current) => ({ ...current, name: value }));
          }}
        />
      ),
    },
    {
      id: steps[1].id,
      label: copyEntry(steps[1].labelKey).message,
      title: copyEntry(steps[1].labelKey).message,
      description: copyEntry(steps[1].helpKey).message,
      canContinue: () => steps[1].complete,
      render: () => (
        <DraftField
          labelKey={steps[1].labelKey}
          helpKey={steps[1].helpKey}
          value={draft.purpose}
          onChange={(value) => {
            setDraft((current) => ({ ...current, purpose: value }));
          }}
        />
      ),
    },
    {
      id: steps[2].id,
      label: copyEntry(steps[2].labelKey).message,
      title: copyEntry(steps[2].labelKey).message,
      description: copyEntry(steps[2].helpKey).message,
      canContinue: () => steps[2].complete && !submitting,
      render: () => (
        <DraftField
          labelKey={steps[2].labelKey}
          helpKey={steps[2].helpKey}
          value={draft.limitInput}
          numeric
          onChange={(value) => {
            setDraft((current) => ({ ...current, limitInput: value }));
          }}
        />
      ),
    },
  ];

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry("agent.create.title").message}
      description={copyEntry("agent.create.summary").message}
    >
      {errorSentence === undefined ? null : (
        <InlineNotice tone="danger" role="alert">
          {errorSentence}
        </InlineNotice>
      )}
      <Wizard
        steps={wizardSteps}
        summary={[
          { label: copyEntry(steps[0].labelKey).message, value: draft.name },
          { label: copyEntry(steps[1].labelKey).message, value: draft.purpose },
          {
            label: copyEntry(steps[2].labelKey).message,
            value: (
              <PrivateFigure>
                {limitMoney === undefined ? draft.limitInput : formatMoney(limitMoney, AGENT_LOCALE)}
              </PrivateFigure>
            ),
          },
        ]}
        completeLabel={copyEntry("agent.create.submit").message}
        onComplete={() => {
          if (creationReady(draft) && !submitting) {
            void submit();
          }
        }}
        onCancel={() => {
          router.push("/app/agents");
        }}
      />
    </ScreenCard>
  );
}
