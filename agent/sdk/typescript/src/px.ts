import * as http from "node:http";
import * as https from "node:https";

import { LayerXKeyCredential } from "./agent-http.js";
import { decodeJsonRpcResponse, type RpcParam } from "./rpc.js";

const MAX_RESPONSE_BYTES = 9 * 1024 * 1024;
const MAX_REQUEST_BYTES = 1024 * 1024;
const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_U128 = 340282366920938463463374607431768211455n;
const MAX_U64 = 18446744073709551615n;
const EVM_ADDRESS = /^0x[0-9a-f]{40}$/u;
const HEX32 = /^[0-9a-f]{64}$/u;
const DID = /^did:layerx:[0-9a-f]{64}$/u;
const DECIMAL = /^(0|[1-9][0-9]*)$/u;
const HEX_QUANTITY = /^0x[0-9a-f]{1,32}$/u;

/** The joined asset ceiling the gateway answers within. */
export const PX_MAXIMUM_JOINED_ASSETS = 1024;

export const PX_METHODS = [
  "px_resolveAccount",
  "px_getAccount",
  "px_getBalances",
  "px_listAssets",
  "px_getNetwork",
] as const;
export type PxMethod = (typeof PX_METHODS)[number];

export interface PxResolvedIdentities {
  readonly evmAddress: string | null;
  readonly paxAddress: string | null;
  readonly layerxDid: string | null;
  readonly layerxAccount: string | null;
  readonly bound: boolean;
}

export interface PxPaxeerAccount {
  readonly address: string;
  readonly balance: bigint;
  readonly nonce: bigint;
}

export interface PxAccountJoin {
  readonly account: PxResolvedIdentities;
  readonly paxeer: PxPaxeerAccount | null;
  readonly layerx: Readonly<Record<string, unknown>> | null;
}

export interface PxCustodyAsset {
  readonly assetId: string;
  readonly denom: string;
  readonly pointer: string;
  readonly enabled: boolean;
  readonly paused: boolean;
  readonly minimumDeposit: bigint;
  readonly custodyCap: bigint;
  readonly custodied: bigint;
  readonly released: bigint;
  readonly pending: bigint;
}

export interface PxPaxeerAssetBalance {
  readonly denom: string;
  readonly amount: bigint;
}

export interface PxAssetBalance {
  readonly assetId: string;
  readonly denom: string | null;
  readonly custody: PxCustodyAsset | null;
  readonly paxeer: PxPaxeerAssetBalance | null;
  readonly layerx: Readonly<Record<string, unknown>> | null;
}

export interface PxAccountBalances {
  readonly account: PxResolvedIdentities;
  readonly balances: readonly PxAssetBalance[];
  readonly joinedLimit: bigint;
}

export interface PxAssetEntry {
  readonly assetId: string;
  readonly layerx: Readonly<Record<string, unknown>> | null;
  readonly paxeer: PxCustodyAsset | null;
}

export interface PxAssetTable {
  readonly assets: readonly PxAssetEntry[];
  readonly joinedLimit: bigint;
}

export interface PxAnchorHead {
  readonly latestFinalizedBatch: bigint | null;
  readonly status: bigint | null;
  readonly statusName: string | null;
  readonly statusLadder: Readonly<Record<string, string>> | null;
}

export interface PxPaxeerHead {
  readonly chainId: bigint;
  readonly latestBlock: bigint;
}

export interface PxNetworkHead {
  readonly networkId: string;
  readonly paxeer: PxPaxeerHead;
  readonly layerx: Readonly<Record<string, unknown>> | null;
  readonly anchor: PxAnchorHead | null;
}

export interface PxClientOptions {
  readonly endpoint: URL | string;
  readonly credential?: LayerXKeyCredential;
  readonly timeoutMs?: number;
  readonly maximumResponseBytes?: number;
}

/** The unified-network read namespace over one network-gateway endpoint. */
export class PxClient {
  readonly #endpoint: URL;
  readonly #credential: LayerXKeyCredential | undefined;
  readonly #timeoutMs: number;
  readonly #maximumResponseBytes: number;
  #id = 0n;

  public constructor(options: PxClientOptions) {
    this.#endpoint = validateEndpoint(options.endpoint);
    this.#credential = options.credential;
    this.#timeoutMs = exactPositive(options.timeoutMs ?? DEFAULT_TIMEOUT_MS);
    this.#maximumResponseBytes = exactPositive(options.maximumResponseBytes ?? MAX_RESPONSE_BYTES);
    if (this.#maximumResponseBytes > MAX_RESPONSE_BYTES) throw new Error("Invalid gateway response bound");
  }

  public async resolveAccount(account: string): Promise<PxResolvedIdentities> {
    return decodePxResolvedIdentities(await this.call("px_resolveAccount", [pxAccountKey(account)]));
  }

  public async getAccount(account: string): Promise<PxAccountJoin> {
    return decodePxAccountJoin(await this.call("px_getAccount", [pxAccountKey(account)]));
  }

  public async getBalances(account: string): Promise<PxAccountBalances> {
    return decodePxAccountBalances(await this.call("px_getBalances", [pxAccountKey(account)]));
  }

  public async listAssets(): Promise<PxAssetTable> {
    return decodePxAssetTable(await this.call("px_listAssets", []));
  }

  public async getNetwork(): Promise<PxNetworkHead> {
    return decodePxNetworkHead(await this.call("px_getNetwork", []));
  }

  private async call(method: PxMethod, params: readonly RpcParam[]): Promise<Record<string, unknown>> {
    const id = (++this.#id).toString();
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", id, method, params }), "utf8");
    if (body.length > MAX_REQUEST_BYTES) throw new Error("Gateway request too large");
    const headers: Record<string, string> = {
      accept: "application/json",
      "content-type": "application/json",
      "content-length": String(body.length),
      "user-agent": "layerx-typescript/0.1.0",
    };
    this.#credential?.use((authorization) => { headers.authorization = authorization; });
    const bytes = await this.send(headers, body);
    return decodeJsonRpcResponse(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes)) as unknown, id);
  }

  private send(headers: Record<string, string>, body: Buffer): Promise<Buffer> {
    return new Promise<Buffer>((resolve, reject) => {
      const driver = this.#endpoint.protocol === "https:" ? https : http;
      const request = driver.request(this.#endpoint, { method: "POST", headers, timeout: this.#timeoutMs }, (response) => {
        const chunks: Buffer[] = [];
        let received = 0;
        if (response.statusCode !== 200 || response.headers["content-type"]?.split(";")[0]?.trim() !== "application/json") {
          response.destroy();
          reject(new Error("Invalid gateway HTTP answer"));
          return;
        }
        response.on("data", (chunk: Buffer) => {
          received += chunk.length;
          if (received > this.#maximumResponseBytes) {
            request.destroy(new Error("Gateway answer too large"));
            return;
          }
          chunks.push(Buffer.from(chunk));
        });
        response.on("error", reject);
        response.on("end", () => resolve(Buffer.concat(chunks)));
      });
      request.on("timeout", () => request.destroy(new Error("Gateway deadline exceeded")));
      request.on("error", reject);
      request.end(body);
    });
  }
}

/** Normalises the three public account spellings the gateway addresses. */
export function pxAccountKey(account: string): string {
  if (typeof account !== "string") throw new Error("Invalid account identifier");
  const lowered = account.trim().toLowerCase();
  if (EVM_ADDRESS.test(lowered) || DID.test(lowered) || HEX32.test(lowered)) return lowered;
  throw new Error("Invalid account identifier");
}

/** Decodes a gateway quantity: an unsigned decimal string, a `0x` quantity or a JSON integer. */
export function pxQuantity(value: unknown): bigint {
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value) || value < 0) throw new Error("Invalid gateway quantity");
    return BigInt(value);
  }
  if (typeof value !== "string") throw new Error("Invalid gateway quantity");
  const text = value.trim().toLowerCase();
  const parsed = HEX_QUANTITY.test(text) ? BigInt(text) : DECIMAL.test(text) ? BigInt(text) : null;
  if (parsed === null || parsed > MAX_U128) throw new Error("Invalid gateway quantity");
  return parsed;
}

/** Decodes an absent gateway quantity as absent, never as zero. */
export function pxOptionalQuantity(value: unknown): bigint | null {
  return value === null ? null : pxQuantity(value);
}

function pxCounted(value: unknown): bigint {
  const parsed = pxQuantity(value);
  if (parsed > MAX_U64) throw new Error("Invalid gateway count");
  return parsed;
}

function pxOptionalCounted(value: unknown): bigint | null {
  return value === null ? null : pxCounted(value);
}

export function decodePxResolvedIdentities(value: unknown): PxResolvedIdentities {
  const document = object(value);
  present(document, ["evm_address", "pax_address", "layerx_did", "layerx_account", "bound"]);
  const bound = document.bound;
  if (typeof bound !== "boolean") throw new Error("Invalid gateway identities");
  return Object.freeze({
    evmAddress: optionalPattern(document.evm_address, EVM_ADDRESS),
    paxAddress: optionalText(document.pax_address, 128),
    layerxDid: optionalPattern(document.layerx_did, DID),
    layerxAccount: optionalPattern(document.layerx_account, HEX32),
    bound,
  });
}

export function decodePxAccountJoin(value: unknown): PxAccountJoin {
  const document = object(value);
  present(document, ["account", "paxeer", "layerx"]);
  const paxeer = document.paxeer;
  return Object.freeze({
    account: decodePxResolvedIdentities(document.account),
    paxeer: paxeer === null ? null : decodePaxeerAccount(paxeer),
    layerx: objectOrNull(document.layerx),
  });
}

export function decodePxAccountBalances(value: unknown): PxAccountBalances {
  const document = object(value);
  present(document, ["account", "balances", "joined_limit"]);
  const rows = document.balances;
  if (!Array.isArray(rows) || rows.length > PX_MAXIMUM_JOINED_ASSETS) throw new Error("Invalid gateway balances");
  return Object.freeze({
    account: decodePxResolvedIdentities(document.account),
    balances: Object.freeze(rows.map(decodePxAssetBalance)),
    joinedLimit: pxCounted(document.joined_limit),
  });
}

export function decodePxAssetBalance(value: unknown): PxAssetBalance {
  const row = object(value);
  present(row, ["asset_id", "denom", "custody", "paxeer", "layerx"]);
  const paxeer = row.paxeer;
  let balance: PxPaxeerAssetBalance | null = null;
  if (paxeer !== null) {
    const half = object(paxeer);
    present(half, ["denom", "amount"]);
    balance = Object.freeze({ denom: text(half.denom, 128), amount: pxQuantity(half.amount) });
  }
  return Object.freeze({
    assetId: pattern(row.asset_id, HEX32),
    denom: optionalText(row.denom, 128),
    custody: decodePxCustodyAsset(row.custody),
    paxeer: balance,
    layerx: objectOrNull(row.layerx),
  });
}

export function decodePxCustodyAsset(value: unknown): PxCustodyAsset | null {
  if (value === null) return null;
  const custody = object(value);
  present(custody, ["asset_id", "denom", "pointer", "enabled", "paused", "minimum_deposit", "custody_cap", "custodied", "released", "pending"]);
  const enabled = custody.enabled;
  const paused = custody.paused;
  if (typeof enabled !== "boolean" || typeof paused !== "boolean") throw new Error("Invalid custody asset");
  return Object.freeze({
    assetId: pattern(custody.asset_id, HEX32),
    denom: text(custody.denom, 128),
    pointer: pattern(custody.pointer, EVM_ADDRESS),
    enabled,
    paused,
    minimumDeposit: pxQuantity(custody.minimum_deposit),
    custodyCap: pxQuantity(custody.custody_cap),
    custodied: pxQuantity(custody.custodied),
    released: pxQuantity(custody.released),
    pending: pxQuantity(custody.pending),
  });
}

export function decodePxAssetTable(value: unknown): PxAssetTable {
  const document = object(value);
  present(document, ["assets", "joined_limit"]);
  const rows = document.assets;
  if (!Array.isArray(rows) || rows.length > PX_MAXIMUM_JOINED_ASSETS) throw new Error("Invalid gateway asset map");
  const assets = rows.map((row): PxAssetEntry => {
    const entry = object(row);
    present(entry, ["asset_id", "layerx", "paxeer"]);
    return Object.freeze({
      assetId: pattern(entry.asset_id, HEX32),
      layerx: objectOrNull(entry.layerx),
      paxeer: decodePxCustodyAsset(entry.paxeer),
    });
  });
  return Object.freeze({ assets: Object.freeze(assets), joinedLimit: pxCounted(document.joined_limit) });
}

export function decodePxNetworkHead(value: unknown): PxNetworkHead {
  const document = object(value);
  present(document, ["network_id", "paxeer", "layerx", "anchor"]);
  const paxeer = object(document.paxeer);
  present(paxeer, ["chain_id", "latest_block"]);
  const anchor = document.anchor;
  return Object.freeze({
    networkId: text(document.network_id, 128),
    paxeer: Object.freeze({ chainId: pxCounted(paxeer.chain_id), latestBlock: pxCounted(paxeer.latest_block) }),
    layerx: objectOrNull(document.layerx),
    anchor: anchor === null ? null : decodePxAnchorHead(anchor),
  });
}

export function decodePxAnchorHead(value: unknown): PxAnchorHead {
  const anchor = object(value);
  present(anchor, ["latest_finalized_batch", "status", "status_name", "status_ladder"]);
  const ladder = anchor.status_ladder;
  let statusLadder: Readonly<Record<string, string>> | null = null;
  if (ladder !== null) {
    const rungs = object(ladder);
    for (const rung of Object.values(rungs)) if (typeof rung !== "string" || rung.length === 0 || rung.length > 64) throw new Error("Invalid anchor ladder");
    statusLadder = Object.freeze({ ...rungs } as Record<string, string>);
  }
  return Object.freeze({
    latestFinalizedBatch: pxOptionalCounted(anchor.latest_finalized_batch),
    status: pxOptionalCounted(anchor.status),
    statusName: optionalText(anchor.status_name, 64),
    statusLadder,
  });
}

function decodePaxeerAccount(value: unknown): PxPaxeerAccount {
  const document = object(value);
  present(document, ["address", "balance", "nonce"]);
  return Object.freeze({
    address: pattern(document.address, EVM_ADDRESS),
    balance: pxQuantity(document.balance),
    nonce: pxCounted(document.nonce),
  });
}

function object(value: unknown): Readonly<Record<string, unknown>> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid gateway document");
  return value as Readonly<Record<string, unknown>>;
}

function objectOrNull(value: unknown): Readonly<Record<string, unknown>> | null {
  return value === null ? null : object(value);
}

function present(value: Readonly<Record<string, unknown>>, required: readonly string[]): void {
  if (required.some((key) => !(key in value))) throw new Error("Incomplete gateway document");
}

function pattern(value: unknown, expected: RegExp): string {
  if (typeof value !== "string" || !expected.test(value.toLowerCase())) throw new Error("Invalid gateway field");
  return value.toLowerCase();
}

function optionalPattern(value: unknown, expected: RegExp): string | null {
  return value === null || value === "" ? null : pattern(value, expected);
}

function text(value: unknown, maximum: number): string {
  if (typeof value !== "string" || value.length === 0 || value.length > maximum) throw new Error("Invalid gateway text");
  return value;
}

function optionalText(value: unknown, maximum: number): string | null {
  return value === null ? null : text(value, maximum);
}

function validateEndpoint(value: URL | string): URL {
  let endpoint: URL;
  try { endpoint = new URL(value); } catch { throw new Error("Invalid gateway endpoint"); }
  if ((endpoint.protocol !== "https:" && endpoint.protocol !== "http:")
    || endpoint.username !== "" || endpoint.password !== "" || endpoint.search !== "" || endpoint.hash !== "") {
    throw new Error("Invalid gateway endpoint");
  }
  const host = endpoint.hostname.toLowerCase();
  if (endpoint.protocol === "http:" && host !== "localhost" && host !== "[::1]" && !/^127(?:\.[0-9]{1,3}){3}$/u.test(host)) {
    throw new Error("Invalid gateway endpoint");
  }
  return endpoint;
}

function exactPositive(value: number): number {
  if (!Number.isSafeInteger(value) || value <= 0) throw new Error("Invalid gateway bound");
  return value;
}
