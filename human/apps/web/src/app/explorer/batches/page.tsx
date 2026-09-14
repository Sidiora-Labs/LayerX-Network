import { copyEntry } from "../../../../copy/runtime";
import { batchPage } from "../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../explorer/components";
import { ExplorerLink, ExplorerTable, ExplorerVerificationBadge } from "../../../kit/explorer";

export default async function BatchesPage({
  searchParams,
}: Readonly<{ searchParams: Promise<{ before?: string }> }>) {
  let page;
  try {
    page = await batchPage((await searchParams).before);
  } catch {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.batches.title").message}
      description={copyEntry("explorer.batches.body").message}
    >
      <FreshnessDisplay freshness={page.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.batches.table").message}
        columns={[
          copyEntry("explorer.column.batch").message,
          copyEntry("explorer.column.activities").message,
          copyEntry("explorer.column.receipts").message,
          copyEntry("explorer.column.events").message,
          copyEntry("explorer.column.bytes").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={page.items.map((batch) => ({
          id: batch.batchNumber,
          cells: [
            <ExplorerLink key="batch" href={`/explorer/batches/${batch.batchNumber}`}>
              {batch.batchNumber}
            </ExplorerLink>,
            batch.activityCount,
            batch.receiptCount,
            batch.eventCount,
            batch.totalAvailabilityBytes,
            <ExplorerVerificationBadge
              key="verification"
              label={verificationLabel(batch.verificationLevel)}
              unverified={batch.verificationLevel === "unverified"}
            />,
          ],
        }))}
      />
      {page.nextBefore === undefined ? null : (
        <ExplorerLink href={`/explorer/batches?before=${page.nextBefore}`}>
          {copyEntry("explorer.pagination.older").message}
        </ExplorerLink>
      )}
    </ExplorerFrame>
  );
}
