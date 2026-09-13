import { copyEntry } from "../../../../../copy/runtime";
import { accountActivityPage } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../../explorer/components";
import { ExplorerLink, ExplorerTable, ExplorerVerificationBadge } from "../../../../kit/explorer";

export default async function AccountPage({
  params,
  searchParams,
}: Readonly<{
  params: Promise<{ accountId: string }>;
  searchParams: Promise<{ before?: string }>;
}>) {
  const accountId = (await params).accountId;
  let page;
  try {
    page = await accountActivityPage(accountId, (await searchParams).before);
  } catch {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.account.title").message}
      description={accountId}
    >
      <FreshnessDisplay freshness={page.freshness} />
      <ExplorerTable
        caption={copyEntry("explorer.account.table").message}
        columns={[
          copyEntry("explorer.column.sequence").message,
          copyEntry("explorer.column.receipt").message,
          copyEntry("explorer.column.operation").message,
          copyEntry("explorer.column.amount").message,
          copyEntry("explorer.column.result").message,
          copyEntry("explorer.column.verification").message,
        ]}
        rows={page.items.map((activity) => ({
          id: activity.receiptId,
          cells: [
            activity.globalSequence,
            <ExplorerLink key="receipt" href={`/explorer/receipts/${activity.receiptDigest}`}>
              {activity.receiptDigest}
            </ExplorerLink>,
            activity.operation,
            activity.amount,
            activity.resultCode,
            <ExplorerVerificationBadge
              key="verification"
              label={verificationLabel(activity.verificationLevel)}
              unverified={activity.verificationLevel === "unverified"}
            />,
          ],
        }))}
      />
      {page.nextBefore === undefined ? null : (
        <ExplorerLink href={`/explorer/accounts/${accountId}?before=${page.nextBefore}`}>
          {copyEntry("explorer.pagination.older").message}
        </ExplorerLink>
      )}
    </ExplorerFrame>
  );
}
