/**
 * Calldata builders, a wallet send helper, and event decoders for the
 * LayerXExchange precompile, plus the shared EVM ABI machinery the bridge and
 * launchpad precompile modules use.
 */

import { keccak_256 } from "@noble/hashes/sha3.js";

import type { Eip1193Requester, WalletRequest } from "./account-derivation.js";

export const LAYERX_EXCHANGE_PRECOMPILE = "0x0000000000000000000000000000000000001015";

export type PrecompileAbiErrorCode =
  | "invalid_value"
  | "unknown_event"
  | "topic_count"
  | "data_length"
  | "non_canonical_word"
  | "invalid_string"
  | "malformed_wallet_answer";

export class PrecompileAbiError extends Error {
  readonly code: PrecompileAbiErrorCode;

  constructor(code: PrecompileAbiErrorCode, detail?: string) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "PrecompileAbiError";
    this.code = code;
  }
}

export type PrecompileAbiType =
  | "address"
  | "bool"
  | "bytes32"
  | "bytes[]"
  | "string"
  | "uint8"
  | "uint64"
  | "uint256";

export type PrecompileAbiValue = bigint | boolean | string | readonly string[];

export interface PrecompileEventInput {
  readonly name: string;
  readonly type: PrecompileAbiType;
  readonly indexed: boolean;
}

export interface PrecompileEventSpec {
  readonly name: string;
  readonly precompile: string;
  readonly inputs: readonly PrecompileEventInput[];
}

/** A transaction that calls a precompile write. `value` is the attached native wei. */
export interface PrecompileCall {
  readonly to: string;
  readonly data: string;
  readonly value: bigint;
}

/** An EVM log as `eth_getLogs` and receipts return it. */
export interface PrecompileLog {
  readonly address: string;
  readonly topics: readonly string[];
  readonly data: string;
}

export type PrecompileEventFields = Readonly<Record<string, bigint | boolean | string>>;

export interface DecodedPrecompileEvent {
  readonly event: string;
  readonly precompile: string;
  readonly topic0: string;
  readonly fields: PrecompileEventFields;
}

const ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const BYTES32 = /^0x[0-9a-fA-F]{64}$/u;
const HEX = /^0x(?:[0-9a-fA-F]{2})*$/u;
const WORD = 32;

function hex(bytes: Uint8Array): string {
  return `0x${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function unhex(value: string, label: string): Uint8Array {
  if (!HEX.test(value)) {
    throw new PrecompileAbiError("invalid_value", label);
  }
  const body = value.slice(2);
  return Uint8Array.from({ length: body.length / 2 }, (_, index) =>
    Number.parseInt(body.slice(index * 2, index * 2 + 2), 16),
  );
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const out = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function uintWord(value: bigint, bits: number, label: string): Uint8Array {
  if (value < 0n || value >= 1n << BigInt(bits)) {
    throw new PrecompileAbiError("invalid_value", label);
  }
  const out = new Uint8Array(WORD);
  let rest = value;
  for (let index = WORD - 1; index >= 0; index -= 1) {
    out[index] = Number(rest & 0xffn);
    rest >>= 8n;
  }
  return out;
}

function padded(bytes: Uint8Array): Uint8Array {
  const out = new Uint8Array(Math.ceil(bytes.length / WORD) * WORD);
  out.set(bytes);
  return out;
}

function dynamicBytes(bytes: Uint8Array): Uint8Array {
  return concat([uintWord(BigInt(bytes.length), 256, "length"), padded(bytes)]);
}

function bits(type: PrecompileAbiType): number {
  return type === "uint8" ? 8 : type === "uint64" ? 64 : 256;
}

function encodeStatic(type: PrecompileAbiType, value: PrecompileAbiValue, label: string): Uint8Array {
  switch (type) {
    case "address": {
      if (typeof value !== "string" || !ADDRESS.test(value)) {
        throw new PrecompileAbiError("invalid_value", label);
      }
      const out = new Uint8Array(WORD);
      out.set(unhex(value, label), 12);
      return out;
    }
    case "bytes32":
      if (typeof value !== "string" || !BYTES32.test(value)) {
        throw new PrecompileAbiError("invalid_value", label);
      }
      return unhex(value, label);
    case "bool":
      if (typeof value !== "boolean") {
        throw new PrecompileAbiError("invalid_value", label);
      }
      return uintWord(value ? 1n : 0n, 8, label);
    default:
      if (typeof value !== "bigint") {
        throw new PrecompileAbiError("invalid_value", label);
      }
      return uintWord(value, bits(type), label);
  }
}

function encodeDynamic(type: PrecompileAbiType, value: PrecompileAbiValue, label: string): Uint8Array {
  if (type === "string") {
    if (typeof value !== "string") {
      throw new PrecompileAbiError("invalid_value", label);
    }
    return dynamicBytes(new TextEncoder().encode(value));
  }
  if (!Array.isArray(value)) {
    throw new PrecompileAbiError("invalid_value", label);
  }
  const items = (value as readonly string[]).map((item) => dynamicBytes(unhex(item, label)));
  const offsets: Uint8Array[] = [];
  let offset = items.length * WORD;
  for (const item of items) {
    offsets.push(uintWord(BigInt(offset), 256, label));
    offset += item.length;
  }
  return concat([uintWord(BigInt(items.length), 256, label), ...offsets, ...items]);
}

function dynamic(type: PrecompileAbiType): boolean {
  return type === "string" || type === "bytes[]";
}

/** The canonical signature of a function or event. */
export function abiSignature(name: string, types: readonly PrecompileAbiType[]): string {
  return `${name}(${types.join(",")})`;
}

/** The 4-byte function selector as `0x`-prefixed hex. */
export function abiSelector(signature: string): string {
  return hex(keccak_256(new TextEncoder().encode(signature)).subarray(0, 4));
}

/** The event topic0 as `0x`-prefixed hex. */
export function abiEventTopic(signature: string): string {
  return hex(keccak_256(new TextEncoder().encode(signature)));
}

/** ABI-encodes a call to `name(types...)` with `values`. */
export function encodeAbiCall(
  name: string,
  types: readonly PrecompileAbiType[],
  values: readonly PrecompileAbiValue[],
): string {
  if (types.length !== values.length) {
    throw new PrecompileAbiError("invalid_value", "argument count");
  }
  const heads: Uint8Array[] = [];
  const tails: Uint8Array[] = [];
  let tailOffset = types.length * WORD;
  types.forEach((type, index) => {
    const value = values[index];
    const label = `${name} argument ${index}`;
    if (value === undefined) {
      throw new PrecompileAbiError("invalid_value", label);
    }
    if (dynamic(type)) {
      const tail = encodeDynamic(type, value, label);
      heads.push(uintWord(BigInt(tailOffset), 256, label));
      tails.push(tail);
      tailOffset += tail.length;
    } else {
      heads.push(encodeStatic(type, value, label));
    }
  });
  return abiSelector(abiSignature(name, types)) + hex(concat([...heads, ...tails])).slice(2);
}

/** The `eth_sendTransaction` request a browser wallet sends for a precompile write. */
export function precompileTransactionRequest(from: string, call: PrecompileCall): WalletRequest {
  if (!ADDRESS.test(from)) {
    throw new PrecompileAbiError("invalid_value", "from");
  }
  return {
    method: "eth_sendTransaction",
    params: [{ from, to: call.to, data: call.data, value: `0x${call.value.toString(16)}` }],
  };
}

/** Asks the wallet to send a precompile write and returns its transaction hash. */
export async function sendPrecompileCall(wallet: Eip1193Requester, from: string, call: PrecompileCall): Promise<string> {
  const hash = await wallet.request(precompileTransactionRequest(from, call));
  if (typeof hash !== "string" || !BYTES32.test(hash)) {
    throw new PrecompileAbiError("malformed_wallet_answer", "wallet returned no transaction hash");
  }
  return hash;
}

/** The canonical signature of an event spec. */
export function precompileEventSignature(spec: PrecompileEventSpec): string {
  return abiSignature(
    spec.name,
    spec.inputs.map((input) => input.type),
  );
}

function word(data: Uint8Array, offset: number): Uint8Array {
  if (offset + WORD > data.length) {
    throw new PrecompileAbiError("data_length", String(data.length));
  }
  return data.subarray(offset, offset + WORD);
}

function wordInteger(bytes: Uint8Array): bigint {
  return bytes.reduce((value, byte) => (value << 8n) | BigInt(byte), 0n);
}

function decodeWord(type: PrecompileAbiType, bytes: Uint8Array): bigint | boolean | string {
  switch (type) {
    case "address":
      if (bytes.subarray(0, 12).some((byte) => byte !== 0)) {
        throw new PrecompileAbiError("non_canonical_word", type);
      }
      return hex(bytes.subarray(12));
    case "bytes32":
      return hex(bytes);
    case "bool": {
      const value = wordInteger(bytes);
      if (value > 1n) {
        throw new PrecompileAbiError("non_canonical_word", type);
      }
      return value === 1n;
    }
    default: {
      const value = wordInteger(bytes);
      if (value >= 1n << BigInt(bits(type))) {
        throw new PrecompileAbiError("non_canonical_word", type);
      }
      return value;
    }
  }
}

function decodeString(data: Uint8Array, headOffset: number): string {
  const offset = Number(decodeWord("uint64", word(data, headOffset)));
  const length = Number(decodeWord("uint64", word(data, offset)));
  const start = offset + WORD;
  if (start + length > data.length) {
    throw new PrecompileAbiError("data_length", String(data.length));
  }
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(data.subarray(start, start + length));
  } catch {
    throw new PrecompileAbiError("invalid_string");
  }
}

/** Decodes a log against one event spec. */
export function decodeEventWithSpec(spec: PrecompileEventSpec, log: PrecompileLog): DecodedPrecompileEvent {
  const topic0 = abiEventTopic(precompileEventSignature(spec));
  if (log.address.toLowerCase() !== spec.precompile || log.topics[0]?.toLowerCase() !== topic0) {
    throw new PrecompileAbiError("unknown_event", spec.name);
  }
  const indexed = spec.inputs.filter((input) => input.indexed);
  if (log.topics.length !== indexed.length + 1) {
    throw new PrecompileAbiError("topic_count", String(log.topics.length));
  }
  const data = unhex(log.data, "data");
  const fields: Record<string, bigint | boolean | string> = {};
  let topic = 1;
  let head = 0;
  for (const input of spec.inputs) {
    if (input.indexed) {
      const value = log.topics[topic] ?? "";
      if (!BYTES32.test(value)) {
        throw new PrecompileAbiError("non_canonical_word", input.name);
      }
      fields[input.name] = decodeWord(input.type, unhex(value, input.name));
      topic += 1;
    } else {
      fields[input.name] = input.type === "string" ? decodeString(data, head) : decodeWord(input.type, word(data, head));
      head += WORD;
    }
  }
  const hasString = spec.inputs.some((input) => !input.indexed && input.type === "string");
  if (!hasString && data.length !== head) {
    throw new PrecompileAbiError("data_length", String(data.length));
  }
  return { event: spec.name, precompile: spec.precompile, topic0, fields };
}

/** Finds the spec a log belongs to and decodes it. */
export function decodeEventFrom(specs: readonly PrecompileEventSpec[], log: PrecompileLog): DecodedPrecompileEvent {
  const topic0 = log.topics[0]?.toLowerCase();
  const spec = specs.find(
    (candidate) =>
      candidate.precompile === log.address.toLowerCase() &&
      abiEventTopic(precompileEventSignature(candidate)) === topic0,
  );
  if (spec === undefined) {
    throw new PrecompileAbiError("unknown_event", topic0 ?? "no topic");
  }
  return decodeEventWithSpec(spec, log);
}

function input(name: string, type: PrecompileAbiType, indexed = false): PrecompileEventInput {
  return { name, type, indexed };
}

export const EXCHANGE_EVENTS: readonly PrecompileEventSpec[] = [
  {
    name: "MarginDeposited",
    precompile: LAYERX_EXCHANGE_PRECOMPILE,
    inputs: [
      input("intentId", "bytes32", true),
      input("account", "bytes32", true),
      input("owner", "address", true),
      input("assetId", "bytes32"),
      input("amount", "uint256"),
      input("depositId", "bytes32"),
      input("nonce", "uint64"),
    ],
  },
  {
    name: "MarginWithdrawalRequested",
    precompile: LAYERX_EXCHANGE_PRECOMPILE,
    inputs: [
      input("intentId", "bytes32", true),
      input("account", "bytes32", true),
      input("owner", "address", true),
      input("assetId", "bytes32"),
      input("amount", "uint256"),
      input("nonce", "uint64"),
    ],
  },
  {
    name: "OrderCancelRequested",
    precompile: LAYERX_EXCHANGE_PRECOMPILE,
    inputs: [input("intentId", "bytes32", true), input("orderId", "bytes32", true), input("owner", "address", true), input("nonce", "uint64")],
  },
  {
    name: "OrderPlaced",
    precompile: LAYERX_EXCHANGE_PRECOMPILE,
    inputs: [
      input("intentId", "bytes32", true),
      input("marketId", "bytes32", true),
      input("owner", "address", true),
      input("side", "uint8"),
      input("price", "uint256"),
      input("quantity", "uint256"),
      input("timeInForce", "uint8"),
      input("nonce", "uint64"),
    ],
  },
  {
    name: "SettlementRequested",
    precompile: LAYERX_EXCHANGE_PRECOMPILE,
    inputs: [input("intentId", "bytes32", true), input("positionId", "bytes32", true), input("owner", "address", true), input("nonce", "uint64")],
  },
];

/** Decodes a LayerXExchange precompile log. */
export function decodeExchangeEvent(log: PrecompileLog): DecodedPrecompileEvent {
  return decodeEventFrom(EXCHANGE_EVENTS, log);
}

function exchangeCall(name: string, types: readonly PrecompileAbiType[], values: readonly PrecompileAbiValue[], value = 0n): PrecompileCall {
  return { to: LAYERX_EXCHANGE_PRECOMPILE, data: encodeAbiCall(name, types, values), value };
}

/** `placeOrder(bytes32,uint8,uint256,uint256,uint8)`. */
export function exchangePlaceOrderCall(order: {
  readonly marketId: string;
  readonly side: number;
  readonly price: bigint;
  readonly quantity: bigint;
  readonly timeInForce: number;
}): PrecompileCall {
  return exchangeCall(
    "placeOrder",
    ["bytes32", "uint8", "uint256", "uint256", "uint8"],
    [order.marketId, BigInt(order.side), order.price, order.quantity, BigInt(order.timeInForce)],
  );
}

/** `cancelOrder(bytes32)`. */
export function exchangeCancelOrderCall(orderId: string): PrecompileCall {
  return exchangeCall("cancelOrder", ["bytes32"], [orderId]);
}

/** `requestSettlement(bytes32)`. */
export function exchangeRequestSettlementCall(positionId: string): PrecompileCall {
  return exchangeCall("requestSettlement", ["bytes32"], [positionId]);
}

/** `depositMargin(bytes32)`, payable with the native amount in wei. */
export function exchangeDepositMarginCall(account: string, amountWei: bigint): PrecompileCall {
  if (amountWei <= 0n) {
    throw new PrecompileAbiError("invalid_value", "deposit amount");
  }
  return exchangeCall("depositMargin", ["bytes32"], [account], amountWei);
}

/** `depositMarginToken(address,uint256,bytes32)`. */
export function exchangeDepositMarginTokenCall(pointer: string, amount: bigint, account: string): PrecompileCall {
  return exchangeCall("depositMarginToken", ["address", "uint256", "bytes32"], [pointer, amount, account]);
}

/** `withdrawMargin(bytes32,bytes32,uint256)`. */
export function exchangeWithdrawMarginCall(account: string, assetId: string, amount: bigint): PrecompileCall {
  return exchangeCall("withdrawMargin", ["bytes32", "bytes32", "uint256"], [account, assetId, amount]);
}

export const sendExchangePlaceOrder = (
  wallet: Eip1193Requester,
  from: string,
  order: Parameters<typeof exchangePlaceOrderCall>[0],
): Promise<string> => sendPrecompileCall(wallet, from, exchangePlaceOrderCall(order));

export const sendExchangeCancelOrder = (wallet: Eip1193Requester, from: string, orderId: string): Promise<string> =>
  sendPrecompileCall(wallet, from, exchangeCancelOrderCall(orderId));

export const sendExchangeRequestSettlement = (wallet: Eip1193Requester, from: string, positionId: string): Promise<string> =>
  sendPrecompileCall(wallet, from, exchangeRequestSettlementCall(positionId));

export const sendExchangeDepositMargin = (
  wallet: Eip1193Requester,
  from: string,
  account: string,
  amountWei: bigint,
): Promise<string> => sendPrecompileCall(wallet, from, exchangeDepositMarginCall(account, amountWei));

export const sendExchangeDepositMarginToken = (
  wallet: Eip1193Requester,
  from: string,
  pointer: string,
  amount: bigint,
  account: string,
): Promise<string> => sendPrecompileCall(wallet, from, exchangeDepositMarginTokenCall(pointer, amount, account));

export const sendExchangeWithdrawMargin = (
  wallet: Eip1193Requester,
  from: string,
  account: string,
  assetId: string,
  amount: bigint,
): Promise<string> => sendPrecompileCall(wallet, from, exchangeWithdrawMarginCall(account, assetId, amount));
