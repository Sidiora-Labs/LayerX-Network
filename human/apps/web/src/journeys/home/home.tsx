"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/catalog.ts";
import { humanApi } from "../../api/index.ts";
import {
  ActionGrid,
  BalanceSummary,
  CountBadge,
  KitButton,
  KitList,
  KitListItem,
  KitSectionHeader,
  KitViewAllChip,
  LabelValue,
  ScreenCard,
  SignedWordedAmount,
  StateFrame,
  StatusPill,
  protocolAmount,
} from "../../kit";
import { useShellSelection } from "../../shell/app-shell";
import { PrivateFigure, usePrivacyMode } from "../../settings/privacy";
import { LoadingSurface, OfflineSurface } from "../../states/surfaces";
import { AMOUNT_LOCALE } from "../move/model.ts";
import {
  HOME_DESTINATIONS,
  approvalBadge,
  agentsSummary,
  classifyHomeFailure,
  homeActions,
  homeActivityRows,
  homeAgentRows,
  homeBalance,
  loadHome,
  type HomeData,
} from "./model.ts";

type HomeLoad =
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "loaded"; data: HomeData }>
  | Readonly<{ kind: "offline" }>
  | Readonly<{ kind: "error" }>;

export function Home() {
  const router = useRouter();
  const shell = useShellSelection().shell;
  const client = useMemo(() => humanApi(), []);
  const [load, setLoad] = useState<HomeLoad>({ kind: "loading" });
  const { masked, setMasked } = usePrivacyMode();

  const refresh = useCallback(() => {
    setLoad({ kind: "loading" });
    loadHome(client)
      .then((data) => {
        setLoad({ kind: "loaded", data });
      })
      .catch((error: unknown) => {
        setLoad({ kind: classifyHomeFailure(error) });
      });
  }, [client]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (load.kind === "loading") {
    return <LoadingSurface rows={4} />;
  }
  if (load.kind === "offline") {
    return <OfflineSurface onRetry={refresh} />;
  }
  if (load.kind === "error") {
    return (
      <StateFrame
        tone="danger"
        role="alert"
        title={copyEntry("state.error").message}
        description={copyEntry("state.error.body").message}
      >
        <div className="flex flex-wrap gap-3">
          <KitButton variant="primary" onClick={refresh}>{copyEntry("action.retry").message}</KitButton>
        </div>
      </StateFrame>
    );
  }

  const balance = homeBalance(load.data.balance);
  const approvals = approvalBadge(load.data.approvals);
  const agents = homeAgentRows(load.data.agents);
  const activity = homeActivityRows(load.data.entries);
  const dense = shell === "desktop";

  return (
    <ScreenCard landmark="section" title={copyEntry("navigation.home").message}>
      <div className={dense ? "grid grid-cols-2 gap-8" : "flex flex-col gap-6"}>
        <section className="flex flex-col gap-4">
          {balance.kind === "verified" ? (
            <div className="flex flex-col gap-1">
              <p className="text-sm text-muted-foreground">{balance.label}</p>
              <PrivateFigure className="contents">
                <BalanceSummary
                  label=""
                  value={balance.amount}
                  currency={balance.currency}
                  hidden={false}
                  onHiddenChange={setMasked}
                />
              </PrivateFigure>
              {masked ? (
                <KitButton variant="secondary" size="sm" onClick={() => { setMasked(false); }}>
                  {copyEntry("home.balance.show").message}
                </KitButton>
              ) : null}
              <p
                className={balance.current ? "text-sm text-muted-foreground" : "text-sm text-warning"}
                role={balance.current ? undefined : "status"}
              >
                {`${balance.verification} · ${balance.freshness}`}
              </p>
            </div>
          ) : (
            <div className="flex flex-col gap-2">
              <LabelValue label={balance.label} value={balance.message} />
              <div className="flex flex-wrap gap-3">
                <KitButton
                  variant="secondary"
                  size="sm"
                  aria-pressed={masked}
                  onClick={() => {
                    setMasked(!masked);
                  }}
                >
                  {copyEntry(masked ? "home.balance.show" : "home.balance.hide").message}
                </KitButton>
              </div>
              {masked ? (
                <p className="text-sm text-muted-foreground">{copyEntry("home.balance.hidden").message}</p>
              ) : null}
            </div>
          )}
          <ActionGrid
            actions={homeActions()}
            onAction={(id) => {
              const action = homeActions().find((candidate) => candidate.id === id);
              if (action !== undefined) {
                router.push(action.route);
              }
            }}
          />
          <div className="flex items-center gap-3">
            <CountBadge
              variant={approvals.count > 0 ? "warning" : "neutral"}
              label={approvals.label}
            />
            {approvals.count > 0 ? (
              <KitViewAllChip
                onClick={() => {
                  router.push(HOME_DESTINATIONS.approvals);
                }}
              >
                {copyEntry("home.approvals.review").message}
              </KitViewAllChip>
            ) : null}
          </div>
        </section>
        <section className="flex flex-col gap-4">
          <div className="flex flex-col gap-2">
            <KitSectionHeader
              title={copyEntry("home.agents.title").message}
              action={
                <KitViewAllChip
                  onClick={() => {
                    router.push(HOME_DESTINATIONS.agents);
                  }}
                />
              }
            />
            <p className="text-sm text-muted-foreground">{agentsSummary(load.data.agents)}</p>
            {agents.length === 0 ? null : (
              <KitList>
                {agents.map((agent) => (
                  <KitListItem
                    key={agent.id}
                    title={agent.name}
                    subtitle={agent.purpose}
                    trailing={
                      <PrivateFigure className="text-sm font-semibold tabular-nums text-foreground">
                        {agent.spend}
                      </PrivateFigure>
                    }
                    trailingCaption={agent.spendVerification}
                  />
                ))}
              </KitList>
            )}
          </div>
          <div className="flex flex-col gap-2">
            <KitSectionHeader
              title={copyEntry("home.activity.title").message}
              action={
                <KitViewAllChip
                  onClick={() => {
                    router.push(HOME_DESTINATIONS.activity);
                  }}
                />
              }
            />
            {activity.length === 0 ? (
              <p className="text-sm text-muted-foreground">{copyEntry("home.activity.empty").message}</p>
            ) : (
              <KitList>
                {activity.map((row) => (
                  <KitListItem
                    key={row.id}
                    title={row.title}
                    subtitle={row.when}
                    trailing={
                      <span className="flex items-center gap-2">
                        {row.amount === undefined || row.currency === undefined ? null : (
                          <PrivateFigure className="contents">
                            <SignedWordedAmount
                              value={protocolAmount(row.direction === "outbound" ? row.amount * -1 : row.amount)}
                              currency={row.currency}
                              locale={AMOUNT_LOCALE}
                              decimals={0}
                              {...(row.direction === undefined ? {} : { direction: row.direction })}
                            />
                          </PrivateFigure>
                        )}
                        <StatusPill status={row.status} />
                      </span>
                    }
                  />
                ))}
              </KitList>
            )}
          </div>
        </section>
      </div>
    </ScreenCard>
  );
}
