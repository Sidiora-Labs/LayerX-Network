"use client";

import { useEffect, useState } from "react";

import { copyEntry } from "../../../copy/runtime.ts";
import { formatCopy } from "../../../copy/format.ts";
import type { EvidenceRef, Journey } from "../../api/index.ts";
import { CopyableIdentifier, StatusPill } from "../../kit/money";
import { DesktopDetail, MobileDetail } from "../../kit/patterns";
import { InlineNotice, StateFrame } from "../../kit/surface";
import { KitButton } from "../../kit/control";
import type { CustodyShell, RefusalPresentation, TimelineRow, WalletPanelPlan } from "./model.ts";

export function useCustodyShell(): CustodyShell {
  const [shell, setShell] = useState<CustodyShell>("desktop");
  useEffect(() => {
    const update = () => {
      const current = document.querySelector<HTMLElement>("[data-shell]")?.dataset["shell"];
      setShell(current === "mobile" ? "mobile" : "desktop");
    };
    update();
    const media = window.matchMedia("(max-width: 767px)");
    media.addEventListener("change", update);
    return () => {
      media.removeEventListener("change", update);
    };
  }, []);
  return shell;
}

export function JourneyTimelineView({ rows }: Readonly<{ rows: readonly TimelineRow[] }>) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="text-base font-semibold text-foreground">
        {copyEntry("journey.timeline.title").message}
      </h2>
      <ol className="flex flex-col divide-y divide-border">
        {rows.map((row) => (
          <li key={row.stageId} className="flex items-center justify-between gap-4 py-3">
            <span className="font-medium text-foreground">{copyEntry(row.nameKey).message}</span>
            <StatusPill status={row.status} />
          </li>
        ))}
      </ol>
    </section>
  );
}

function journeyEvidence(journey: Journey): readonly EvidenceRef[] {
  const references = [...journey.evidence, ...journey.stages.flatMap((stage) => stage.evidence)];
  return [...new Map(references.map((reference) => [reference.evidence_id, reference])).values()];
}

export function JourneyTechnicalDetails({ journey }: Readonly<{ journey: Journey }>) {
  const shell = useCustodyShell();
  const [open, setOpen] = useState(false);
  const Detail = shell === "mobile" ? MobileDetail : DesktopDetail;
  const evidence = journeyEvidence(journey);
  return (
    <Detail
      open={open}
      onOpenChange={setOpen}
      title={copyEntry("error.technical.title").message}
      summary={copyEntry("error.technical.title").message}
      mobileVariant="sheet"
      desktopVariant="inline"
    >
      <div className="flex flex-col gap-3">
        <CopyableIdentifier
          label={copyEntry("journey.technical.journey_id").message}
          value={journey.journey_id}
        />
        {evidence.map((reference) => (
          <CopyableIdentifier
            key={reference.evidence_id}
            label={formatCopy("journey.technical.evidence", {
              evidenceClass: reference.class,
              verification: reference.verification,
            })}
            value={reference.evidence_id}
          />
        ))}
      </div>
    </Detail>
  );
}

const WALLET_PANEL_TONES = Object.freeze({
  idle: "neutral",
  waiting: "neutral",
  approved: "neutral",
  cancelled: "warning",
  rejected: "warning",
  unavailable: "warning",
  failed: "danger",
} as const);

export function WalletPanelView({
  panel,
  onOpen,
  busy,
}: Readonly<{ panel: WalletPanelPlan; onOpen: () => void; busy: boolean }>) {
  return (
    <StateFrame
      title={copyEntry(panel.titleKey).message}
      description={copyEntry(panel.bodyKey).message}
      tone={WALLET_PANEL_TONES[panel.phase]}
      busy={panel.phase === "waiting"}
      role="status"
    >
      {panel.phase === "idle" ? null : (
        <p className="text-sm">{copyEntry(panel.signKey).message}</p>
      )}
      {panel.actionKey === undefined ? null : busy ? (
        <KitButton variant="primary" disabled disabledReason={copyEntry("wallet.handoff.in_progress.body").message}>
          {copyEntry(panel.actionKey).message}
        </KitButton>
      ) : (
        <KitButton variant="primary" onClick={onOpen}>
          {copyEntry(panel.actionKey).message}
        </KitButton>
      )}
    </StateFrame>
  );
}

export function RefusalView({
  refusal,
  onNavigate,
}: Readonly<{ refusal: RefusalPresentation; onNavigate: (path: string) => void }>) {
  const changePath = refusal.changePath;
  return (
    <StateFrame
      title={copyEntry("status.refused").message}
      description={copyEntry(refusal.bodyKey).message}
      tone="danger"
      role="alert"
    >
      <p className="text-sm font-semibold">{copyEntry(refusal.moneyKey).message}</p>
      {changePath === undefined || refusal.changeActionKey === undefined ? null : (
        <KitButton
          variant="secondary"
          onClick={() => {
            onNavigate(changePath);
          }}
        >
          {copyEntry(refusal.changeActionKey).message}
        </KitButton>
      )}
    </StateFrame>
  );
}

export function DelayNotice({ duration }: Readonly<{ duration: string }>) {
  return (
    <InlineNotice tone="warning" role="status">
      <span className="font-semibold">{copyEntry("journey.delayed").message}</span>{" "}
      {formatCopy("journey.delayed.expectation", { duration })}
    </InlineNotice>
  );
}

export function SafeToCloseNotice({ messageKey }: Readonly<{ messageKey: string }>) {
  return (
    <InlineNotice tone="neutral" role="status">
      {copyEntry(messageKey).message}
    </InlineNotice>
  );
}

export function CompleteView({
  titleKey,
  bodyKey,
}: Readonly<{ titleKey: string; bodyKey: string }>) {
  return (
    <StateFrame
      title={copyEntry(titleKey).message}
      description={copyEntry(bodyKey).message}
      tone="success"
      role="status"
    />
  );
}
