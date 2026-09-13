"use client";

import { useCallback, useEffect, useMemo, useState } from "react";

import { copyEntry } from "../../../copy/runtime";
import { formatCopy } from "../../../copy/format";
import { humanApi, type Session } from "../../api";
import { DeviceSessionList } from "../../kit/device-list";
import { ScreenCard, StateEmpty } from "../../kit/surface";
import { ErrorSurface, errorPresentation, LoadingSurface, OfflineSurface } from "../../states";

type DeviceListState =
  | Readonly<{ name: "loading" }>
  | Readonly<{ name: "ready"; sessions: readonly Session[] }>
  | Readonly<{ name: "error"; error: unknown }>;

const timeFormat = new Intl.DateTimeFormat("en", {
  dateStyle: "medium",
  timeStyle: "short",
});

function time(value: string): string {
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? value : timeFormat.format(parsed);
}

export function DeviceList() {
  const api = useMemo(() => humanApi(), []);
  const [state, setState] = useState<DeviceListState>({ name: "loading" });

  const load = useCallback(async () => {
    setState({ name: "loading" });
    try {
      const result = await api.sessionList();
      setState({ name: "ready", sessions: result.sessions });
    } catch (error) {
      setState({ name: "error", error });
    }
  }, [api]);

  useEffect(() => {
    void load();
  }, [load]);

  if (state.name === "loading") {
    return <LoadingSurface rows={2} />;
  }
  if (state.name === "error") {
    if (!navigator.onLine) {
      return <OfflineSurface onRetry={() => { void load(); }} />;
    }
    return (
      <ErrorSurface
        error={errorPresentation(state.error)}
        route="/app/settings/devices"
        onRetry={() => { void load(); }}
        onReload={() => { window.location.reload(); }}
      />
    );
  }

  return (
    <ScreenCard
      landmark="section"
      title={copyEntry("device.list.title").message}
      description={copyEntry("device.list.body").message}
    >
      {state.sessions.length === 0 ? (
        <StateEmpty
          title={copyEntry("state.empty").message}
          description={copyEntry("state.empty.body").message}
        />
      ) : (
        <DeviceSessionList
          items={state.sessions.map((session) => ({
            id: session.session_id,
            title: session.device.label,
            subtitle: (
              <>
                {session.device.platform} ·{" "}
                <time dateTime={session.opened_at}>
                  {formatCopy("device.opened", { when: time(session.opened_at) })}
                </time>
              </>
            ),
            trailing: session.current ? copyEntry("device.current").message : undefined,
            trailingCaption: (
              <time dateTime={session.last_active_at}>
                {formatCopy("device.last_active", { when: time(session.last_active_at) })}
              </time>
            ),
            current: session.current,
          }))}
        />
      )}
    </ScreenCard>
  );
}
