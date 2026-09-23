import "server-only";

import type { PrecompileCall, PrecompileLog } from "./sdk.ts";

const FETCH_TIMEOUT_MS = 8_000;
const MAX_RESPONSE_BYTES = 9 * 1024 * 1024;
const EVM_ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const HEX32 = /^[0-9a-f]{64}$/u;
const DID = /^did:layerx:[0-9a-f]{64}$/u;
const DECIMAL = /^(0|[1-9][0-9]*)$/u;
const CURSOR = /^[0-9A-Za-z]{1,32}$/u;
const KIND = /^[a-z0-9_]{1,64}$/u;
const QUANTITY = /^0x[0-9a-fA-F]{1,64}$/u;
const DATA = /^0x(?:[0-9a-fA-F]{2})*$/u;
const HASH = /^0x[0-9a-fA-F]{64}$/u;

export const HISTORY_PAGE_LIMIT = 25;

export class GatewayUnavailableError extends Error {
  constructor(detail: string) {
    super(`The network gateway is unavailable: ${detail}`);
    this.name = "GatewayUnavailableError";
  }
}

export class GatewayRpcError extends Error {
  readonly code: number;

  constructor(code: number, message: string) {
    super(message);
    this.name = "GatewayRpcError";
    this.code = code;
  }
}

function gatewayEndpoint(): URL {
  const configured = process.env.LAYERX_GATEWAY_URL;
  if (configured === undefined) {
    throw new GatewayUnavailableError("LAYERX_GATEWAY_URL is not set");
  }
  let endpoint: URL;
  try {
    endpoint = new URL(configured);
  } catch {
    throw new GatewayUnavailableError("LAYERX_GATEWAY_URL is not a URL");
  }
  const loopback = endpoint.hostname === "127.0.0.1" || endpoint.hostname === "localhost";
  if (
    (endpoint.protocol !== "https:" && !(loopback && endpoint.protocol === "http:")) ||
    endpoint.username !== "" ||
    endpoint.password !== "" ||
    endpoint.search !== "" ||
    endpoint.hash !== ""
  ) {
    throw new GatewayUnavailableError("LAYERX_GATEWAY_URL must be HTTPS or loopback HTTP");
  }
  return endpoint;
}

function isObject(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

let nextId = 0;

async function rpc(method: string, params: readonly unknown[]): Promise<unknown> {
  nextId += 1;
  const id = String(nextId);
  let response: Response;
  try {
    response = await fetch(gatewayEndpoint(), {
      method: "POST",
      headers: { accept: "application/json", "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
      cache: "no-store",
      signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
    });
  } catch (error) {
    if (error instanceof GatewayUnavailableError) {
      throw error;
    }
    throw new GatewayUnavailableError(`${method} did not answer`);
  }
  if (!response.ok) {
    throw new GatewayUnavailableError(`${method} answered HTTP ${String(response.status)}`);
  }
  const body = await response.text();
  if (body.length > MAX_RESPONSE_BYTES) {
    throw new GatewayUnavailableError(`${method} answer too large`);
  }
  const document: unknown = JSON.parse(body);
  if (!isObject(document) || document.jsonrpc !== "2.0" || document.id !== id) {
    throw new GatewayUnavailableError(`${method} answered a malformed envelope`);
  }
  if ("error" in document) {
    const failure = document.error;
    if (!isObject(failure) || typeof failure.code !== "number" || typeof failure.message !== "string") {
      throw new GatewayUnavailableError(`${method} answered a malformed error`);
    }
    throw new GatewayRpcError(failure.code, failure.message);
  }
  if (!("result" in document)) {
    throw new GatewayUnavailableError(`${method} answered no result`);
  }
  return document.result;
}

export async function ethCall(call: Readonly<{ to: string; data: string }>): Promise<string> {
  const result = await rpc("eth_call", [{ to: call.to, data: call.data }, "latest"]);
  if (typeof result !== "string" || !DATA.test(result)) {
    throw new GatewayUnavailableError("eth_call answered malformed data");
  }
  return result;
}

export async function ethBlockNumber(): Promise<bigint> {
  const result = await rpc("eth_blockNumber", []);
  if (typeof result !== "string" || !QUANTITY.test(result)) {
    throw new GatewayUnavailableError("eth_blockNumber answered a malformed quantity");
  }
  return BigInt(result);
}

export interface GatewayLog extends PrecompileLog {
  readonly blockNumber: bigint;
  readonly transactionHash: string;
  readonly logIndex: bigint;
}

export async function ethGetLogs(
  filter: Readonly<{ address: string; topics: readonly (string | null)[]; fromBlock: bigint }>,
): Promise<readonly GatewayLog[]> {
  const result = await rpc("eth_getLogs", [
    {
      address: filter.address,
      topics: filter.topics,
      fromBlock: `0x${filter.fromBlock.toString(16)}`,
      toBlock: "latest",
    },
  ]);
  if (!Array.isArray(result)) {
    throw new GatewayUnavailableError("eth_getLogs answered a malformed list");
  }
  return result.map((entry: unknown): GatewayLog => {
    if (
      !isObject(entry) ||
      typeof entry.address !== "string" ||
      !Array.isArray(entry.topics) ||
      typeof entry.data !== "string" ||
      typeof entry.blockNumber !== "string" ||
      !QUANTITY.test(entry.blockNumber) ||
      typeof entry.transactionHash !== "string" ||
      !HASH.test(entry.transactionHash) ||
      typeof entry.logIndex !== "string" ||
      !QUANTITY.test(entry.logIndex)
    ) {
      throw new GatewayUnavailableError("eth_getLogs answered a malformed log");
    }
    const topics = entry.topics.map((topic: unknown) => {
      if (typeof topic !== "string") {
        throw new GatewayUnavailableError("eth_getLogs answered a malformed topic");
      }
      return topic;
    });
    return {
      address: entry.address,
      topics,
      data: entry.data,
      blockNumber: BigInt(entry.blockNumber),
      transactionHash: entry.transactionHash,
      logIndex: BigInt(entry.logIndex),
    };
  });
}

/** The Paxeer X fork surfaces the gateway probes with `eth_getCode`. */
export type ForkSurface = "exchange" | "bridge" | "launchpad";

/** The gateway's typed refusal of a write to a surface whose precompile has no code yet. */
export const SURFACE_UNAVAILABLE_CODE = -32003;

export interface NetworkCapabilities {
  readonly exchange: boolean;
  readonly bridge: boolean;
  readonly launchpad: boolean;
  readonly probedAt: number;
  readonly rpcHeight: bigint;
}

/** `px_getCapabilities`: which fork surfaces the chain answers for, at one Paxeer height. */
export async function pxGetCapabilities(): Promise<NetworkCapabilities> {
  const result = await rpc("px_getCapabilities", []);
  if (
    !isObject(result) ||
    typeof result.exchange !== "boolean" ||
    typeof result.bridge !== "boolean" ||
    typeof result.launchpad !== "boolean" ||
    typeof result.probed_at !== "number" ||
    !Number.isSafeInteger(result.probed_at) ||
    result.probed_at < 0 ||
    typeof result.rpc_height !== "string" ||
    !DECIMAL.test(result.rpc_height)
  ) {
    throw new GatewayUnavailableError("px_getCapabilities answered a malformed document");
  }
  return {
    exchange: result.exchange,
    bridge: result.bridge,
    launchpad: result.launchpad,
    probedAt: result.probed_at,
    rpcHeight: BigInt(result.rpc_height),
  };
}

export type SurfaceCapability = Readonly<{ live: boolean; detail: string | null }>;

/** Whether one surface's writes are open; an unreadable answer keeps the surface read-only and says why. */
export async function surfaceCapability(surface: ForkSurface): Promise<SurfaceCapability> {
  try {
    const capabilities = await pxGetCapabilities();
    return { live: capabilities[surface], detail: null };
  } catch (error) {
    if (error instanceof GatewayUnavailableError || error instanceof GatewayRpcError) {
      return { live: false, detail: error.message };
    }
    throw error;
  }
}

export type PrecompileView = Readonly<Pick<PrecompileCall, "to" | "data">>;

export interface ResolvedAccount {
  readonly evmAddress: string | null;
  readonly layerxDid: string | null;
  readonly layerxAccount: string | null;
  readonly bound: boolean;
}

function optionalMatch(value: unknown, pattern: RegExp): string | null {
  return typeof value === "string" && pattern.test(value) ? value : null;
}

export async function pxResolveAccount(account: string): Promise<ResolvedAccount> {
  const result = await rpc("px_resolveAccount", [account]);
  if (!isObject(result) || typeof result.bound !== "boolean") {
    throw new GatewayUnavailableError("px_resolveAccount answered a malformed document");
  }
  return {
    evmAddress: optionalMatch(result.evm_address, /^0x[0-9a-f]{40}$/u),
    layerxDid: optionalMatch(result.layerx_did, DID),
    layerxAccount: optionalMatch(result.layerx_account, HEX32),
    bound: result.bound,
  };
}

export type HistoryChain = "layerx" | "paxeer";

export interface HistoryRow {
  readonly id: bigint;
  readonly heightOrSeq: bigint;
  readonly chain: HistoryChain;
  readonly kind: string;
  readonly direction: "in" | "out";
  readonly account: string;
  readonly counterparty: string | null;
  readonly asset: string;
  readonly assetSymbol: string | null;
  readonly assetDecimals: number | null;
  readonly amount: bigint;
  readonly txId: string;
  readonly final: boolean;
  readonly side: HistoryChain | null;
}

export interface HistoryPage {
  readonly items: readonly HistoryRow[];
  readonly nextCursor: string | null;
}

export interface HistoryOptions {
  readonly cursor?: string | undefined;
  readonly kind?: string | undefined;
}

function chainOf(value: unknown): HistoryChain {
  if (value !== "layerx" && value !== "paxeer") {
    throw new GatewayUnavailableError("history answered an unknown chain");
  }
  return value;
}

function decimalOf(value: unknown): bigint {
  if (typeof value !== "string" || !DECIMAL.test(value)) {
    throw new GatewayUnavailableError("history answered a malformed decimal");
  }
  return BigInt(value);
}

function textOf(value: unknown): string {
  if (typeof value !== "string" || value.length === 0 || value.length > 256) {
    throw new GatewayUnavailableError("history answered malformed text");
  }
  return value;
}

function historyRow(value: unknown, unified: boolean): HistoryRow {
  if (!isObject(value)) {
    throw new GatewayUnavailableError("history answered a malformed row");
  }
  const direction = value.direction;
  if (direction !== "in" && direction !== "out") {
    throw new GatewayUnavailableError("history answered an unknown direction");
  }
  if (typeof value.final !== "boolean" || typeof value.kind !== "string" || !KIND.test(value.kind)) {
    throw new GatewayUnavailableError("history answered a malformed row");
  }
  const metadata = isObject(value.asset_metadata) ? value.asset_metadata : null;
  const decimals = metadata?.decimals;
  return {
    id: decimalOf(value.id),
    heightOrSeq: decimalOf(value.height_or_seq),
    chain: chainOf(value.chain),
    kind: value.kind,
    direction,
    account: textOf(value.account),
    counterparty: value.counterparty === null ? null : textOf(value.counterparty),
    asset: textOf(value.asset),
    assetSymbol: typeof metadata?.symbol === "string" ? metadata.symbol : null,
    assetDecimals:
      typeof decimals === "number" && Number.isSafeInteger(decimals) && decimals >= 0 && decimals <= 255
        ? decimals
        : null,
    amount: decimalOf(value.amount),
    txId: textOf(value.tx_id),
    final: value.final,
    side: unified ? chainOf(value.side) : null,
  };
}

function historyPage(value: unknown, unified: boolean): HistoryPage {
  if (!isObject(value) || !Array.isArray(value.items)) {
    throw new GatewayUnavailableError("history answered a malformed page");
  }
  const cursor = value.next_cursor;
  if (cursor !== null && (typeof cursor !== "string" || !CURSOR.test(cursor))) {
    throw new GatewayUnavailableError("history answered a malformed cursor");
  }
  return {
    items: value.items.map((row: unknown) => historyRow(row, unified)),
    nextCursor: cursor,
  };
}

function historyParams(account: string, options: HistoryOptions): readonly unknown[] {
  const cursor = options.cursor !== undefined && CURSOR.test(options.cursor) ? options.cursor : null;
  const kind = options.kind !== undefined && KIND.test(options.kind) ? options.kind : null;
  return [account, cursor, HISTORY_PAGE_LIMIT, kind];
}

/** `lx_getHistory`: the indexed history of one LayerX account id. */
export async function layerxHistory(account: string, options: HistoryOptions = {}): Promise<HistoryPage> {
  const lowered = account.trim().toLowerCase();
  if (!HEX32.test(lowered)) {
    throw new GatewayUnavailableError("lx_getHistory takes a LayerX account id");
  }
  return historyPage(await rpc("lx_getHistory", historyParams(lowered, options)), false);
}

/** `px_getHistory`: the indexed history of one Paxeer EVM address. */
export async function paxeerHistory(account: string, options: HistoryOptions = {}): Promise<HistoryPage> {
  const lowered = account.trim().toLowerCase();
  if (!EVM_ADDRESS.test(lowered)) {
    throw new GatewayUnavailableError("px_getHistory takes an EVM address");
  }
  return historyPage(await rpc("px_getHistory", historyParams(lowered, options)), false);
}

/** `px_getUnifiedHistory`: both halves of a unified account, newest first. */
export async function unifiedHistory(account: string, options: HistoryOptions = {}): Promise<HistoryPage> {
  const lowered = account.trim().toLowerCase();
  if (!EVM_ADDRESS.test(lowered) && !HEX32.test(lowered) && !DID.test(lowered)) {
    throw new GatewayUnavailableError("px_getUnifiedHistory takes an account identifier");
  }
  return historyPage(await rpc("px_getUnifiedHistory", historyParams(lowered, options)), true);
}
