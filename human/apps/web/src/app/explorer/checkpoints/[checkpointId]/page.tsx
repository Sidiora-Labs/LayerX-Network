import { copyEntry } from "../../../../../copy/catalog";
import { checkpointRecord } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerNotFound,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../../explorer/components";
import { ExplorerTable, ExplorerVerificationBadge } from "../../../../kit/explorer";

export default async function CheckpointPage({
  params,
}: Readonly<{ params: Promise<{ checkpointId: string }> }>) {
  let record;
  try {
    record = await checkpointRecord((await params).checkpointId);
  } catch {
    return <ExplorerUnavailable />;
  }
  if (record.value === undefined) {
    return (
      <ExplorerNotFound
        title={copyEntry("explorer.checkpoint.title").message}
        freshness={record.freshness}
      />
    );
  }
  const checkpoint = record.value;
  const verification = (
    <ExplorerVerificationBadge
      label={verificationLabel(checkpoint.verificationLevel)}
      unverified={checkpoint.verificationLevel === "unverified"}
    />
  );
  return (
    <ExplorerFrame
      title={copyEntry("explorer.checkpoint.title").message}
      description={checkpoint.checkpointId}
    >
      <FreshnessDisplay freshness={record.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.checkpoint.facts").message}
        columns={[
          copyEntry("explorer.column.fact").message,
          copyEntry("explorer.column.value").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={[
          { id: "checkpoint", cells: [copyEntry("explorer.column.checkpoint").message, checkpoint.checkpointId, verification] },
          { id: "batch", cells: [copyEntry("explorer.column.batch").message, checkpoint.batchNumber, verification] },
          { id: "first", cells: [copyEntry("explorer.fact.first_sequence").message, checkpoint.firstSequence, verification] },
          { id: "last", cells: [copyEntry("explorer.fact.last_sequence").message, checkpoint.lastSequence, verification] },
          {
            id: "signatures",
            cells: [
              copyEntry("explorer.column.signatures").message,
              `${checkpoint.achievedSignatures}/${checkpoint.requiredSignatures}`,
              verification,
            ],
          },
        ]}
      />
    </ExplorerFrame>
  );
}
