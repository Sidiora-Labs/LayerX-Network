import { Badge } from "@layerx/ui/components/badge";

import { GatewayRpcError, GatewayUnavailableError } from "../../api/gateway";
import {
  launchpadConfig,
  launchpadMarketCount,
  launchpadMarkets,
  type LaunchpadConfig,
  type LaunchpadMarket,
} from "../../api/precompiles";
import { ExplorerPanel, ExplorerTable } from "../../kit/explorer";
import { LabelValue } from "../../kit/money";
import { StateEmpty } from "../../kit/surface";
import { ActivityFeed, activitySide } from "../_markets/activity-feed";
import {
  LAUNCHPAD_DECIMALS,
  firstParam,
  formatUnits,
  isEvmAddress,
  shortId,
  type MarketSearchParams,
} from "../_markets/format";
import { MarketFrame, MarketUnavailable } from "../_markets/frame";
import { LaunchpadActions } from "./launchpad-client";

export const dynamic = "force-dynamic";

const PAGE_SIZE = 50n;
const FEE_STRATEGIES = ["Claim", "Burn", "Airdrop", "LP rewards"] as const;

export default async function LaunchpadPage({ searchParams }: Readonly<{ searchParams: Promise<MarketSearchParams> }>) {
  const parameters = await searchParams;
  const candidate = firstParam(parameters.account)?.toLowerCase();
  const account = isEvmAddress(candidate) ? candidate : undefined;
  let config: LaunchpadConfig | undefined;
  let count = 0n;
  let markets: readonly LaunchpadMarket[] = [];
  let problem: string | undefined;
  try {
    [config, count, markets] = await Promise.all([
      launchpadConfig(),
      launchpadMarketCount(),
      launchpadMarkets(0n, PAGE_SIZE),
    ]);
  } catch (error) {
    if (!(error instanceof GatewayUnavailableError || error instanceof GatewayRpcError)) {
      throw error;
    }
    problem = error.message;
  }
  return (
    <MarketFrame
      title="Launchpad"
      description="Bonding-curve markets on the Launchpad precompile. Quotes come from the curve before your wallet opens."
      account={account}
    >
      {problem === undefined ? null : <MarketUnavailable detail={problem} />}
      {config === undefined ? null : (
        <div className="flex flex-wrap gap-6">
          <LabelValue label="Quote asset" value={config.quoteDenom} />
          <LabelValue label="Markets" value={count.toString()} />
          <LabelValue label="Creation fee" value={`${formatUnits(config.creationFee, LAUNCHPAD_DECIMALS)} ${config.quoteDenom}`} />
          <LabelValue
            label="Swap fee range"
            value={`${config.minFeeBps.toString()}–${config.maxFeeBps.toString()} bps (base ${config.baseFeeBps.toString()})`}
          />
        </div>
      )}
      <ExplorerPanel title="Markets">
        {markets.length === 0 ? (
          <StateEmpty title="No markets yet" description="Create the first market below." />
        ) : (
          <ExplorerTable
            caption="Launchpad markets"
            columns={["Market", "Token", "Price", "Supply", "Reserve", "Volume", "Fee strategy", "State"]}
            rows={markets.map((market) => ({
              id: market.token,
              cells: [
                `${market.name} (${market.symbol})`,
                shortId(market.token),
                formatUnits(market.price, LAUNCHPAD_DECIMALS),
                formatUnits(market.totalSupply, LAUNCHPAD_DECIMALS),
                formatUnits(market.tokenReserve, LAUNCHPAD_DECIMALS),
                formatUnits(market.cumulativeVolume, LAUNCHPAD_DECIMALS),
                FEE_STRATEGIES[Number(market.feeStrategy)] ?? market.feeStrategy.toString(),
                <Badge key="state" variant={market.paused ? "warning" : "success"}>
                  {market.paused ? "Paused" : "Trading"}
                </Badge>,
              ],
            }))}
          />
        )}
      </ExplorerPanel>
      <LaunchpadActions
        account={account}
        quoteDenom={config?.quoteDenom ?? "quote"}
        markets={markets.map((market) => ({
          token: market.token,
          label: `${market.name} (${market.symbol})`,
          paused: market.paused,
        }))}
        feeStrategies={FEE_STRATEGIES}
      />
      <ActivityFeed
        route="/launchpad"
        account={account}
        side={activitySide(firstParam(parameters.side))}
        cursor={firstParam(parameters.cursor)}
        kind={firstParam(parameters.kind)}
      />
    </MarketFrame>
  );
}
