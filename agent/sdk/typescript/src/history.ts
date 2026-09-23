import type { RpcParam } from "./rpc.js";

/** Rows the gateway answers per history page when no limit is named. */
export const HISTORY_DEFAULT_LIMIT = 50;
/** The largest history page the gateway answers. */
export const HISTORY_MAXIMUM_LIMIT = 100;
/** The most indexer accounts one unified history merges. */
export const HISTORY_MAXIMUM_ACCOUNTS = 16;

const MAX_U256 = (1n << 256n) - 1n;
const MAX_U64 = 18446744073709551615n;
const DECIMAL = /^(0|[1-9][0-9]*)$/u;
const ROW_ID = /^[1-9][0-9]*$/u;
const CURSOR = /^[0-9A-Za-z]{1,32}$/u;
const KIND = /^[a-z0-9_]{1,64}$/u;
const HEX32 = /^[0-9a-f]{64}$/u;
const EVM_ADDRESS = /^0x[0-9a-f]{40}$/u;
const DID = /^did:layerx:[0-9a-f]{64}$/u;

export type HistoryChain = "layerx" | "paxeer";
export type HistoryDirection = "in" | "out";

export interface HistoryOptions {
  /** The opaque `nextCursor` of the previous page. */
  readonly cursor?: string;
  readonly limit?: number;
  /** Only rows of this indexer kind, such as `lxp_transfer` or `erc20_transfer`. */
  readonly kind?: string;
}

export interface HistoryAssetMetadata {
  readonly asset: string;
  readonly chain: HistoryChain;
  readonly kind: string;
  readonly address: string | null;
  readonly denom: string | null;
  readonly symbol: string | null;
  readonly decimals: number | null;
  readonly nativeId: string | null;
  readonly pointer: string | null;
  readonly metadata: unknown;
}

export interface HistoryRow {
  readonly id: bigint;
  readonly heightOrSeq: bigint;
  readonly chain: HistoryChain;
  readonly kind: string;
  readonly direction: HistoryDirection;
  readonly account: string;
  readonly counterparty: string | null;
  readonly asset: string;
  readonly amount: bigint;
  readonly txId: string;
  readonly ordinal: bigint;
  readonly final: boolean;
  readonly decoded: unknown;
  readonly assetMetadata: HistoryAssetMetadata | null;
  /** Which half of a unified account the row belongs to; null outside unified history. */
  readonly side: HistoryChain | null;
}

export interface HistoryPage {
  readonly account: string;
  readonly items: readonly HistoryRow[];
  readonly nextCursor: string | null;
}

export interface HistorySide {
  readonly side: HistoryChain;
  readonly account: string;
}

export interface UnifiedHistoryPage {
  readonly account: Readonly<Record<string, unknown>>;
  readonly accounts: readonly HistorySide[];
  readonly items: readonly HistoryRow[];
  readonly nextCursor: string | null;
}

/** The positional `[account, cursor, limit, kind]` parameters of a history read. */
export function historyParams(account: string, options: HistoryOptions = {}): readonly RpcParam[] {
  const { cursor, limit, kind } = options;
  if (cursor !== undefined && !CURSOR.test(cursor)) throw new Error("Invalid history cursor");
  if (limit !== undefined && (!Number.isSafeInteger(limit) || limit < 1 || limit > HISTORY_MAXIMUM_LIMIT)) throw new Error("Invalid history limit");
  if (kind !== undefined && !KIND.test(kind)) throw new Error("Invalid history kind");
  return Object.freeze([account, cursor ?? null, limit ?? null, kind ?? null]);
}

/** A LayerX account id as `lx_getHistory` takes it. */
export function layerxHistoryAccount(account: string): string {
  const lowered = typeof account === "string" ? account.trim().toLowerCase() : "";
  if (!HEX32.test(lowered) || lowered === "00".repeat(32)) throw new Error("Invalid LayerX account");
  return lowered;
}

/** A Paxeer EVM address as `px_getHistory` takes it. */
export function paxeerHistoryAccount(account: string): string {
  const lowered = typeof account === "string" ? account.trim().toLowerCase() : "";
  if (!EVM_ADDRESS.test(lowered)) throw new Error("Invalid Paxeer address");
  return lowered;
}

/** Any unified account spelling `px_getUnifiedHistory` resolves. */
export function unifiedHistoryAccount(account: string): string {
  const lowered = typeof account === "string" ? account.trim().toLowerCase() : "";
  if (EVM_ADDRESS.test(lowered) || DID.test(lowered) || HEX32.test(lowered)) return lowered;
  throw new Error("Invalid account identifier");
}

export function decodeHistoryPage(value: unknown, pattern: RegExp, limit = HISTORY_MAXIMUM_LIMIT): HistoryPage {
  const document = object(value);
  present(document, ["account", "items", "next_cursor"]);
  const account = document.account;
  if (typeof account !== "string" || !pattern.test(account)) throw new Error("Invalid history account");
  const items = rows(document.items, false, limit);
  return Object.freeze({ account, items, nextCursor: nextCursor(document.next_cursor, items) });
}

export function decodeLayerxHistoryPage(value: unknown, limit?: number): HistoryPage {
  return decodeHistoryPage(value, HEX32, limit);
}

export function decodePaxeerHistoryPage(value: unknown, limit?: number): HistoryPage {
  return decodeHistoryPage(value, EVM_ADDRESS, limit);
}

export function decodeUnifiedHistoryPage(value: unknown, limit = HISTORY_MAXIMUM_LIMIT): UnifiedHistoryPage {
  const document = object(value);
  present(document, ["account", "accounts", "items", "next_cursor"]);
  const sides = document.accounts;
  if (!Array.isArray(sides) || sides.length > HISTORY_MAXIMUM_ACCOUNTS) throw new Error("Invalid history accounts");
  const accounts = Object.freeze(sides.map((entry): HistorySide => {
    const side = object(entry);
    present(side, ["side", "account"]);
    const name = chain(side.side);
    const account = side.account;
    if (typeof account !== "string" || !(name === "layerx" ? HEX32 : EVM_ADDRESS).test(account)) throw new Error("Invalid history account");
    return Object.freeze({ side: name, account });
  }));
  const items = rows(document.items, true, limit);
  for (let index = 1; index < items.length; index += 1) {
    if (items[index - 1]!.id <= items[index]!.id) throw new Error("Unordered unified history");
  }
  return Object.freeze({ account: object(document.account), accounts, items, nextCursor: nextCursor(document.next_cursor, items) });
}

export function decodeHistoryRow(value: unknown, unified: boolean): HistoryRow {
  const row = object(value);
  present(row, ["id", "height_or_seq", "chain", "kind", "direction", "account", "counterparty", "asset", "amount", "tx_id", "ordinal", "final", "decoded", "asset_metadata"]);
  const direction = row.direction;
  if (direction !== "in" && direction !== "out") throw new Error("Invalid history direction");
  if (typeof row.final !== "boolean") throw new Error("Invalid history finality");
  const kind = row.kind;
  if (typeof kind !== "string" || !KIND.test(kind)) throw new Error("Invalid history kind");
  const id = decimal(row.id, MAX_U64);
  if (id === 0n || !ROW_ID.test(row.id as string)) throw new Error("Invalid history row id");
  if (unified) present(row, ["side"]);
  return Object.freeze({
    id,
    heightOrSeq: decimal(row.height_or_seq, MAX_U64),
    chain: chain(row.chain),
    kind,
    direction,
    account: text(row.account, 256),
    counterparty: row.counterparty === null ? null : text(row.counterparty, 256),
    asset: text(row.asset, 256),
    amount: decimal(row.amount, MAX_U256),
    txId: text(row.tx_id, 256),
    ordinal: decimal(row.ordinal, MAX_U64),
    final: row.final,
    decoded: row.decoded,
    assetMetadata: decodeHistoryAssetMetadata(row.asset_metadata, row.asset as string),
    side: unified ? chain(row.side) : null,
  });
}

export function decodeHistoryAssetMetadata(value: unknown, asset?: string): HistoryAssetMetadata | null {
  if (value === null) return null;
  const document = object(value);
  present(document, ["asset", "chain", "kind", "address", "denom", "symbol", "decimals", "native_id", "pointer", "metadata"]);
  const label = text(document.asset, 256);
  if (asset !== undefined && label !== asset) throw new Error("Mismatched history asset");
  const decimals = document.decimals;
  if (decimals !== null && (!Number.isSafeInteger(decimals) || (decimals as number) < 0 || (decimals as number) > 255)) throw new Error("Invalid asset decimals");
  return Object.freeze({
    asset: label,
    chain: chain(document.chain),
    kind: text(document.kind, 64),
    address: optionalPattern(document.address, EVM_ADDRESS),
    denom: document.denom === null ? null : text(document.denom, 128),
    symbol: document.symbol === null ? null : text(document.symbol, 64),
    decimals: decimals as number | null,
    nativeId: optionalPattern(document.native_id, HEX32),
    pointer: optionalPattern(document.pointer, EVM_ADDRESS),
    metadata: document.metadata,
  });
}

function rows(value: unknown, unified: boolean, limit: number): readonly HistoryRow[] {
  if (!Array.isArray(value) || value.length > limit) throw new Error("Invalid history page");
  return Object.freeze(value.map((row) => decodeHistoryRow(row, unified)));
}

function nextCursor(value: unknown, items: readonly HistoryRow[]): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || !CURSOR.test(value) || items.length === 0) throw new Error("Invalid history cursor");
  return value;
}

function chain(value: unknown): HistoryChain {
  if (value !== "layerx" && value !== "paxeer") throw new Error("Invalid history chain");
  return value;
}

function decimal(value: unknown, maximum: bigint): bigint {
  if (typeof value !== "string" || !DECIMAL.test(value)) throw new Error("Invalid history decimal");
  const parsed = BigInt(value);
  if (parsed > maximum) throw new Error("Invalid history decimal");
  return parsed;
}

function object(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid history document");
  return value as Readonly<Record<string, unknown>>;
}

function present(value: Readonly<Record<string, unknown>>, required: readonly string[]): void {
  if (required.some((key) => !(key in value))) throw new Error("Incomplete history document");
}

function text(value: unknown, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum) throw new Error("Invalid history text");
  return value;
}

function optionalPattern(value: unknown, expected: RegExp): string | null {
  if (value === null) return null;
  if (typeof value !== "string" || !expected.test(value)) throw new Error("Invalid history field");
  return value;
}
