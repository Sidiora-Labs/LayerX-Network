import { Badge } from "@layerx/ui/components/badge";

import {
  GatewayRpcError,
  GatewayUnavailableError,
  pxResolveAccount,
  surfaceCapability,
  type ResolvedAccount,
} from "../../api/gateway";
import { PRECOMPILE_LOG_WINDOW, exchangeState, type ExchangeState } from "../../api/precompiles";
import { PrecompileAbiError } from "../../api/sdk";
import { ExplorerPanel, ExplorerTable } from "../../kit/explorer";
import { StateEmpty } from "../../kit/surface";
import { ActivityFeed, activitySide } from "../_markets/activity-feed";
import { SurfaceCapabilityGate } from "../_markets/capability";
import { WEI_DECIMALS, firstParam, formatUnits, isEvmAddress, shortId, type MarketSearchParams } from "../_markets/format";
import { MarketFrame, MarketUnavailable } from "../_markets/frame";
import { ExchangeActions } from "./exchange-client";

export const dynamic = "force-dynamic";

const TIME_IN_FORCE = ["Good till cancelled", "Immediate or cancel", "Fill or kill", "Post only"] as const;

function sideLabel(side: bigint): string {
  return side === 1n ? "Buy" : side === 2n ? "Sell" : side.toString();
}

function eventAmount(value: bigint | boolean | string | undefined): string {
  return typeof value === "bigint" ? formatUnits(value, WEI_DECIMALS) : "—";
}

export default async function ExchangePage({ searchParams }: Readonly<{ searchParams: Promise<MarketSearchParams> }>) {
  const parameters = await searchParams;
  const candidate = firstParam(parameters.account)?.toLowerCase();
  const account = isEvmAddress(candidate) ? candidate : undefined;
  const capability = await surfaceCapability("exchange");
  const feed = (
    <ActivityFeed
      route="/exchange"
      account={account}
      side={activitySide(firstParam(parameters.side))}
      cursor={firstParam(parameters.cursor)}
      kind={firstParam(parameters.kind)}
    />
  );
  if (!capability.live) {
    return (
      <MarketFrame
        title="Exchange"
        description="Perps orders and margin through the LayerXExchange precompile. Every write is an intent the LayerX exchange settles."
        account={account}
      >
        <SurfaceCapabilityGate surface="exchange" live={false} detail={capability.detail} />
        {feed}
      </MarketFrame>
    );
  }
  let resolved: ResolvedAccount | undefined;
  let state: ExchangeState | undefined;
  let problem: string | undefined;
  if (account !== undefined) {
    try {
      [resolved, state] = await Promise.all([pxResolveAccount(account), exchangeState(account)]);
    } catch (error) {
      if (
        !(error instanceof GatewayUnavailableError || error instanceof GatewayRpcError || error instanceof PrecompileAbiError)
      ) {
        throw error;
      }
      problem = error.message;
    }
  }
  const marginEvents =
    state?.events.filter(
      (entry) => entry.event.event === "MarginDeposited" || entry.event.event === "MarginWithdrawalRequested",
    ) ?? [];
  return (
    <MarketFrame
      title="Exchange"
      description="Perps orders and margin through the LayerXExchange precompile. Every write is an intent the LayerX exchange settles."
      account={account}
    >
      <SurfaceCapabilityGate surface="exchange" live detail={null} />
      {problem === undefined ? null : <MarketUnavailable detail={problem} />}
      {account !== undefined && resolved !== undefined && resolved.layerxAccount === null ? (
        <MarketUnavailable detail="This wallet has no bound LayerX account yet, so margin has nowhere to land. Bind the wallet in Settings first." />
      ) : null}
      <ExplorerPanel title="Open orders">
        {state === undefined || state.openOrders.length === 0 ? (
          <StateEmpty
            title="No open orders"
            description={`Orders placed from this wallet in the last ${PRECOMPILE_LOG_WINDOW.toString()} blocks appear here.`}
          />
        ) : (
          <ExplorerTable
            caption="Open orders"
            columns={["Market", "Side", "Price", "Quantity", "Time in force", "Order", "Intent"]}
            rows={state.openOrders.map((order) => ({
              id: order.intentId,
              cells: [
                shortId(order.marketId),
                sideLabel(order.side),
                order.price.toString(),
                order.quantity.toString(),
                TIME_IN_FORCE[Number(order.timeInForce)] ?? order.timeInForce.toString(),
                order.orderId === null ? "Awaiting LayerX" : shortId(order.orderId),
                <Badge key="intent" variant={order.pending ? "warning" : "success"}>
                  {order.pending ? "Pending" : "Processed"}
                </Badge>,
              ],
            }))}
          />
        )}
      </ExplorerPanel>
      <ExplorerPanel title="Positions">
        {state === undefined || state.positions.length === 0 ? (
          <StateEmpty
            title="No positions in settlement"
            description="Positions you ask to settle appear here. Use Request settlement with a position id from your LayerX activity."
          />
        ) : (
          <ExplorerTable
            caption="Positions"
            columns={["Position", "Settlement", "Transaction"]}
            rows={state.positions.map((position) => ({
              id: `${position.positionId}-${position.transactionHash}`,
              cells: [
                shortId(position.positionId),
                <Badge key="settlement" variant={position.settlementPending ? "warning" : "success"}>
                  {position.settlementPending ? "Settlement pending" : "Settlement processed"}
                </Badge>,
                shortId(position.transactionHash),
              ],
            }))}
          />
        )}
      </ExplorerPanel>
      <ExplorerPanel title="Margin movements">
        {marginEvents.length === 0 ? (
          <StateEmpty title="No margin movements" description="Deposits and withdrawal requests from this wallet appear here." />
        ) : (
          <ExplorerTable
            caption="Margin movements"
            columns={["Movement", "Asset", "Amount", "Block", "Transaction"]}
            rows={marginEvents.map((entry) => ({
              id: `${entry.transactionHash}-${entry.logIndex.toString()}`,
              cells: [
                entry.event.event === "MarginDeposited" ? "Deposit" : "Withdrawal request",
                shortId(String(entry.event.fields.assetId ?? "")),
                eventAmount(entry.event.fields.amount),
                entry.blockNumber.toString(),
                shortId(entry.transactionHash),
              ],
            }))}
          />
        )}
      </ExplorerPanel>
      <ExchangeActions
        account={account}
        layerxAccount={resolved?.layerxAccount ?? undefined}
        openOrders={(state?.openOrders ?? []).flatMap((order) =>
          order.orderId === null ? [] : [{ orderId: order.orderId, label: `${sideLabel(order.side)} ${order.quantity.toString()} @ ${order.price.toString()}` }],
        )}
        timeInForce={TIME_IN_FORCE}
      />
      {feed}
    </MarketFrame>
  );
}
