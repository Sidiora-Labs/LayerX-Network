export const MAX_NATIVE_CAPABILITIES = 238;
export const MAX_NATIVE_BALANCE_VIEWS = 32;
export const MAX_NATIVE_CAPABILITY_BYTES = 65_452;

export type NativeCapability =
  | Readonly<{ kind: "storage_read" | "storage_write" | "emit_event" | "shared_storage_read" | "shared_storage_write" }>
  | Readonly<{ kind: "call"; program: Uint8Array }>
  | Readonly<{ kind: "transfer402"; asset: Uint8Array; to: Uint8Array; maximumAmount: bigint }>
  | Readonly<{ kind: "program_spend"; ownerProgram: Uint8Array; seed: Uint8Array; sourceAccount: Uint8Array; asset: Uint8Array; to: Uint8Array; maximumAmount: bigint }>
  | Readonly<{ kind: "receipt_read"; receiptDigest: Uint8Array }>
  | Readonly<{ kind: "balance_view"; account: Uint8Array; asset: Uint8Array; receiptDigest: Uint8Array }>;

function refuse(): never { throw new TypeError("invalid native capability set"); }
function fixed(value: Uint8Array): Uint8Array {
  if (!(value instanceof Uint8Array) || value.length !== 32 || !value.some((byte) => byte !== 0)) refuse();
  return value;
}
function word(value: bigint, size: number): Uint8Array {
  if (typeof value !== "bigint" || value < 0n || value >= 1n << BigInt(size * 8)) refuse();
  const bytes = new Uint8Array(size);
  for (let index = size - 1; index >= 0; index--) { bytes[index] = Number(value & 255n); value >>= 8n; }
  return bytes;
}
function join(...parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(parts.reduce((size, part) => size + part.length, 0));
  let offset = 0; for (const part of parts) { output.set(part, offset); offset += part.length; }
  return output;
}
function compare(left: Uint8Array, right: Uint8Array): number {
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    const delta = left[index]! - right[index]!; if (delta !== 0) return delta;
  }
  return left.length - right.length;
}
function key(grant: NativeCapability): readonly Uint8Array[] {
  const rank = (value: number) => Uint8Array.of(value);
  switch (grant.kind) {
    case "storage_read": return [rank(0)];
    case "storage_write": return [rank(1)];
    case "emit_event": return [rank(2)];
    case "call": return [rank(3), grant.program];
    case "transfer402": return [rank(4), grant.asset, grant.to];
    case "program_spend": return [rank(5), grant.ownerProgram, grant.seed, grant.sourceAccount, grant.asset, grant.to];
    case "receipt_read": return [rank(6), grant.receiptDigest];
    case "balance_view": return [rank(7), grant.account, grant.asset];
    case "shared_storage_read": return [rank(8)];
    case "shared_storage_write": return [rank(9)];
    default: return refuse();
  }
}
function compareGrants(left: NativeCapability, right: NativeCapability): number {
  const leftKey = key(left), rightKey = key(right);
  for (let index = 0; index < Math.min(leftKey.length, rightKey.length); index++) {
    const delta = compare(leftKey[index]!, rightKey[index]!); if (delta !== 0) return delta;
  }
  return leftKey.length - rightKey.length;
}
function snapshot(grant: NativeCapability): NativeCapability {
  return Object.freeze(Object.fromEntries(Object.entries(grant).map(([name, value]) => [name, value instanceof Uint8Array ? new Uint8Array(value) : value]))) as NativeCapability;
}

export async function deriveNativeProgramAccount(owner: Uint8Array, seed: Uint8Array): Promise<Uint8Array> {
  fixed(owner); if (!(seed instanceof Uint8Array) || seed.length > 128) refuse();
  const bytes = join(new TextEncoder().encode("LayerX/programs/program-account/v1\0"), owner, word(BigInt(seed.length), 4), seed);
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new Uint8Array(bytes)));
}

async function encodeGrant(grant: NativeCapability): Promise<Uint8Array> {
  switch (grant.kind) {
    case "storage_read": return Uint8Array.of(1);
    case "storage_write": return Uint8Array.of(2);
    case "emit_event": return Uint8Array.of(3);
    case "call": return join(Uint8Array.of(4), fixed(grant.program));
    case "transfer402":
      if (grant.maximumAmount === 0n) refuse();
      return join(Uint8Array.of(5), fixed(grant.asset), fixed(grant.to), word(grant.maximumAmount, 16));
    case "program_spend": {
      if (grant.maximumAmount === 0n || compare(await deriveNativeProgramAccount(grant.ownerProgram, grant.seed), grant.sourceAccount) !== 0) refuse();
      return join(Uint8Array.of(9), fixed(grant.ownerProgram), word(BigInt(grant.seed.length), 2), grant.seed,
        fixed(grant.sourceAccount), fixed(grant.asset), fixed(grant.to), word(grant.maximumAmount, 16));
    }
    case "receipt_read": return join(Uint8Array.of(6), fixed(grant.receiptDigest));
    case "balance_view": return join(Uint8Array.of(10), fixed(grant.account), fixed(grant.asset), fixed(grant.receiptDigest));
    case "shared_storage_read": return Uint8Array.of(7);
    case "shared_storage_write": return Uint8Array.of(8);
    default: return refuse();
  }
}

export async function encodeNativeCapabilitySet(grants: readonly NativeCapability[]): Promise<Uint8Array> {
  if (grants.length > MAX_NATIVE_CAPABILITIES) refuse();
  const ordered = grants.map(snapshot).sort(compareGrants);
  if (ordered.filter((grant) => grant.kind === "balance_view").length > MAX_NATIVE_BALANCE_VIEWS) refuse();
  for (let index = 1; index < ordered.length; index++) if (compareGrants(ordered[index - 1]!, ordered[index]!) === 0) refuse();
  const encoded = join(word(BigInt(ordered.length), 2), ...await Promise.all(ordered.map(encodeGrant)));
  if (encoded.length > MAX_NATIVE_CAPABILITY_BYTES) refuse();
  return encoded;
}

class Cursor {
  private offset = 0;
  constructor(private readonly bytes: Uint8Array) {}
  take(length: number): Uint8Array {
    if (length < 0 || length > this.bytes.length - this.offset) refuse();
    const value = this.bytes.slice(this.offset, this.offset + length); this.offset += length; return value;
  }
  number(length: number): bigint { let value = 0n; for (const byte of this.take(length)) value = (value << 8n) | BigInt(byte); return value; }
  end(): void { if (this.offset !== this.bytes.length) refuse(); }
}
function decodeGrant(reader: Cursor): NativeCapability {
  switch (Number(reader.number(1))) {
    case 1: return { kind: "storage_read" };
    case 2: return { kind: "storage_write" };
    case 3: return { kind: "emit_event" };
    case 4: return { kind: "call", program: reader.take(32) };
    case 5: return { kind: "transfer402", asset: reader.take(32), to: reader.take(32), maximumAmount: reader.number(16) };
    case 6: return { kind: "receipt_read", receiptDigest: reader.take(32) };
    case 7: return { kind: "shared_storage_read" };
    case 8: return { kind: "shared_storage_write" };
    case 9: {
      const ownerProgram = reader.take(32), length = Number(reader.number(2)); if (length > 128) refuse();
      return { kind: "program_spend", ownerProgram, seed: reader.take(length), sourceAccount: reader.take(32), asset: reader.take(32), to: reader.take(32), maximumAmount: reader.number(16) };
    }
    case 10: return { kind: "balance_view", account: reader.take(32), asset: reader.take(32), receiptDigest: reader.take(32) };
    default: return refuse();
  }
}
export async function decodeNativeCapabilitySet(bytes: Uint8Array): Promise<readonly NativeCapability[]> {
  if (!(bytes instanceof Uint8Array)) refuse();
  bytes = new Uint8Array(bytes);
  if (bytes.length < 2 || bytes.length > MAX_NATIVE_CAPABILITY_BYTES) refuse();
  const reader = new Cursor(bytes), count = Number(reader.number(2)); if (count > MAX_NATIVE_CAPABILITIES) refuse();
  const grants = Array.from({ length: count }, () => decodeGrant(reader)); reader.end();
  if (compare(await encodeNativeCapabilitySet(grants), bytes) !== 0) refuse();
  return Object.freeze(grants.map(snapshot));
}

export async function narrowNativeCapabilitySet(parent: readonly NativeCapability[], requested: readonly NativeCapability[]): Promise<readonly NativeCapability[]> {
  if (parent.length > MAX_NATIVE_CAPABILITIES || requested.length > MAX_NATIVE_CAPABILITIES) refuse();
  const parentSnapshot = parent.map(snapshot), childSnapshot = requested.map(snapshot);
  const parents = await decodeNativeCapabilitySet(await encodeNativeCapabilitySet(parentSnapshot));
  const children = await decodeNativeCapabilitySet(await encodeNativeCapabilitySet(childSnapshot));
  for (const child of children) {
    const ancestor = parents.find((grant) => compareGrants(grant, child) === 0); if (ancestor === undefined) refuse();
    if ((child.kind === "transfer402" && ancestor.kind === "transfer402" || child.kind === "program_spend" && ancestor.kind === "program_spend") && child.maximumAmount > ancestor.maximumAmount) refuse();
    if (child.kind === "balance_view" && ancestor.kind === "balance_view" && compare(child.receiptDigest, ancestor.receiptDigest) !== 0) refuse();
  }
  return children;
}
