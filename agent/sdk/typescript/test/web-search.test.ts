import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { once } from "node:events";
import type { AddressInfo } from "node:net";
import { isDeepStrictEqual } from "node:util";

import {
  WebSearchClient,
  WebSearchError,
  contentDigest,
  decodePayerGrant,
  decodeWebContent,
  encodeGrant,
  webContentBytes,
  type AuthorizedReceiptBatch,
  type WebSearchCurrency,
  type WebSearchOffer,
  type WebSearchPayer,
} from "../src/index.js";

type Json = Record<string, unknown>;
interface Step {
  readonly request: { readonly method: string; readonly target: string; readonly headers: Json };
  readonly response: { readonly status: number; readonly headers: Json; readonly body?: unknown; readonly bodyBase64?: string };
}
interface Vector { readonly payload: string; readonly media_type: string; readonly text: string; readonly digest: string }

const fixture = (path: string): Json => JSON.parse(readFileSync(new URL(`../../../../../interop/crates/x-websearch/tests/fixtures/${path}`, import.meta.url), "utf8")) as Json;
const exchange = (fixture("client-exchange.json")["exchange"] as Step[]);
const vectors = (fixture("content-vectors.json")["vectors"] as Vector[]);
const buyer = fixture("gateway/buyer.json");
const config = fixture("config/valid.json");
const configured = config["assets"] as Record<WebSearchCurrency, { asset_id: string; price: string }>;
const trustedKey = buyer["sequencerPublicKey"] as string;
const untrustedKey = (config["gateway"] as Json)["sequencer_public_key"] as string;
const payerDid = buyer["payerDid"] as string;
const currencies: readonly WebSearchCurrency[] = ["SID", "PAX", "USDC", "USDL"];
const bytes = (hex: string): Uint8Array => new Uint8Array(Buffer.from(hex, "hex"));
const base64 = (value: string): Uint8Array => new Uint8Array(Buffer.from(value, "base64"));
const clone = (): Step[] => JSON.parse(JSON.stringify(exchange)) as Step[];
const signature = (step: Step): Json => step.request.headers["PAYMENT-SIGNATURE"] as Json;
const payload = (step: Step): Json => signature(step)["payload"] as Json;
const settlementOf = (step: Step): Json => step.response.headers["PAYMENT-RESPONSE"] as Json;

// The recording: a SID metered search, an exact fetch paid in each asset of one committed page, and the stored content.
assert.equal(exchange.length, 11);
const searchPaid = exchange[1]!;
const fetchIndex = (currency: WebSearchCurrency): number => 3 + 2 * currencies.indexOf(currency);
const fetchUrl = (currency: WebSearchCurrency): string => vectors[currencies.indexOf(currency)]!.payload;
assert.equal(exchange[0]!.request.target, "/search?q=paxeer");
for (const currency of currencies) {
  assert.equal(exchange[fetchIndex(currency)]!.request.target, `/fetch?url=${encodeURIComponent(fetchUrl(currency))}`);
}
assert.equal(exchange[10]!.request.target, `/content/${vectors[0]!.digest}`);
const usdcPaid = fetchIndex("USDC");

function batchFacts(receipt: Uint8Array, sequencerPublicKey: string): AuthorizedReceiptBatch {
  const view = Buffer.from(receipt);
  let offset = 6;
  const bounded = (): Uint8Array => {
    const length = view.readUInt32BE(offset);
    offset += 4;
    const field = new Uint8Array(view.subarray(offset, offset + length));
    offset += length;
    return field;
  };
  bounded();
  offset += 8;
  const previousStateRoot = bounded();
  const resultingStateRoot = bounded();
  bounded();
  offset += 4;
  const effects = view.readUInt32BE(offset);
  offset += 4;
  for (let index = 0; index < effects; index += 1) { offset += 8; bounded(); bounded(); }
  offset += 16;
  const batchId = bounded();
  offset += 11;
  const asset = bounded();
  return { batchId, asset, previousStateRoot, resultingStateRoot, sequencerPublicKey: bytes(sequencerPublicKey) };
}

interface Replay {
  readonly endpoint: string;
  readonly served: number[];
  readonly unrecorded: Json[];
  close(): Promise<void>;
}

function matches(step: Step, request: IncomingMessage): boolean {
  if (request.method !== step.request.method || request.url !== step.request.target) return false;
  for (const name of ["LAYERX-PAYER-DID", "PAYMENT-SIGNATURE"]) {
    const sent = request.headers[name.toLowerCase()];
    const recorded = step.request.headers[name];
    if ((sent === undefined) !== (recorded === undefined)) return false;
    if (recorded === undefined) continue;
    if (typeof sent !== "string") return false;
    if (name === "PAYMENT-SIGNATURE") {
      if (!isDeepStrictEqual(JSON.parse(Buffer.from(sent, "base64").toString("utf8")), recorded)) return false;
    } else if (sent !== recorded) return false;
  }
  return true;
}

async function replay(steps: readonly Step[], content: ReadonlyMap<string, Uint8Array> = new Map()): Promise<Replay> {
  const served: number[] = [];
  const unrecorded: Json[] = [];
  const server = createServer((request: IncomingMessage, response: ServerResponse) => {
    const index = steps.findIndex((step, position) => !served.includes(position) && matches(step, request));
    const step = steps[index];
    if (step === undefined) {
      const stored = content.get(request.url ?? "");
      if (stored !== undefined) {
        response.writeHead(200, { "content-type": "application/octet-stream", "content-length": stored.length });
        response.end(stored);
        return;
      }
      unrecorded.push({ target: request.url, headers: request.headers });
      const body = JSON.stringify({ error: "unrecorded_request" });
      response.writeHead(400, { "content-type": "application/json", "content-length": Buffer.byteLength(body) });
      response.end(body);
      return;
    }
    served.push(index);
    const headers: Record<string, string | number> = { "content-type": "application/json" };
    for (const [name, value] of Object.entries(step.response.headers)) {
      headers[name] = typeof value === "string" ? value : Buffer.from(JSON.stringify(value)).toString("base64");
    }
    const body = step.response.bodyBase64 === undefined ? Buffer.from(JSON.stringify(step.response.body)) : Buffer.from(step.response.bodyBase64, "base64");
    if (step.response.bodyBase64 !== undefined) headers["content-type"] = "application/octet-stream";
    headers["content-length"] = body.length;
    response.writeHead(step.response.status, headers);
    response.end(body);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address() as AddressInfo;
  return {
    endpoint: `http://127.0.0.1:${port}`,
    served,
    unrecorded,
    close: async () => { server.close(); await once(server, "close"); },
  };
}

function client(endpoint: string, payer: WebSearchPayer, key = trustedKey): WebSearchClient {
  const assets = Object.fromEntries(currencies.map((currency) => [currency, { assetId: configured[currency].asset_id, maxAmount: BigInt(configured[currency].price) }]));
  return new WebSearchClient({
    endpoint,
    network: "layerx:1",
    nativeAsset: configured.PAX.asset_id,
    assets,
    payer,
    authority: async (receipt: Uint8Array) => batchFacts(receipt, key),
    protocolVersion: 3,
    now: () => 1_000_000_000n,
  });
}

async function refused(action: Promise<unknown>, code: string): Promise<void> {
  await assert.rejects(action, (error: unknown) => {
    assert.ok(error instanceof WebSearchError, String(error));
    assert.equal(error.code, code);
    return true;
  });
}

const meteredSid: WebSearchPayer = {
  did: payerDid,
  preferences: [{ currency: "SID", scheme: "metered" }],
  metered: async () => ({ grant: bytes(payload(searchPaid)["grant"] as string), idempotencyKey: payload(searchPaid)["idempotencyKey"] as string }),
};
const exactIn = (currency: WebSearchCurrency): WebSearchPayer => ({
  preferences: [{ currency, scheme: "exact" }],
  exact: async () => ({ receipt: base64(payload(exchange[fetchIndex(currency)]!)["receipt"] as string) }),
});
const exactUsdc = exactIn("USDC");

// The content digest of every committed vector.
for (const vector of vectors) {
  const canonical = webContentBytes({ kind: 1, payload: vector.payload, mediaType: vector.media_type, text: vector.text });
  assert.equal(contentDigest(canonical), vector.digest, vector.payload);
  const decoded = decodeWebContent(canonical);
  assert.equal(decoded.kind, 1);
  assert.equal(Buffer.from(decoded.payload).toString("utf8"), vector.payload);
  assert.equal(decoded.mediaType, vector.media_type);
  assert.equal(decoded.text, vector.text);
  assert.throws(() => decodeWebContent(canonical.subarray(0, canonical.length - 1)), WebSearchError);
}
assert.throws(() => webContentBytes({ kind: 1, payload: "x", mediaType: "Text/HTML; charset=utf-8", text: "" }), WebSearchError);

// The payer grants round-trip through the SDK's grant codec.
for (const currency of currencies) {
  const canonical = bytes((buyer["grants"] as Json)[currency] as string);
  assert.deepEqual(encodeGrant(decodePayerGrant(canonical)), canonical);
}

// Exact in every asset, PAX into the main accounts: each recorded payment is byte-for-byte the client's, the settlement
// verifies and the fetched text matches its digest.
for (const [position, currency] of currencies.entries()) {
  const paid = fetchIndex(currency);
  const vector = vectors[position]!;
  const sidecar = await replay(clone());
  const fetched = await client(sidecar.endpoint, exactIn(currency)).fetch(vector.payload);
  assert.deepEqual(sidecar.served, [paid - 1, paid]);
  assert.deepEqual(sidecar.unrecorded, []);
  assert.equal(fetched.url, vector.payload);
  assert.equal(fetched.text, vector.text);
  assert.equal(fetched.mediaType, vector.media_type);
  assert.equal(fetched.digest, vector.digest);
  const settlement = settlementOf(exchange[paid]!);
  assert.deepEqual(fetched.settlement, {
    scheme: "exact", currency, network: "layerx:1", asset: configured[currency].asset_id, amount: configured[currency].price,
    payer: settlement["payer"], receiptDigest: (settlement["extensions"] as { layerx: Json }).layerx["receiptDigest"], transaction: settlement["transaction"],
  });
  await sidecar.close();
}

// SID metered: the client's payment is byte-for-byte the recorded one, the settlement repeats the challenge's
// purposeHash and the search results are released.
{
  const sidecar = await replay(clone());
  const found = await client(sidecar.endpoint, meteredSid).search("paxeer");
  assert.deepEqual(sidecar.served, [0, 1]);
  assert.deepEqual(sidecar.unrecorded, []);
  assert.deepEqual(found.results, (searchPaid.response.body as Json)["results"]);
  const settlement = settlementOf(searchPaid);
  const layerx = (settlement["extensions"] as { layerx: Json }).layerx;
  const offer = ((exchange[0]!.response.headers["PAYMENT-REQUIRED"] as Json)["accepts"] as Json[])
    .find((candidate) => candidate["scheme"] === "metered" && ((candidate["extra"] as Json)["layerx"] as Json)["currency"] === "SID")!;
  assert.equal(layerx["purposeHash"], ((offer["extra"] as Json)["layerx"] as Json)["purposeHash"]);
  assert.deepEqual(found.settlement, {
    scheme: "metered", currency: "SID", network: "layerx:1", asset: configured.SID.asset_id, amount: configured.SID.price,
    payer: settlement["payer"], receiptDigest: layerx["receiptDigest"], transaction: settlement["transaction"],
  });
  await sidecar.close();
}
for (const purposeHash of [undefined, "44".repeat(32)]) {
  const steps = clone();
  const layerx = (settlementOf(steps[1]!)["extensions"] as { layerx: Json }).layerx;
  if (purposeHash === undefined) delete layerx["purposeHash"]; else layerx["purposeHash"] = purposeHash;
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, meteredSid).search("paxeer"), "settlement-purpose-mismatch");
  assert.deepEqual(sidecar.served, [0, 1]);
  await sidecar.close();
}

// Refused settlements release nothing to the caller.
for (const [mutate, code] of [
  [(settlement: Json) => { settlement["payer"] = settlementOf(exchange[fetchIndex("SID")]!)["payer"]; }, "settlement-payer-mismatch"],
  [(settlement: Json) => { settlement["success"] = false; }, "settlement-mismatch"],
  [(settlement: Json) => { settlement["amount"] = "1"; }, "settlement-mismatch"],
  [(settlement: Json) => { settlement["transaction"] = `lxp:${"11".repeat(32)}`; }, "settlement-receipt-mismatch"],
  [(settlement: Json) => {
    const other = (buyer["exact"] as Record<string, Json>)["SID"]!;
    settlement["extensions"] = { layerx: { receipt: other["receipt"], receiptDigest: other["receiptDigest"], verificationLevel: "sequencer-signed" } };
    settlement["transaction"] = `lxp:${other["receiptDigest"] as string}`;
  }, "settlement-receipt-mismatch"],
] as const) {
  const steps = clone();
  mutate(settlementOf(steps[usdcPaid]!));
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch(fetchUrl("USDC")), code);
  assert.deepEqual(sidecar.served, [usdcPaid - 1, usdcPaid]);
  await sidecar.close();
}
{
  const steps = clone();
  delete steps[usdcPaid]!.response.headers["PAYMENT-RESPONSE"];
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch(fetchUrl("USDC")), "missing-payment-response");
  await sidecar.close();
}

// A receipt the configured sequencer key did not sign is never presented as payment.
{
  const sidecar = await replay(clone());
  await refused(client(sidecar.endpoint, exactUsdc, untrustedKey).fetch(fetchUrl("USDC")), "receipt-unverified");
  assert.deepEqual(sidecar.served, [usdcPaid - 1]);
  await sidecar.close();
}

// A fetched text or a stored content whose digest does not match is refused; the recorded content is served unpaid.
{
  const steps = clone();
  const body = steps[usdcPaid]!.response.body as Json;
  body["text"] = `${body["text"] as string} altered`;
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch(fetchUrl("USDC")), "content-digest-mismatch");
  assert.deepEqual(sidecar.served, [usdcPaid - 1, usdcPaid]);
  await sidecar.close();
}
{
  const [first, second] = vectors as [Vector, Vector];
  const recorded = base64(exchange[10]!.response.bodyBase64!);
  assert.equal(contentDigest(recorded), first.digest);
  const stored = new Map<string, Uint8Array>([[`/content/${second.digest}`, recorded]]);
  const sidecar = await replay(clone(), stored);
  const content = await client(sidecar.endpoint, exactUsdc).content(first.digest);
  assert.deepEqual(sidecar.served, [10]);
  assert.equal(content.digest, first.digest);
  assert.equal(content.content.text, first.text);
  assert.equal(Buffer.from(content.content.payload).toString("utf8"), first.payload);
  assert.equal(content.settlement, null);
  await refused(client(sidecar.endpoint, exactUsdc).content(second.digest), "content-digest-mismatch");
  await refused(client(sidecar.endpoint, exactUsdc).content("zz"), "invalid-digest");
  await sidecar.close();
}

// Every recorded offer is checked against the configured asset, price, receiver account and payer before paying:
// PAX is paid from and into the main accounts, every other asset from and into its own.
for (const currency of currencies) {
  const offers: WebSearchOffer[] = [];
  const payer: WebSearchPayer = {
    did: payerDid,
    preferences: [{ currency, scheme: "metered" }],
    metered: async (offer) => { offers.push(offer); return { grant: bytes((buyer["grants"] as Json)[currency] as string), idempotencyKey: "22".repeat(32) }; },
  };
  const sidecar = await replay(clone());
  await refused(client(sidecar.endpoint, payer).search("paxeer"), "payment-refused:unrecorded_request");
  assert.equal(offers.length, 1);
  const offer = offers[0]!;
  assert.equal(offer.asset, configured[currency].asset_id);
  assert.equal(offer.amount, configured[currency].price);
  const suffix = currency === "PAX" ? ":main" : `:asset:${configured[currency].asset_id}`;
  assert.ok(offer.account.endsWith(suffix), offer.account);
  const sent = JSON.parse(Buffer.from((sidecar.unrecorded[0]!["headers"] as Json)["payment-signature"] as string, "base64").toString("utf8")) as Json;
  assert.equal((sent["accepted"] as Json)["payTo"], offer.payTo);
  await sidecar.close();
}
{
  const steps = clone();
  const paxExact = ((steps[fetchIndex("PAX") - 1]!.response.headers["PAYMENT-REQUIRED"] as Json)["accepts"] as Json[])
    .find((offer) => offer["scheme"] === "exact" && ((offer["extra"] as Json)["layerx"] as Json)["currency"] === "PAX")!;
  const layerx = (paxExact["extra"] as Json)["layerx"] as Json;
  layerx["account"] = (layerx["account"] as string).replace(/:main$/, `:asset:${configured.PAX.asset_id}`);
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactIn("PAX")).fetch(fetchUrl("PAX")), "offer-account-mismatch");
  assert.deepEqual(sidecar.unrecorded, []);
  await sidecar.close();
}

// A grant for another purpose, a payer the offer does not name and an offer above the configured price are refused unpaid.
{
  const sidecar = await replay(clone());
  const otherPurpose: WebSearchPayer = { ...meteredSid, metered: async () => ({ grant: bytes((buyer["refusedGrants"] as Json)["otherPurpose"] as string), idempotencyKey: "33".repeat(32) }) };
  await refused(client(sidecar.endpoint, otherPurpose).search("paxeer"), "grant-offer-mismatch");
  assert.deepEqual(sidecar.unrecorded, []);
  await sidecar.close();
}
{
  const sidecar = await replay(clone());
  const steps = clone();
  const sidMetered = ((steps[0]!.response.headers["PAYMENT-REQUIRED"] as Json)["accepts"] as Json[])
    .find((offer) => offer["scheme"] === "metered" && ((offer["extra"] as Json)["layerx"] as Json)["currency"] === "SID")!;
  const layerx = (sidMetered["extra"] as Json)["layerx"] as Json;
  layerx["payer"] = "44".repeat(32);
  const tampered = await replay(steps);
  await refused(client(tampered.endpoint, meteredSid).search("paxeer"), "offer-payer-mismatch");
  await refused(new WebSearchClient({
    endpoint: sidecar.endpoint, network: "layerx:1", nativeAsset: configured.PAX.asset_id,
    assets: { SID: { assetId: configured.SID.asset_id, maxAmount: BigInt(configured.SID.price) - 1n } },
    payer: meteredSid, authority: async (receipt: Uint8Array) => batchFacts(receipt, trustedKey), protocolVersion: 3, now: () => 1_000_000_000n,
  }).search("paxeer"), "offer-price-exceeded");
  await refused(client(sidecar.endpoint, { preferences: [{ currency: "SID", scheme: "metered" }] }).fetch(fetchUrl("SID")), "no-acceptable-offer");
  assert.deepEqual(sidecar.served, [0, 2]);
  assert.deepEqual(tampered.unrecorded, []);
  await tampered.close();
  await sidecar.close();
}

assert.throws(() => client("http://example.com", exactUsdc), WebSearchError);
console.log("web search client tests passed");
