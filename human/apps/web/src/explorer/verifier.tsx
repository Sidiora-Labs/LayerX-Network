"use client";

import { useState, type SyntheticEvent } from "react";

import { copyEntry } from "../../copy/catalog";
import {
  ExplorerEvidenceInput,
  ExplorerPanel,
  ExplorerTable,
  ExplorerVerificationBadge,
  InlineNotice,
  KitButton,
  SettingsSegmentedControl,
} from "../kit";
import { FreshnessDisplay, MirrorFreshnessDisplay, verificationLabel } from "./components";
import {
  decodeVerificationReport,
  type EvidenceVerificationReport,
} from "./model";

type EvidenceKind = EvidenceVerificationReport["kind"];

const KIND_OPTIONS: readonly Readonly<{ value: EvidenceKind; label: string }>[] = [
  { value: "receipt", label: copyEntry("explorer.verify.kind.receipt").message },
  { value: "state-inclusion", label: copyEntry("explorer.verify.kind.state").message },
];

export function EvidenceVerifier() {
  const [kind, setKind] = useState<EvidenceKind>("receipt");
  const [evidence, setEvidence] = useState("");
  const [status, setStatus] = useState<"idle" | "checking" | "refused" | "unavailable" | "divergent">("idle");
  const [report, setReport] = useState<EvidenceVerificationReport | undefined>(undefined);

  const verify = async (event: SyntheticEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (status === "checking" || evidence.trim().length === 0) {
      return;
    }
    setStatus("checking");
    setReport(undefined);
    try {
      const response = await fetch("/api/explorer/verify", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ kind, evidence: evidence.trim() }),
      });
      if (!response.ok) {
        setStatus(response.status === 503 ? "unavailable" : response.status===409?"divergent":"refused");
        return;
      }
      setReport(decodeVerificationReport(await response.json()));
      setStatus("idle");
    } catch {
      setStatus("unavailable");
    }
  };

  return (
    <ExplorerPanel title={copyEntry("explorer.verify.form.title").message}>
      <form className="flex flex-col gap-3" onSubmit={(event) => { void verify(event); }}>
        <SettingsSegmentedControl
          aria-label={copyEntry("explorer.verify.kind.label").message}
          options={KIND_OPTIONS.map((option) => ({ ...option }))}
          value={kind}
          onValueChange={(value) => { setKind(value as EvidenceKind); }}
        />
        <label className="flex flex-col gap-1 text-sm font-semibold text-foreground">
          {copyEntry("explorer.verify.evidence.label").message}
          <ExplorerEvidenceInput
            value={evidence}
            maxLength={1_050_000}
            required
            spellCheck={false}
            placeholder={copyEntry("explorer.verify.evidence.placeholder").message}
            onChange={(event) => { setEvidence(event.target.value); }}
          />
        </label>
        <KitButton
          type="submit"
          loading={status === "checking"}
          {...(evidence.trim().length === 0
            ? {
                disabled: true as const,
                disabledReason: copyEntry("explorer.verify.evidence.required").message,
              }
            : {})}
        >
          {copyEntry("explorer.verify.action").message}
        </KitButton>
      </form>
      {status === "refused" ? (
        <InlineNotice tone="danger" role="alert">{copyEntry("explorer.verify.refused").message}</InlineNotice>
      ) : null}
      {status === "unavailable" ? (
        <InlineNotice tone="warning" role="status">{copyEntry("explorer.verify.unavailable").message}</InlineNotice>
      ) : null}
      {status === "divergent" ? (<InlineNotice tone="danger" role="alert">{copyEntry("explorer.verify.divergent").message}</InlineNotice>) : null}
      {report === undefined ? null : (
        <div className="flex flex-col gap-3">
          {report.mirror === undefined
            ? <FreshnessDisplay {...(report.freshness === undefined ? {} : { freshness: report.freshness })} />
            : <MirrorFreshnessDisplay mirror={report.mirror} />}
          <ExplorerTable
            caption={copyEntry("explorer.verify.result.table").message}
            columns={[
              copyEntry("explorer.column.fact").message,
              copyEntry("explorer.column.value").message,
              copyEntry("explorer.column.verification").message,
            ]}
            rows={[
              {
                id: "kind",
                cells: [
                  copyEntry("explorer.verify.result.kind").message,
                  copyEntry(`explorer.verify.kind.${report.kind === "receipt" ? "receipt" : "state"}`).message,
                  <ExplorerVerificationBadge
                    key="verification"
                    label={verificationLabel(report.achievedLevel)}
                    unverified={report.achievedLevel === "unverified"}
                  />,
                ],
              },
              ...(report.receiptDigest === undefined
                ? []
                : [{ id: "receipt", cells: [copyEntry("explorer.column.receipt").message, report.receiptDigest, verificationLabel(report.achievedLevel)] }]),
              ...(report.headerDigest === undefined
                ? []
                : [{ id: "header", cells: [copyEntry("explorer.verify.result.header").message, report.headerDigest, verificationLabel(report.achievedLevel)] }]),
              ...(report.proofRoot === undefined
                ? []
                : [{ id: "root", cells: [copyEntry("explorer.verify.result.root").message, report.proofRoot, verificationLabel(report.achievedLevel)] }]),
              ...(report.mirror===undefined?[]:[
                {id:"source",cells:[copyEntry("explorer.verify.result.source").message,report.mirror.sourceId,report.mirror.provenance]},
                {id:"target",cells:[copyEntry("explorer.verify.result.target").message,report.mirror.target,report.mirror.canonicalPosition]},
                {id:"freshness",cells:[copyEntry("explorer.verify.result.freshness").message,report.mirror.batchLag.kind==="known"?report.mirror.batchLag.batches:copyEntry("explorer.mirror.lag.unknown").message,report.mirror.degraded?copyEntry("explorer.mirror.degraded").message:copyEntry("explorer.mirror.canonical").message]},
              ]),
            ]}
          />
        </div>
      )}
    </ExplorerPanel>
  );
}
