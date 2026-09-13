import { copyEntry } from "../../../../../copy/runtime";
import { batchRecord } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerNotFound,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../../explorer/components";
import { ExplorerLink, ExplorerTable, ExplorerVerificationBadge } from "../../../../kit/explorer";

export default async function BatchPage({
  params,
}: Readonly<{ params: Promise<{ batchNumber: string }> }>) {
  let record;
  try {
    record = await batchRecord((await params).batchNumber);
  } catch {
    return <ExplorerUnavailable />;
  }
  if (record.value === undefined) {
    return (
      <ExplorerNotFound
        title={copyEntry("explorer.batch.title").message}
        freshness={record.freshness}
      />
    );
  }
  const batch = record.value;
  const verification = (
    <ExplorerVerificationBadge
      label={verificationLabel(batch.verificationLevel)}
      unverified={batch.verificationLevel === "unverified"}
    />
  );
  return (
    <ExplorerFrame
      title={copyEntry("explorer.batch.title").message}
      description={batch.batchNumber}
    >
      <FreshnessDisplay freshness={record.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.batch.facts").message}
        columns={[
          copyEntry("explorer.column.fact").message,
          copyEntry("explorer.column.value").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={[
          { id: "batch", cells: [copyEntry("explorer.column.batch").message, batch.batchNumber, verification] },
          { id: "activities", cells: [copyEntry("explorer.column.activities").message, batch.activityCount, verification] },
          { id: "receipts", cells: [copyEntry("explorer.column.receipts").message, batch.receiptCount, verification] },
          { id: "events", cells: [copyEntry("explorer.column.events").message, batch.eventCount, verification] },
          { id: "bytes", cells: [copyEntry("explorer.column.bytes").message, batch.totalAvailabilityBytes, verification] },
          {
            id: "checkpoint",
            cells: [
              copyEntry("explorer.column.checkpoint").message,
              batch.checkpointId === undefined
                ? copyEntry("explorer.value.none").message
                : (
                    <ExplorerLink href={`/explorer/checkpoints/${batch.checkpointId}`}>
                      {batch.checkpointId}
                    </ExplorerLink>
                  ),
              verification,
            ],
          },
        ]}
      />
    </ExplorerFrame>
  );
}
