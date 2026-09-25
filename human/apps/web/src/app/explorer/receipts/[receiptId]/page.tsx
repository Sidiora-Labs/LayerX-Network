import { copyEntry } from "../../../../../copy/runtime";
import { explorerLink, receiptRecord } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerNotFound,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../../explorer/components";
import { ExplorerExternalLink, ExplorerPanel, ExplorerTable, ExplorerVerificationBadge } from "../../../../kit/explorer";

export default async function ReceiptPage({
  params,
}: Readonly<{ params: Promise<{ receiptId: string }> }>) {
  let record;
  try {
    record = await receiptRecord((await params).receiptId);
  } catch {
    return <ExplorerUnavailable />;
  }
  let anchors: string | undefined;
  try {
    anchors = explorerLink({ kind: "batch" });
  } catch {
    anchors = undefined;
  }
  if (record.value === undefined) {
    return (
      <ExplorerNotFound
        title={copyEntry("explorer.receipt.title").message}
        freshness={record.freshness}
      />
    );
  }
  const receipt = record.value;
  const verification = (
    <ExplorerVerificationBadge
      label={verificationLabel(receipt.verificationLevel)}
      unverified={receipt.verificationLevel === "unverified"}
    />
  );
  return (
    <ExplorerFrame
      title={copyEntry("explorer.receipt.title").message}
      description={receipt.receiptId}
    >
      <FreshnessDisplay freshness={record.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.receipt.facts").message}
        columns={[
          copyEntry("explorer.column.fact").message,
          copyEntry("explorer.column.value").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={[
          { id: "receipt", cells: [copyEntry("explorer.column.receipt").message, receipt.receiptId, verification] },
          {
            id: "batch",
            cells: [
              copyEntry("explorer.column.batch").message,
              anchors === undefined
                ? receipt.batchNumber
                : (
                  <ExplorerExternalLink key="batch" href={anchors}>
                    {receipt.batchNumber}
                  </ExplorerExternalLink>
                ),
              verification,
            ],
          },
          { id: "ordinal", cells: [copyEntry("explorer.fact.ordinal").message, receipt.ordinal, verification] },
        ]}
      />
      <ExplorerPanel title={copyEntry("explorer.receipt.bytes").message}>
        <p className="break-all font-mono text-xs text-foreground-secondary">
          {receipt.canonicalBytes}
        </p>
      </ExplorerPanel>
    </ExplorerFrame>
  );
}
