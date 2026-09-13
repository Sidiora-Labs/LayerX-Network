"use client";

import { useEffect, type ReactNode } from "react";

import { copyEntry } from "../../copy/runtime.ts";
import { formatCopy } from "../../copy/format.ts";
import { KitButton } from "../kit/control";
import { StateEmpty, StateFrame, StateSkeleton } from "../kit/surface";

export function LoadingSurface({ rows = 3 }: Readonly<{ rows?: number }>) {
  return (
    <StateFrame
      title={copyEntry("state.loading").message}
      description={copyEntry("state.loading.body").message}
      busy
      role="status"
    >
      <StateSkeleton rows={rows} />
    </StateFrame>
  );
}

export function EmptySurface({
  action,
  actionLabelKey,
}: Readonly<{ action?: () => void; actionLabelKey?: string }>) {
  return (
    <StateEmpty
      title={copyEntry("state.empty").message}
      description={copyEntry("state.empty.body").message}
      action={action === undefined || actionLabelKey === undefined ? undefined : (
        <KitButton variant="primary" onClick={action}>{copyEntry(actionLabelKey).message}</KitButton>
      )}
    />
  );
}

export function OfflineSurface({ onRetry }: Readonly<{ onRetry: () => void }>) {
  return (
    <StateFrame
      title={copyEntry("state.offline").message}
      description={copyEntry("state.offline.body").message}
      tone="warning"
      role="status"
    >
      <KitButton variant="primary" onClick={onRetry}>{copyEntry("action.retry").message}</KitButton>
    </StateFrame>
  );
}

export function DegradedSurface({
  age,
  referencePoint,
  children,
  onReload,
}: Readonly<{
  age: string;
  referencePoint: string;
  children?: ReactNode;
  onReload?: () => void;
}>) {
  return (
    <StateFrame
      title={copyEntry("state.degraded").message}
      description={formatCopy("state.degraded.age", { age })}
      tone="warning"
      role="status"
    >
      <p className="text-sm">{formatCopy("state.degraded.reference", { referencePoint })}</p>
      {children}
      {onReload === undefined ? null : (
        <KitButton variant="secondary" onClick={onReload}>{copyEntry("action.reload").message}</KitButton>
      )}
    </StateFrame>
  );
}

export interface StillCheckingSurfaceProps {
  readonly lookupOutcome: () => Promise<"pending" | "resolved">;
  readonly onResolved: () => void;
  readonly pollIntervalMs?: number;
  readonly children?: ReactNode;
}

export function StillCheckingSurface({
  lookupOutcome,
  onResolved,
  pollIntervalMs = 3_000,
  children,
}: StillCheckingSurfaceProps) {
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = async () => {
      try {
        const outcome = await lookupOutcome();
        if (cancelled) {
          return;
        }
        if (outcome === "resolved") {
          onResolved();
          return;
        }
      } catch {
        if (cancelled) {
          return;
        }
      }
      timer = setTimeout(() => { void check(); }, pollIntervalMs);
    };
    void check();
    return () => {
      cancelled = true;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }, [lookupOutcome, onResolved, pollIntervalMs]);

  return (
    <StateFrame
      title={copyEntry("status.still_checking").message}
      description={copyEntry("state.still_checking.body").message}
      tone="warning"
      busy
      role="status"
    >
      {children}
      <p className="text-sm font-semibold">
        {copyEntry("state.still_checking.locked").message}
      </p>
    </StateFrame>
  );
}
