import { copyEntry } from "../../../copy/catalog";
import { batchPage, checkpointPage } from "../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../explorer/components";
import {
  ExplorerLink,
  ExplorerLookupForm,
  ExplorerPanel,
  ExplorerTable,
  ExplorerVerificationBadge,
  PlaneRouteAction,
} from "../../kit";

export default async function ExplorerPlanePage() {
  let checkpoints;
  let batches;
  try {
    [checkpoints, batches] = await Promise.all([checkpointPage(undefined, "10"), batchPage(undefined, "10")]);
  } catch {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.title").message}
      description={copyEntry("explorer.summary").message}
    >
      <FreshnessDisplay freshness={checkpoints.freshness} />
      <div className="grid gap-4 lg:grid-cols-2">
        <ExplorerPanel title={copyEntry("explorer.lookup.receipt.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="receipt"
            label={copyEntry("explorer.lookup.receipt.label").message}
            placeholder={copyEntry("explorer.lookup.receipt.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
        <ExplorerPanel title={copyEntry("explorer.lookup.account.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="account"
            label={copyEntry("explorer.lookup.account.label").message}
            placeholder={copyEntry("explorer.lookup.account.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
        <ExplorerPanel title={copyEntry("explorer.lookup.program.title").message}>
          <ExplorerLookupForm
            action="/explorer/lookup"
            kind="program"
            label={copyEntry("explorer.lookup.program.label").message}
            placeholder={copyEntry("explorer.lookup.program.placeholder").message}
            submitLabel={copyEntry("explorer.lookup.action").message}
          />
        </ExplorerPanel>
      </div>
      <ExplorerPanel title={copyEntry("explorer.checkpoints.recent").message}>
        <ExplorerTable
          caption={copyEntry("explorer.checkpoints.table").message}
          columns={[
            copyEntry("explorer.column.batch").message,
            copyEntry("explorer.column.checkpoint").message,
            copyEntry("explorer.column.sequences").message,
            copyEntry("explorer.column.verification").message,
          ]}
          rows={checkpoints.items.map((checkpoint) => ({
            id: checkpoint.checkpointId,
            cells: [
              checkpoint.batchNumber,
              <ExplorerLink key="checkpoint" href={`/explorer/checkpoints/${checkpoint.checkpointId}`}>
                {checkpoint.checkpointId}
              </ExplorerLink>,
              `${checkpoint.firstSequence}–${checkpoint.lastSequence}`,
              <ExplorerVerificationBadge
                key="verification"
                label={verificationLabel(checkpoint.verificationLevel)}
                unverified={checkpoint.verificationLevel === "unverified"}
              />,
            ],
          }))}
        />
      </ExplorerPanel>
      <ExplorerPanel title={copyEntry("explorer.batches.recent").message}>
        <ExplorerTable
          caption={copyEntry("explorer.batches.table").message}
          columns={[
            copyEntry("explorer.column.batch").message,
            copyEntry("explorer.column.receipts").message,
            copyEntry("explorer.column.events").message,
            copyEntry("explorer.column.verification").message,
          ]}
          rows={batches.items.map((batch) => ({
            id: batch.batchNumber,
            cells: [
              <ExplorerLink key="batch" href={`/explorer/batches/${batch.batchNumber}`}>
                {batch.batchNumber}
              </ExplorerLink>,
              batch.receiptCount,
              batch.eventCount,
              <ExplorerVerificationBadge
                key="verification"
                label={verificationLabel(batch.verificationLevel)}
                unverified={batch.verificationLevel === "unverified"}
              />,
            ],
          }))}
        />
      </ExplorerPanel>
      <div>
        <PlaneRouteAction destination="/app">
          {copyEntry("action.open_app").message}
        </PlaneRouteAction>
      </div>
    </ExplorerFrame>
  );
}
