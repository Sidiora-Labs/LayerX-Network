"use server";

import {
  SIDIORA_DECIMALS,
  SIDIORA_TOKEN,
  assembleEip7702Authorization,
  requestGasQuote,
  sponsoredBatchCall,
  type GasQuoteRequest,
  type GasStationConfig,
  type SponsoredBatch,
} from "../../../../../agent/sdk/typescript/src/gas-station.ts";
import type { PrecompileSendOutcome } from "./wallet.ts";

export interface SidioraGasQuote {
  readonly batch: SponsoredBatch;
  readonly relayerSignature: string;
  readonly paymaster: string;
}

export type GasQuoteOutcome =
  | Readonly<{ ok: true; value: SidioraGasQuote }>
  | Readonly<{ ok: false; failure: PrecompileSendOutcome }>;

function stationConfiguration(): GasStationConfig | undefined {
  const quoteUrl = process.env.PAXEER_GAS_STATION_QUOTE_URL;
  const chainId = process.env.PAXEER_GAS_STATION_CHAIN_ID;
  const sponsor = process.env.PAXEER_GAS_STATION_SPONSOR;
  const paymaster = process.env.PAXEER_GAS_STATION_PAYMASTER;
  if (!quoteUrl || !chainId || !sponsor || !paymaster || !/^[1-9][0-9]*$/u.test(chainId)) return undefined;
  try {
    const endpoint = new URL(quoteUrl);
    if (endpoint.protocol !== "https:" || endpoint.username || endpoint.password || endpoint.hash) return undefined;
    return { quoteUrl: endpoint.href, chainId: BigInt(chainId), sponsor, paymaster, token: SIDIORA_TOKEN, decimals: SIDIORA_DECIMALS };
  } catch {
    return undefined;
  }
}

export async function requestSidioraGasQuote(request: GasQuoteRequest, chainId: bigint): Promise<GasQuoteOutcome> {
  const config = stationConfiguration();
  if (config === undefined) return { ok: false, failure: { outcome: "unavailable" } };
  if (chainId !== config.chainId) return { ok: false, failure: { outcome: "rejected" } };
  const result = await requestGasQuote(config, request);
  if (!result.ok) {
    const code = result.refusal.code;
    return { ok: false, failure: { outcome: code === "unavailable" || code === "cancelled" ? code : "rejected" } };
  }
  return {
    ok: true,
    value: {
      batch: { chainId, account: request.account, nonce: request.nonce, calls: request.calls, quote: result.value.quote },
      relayerSignature: result.value.relayerSignature,
      paymaster: config.paymaster,
    },
  };
}

export async function sendSponsoredBatch(
  quoted: SidioraGasQuote,
  accountSignature: string,
  authorizationNonce: bigint,
  authorizationSignature: string,
): Promise<PrecompileSendOutcome> {
  const config = stationConfiguration();
  const submitUrl = process.env.PAXEER_GAS_STATION_SUBMIT_URL;
  if (config === undefined || submitUrl === undefined) return { outcome: "unavailable" };
  if (quoted.paymaster.toLowerCase() !== config.paymaster.toLowerCase()) return { outcome: "rejected" };
  const call = sponsoredBatchCall(config, quoted.batch, accountSignature, quoted.relayerSignature);
  if (!call.ok) return { outcome: "rejected" };
  const authorization = assembleEip7702Authorization(config, quoted.batch.account, authorizationNonce, authorizationSignature);
  if (!authorization.ok) return { outcome: "rejected" };
  let endpoint: URL;
  try {
    endpoint = new URL(submitUrl);
    if (endpoint.protocol !== "https:" || endpoint.username || endpoint.password || endpoint.hash) return { outcome: "unavailable" };
  } catch {
    return { outcome: "unavailable" };
  }
  let response: Response;
  try {
    response = await fetch(endpoint, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ call: call.value, authorization: authorization.value, batch: quoted.batch, accountSignature, relayerSignature: quoted.relayerSignature },
        (_key, value: unknown) => typeof value === "bigint" ? value.toString() : value),
      signal: AbortSignal.timeout(15_000),
      redirect: "error",
    });
  } catch {
    return { outcome: "failed" };
  }
  if (!response.ok) return { outcome: response.status >= 500 ? "failed" : "rejected" };
  try {
    const body: unknown = await response.json();
    if (typeof body === "object" && body !== null && "transactionHash" in body
      && typeof body.transactionHash === "string" && /^0x[0-9a-fA-F]{64}$/u.test(body.transactionHash)) {
      return { outcome: "sent", transactionHash: body.transactionHash };
    }
  } catch {
    return { outcome: "failed" };
  }
  return { outcome: "failed" };
}
