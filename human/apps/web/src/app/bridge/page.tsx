import { Badge } from "@layerx/ui/components/badge";

import { GatewayRpcError, GatewayUnavailableError } from "../../api/gateway";
import {
  PRECOMPILE_LOG_WINDOW,
  bridgeActivity,
  bridgeAttestors,
  bridgeCap,
  bridgeChain,
  bridgeNullified,
  bridgePaused,
  type BridgeAttestors,
  type BridgeCap,
  type BridgeChain,
  type ObservedEvent,
} from "../../api/precompiles";
import { PrecompileAbiError } from "../../api/sdk";
import { ExplorerPanel, ExplorerTable } from "../../kit/explorer";
import { LabelValue } from "../../kit/money";
import { InlineNotice, StateEmpty } from "../../kit/surface";
import { ActivityFeed, activitySide } from "../_markets/activity-feed";
import { firstParam, isBytes32, isEvmAddress, shortId, type MarketSearchParams } from "../_markets/format";
import { MarketFrame, MarketUnavailable } from "../_markets/frame";
import { BridgeOutForm, BridgeStatusPolling, BridgeTrackForm } from "./bridge-client";

export const dynamic = "force-dynamic";

const CHAIN = /^[1-9][0-9]{0,18}$/u;
const LOG_INDEX = /^(0|[1-9][0-9]{0,18})$/u;
const DEFAULT_CHAIN = "1";

function recipientWord(address: string): string {
  return `0x${"00".repeat(12)}${address.slice(2)}`;
}

function field(entry: ObservedEvent, name: string): string {
  const value = entry.event.fields[name];
  return value === undefined ? "—" : String(value);
}

function isGatewayProblem(error: unknown): error is Error {
  return error instanceof GatewayUnavailableError || error instanceof GatewayRpcError || error instanceof PrecompileAbiError;
}

export default async function BridgePage({ searchParams }: Readonly<{ searchParams: Promise<MarketSearchParams> }>) {
  const parameters = await searchParams;
  const candidate = firstParam(parameters.account)?.toLowerCase();
  const account = isEvmAddress(candidate) ? candidate : undefined;
  const chainText = firstParam(parameters.chain) ?? DEFAULT_CHAIN;
  const chainId = CHAIN.test(chainText) ? BigInt(chainText) : BigInt(DEFAULT_CHAIN);
  const assetCandidate = firstParam(parameters.asset)?.toLowerCase();
  const asset = isEvmAddress(assetCandidate) ? assetCandidate : undefined;
  const trackedTx = firstParam(parameters.tx)?.toLowerCase();
  const trackedLogText = firstParam(parameters.log);
  const tracked =
    isBytes32(trackedTx) && trackedLogText !== undefined && LOG_INDEX.test(trackedLogText)
      ? { txHash: trackedTx, logIndex: BigInt(trackedLogText) }
      : undefined;

  let paused: boolean | undefined;
  let attestors: BridgeAttestors | undefined;
  let chain: BridgeChain | undefined;
  let cap: BridgeCap | undefined;
  let credited: boolean | undefined;
  let activity: readonly ObservedEvent[] = [];
  let problem: string | undefined;
  try {
    [paused, attestors, chain] = await Promise.all([bridgePaused(), bridgeAttestors(), bridgeChain(chainId)]);
    [cap, credited, activity] = await Promise.all([
      asset === undefined ? Promise.resolve(undefined) : bridgeCap(chainId, asset),
      tracked === undefined ? Promise.resolve(undefined) : bridgeNullified(chainId, tracked.txHash, tracked.logIndex),
      account === undefined ? Promise.resolve([]) : bridgeActivity(account),
    ]);
  } catch (error) {
    if (!isGatewayProblem(error)) {
      throw error;
    }
    problem = error.message;
  }

  const capsZero = cap !== undefined && (cap.maxInFlight === 0n || cap.maxPerTx === 0n);
  const chainOpen = chain !== undefined && chain.registered && chain.enabled && paused === false;
  const closedReason =
    paused === true
      ? "The bridge is paused."
      : chain === undefined || !chain.registered
        ? `Chain ${chainId.toString()} is not registered with the bridge.`
        : !chain.enabled
          ? `Chain ${chainId.toString()} is registered but disabled.`
          : capsZero
            ? "Caps for this asset are zero: bridging is closed for it."
            : undefined;

  return (
    <MarketFrame
      title="Bridge"
      description="Move assets between Ethereum and Paxeer through the attested LayerXBridge precompile."
      account={account}
    >
      {problem === undefined ? null : <MarketUnavailable detail={problem} />}
      <ExplorerPanel title="Bridge status">
        <form method="get" action="/bridge" className="flex flex-wrap items-end gap-3">
          {account === undefined ? null : <input type="hidden" name="account" value={account} />}
          <label className="flex flex-col gap-1 text-sm font-semibold">
            Chain id
            <input name="chain" defaultValue={chainId.toString()} inputMode="numeric" className="h-11 rounded-md border border-border-strong bg-surface px-3" />
          </label>
          <label className="flex flex-1 flex-col gap-1 text-sm font-semibold">
            Asset address on that chain
            <input name="asset" defaultValue={asset ?? ""} spellCheck={false} placeholder="0x…" className="h-11 rounded-md border border-border-strong bg-surface px-3" />
          </label>
          <button type="submit" className="h-11 rounded-full border border-border-strong bg-surface px-5 text-sm font-semibold">
            Show
          </button>
        </form>
        <div className="flex flex-wrap gap-6">
          <LabelValue
            label="Bridge"
            value={
              <Badge variant={paused === false ? "success" : "warning"}>
                {paused === undefined ? "Unknown" : paused ? "Paused" : "Running"}
              </Badge>
            }
          />
          <LabelValue
            label={`Chain ${chainId.toString()}`}
            value={
              <Badge variant={chainOpen ? "success" : "warning"}>
                {chain === undefined ? "Unknown" : !chain.registered ? "Not registered" : chain.enabled ? "Enabled" : "Disabled"}
              </Badge>
            }
          />
          {chain?.registered === true ? <LabelValue label="Vault" value={chain.vault} /> : null}
          {chain?.registered === true ? <LabelValue label="Finality depth" value={`${chain.finalityDepth.toString()} blocks`} /> : null}
          {attestors === undefined ? null : (
            <LabelValue
              label="Attestation"
              value={`${attestors.threshold.toString()} of ${String(attestors.signers.length)} attestors`}
            />
          )}
        </div>
        {cap === undefined ? (
          <p className="text-sm text-muted-foreground">Enter an asset address to see its caps.</p>
        ) : (
          <div className="flex flex-col gap-2">
            <div className="flex flex-wrap gap-6">
              <LabelValue label="Bridged denom" value={cap.denom === "" ? "—" : cap.denom} />
              <LabelValue label="Per transfer cap" value={cap.maxPerTx.toString()} />
              <LabelValue label="In-flight cap" value={cap.maxInFlight.toString()} />
              <LabelValue label="In flight now" value={cap.inFlight.toString()} />
            </div>
            {capsZero ? (
              <InlineNotice tone="warning" role="alert">
                Caps are zero for this asset. Governance has not opened it, so bridge in and bridge out are both closed.
              </InlineNotice>
            ) : null}
          </div>
        )}
      </ExplorerPanel>
      <div className="grid gap-4 lg:grid-cols-2">
        <ExplorerPanel title="Bridge in from Ethereum">
          {chain?.registered !== true ? (
            <InlineNotice tone="warning">{closedReason ?? "This chain is not open for bridging."}</InlineNotice>
          ) : (
            <ol className="flex list-decimal flex-col gap-2 pl-5 text-sm">
              <li>
                On chain {chainId.toString()}, deposit into the PaxeerX vault <strong className="break-all">{chain.vault}</strong>.
              </li>
              <li>
                Set the Paxeer recipient to{" "}
                <strong className="break-all">{account === undefined ? "your connected wallet, padded to 32 bytes" : recipientWord(account)}</strong>.
              </li>
              <li>
                Wait {chain.finalityDepth.toString()} Ethereum blocks. Then {attestors?.threshold.toString() ?? "the threshold of"} attestors sign and relay it; the bridged denom is minted to you.
              </li>
              <li>Track the deposit below with its Ethereum transaction hash and the BridgeDeposit log index.</li>
            </ol>
          )}
          {closedReason !== undefined && chain?.registered === true ? (
            <InlineNotice tone="warning">{closedReason}</InlineNotice>
          ) : null}
          <BridgeTrackForm account={account} chain={chainId.toString()} asset={asset} />
          {tracked === undefined || credited === undefined ? null : (
            <div className="flex flex-col gap-2">
              <InlineNotice tone={credited ? "success" : "neutral"}>
                {credited
                  ? `Credited: deposit ${shortId(tracked.txHash)} log ${tracked.logIndex.toString()} was attested and minted on Paxeer.`
                  : `Waiting for attestation: deposit ${shortId(tracked.txHash)} log ${tracked.logIndex.toString()} has not been bridged yet.`}
              </InlineNotice>
              <BridgeStatusPolling active={!credited} />
            </div>
          )}
        </ExplorerPanel>
        <ExplorerPanel title="Bridge out to Ethereum">
          <BridgeOutForm
            account={account}
            chain={chainId.toString()}
            asset={asset}
            closedReason={closedReason ?? (asset === undefined ? "Enter an asset address above to see its caps first." : undefined)}
            maxPerTx={cap?.maxPerTx.toString()}
            headroom={cap === undefined ? undefined : (cap.maxInFlight > cap.inFlight ? cap.maxInFlight - cap.inFlight : 0n).toString()}
          />
        </ExplorerPanel>
      </div>
      <ExplorerPanel title="Attestations and requests">
        {account === undefined || activity.length === 0 ? (
          <StateEmpty
            title="Nothing bridged yet"
            description={`Bridge-ins credited to this wallet and bridge-outs paying it in the last ${PRECOMPILE_LOG_WINDOW.toString()} blocks appear here.`}
          />
        ) : (
          <ExplorerTable
            caption="Bridge events"
            columns={["Event", "Chain", "Asset", "Amount", "Reference", "Block", "Transaction"]}
            rows={activity.map((entry) => ({
              id: `${entry.transactionHash}-${entry.logIndex.toString()}`,
              cells: [
                entry.event.event === "BridgeIn" ? "Bridged in (attested)" : "Bridge-out requested",
                field(entry, "chain"),
                shortId(field(entry, "asset")),
                field(entry, "amount"),
                entry.event.event === "BridgeIn" ? shortId(field(entry, "txHash")) : `nonce ${field(entry, "nonce")}`,
                entry.blockNumber.toString(),
                shortId(entry.transactionHash),
              ],
            }))}
          />
        )}
      </ExplorerPanel>
      <ActivityFeed
        route="/bridge"
        account={account}
        side={activitySide(firstParam(parameters.side))}
        cursor={firstParam(parameters.cursor)}
        kind={firstParam(parameters.kind)}
      />
    </MarketFrame>
  );
}
