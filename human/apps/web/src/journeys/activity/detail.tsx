"use client";

import { useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { copyEntry } from "../../../copy/runtime";
import { formatCopy } from "../../../copy/format";
import { humanApi, type ActivityEntryDetail, type ExportArtefact } from "../../api";
import { ActivityEvidenceBadge } from "../../kit/activity";
import { CopyableIdentifier, LabelValue, SignedWordedAmount, StatusPill } from "../../kit/money";
import { DesktopDetail, MobileDetail } from "../../kit/pattern-detail";
import { ExplorerLink } from "../../kit/explorer";
import { KitButton } from "../../kit/control";
import { KitList, KitListItem } from "../../kit/display";
import { ScreenCard, StateFrame } from "../../kit/surface";
import { protocolAmount } from "../../kit/model";
import { useShellSelection } from "../../shell/app-shell";
import { PrivateFigure } from "../../settings/privacy";
import { LoadingSurface, OfflineSurface, StillCheckingSurface } from "../../states/surfaces";
import { ActivityErrorSurface } from "./activity";
import {
  activityFailure,
  detailUnresolved,
  entryVerification,
  evidenceClassLabel,
  explorerPath,
  formatEntryTimestamp,
  kindLabel,
  newExportKey,
  plainSentence,
  safeExportArtefact,
  signedBaseUnits,
  stageView,
  toKitDirection,
  unsignedBaseUnits,
  validatedDetail,
  verificationLabel,
  type ActivityFailure,
} from "./model";

const AMOUNT_LOCALE = "en";

type DetailLoad =
  | Readonly<{ kind: "loading" }>
  | Readonly<{ kind: "loaded"; detail: ActivityEntryDetail }>
  | Readonly<{ kind: "offline"; failure: ActivityFailure }>
  | Readonly<{ kind: "error"; failure: ActivityFailure }>;

type DetailExport =
  | Readonly<{ kind: "idle" }>
  | Readonly<{ kind: "preparing" }>
  | Readonly<{ kind: "ready"; artefact: ExportArtefact }>
  | Readonly<{ kind: "failed"; failure: ActivityFailure }>;

function TechnicalDetails({ detail }: Readonly<{ detail: ActivityEntryDetail }>) {
  const shell = useShellSelection().shell;
  const [open, setOpen] = useState(false);
  const Disclosure = shell === "mobile" ? MobileDetail : DesktopDetail;
  const verification = entryVerification([
    ...detail.evidence,
    ...detail.stages.flatMap((stage) => stage.evidence),
  ]);

  return (
    <section className="flex flex-col gap-3">
      {shell === "mobile" ? (
        <KitButton type="button" variant="secondary" onClick={() => { setOpen(true); }}>
          {copyEntry("error.technical.title").message}
        </KitButton>
      ) : null}
      <Disclosure
        open={open}
        onOpenChange={setOpen}
        title={copyEntry("error.technical.title").message}
        summary={copyEntry("error.technical.title").message}
        desktopVariant="inline"
      >
        <div className="flex flex-col gap-5">
          <div className="grid gap-3 sm:grid-cols-2">
            <CopyableIdentifier label={copyEntry("activity.technical.entry_id").message} value={detail.entry_id} />
            {detail.journey_id === undefined ? null : (
              <CopyableIdentifier label={copyEntry("activity.technical.journey_id").message} value={detail.journey_id} />
            )}
            {detail.approval_id === undefined ? null : (
              <CopyableIdentifier label={copyEntry("activity.technical.approval_id").message} value={detail.approval_id} />
            )}
            {detail.agent_id === undefined ? null : (
              <CopyableIdentifier label={copyEntry("activity.technical.agent_id").message} value={detail.agent_id} />
            )}
          </div>
          <dl className="grid gap-3 sm:grid-cols-2">
            <div><dt className="text-sm text-muted-foreground">{copyEntry("activity.detail.state").message}</dt><dd className="font-mono text-sm">{detail.state}</dd></div>
            <div><dt className="text-sm text-muted-foreground">{copyEntry("activity.detail.state_copy").message}</dt><dd className="font-mono text-sm">{detail.state_copy_key}</dd></div>
            <div><dt className="text-sm text-muted-foreground">{copyEntry("activity.detail.summary_copy").message}</dt><dd className="font-mono text-sm">{detail.summary_copy_key}</dd></div>
            <div><dt className="text-sm text-muted-foreground">{copyEntry("activity.technical.verification").message}</dt><dd><ActivityEvidenceBadge>{verificationLabel(verification)}</ActivityEvidenceBadge></dd></div>
          </dl>
          <section className="flex flex-col gap-2">
            <h3 className="font-bold text-foreground">{copyEntry("activity.technical.evidence").message}</h3>
            {detail.evidence.length === 0 ? (
              <p className="text-sm text-muted-foreground">{copyEntry("activity.detail.evidence_empty").message}</p>
            ) : (
              <KitList>
                {detail.evidence.map((reference) => {
                  const path = explorerPath(reference);
                  return (
                    <KitListItem
                      key={`${reference.class}-${reference.evidence_id}`}
                      title={evidenceClassLabel(reference.class)}
                      subtitle={reference.evidence_id}
                      trailing={<ActivityEvidenceBadge>{verificationLabel(reference.verification)}</ActivityEvidenceBadge>}
                      trailingCaption={path === undefined ? undefined : (
                        <ExplorerLink href={path}>{copyEntry("activity.technical.view_in_explorer").message}</ExplorerLink>
                      )}
                    />
                  );
                })}
              </KitList>
            )}
          </section>
        </div>
      </Disclosure>
    </section>
  );
}

export function ActivityDetail({ entryId }: Readonly<{ entryId: string }>) {
  const router = useRouter();
  const client = useMemo(() => humanApi(), []);
  const [load, setLoad] = useState<DetailLoad>({ kind: "loading" });
  const [exportState, setExportState] = useState<DetailExport>({ kind: "idle" });
  const exportKey = useRef<string | undefined>(undefined);

  const refresh = useCallback(() => {
    setLoad({ kind: "loading" });
    client.activityEntry(entryId)
      .then((detail) => { setLoad({ kind: "loaded", detail: validatedDetail(detail) }); })
      .catch((error: unknown) => {
        const failure = activityFailure(error);
        setLoad({ kind: failure.kind === "offline" ? "offline" : "error", failure });
      });
  }, [client, entryId]);

  useEffect(() => { refresh(); }, [refresh]);

  const lookupOutcome = useCallback(async (): Promise<"pending" | "resolved"> => {
    const detail = await client.activityEntry(entryId);
    if (detailUnresolved(detail)) {
      return "pending";
    }
    validatedDetail(detail);
    return "resolved";
  }, [client, entryId]);

  if (load.kind === "loading") {
    return <LoadingSurface rows={5} />;
  }
  if (load.kind === "offline") {
    return <OfflineSurface onRetry={refresh} />;
  }
  if (load.kind === "error") {
    return <ActivityErrorSurface failure={load.failure} onRetry={refresh} />;
  }

  const detail = load.detail;
  if (detailUnresolved(detail)) {
    return (
      <StillCheckingSurface lookupOutcome={lookupOutcome} onResolved={refresh}>
        <p>{plainSentence(detail)}</p>
        <CopyableIdentifier label={copyEntry("activity.technical.entry_id").message} value={detail.entry_id} />
      </StillCheckingSurface>
    );
  }

  const exportEvidence = () => {
    if (exportState.kind === "preparing") return;
    const key = exportKey.current ?? newExportKey();
    exportKey.current = key;
    setExportState({ kind: "preparing" });
    client.activityExportEvidence({ entry_ids: [detail.entry_id] }, key)
      .then((value) => {
        const artefact = safeExportArtefact(value);
        if (artefact.kind !== "evidence-bundle") {
          throw new Error("The detail export was not an evidence bundle");
        }
        exportKey.current = undefined;
        setExportState({ kind: "ready", artefact });
      })
      .catch((error: unknown) => { setExportState({ kind: "failed", failure: activityFailure(error) }); });
  };

  const amount = detail.money === undefined ? undefined : signedBaseUnits(detail.money, detail.direction);
  const fee = detail.fees === undefined ? undefined : unsignedBaseUnits(detail.fees);

  return (
    <ScreenCard title={kindLabel(detail.kind)} description={plainSentence(detail)} landmark="section">
      <div className="mt-4 flex flex-col gap-6">
        <div><KitButton type="button" variant="secondary" onClick={() => { router.push("/app/activity"); }}>{copyEntry("activity.detail.back").message}</KitButton></div>
        <section aria-labelledby="activity-what-happened" className="flex flex-col gap-3">
          <h2 id="activity-what-happened" className="text-lg font-bold text-foreground">{copyEntry("activity.detail.what_happened").message}</h2>
          <p className="text-foreground-secondary">{plainSentence(detail)}</p>
          <div className="grid gap-4 sm:grid-cols-3">
            <LabelValue
              label={copyEntry("activity.detail.amount").message}
              value={amount === undefined || detail.money === undefined ? copyEntry("activity.detail.no_amount").message : (
                <PrivateFigure><SignedWordedAmount
                  value={amount}
                  currency={detail.money.currency}
                  locale={AMOUNT_LOCALE}
                  decimals={0}
                  direction={toKitDirection(detail.direction)}
                /></PrivateFigure>
              )}
            />
            <LabelValue
              label={copyEntry("activity.detail.fee").message}
              value={fee === undefined || detail.fees === undefined ? copyEntry("activity.detail.no_fee").message : (
                <PrivateFigure><SignedWordedAmount
                  value={protocolAmount(fee * -1)}
                  currency={detail.fees.currency}
                  locale={AMOUNT_LOCALE}
                  decimals={0}
                  direction="outbound"
                /></PrivateFigure>
              )}
            />
            <LabelValue label={copyEntry("activity.detail.when").message} value={formatEntryTimestamp(detail.occurred_at)} />
          </div>
        </section>
        {detail.stages.length === 0 ? null : (
          <section aria-labelledby="activity-progress" className="flex flex-col gap-3">
            <h2 id="activity-progress" className="text-lg font-bold text-foreground">{copyEntry("activity.detail.timeline").message}</h2>
            <KitList>
              {detail.stages.map((stage) => {
                const view = stageView(stage);
                return (
                  <KitListItem
                    key={view.id}
                    title={view.label}
                    subtitle={view.evidence.length === 0
                      ? undefined
                      : formatCopy("activity.export.evidence_count", { count: view.evidence.length })}
                    trailing={<StatusPill status={view.status} />}
                  />
                );
              })}
            </KitList>
          </section>
        )}
        <section className="flex flex-col gap-3">
          <div>
            <KitButton
              type="button"
              variant="secondary"
              {...(exportState.kind === "preparing" ? {
                disabled: true,
                disabledReason: copyEntry("activity.export.preparing").message,
              } : {})}
              onClick={exportEvidence}
            >
              {copyEntry("activity.export.evidence").message}
            </KitButton>
          </div>
          {exportState.kind === "ready" ? (
            <StateFrame tone="success" role="status" title={copyEntry("activity.export.ready").message} description={copyEntry("activity.export.ready.body").message}>
              <KitButton asChild><a href={exportState.artefact.download_path} download>{copyEntry("activity.export.download").message}</a></KitButton>
            </StateFrame>
          ) : exportState.kind === "failed" ? (
            <StateFrame tone="danger" role="alert" title={copyEntry("activity.export.failed").message} description={exportState.failure.code} />
          ) : null}
        </section>
        <TechnicalDetails detail={detail} />
      </div>
    </ScreenCard>
  );
}
