import { copyEntry } from "../../../../copy/runtime";
import { checkpointPage } from "../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../explorer/components";
import { ExplorerLink, ExplorerTable, ExplorerVerificationBadge } from "../../../kit/explorer";

export default async function CheckpointsPage({
  searchParams,
}: Readonly<{ searchParams: Promise<{ before?: string }> }>) {
  let page;
  try {
    page = await checkpointPage((await searchParams).before);
  } catch {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.checkpoints.title").message}
      description={copyEntry("explorer.checkpoints.body").message}
    >
      <FreshnessDisplay freshness={page.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.checkpoints.table").message}
        columns={[
          copyEntry("explorer.column.batch").message,
          copyEntry("explorer.column.checkpoint").message,
          copyEntry("explorer.column.sequences").message,
          copyEntry("explorer.column.signatures").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={page.items.map((checkpoint) => ({
          id: checkpoint.checkpointId,
          cells: [
            checkpoint.batchNumber,
            <ExplorerLink key="checkpoint" href={`/explorer/checkpoints/${checkpoint.checkpointId}`}>
              {checkpoint.checkpointId}
            </ExplorerLink>,
            `${checkpoint.firstSequence}–${checkpoint.lastSequence}`,
            `${checkpoint.achievedSignatures}/${checkpoint.requiredSignatures}`,
            <ExplorerVerificationBadge
              key="verification"
              label={verificationLabel(checkpoint.verificationLevel)}
              unverified={checkpoint.verificationLevel === "unverified"}
            />,
          ],
        }))}
      />
      {page.nextBefore === undefined ? null : (
        <ExplorerLink href={`/explorer/checkpoints?before=${page.nextBefore}`}>
          {copyEntry("explorer.pagination.older").message}
        </ExplorerLink>
      )}
    </ExplorerFrame>
  );
}
