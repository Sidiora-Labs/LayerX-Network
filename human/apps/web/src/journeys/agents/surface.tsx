"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/catalog.ts";
import { useActiveAccountId } from "../../auth/use-active-account.ts";
import {
  Badge,
  InlineNotice,
  KitButton,
  KitList,
  KitListItem,
  ScreenCard,
  StateEmpty,
} from "../../kit";
import {
  ErrorSurface,
  LoadingSurface,
  OfflineSurface,
  errorPresentation,
} from "../../states";
import { PrivateFigure } from "../../settings";
import { AgentDetailScreen } from "./detail.tsx";
import {
  AGENT_LOCALE,
  Agents,
  agentListItems,
  agentsLayout,
  type AgentListItemView,
  type AgentsShell,
} from "./model.ts";
import { useAgentsShell } from "./shell.ts";

function AgentList({
  items,
  onSelect,
  selected,
}: Readonly<{
  items: readonly AgentListItemView[];
  onSelect: (agentId: string) => void;
  selected?: string;
}>) {
  return (
    <KitList>
      {items.map((item) => (
        <KitListItem
          key={item.agentId}
          title={item.name}
          subtitle={<PrivateFigure>{item.spendSummary}</PrivateFigure>}
          trailing={<Badge variant={item.tone}>{item.stateLabel}</Badge>}
          trailingCaption={item.verificationSentence}
          navigates
          aria-current={item.agentId === selected ? "true" : undefined}
          onClick={() => {
            onSelect(item.agentId);
          }}
        />
      ))}
    </KitList>
  );
}

export function AgentsSurface({
  shell: initialShell,
  ownerAccount,
}: Readonly<{ shell: AgentsShell; ownerAccount?: string }>) {
  const router = useRouter();
  const shell = useAgentsShell(initialShell);
  const layout = agentsLayout(shell);
  const agents = useMemo(() => new Agents(), []);
  const accountId = useActiveAccountId(ownerAccount);
  const [items, setItems] = useState<readonly AgentListItemView[] | undefined>(undefined);
  const [selected, setSelected] = useState<string | undefined>(undefined);
  const [loadError, setLoadError] = useState<unknown>(undefined);
  const [offline, setOffline] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(undefined);
    setOffline(false);
    try {
      const nextItems = agentListItems(await agents.overview(), AGENT_LOCALE);
      setItems(nextItems);
      setSelected((current) => (
        current !== undefined && nextItems.some((item) => item.agentId === current)
          ? current
          : nextItems[0]?.agentId
      ));
    } catch (error) {
      if (!navigator.onLine) {
        setOffline(true);
      } else {
        setLoadError(error);
      }
    } finally {
      setLoading(false);
    }
  }, [agents]);

  useEffect(() => {
    void load();
  }, [load]);

  if (loading && items === undefined) {
    return <LoadingSurface />;
  }
  if (items === undefined && offline) {
    return <OfflineSurface onRetry={() => { void load(); }} />;
  }
  if (items === undefined) {
    return (
      <ErrorSurface
        error={errorPresentation(loadError)}
        route="/app/agents"
        platform={shell}
        onRetry={() => { void load(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  const newAgent = (
    <KitButton
      variant="primary"
      onClick={() => {
        router.push("/app/agents/new");
      }}
    >
      {copyEntry("action.new_agent").message}
    </KitButton>
  );

  let body;
  if (items.length === 0) {
    body = (
      <StateEmpty
        title={copyEntry("agents.empty").message}
        description={copyEntry("agents.empty.body").message}
        action={newAgent}
      />
    );
  } else if (layout === "stacked") {
    body = (
      <AgentList
        items={items}
        onSelect={(agentId) => {
          router.push(`/app/agents/${agentId}`);
        }}
      />
    );
  } else {
    body = (
      <div className="grid grid-cols-[minmax(280px,360px)_1fr] items-start gap-6">
        <AgentList
          items={items}
          {...(selected === undefined ? {} : { selected })}
          onSelect={setSelected}
        />
        {selected === undefined ? (
          <StateEmpty title={copyEntry("agent.list.select").message} />
        ) : (
          <AgentDetailScreen
            shell="desktop"
            agentId={selected}
            embedded
            onChanged={() => { void load(); }}
            {...(accountId === undefined ? {} : { ownerAccount: accountId })}
          />
        )}
      </div>
    );
  }

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry("navigation.agents").message}
      description={copyEntry("agents.summary").message}
    >
      <div className="flex">{newAgent}</div>
      {items.some((item) => item.verificationSentence === copyEntry("agent.state.unverified").message) ? (
        <InlineNotice tone="warning" role="status">
          {copyEntry("agent.state.unverified").message}
        </InlineNotice>
      ) : null}
      {body}
    </ScreenCard>
  );
}
