import { secp256k1 } from "@noble/curves/secp256k1.js";
import { keccak_256 } from "@noble/hashes/sha3.js";

import { abiEventTopic, abiSelector, encodeAbiCall, type PrecompileCall } from "./exchange.js";

export const SIDIORA_TOKEN = "0x21f7b20a555199fa73A238B1a91FD0f549068fEe";
export const SIDIORA_DECIMALS = 6;

export interface GasQuote {
  readonly sponsor: string;
  readonly token: string;
  readonly maxTokenAmount: bigint;
  readonly tokenAmount: bigint;
  readonly deadline: bigint;
  readonly quoteNonce: bigint;
  readonly gasCost: bigint;
  readonly decimals: number;
}

export interface GasStationConfig {
  readonly quoteUrl: string;
  readonly chainId: bigint;
  readonly sponsor: string;
  readonly token: string;
  readonly decimals: number;
  readonly paymaster: string;
}

export interface SponsoredBatch {
  readonly chainId: bigint;
  readonly account: string;
  readonly nonce: bigint;
  readonly calls: readonly PrecompileCall[];
  readonly quote: GasQuote;
}

export interface SignedGasQuote {
  readonly quote: GasQuote;
  readonly relayerSignature: string;
}

export interface GasQuoteRequest {
  readonly account: string;
  readonly nonce: bigint;
  readonly calls: readonly PrecompileCall[];
  readonly maxTokenAmount: bigint;
  readonly gasCost: bigint;
}

export type GasRefusalCode =
  | "expired_quote"
  | "above_maximum"
  | "sponsor_mismatch"
  | "token_mismatch"
  | "decimals_mismatch"
  | "chain_mismatch"
  | "invalid_value"
  | "invalid_signature"
  | "invalid_response"
  | "unavailable"
  | "refused"
  | "cancelled";

export interface GasRefusal {
  readonly code: GasRefusalCode;
  readonly field: string;
}

export type GasResult<T> =
  | { readonly ok: true; readonly value: T }
  | { readonly ok: false; readonly refusal: GasRefusal };

export interface Eip7702Authorization {
  readonly chainId: bigint;
  readonly address: string;
  readonly nonce: bigint;
}

export interface SignedEip7702Authorization extends Eip7702Authorization {
  readonly yParity: number;
  readonly r: string;
  readonly s: string;
}

class Refusal extends Error {
  constructor(readonly code: GasRefusalCode, readonly field: string) {
    super(code);
  }
}

function result<T>(build: () => T): GasResult<T> {
  try {
    return { ok: true, value: build() };
  } catch (error) {
    if (error instanceof Refusal) return { ok: false, refusal: { code: error.code, field: error.field } };
    throw error;
  }
}

function uint(value: bigint, field: string, bits = 256): bigint {
  if (typeof value !== "bigint" || value < 0n || value >= 1n << BigInt(bits)) {
    throw new Refusal("invalid_value", field);
  }
  return value;
}

function address(value: string, field: string): string {
  if (typeof value !== "string" || !/^0x[0-9a-fA-F]{40}$/u.test(value)) {
    throw new Refusal("invalid_value", field);
  }
  return value.toLowerCase();
}

function bytes(value: string, field: string): string {
  if (typeof value !== "string" || !/^0x(?:[0-9a-fA-F]{2})*$/u.test(value)) {
    throw new Refusal("invalid_value", field);
  }
  return value.slice(2).toLowerCase();
}

function unhex(body: string): Uint8Array {
  return Uint8Array.from(body.match(/../gu) ?? [], (byte) => Number.parseInt(byte, 16));
}

function hex(data: Uint8Array): string {
  return Array.from(data, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function hash(body: string): string {
  return `0x${hex(keccak_256(unhex(body)))}`;
}

function word(value: bigint): string {
  return uint(value, "word").toString(16).padStart(64, "0");
}

function dynamicBytes(body: string): string {
  return word(BigInt(body.length / 2)) + body.padEnd(Math.ceil(body.length / 64) * 64, "0");
}

function callsBody(calls: readonly PrecompileCall[]): string {
  const tuples = calls.map((call) =>
    address(call.to, "calls.to").slice(2).padStart(64, "0") +
    word(uint(call.value, "calls.value")) + word(96n) + dynamicBytes(bytes(call.data, "calls.data")),
  );
  let offset = tuples.length * 32;
  const offsets = tuples.map((tuple) => {
    const head = word(BigInt(offset));
    offset += tuple.length / 2;
    return head;
  });
  return word(BigInt(calls.length)) + offsets.join("") + tuples.join("");
}

function quoteBody(quote: GasQuote): string {
  return encodeAbiCall(
    "quote",
    ["address", "address", "uint256", "uint256", "uint256", "uint256", "uint256"],
    [
      address(quote.sponsor, "sponsor"), address(quote.token, "token"),
      uint(quote.maxTokenAmount, "maxTokenAmount"), uint(quote.tokenAmount, "tokenAmount"),
      uint(quote.deadline, "deadline"), uint(quote.quoteNonce, "quoteNonce"), uint(quote.gasCost, "gasCost"),
    ],
  ).slice(10);
}

function signedMessageHash(body: string): string {
  return hash(hex(new TextEncoder().encode("\x19Ethereum Signed Message:\n32")) + hash(body).slice(2));
}

function quoteHash(chainId: bigint, account: string, quote: GasQuote): string {
  return signedMessageHash(
    abiEventTopic("Quote(uint256 chainId,address account,address sponsor,address token,uint256 maxTokenAmount,uint256 tokenAmount,uint256 deadline,uint256 quoteNonce,uint256 gasCost)").slice(2) +
    word(uint(chainId, "chainId")) + address(account, "account").slice(2).padStart(64, "0") + quoteBody(quote),
  );
}

export function gasQuoteDigest(chainId: bigint, account: string, quote: GasQuote): GasResult<string> {
  return result(() => quoteHash(chainId, account, quote));
}

function batchHash(batch: SponsoredBatch): string {
  return signedMessageHash(
    abiEventTopic("SponsoredBatch(uint256 nonce,bytes32 callsHash,bytes32 quoteDigest)").slice(2) +
    word(uint(batch.nonce, "nonce")) + hash(word(32n) + callsBody(batch.calls)).slice(2) +
    quoteHash(batch.chainId, batch.account, batch.quote).slice(2),
  );
}

export function sponsoredBatchDigest(batch: SponsoredBatch): GasResult<string> {
  return result(() => batchHash(batch));
}

function validateConfig(config: GasStationConfig): void {
  if (uint(config.chainId, "chainId") === 0n) throw new Refusal("invalid_value", "chainId");
  if (BigInt(address(config.sponsor, "sponsor")) === 0n) throw new Refusal("invalid_value", "sponsor");
  if (BigInt(address(config.paymaster, "paymaster")) === 0n) throw new Refusal("invalid_value", "paymaster");
  if (address(config.token, "token") !== SIDIORA_TOKEN.toLowerCase()) throw new Refusal("token_mismatch", "token");
  if (config.decimals !== SIDIORA_DECIMALS) throw new Refusal("decimals_mismatch", "decimals");
}

function validateQuote(config: GasStationConfig, quote: GasQuote, now: bigint): void {
  validateConfig(config);
  quoteBody(quote);
  if (quote.decimals !== SIDIORA_DECIMALS) throw new Refusal("decimals_mismatch", "decimals");
  if (address(quote.sponsor, "sponsor") !== config.sponsor.toLowerCase()) throw new Refusal("sponsor_mismatch", "sponsor");
  if (address(quote.token, "token") !== config.token.toLowerCase()) throw new Refusal("token_mismatch", "token");
  if (uint(now, "now") > quote.deadline) throw new Refusal("expired_quote", "deadline");
  if (quote.tokenAmount > quote.maxTokenAmount) throw new Refusal("above_maximum", "maxTokenAmount");
  if (quote.tokenAmount === 0n) throw new Refusal("invalid_value", "tokenAmount");
  if (quote.gasCost === 0n) throw new Refusal("invalid_value", "gasCost");
}

function verifySignature(signature: string, digest: string, signer: string): string {
  const body = bytes(signature, "signature");
  if (body.length !== 130 || !["1b", "1c"].includes(body.slice(128))) {
    throw new Refusal("invalid_signature", "signature");
  }
  try {
    const parsed = secp256k1.Signature.fromBytes(unhex(body.slice(0, 128)), "compact");
    if (parsed.hasHighS()) throw new Refusal("invalid_signature", "signature");
    const publicKey = parsed.addRecoveryBit(Number.parseInt(body.slice(128), 16) - 27)
      .recoverPublicKey(unhex(digest.slice(2))).toBytes(false);
    if (hash(hex(publicKey.slice(1))).slice(-40) !== address(signer, "signer").slice(2)) {
      throw new Refusal("invalid_signature", "signature");
    }
  } catch {
    throw new Refusal("invalid_signature", "signature");
  }
  return body;
}

export function sponsoredBatchCall(
  config: GasStationConfig,
  batch: SponsoredBatch,
  accountSignature: string,
  relayerSignature: string,
  now: bigint = BigInt(Math.floor(Date.now() / 1000)),
): GasResult<PrecompileCall> {
  return result(() => {
    validateQuote(config, batch.quote, now);
    if (batch.chainId !== config.chainId) throw new Refusal("chain_mismatch", "chainId");
    if (address(batch.account, "account") === address(batch.quote.sponsor, "sponsor")) {
      throw new Refusal("invalid_value", "sponsor");
    }
    const calls = callsBody(batch.calls);
    const account = dynamicBytes(verifySignature(accountSignature, batchHash(batch), batch.account));
    const relayer = dynamicBytes(verifySignature(relayerSignature, quoteHash(batch.chainId, batch.account, batch.quote), batch.quote.sponsor));
    const headSize = 320n;
    const data = abiSelector("executeSponsored((address,uint256,bytes)[],(address,address,uint256,uint256,uint256,uint256,uint256),bytes,bytes)") +
      word(headSize) + quoteBody(batch.quote) + word(headSize + BigInt(calls.length / 2)) +
      word(headSize + BigInt((calls.length + account.length) / 2)) + calls + account + relayer;
    return { to: batch.account, value: 0n, data };
  });
}

function rlpBytes(body: string): string {
  const length = body.length / 2;
  if (length === 1 && Number.parseInt(body, 16) < 128) return body;
  if (length <= 55) return (128 + length).toString(16) + body;
  const size = integerHex(BigInt(length));
  return (183 + size.length / 2).toString(16) + size + body;
}

function integerHex(value: bigint): string {
  if (value === 0n) return "";
  const body = value.toString(16);
  return body.padStart(Math.ceil(body.length / 2) * 2, "0");
}

function authorizationHash(authorization: Eip7702Authorization): string {
  const nonce = uint(authorization.nonce, "authorization.nonce", 64);
  if (nonce === (1n << 64n) - 1n) throw new Refusal("invalid_value", "authorization.nonce");
  const body = rlpBytes(integerHex(uint(authorization.chainId, "authorization.chainId"))) +
    rlpBytes(address(authorization.address, "authorization.address").slice(2)) + rlpBytes(integerHex(nonce));
  const length = body.length / 2;
  const size = integerHex(BigInt(length));
  const list = length <= 55 ? (192 + length).toString(16) : (247 + size.length / 2).toString(16) + size;
  return hash("05" + list + body);
}

export function eip7702AuthorizationDigest(authorization: Eip7702Authorization): GasResult<string> {
  return result(() => authorizationHash(authorization));
}

export function assembleEip7702Authorization(
  config: GasStationConfig,
  account: string,
  nonce: bigint,
  signature: string,
): GasResult<SignedEip7702Authorization> {
  return result(() => {
    validateConfig(config);
    const authorization = { chainId: config.chainId, address: config.paymaster, nonce };
    const body = verifySignature(signature, authorizationHash(authorization), account);
    return { ...authorization, yParity: Number.parseInt(body.slice(128), 16) - 27, r: `0x${body.slice(0, 64)}`, s: `0x${body.slice(64, 128)}` };
  });
}

function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Refusal("invalid_response", "quote");
  return value as Record<string, unknown>;
}

function wireString(value: unknown, field: string): string {
  if (typeof value !== "string") throw new Refusal("invalid_response", field);
  return value;
}

function wireUint(value: unknown, field: string): bigint {
  const text = wireString(value, field);
  if (!/^(0|[1-9][0-9]*)$/u.test(text) || text.length > 78) throw new Refusal("invalid_response", field);
  return uint(BigInt(text), field);
}

export async function requestGasQuote(
  config: GasStationConfig,
  request: GasQuoteRequest,
  options: { readonly signal?: AbortSignal; readonly now?: bigint } = {},
): Promise<GasResult<SignedGasQuote>> {
  const prepared = result(() => {
    validateConfig(config);
    address(request.account, "account");
    uint(request.nonce, "nonce");
    callsBody(request.calls);
    if (uint(request.maxTokenAmount, "maxTokenAmount") === 0n || uint(request.gasCost, "gasCost") === 0n) {
      throw new Refusal("invalid_value", "request");
    }
    let url: URL;
    try { url = new URL(config.quoteUrl); } catch { throw new Refusal("invalid_value", "quoteUrl"); }
    if (!["https:", "http:"].includes(url.protocol) || url.username || url.password || url.hash) {
      throw new Refusal("invalid_value", "quoteUrl");
    }
    return JSON.stringify({ ...request, chainId: config.chainId, token: config.token, decimals: config.decimals },
      (_key, value: unknown) => typeof value === "bigint" ? value.toString() : value);
  });
  if (!prepared.ok) return prepared;
  let response: Response;
  try {
    response = await fetch(config.quoteUrl, {
      method: "POST", headers: { "content-type": "application/json" }, body: prepared.value,
      signal: options.signal ?? AbortSignal.timeout(15_000), redirect: "error",
    });
  } catch {
    return { ok: false, refusal: { code: options.signal?.aborted ? "cancelled" : "unavailable", field: "quoteUrl" } };
  }
  if (!response.ok) return { ok: false, refusal: { code: response.status >= 500 ? "unavailable" : "refused", field: "quoteUrl" } };
  let payload: unknown;
  try { payload = await response.json(); } catch { return { ok: false, refusal: { code: "invalid_response", field: "quote" } }; }
  return result(() => {
    const envelope = record(payload);
    const data = record(envelope.quote);
    if (typeof data.decimals !== "number") throw new Refusal("invalid_response", "decimals");
    const quote: GasQuote = {
      sponsor: wireString(data.sponsor, "sponsor"), token: wireString(data.token, "token"), decimals: data.decimals,
      maxTokenAmount: wireUint(data.maxTokenAmount, "maxTokenAmount"), tokenAmount: wireUint(data.tokenAmount, "tokenAmount"),
      deadline: wireUint(data.deadline, "deadline"), quoteNonce: wireUint(data.quoteNonce, "quoteNonce"), gasCost: wireUint(data.gasCost, "gasCost"),
    };
    validateQuote(config, quote, options.now ?? BigInt(Math.floor(Date.now() / 1000)));
    if (quote.maxTokenAmount > request.maxTokenAmount) throw new Refusal("above_maximum", "maxTokenAmount");
    if (quote.gasCost !== request.gasCost) throw new Refusal("invalid_response", "gasCost");
    const relayerSignature = wireString(envelope.relayerSignature, "relayerSignature");
    verifySignature(relayerSignature, quoteHash(config.chainId, request.account, quote), quote.sponsor);
    return { quote, relayerSignature };
  });
}
