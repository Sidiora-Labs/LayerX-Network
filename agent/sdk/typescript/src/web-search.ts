import { createHash } from "node:crypto";
import * as http from "node:http";
import * as https from "node:https";
import { keccak_256 } from "@noble/hashes/sha3.js";
import { walletAccount } from "./rpc.js";
import { verifyReceipt, type AuthorizedReceiptBatch, type ReceiptVerification, type SelectableProtocolVersion } from "./verifier.js";
import { encodeGrant, type PayerGrant } from "./x402/receive.js";

export const WEB_CONTENT_DOMAIN = "PAXEERX_WEB_CONTENT_V1";
export const WEB_CONTENT_FETCH = 1;
export const WEB_CONTENT_SEARCH = 2;
export const WEB_SEARCH_CURRENCIES = ["SID", "PAX", "USDC", "USDL"] as const;
export const WEB_SEARCH_PAYER_HEADER = "LAYERX-PAYER-DID";
export const WEB_SEARCH_MAX_RESULTS = 10;

export type WebSearchCurrency = typeof WEB_SEARCH_CURRENCIES[number];
export type WebSearchScheme = "metered" | "exact";
export type WebContentKind = typeof WEB_CONTENT_FETCH | typeof WEB_CONTENT_SEARCH;

export class WebSearchError extends Error {
  public constructor(public readonly code: string, public readonly status?: number) {
    super(code);
    this.name = "WebSearchError";
  }
}

export interface WebContent {
  readonly kind: WebContentKind;
  readonly payload: Uint8Array;
  readonly mediaType: string;
  readonly text: string;
}

export interface WebSearchOffer {
  readonly scheme: WebSearchScheme;
  readonly network: string;
  readonly amount: string;
  readonly asset: string;
  readonly payTo: string;
  readonly maxTimeoutSeconds: number;
  readonly currency: WebSearchCurrency;
  readonly account: string;
  readonly payer?: string;
  readonly purposeHash?: string;
}

export interface WebSearchPreference {
  readonly currency: WebSearchCurrency;
  readonly scheme: WebSearchScheme;
}

export interface WebSearchMeteredPayment {
  readonly grant: Uint8Array;
  readonly idempotencyKey: string;
}

export interface WebSearchExactPayment {
  readonly receipt: Uint8Array;
}

export interface WebSearchPayer {
  readonly did?: string;
  readonly preferences: readonly WebSearchPreference[];
  metered?(offer: WebSearchOffer, target: string): Promise<WebSearchMeteredPayment>;
  exact?(offer: WebSearchOffer, target: string): Promise<WebSearchExactPayment>;
}

export interface WebSearchAssetTerms {
  readonly assetId: string;
  readonly maxAmount: bigint;
}

export interface WebSearchClientOptions {
  readonly endpoint: string | URL;
  readonly network: string;
  readonly nativeAsset: string;
  readonly assets: Partial<Readonly<Record<WebSearchCurrency, WebSearchAssetTerms>>>;
  readonly payer: WebSearchPayer;
  readonly authority: (receipt: Uint8Array, offer: WebSearchOffer) => Promise<AuthorizedReceiptBatch>;
  readonly protocolVersion?: SelectableProtocolVersion;
  readonly now?: () => bigint;
  readonly pendingAttempts?: number;
}

export interface WebSettlement {
  readonly scheme: WebSearchScheme;
  readonly currency: WebSearchCurrency;
  readonly network: string;
  readonly asset: string;
  readonly amount: string;
  readonly payer: string;
  readonly receiptDigest: string;
  readonly transaction: string;
}

export interface WebSearchResult {
  readonly url: string;
  readonly title: string;
  readonly snippet: string;
}

export interface WebSearchResponse {
  readonly results: readonly WebSearchResult[];
  readonly settlement: WebSettlement | null;
}

export interface WebFetchResponse {
  readonly url: string;
  readonly finalUrl: string;
  readonly mediaType: string;
  readonly digest: string;
  readonly text: string;
  readonly settlement: WebSettlement | null;
}

export interface WebContentResponse {
  readonly digest: string;
  readonly canonical: Uint8Array;
  readonly content: WebContent;
  readonly settlement: WebSettlement | null;
}

interface Reply {
  readonly status: number;
  readonly headers: Readonly<Record<string, string>>;
  readonly body: Buffer;
}

interface PreparedPayment {
  readonly header: string;
  readonly receipt?: Uint8Array;
}

const HEX32 = /^[0-9a-f]{64}$/u;
const ZERO32 = "00".repeat(32);
const MEDIA_TYPE = /^[a-z0-9!#$&^_.+-]{1,127}\/[a-z0-9!#$&^_.+-]{1,127}$/u;
const AMOUNT = /^[1-9][0-9]{0,38}$/u;
const MAX_REPLY_BYTES = 9 * 1048576;
const MAX_HEADER_CHARACTERS = 131072;
const MAX_RETRY_AFTER_SECONDS = 30;
const GRANT_BYTES = 346;
const RECEIPT_LEAF_DOMAIN = "LXP/v1/merkle-leaf\0";

function hex(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("hex");
}

function hex32(value: unknown): value is string {
  return typeof value === "string" && HEX32.test(value);
}

function record(value: unknown, code: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new WebSearchError(code);
  return value as Record<string, unknown>;
}

function exactKeys(value: Record<string, unknown>, keys: readonly string[], code: string): void {
  if (Object.keys(value).length !== keys.length || keys.some((key) => !Object.hasOwn(value, key))) throw new WebSearchError(code);
}

function utf8(bytes: Uint8Array, code: string): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw new WebSearchError(code);
  }
}

function concatenate(...parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((size, part) => size + part.length, 0));
  let offset = 0;
  for (const part of parts) { output.set(part, offset); offset += part.length; }
  return output;
}

function bigEndian(value: bigint, size: number): Uint8Array {
  const bytes = new Uint8Array(size);
  let rest = value;
  for (let index = size - 1; index >= 0; index -= 1) { bytes[index] = Number(rest & 255n); rest >>= 8n; }
  if (rest !== 0n) throw new WebSearchError("content-too-long");
  return bytes;
}

function readBigEndian(bytes: Uint8Array, offset: number, size: number): bigint {
  let value = 0n;
  for (let index = offset; index < offset + size; index += 1) value = value * 256n + BigInt(bytes[index]!);
  return value;
}

export function webContentBytes(content: { readonly kind: WebContentKind; readonly payload: Uint8Array | string; readonly mediaType: string; readonly text: string }): Uint8Array {
  if (content.kind !== WEB_CONTENT_FETCH && content.kind !== WEB_CONTENT_SEARCH) throw new WebSearchError("invalid-content-kind");
  if (!MEDIA_TYPE.test(content.mediaType)) throw new WebSearchError("invalid-media-type");
  const payload = typeof content.payload === "string" ? new TextEncoder().encode(content.payload) : content.payload;
  const mediaType = new TextEncoder().encode(content.mediaType);
  const text = new TextEncoder().encode(content.text);
  return concatenate(new TextEncoder().encode(WEB_CONTENT_DOMAIN), new Uint8Array([content.kind]),
    bigEndian(BigInt(payload.length), 4), payload, bigEndian(BigInt(mediaType.length), 4), mediaType,
    bigEndian(BigInt(text.length), 8), text);
}

export function contentDigest(canonical: Uint8Array): string {
  return hex(keccak_256(canonical));
}

export function decodeWebContent(canonical: Uint8Array): WebContent {
  const domain = new TextEncoder().encode(WEB_CONTENT_DOMAIN);
  if (canonical.length < domain.length + 17 || domain.some((byte, index) => canonical[index] !== byte)) throw new WebSearchError("invalid-content");
  let offset = domain.length;
  const kind = canonical[offset]!;
  offset += 1;
  if (kind !== WEB_CONTENT_FETCH && kind !== WEB_CONTENT_SEARCH) throw new WebSearchError("invalid-content");
  const take = (lengthSize: number): Uint8Array => {
    if (offset + lengthSize > canonical.length) throw new WebSearchError("invalid-content");
    const length = readBigEndian(canonical, offset, lengthSize);
    offset += lengthSize;
    if (length > BigInt(canonical.length - offset)) throw new WebSearchError("invalid-content");
    const field = canonical.slice(offset, offset + Number(length));
    offset += field.length;
    return field;
  };
  const payload = take(4);
  const mediaType = utf8(take(4), "invalid-content");
  const text = utf8(take(8), "invalid-content");
  if (offset !== canonical.length || !MEDIA_TYPE.test(mediaType)) throw new WebSearchError("invalid-content");
  return Object.freeze({ kind, payload, mediaType, text });
}

type GrantField = readonly [keyof PayerGrant, "hex" | "integer" | "boolean", number];
const GRANT_LAYOUT: readonly GrantField[] = [
  ["grant_id", "hex", 32], ["from", "hex", 32], ["recipient", "hex", 32], ["asset", "hex", 32],
  ["per_draw_maximum", "integer", 16], ["allowance", "integer", 16], ["recurring", "boolean", 1],
  ["window_length", "integer", 8], ["expiration", "integer", 8], ["purpose_hash", "hex", 32],
  ["has_reference", "boolean", 1], ["reference_hash", "hex", 32], ["revocation_sequence", "integer", 8],
  ["public_key", "hex", 32], ["signature", "hex", 64],
];

export function decodePayerGrant(canonical: Uint8Array): PayerGrant {
  if (!(canonical instanceof Uint8Array) || canonical.length !== GRANT_BYTES) throw new WebSearchError("invalid-grant");
  let offset = 0;
  const fields: Record<string, string | boolean> = {};
  for (const [key, kind, size] of GRANT_LAYOUT) {
    const field = canonical.slice(offset, offset + size);
    offset += size;
    if (kind === "hex") fields[key] = hex(field);
    else if (kind === "integer") fields[key] = readBigEndian(field, 0, size).toString();
    else if (field[0] === 0 || field[0] === 1) fields[key] = field[0] === 1;
    else throw new WebSearchError("invalid-grant");
  }
  const grant = fields as unknown as PayerGrant;
  if (hex(encodeGrant(grant)) !== hex(canonical)) throw new WebSearchError("invalid-grant");
  return Object.freeze(grant);
}

function decodeHeader(value: string | undefined, code: string): Record<string, unknown> {
  if (value === undefined || value.length === 0 || value.length > MAX_HEADER_CHARACTERS || !/^[A-Za-z0-9+/]+={0,2}$/u.test(value)) throw new WebSearchError(code);
  const raw = Buffer.from(value, "base64");
  if (raw.toString("base64") !== value) throw new WebSearchError(code);
  let parsed: unknown;
  try {
    parsed = JSON.parse(utf8(raw, code));
  } catch {
    throw new WebSearchError(code);
  }
  return record(parsed, code);
}

function encodeHeader(value: unknown): string {
  return Buffer.from(JSON.stringify(value), "utf8").toString("base64");
}

function receiptDigest(receipt: Uint8Array): string {
  return createHash("sha256").update(RECEIPT_LEAF_DOMAIN, "ascii").update(receipt).digest("hex");
}

function accountDid(account: string): { readonly did: string; readonly asset: string | null } {
  if (!account.startsWith("agent:")) throw new WebSearchError("offer-account-mismatch");
  const tail = account.slice("agent:".length);
  if (tail.endsWith(":main")) return { did: tail.slice(0, -":main".length), asset: null };
  const marker = tail.lastIndexOf(":asset:");
  if (marker <= 0 || !hex32(tail.slice(marker + ":asset:".length))) throw new WebSearchError("offer-account-mismatch");
  return { did: tail.slice(0, marker), asset: tail.slice(marker + ":asset:".length) };
}

function derivedAccount(did: string, asset: string, nativeAsset: string, code: string): { readonly name: string; readonly id: string } {
  try {
    return { name: asset === nativeAsset ? `agent:${did}:main` : `agent:${did}:asset:${asset}`, id: walletAccount(did, asset, nativeAsset) };
  } catch {
    throw new WebSearchError(code);
  }
}

function errorOf(reply: Reply): string | undefined {
  try {
    const body = JSON.parse(reply.body.toString("utf8")) as unknown;
    const error = body !== null && typeof body === "object" && !Array.isArray(body) ? (body as Record<string, unknown>)["error"] : undefined;
    return typeof error === "string" && /^[a-z0-9_]{1,64}$/u.test(error) ? error : undefined;
  } catch {
    return undefined;
  }
}

function jsonBody(reply: Reply): Record<string, unknown> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(utf8(reply.body, "invalid-resource"));
  } catch {
    throw new WebSearchError("invalid-resource");
  }
  return record(parsed, "invalid-resource");
}

export class WebSearchClient {
  readonly #endpoint: URL;
  readonly #basePath: string;
  readonly #options: WebSearchClientOptions;
  readonly #pendingAttempts: number;
  readonly #now: () => bigint;

  public constructor(options: WebSearchClientOptions) {
    const endpoint = new URL(options.endpoint);
    if (!["https:", "http:"].includes(endpoint.protocol) || endpoint.username || endpoint.password || endpoint.search || endpoint.hash
      || (endpoint.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(endpoint.hostname))) throw new WebSearchError("invalid-endpoint");
    if (!/^layerx:[A-Za-z0-9._-]{1,64}$/u.test(options.network)) throw new WebSearchError("invalid-network");
    if (!hex32(options.nativeAsset) || options.nativeAsset === ZERO32) throw new WebSearchError("invalid-native-asset");
    for (const [currency, terms] of Object.entries(options.assets)) {
      if (!(WEB_SEARCH_CURRENCIES as readonly string[]).includes(currency) || terms === undefined
        || !hex32(terms.assetId) || terms.assetId === ZERO32 || terms.maxAmount <= 0n || terms.maxAmount >= 1n << 128n) throw new WebSearchError("invalid-asset-terms");
    }
    if (options.payer.preferences.length === 0) throw new WebSearchError("invalid-payer");
    const pendingAttempts = options.pendingAttempts ?? 5;
    if (!Number.isSafeInteger(pendingAttempts) || pendingAttempts < 1 || pendingAttempts > 60) throw new WebSearchError("invalid-pending-attempts");
    this.#basePath = endpoint.pathname.replace(/\/+$/u, "");
    this.#endpoint = endpoint;
    this.#options = options;
    this.#pendingAttempts = pendingAttempts;
    this.#now = options.now ?? (() => BigInt(Math.floor(Date.now() / 1000)));
  }

  public async search(query: string): Promise<WebSearchResponse> {
    if (typeof query !== "string" || query.length === 0) throw new WebSearchError("invalid-query");
    const { reply, settlement } = await this.#exchange(`/search?q=${encodeURIComponent(query)}`);
    const results = jsonBody(reply)["results"];
    if (!Array.isArray(results) || results.length > WEB_SEARCH_MAX_RESULTS) throw new WebSearchError("invalid-resource");
    return Object.freeze({
      results: Object.freeze(results.map((item: unknown) => {
        const result = record(item, "invalid-resource");
        exactKeys(result, ["url", "title", "snippet"], "invalid-resource");
        if (typeof result["url"] !== "string" || typeof result["title"] !== "string" || typeof result["snippet"] !== "string") throw new WebSearchError("invalid-resource");
        return Object.freeze({ url: result["url"], title: result["title"], snippet: result["snippet"] });
      })),
      settlement,
    });
  }

  public async fetch(url: string): Promise<WebFetchResponse> {
    if (typeof url !== "string" || url.length === 0) throw new WebSearchError("invalid-url");
    const { reply, settlement } = await this.#exchange(`/fetch?url=${encodeURIComponent(url)}`);
    const body = jsonBody(reply);
    const { final_url: finalUrl, media_type: mediaType, digest, length, text } = body;
    if (body["url"] !== url) throw new WebSearchError("content-url-mismatch");
    if (typeof finalUrl !== "string" || typeof mediaType !== "string" || !hex32(digest) || typeof text !== "string" || !Number.isSafeInteger(length)) throw new WebSearchError("invalid-resource");
    if (contentDigest(webContentBytes({ kind: WEB_CONTENT_FETCH, payload: url, mediaType, text })) !== digest) throw new WebSearchError("content-digest-mismatch");
    if (Buffer.byteLength(text, "utf8") !== length) throw new WebSearchError("content-length-mismatch");
    return Object.freeze({ url, finalUrl, mediaType, digest, text, settlement });
  }

  public async content(digest: string): Promise<WebContentResponse> {
    if (!hex32(digest)) throw new WebSearchError("invalid-digest");
    const { reply, settlement } = await this.#exchange(`/content/${digest}`);
    const canonical = new Uint8Array(reply.body);
    if (contentDigest(canonical) !== digest) throw new WebSearchError("content-digest-mismatch");
    return Object.freeze({ digest, canonical, content: decodeWebContent(canonical), settlement });
  }

  async #exchange(target: string): Promise<{ readonly reply: Reply; readonly settlement: WebSettlement | null }> {
    const base: Record<string, string> = {};
    if (this.#options.payer.did !== undefined) base[WEB_SEARCH_PAYER_HEADER] = this.#options.payer.did;
    const challenge = await this.#get(target, base);
    if (challenge.status === 200) return { reply: challenge, settlement: null };
    if (challenge.status !== 402) throw new WebSearchError(`sidecar-refused:${errorOf(challenge) ?? challenge.status}`, challenge.status);
    const [offer, raw] = this.#select(decodeHeader(challenge.headers["payment-required"], "invalid-payment-required"));
    const payment = await this.#prepare(offer, raw, target);
    for (let attempt = 1; ; attempt += 1) {
      const reply = await this.#get(target, { ...base, "PAYMENT-SIGNATURE": payment.header });
      if (reply.status === 503 && errorOf(reply) === "payment_pending") {
        if (attempt >= this.#pendingAttempts) throw new WebSearchError("payment-pending", 503);
        const retry = reply.headers["retry-after"];
        if (retry === undefined || !/^[0-9]{1,2}$/u.test(retry) || Number(retry) > MAX_RETRY_AFTER_SECONDS) throw new WebSearchError("invalid-retry-after", 503);
        await new Promise((resolve) => setTimeout(resolve, Number(retry) * 1000));
        continue;
      }
      if (reply.status !== 200) throw new WebSearchError(`payment-refused:${errorOf(reply) ?? reply.status}`, reply.status);
      return { reply, settlement: await this.#settle(reply.headers["payment-response"], offer, payment) };
    }
  }

  #select(required: Record<string, unknown>): readonly [WebSearchOffer, Record<string, unknown>] {
    if (required["x402Version"] !== 2) throw new WebSearchError("invalid-payment-required");
    const accepts = required["accepts"];
    if (!Array.isArray(accepts) || accepts.length === 0 || accepts.length > 32) throw new WebSearchError("invalid-payment-required");
    for (const preference of this.#options.payer.preferences) {
      const matching = accepts.map((offer: unknown) => record(offer, "invalid-payment-required")).filter((offer) => {
        const extra = offer["extra"];
        const layerx = extra !== null && typeof extra === "object" ? (extra as Record<string, unknown>)["layerx"] : undefined;
        return offer["scheme"] === preference.scheme && layerx !== null && typeof layerx === "object"
          && (layerx as Record<string, unknown>)["currency"] === preference.currency;
      });
      if (matching.length > 1) throw new WebSearchError("ambiguous-offer");
      const raw = matching[0];
      if (raw !== undefined) return [this.#offer(raw, preference), raw];
    }
    throw new WebSearchError("no-acceptable-offer");
  }

  #offer(raw: Record<string, unknown>, preference: WebSearchPreference): WebSearchOffer {
    exactKeys(raw, ["scheme", "network", "amount", "asset", "payTo", "maxTimeoutSeconds", "extra"], "invalid-offer");
    const extra = record(raw["extra"], "invalid-offer");
    exactKeys(extra, ["layerx"], "invalid-offer");
    const layerx = record(extra["layerx"], "invalid-offer");
    const metered = preference.scheme === "metered";
    exactKeys(layerx, metered ? ["account", "commitment", "currency", "payer", "purposeHash"] : ["account", "commitment", "currency"], "invalid-offer");
    const { network, amount, asset, payTo, maxTimeoutSeconds } = raw;
    const account = layerx["account"];
    if (typeof network !== "string" || typeof amount !== "string" || !AMOUNT.test(amount) || BigInt(amount) >= 1n << 128n || !hex32(asset) || !hex32(payTo)
      || typeof maxTimeoutSeconds !== "number" || !Number.isSafeInteger(maxTimeoutSeconds) || maxTimeoutSeconds <= 0 || typeof account !== "string") throw new WebSearchError("invalid-offer");
    if (network !== this.#options.network) throw new WebSearchError("offer-network-mismatch");
    if (layerx["commitment"] !== "executed") throw new WebSearchError("unsupported-commitment");
    const terms = this.#options.assets[preference.currency];
    if (terms === undefined) throw new WebSearchError("asset-not-configured");
    if (asset !== terms.assetId) throw new WebSearchError("offer-asset-mismatch");
    if (BigInt(amount) > terms.maxAmount) throw new WebSearchError("offer-price-exceeded");
    const receiver = accountDid(account);
    const expected = derivedAccount(receiver.did, asset, this.#options.nativeAsset, "offer-account-mismatch");
    if (account !== expected.name || payTo !== expected.id) throw new WebSearchError("offer-account-mismatch");
    const offer = { scheme: preference.scheme, network, amount, asset, payTo, maxTimeoutSeconds, currency: preference.currency, account };
    if (!metered) return Object.freeze(offer);
    const { payer, purposeHash } = layerx;
    if (!hex32(purposeHash) || purposeHash === ZERO32 || !hex32(payer)) throw new WebSearchError("invalid-offer");
    const did = this.#options.payer.did;
    if (did === undefined || payer !== derivedAccount(did, asset, this.#options.nativeAsset, "offer-payer-mismatch").id) throw new WebSearchError("offer-payer-mismatch");
    return Object.freeze({ ...offer, payer, purposeHash });
  }

  async #prepare(offer: WebSearchOffer, raw: Record<string, unknown>, target: string): Promise<PreparedPayment> {
    const payer = this.#options.payer;
    if (offer.scheme === "metered") {
      if (payer.metered === undefined) throw new WebSearchError("metered-payer-unavailable");
      const { grant: canonical, idempotencyKey } = await payer.metered(offer, target);
      const grant = decodePayerGrant(canonical);
      const amount = BigInt(offer.amount);
      if (grant.from !== offer.payer || grant.recipient !== offer.payTo || grant.asset !== offer.asset || grant.purpose_hash !== offer.purposeHash
        || grant.recurring || grant.window_length !== "0" || grant.has_reference || grant.reference_hash !== ZERO32
        || BigInt(grant.per_draw_maximum) < amount || BigInt(grant.allowance) < amount || this.#now() >= BigInt(grant.expiration)) throw new WebSearchError("grant-offer-mismatch");
      if (!hex32(idempotencyKey) || idempotencyKey === ZERO32) throw new WebSearchError("invalid-idempotency-key");
      return { header: encodeHeader({ x402Version: 2, accepted: raw, payload: { grant: hex(canonical), idempotencyKey } }) };
    }
    if (payer.exact === undefined) throw new WebSearchError("exact-payer-unavailable");
    const { receipt } = await payer.exact(offer, target);
    await this.#verify(receipt, offer);
    const payload = { receipt: Buffer.from(receipt).toString("base64"), receiptDigest: receiptDigest(receipt), verificationLevel: "sequencer-signed" };
    return { header: encodeHeader({ x402Version: 2, accepted: raw, payload }), receipt: new Uint8Array(receipt) };
  }

  async #verify(receipt: Uint8Array, offer: WebSearchOffer): Promise<ReceiptVerification> {
    let verified: ReceiptVerification;
    try {
      const authority = await this.#options.authority(receipt, offer);
      verified = await verifyReceipt(receipt, authority, this.#options.protocolVersion === undefined ? undefined : { protocolVersion: this.#options.protocolVersion });
    } catch {
      throw new WebSearchError("receipt-unverified");
    }
    if (hex(verified.receipt.asset) !== offer.asset || verified.receipt.amount !== BigInt(offer.amount) || hex(verified.receipt.to) !== offer.payTo) throw new WebSearchError("receipt-offer-mismatch");
    return verified;
  }

  async #settle(header: string | undefined, offer: WebSearchOffer, payment: PreparedPayment): Promise<WebSettlement> {
    if (header === undefined) throw new WebSearchError("missing-payment-response");
    const settlement = decodeHeader(header, "invalid-payment-response");
    const extensions = record(settlement["extensions"], "invalid-payment-response");
    const layerx = record(extensions["layerx"], "invalid-payment-response");
    if (settlement["success"] !== true || settlement["network"] !== offer.network || settlement["amount"] !== offer.amount
      || layerx["verificationLevel"] !== "sequencer-signed" || typeof layerx["receipt"] !== "string") throw new WebSearchError("settlement-mismatch");
    const encoded = layerx["receipt"];
    const receipt = new Uint8Array(Buffer.from(encoded, "base64"));
    if (Buffer.from(receipt).toString("base64") !== encoded) throw new WebSearchError("settlement-receipt-mismatch");
    const digest = receiptDigest(receipt);
    if (layerx["receiptDigest"] !== digest || settlement["transaction"] !== `lxp:${digest}`) throw new WebSearchError("settlement-receipt-mismatch");
    if (payment.receipt !== undefined && hex(payment.receipt) !== hex(receipt)) throw new WebSearchError("settlement-receipt-mismatch");
    const verified = await this.#verify(receipt, offer);
    const from = hex(verified.receipt.from);
    if (settlement["payer"] !== from) throw new WebSearchError("settlement-payer-mismatch");
    if (offer.scheme === "metered") {
      if (verified.receipt.moduleId !== 1 || verified.receipt.operation !== 6) throw new WebSearchError("settlement-operation-mismatch");
      if (from !== offer.payer) throw new WebSearchError("settlement-payer-mismatch");
      if (layerx["purposeHash"] !== offer.purposeHash) throw new WebSearchError("settlement-purpose-mismatch");
    }
    return Object.freeze({
      scheme: offer.scheme, currency: offer.currency, network: offer.network, asset: offer.asset, amount: offer.amount,
      payer: from, receiptDigest: digest, transaction: `lxp:${digest}`,
    });
  }

  async #get(target: string, headers: Readonly<Record<string, string>>): Promise<Reply> {
    const url = new URL(this.#endpoint.toString());
    const [path, query] = target.split("?", 2) as [string, string | undefined];
    url.pathname = `${this.#basePath}${path}`;
    if (query !== undefined) url.search = `?${query}`;
    return new Promise<Reply>((resolve, reject) => {
      const request = (url.protocol === "https:" ? https : http).request(url, { method: "GET", headers: { ...headers }, agent: false }, (response) => {
        const chunks: Buffer[] = [];
        let size = 0;
        response.on("data", (chunk: Buffer) => {
          size += chunk.length;
          if (size > MAX_REPLY_BYTES) { request.destroy(new WebSearchError("reply-too-large")); return; }
          chunks.push(chunk);
        });
        response.on("error", reject);
        response.on("end", () => {
          const collected: Record<string, string> = {};
          for (const [name, value] of Object.entries(response.headers)) if (typeof value === "string") collected[name.toLowerCase()] = value;
          resolve({ status: response.statusCode ?? 0, headers: collected, body: Buffer.concat(chunks) });
        });
      });
      const timer = setTimeout(() => request.destroy(new WebSearchError("sidecar-deadline-exceeded")), 30000);
      request.on("close", () => clearTimeout(timer));
      request.on("error", reject);
      request.end();
    });
  }
}
