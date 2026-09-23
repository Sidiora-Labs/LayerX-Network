import { Badge } from "@layerx/ui/components/badge";

import {
  GatewayRpcError,
  GatewayUnavailableError,
  layerxHistory,
  paxeerHistory,
  unifiedHistory,
  type HistoryPage,
} from "../../api/gateway";
import { ExplorerLink, ExplorerPanel, ExplorerTable } from "../../kit/explorer";
import { StateEmpty } from "../../kit/surface";
import { MarketUnavailable } from "./frame";
import { formatUnits, shortId } from "./format";

export type ActivitySide = "unified" | "layerx" | "paxeer";

export function activitySide(value: string | undefined): ActivitySide {
  return value === "layerx" || value === "paxeer" ? value : "unified";
}

async function page(
  side: ActivitySide,
  account: string,
  cursor: string | undefined,
  kind: string | undefined,
): Promise<HistoryPage> {
  switch (side) {
    case "layerx":
      return layerxHistory(account, { cursor, kind });
    case "paxeer":
      return paxeerHistory(account, { cursor, kind });
    case "unified":
      return unifiedHistory(account, { cursor, kind });
  }
}

const METHOD: Readonly<Record<ActivitySide, string>> = {
  unified: "px_getUnifiedHistory",
  layerx: "lx_getHistory",
  paxeer: "px_getHistory",
};

/** The gateway history of `account`, the same rows an agent reads through the SDK history methods. */
export async function ActivityFeed({
  route,
  account,
  side,
  cursor,
  kind,
}: Readonly<{
  route: string;
  account: string | undefined;
  side: ActivitySide;
  cursor: string | undefined;
  kind: string | undefined;
}>) {
  const title = `Activity (${METHOD[side]})`;
  if (account === undefined) {
    return (
      <ExplorerPanel title={title}>
        <StateEmpty title="No account yet" description="Connect a wallet to see its history." />
      </ExplorerPanel>
    );
  }
  let history: HistoryPage;
  try {
    history = await page(side, account, cursor, kind);
  } catch (error) {
    if (error instanceof GatewayUnavailableError || error instanceof GatewayRpcError) {
      return (
        <ExplorerPanel title={title}>
          <MarketUnavailable detail={error.message} />
        </ExplorerPanel>
      );
    }
    throw error;
  }
  const query = (extra: Readonly<Record<string, string>>) => {
    const parameters = new URLSearchParams({ account, side, ...(kind === undefined ? {} : { kind }), ...extra });
    return `${route}?${parameters.toString()}`;
  };
  return (
    <ExplorerPanel title={title}>
      <nav aria-label="History source" className="flex flex-wrap gap-3 text-sm">
        {(["unified", "paxeer", "layerx"] as const).map((option) => (
          <ExplorerLink key={option} href={query({ side: option })}>
            {option === side ? `[${METHOD[option]}]` : METHOD[option]}
          </ExplorerLink>
        ))}
      </nav>
      {history.items.length === 0 ? (
        <StateEmpty title="No activity" description="The indexer has no rows for this account yet." />
      ) : (
        <ExplorerTable
          caption={title}
          columns={["Chain", "Kind", "Direction", "Amount", "Counterparty", "Transaction", "Finality"]}
          rows={history.items.map((row) => ({
            id: row.id.toString(),
            cells: [
              row.side ?? row.chain,
              row.kind,
              row.direction,
              `${row.assetDecimals === null ? row.amount.toString() : formatUnits(row.amount, row.assetDecimals)} ${row.assetSymbol ?? shortId(row.asset)}`,
              row.counterparty === null ? "—" : shortId(row.counterparty),
              shortId(row.txId),
              <Badge key="final" variant={row.final ? "success" : "warning"}>
                {row.final ? "Final" : "Pending finality"}
              </Badge>,
            ],
          }))}
        />
      )}
      {history.nextCursor === null ? null : (
        <ExplorerLink href={query({ cursor: history.nextCursor })}>Older activity</ExplorerLink>
      )}
    </ExplorerPanel>
  );
}
