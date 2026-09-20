import { redirect } from "next/navigation";

import { copyEntry } from "../../../../../copy/runtime";
import { formatCopy } from "../../../../../copy/format";
import { accountActivityPage, unifiedAccount } from "../../../../explorer/client";
import {
  ExplorerFrame,
  ExplorerUnavailable,
  FreshnessDisplay,
  verificationLabel,
} from "../../../../explorer/components";
import {
  accountIdentifierPath,
  parseAccountIdentifier,
  type PaxeerActivityEvent,
  type UnifiedAccountRecord,
} from "../../../../explorer/model";
import { ExplorerLink, ExplorerPanel, ExplorerTable, ExplorerVerificationBadge } from "../../../../kit/explorer";

function eventLabel(event: PaxeerActivityEvent): string {
  return copyEntry(`explorer.account.event.${event.replaceAll("-", "_")}`).message;
}

function absentValue(): string {
  return copyEntry("explorer.value.none").message;
}

function IdentitiesPanel({ account }: Readonly<{ account?: UnifiedAccountRecord }>) {
  const identities = account?.identities;
  return (
    <ExplorerPanel title={copyEntry("explorer.account.identities").message}>
      <p className="text-sm text-foreground-secondary">
        {copyEntry(identities?.bound === true
          ? "explorer.account.linked.body"
          : "explorer.account.not_linked.body").message}
      </p>
      <ExplorerTable
        caption={copyEntry("explorer.account.identities.table").message}
        columns={[
          copyEntry("explorer.column.fact").message,
          copyEntry("explorer.column.value").message,
        ]}
        rows={[
          {
            id: "evm",
            cells: [copyEntry("explorer.account.identity.evm").message, identities?.evmAddress ?? absentValue()],
          },
          {
            id: "pax",
            cells: [copyEntry("explorer.account.identity.pax").message, identities?.paxAddress ?? absentValue()],
          },
          {
            id: "did",
            cells: [copyEntry("explorer.account.identity.did").message, identities?.layerxDid ?? absentValue()],
          },
          {
            id: "account",
            cells: [
              copyEntry("explorer.account.identity.account").message,
              identities?.layerxAccount === undefined
                ? absentValue()
                : (
                    <ExplorerLink href={accountIdentifierPath(identities.layerxAccount)}>
                      {identities.layerxAccount}
                    </ExplorerLink>
                  ),
            ],
          },
        ]}
      />
    </ExplorerPanel>
  );
}

function BalancesPanel({ account }: Readonly<{ account?: UnifiedAccountRecord }>) {
  return (
    <ExplorerPanel title={copyEntry("explorer.account.balances").message}>
      <ExplorerTable
        caption={copyEntry("explorer.account.balances.table").message}
        columns={[
          copyEntry("explorer.column.asset").message,
          copyEntry("explorer.column.on_paxeer").message,
          copyEntry("explorer.column.on_layerx").message,
          copyEntry("explorer.column.in_custody").message,
        ]}
        rows={(account?.balances.items ?? []).map((balance) => ({
          id: balance.assetId,
          cells: [
            balance.denom,
            balance.paxeer ?? absentValue(),
            balance.layerx ?? absentValue(),
            balance.custody ?? absentValue(),
          ],
        }))}
      />
      <p className="text-sm text-foreground-secondary">
        {copyEntry("explorer.account.gateway_reported").message}
      </p>
    </ExplorerPanel>
  );
}

function SettlementPanel({ account }: Readonly<{ account?: UnifiedAccountRecord }>) {
  const settlement = account?.settlement;
  return (
    <ExplorerPanel title={copyEntry("explorer.account.settlement").message}>
      <ExplorerTable
        caption={copyEntry("explorer.account.settlement.table").message}
        columns={[
          copyEntry("explorer.column.stage").message,
          copyEntry("explorer.column.value").message,
        ]}
        rows={[
          {
            id: "instant",
            cells: [
              copyEntry("explorer.settlement.instant").message,
              settlement === undefined
                ? absentValue()
                : formatCopy("explorer.settlement.instant.detail", { block: settlement.instantBlock }),
            ],
          },
          {
            id: "sealed",
            cells: [
              copyEntry("explorer.settlement.sealed").message,
              settlement === undefined
                ? absentValue()
                : formatCopy("explorer.settlement.sealed.detail", { batch: settlement.sealedBatch }),
            ],
          },
          {
            id: "final",
            cells: [
              copyEntry("explorer.settlement.final").message,
              settlement === undefined
                ? absentValue()
                : formatCopy("explorer.settlement.final.detail", {
                    batch: settlement.finalizedBatch,
                    status: settlement.anchorStatusName,
                  }),
            ],
          },
        ]}
      />
    </ExplorerPanel>
  );
}

function PaxeerActivityPanel({ account }: Readonly<{ account?: UnifiedAccountRecord }>) {
  const activity = account?.paxeerActivity;
  return (
    <ExplorerPanel title={copyEntry("explorer.account.paxeer_activity").message}>
      <p className="text-sm text-foreground-secondary">
        {activity === undefined
          ? copyEntry("explorer.account.paxeer_activity.unavailable").message
          : formatCopy("explorer.account.paxeer_activity.window", {
              fromBlock: activity.fromBlock,
              toBlock: activity.toBlock,
            })}
      </p>
      <ExplorerTable
        caption={copyEntry("explorer.account.paxeer_activity.table").message}
        columns={[
          copyEntry("explorer.column.block").message,
          copyEntry("explorer.column.event").message,
          copyEntry("explorer.column.amount").message,
          copyEntry("explorer.column.transaction").message,
        ]}
        rows={(activity?.items ?? []).map((record) => ({
          id: `${record.blockNumber}-${record.logIndex}`,
          cells: [
            record.blockNumber,
            eventLabel(record.event),
            record.amount ?? absentValue(),
            record.transactionHash,
          ],
        }))}
      />
    </ExplorerPanel>
  );
}

export default async function AccountPage({
  params,
  searchParams,
}: Readonly<{
  params: Promise<{ accountId: string }>;
  searchParams: Promise<{ before?: string; beforeBlock?: string }>;
}>) {
  const requested = (await params).accountId;
  const identifier = parseAccountIdentifier(requested);
  if (identifier === undefined) {
    return (
      <ExplorerFrame
        title={copyEntry("explorer.account.title").message}
        description={copyEntry("explorer.account.invalid").message}
      >
        <FreshnessDisplay />
        <p className="text-sm text-foreground-secondary">{copyEntry("explorer.not_found.body").message}</p>
      </ExplorerFrame>
    );
  }
  if (identifier.canonical !== requested) {
    redirect(accountIdentifierPath(identifier.canonical));
  }
  const query = await searchParams;
  let account: UnifiedAccountRecord | undefined;
  try {
    account = await unifiedAccount(identifier.canonical, query.beforeBlock);
  } catch {
    account = undefined;
  }
  if (account !== undefined && account.canonical !== identifier.canonical) {
    redirect(accountIdentifierPath(account.canonical));
  }
  const layerxAccount = account?.identities.layerxAccount
    ?? (identifier.kind === "account" ? identifier.canonical : undefined);
  let activity;
  if (layerxAccount !== undefined) {
    try {
      activity = await accountActivityPage(layerxAccount, query.before);
    } catch {
      activity = undefined;
    }
  }
  if (account === undefined && activity === undefined) {
    return <ExplorerUnavailable />;
  }
  return (
    <ExplorerFrame
      title={copyEntry("explorer.account.title").message}
      description={identifier.canonical}
    >
      <FreshnessDisplay freshness={activity?.freshness} />
      <IdentitiesPanel account={account} />
      <BalancesPanel account={account} />
      <SettlementPanel account={account} />
      <ExplorerPanel title={copyEntry("explorer.account.layerx_activity").message}>
        {activity === undefined
          ? (
              <p className="text-sm text-foreground-secondary">
                {copyEntry("explorer.account.layerx_activity.absent").message}
              </p>
            )
          : (
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
                rows={activity.items.map((record) => ({
                  id: record.receiptId,
                  cells: [
                    record.globalSequence,
                    <ExplorerLink key="receipt" href={`/explorer/receipts/${record.receiptDigest}`}>
                      {record.receiptDigest}
                    </ExplorerLink>,
                    record.operation,
                    record.amount,
                    record.resultCode,
                    <ExplorerVerificationBadge
                      key="verification"
                      label={verificationLabel(record.verificationLevel)}
                      unverified={record.verificationLevel === "unverified"}
                    />,
                  ],
                }))}
              />
            )}
        {activity?.nextBefore === undefined ? null : (
          <ExplorerLink href={`${accountIdentifierPath(identifier.canonical)}?before=${activity.nextBefore}`}>
            {copyEntry("explorer.pagination.older").message}
          </ExplorerLink>
        )}
      </ExplorerPanel>
      <PaxeerActivityPanel account={account} />
      {account?.paxeerActivity.nextBeforeBlock === undefined ? null : (
        <ExplorerLink
          href={`${accountIdentifierPath(identifier.canonical)}?beforeBlock=${account.paxeerActivity.nextBeforeBlock}`}
        >
          {copyEntry("explorer.pagination.older").message}
        </ExplorerLink>
      )}
    </ExplorerFrame>
  );
}
