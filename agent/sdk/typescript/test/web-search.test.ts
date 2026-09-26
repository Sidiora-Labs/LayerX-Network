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
  readonly response: { readonly status: number; readonly headers: Json; readonly body: unknown };
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

assert.equal(exchange.length, 4);
const [searchChallenge, searchPaid, fetchChallenge, fetchPaid] = exchange as [Step, Step, Step, Step];
assert.equal(searchChallenge.request.target, "/search?q=paxeer");
assert.equal(fetchPaid.request.target, "/fetch?url=paxeer");

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
    const body = JSON.stringify(step.response.body);
    headers["content-length"] = Buffer.byteLength(body);
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

function fetchBody(url: string, vector: Vector, text = vector.text): Json {
  const digest = contentDigest(webContentBytes({ kind: 1, payload: url, mediaType: vector.media_type, text: vector.text }));
  return { url, final_url: url, media_type: vector.media_type, digest, length: Buffer.byteLength(text, "utf8"), text };
}

const meteredSid: WebSearchPayer = {
  did: payerDid,
  preferences: [{ currency: "SID", scheme: "metered" }],
  metered: async () => ({ grant: bytes(payload(searchPaid)["grant"] as string), idempotencyKey: payload(searchPaid)["idempotencyKey"] as string }),
};
const exactUsdc: WebSearchPayer = {
  preferences: [{ currency: "USDC", scheme: "exact" }],
  exact: async () => ({ receipt: base64(payload(fetchPaid)["receipt"] as string) }),
};

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

// USDC exact: the recorded exchange settles, the settlement verifies and the fetched text matches its digest.
{
  const steps = clone();
  steps[3] = { ...steps[3]!, response: { ...steps[3]!.response, body: fetchBody("paxeer", vectors[0]!) } };
  const sidecar = await replay(steps);
  const fetched = await client(sidecar.endpoint, exactUsdc).fetch("paxeer");
  assert.deepEqual(sidecar.served, [2, 3]);
  assert.equal(fetched.text, vectors[0]!.text);
  assert.equal(fetched.mediaType, vectors[0]!.media_type);
  assert.equal(fetched.digest, (steps[3]!.response.body as Json)["digest"]);
  const settlement = settlementOf(fetchPaid);
  assert.deepEqual(fetched.settlement, {
    scheme: "exact", currency: "USDC", network: "layerx:1", asset: configured.USDC.asset_id, amount: configured.USDC.price,
    payer: settlement["payer"], receiptDigest: (settlement["extensions"] as { layerx: Json }).layerx["receiptDigest"], transaction: settlement["transaction"],
  });
  await sidecar.close();
}

// SID metered: the client's payment is byte-for-byte the recorded one and the sidecar settles it, but the recorded
// settlement does not repeat the challenge's purposeHash, so the client refuses the resource.
{
  const sidecar = await replay(clone());
  await refused(client(sidecar.endpoint, meteredSid).search("paxeer"), "settlement-purpose-mismatch");
  assert.deepEqual(sidecar.served, [0, 1]);
  assert.deepEqual(sidecar.unrecorded, []);
  await sidecar.close();
}

// Refused settlements release nothing to the caller.
for (const [mutate, code] of [
  [(settlement: Json) => { settlement["payer"] = settlementOf(searchPaid)["payer"]; }, "settlement-payer-mismatch"],
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
  mutate(settlementOf(steps[3]!));
  steps[3] = { ...steps[3]!, response: { ...steps[3]!.response, body: fetchBody("paxeer", vectors[0]!) } };
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch("paxeer"), code);
  assert.deepEqual(sidecar.served, [2, 3]);
  await sidecar.close();
}
{
  const steps = clone();
  delete steps[3]!.response.headers["PAYMENT-RESPONSE"];
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch("paxeer"), "missing-payment-response");
  await sidecar.close();
}

// A receipt the configured sequencer key did not sign is never presented as payment.
{
  const sidecar = await replay(clone());
  await refused(client(sidecar.endpoint, exactUsdc, untrustedKey).fetch("paxeer"), "receipt-unverified");
  assert.deepEqual(sidecar.served, [2]);
  await sidecar.close();
}

// A fetched text or a stored content whose digest does not match is refused.
{
  const steps = clone();
  steps[3] = { ...steps[3]!, response: { ...steps[3]!.response, body: fetchBody("paxeer", vectors[0]!, `${vectors[0]!.text} altered`) } };
  const sidecar = await replay(steps);
  await refused(client(sidecar.endpoint, exactUsdc).fetch("paxeer"), "content-digest-mismatch");
  assert.deepEqual(sidecar.served, [2, 3]);
  await sidecar.close();
}
{
  const stored = new Map<string, Uint8Array>();
  for (const vector of vectors) stored.set(`/content/${vector.digest}`, webContentBytes({ kind: 1, payload: vector.payload, mediaType: vector.media_type, text: vector.text }));
  const [first, second] = vectors as [Vector, Vector];
  stored.set(`/content/${second.digest}`, stored.get(`/content/${first.digest}`)!);
  const sidecar = await replay([], stored);
  const content = await client(sidecar.endpoint, exactUsdc).content(first.digest);
  assert.equal(content.digest, first.digest);
  assert.equal(content.content.text, first.text);
  assert.equal(content.settlement, null);
  await refused(client(sidecar.endpoint, exactUsdc).content(second.digest), "content-digest-mismatch");
  await refused(client(sidecar.endpoint, exactUsdc).content("zz"), "invalid-digest");
  await sidecar.close();
}

// Every recorded offer is checked against the configured asset, price, receiver account and payer before paying.
for (const currency of currencies) {
  for (const scheme of ["metered", "exact"] as const) {
    if (currency === "USDC" && scheme === "exact") continue;
    const offers: WebSearchOffer[] = [];
    const payer: WebSearchPayer = {
      ...(scheme === "metered" ? { did: payerDid } : {}),
      preferences: [{ currency, scheme }],
      metered: async (offer) => { offers.push(offer); return { grant: bytes((buyer["grants"] as Json)[currency] as string), idempotencyKey: "22".repeat(32) }; },
      exact: async (offer) => { offers.push(offer); return { receipt: base64((buyer["exact"] as Record<string, Json>)[currency]!["receipt"] as string) }; },
    };
    const sidecar = await replay(clone());
    const action = scheme === "metered" ? client(sidecar.endpoint, payer).search("paxeer") : client(sidecar.endpoint, payer).fetch("paxeer");
    if (currency === "PAX") {
      await refused(action, "offer-account-mismatch");
      assert.deepEqual(offers, []);
    } else {
      await refused(action, "payment-refused:unrecorded_request");
      assert.equal(offers.length, 1);
      assert.equal(offers[0]!.asset, configured[currency].asset_id);
      assert.equal(offers[0]!.amount, configured[currency].price);
      const sent = JSON.parse(Buffer.from((sidecar.unrecorded[0]!["headers"] as Json)["payment-signature"] as string, "base64").toString("utf8")) as Json;
      assert.equal((sent["accepted"] as Json)["payTo"], offers[0]!.payTo);
    }
    await sidecar.close();
  }
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
  await refused(client(sidecar.endpoint, { preferences: [{ currency: "SID", scheme: "metered" }] }).fetch("paxeer"), "no-acceptable-offer");
  assert.deepEqual(sidecar.served, [0, 2]);
  assert.deepEqual(tampered.unrecorded, []);
  await tampered.close();
  await sidecar.close();
}

assert.throws(() => client("http://example.com", exactUsdc), WebSearchError);
console.log("web search client tests passed");
