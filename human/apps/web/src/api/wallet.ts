import { windowWalletProvider } from "../journeys/custody/handoff.ts";
import { copyEntry } from "../../copy/runtime.ts";
import { parseFeeAmount } from "../auth/native-fee-budget.ts";
import {
  SIDIORA_DECIMALS, SIDIORA_TOKEN, encodeAbiCall, eip7702AuthorizationDigest,
  requestSidioraGasQuote, sendSponsoredBatch, sponsoredBatchDigest,
  sendPrecompileCall, type PrecompileCall, type SidioraGasQuote,
} from "./sdk.ts";

export type PrecompileSendOutcome =
  | Readonly<{ outcome: "sent"; transactionHash: string }>
  | Readonly<{ outcome: "cancelled" | "rejected" | "unavailable" | "failed"; detail?: string }>;

const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const USER_REJECTED_REQUEST = 4001;
const UNAUTHORIZED_REQUEST = 4100;
const PROVIDER_DISCONNECTED = 4900;
const CHAIN_DISCONNECTED = 4901;

export function walletSendFailure(error: unknown): PrecompileSendOutcome {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : undefined;
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
    const preference = walletFeePreference();
    if (preference.currency === "sidiora") {
      if (preference.maximum === undefined) return { outcome: "rejected" };
      const call = build();
      const chainId = rpcQuantity(await wallet.request({ method: "eth_chainId" }));
      const code = await wallet.request({ method: "eth_getCode", params: [from, "latest"] });
      const nonce = code === "0x" ? 0n : rpcQuantity(await wallet.request({
        method: "eth_call", params: [{ to: from, data: encodeAbiCall("nonce", [], []) }, "latest"],
      }));
      const gas = rpcQuantity(await wallet.request({ method: "eth_estimateGas", params: [{
        from, to: call.to, data: call.data, value: `0x${call.value.toString(16)}`,
      }] }));
      const price = rpcQuantity(await wallet.request({ method: "eth_gasPrice" }));
      const quoted = await requestSidioraGasQuote({ account: from, nonce, calls: [call], maxTokenAmount: preference.maximum, gasCost: gas * price }, chainId);
      if (!quoted.ok) return quoted.failure;
      const review = sidioraQuotePresentation(quoted.value);
      if (review === undefined) return { outcome: "rejected" };
      const consent = typeof window !== "undefined" && window.confirm(review.consent);
      return sendWalletSponsoredBatch(quoted.value, consent ? review.identity : undefined);
    }
    const transactionHash = await sendPrecompileCall(wallet, from, build());
    return { outcome: "sent", transactionHash };
  } catch (error) {
    return walletSendFailure(error);
  }
}


export type WalletFeePreference = Readonly<{ currency: "paxeer" | "sidiora"; maximum?: bigint }>;
const FEE_PREFERENCE = "paxeer.wallet.fee";
let feePreference: WalletFeePreference = { currency: "paxeer" };

export function walletFeePreference(): WalletFeePreference {
  if (typeof window === "undefined") return feePreference;
  try {
    const stored = window.sessionStorage.getItem(FEE_PREFERENCE);
    if (stored === null) return feePreference;
    if (stored === "paxeer") return { currency: "paxeer" };
    const maximum = parseFeeAmount(stored, SIDIORA_DECIMALS);
    return maximum === undefined ? { currency: "sidiora" } : { currency: "sidiora", maximum };
  } catch {
    return feePreference;
  }
}

export function chooseWalletFee(currency: WalletFeePreference["currency"], maximum: string): boolean {
  const parsed = parseFeeAmount(maximum, SIDIORA_DECIMALS);
  feePreference = currency === "paxeer" ? { currency } : parsed === undefined ? { currency } : { currency, maximum: parsed };
  if (typeof window !== "undefined") {
    try { window.sessionStorage.setItem(FEE_PREFERENCE, currency === "paxeer" ? currency : maximum); } catch { return currency === "paxeer" || parsed !== undefined; }
  }
  return currency === "paxeer" || parsed !== undefined;
}

export function sidioraAmount(amount: bigint): string {
  const scale = 10n ** BigInt(SIDIORA_DECIMALS);
  return `${amount / scale}.${(amount % scale).toString().padStart(SIDIORA_DECIMALS, "0")}`;
}

export function sidioraQuotePresentation(quoted: SidioraGasQuote, now = BigInt(Math.floor(Date.now() / 1000))) {
  const quote = quoted.batch.quote;
  const digest = sponsoredBatchDigest(quoted.batch);
  if (!digest.ok || quote.token.toLowerCase() !== SIDIORA_TOKEN.toLowerCase()
    || quote.decimals !== SIDIORA_DECIMALS || quote.tokenAmount <= 0n
    || quote.tokenAmount > quote.maxTokenAmount || quote.deadline < now
    || quote.deadline > 8_640_000_000_000n) return undefined;
  const amount = sidioraAmount(quote.tokenAmount);
  const maximum = sidioraAmount(quote.maxTokenAmount);
  const deadline = new Date(Number(quote.deadline) * 1000).toUTCString();
  const consent = copyEntry("gas.sidiora.consent").message
    .replace("{amount}", amount).replace("{maximum}", maximum).replace("{deadline}", deadline);
  return { amount, maximum, deadline, consent, identity: `${digest.value}:${quoted.paymaster.toLowerCase()}` };
}

function rpcQuantity(value: unknown): bigint {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]+$/u.test(value)) throw new Error(copyEntry("gas.sidiora.failed").message);
  return BigInt(value);
}

export async function sendWalletSponsoredBatch(quoted: SidioraGasQuote, consentIdentity: string | undefined): Promise<PrecompileSendOutcome> {
  if (consentIdentity === undefined) return { outcome: "cancelled" };
  const snapshot = structuredClone(quoted);
  const review = sidioraQuotePresentation(snapshot);
  if (review === undefined || review.identity !== consentIdentity) return { outcome: "rejected" };
  const wallet = windowWalletProvider();
  if (wallet === undefined) return { outcome: "unavailable" };
  try {
    const chainId = rpcQuantity(await wallet.request({ method: "eth_chainId" }));
    if (chainId !== snapshot.batch.chainId) return { outcome: "rejected" };
    const nonce = rpcQuantity(await wallet.request({ method: "eth_getTransactionCount", params: [snapshot.batch.account, "pending"] }));
    const batchDigest = sponsoredBatchDigest(snapshot.batch);
    const authorizationDigest = eip7702AuthorizationDigest({ chainId, address: snapshot.paymaster, nonce });
    if (!batchDigest.ok || !authorizationDigest.ok) return { outcome: "rejected" };
    const accountSignature = await wallet.request({ method: "eth_sign", params: [snapshot.batch.account, batchDigest.value] });
    if (typeof accountSignature !== "string") return { outcome: "failed" };
    if (sidioraQuotePresentation(snapshot) === undefined) return { outcome: "rejected" };
    const authorizationSignature = await wallet.request({ method: "eth_sign", params: [snapshot.batch.account, authorizationDigest.value] });
    if (typeof authorizationSignature !== "string") return { outcome: "failed" };
    return await sendSponsoredBatch(snapshot, accountSignature, nonce, authorizationSignature);
  } catch (error) {
    return walletSendFailure(error);
  }
}
