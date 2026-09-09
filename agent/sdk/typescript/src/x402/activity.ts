import { createHash } from "node:crypto";

export function bindReceiveActivity(canonical: Uint8Array, receive: Uint8Array, actor: string, network: number, key: string): string {
  let offset = 0;
  const take = (size: number): Uint8Array => {
    if (!Number.isSafeInteger(size) || size < 0 || offset + size > canonical.length) throw new Error("invalid-activity");
    const value = canonical.slice(offset, offset + size); offset += size; return value;
  };
  const integer = (size: number): bigint => Array.from(take(size)).reduce((value, byte) => value * 256n + BigInt(byte), 0n);
  const field = (tag: number) => { if (integer(1) !== BigInt(tag)) throw new Error("invalid-activity-field"); };
  const bytes = (maximum: number) => { const size = Number(integer(4)); if (size > maximum) throw new Error("invalid-activity-length"); return take(size); };
  const equal = (a: Uint8Array, b: Uint8Array) => a.length === b.length && a.every((value, i) => value === b[i]);
  if (canonical.length > 524288) throw new Error("invalid-activity-length");
  const version = integer(2);
  if (version < 1n || version > 4n || integer(2) !== 0x1001n || integer(1) !== 12n) throw new Error("invalid-activity-header");
  field(1); if (integer(2) !== version) throw new Error("invalid-activity-version");
  field(2); if (integer(4) !== BigInt(network)) throw new Error("activity-network-mismatch");
  field(3); if (integer(4) !== 0x10006n) throw new Error("activity-type-mismatch");
  field(4); if (new TextDecoder("utf-8", { fatal: true }).decode(bytes(255)) !== actor) throw new Error("activity-actor-mismatch");
  field(5); if (bytes(524288).length === 0) throw new Error("missing-activity-authority");
  field(6); integer(8);
  field(7); const before = integer(8); if (integer(8) < before) throw new Error("invalid-activity-time");
  field(8); if (Buffer.from(bytes(32)).toString("hex") !== key) throw new Error("activity-key-mismatch");
  field(9); integer(16);
  field(10); const hash = bytes(32);
  field(11); const payload = bytes(524288);
  field(12); if (bytes(128).length !== 64 || offset !== canonical.length) throw new Error("invalid-activity-signature");
  if (!equal(payload, receive) || !equal(hash, createHash("sha256").update("LXP/v1/payload-hash\0").update(payload).digest())) throw new Error("activity-payload-mismatch");
  return createHash("sha256").update("LXP/v1/activity-id\0").update(canonical).digest("hex");
}
