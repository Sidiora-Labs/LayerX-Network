import { windowWalletProvider } from "../journeys/custody/handoff.ts";
import { sendPrecompileCall, type PrecompileCall } from "./sdk.ts";

export type PrecompileSendOutcome =
  | Readonly<{ outcome: "sent"; transactionHash: string }>
  | Readonly<{ outcome: "cancelled" | "rejected" | "unavailable" | "failed"; detail?: string }>;

const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const USER_REJECTED_REQUEST = 4001;
const UNAUTHORIZED_REQUEST = 4100;
const PROVIDER_DISCONNECTED = 4900;
const CHAIN_DISCONNECTED = 4901;

function failure(error: unknown): PrecompileSendOutcome {
  const code = (error as Readonly<{ code?: unknown }>).code;
  const detail = error instanceof Error ? error.message : undefined;
  if (code === USER_REJECTED_REQUEST) {
    return { outcome: "cancelled" };
  }
  if (code === UNAUTHORIZED_REQUEST) {
    return { outcome: "rejected" };
  }
  if (code === PROVIDER_DISCONNECTED || code === CHAIN_DISCONNECTED) {
    return { outcome: "unavailable" };
  }
  return detail === undefined ? { outcome: "failed" } : { outcome: "failed", detail };
}

/** Asks the Paxeer wallet for its first account. */
export async function connectWalletAccount(): Promise<string | undefined> {
  const wallet = windowWalletProvider();
  if (wallet === undefined) {
    return undefined;
  }
  const response = await wallet.request({ method: "eth_requestAccounts" });
  const first: unknown = Array.isArray(response) ? (response as readonly unknown[])[0] : undefined;
  return typeof first === "string" && EVM_ADDRESS.test(first) ? first.toLowerCase() : undefined;
}

/** Sends one SDK-built precompile call through the Paxeer wallet. */
export async function sendWalletPrecompileCall(from: string, build: () => PrecompileCall): Promise<PrecompileSendOutcome> {
  const wallet = windowWalletProvider();
  if (wallet === undefined) {
    return { outcome: "unavailable" };
  }
  try {
    const transactionHash = await sendPrecompileCall(wallet, from, build());
    return { outcome: "sent", transactionHash };
  } catch (error) {
    return failure(error);
  }
}
