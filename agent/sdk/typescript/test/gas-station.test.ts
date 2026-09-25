import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { once } from "node:events";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { keccak_256 } from "@noble/hashes/sha3.js";

import {
  SIDIORA_TOKEN,
  SIDIORA_DECIMALS,
  abiSelector,
  assembleEip7702Authorization,
  eip7702AuthorizationDigest,
  gasQuoteDigest,
  requestGasQuote,
  sponsoredBatchCall,
  sponsoredBatchDigest,
  type GasQuote,
  type GasRefusalCode,
  type GasResult,
  type GasStationConfig,
  type SponsoredBatch,
} from "../src/index.js";

function value<T>(result: GasResult<T>): T {
  assert.equal(result.ok, true, JSON.stringify(result, (_key, item: unknown) => typeof item === "bigint" ? item.toString() : item));
  return result.value;
}

function refused(result: GasResult<unknown>, code: GasRefusalCode): void {
  assert.equal(result.ok, false);
  if (result.ok) throw new Error("expected refusal");
  assert.equal(result.refusal.code, code);
}

const quote: GasQuote = {
  sponsor: `0x${"22".repeat(20)}`,
  token: SIDIORA_TOKEN,
  maxTokenAmount: 2_100_000n,
  tokenAmount: 2_000_000n,
  deadline: 1000n,
  quoteNonce: 7n,
  gasCost: 10n ** 18n,
  decimals: SIDIORA_DECIMALS,
};
const vector: SponsoredBatch = {
  chainId: 1325n,
  account: `0x${"11".repeat(20)}`,
  nonce: 0n,
  calls: [{ to: `0x${"33".repeat(20)}`, value: 0n, data: "0x1234" }],
  quote,
};
const quoteDigest = "0x6c11f34e7848d98b1ae328fe84bf47223eb14e5274ae04daf3dc64c304c813ba";
const batchDigest = "0xeba39a1c4de2cb2415a6e26ed362f8237cfe005c8d55ebe8f089cf6f7c636d50";
const foundry = readFileSync(new URL("../../../../../contracts/test/BatchCallAndSponsorTest.t.sol", import.meta.url), "utf8");
assert.ok(foundry.includes(quoteDigest));
assert.ok(foundry.includes(batchDigest));
assert.equal(value(gasQuoteDigest(vector.chainId, vector.account, quote)), quoteDigest);
assert.equal(value(sponsoredBatchDigest(vector)), batchDigest);
for (const field of ["maxTokenAmount", "tokenAmount", "deadline", "quoteNonce", "gasCost"] as const) {
  const changed = { ...quote, [field]: quote[field] + 1n };
  assert.notEqual(value(gasQuoteDigest(vector.chainId, vector.account, changed)), quoteDigest);
  assert.notEqual(value(sponsoredBatchDigest({ ...vector, quote: changed })), batchDigest);
}
for (const field of ["token", "sponsor"] as const) {
  const changed = { ...quote, [field]: vector.account };
  assert.notEqual(value(gasQuoteDigest(vector.chainId, vector.account, changed)), quoteDigest);
  assert.notEqual(value(sponsoredBatchDigest({ ...vector, quote: changed })), batchDigest);
}
for (const changed of [
  { ...vector, chainId: 1326n }, { ...vector, account: quote.sponsor }, { ...vector, nonce: 1n },
  { ...vector, calls: [] }, { ...vector, calls: [{ ...vector.calls[0]!, data: "0x1235" }] },
  { ...vector, calls: [{ ...vector.calls[0]!, value: 1n }] },
  { ...vector, calls: [{ ...vector.calls[0]!, to: vector.account }] },
]) assert.notEqual(value(sponsoredBatchDigest(changed)), batchDigest);

const accountKey = keccak_256(new TextEncoder().encode("sponsored account fixture"));
const sponsorKey = keccak_256(new TextEncoder().encode("sponsored relayer fixture"));
const evmAddress = (key: Uint8Array): string => `0x${Buffer.from(keccak_256(secp256k1.getPublicKey(key, false).slice(1))).subarray(12).toString("hex")}`;
function sign(digest: string, key: Uint8Array): string {
  const signed = secp256k1.sign(Buffer.from(digest.slice(2), "hex"), key, { prehash: false, format: "recovered" });
  return `0x${Buffer.from(signed.slice(1)).toString("hex")}${(27 + signed[0]!).toString(16)}`;
}
const batch: SponsoredBatch = {
  ...vector, account: evmAddress(accountKey), quote: { ...quote, sponsor: evmAddress(sponsorKey) },
};
const config: GasStationConfig = {
  chainId: vector.chainId, sponsor: batch.quote.sponsor, token: SIDIORA_TOKEN,
  decimals: SIDIORA_DECIMALS, paymaster: `0x${"44".repeat(20)}`, quoteUrl: "",
};
const accountSignature = sign(value(sponsoredBatchDigest(batch)), accountKey);
const relayerSignature = sign(value(gasQuoteDigest(batch.chainId, batch.account, batch.quote)), sponsorKey);
const call = value(sponsoredBatchCall(config, batch, accountSignature, relayerSignature, 1000n));
assert.equal(call.to, batch.account);
assert.equal(call.value, 0n);
assert.equal(call.data.slice(0, 10), abiSelector("executeSponsored((address,uint256,bytes)[],(address,address,uint256,uint256,uint256,uint256,uint256),bytes,bytes)"));
const body = call.data.slice(10);
const at = (offset: number): bigint => BigInt(`0x${body.slice(offset * 2, (offset + 32) * 2)}`);
assert.equal(at(0), 320n);
assert.equal(at(32), BigInt(batch.quote.sponsor));
assert.equal(at(64), BigInt(SIDIORA_TOKEN));
assert.equal(at(96), quote.maxTokenAmount);
assert.equal(at(128), quote.tokenAmount);
assert.equal(at(160), quote.deadline);
assert.equal(at(192), quote.quoteNonce);
assert.equal(at(224), quote.gasCost);
assert.equal(at(256), 544n);
assert.equal(at(288), 672n);
assert.equal(at(320), 1n);
assert.equal(at(352), 32n);
assert.equal(at(384), BigInt(vector.calls[0]!.to));
assert.equal(at(416), 0n);
assert.equal(at(448), 96n);
assert.equal(at(480), 2n);
assert.equal(body.slice(1024, 1088), "1234".padEnd(64, "0"));
assert.equal(at(544), 65n);
assert.equal(body.slice(576 * 2, 641 * 2), accountSignature.slice(2));
assert.equal(at(672), 65n);
assert.equal(body.slice(704 * 2, 769 * 2), relayerSignature.slice(2));
assert.equal(body.length / 2, 800);

for (const calls of [[], [
  { to: vector.account, value: 2n, data: "0x" },
  { to: quote.sponsor, value: 3n, data: `0x${"ab".repeat(33)}` },
]]) {
  const multiple = { ...batch, calls };
  const encoded = value(sponsoredBatchCall(config, multiple, sign(value(sponsoredBatchDigest(multiple)), accountKey), relayerSignature, 999n));
  const raw = encoded.data.slice(10);
  const read = (offset: number): number => Number(BigInt(`0x${raw.slice(offset * 2, (offset + 32) * 2)}`));
  const array = read(0);
  assert.equal(read(array), calls.length);
  for (const [index, item] of calls.entries()) {
    const tuple = array + 32 + read(array + 32 + index * 32);
    assert.equal(`0x${raw.slice((tuple + 12) * 2, (tuple + 32) * 2)}`, item.to.toLowerCase());
    assert.equal(BigInt(read(tuple + 32)), item.value);
    const dataStart = tuple + read(tuple + 64);
    const length = read(dataStart);
    assert.equal(`0x${raw.slice((dataStart + 32) * 2, (dataStart + 32 + length) * 2)}`, item.data);
  }
}

for (const [change, code] of [
  [{ deadline: 998n }, "expired_quote"],
  [{ maxTokenAmount: 1n }, "above_maximum"],
  [{ sponsor: vector.account }, "sponsor_mismatch"],
  [{ token: vector.account }, "token_mismatch"],
  [{ decimals: 18 }, "decimals_mismatch"],
  [{ tokenAmount: 0n }, "invalid_value"],
  [{ gasCost: 0n }, "invalid_value"],
  [{ quoteNonce: -1n }, "invalid_value"],
  [{ maxTokenAmount: 1n << 256n }, "invalid_value"],
] as const) refused(sponsoredBatchCall(config, { ...batch, quote: { ...batch.quote, ...change } }, accountSignature, relayerSignature, 999n), code);
refused(sponsoredBatchCall({ ...config, decimals: 18 }, batch, accountSignature, relayerSignature, 999n), "decimals_mismatch");
refused(sponsoredBatchCall({ ...config, token: vector.account }, batch, accountSignature, relayerSignature, 999n), "token_mismatch");
refused(sponsoredBatchCall(config, { ...batch, chainId: 1n }, accountSignature, relayerSignature, 999n), "chain_mismatch");
refused(sponsoredBatchCall(config, batch, "0x", relayerSignature, 999n), "invalid_signature");
refused(sponsoredBatchCall(config, batch, accountSignature, "0x", 999n), "invalid_signature");
refused(sponsoredBatchCall(config, batch, relayerSignature, accountSignature, 999n), "invalid_signature");
refused(sponsoredBatchCall(config, { ...batch, nonce: 1n }, accountSignature, relayerSignature, 999n), "invalid_signature");
refused(sponsoredBatchCall(config, batch, accountSignature, sign(value(gasQuoteDigest(batch.chainId, batch.account, { ...batch.quote, quoteNonce: 8n })), sponsorKey), 999n), "invalid_signature");
refused(sponsoredBatchDigest({ ...batch, calls: [{ to: vector.account, value: 0n, data: "0x1" }] }), "invalid_value");
refused(gasQuoteDigest(-1n, batch.account, quote), "invalid_value");

const authorization = { chainId: config.chainId, address: config.paymaster, nonce: 0n };
const authorizationDigest = value(eip7702AuthorizationDigest(authorization));
assert.equal(authorizationDigest, `0x${Buffer.from(keccak_256(Buffer.from(`05d982052d94${"44".repeat(20)}80`, "hex"))).toString("hex")}`);
assert.notEqual(authorizationDigest, value(sponsoredBatchDigest(batch)));
const authorizationSignature = sign(authorizationDigest, accountKey);
assert.deepEqual(value(assembleEip7702Authorization(config, batch.account, 0n, authorizationSignature)), {
  ...authorization, r: authorizationSignature.slice(0, 66), s: `0x${authorizationSignature.slice(66, 130)}`,
  yParity: Number.parseInt(authorizationSignature.slice(130), 16) - 27,
});
for (const changed of [{ ...authorization, nonce: 1n }, { ...authorization, chainId: 1n }, { ...authorization, address: vector.account }]) {
  assert.notEqual(value(eip7702AuthorizationDigest(changed)), authorizationDigest);
}
refused(eip7702AuthorizationDigest({ ...authorization, nonce: (1n << 64n) - 1n }), "invalid_value");
refused(assembleEip7702Authorization(config, batch.account, 1n, authorizationSignature), "invalid_signature");
refused(assembleEip7702Authorization(config, batch.account, 0n, accountSignature), "invalid_signature");

const request = { account: batch.account, nonce: batch.nonce, calls: batch.calls, maxTokenAmount: quote.maxTokenAmount, gasCost: quote.gasCost };
const wireQuote = JSON.parse(JSON.stringify(batch.quote, (_key, item: unknown) => typeof item === "bigint" ? item.toString() : item)) as Record<string, unknown>;
const envelope = { quote: wireQuote, relayerSignature };
let responseBody = JSON.stringify(envelope);
let responseStatus = 200;
let requestCount = 0;
let stallRequests = false;
let onStalledRequest: (() => void) | undefined;
const server = createServer(async (incoming, outgoing) => {
  try {
    assert.equal(incoming.method, "POST");
    assert.equal(incoming.url, "/quote");
    assert.equal(incoming.headers["content-type"], "application/json");
    const chunks: Buffer[] = [];
    for await (const chunk of incoming) chunks.push(Buffer.from(chunk));
    const payload = JSON.parse(Buffer.concat(chunks).toString("utf8")) as Record<string, unknown>;
    assert.equal(payload.chainId, "1325");
    assert.equal(payload.account, batch.account);
    assert.equal(payload.gasCost, "1000000000000000000");
    assert.equal(payload.maxTokenAmount, "2100000");
    assert.deepEqual(payload.calls, batch.calls.map((item) => ({ ...item, value: item.value.toString() })));
    requestCount++;
    if (stallRequests) {
      onStalledRequest?.();
      return;
    }
    outgoing.writeHead(responseStatus, { "content-type": "application/json" });
    outgoing.end(responseBody);
  } catch (error) {
    outgoing.destroy(error instanceof Error ? error : new Error("invalid request"));
  }
});
server.listen(0);
await once(server, "listening");
const bound = server.address();
assert.ok(bound && typeof bound !== "string");
const quoteUrl = `http://${bound.family === "IPv6" ? `[${bound.address}]` : bound.address}:${bound.port}/quote`;
const httpConfig = { ...config, quoteUrl };
try {
  assert.deepEqual(value(await requestGasQuote(httpConfig, request, { now: 999n })), { quote: batch.quote, relayerSignature });
  refused(await requestGasQuote(httpConfig, request, { now: 1001n }), "expired_quote");
  for (const [field, item, code] of [
    ["sponsor", vector.account, "sponsor_mismatch"], ["token", vector.account, "token_mismatch"],
    ["decimals", 18, "decimals_mismatch"], ["maxTokenAmount", "1", "above_maximum"],
    ["maxTokenAmount", "2100001", "above_maximum"], ["gasCost", "1", "invalid_response"],
    ["tokenAmount", 2_000_000, "invalid_response"], ["tokenAmount", "-1", "invalid_response"],
    ["tokenAmount", "2e6", "invalid_response"], ["tokenAmount", "02000000", "invalid_response"],
  ] as const) {
    responseBody = JSON.stringify({ ...envelope, quote: { ...wireQuote, [field]: item } });
    refused(await requestGasQuote(httpConfig, request, { now: 999n }), code);
  }
  responseBody = JSON.stringify({ ...envelope, relayerSignature: accountSignature });
  refused(await requestGasQuote(httpConfig, request, { now: 999n }), "invalid_signature");
  responseBody = "{";
  refused(await requestGasQuote(httpConfig, request, { now: 999n }), "invalid_response");
  responseBody = "null";
  refused(await requestGasQuote(httpConfig, request, { now: 999n }), "invalid_response");
  responseStatus = 403;
  refused(await requestGasQuote(httpConfig, request, { now: 999n }), "refused");
  responseStatus = 503;
  refused(await requestGasQuote(httpConfig, request, { now: 999n }), "unavailable");
  refused(await requestGasQuote(httpConfig, request, { signal: AbortSignal.abort(), now: 999n }), "cancelled");
  refused(await requestGasQuote(config, request, { now: 999n }), "invalid_value");
  assert.equal(requestCount, 17);
  stallRequests = true;
  const controller = new AbortController();
  const watchdog = setTimeout(() => controller.abort(), 20_000);
  try {
    const results = await Promise.all([
      requestGasQuote(httpConfig, request, { signal: controller.signal, now: 999n }),
      requestGasQuote(httpConfig, request, { now: 999n }),
    ]);
    for (const result of results) refused(result, "unavailable");
    assert.equal(controller.signal.aborted, false);
  } finally {
    clearTimeout(watchdog);
  }
  const cancellation = new AbortController();
  onStalledRequest = () => cancellation.abort();
  refused(await requestGasQuote(httpConfig, request, { signal: cancellation.signal, now: 999n }), "cancelled");
  assert.equal(requestCount, 20);
} finally {
  const closed = once(server, "close");
  server.close();
  server.closeAllConnections();
  await closed;
}
refused(await requestGasQuote(httpConfig, request, { now: 999n }), "unavailable");
console.log("gas station: digest vectors, calldata, refusals, authorization and HTTP passed");
