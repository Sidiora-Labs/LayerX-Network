import { createHash } from "node:crypto";

export interface PayerGrant {
  readonly grant_id: string;
  readonly from: string;
  readonly recipient: string;
  readonly asset: string;
  readonly per_draw_maximum: string;
  readonly allowance: string;
  readonly recurring: boolean;
  readonly window_length: string;
  readonly expiration: string;
  readonly purpose_hash: string;
  readonly has_reference: boolean;
  readonly reference_hash: string;
  readonly revocation_sequence: string;
  readonly public_key: string;
  readonly signature: string;
}

export interface ReceiverAuthorization {
  readonly kind: number;
  readonly controller: string;
  readonly public_key: string;
  readonly signature: string;
  readonly signed_context_hash: string;
  readonly network_id: number;
  readonly protocol_version: number;
}

export interface Receive {
  readonly from: string;
  readonly to: string;
  readonly asset: string;
  readonly amount: string;
  readonly grant_id: string;
  readonly receiver_sequence: string;
  readonly idempotency_key: string;
  readonly context_hash: string;
  readonly receiver_authorization: ReceiverAuthorization;
  readonly payer_grant: PayerGrant;
}

type Field = readonly [string, "hex" | "integer" | "number" | "boolean", number];
const CORE: readonly Field[] = [
  ["from", "hex", 32], ["to", "hex", 32], ["asset", "hex", 32],
  ["amount", "integer", 16], ["grant_id", "hex", 32],
  ["receiver_sequence", "integer", 8], ["idempotency_key", "hex", 32],
  ["context_hash", "hex", 32],
];
const AUTH: readonly Field[] = [
  ["kind", "number", 1], ["controller", "hex", 32], ["public_key", "hex", 32],
  ["signature", "hex", 64], ["signed_context_hash", "hex", 32],
  ["network_id", "number", 4], ["protocol_version", "number", 2],
];
const GRANT: readonly Field[] = [
  ["grant_id", "hex", 32], ["from", "hex", 32], ["recipient", "hex", 32],
  ["asset", "hex", 32], ["per_draw_maximum", "integer", 16], ["allowance", "integer", 16],
  ["recurring", "boolean", 1], ["window_length", "integer", 8], ["expiration", "integer", 8],
  ["purpose_hash", "hex", 32], ["has_reference", "boolean", 1], ["reference_hash", "hex", 32],
  ["revocation_sequence", "integer", 8], ["public_key", "hex", 32], ["signature", "hex", 64],
];

function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid-receive");
  return value as Record<string, unknown>;
}

function encode(value: unknown, fields: readonly Field[]): Uint8Array {
  const input = record(value);
  const chunks = fields.map(([key, kind, size]) => {
    const field = input[key];
    if (kind === "hex") {
      if (typeof field !== "string" || !new RegExp(`^[0-9a-f]{${size * 2}}$`, "u").test(field)) throw new Error("invalid-receive");
      return Uint8Array.from({ length: size }, (_, index) => parseInt(field.slice(index * 2, index * 2 + 2), 16));
    }
    if (kind === "integer" && (typeof field !== "string" || !/^(0|[1-9][0-9]{0,38})$/u.test(field))) throw new Error("invalid-receive");
    if (kind === "number" && (typeof field !== "number" || !Number.isSafeInteger(field))) throw new Error("invalid-receive");
    if (kind === "boolean" && typeof field !== "boolean") throw new Error("invalid-receive");
    let number = BigInt(field as string | number | boolean);
    if (number < 0n || number >= 1n << BigInt(size * 8)) throw new Error("invalid-receive");
    const bytes = new Uint8Array(size);
    for (let index = size - 1; index >= 0; index--) {
      bytes[index] = Number(number & 255n);
      number >>= 8n;
    }
    return bytes;
  });
  return concatenate(...chunks);
}

function concatenate(...parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((size, part) => size + part.length, 0));
  let offset = 0;
  for (const part of parts) { output.set(part, offset); offset += part.length; }
  return output;
}

function exact(value: unknown, keys: readonly string[]): void {
  const input = record(value);
  if (Object.keys(input).length !== keys.length || keys.some((key) => !Object.hasOwn(input, key))) throw new Error("invalid-receive");
}

export function encodeGrant(grant: PayerGrant): Uint8Array {
  exact(grant, GRANT.map(([key]) => key));
  return encode(grant, GRANT);
}

export function grantAuthorizationMessage(grant: PayerGrant): Uint8Array {
  encodeGrant(grant);
  return concatenate(new TextEncoder().encode("LXP:GRANT:v1"), encode(grant, GRANT.filter(([key]) => key !== "grant_id" && key !== "signature")));
}

export function encodeReceive(receive: Receive): Uint8Array {
  exact(receive, [...CORE.map(([key]) => key), "receiver_authorization", "payer_grant"]);
  exact(receive.receiver_authorization, AUTH.map(([key]) => key));
  return concatenate(new Uint8Array([0x52, 1, 0, 10]), encode(receive, CORE), encode(receive.receiver_authorization, AUTH), encodeGrant(receive.payer_grant));
}

export function receiveAuthorizationMessage(receive: Receive): Uint8Array {
  encodeReceive(receive);
  return concatenate(new TextEncoder().encode("LXP:RECEIVE:v1"), encode(receive, CORE), encode(receive.receiver_authorization, AUTH.filter(([key]) => key !== "public_key" && key !== "signature")));
}

export function decodeReceive(bytes: Uint8Array): Receive {
  if (!(bytes instanceof Uint8Array) || bytes.length !== 733 || bytes[0] !== 0x52 || bytes[1] !== 1 || bytes[2] !== 0 || bytes[3] !== 10) throw new Error("invalid-receive");
  let offset = 4;
  const decode = (fields: readonly Field[]): Record<string, unknown> => Object.fromEntries(fields.map(([key, kind, size]) => {
    const field = bytes.slice(offset, offset + size);
    offset += size;
    if (kind === "hex") return [key, Array.from(field, (byte) => byte.toString(16).padStart(2, "0")).join("")];
    let number = 0n;
    for (const byte of field) number = number * 256n + BigInt(byte);
    if (kind === "boolean" && number > 1n) throw new Error("invalid-receive");
    return [key, kind === "boolean" ? number === 1n : kind === "number" ? Number(number) : number.toString()];
  }));
  const core = decode(CORE);
  const receiver_authorization = decode(AUTH);
  const payer_grant = decode(GRANT);
  if (offset !== bytes.length) throw new Error("invalid-receive");
  return { ...core, receiver_authorization, payer_grant } as unknown as Receive;
}

export function encodeAccountOpen(asset: string): Uint8Array {
  return concatenate(new Uint8Array([0, 1]), encode({ asset }, [["asset", "hex", 32]]));
}

export function encodeGrantRevoke(grant_id: string, revocation_sequence: string): Uint8Array {
  return concatenate(new Uint8Array([0, 1]), encode({ grant_id, revocation_sequence }, [["grant_id", "hex", 32], ["revocation_sequence", "integer", 8]]));
}

export function encodeAssetSupply(asset: string, account: string, amount: string): Uint8Array {
  const body = encode({ asset, account, amount }, [["asset", "hex", 32], ["account", "hex", 32], ["amount", "integer", 16]]);
  if (BigInt(amount) === 0n) throw new Error("invalid-asset-amount");
  return concatenate(new Uint8Array([0, 1]), body);
}

function id32(value: unknown): Uint8Array {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/u.test(value)) throw new Error("invalid-register");
  return encode({ value }, [["value", "hex", 32]]);
}

function sha256(bytes: Uint8Array): Uint8Array {
  return Uint8Array.from(createHash("sha256").update(bytes).digest());
}

export function nativeAssetId(issuerDidId32: string, salt: string): string {
  return Array.from(
    sha256(concatenate(new TextEncoder().encode("LX:ASSET:v1"), id32(issuerDidId32), id32(salt))),
    (byte) => byte.toString(16).padStart(2, "0"),
  ).join("");
}

export interface AssetRegister {
  readonly issuer_did_id32: string;
  readonly salt: string;
  readonly symbol: string;
  readonly name: string;
  readonly decimals: number;
  readonly supply_cap: string;
  readonly issuer_kind: 1 | 2;
  readonly custody_ref: string;
  readonly asset_id?: string;
}

export function encodeRegister(value: AssetRegister): Uint8Array {
  const keys = ["issuer_did_id32", "salt", "symbol", "name", "decimals", "supply_cap", "issuer_kind", "custody_ref"] as const;
  exact(value, value.asset_id === undefined ? keys : [...keys, "asset_id"]);
  if (typeof value.symbol !== "string" || value.symbol.length < 1 || value.symbol.length > 16
    || !/^[\x00-\x7f]+$/u.test(value.symbol)) throw new Error("invalid-register");
  const nameBytes = new TextEncoder().encode(value.name);
  if (typeof value.name !== "string" || nameBytes.length < 1 || nameBytes.length > 32) throw new Error("invalid-register");
  if (typeof value.decimals !== "number" || !Number.isSafeInteger(value.decimals) || value.decimals < 0 || value.decimals > 38) {
    throw new Error("invalid-register");
  }
  if (value.issuer_kind !== 1 && value.issuer_kind !== 2) throw new Error("invalid-register");
  if (typeof value.custody_ref !== "string" || value.custody_ref.length % 2 !== 0 || value.custody_ref.length > 256
    || (value.custody_ref.length > 0 && !/^[0-9a-f]+$/u.test(value.custody_ref))) throw new Error("invalid-register");
  const custody = value.custody_ref.length === 0 ? new Uint8Array() : encode({ custody_ref: value.custody_ref }, [["custody_ref", "hex", value.custody_ref.length / 2]]);
  const derived = sha256(concatenate(new TextEncoder().encode("LX:ASSET:v1"), id32(value.issuer_did_id32), id32(value.salt)));
  let asset: Uint8Array;
  if (value.issuer_kind === 1) {
    if (custody.length !== 0) throw new Error("invalid-register");
    asset = derived;
    if (value.asset_id !== undefined && !equalBytes(id32(value.asset_id), asset)) throw new Error("invalid-register");
  } else {
    if (value.asset_id === undefined) throw new Error("invalid-register");
    asset = id32(value.asset_id);
  }
  const cap = encode({ supply_cap: value.supply_cap }, [["supply_cap", "integer", 16]]);
  return concatenate(
    new Uint8Array([0, 1]),
    asset,
    id32(value.salt),
    Uint8Array.from([value.symbol.length]),
    new TextEncoder().encode(value.symbol),
    Uint8Array.from([nameBytes.length]),
    nameBytes,
    Uint8Array.from([value.decimals]),
    cap,
    Uint8Array.from([value.issuer_kind, custody.length]),
    custody,
  );
}

function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}
