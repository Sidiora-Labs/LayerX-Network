"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { copyEntry } from "../../../copy/runtime";
import { formatCopy } from "../../../copy/format";
import { humanApi, type ActivityFilter, type ActivityPage, type ExportArtefact } from "../../api";
import { DesktopActivityFeed, MobileActivityFeed, type ActivityFeedGroup } from "../../kit/activity";
import { DesktopDetail, DesktopFilters, MobileDetail, MobileFilters } from "../../kit/patterns";
import { InlineNotice, ScreenCard, StateEmpty, StateFrame } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import { SignedWordedAmount, StatusPill } from "../../kit/money";
import { protocolAmount } from "../../kit/model";
import { useShellSelection } from "../../shell/app-shell";
import { PrivateFigure, usePrivacyMode } from "../../settings/privacy";
import { LoadingSurface, OfflineSurface } from "../../states/surfaces";
import {
  activityFailure,
  agentFilterOptions,
  emptyFilterValues,
  feedFilterDefs,
  feedGroups,
  filterEchoLines,
  formatEntryDate,
  loadActivity,
  mergePages,
  newExportKey,
  safeExportArtefact,
  sameFilterValues,
  toKitDirection,
  toWireFilter,
  type ActivityFailure,
  type FeedFilterValues,
} from "./model";

const AMOUNT_LOCALE = "en";

type ActivityLoad =
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "loaded"; page: ActivityPage; agents: ReturnType<typeof agentFilterOptions> }>
  | Readonly<{ kind: "offline"; failure: ActivityFailure }>
  | Readonly<{ kind: "error"; failure: ActivityFailure }>;

type ExportState =
  | Readonly<{ kind: "idle" }>
  | Readonly<{ kind: "preparing"; exportKind: "evidence-bundle" | "statement" }>
  | Readonly<{ kind: "ready"; artefact: ExportArtefact }>
  | Readonly<{ kind: "failed"; failure: ActivityFailure }>;

export function ActivityErrorSurface({ failure, onRetry }: Readonly<{ failure: ActivityFailure; onRetry: () => void }>) {
  const shell = useShellSelection().shell;
  const Detail = shell === "mobile" ? MobileDetail : DesktopDetail;
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const reportPath = `/app/support?code=${encodeURIComponent(failure.code)}${
    failure.trace === undefined ? "" : `&trace=${encodeURIComponent(failure.trace)}`
  }`;
  return (
    <StateFrame
      tone="danger"
      role="alert"
      title={copyEntry("state.error").message}
      description={failure.message}
    >
      <div className="flex flex-wrap gap-3">
        {failure.retriable ? (
          <KitButton type="button" onClick={onRetry}>{copyEntry("action.retry").message}</KitButton>
        ) : null}
        <KitButton type="button" variant="secondary" onClick={() => { window.location.reload(); }}>
          {copyEntry("action.reload").message}
        </KitButton>
        <KitButton asChild variant="secondary"><a href={reportPath}>{copyEntry("action.report").message}</a></KitButton>
      </div>
      <Detail
        open={technicalOpen}
        onOpenChange={setTechnicalOpen}
        title={copyEntry("activity.error.technical").message}
        summary={copyEntry("activity.error.technical").message}
        mobileVariant="sheet"
        desktopVariant="inline"
      >
        <dl className="grid gap-2 text-sm text-foreground-secondary">
          <div><dt className="font-semibold">{copyEntry("activity.error.code").message}</dt><dd>{failure.code}</dd></div>
          {failure.trace === undefined ? null : (
            <div><dt className="font-semibold">{copyEntry("activity.error.trace").message}</dt><dd>{failure.trace}</dd></div>
          )}
        </dl>
      </Detail>
    </StateFrame>
  );
}

function groupViews(page: ActivityPage): readonly ActivityFeedGroup[] {
  return feedGroups(page).map((group) => ({
    id: group.id,
    label: group.label,
    subtotal: (
      <span className="inline-flex flex-wrap justify-end gap-2">
        <span>{copyEntry("activity.feed.subtotal_in").message} <PrivateFigure><SignedWordedAmount
          value={group.subtotalIn}
          currency={group.currency}
          locale={AMOUNT_LOCALE}
          decimals={0}
          direction="inbound"
        /></PrivateFigure></span>
        <span>{copyEntry("activity.feed.subtotal_out").message} <PrivateFigure><SignedWordedAmount
          value={protocolAmount(group.subtotalOut * -1)}
          currency={group.currency}
          locale={AMOUNT_LOCALE}
          decimals={0}
          direction="outbound"
        /></PrivateFigure></span>
      </span>
    ),
    rows: group.items.map((row) => ({
      id: row.id,
      title: row.title,
      subtitle: row.subtitle,
      occurredAt: row.date,
      sortAmount: row.amount,
      amount: row.currency === undefined ? <span aria-label={copyEntry("activity.detail.no_amount").message}>—</span> : (
        <PrivateFigure>
          <SignedWordedAmount
            value={row.amount}
            currency={row.currency}
            locale={AMOUNT_LOCALE}
            decimals={0}
            direction={toKitDirection(row.amount < 0 ? "out" : "in")}
          />
        </PrivateFigure>
      ),
      status: <StatusPill status={row.statusKey} />,
    })),
  }));
}

export function Activity() {
  const router = useRouter();
  const shell = useShellSelection().shell;
  const client = useMemo(() => humanApi(), []);
  const { masked } = usePrivacyMode();
  const [load, setLoad] = useState<ActivityLoad>({ kind: "loading" });
  const [draft, setDraft] = useState<FeedFilterValues>(() => emptyFilterValues());
  const [applied, setApplied] = useState<ActivityFilter>({});
  const [paging, setPaging] = useState(false);
  const [pagingFailure, setPagingFailure] = useState<ActivityFailure | undefined>();
  const [exportState, setExportState] = useState<ExportState>({ kind: "idle" });
  const exportKeys = useRef(new Map<"evidence-bundle" | "statement", string>());

  const refresh = useCallback((filter: ActivityFilter) => {
    setLoad({ kind: "loading" });
    setPagingFailure(undefined);
    loadActivity(client, filter)
      .then((result) => {
        feedGroups(result.page);
        setLoad({ kind: "loaded", page: result.page, agents: agentFilterOptions(result.agents) });
      })
      .catch((error: unknown) => {
        const failure = activityFailure(error);
        setLoad({ kind: failure.kind === "offline" ? "offline" : "error", failure });
      });
  }, [client]);

  useEffect(() => {
    refresh({});
  }, [refresh]);

  const applyFilters = (values: FeedFilterValues) => {
    const filter = toWireFilter(values);
    setDraft(values);
    setApplied(filter);
    refresh(filter);
  };

  const loadMore = () => {
    if (load.kind !== "loaded" || load.page.next_cursor.length === 0 || paging) {
      return;
    }
    setPaging(true);
    setPagingFailure(undefined);
    client.activityQuery({ cursor: load.page.next_cursor, filter: load.page.filter, page_limit: 50 })
      .then((page) => {
        const merged = mergePages(load.page, page);
        feedGroups(merged);
        setLoad({ ...load, page: merged });
      })
      .catch((error: unknown) => { setPagingFailure(activityFailure(error)); })
      .finally(() => { setPaging(false); });
  };

  const requestExport = (exportKind: "evidence-bundle" | "statement") => {
    if (load.kind !== "loaded" || exportState.kind === "preparing") {
      return;
    }
    const key = exportKeys.current.get(exportKind) ?? newExportKey();
    exportKeys.current.set(exportKind, key);
    setExportState({ kind: "preparing", exportKind });
    const request = exportKind === "statement"
      ? client.activityExportStatement({ filter: load.page.filter }, key)
      : client.activityExportEvidence({ filter: load.page.filter }, key);
    request.then((value) => {
      const artefact = safeExportArtefact(value);
      if (artefact.kind !== exportKind) {
        throw new Error("The export kind did not match the request");
      }
      exportKeys.current.delete(exportKind);
      setExportState({ kind: "ready", artefact });
    }).catch((error: unknown) => { setExportState({ kind: "failed", failure: activityFailure(error) }); });
  };

  if (load.kind === "loading") {
    return <LoadingSurface rows={6} />;
  }
  if (load.kind === "offline") {
    return <OfflineSurface onRetry={() => { refresh(applied); }} />;
  }
  if (load.kind === "error") {
    return <ActivityErrorSurface failure={load.failure} onRetry={() => { refresh(applied); }} />;
  }

  const names = new Map(load.agents.map((agent) => [agent.value, agent.label]));
  const echo = filterEchoLines(load.page.filter, names);
  const filters = feedFilterDefs(load.agents);
  const Feed = shell === "mobile" ? MobileActivityFeed : DesktopActivityFeed;
  const Filter = shell === "mobile" ? MobileFilters : DesktopFilters;
  const groups = groupViews(load.page);

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry("navigation.activity").message}
      description={copyEntry("activity.summary").message}
    >
      <div className="mt-4 flex flex-col gap-5">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <Filter
            filters={filters}
            values={draft}
            onChange={(next) => {
              const values = next as FeedFilterValues;
              if (shell === "mobile") {
                applyFilters(values);
              } else {
                setDraft(values);
              }
            }}
          />
          {shell === "desktop" ? (
            <KitButton
              type="button"
              {...(sameFilterValues(draft, (() => {
                const current: Record<string, string | { from: Date; to?: Date } | undefined> = emptyFilterValues();
                const kind = load.page.filter.kinds?.[0];
                if (kind !== undefined) current["kind"] = kind;
                if (load.page.filter.agent_id !== undefined) current["agent"] = load.page.filter.agent_id;
                if (load.page.filter.from !== undefined) {
                  current["date"] = {
                    from: new Date(load.page.filter.from),
                    ...(load.page.filter.to === undefined ? {} : { to: new Date(new Date(load.page.filter.to).getTime() - 86_400_000) }),
                  };
                }
                return current;
              })()) ? {
                disabled: true,
                disabledReason: copyEntry("activity.filter.no_changes").message,
              } : {})}
              onClick={() => { applyFilters(draft); }}
            >
              {copyEntry("activity.filter.apply").message}
            </KitButton>
          ) : null}
        </div>
        <div role="status" className="flex flex-col gap-1 text-sm text-foreground-secondary">
          {echo.map((line) => <p key={line}>{line}</p>)}
        </div>
        <div className="flex flex-wrap gap-3">
          <KitButton
            type="button"
            variant="secondary"
            {...(exportState.kind === "preparing" ? {
              disabled: true,
              disabledReason: copyEntry("activity.export.preparing").message,
            } : {})}
            onClick={() => { requestExport("statement"); }}
          >
            {copyEntry("activity.export.statement").message}
          </KitButton>
          <KitButton
            type="button"
            variant="secondary"
            {...(exportState.kind === "preparing" ? {
              disabled: true,
              disabledReason: copyEntry("activity.export.preparing").message,
            } : {})}
            onClick={() => { requestExport("evidence-bundle"); }}
          >
            {copyEntry("activity.export.evidence").message}
          </KitButton>
        </div>
        {exportState.kind === "ready" ? (
          <InlineNotice tone="success">
            <span className="flex flex-wrap items-center justify-between gap-3">
              <span>{copyEntry("activity.export.ready").message} {formatCopy("activity.export.evidence_count", { count: exportState.artefact.evidence.length })}</span>
              <KitButton asChild size="sm"><a href={exportState.artefact.download_path} download>{copyEntry("activity.export.download").message}</a></KitButton>
            </span>
          </InlineNotice>
        ) : exportState.kind === "failed" ? (
          <InlineNotice tone="danger" role="alert">
            <span>{copyEntry("activity.export.failed").message}</span>{" "}
            <span>{formatCopy("error.technical.machine_code", { machineCode: exportState.failure.code })}</span>
          </InlineNotice>
        ) : null}
        {groups.length === 0 ? (
          <StateEmpty title={copyEntry("activity.feed.empty").message} description={echo.join(" ")} />
        ) : (
          <Feed
            groups={groups}
            onSelect={(entryId) => { router.push(`/app/activity/${encodeURIComponent(entryId)}`); }}
            dateLabel={(date) => formatEntryDate(date.toISOString())}
            columns={{
              activity: copyEntry("activity.column.activity").message,
              date: copyEntry("activity.column.date").message,
              status: copyEntry("activity.column.status").message,
              amount: copyEntry("activity.column.amount").message,
            }}
            amountSortEnabled={!masked}
          />
        )}
        {pagingFailure === undefined ? null : (
          <InlineNotice tone={pagingFailure.kind === "offline" ? "warning" : "danger"} role="alert">
            {pagingFailure.message}
          </InlineNotice>
        )}
        {load.page.next_cursor.length === 0 ? null : (
          <KitButton
            type="button"
            variant="secondary"
            {...(paging ? {
              disabled: true,
              disabledReason: copyEntry("state.loading.body").message,
            } : {})}
            onClick={loadMore}
          >
            {copyEntry("activity.load_more").message}
          </KitButton>
        )}
      </div>
    </ScreenCard>
  );
}

export function human_web_journeys_Activity() {
  return Activity;
}
