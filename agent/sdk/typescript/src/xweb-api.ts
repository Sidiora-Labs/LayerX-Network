/**
 * Builders for xweb api requests: the api payload a contract passes to the
 * xweb precompile with kind 3, and the credential envelopes sealed to the
 * attestors getAttestors returns.
 *
 * The payload bytes are exactly what modules/xweb/types decodes (api.go), and
 * an envelope is ECIES over secp256k1 with HKDF-SHA256 and AES-256-GCM
 * (envelope.go). modules/xweb/types/testdata/api-vectors.json and
 * envelope-vectors.json pin both.
 */

import { ECDH, createCipheriv, createECDH, hkdfSync, randomBytes } from "node:crypto";
import { keccak_256 } from "@noble/hashes/sha3.js";

import { abiSelector, type PrecompileCall } from "./exchange.js";

export const XWEB_PRECOMPILE = "0x0000000000000000000000000000000000001019";
export const XWEB_KIND_API = 3;
export const XWEB_API_VERSION = 1;
export const XWEB_METHOD_GET = 1;
export const XWEB_METHOD_POST = 2;
export const XWEB_LEVEL_MAJORITY = 0;
export const XWEB_LEVEL_SINGLE = 1;
export const XWEB_ENVELOPE_INFO = "PAXEERX_WEB_API_ENVELOPE_V1";
export const XWEB_DEFAULT_MAX_PAYLOAD_BYTES = 8192;

export const XWEB_API_LIMITS = {
  urlBytes: 2048,
  headers: 16,
  headerNameBytes: 64,
  headerValueBytes: 1024,
  bodyBytes: 4096,
  pointers: 16,
  pointerBytes: 256,
  envelopes: 64,
  credentialHeaders: 8,
  credentialBytes: 1024,
} as const;

export const XWEB_ENVELOPE_KEY_LENGTH = 33;
export const XWEB_ENVELOPE_NONCE_LENGTH = 12;
export const XWEB_ENVELOPE_TAG_LENGTH = 16;
export const XWEB_ENVELOPE_OVERHEAD = 20 + XWEB_ENVELOPE_KEY_LENGTH + XWEB_ENVELOPE_NONCE_LENGTH + XWEB_ENVELOPE_TAG_LENGTH;
export const XWEB_MAX_ENVELOPE_BYTES = XWEB_ENVELOPE_OVERHEAD + XWEB_API_LIMITS.credentialBytes;

export type XWebApiErrorCode =
  | "invalid_api"
  | "invalid_level"
  | "invalid_envelope"
  | "invalid_attestors"
  | "unknown_attestor"
  | "payload_too_large"
  | "invalid_abi";

export class XWebApiError extends Error {
  readonly code: XWebApiErrorCode;

  constructor(code: XWebApiErrorCode, detail: string) {
    super(`${code}: ${detail}`);
    this.name = "XWebApiError";
    this.code = code;
  }
}

export type XWebApiMethod = "GET" | "POST";

export interface XWebApiHeader {
  readonly name: string;
  readonly value: string;
}

/** A decoded api payload. Addresses are lower-case 0x hex; envelopes are wire bytes. */
export interface XWebApiPayload {
  readonly method: XWebApiMethod;
  readonly level: number;
  readonly attestor: string;
  readonly url: string;
  readonly headers: readonly XWebApiHeader[];
  readonly body: Uint8Array;
  readonly pointers: readonly string[];
  readonly envelopes: readonly Uint8Array[];
}

/** One entry of getAttestors(). publicKey is "0x" for an attestor that takes no envelopes. */
export interface XWebAttestor {
  readonly signer: string;
  readonly payout: string;
  readonly publicKey: string;
}

/** The answer of getAttestors(): the set and the signatures a majority fulfilment needs. */
export interface XWebAttestorSet {
  readonly attestors: readonly XWebAttestor[];
  readonly required: number;
}

/** The ephemeral private key and nonce of one envelope. Supplied only to reproduce a vector. */
export interface XWebEnvelopeRandomness {
  readonly ephemeralPrivateKey: Uint8Array;
  readonly nonce: Uint8Array;
}

/** The call a developer wants made. `single` names the one attestor that makes it. */
export interface XWebApiCall {
  readonly method: XWebApiMethod;
  readonly url: string;
  readonly headers?: readonly XWebApiHeader[];
  readonly body?: Uint8Array | string;
  readonly pointers?: readonly string[];
  readonly single?: string;
}

export interface XWebApiBuildOptions {
  /** Credential headers, sealed to every attestor that can make the call; never sent in the clear. */
  readonly credential?: readonly XWebApiHeader[];
  /** The payload cap getParams() reports; 8192 when omitted. */
  readonly maxPayloadBytes?: number;
  /** Fixed randomness per attestor, for reproducing a vector; a secure source otherwise. */
  readonly randomness?: (attestor: string) => XWebEnvelopeRandomness;
}

export interface XWebSealedEnvelope {
  readonly attestor: string;
  readonly envelope: Uint8Array;
}

export interface XWebApiRequest {
  readonly payload: Uint8Array;
  readonly payloadHash: string;
  readonly origin: string;
  readonly level: number;
  readonly attestor: string;
  readonly envelopes: readonly XWebSealedEnvelope[];
}

const ZERO_ADDRESS = "0x0000000000000000000000000000000000000000";
const ADDRESS = /^0x[0-9a-fA-F]{40}$/u;
const HEX = /^0x(?:[0-9a-fA-F]{2})*$/u;
const TOKEN = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]$/u;
const LONE_SURROGATE = /[\uD800-\uDFFF]/u;
const RESTRICTED_HEADERS = new Set([
  "host", "content-length", "transfer-encoding", "connection", "keep-alive", "te", "trailer", "upgrade",
]);
const SECP256K1_ORDER = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;
const WORD = 32;

function hex(bytes: Uint8Array): string {
  return `0x${Buffer.from(bytes).toString("hex")}`;
}

function unhex(value: string, label: string, code: XWebApiErrorCode = "invalid_api"): Uint8Array {
  if (!HEX.test(value)) {
    throw new XWebApiError(code, `${label} is not 0x-prefixed hex`);
  }
  return new Uint8Array(Buffer.from(value.slice(2), "hex"));
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

function address(value: string, label: string): Uint8Array {
  if (!ADDRESS.test(value)) {
    throw new XWebApiError("invalid_api", `${label} ${JSON.stringify(value)} is not a 20-byte address`);
  }
  return unhex(value.toLowerCase(), label);
}

function sameAddress(left: string, right: string): boolean {
  return left.toLowerCase() === right.toLowerCase();
}

const ascii = (text: string): Uint8Array => new TextEncoder().encode(text);

function apiError(detail: string): XWebApiError {
  return new XWebApiError("invalid_api", detail);
}

export function xwebMethodCode(method: XWebApiMethod): number {
  if (method === "GET") {
    return XWEB_METHOD_GET;
  }
  if (method === "POST") {
    return XWEB_METHOD_POST;
  }
  throw apiError(`method ${String(method)}, want GET or POST`);
}

function methodName(code: number): XWebApiMethod {
  if (code === XWEB_METHOD_GET) {
    return "GET";
  }
  if (code === XWEB_METHOD_POST) {
    return "POST";
  }
  throw apiError(`method ${code}, want 1 (GET) or 2 (POST)`);
}

function validPort(port: string): boolean {
  return port === "" || /^:[0-9]*$/u.test(port);
}

function validPathEscapes(path: string): boolean {
  for (let index = 0; index < path.length; index += 1) {
    if (path[index] === "%" && !/^[0-9A-Fa-f]{2}$/u.test(path.slice(index + 1, index + 3))) {
      return false;
    }
  }
  return true;
}

/** "https://" and the URL's authority: the origin every envelope of a payload is bound to. */
export function xwebApiOrigin(url: string): string {
  const raw = ascii(url);
  if (raw.length === 0 || raw.length > XWEB_API_LIMITS.urlBytes) {
    throw apiError(`url is ${raw.length} bytes, want 1 to ${XWEB_API_LIMITS.urlBytes}`);
  }
  for (let index = 0; index < raw.length; index += 1) {
    const byte = raw[index]!;
    if (byte < 0x21 || byte > 0x7e) {
      throw apiError(`url byte ${index} is 0x${byte.toString(16).padStart(2, "0")}, not printable ASCII`);
    }
  }
  if (url.includes("#")) {
    throw apiError("url carries a fragment");
  }
  if (!url.startsWith("https://")) {
    throw apiError(`url ${JSON.stringify(url)} is not https`);
  }
  const rest = url.slice("https://".length);
  const end = rest.search(/[/?]/u);
  const authority = end < 0 ? rest : rest.slice(0, end);
  if (authority === "") {
    throw apiError(`url ${JSON.stringify(url)} has no host`);
  }
  for (const char of authority) {
    if (!/^[a-z0-9.\-:[\]]$/u.test(char)) {
      throw apiError(`url authority ${JSON.stringify(authority)} holds ${JSON.stringify(char)}: only lower-case letters, digits and .-:[] are accepted`);
    }
  }
  let hostname: string;
  if (authority.startsWith("[")) {
    const close = authority.lastIndexOf("]");
    if (close < 0 || !validPort(authority.slice(close + 1))) {
      throw apiError(`url ${JSON.stringify(url)} has a malformed host`);
    }
    hostname = authority.slice(1, close);
  } else {
    const colon = authority.lastIndexOf(":");
    if (colon >= 0 && !validPort(authority.slice(colon))) {
      throw apiError(`url ${JSON.stringify(url)} has a malformed port`);
    }
    hostname = colon >= 0 ? authority.slice(0, colon) : authority;
  }
  const query = rest.indexOf("?");
  const path = end < 0 ? "" : rest.slice(end, query < 0 ? rest.length : Math.max(query, end));
  if (!validPathEscapes(path)) {
    throw apiError(`url ${JSON.stringify(url)} has an invalid escape`);
  }
  if (hostname === "") {
    throw apiError(`url ${JSON.stringify(url)} has no host`);
  }
  return `https://${authority}`;
}

function validateHeaders(what: string, headers: readonly XWebApiHeader[], bound: number): void {
  if (headers.length > bound) {
    throw apiError(`${headers.length} ${what}s, bound ${bound}`);
  }
  const seen = new Set<string>();
  headers.forEach((header, index) => {
    const name = ascii(header.name);
    if (name.length === 0 || name.length > XWEB_API_LIMITS.headerNameBytes) {
      throw apiError(`${what} ${index} name is ${name.length} bytes, want 1 to ${XWEB_API_LIMITS.headerNameBytes}`);
    }
    for (const char of header.name) {
      if (!TOKEN.test(char)) {
        throw apiError(`${what} ${index} name ${JSON.stringify(header.name)} holds ${JSON.stringify(char)}, not a token character`);
      }
    }
    const lower = header.name.toLowerCase();
    if (RESTRICTED_HEADERS.has(lower)) {
      throw apiError(`${what} ${index} is ${header.name}, which the sidecar sets itself`);
    }
    if (seen.has(lower)) {
      throw apiError(`${what} ${index} repeats ${header.name}`);
    }
    seen.add(lower);
    const value = ascii(header.value);
    if (value.length > XWEB_API_LIMITS.headerValueBytes) {
      throw apiError(`${what} ${header.name} value is ${value.length} bytes, bound ${XWEB_API_LIMITS.headerValueBytes}`);
    }
    value.forEach((byte, at) => {
      if (byte !== 0x09 && (byte < 0x20 || byte > 0x7e)) {
        throw apiError(`${what} ${header.name} value byte ${at} is 0x${byte.toString(16).padStart(2, "0")}`);
      }
    });
  });
}

function validatePointers(pointers: readonly string[]): void {
  if (pointers.length > XWEB_API_LIMITS.pointers) {
    throw apiError(`${pointers.length} pointers, bound ${XWEB_API_LIMITS.pointers}`);
  }
  const seen = new Set<string>();
  pointers.forEach((pointer, index) => {
    if (LONE_SURROGATE.test(pointer)) {
      throw apiError(`pointer ${index} is not UTF-8`);
    }
    const length = ascii(pointer).length;
    if (length > XWEB_API_LIMITS.pointerBytes) {
      throw apiError(`pointer ${index} is ${length} bytes, bound ${XWEB_API_LIMITS.pointerBytes}`);
    }
    if (pointer !== "" && !pointer.startsWith("/")) {
      throw apiError(`pointer ${JSON.stringify(pointer)} does not start with /`);
    }
    for (let at = 0; at < pointer.length; at += 1) {
      if (pointer[at] === "~" && pointer[at + 1] !== "0" && pointer[at + 1] !== "1") {
        throw apiError(`pointer ${JSON.stringify(pointer)} has a ~ not followed by 0 or 1`);
      }
    }
    if (seen.has(pointer)) {
      throw apiError(`pointer ${JSON.stringify(pointer)} is repeated`);
    }
    seen.add(pointer);
  });
}

function decompress(publicKey: Uint8Array, label: string): Uint8Array {
  if (publicKey.length !== XWEB_ENVELOPE_KEY_LENGTH || (publicKey[0] !== 0x02 && publicKey[0] !== 0x03)) {
    throw new XWebApiError("invalid_envelope", `${label} is not a 33-byte compressed secp256k1 key`);
  }
  try {
    return new Uint8Array(ECDH.convertKey(Buffer.from(publicKey), "secp256k1", undefined, undefined, "uncompressed") as Buffer);
  } catch {
    throw new XWebApiError("invalid_envelope", `${label} is not a point on secp256k1`);
  }
}

/** The EVM address a compressed secp256k1 public key signs as. */
export function xwebAttestorAddress(publicKey: Uint8Array | string): string {
  const key = typeof publicKey === "string" ? unhex(publicKey, "public key", "invalid_envelope") : publicKey;
  const uncompressed = decompress(key, "public key");
  return hex(keccak_256(uncompressed.subarray(1)).subarray(12));
}

function validateEnvelope(envelope: Uint8Array, index: number): string {
  if (envelope.length < XWEB_ENVELOPE_OVERHEAD + 1 || envelope.length > XWEB_MAX_ENVELOPE_BYTES) {
    throw apiError(`envelope ${index}: ${envelope.length} bytes, want ${XWEB_ENVELOPE_OVERHEAD + 1} to ${XWEB_MAX_ENVELOPE_BYTES}`);
  }
  const attestor = hex(envelope.subarray(0, 20));
  if (attestor === ZERO_ADDRESS) {
    throw apiError(`envelope ${index}: addressed to the zero attestor`);
  }
  try {
    decompress(envelope.subarray(20, 20 + XWEB_ENVELOPE_KEY_LENGTH), "ephemeral key");
  } catch (error) {
    throw apiError(`envelope ${index}: ${(error as Error).message}`);
  }
  return attestor;
}

function validatePayload(payload: XWebApiPayload): void {
  xwebMethodCode(payload.method);
  const attestor = address(payload.attestor, "attestor");
  const zero = attestor.every((byte) => byte === 0);
  if (payload.level === XWEB_LEVEL_MAJORITY) {
    if (!zero) {
      throw new XWebApiError("invalid_level", `majority level names attestor ${payload.attestor.toLowerCase()}`);
    }
  } else if (payload.level === XWEB_LEVEL_SINGLE) {
    if (zero) {
      throw new XWebApiError("invalid_level", "single level names no attestor");
    }
  } else {
    throw new XWebApiError("invalid_level", `level ${payload.level}`);
  }
  xwebApiOrigin(payload.url);
  validateHeaders("header", payload.headers, XWEB_API_LIMITS.headers);
  if (payload.body.length > XWEB_API_LIMITS.bodyBytes) {
    throw apiError(`body is ${payload.body.length} bytes, bound ${XWEB_API_LIMITS.bodyBytes}`);
  }
  if (payload.method === "GET" && payload.body.length !== 0) {
    throw apiError(`GET carries a ${payload.body.length}-byte body`);
  }
  validatePointers(payload.pointers);
  if (payload.envelopes.length > XWEB_API_LIMITS.envelopes) {
    throw apiError(`${payload.envelopes.length} envelopes, bound ${XWEB_API_LIMITS.envelopes}`);
  }
  const addressed = new Set<string>();
  payload.envelopes.forEach((envelope, index) => {
    const to = validateEnvelope(envelope, index);
    if (addressed.has(to)) {
      throw apiError(`envelope ${index} repeats attestor ${to}`);
    }
    addressed.add(to);
    if (payload.level === XWEB_LEVEL_SINGLE && !sameAddress(to, payload.attestor)) {
      throw apiError(`envelope ${index} is addressed to ${to}, the single level names ${payload.attestor.toLowerCase()}`);
    }
  });
}

class Writer {
  private readonly parts: Uint8Array[] = [];

  u8(field: string, value: number): void {
    if (!Number.isInteger(value) || value < 0 || value > 0xff) {
      throw apiError(`${field} count ${value} does not fit one byte`);
    }
    this.parts.push(Uint8Array.of(value));
  }

  raw(bytes: Uint8Array): void {
    this.parts.push(bytes);
  }

  field(name: string, data: Uint8Array): void {
    if (data.length > 0xffff) {
      throw apiError(`${name} is ${data.length} bytes, over the uint16 length`);
    }
    this.parts.push(Uint8Array.of(data.length >> 8, data.length & 0xff), data);
  }

  headers(what: string, headers: readonly XWebApiHeader[]): void {
    this.u8(`${what}s`, headers.length);
    for (const header of headers) {
      this.field(`${what} name`, ascii(header.name));
      this.field(`${what} value`, ascii(header.value));
    }
  }

  bytes(): Uint8Array {
    return concat(this.parts);
  }
}

/** Validates payload and returns its api payload bytes, exactly as modules/xweb encodes them. */
export function encodeXWebApiPayload(payload: XWebApiPayload): Uint8Array {
  validatePayload(payload);
  const writer = new Writer();
  writer.raw(Uint8Array.of(XWEB_API_VERSION, xwebMethodCode(payload.method), payload.level));
  writer.raw(address(payload.attestor, "attestor"));
  writer.field("url", ascii(payload.url));
  writer.headers("header", payload.headers);
  writer.field("body", payload.body);
  writer.u8("pointers", payload.pointers.length);
  for (const pointer of payload.pointers) {
    writer.field("pointer", ascii(pointer));
  }
  writer.u8("envelopes", payload.envelopes.length);
  for (const envelope of payload.envelopes) {
    writer.field("envelope", envelope);
  }
  return writer.bytes();
}

class Reader {
  private at = 0;

  constructor(private readonly data: Uint8Array) {}

  take(field: string, length: number): Uint8Array {
    if (this.data.length - this.at < length) {
      throw apiError(`payload ends inside ${field} at byte ${this.at}`);
    }
    const out = this.data.subarray(this.at, this.at + length);
    this.at += length;
    return out;
  }

  u8(field: string): number {
    return this.take(field, 1)[0]!;
  }

  field(name: string): Uint8Array {
    const length = this.take(`${name} length`, 2);
    return this.take(name, (length[0]! << 8) | length[1]!);
  }

  headers(what: string): XWebApiHeader[] {
    const count = this.u8(`${what} count`);
    const headers: XWebApiHeader[] = [];
    for (let index = 0; index < count; index += 1) {
      const name = this.field(`${what} ${index} name`);
      const value = this.field(`${what} ${index} value`);
      headers.push({ name: Buffer.from(name).toString("latin1"), value: Buffer.from(value).toString("latin1") });
    }
    return headers;
  }

  remaining(): number {
    return this.data.length - this.at;
  }
}

function utf8(bytes: Uint8Array, label: string): string {
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    throw apiError(`${label} is not UTF-8`);
  }
}

/** Decodes and validates api payload bytes; nothing may follow the last envelope. */
export function decodeXWebApiPayload(raw: Uint8Array): XWebApiPayload {
  const reader = new Reader(raw);
  const head = reader.take("the version, method, level and attestor", 3 + 20);
  if (head[0] !== XWEB_API_VERSION) {
    throw apiError(`version ${head[0]!}, want ${XWEB_API_VERSION}`);
  }
  const method = methodName(head[1]!);
  const level = head[2]!;
  const attestor = hex(head.subarray(3, 23));
  const url = Buffer.from(reader.field("url")).toString("latin1");
  const headers = reader.headers("header");
  const body = new Uint8Array(reader.field("body"));
  const pointers: string[] = [];
  const pointerCount = reader.u8("pointer count");
  for (let index = 0; index < pointerCount; index += 1) {
    pointers.push(utf8(reader.field(`pointer ${index}`), `pointer ${index}`));
  }
  const envelopes: Uint8Array[] = [];
  const envelopeCount = reader.u8("envelope count");
  for (let index = 0; index < envelopeCount; index += 1) {
    const envelope = new Uint8Array(reader.field(`envelope ${index}`));
    validateEnvelope(envelope, index);
    envelopes.push(envelope);
  }
  if (reader.remaining() !== 0) {
    throw apiError(`${reader.remaining()} bytes follow the last envelope`);
  }
  const payload: XWebApiPayload = { method, level, attestor, url, headers, body, pointers, envelopes };
  validatePayload(payload);
  return payload;
}

/** The plaintext a credential envelope carries: the credential headers in the payload's header encoding. */
export function encodeXWebCredential(headers: readonly XWebApiHeader[], publicHeaders: readonly XWebApiHeader[] = []): Uint8Array {
  if (headers.length === 0) {
    throw apiError("credential carries no header");
  }
  validateHeaders("credential header", headers, XWEB_API_LIMITS.credentialHeaders);
  for (const header of headers) {
    if (publicHeaders.some((other) => other.name.toLowerCase() === header.name.toLowerCase())) {
      throw apiError(`credential header ${header.name} repeats a public header`);
    }
  }
  const writer = new Writer();
  writer.headers("credential header", headers);
  const out = writer.bytes();
  if (out.length > XWEB_API_LIMITS.credentialBytes) {
    throw apiError(`credential is ${out.length} bytes, bound ${XWEB_API_LIMITS.credentialBytes}`);
  }
  return out;
}

function scalar(bytes: Uint8Array): bigint {
  return bytes.reduce((value, byte) => (value << 8n) | BigInt(byte), 0n);
}

/**
 * Seals a credential plaintext to one attestor's compressed public key for an
 * origin. Without `randomness` the ephemeral key and nonce come from the
 * operating system's secure source; a fixed pair must never be reused.
 */
export function sealXWebEnvelope(
  publicKey: Uint8Array | string,
  origin: string,
  plaintext: Uint8Array,
  randomness?: XWebEnvelopeRandomness,
): Uint8Array {
  if (plaintext.length === 0 || plaintext.length > XWEB_API_LIMITS.credentialBytes) {
    throw new XWebApiError("invalid_envelope", `plaintext is ${plaintext.length} bytes, want 1 to ${XWEB_API_LIMITS.credentialBytes}`);
  }
  const recipient = typeof publicKey === "string" ? unhex(publicKey, "public key", "invalid_envelope") : publicKey;
  const attestor = unhex(xwebAttestorAddress(recipient), "attestor");
  const ecdh = createECDH("secp256k1");
  let nonce: Uint8Array;
  if (randomness === undefined) {
    ecdh.generateKeys();
    nonce = new Uint8Array(randomBytes(XWEB_ENVELOPE_NONCE_LENGTH));
  } else {
    const secret = scalar(randomness.ephemeralPrivateKey);
    if (randomness.ephemeralPrivateKey.length !== 32 || secret === 0n || secret >= SECP256K1_ORDER) {
      throw new XWebApiError("invalid_envelope", "ephemeral private key is not a secp256k1 scalar");
    }
    if (randomness.nonce.length !== XWEB_ENVELOPE_NONCE_LENGTH) {
      throw new XWebApiError("invalid_envelope", `nonce is ${randomness.nonce.length} bytes, want ${XWEB_ENVELOPE_NONCE_LENGTH}`);
    }
    ecdh.setPrivateKey(Buffer.from(randomness.ephemeralPrivateKey));
    nonce = randomness.nonce;
  }
  const ephemeralKey = new Uint8Array(ecdh.getPublicKey(null, "compressed"));
  const shared = new Uint8Array(ecdh.computeSecret(Buffer.from(recipient)));
  if (shared.every((byte) => byte === 0)) {
    throw new XWebApiError("invalid_envelope", "shared point is the identity");
  }
  const key = new Uint8Array(hkdfSync("sha256", shared, ephemeralKey, XWEB_ENVELOPE_INFO, 32));
  const cipher = createCipheriv("aes-256-gcm", key, nonce, { authTagLength: XWEB_ENVELOPE_TAG_LENGTH });
  cipher.setAAD(concat([attestor, ascii(origin)]));
  const ciphertext = concat([cipher.update(plaintext), cipher.final(), cipher.getAuthTag()]);
  key.fill(0);
  shared.fill(0);
  return concat([attestor, ephemeralKey, nonce, ciphertext]);
}

function envelopeRecipients(call: XWebApiCall, set: XWebAttestorSet): readonly XWebAttestor[] {
  if (call.single !== undefined) {
    const named = set.attestors.find((attestor) => sameAddress(attestor.signer, call.single!));
    if (named === undefined) {
      throw new XWebApiError("unknown_attestor", `the single level names ${call.single.toLowerCase()}`);
    }
    if (named.publicKey === "0x") {
      throw new XWebApiError("invalid_attestors", `attestor ${named.signer.toLowerCase()} takes no credential envelopes`);
    }
    return [named];
  }
  const keyed = set.attestors.filter((attestor) => attestor.publicKey !== "0x");
  if (keyed.length < set.required || keyed.length === 0) {
    throw new XWebApiError(
      "invalid_attestors",
      `${keyed.length} of ${set.attestors.length} attestors take credential envelopes, a majority fulfilment needs ${set.required}`,
    );
  }
  return keyed;
}

/**
 * Builds an api request from the call and the attestor set getAttestors
 * returns: the payload a contract passes to request(3, payload, callbackGas)
 * or XWebApi's withCredential, and one envelope per attestor that can make
 * the call when a credential is given.
 */
export function buildXWebApiRequest(
  call: XWebApiCall,
  attestors: XWebAttestorSet,
  options: XWebApiBuildOptions = {},
): XWebApiRequest {
  const origin = xwebApiOrigin(call.url);
  const headers = call.headers ?? [];
  const level = call.single === undefined ? XWEB_LEVEL_MAJORITY : XWEB_LEVEL_SINGLE;
  const attestor = call.single === undefined ? ZERO_ADDRESS : hex(address(call.single, "single attestor"));
  if (call.single !== undefined && !attestors.attestors.some((entry) => sameAddress(entry.signer, attestor))) {
    throw new XWebApiError("unknown_attestor", `the single level names ${attestor}`);
  }
  const envelopes: XWebSealedEnvelope[] = [];
  if (options.credential !== undefined) {
    const plaintext = encodeXWebCredential(options.credential, headers);
    try {
      for (const recipient of envelopeRecipients(call, attestors)) {
        const signer = hex(address(recipient.signer, "attestor signer"));
        const publicKey = unhex(recipient.publicKey, `public key of ${signer}`, "invalid_attestors");
        if (xwebAttestorAddress(publicKey) !== signer) {
          throw new XWebApiError("invalid_attestors", `public key of ${signer} belongs to ${xwebAttestorAddress(publicKey)}`);
        }
        const envelope = sealXWebEnvelope(publicKey, origin, plaintext, options.randomness?.(signer));
        envelopes.push({ attestor: signer, envelope });
      }
    } finally {
      plaintext.fill(0);
    }
  }
  if (envelopes.length > attestors.attestors.length) {
    throw apiError(`${envelopes.length} envelopes for ${attestors.attestors.length} registered attestors`);
  }
  const body = typeof call.body === "string" ? ascii(call.body) : call.body ?? new Uint8Array();
  const payload = encodeXWebApiPayload({
    method: call.method,
    level,
    attestor,
    url: call.url,
    headers,
    body,
    pointers: call.pointers ?? [],
    envelopes: envelopes.map((sealed) => sealed.envelope),
  });
  const cap = options.maxPayloadBytes ?? XWEB_DEFAULT_MAX_PAYLOAD_BYTES;
  if (payload.length > cap) {
    throw new XWebApiError("payload_too_large", `payload is ${payload.length} bytes, cap ${cap}`);
  }
  return { payload, payloadHash: hex(keccak_256(payload)), origin, level, attestor, envelopes };
}

function uintWord(value: bigint): Uint8Array {
  const out = new Uint8Array(WORD);
  let rest = value;
  for (let index = WORD - 1; index >= 0; index -= 1) {
    out[index] = Number(rest & 0xffn);
    rest >>= 8n;
  }
  return out;
}

/** The request(uint8,bytes,uint64) transaction for an api payload, paying `fee` wei of PAX. */
export function xwebApiRequestCall(payload: Uint8Array, callbackGas: bigint, fee: bigint): PrecompileCall {
  if (callbackGas <= 0n || callbackGas >= 1n << 64n) {
    throw new XWebApiError("invalid_abi", `callback gas ${callbackGas} is not a positive uint64`);
  }
  if (fee < 0n || fee >= 1n << 256n) {
    throw new XWebApiError("invalid_abi", `fee ${fee} is not a uint256`);
  }
  const padded = new Uint8Array(Math.ceil(payload.length / WORD) * WORD);
  padded.set(payload);
  const data = concat([
    uintWord(BigInt(XWEB_KIND_API)),
    uintWord(3n * BigInt(WORD)),
    uintWord(callbackGas),
    uintWord(BigInt(payload.length)),
    padded,
  ]);
  return { to: XWEB_PRECOMPILE, data: abiSelector("request(uint8,bytes,uint64)") + hex(data).slice(2), value: fee };
}

/** The eth_call data of getAttestors(). */
export function xwebGetAttestorsCallData(): string {
  return abiSelector("getAttestors()");
}

class AbiReader {
  constructor(private readonly data: Uint8Array) {}

  word(offset: number): Uint8Array {
    if (!Number.isSafeInteger(offset) || offset < 0 || offset + WORD > this.data.length) {
      throw new XWebApiError("invalid_abi", `getAttestors answer ends before byte ${offset + WORD}`);
    }
    return this.data.subarray(offset, offset + WORD);
  }

  uint(offset: number, bits: number): bigint {
    const value = scalar(this.word(offset));
    if (value >= 1n << BigInt(bits)) {
      throw new XWebApiError("invalid_abi", `word at ${offset} is not a uint${bits}`);
    }
    return value;
  }

  index(offset: number): number {
    return Number(this.uint(offset, 32));
  }

  address(offset: number): string {
    const word = this.word(offset);
    if (word.subarray(0, 12).some((byte) => byte !== 0)) {
      throw new XWebApiError("invalid_abi", `word at ${offset} is not an address`);
    }
    return hex(word.subarray(12));
  }

  bytes(offset: number): Uint8Array {
    const length = this.index(offset);
    const start = offset + WORD;
    if (start + length > this.data.length) {
      throw new XWebApiError("invalid_abi", `bytes at ${offset} run past the answer`);
    }
    return this.data.subarray(start, start + length);
  }
}

/** Decodes the answer of getAttestors() into the attestor set a request is built from. */
export function decodeXWebAttestors(answer: string): XWebAttestorSet {
  const reader = new AbiReader(unhex(answer, "getAttestors answer", "invalid_abi"));
  const array = reader.index(0);
  const required = Number(reader.uint(WORD, 32));
  const count = reader.index(array);
  const base = array + WORD;
  const attestors: XWebAttestor[] = [];
  for (let index = 0; index < count; index += 1) {
    const tuple = base + reader.index(base + index * WORD);
    const signer = reader.address(tuple);
    const payout = utf8(reader.bytes(tuple + reader.index(tuple + WORD)), `payout of ${signer}`);
    const publicKey = hex(reader.bytes(tuple + reader.index(tuple + 2 * WORD)));
    if (publicKey !== "0x" && xwebAttestorAddress(publicKey) !== signer) {
      throw new XWebApiError("invalid_attestors", `public key of ${signer} belongs to ${xwebAttestorAddress(publicKey)}`);
    }
    attestors.push({ signer, payout, publicKey });
  }
  return { attestors, required };
}
