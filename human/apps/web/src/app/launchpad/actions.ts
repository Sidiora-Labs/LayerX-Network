"use server";

import { GatewayRpcError, GatewayUnavailableError } from "../../api/gateway";
import { launchpadQuote } from "../../api/precompiles";

export type LaunchpadQuoteAnswer =
  | Readonly<{ ok: true; amountOut: string; feeBps: string; feeAmount: string }>
  | Readonly<{ ok: false; detail: string }>;

const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const DECIMAL = /^[1-9][0-9]{0,77}$/u;

function isLaunchpadSide(side: string): side is "buy" | "sell" {
  return side === "buy" || side === "sell";
}

/** The curve's quote for a buy or sell, read server side through the gateway before the wallet opens. */
export async function quoteLaunchpadSwap(token: string, side: string, amountIn: string): Promise<LaunchpadQuoteAnswer> {
  if (!EVM_ADDRESS.test(token) || !isLaunchpadSide(side) || !DECIMAL.test(amountIn)) {
    return { ok: false, detail: "Choose a market and a positive amount." };
  }
  try {
    const quote = await launchpadQuote(token, side, BigInt(amountIn));
    return {
      ok: true,
      amountOut: quote.amountOut.toString(),
      feeBps: quote.feeBps.toString(),
      feeAmount: quote.feeAmount.toString(),
    };
  } catch (error) {
    if (error instanceof GatewayUnavailableError || error instanceof GatewayRpcError) {
      return { ok: false, detail: error.message };
    }
    throw error;
  }
}
