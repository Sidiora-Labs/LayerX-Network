import assert from "node:assert/strict";
import { createDecipheriv, createECDH, hkdfSync } from "node:crypto";
import { readFileSync } from "node:fs";
import { keccak_256 } from "@noble/hashes/sha3.js";

import {
  XWEB_ENVELOPE_INFO,
  XWEB_KIND_API,
  XWEB_LEVEL_MAJORITY,
  XWEB_LEVEL_SINGLE,
  XWEB_PRECOMPILE,
  XWebApiError,
  buildXWebApiRequest,
  decodeXWebApiPayload,
  decodeXWebAttestors,
  encodeXWebApiPayload,
  encodeXWebCredential,
  sealXWebEnvelope,
  xwebApiOrigin,
  xwebApiRequestCall,
  xwebAttestorAddress,
  xwebGetAttestorsCallData,
  type XWebApiHeader,
  type XWebAttestorSet,
  type XWebEnvelopeRandomness,
} from "../src/index.js";

type Json = Record<string, unknown>;
interface EnvelopeVector {
  readonly name: string;
  readonly attestor_key_label: string;
  readonly attestor_public_key: string;
  readonly attestor: string;
  readonly ephemeral_key_label: string;
  readonly ephemeral_public_key: string;
  readonly nonce: string;
  readonly origin: string;
  readonly credential: readonly XWebApiHeader[];
  readonly plaintext: string;
  readonly shared_x: string;
  readonly aes_key: string;
  readonly aad: string;
  readonly ciphertext: string;
  readonly envelope: string;
}
interface ApiVector {
  readonly name: string;
  readonly method: "GET" | "POST";
  readonly level: number;
  readonly attestor: string;
  readonly url: string;
  readonly origin: string;
  readonly headers: readonly XWebApiHeader[];
  readonly body: string;
  readonly pointers: readonly string[];
  readonly envelopes: readonly string[];
  readonly payload: string;
  readonly payload_hash: string;
}
interface Refusal { readonly name: string; readonly payload: string; readonly refuses: string }

const testdata = (name: string): Json =>
  JSON.parse(readFileSync(new URL(`../../../../../modules/xweb/types/testdata/${name}`, import.meta.url), "utf8")) as Json;
const envelopeFile = testdata("envelope-vectors.json");
const apiFile = testdata("api-vectors.json");
const envelopeVectors = envelopeFile["vectors"] as EnvelopeVector[];
const apiVectors = apiFile["vectors"] as ApiVector[];
const refusals = apiFile["refusals"] as Refusal[];

const hex = (bytes: Uint8Array): string => `0x${Buffer.from(bytes).toString("hex")}`;
const bytes = (value: string): Uint8Array => new Uint8Array(Buffer.from(value.slice(2), "hex"));
const labelKey = (label: string): Uint8Array => keccak_256(new TextEncoder().encode(label));
const envelopeNamed = (name: string): EnvelopeVector => {
  const vector = envelopeVectors.find((entry) => entry.name === name);
  assert.ok(vector, name);
  return vector;
};
const randomnessOf = (vector: EnvelopeVector): XWebEnvelopeRandomness => ({
  ephemeralPrivateKey: labelKey(vector.ephemeral_key_label),
  nonce: bytes(vector.nonce),
});
const refuses = (run: () => unknown, fragment: string): void => {
  assert.throws(run, (error: unknown) => error instanceof XWebApiError && error.message.includes(fragment), fragment);
};

// The shared constants of the scheme and the codec.
assert.equal(envelopeFile["hkdf_info"], XWEB_ENVELOPE_INFO);
assert.equal(apiFile["kind"], XWEB_KIND_API);
assert.equal((apiFile["levels"] as Json)["majority"], XWEB_LEVEL_MAJORITY);
assert.equal((apiFile["levels"] as Json)["single"], XWEB_LEVEL_SINGLE);
assert.equal(XWEB_PRECOMPILE, "0x0000000000000000000000000000000000001019");

// Every envelope vector: the attestor key, the credential plaintext, each
// intermediate of the scheme and the sealed bytes, byte for byte.
for (const vector of envelopeVectors) {
  const attestor = createECDH("secp256k1");
  attestor.setPrivateKey(Buffer.from(labelKey(vector.attestor_key_label)));
  assert.equal(hex(attestor.getPublicKey(null, "compressed")), vector.attestor_public_key, vector.name);
  assert.equal(xwebAttestorAddress(vector.attestor_public_key), vector.attestor, vector.name);

  const plaintext = encodeXWebCredential(vector.credential);
  assert.equal(hex(plaintext), vector.plaintext, vector.name);

  const ephemeral = createECDH("secp256k1");
  ephemeral.setPrivateKey(Buffer.from(labelKey(vector.ephemeral_key_label)));
  assert.equal(hex(ephemeral.getPublicKey(null, "compressed")), vector.ephemeral_public_key, vector.name);
  const shared = ephemeral.computeSecret(Buffer.from(bytes(vector.attestor_public_key)));
  assert.equal(hex(shared), vector.shared_x, vector.name);
  const key = new Uint8Array(hkdfSync("sha256", shared, bytes(vector.ephemeral_public_key), XWEB_ENVELOPE_INFO, 32));
  assert.equal(hex(key), vector.aes_key, vector.name);
  assert.equal(`${vector.attestor}${Buffer.from(vector.origin).toString("hex")}`, vector.aad, vector.name);

  const sealed = sealXWebEnvelope(vector.attestor_public_key, vector.origin, plaintext, randomnessOf(vector));
  assert.equal(hex(sealed), vector.envelope, vector.name);
  assert.equal(hex(sealed.subarray(20 + 33 + 12)), vector.ciphertext, vector.name);

  // The attestor opens it with its own key for the same origin.
  const decipher = createDecipheriv("aes-256-gcm", key, bytes(vector.nonce), { authTagLength: 16 });
  decipher.setAAD(bytes(vector.aad));
  const body = sealed.subarray(20 + 33 + 12);
  decipher.setAuthTag(body.subarray(body.length - 16));
  const opened = Buffer.concat([decipher.update(body.subarray(0, body.length - 16)), decipher.final()]);
  assert.equal(hex(opened), vector.plaintext, vector.name);
}

// Without injected randomness each seal draws a fresh ephemeral key and nonce.
{
  const vector = envelopeNamed("attestor-1-api-key");
  const first = sealXWebEnvelope(vector.attestor_public_key, vector.origin, bytes(vector.plaintext));
  const second = sealXWebEnvelope(vector.attestor_public_key, vector.origin, bytes(vector.plaintext));
  assert.equal(first.length, bytes(vector.envelope).length);
  assert.equal(hex(first.subarray(0, 20)), vector.attestor);
  assert.notEqual(hex(first.subarray(20, 65)), hex(second.subarray(20, 65)));
  refuses(() => sealXWebEnvelope(vector.attestor_public_key, vector.origin, new Uint8Array()), "plaintext is 0 bytes");
  refuses(
    () => sealXWebEnvelope(vector.attestor_public_key, vector.origin, bytes(vector.plaintext), { ephemeralPrivateKey: new Uint8Array(32), nonce: bytes(vector.nonce) }),
    "not a secp256k1 scalar",
  );
  refuses(() => sealXWebEnvelope(`0x02${"00".repeat(32)}`, vector.origin, bytes(vector.plaintext)), "not a point on secp256k1");
}

// The attestor set the vectors use, in the order the vectors seal to it.
const attestorOne = envelopeNamed("attestor-1-api-key");
const attestorTwo = envelopeNamed("attestor-2-api-key");
const set: XWebAttestorSet = {
  attestors: [
    { signer: attestorOne.attestor, payout: "pax1attestorone", publicKey: attestorOne.attestor_public_key },
    { signer: attestorTwo.attestor, payout: "pax1attestortwo", publicKey: attestorTwo.attestor_public_key },
  ],
  required: 2,
};
const randomnessFor = (names: readonly string[]) => (attestor: string): XWebEnvelopeRandomness => {
  const vector = names.map(envelopeNamed).find((entry) => entry.attestor === attestor);
  assert.ok(vector, attestor);
  return randomnessOf(vector);
};

// Every api vector: the builder produces the payload bytes and hash the Go codec
// pins, and the payload decodes back to the vector's fields.
for (const vector of apiVectors) {
  const credential = vector.envelopes.length === 0 ? undefined : envelopeNamed(vector.envelopes[0]!).credential;
  const built = buildXWebApiRequest(
    {
      method: vector.method,
      url: vector.url,
      headers: vector.headers,
      body: vector.body,
      pointers: vector.pointers,
      ...(vector.level === XWEB_LEVEL_SINGLE ? { single: vector.attestor } : {}),
    },
    set,
    { ...(credential === undefined ? {} : { credential }), randomness: randomnessFor(vector.envelopes) },
  );
  assert.equal(hex(built.payload), vector.payload, vector.name);
  assert.equal(built.payloadHash, vector.payload_hash, vector.name);
  assert.equal(hex(keccak_256(built.payload)), vector.payload_hash, vector.name);
  assert.equal(built.origin, vector.origin, vector.name);
  assert.equal(built.level, vector.level, vector.name);
  assert.equal(built.attestor, vector.attestor, vector.name);
  assert.deepEqual(built.envelopes.map((sealed) => hex(sealed.envelope)), vector.envelopes.map((name) => envelopeNamed(name).envelope), vector.name);

  const decoded = decodeXWebApiPayload(bytes(vector.payload));
  assert.equal(decoded.method, vector.method, vector.name);
  assert.equal(decoded.level, vector.level, vector.name);
  assert.equal(decoded.attestor, vector.attestor, vector.name);
  assert.equal(decoded.url, vector.url, vector.name);
  assert.deepEqual(decoded.headers, vector.headers, vector.name);
  assert.equal(Buffer.from(decoded.body).toString("utf8"), vector.body, vector.name);
  assert.deepEqual(decoded.pointers, vector.pointers, vector.name);
  assert.equal(hex(encodeXWebApiPayload(decoded)), vector.payload, vector.name);
  assert.equal(xwebApiOrigin(vector.url), vector.origin, vector.name);
}

// Every refusal vector is refused with the reason the Go codec names.
for (const refusal of refusals) {
  refuses(() => decodeXWebApiPayload(bytes(refusal.payload)), refusal.refuses);
}

// The builder refuses what the module would refuse at request time.
const priceCall = { method: "GET" as const, url: "https://paxeer.app/api/v1/price?asset=PAX", pointers: ["/data/price"] };
const apiKey = [{ name: "X-Api-Key", value: "paxeer-vector-credential" }];
refuses(() => buildXWebApiRequest({ ...priceCall, single: "0x00000000000000000000000000000000000000aa" }, set), "the single level names");
refuses(
  () => buildXWebApiRequest(priceCall, { attestors: [set.attestors[0]!, { signer: attestorTwo.attestor, payout: "pax1attestortwo", publicKey: "0x" }], required: 2 }, { credential: apiKey }),
  "1 of 2 attestors take credential envelopes",
);
refuses(
  () => buildXWebApiRequest(priceCall, { attestors: [{ ...set.attestors[0]!, publicKey: attestorTwo.attestor_public_key }], required: 1 }, { credential: apiKey }),
  "belongs to",
);
refuses(() => buildXWebApiRequest(priceCall, set, { credential: [{ name: "Accept", value: "x" }, { name: "accept", value: "y" }] }), "repeats");
refuses(
  () => buildXWebApiRequest({ ...priceCall, headers: [{ name: "X-Api-Key", value: "public" }] }, set, { credential: apiKey }),
  "repeats a public header",
);
refuses(() => buildXWebApiRequest(priceCall, set, { credential: [] }), "credential carries no header");
refuses(() => buildXWebApiRequest({ method: "GET", url: "https://paxeer.app/status", body: "x" }, set), "GET carries");
refuses(() => buildXWebApiRequest({ method: "GET", url: "http://paxeer.app/status" }, set), "is not https");
refuses(() => buildXWebApiRequest({ method: "GET", url: "https://Paxeer.app/status" }, set), "only lower-case letters");
refuses(() => buildXWebApiRequest({ method: "GET", url: "https://paxeer.app/status", pointers: ["data"] }, set), "does not start with /");
refuses(
  () => buildXWebApiRequest({ method: "POST", url: "https://paxeer.app/api/v1/quote", body: "x".repeat(4000), headers: Array.from({ length: 5 }, (_, index) => ({ name: `X-Pad-${index}`, value: "p".repeat(1024) })) }, set, { maxPayloadBytes: 8192 }),
  "payload is",
);

// A majority envelope set for every keyed attestor, sealed with fresh randomness, still decodes.
{
  const built = buildXWebApiRequest(priceCall, set, { credential: apiKey });
  assert.equal(built.envelopes.length, 2);
  assert.deepEqual(built.envelopes.map((sealed) => sealed.attestor), [attestorOne.attestor, attestorTwo.attestor]);
  assert.equal(decodeXWebApiPayload(built.payload).envelopes.length, 2);
}

// The request transaction: request(uint8,bytes,uint64) at the precompile, paying the fee.
{
  const vector = apiVectors.find((entry) => entry.name === "example-consumer")!;
  const call = xwebApiRequestCall(bytes(vector.payload), 200_000n, 1_000n);
  assert.equal(call.to, XWEB_PRECOMPILE);
  assert.equal(call.value, 1_000n);
  const selector = hex(keccak_256(new TextEncoder().encode("request(uint8,bytes,uint64)")).subarray(0, 4));
  assert.ok(call.data.startsWith(selector));
  const words = Buffer.from(call.data.slice(10), "hex");
  assert.equal(words.readBigUInt64BE(24), 3n);
  assert.equal(words.readBigUInt64BE(56), 96n);
  assert.equal(words.readBigUInt64BE(88), 200_000n);
  const length = Number(words.readBigUInt64BE(120));
  assert.equal(hex(words.subarray(128, 128 + length)), vector.payload);
  assert.equal(words.length, 128 + Math.ceil(length / 32) * 32);
  refuses(() => xwebApiRequestCall(bytes(vector.payload), 0n, 1n), "callback gas");
}

// getAttestors(): the call data, and an answer decoded into the set the builder takes.
{
  assert.equal(xwebGetAttestorsCallData(), hex(keccak_256(new TextEncoder().encode("getAttestors()")).subarray(0, 4)));
  const word = (value: bigint | number): string => BigInt(value).toString(16).padStart(64, "0");
  const dynamic = (data: Uint8Array): string => word(data.length) + Buffer.from(data).toString("hex").padEnd(Math.ceil(data.length / 32) * 64, "0");
  const tuple = (signer: string, payout: string, publicKey: string): string => {
    const payoutTail = dynamic(new TextEncoder().encode(payout));
    return word(BigInt(signer)) + word(96) + word(96 + payoutTail.length / 2) + payoutTail + dynamic(bytes(publicKey));
  };
  const first = tuple(attestorOne.attestor, "pax1attestorone", attestorOne.attestor_public_key);
  const second = tuple(attestorTwo.attestor, "pax1attestortwo", "0x");
  const answer = `0x${word(64)}${word(2)}${word(2)}${word(64)}${word(64 + first.length / 2)}${first}${second}`;
  const decoded = decodeXWebAttestors(answer);
  assert.equal(decoded.required, 2);
  assert.deepEqual(decoded.attestors, [
    { signer: attestorOne.attestor, payout: "pax1attestorone", publicKey: attestorOne.attestor_public_key },
    { signer: attestorTwo.attestor, payout: "pax1attestortwo", publicKey: "0x" },
  ]);
  const mismatched = `0x${word(64)}${word(1)}${word(1)}${word(32)}${tuple(attestorTwo.attestor, "pax1attestortwo", attestorOne.attestor_public_key)}`;
  refuses(() => decodeXWebAttestors(mismatched), "belongs to");
  refuses(() => decodeXWebAttestors(answer.slice(0, 200)), "ends before byte");
}

console.log("xweb api helpers: payload, envelope and refusal vectors pinned");
