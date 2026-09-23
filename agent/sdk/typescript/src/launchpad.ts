/** Calldata builders, wallet send helpers and event decoders for the Launchpad precompile. */

import type { Eip1193Requester } from "./account-derivation.js";
import {
  decodeEventFrom,
  encodeAbiCall,
  sendPrecompileCall,
  type DecodedPrecompileEvent,
  type PrecompileAbiType,
  type PrecompileAbiValue,
  type PrecompileCall,
  type PrecompileEventInput,
  type PrecompileEventSpec,
  type PrecompileLog,
} from "./exchange.js";

export const LAUNCHPAD_PRECOMPILE = "0x0000000000000000000000000000000000001017";

function event(name: string, inputs: readonly PrecompileEventInput[]): PrecompileEventSpec {
  return { name, precompile: LAUNCHPAD_PRECOMPILE, inputs };
}

function field(name: string, type: PrecompileAbiType, indexed = false): PrecompileEventInput {
  return { name, type, indexed };
}

export const LAUNCHPAD_EVENTS: readonly PrecompileEventSpec[] = [
  event("AirdropClaimed", [field("token", "address", true), field("holder", "address", true), field("amount", "uint256"), field("epoch", "uint256")]),
  event("AirdropExecuted", [field("token", "address", true), field("amount", "uint256"), field("epoch", "uint256")]),
  event("FeeRecorded", [field("token", "address", true), field("feeAmount", "uint256"), field("protocolCut", "uint256"), field("poolCut", "uint256")]),
  event("FeeStrategyChanged", [field("token", "address", true), field("oldStrategy", "uint8"), field("newStrategy", "uint8")]),
  event("FeesBurned", [field("token", "address", true), field("amount", "uint256")]),
  event("FeesClaimed", [field("token", "address", true), field("recipient", "address", true), field("amount", "uint256")]),
  event("LpRewardsExecuted", [field("token", "address", true), field("amount", "uint256")]),
  event("MarketCreated", [
    field("token", "address", true),
    field("creator", "address", true),
    field("denom", "string"),
    field("name", "string"),
    field("symbol", "string"),
    field("feeStrategy", "uint8"),
  ]),
  event("PauseToggled", [field("token", "address", true), field("paused", "bool")]),
  event("Swap", [
    field("token", "address", true),
    field("trader", "address", true),
    field("recipient", "address", true),
    field("isBuy", "bool"),
    field("amountIn", "uint256"),
    field("amountOut", "uint256"),
    field("feeAmount", "uint256"),
    field("price", "uint256"),
  ]),
];

/** Decodes a Launchpad precompile log. */
export function decodeLaunchpadEvent(log: PrecompileLog): DecodedPrecompileEvent {
  return decodeEventFrom(LAUNCHPAD_EVENTS, log);
}

function launchpadCall(name: string, types: readonly PrecompileAbiType[], values: readonly PrecompileAbiValue[]): PrecompileCall {
  return { to: LAUNCHPAD_PRECOMPILE, data: encodeAbiCall(name, types, values), value: 0n };
}

export interface LaunchpadSwapOrder {
  readonly token: string;
  readonly amountIn: bigint;
  readonly minOut: bigint;
  readonly recipient: string;
  readonly deadline: bigint;
}

const SWAP_TYPES: readonly PrecompileAbiType[] = ["address", "uint256", "uint256", "address", "uint256"];

function swapValues(order: LaunchpadSwapOrder): readonly PrecompileAbiValue[] {
  return [order.token, order.amountIn, order.minOut, order.recipient, order.deadline];
}

/** `buy(address,uint256,uint256,address,uint256)`; `amountIn` is the quote paid in. */
export function launchpadBuyCall(order: LaunchpadSwapOrder): PrecompileCall {
  return launchpadCall("buy", SWAP_TYPES, swapValues(order));
}

/** `sell(address,uint256,uint256,address,uint256)`. */
export function launchpadSellCall(order: LaunchpadSwapOrder): PrecompileCall {
  return launchpadCall("sell", SWAP_TYPES, swapValues(order));
}

/** `createMarket(string,string,uint8)`. */
export function launchpadCreateMarketCall(name: string, symbol: string, feeStrategy: number): PrecompileCall {
  return launchpadCall("createMarket", ["string", "string", "uint8"], [name, symbol, BigInt(feeStrategy)]);
}

/** `setFeeStrategy(address,uint8)`. */
export function launchpadSetFeeStrategyCall(token: string, feeStrategy: number): PrecompileCall {
  return launchpadCall("setFeeStrategy", ["address", "uint8"], [token, BigInt(feeStrategy)]);
}

/** `claimFees(address,address)`. */
export function launchpadClaimFeesCall(token: string, recipient: string): PrecompileCall {
  return launchpadCall("claimFees", ["address", "address"], [token, recipient]);
}

export type LaunchpadTokenWrite = "claimAirdrop" | "executeAirdrop" | "executeBurn" | "executeLpRewards" | "pause" | "unpause";

/** `claimAirdrop`, `executeAirdrop`, `executeBurn`, `executeLpRewards`, `pause` or `unpause` on `(address)`. */
export function launchpadTokenCall(write: LaunchpadTokenWrite, token: string): PrecompileCall {
  return launchpadCall(write, ["address"], [token]);
}

export const sendLaunchpadBuy = (wallet: Eip1193Requester, from: string, order: LaunchpadSwapOrder): Promise<string> =>
  sendPrecompileCall(wallet, from, launchpadBuyCall(order));

export const sendLaunchpadSell = (wallet: Eip1193Requester, from: string, order: LaunchpadSwapOrder): Promise<string> =>
  sendPrecompileCall(wallet, from, launchpadSellCall(order));

export const sendLaunchpadCreateMarket = (
  wallet: Eip1193Requester,
  from: string,
  name: string,
  symbol: string,
  feeStrategy: number,
): Promise<string> => sendPrecompileCall(wallet, from, launchpadCreateMarketCall(name, symbol, feeStrategy));

export const sendLaunchpadSetFeeStrategy = (
  wallet: Eip1193Requester,
  from: string,
  token: string,
  feeStrategy: number,
): Promise<string> => sendPrecompileCall(wallet, from, launchpadSetFeeStrategyCall(token, feeStrategy));

export const sendLaunchpadClaimFees = (
  wallet: Eip1193Requester,
  from: string,
  token: string,
  recipient: string,
): Promise<string> => sendPrecompileCall(wallet, from, launchpadClaimFeesCall(token, recipient));

export const sendLaunchpadTokenWrite = (
  wallet: Eip1193Requester,
  from: string,
  write: LaunchpadTokenWrite,
  token: string,
): Promise<string> => sendPrecompileCall(wallet, from, launchpadTokenCall(write, token));
