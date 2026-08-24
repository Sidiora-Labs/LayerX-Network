import type { ReactNode } from "react";

import { copyEntry } from "../../copy/catalog";
import { formatCopy } from "../../copy/format";
import {
  ExplorerFreshness as ExplorerFreshnessView,
  ExplorerNavigation,
  ScreenCard,
} from "../kit";
import type { ExplorerFreshness, ExplorerVerificationLevel, MirrorVerificationProvenance } from "./model";

const EXPLORER_NAVIGATION = [
  { href: "/explorer", copyKey: "explorer.navigation.overview" },
  { href: "/explorer/checkpoints", copyKey: "explorer.navigation.checkpoints" },
  { href: "/explorer/batches", copyKey: "explorer.navigation.batches" },
  { href: "/explorer/verify", copyKey: "explorer.navigation.verify" },
] as const;

export function ExplorerFrame({
  title,
  description,
  children,
}: Readonly<{ title: ReactNode; description?: ReactNode; children: ReactNode }>) {
  return (
    <ScreenCard title={title} description={description} dataApplication="explorer">
      <div className="mt-4 flex flex-col gap-4">
        <ExplorerNavigation
          label={copyEntry("explorer.navigation.label").message}
          items={EXPLORER_NAVIGATION.map((item) => ({
            href: item.href,
            label: copyEntry(item.copyKey).message,
          }))}
        />
        {children}
      </div>
    </ScreenCard>
  );
}

export function verificationLabel(level: ExplorerVerificationLevel): string {
  return copyEntry(`explorer.verification.${level.replaceAll("-", "_")}`).message;
}

export function FreshnessDisplay({ freshness }: Readonly<{ freshness?: ExplorerFreshness }>) {
  if (freshness === undefined) {
    return (
      <ExplorerFreshnessView
        title={copyEntry("explorer.freshness.unavailable").message}
        description={copyEntry("explorer.freshness.unavailable.body").message}
        current={false}
      />
    );
  }
  return (
    <ExplorerFreshnessView
      title={copyEntry(freshness.current
        ? "explorer.freshness.current"
        : "explorer.freshness.behind").message}
      description={formatCopy("explorer.freshness.detail", {
        indexedBatch: freshness.indexedBatch ?? copyEntry("explorer.value.none").message,
        observedBatch: freshness.observedSealedBatch,
        batchesBehind: freshness.batchesBehind,
        checkpoint: freshness.indexedCheckpoint ?? copyEntry("explorer.value.none").message,
      })}
      current={freshness.current}
    />
  );
}

export function MirrorFreshnessDisplay({ mirror }:Readonly<{mirror:MirrorVerificationProvenance}>){const lag=mirror.batchLag.kind==="known"?formatCopy("explorer.mirror.lag.known",{batches:mirror.batchLag.batches}):copyEntry("explorer.mirror.lag.unknown").message;return <ExplorerFreshnessView title={copyEntry(mirror.degraded?"explorer.mirror.degraded":"explorer.mirror.canonical").message} description={formatCopy("explorer.mirror.detail",{source:mirror.sourceId,target:mirror.target,position:mirror.canonicalPosition,lag,failovers:mirror.failoverCount,agreement:mirror.agreeingSources})} current={!mirror.degraded}/>;}

export function ExplorerUnavailable() {
  return (
    <ExplorerFrame
      title={copyEntry("explorer.title").message}
      description={copyEntry("explorer.summary").message}
    >
      <FreshnessDisplay />
    </ExplorerFrame>
  );
}

export function ExplorerNotFound({
  title,
  freshness,
}: Readonly<{ title: string; freshness: ExplorerFreshness }>) {
  return (
    <ExplorerFrame title={title} description={copyEntry("explorer.not_found").message}>
      <FreshnessDisplay freshness={freshness} />
      <p className="text-sm text-foreground-secondary">{copyEntry("explorer.not_found.body").message}</p>
    </ExplorerFrame>
  );
}

export function explorerReceiptPath(identifier: string): string {
  return `/explorer/receipts/${encodeURIComponent(identifier)}`;
}

export function explorerCheckpointPath(identifier: string): string {
  return `/explorer/checkpoints/${encodeURIComponent(identifier)}`;
}
