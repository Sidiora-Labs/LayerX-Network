"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/catalog.ts";
import type { Agent } from "../../api/index.ts";
import { useActiveAccountId } from "../../auth/use-active-account.ts";
import {
  Badge,
  CopyableIdentifier,
  InlineNotice,
  KitButton,
  KitSectionHeader,
  LabelValue,
  ScreenCard,
  StatPair,
} from "../../kit";
import {
  ErrorSurface,
  LoadingSurface,
  OfflineSurface,
  errorPresentation,
} from "../../states";
import { PrivateFigure } from "../../settings";
import { AgentControls } from "./controls.tsx";
import {
  AGENT_LOCALE,
  Agents,
  agentPresentation,
  creationHeadlineKey,
  formatPlainTimestamp,
  journeyProgress,
  spendPresentation,
  type AgentsShell,
  type JourneyProgress,
} from "./model.ts";
import { JourneyStages } from "./progress.tsx";
import { useAgentsShell } from "./shell.ts";

function SpendSection({ agent }: Readonly<{ agent: Agent }>) {
  const spend = spendPresentation(agent, AGENT_LOCALE);
  return (
    <div className="flex flex-col gap-3">
      <StatPair
        left={{
          value: <PrivateFigure>{spend.spent}</PrivateFigure>,
          label: copyEntry("agent.spend.spent").message,
        }}
        right={{
          value: <PrivateFigure>{spend.remaining}</PrivateFigure>,
          label: copyEntry("agent.spend.remaining").message,
        }}
      />
      <LabelValue
        label={copyEntry("agent.spend.limit").message}
        value={<PrivateFigure>{spend.limit}</PrivateFigure>}
      />
      <p className="text-sm text-foreground-secondary">
        <PrivateFigure>{spend.summary}</PrivateFigure>
      </p>
      <InlineNotice tone={spend.protocolBacked ? "neutral" : "warning"} role="status">
        {spend.enforcementSentence}
      </InlineNotice>
      <p className="text-sm text-muted-foreground">{spend.verificationSentence}</p>
      {spend.reconciliationSentence === undefined ? null : (
        <InlineNotice tone="warning" role="status">
          {spend.reconciliationSentence}
        </InlineNotice>
      )}
    </div>
  );
}

export function AgentDetailScreen({
  shell: initialShell,
  agentId,
  ownerAccount,
  embedded = false,
  onChanged,
}: Readonly<{
  shell: AgentsShell;
  agentId: string;
  ownerAccount?: string;
  embedded?: boolean;
  onChanged?: () => void;
}>) {
  const router = useRouter();
  const shell = useAgentsShell(initialShell);
  const agents = useMemo(() => new Agents(), []);
  const accountId = useActiveAccountId(ownerAccount);
  const [agent, setAgent] = useState<Agent | undefined>(undefined);
  const [creation, setCreation] = useState<JourneyProgress | undefined>(undefined);
  const [loadError, setLoadError] = useState<unknown>(undefined);
  const [offline, setOffline] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(undefined);
    setOffline(false);
    try {
      const loaded = await agents.agent(agentId);
      setAgent(loaded);
      if (loaded.state === "creating" && loaded.creation_journey_id !== undefined) {
        setCreation(journeyProgress(await agents.journey(loaded.creation_journey_id)));
      } else {
        setCreation(undefined);
      }
    } catch (error) {
      if (!navigator.onLine) {
        setOffline(true);
      } else {
        setLoadError(error);
      }
    } finally {
      setLoading(false);
    }
  }, [agentId, agents]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (creation === undefined || creation.complete || creation.refusalSentence !== undefined) {
      return;
    }
    const timer = setTimeout(() => {
      void load();
    }, 2_000);
    return () => {
      clearTimeout(timer);
    };
  }, [creation, load]);

  if (loading && agent === undefined) {
    return <LoadingSurface />;
  }
  if (agent === undefined && offline) {
    return <OfflineSurface onRetry={() => { void load(); }} />;
  }
  if (agent === undefined) {
    return (
      <ErrorSurface
        error={errorPresentation(loadError)}
        route={`/app/agents/${encodeURIComponent(agentId)}`}
        platform={shell}
        onRetry={() => { void load(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  const presentation = agentPresentation(agent);
  return (
    <ScreenCard landmark="section" title={agent.name}>
      <div className="flex items-center gap-3">
        <Badge variant={presentation.tone}>{presentation.label}</Badge>
      </div>
      {presentation.stateVerified ? null : (
        <InlineNotice tone="warning" role="status">
          {copyEntry("agent.state.unverified").message}
        </InlineNotice>
      )}
      {offline ? (
        <InlineNotice tone="warning" role="status">
          {copyEntry("state.offline.body").message}
        </InlineNotice>
      ) : null}
      {loadError === undefined ? null : (
        <InlineNotice tone="danger" role="alert">
          {copyEntry("state.error.body").message}
        </InlineNotice>
      )}
      {presentation.readOnlyKey === undefined ? null : (
        <InlineNotice tone="neutral" role="status">
          {copyEntry(presentation.readOnlyKey).message}
        </InlineNotice>
      )}
      <div className="flex flex-col gap-2">
        <LabelValue label={copyEntry("agent.detail.purpose").message} value={agent.purpose} />
        <LabelValue
          label={copyEntry("agent.detail.created").message}
          value={formatPlainTimestamp(agent.created_at, AGENT_LOCALE)}
        />
      </div>
      <CopyableIdentifier
        label={copyEntry("agent.detail.identifier").message}
        value={agent.agent_id}
      />
      {creation === undefined ? null : (
        <div className="flex flex-col gap-3">
          <KitSectionHeader title={copyEntry("agent.detail.progress").message} />
          <p className="text-sm text-foreground-secondary">
            {copyEntry(creationHeadlineKey(creation)).message}
          </p>
          <JourneyStages progress={creation} />
          {creation.complete || creation.refusalSentence !== undefined ? null : (
            <div className="flex">
              <KitButton
                variant="secondary"
                loading={loading}
                onClick={() => { void load(); }}
              >
                {copyEntry("agent.create.check").message}
              </KitButton>
            </div>
          )}
        </div>
      )}
      <SpendSection agent={agent} />
      <AgentControls
        shell={shell}
        agent={agent}
        agents={agents}
        {...(accountId === undefined ? {} : { ownerAccount: accountId })}
        onAgent={(updated) => {
          setAgent(updated);
          onChanged?.();
        }}
        onChanged={() => {
          void load();
          onChanged?.();
        }}
      />
      {embedded ? null : (
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
      )}
    </ScreenCard>
  );
}
