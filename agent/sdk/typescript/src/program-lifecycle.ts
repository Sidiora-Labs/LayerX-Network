import { createHash } from "node:crypto";
import { bindSignedProgramLifecycle, type DecodedSignedProgramCall } from "./program-wire.js";

export class NativeProgramLifecycleRequest {
  readonly #payload: Uint8Array;
  readonly #signed: Uint8Array;
  private constructor(readonly ordinal: 1 | 2 | 7, payload: Uint8Array, signed: Uint8Array) {
    this.#payload = new Uint8Array(payload); this.#signed = new Uint8Array(signed);
    Object.freeze(this);
  }
  static deploy(value: NativeProgramDeploy, signed: Uint8Array): NativeProgramLifecycleRequest { return new NativeProgramLifecycleRequest(1, encodeNativeProgramDeploy(value), signed); }
  static upgrade(value: NativeProgramUpgrade, signed: Uint8Array): NativeProgramLifecycleRequest { return new NativeProgramLifecycleRequest(2, encodeNativeProgramUpgrade(value), signed); }
  static windDown(value: NativeProgramWindDown, signed: Uint8Array): NativeProgramLifecycleRequest { return new NativeProgramLifecycleRequest(7, encodeNativeProgramWindDown(value), signed); }
  async bind(key?: string): Promise<DecodedSignedProgramCall> { return bindSignedProgramLifecycle(this.#signed, this.#payload, this.ordinal, key); }
}

export type ProgramLifecycleSubmission = Readonly<{ state: "acknowledged"; activity_id: string; receipt: string; result_code: number }>
  | Readonly<{ state: "unknown"; activity_id: string; idempotency_key: string; retained_signed_activity: string }>;

export interface NativeProgramDeploy {
  readonly programId: Uint8Array;
  readonly guestAbi: 1 | 2 | 3;
  readonly policy: 0 | 1;
  readonly authority: Uint8Array;
  readonly newHash: Uint8Array;
  readonly wasm: Uint8Array;
  readonly interface?: Uint8Array;
}
export interface NativeProgramUpgrade {
  readonly programId: Uint8Array;
  readonly guestAbi: 1 | 2 | 3;
  readonly oldHash: Uint8Array;
  readonly newHash: Uint8Array;
  readonly wasm: Uint8Array;
  readonly migrationHook: Uint8Array;
  readonly clearInterface: boolean;
  readonly interface?: Uint8Array;
}
export type NativeProgramWindDown = Readonly<{ programId: Uint8Array }> & (
  Readonly<{ operation: "route"; account: Uint8Array; asset: Uint8Array; destination: Uint8Array; seed: Uint8Array }>
  | Readonly<{ operation: "deprecate"; exitProgram: Uint8Array; deadlineBatch: bigint }>
  | Readonly<{ operation: "tombstone" }>
  | Readonly<{ operation: "exit"; account: Uint8Array }>);

function fixed(value: Uint8Array): Uint8Array {
  if (value.length !== 32) throw new TypeError("expected 32 bytes");
  return value;
}
function integer(value: number, bytes: 2 | 4): Uint8Array {
  if (!Number.isInteger(value) || value < 0 || value >= 2 ** (bytes * 8)) throw new TypeError("integer bounds");
  const output = new Uint8Array(bytes);
  const view = new DataView(output.buffer);
  if (bytes === 2) view.setUint16(0, value); else view.setUint32(0, value);
  return output;
}
function join(...parts: readonly Uint8Array[]): Uint8Array { return Buffer.concat(parts); }
function code(value: NativeProgramDeploy | NativeProgramUpgrade): void {
  if (fixed(value.programId).every(byte => byte === 0) || ![1, 2, 3].includes(value.guestAbi)
    || value.wasm.length > 1_048_576 || !Buffer.from(value.wasm.slice(0, 8)).equals(Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]))
    || !createHash("sha256").update(value.wasm).digest().equals(fixed(value.newHash))) throw new TypeError("invalid program code");
}
export function encodeNativeProgramDeploy(value: NativeProgramDeploy): Uint8Array {
  code(value);
  if (![0, 1].includes(value.policy) || (value.policy === 0) !== fixed(value.authority).every(byte => byte === 0)
    || value.interface !== undefined && (value.interface.length === 0 || value.interface.length > 952)) throw new TypeError("deploy policy or interface");
  return join(value.programId, integer(value.guestAbi, 2), Uint8Array.of(value.policy, 0), value.authority, value.newHash,
    integer(value.wasm.length, 4), ...(value.interface === undefined ? [] : [integer(value.interface.length, 4), value.interface]), value.wasm);
}
export function decodeNativeProgramDeploy(payload: Uint8Array): NativeProgramDeploy {
  if (payload.length < 104 || payload[35] !== 0) throw new TypeError("deploy framing");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  const length = view.getUint32(100);
  let offset = 104;
  let iface: Uint8Array | undefined;
  if (payload.length !== offset + length) {
    if (payload.length < 108) throw new TypeError("interface framing");
    offset = 108 + view.getUint32(104); iface = payload.slice(108, offset);
  }
  const value: NativeProgramDeploy = { programId: payload.slice(0, 32), guestAbi: view.getUint16(32) as 1 | 2 | 3,
    policy: payload[34] as 0 | 1, authority: payload.slice(36, 68), newHash: payload.slice(68, 100), wasm: payload.slice(offset),
    ...(iface === undefined ? {} : { interface: iface }) };
  if (value.wasm.length !== length || !Buffer.from(encodeNativeProgramDeploy(value)).equals(payload)) throw new TypeError("noncanonical deploy");
  return value;
}
export function encodeNativeProgramUpgrade(value: NativeProgramUpgrade): Uint8Array {
  code(value);
  if (value.migrationHook.length > 65535 || typeof value.clearInterface !== "boolean" || value.clearInterface && value.interface === undefined
    || value.interface !== undefined && (value.interface.length > 952 || value.interface.length === 0 && !value.clearInterface)) throw new TypeError("upgrade flags or interface");
  const flags = Number(value.migrationHook.length !== 0) | (Number(value.clearInterface) << 1);
  return join(value.programId, integer(value.guestAbi, 2), Uint8Array.of(flags, 0), fixed(value.oldHash), value.newHash,
    integer(value.migrationHook.length, 2), integer(value.wasm.length, 4), ...(value.interface === undefined ? [] : [integer(value.interface.length, 4)]),
    value.migrationHook, value.interface ?? new Uint8Array(), value.wasm);
}
export function decodeNativeProgramUpgrade(payload: Uint8Array): NativeProgramUpgrade {
  if (payload.length < 106 || payload[35] !== 0 || (payload[34]! & 0xfc) !== 0) throw new TypeError("upgrade framing");
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  const hook = view.getUint16(100), wasm = view.getUint32(102), clearInterface = (payload[34]! & 2) !== 0;
  let offset = 106, iface: number | undefined;
  if (payload.length !== 106 + hook + wasm || clearInterface) {
    if (payload.length < 110) throw new TypeError("interface framing");
    offset = 110; iface = view.getUint32(106);
  }
  const value: NativeProgramUpgrade = { programId: payload.slice(0, 32), guestAbi: view.getUint16(32) as 1 | 2 | 3,
    oldHash: payload.slice(36, 68), newHash: payload.slice(68, 100), migrationHook: payload.slice(offset, offset + hook), clearInterface,
    ...(iface === undefined ? {} : { interface: payload.slice(offset + hook, offset + hook + iface) }), wasm: payload.slice(offset + hook + (iface ?? 0)) };
  if (value.wasm.length !== wasm || !Buffer.from(encodeNativeProgramUpgrade(value)).equals(payload)) throw new TypeError("noncanonical upgrade");
  return value;
}
export function encodeNativeProgramWindDown(value: NativeProgramWindDown): Uint8Array {
  if (fixed(value.programId).every(byte => byte === 0)) throw new TypeError("program id");
  switch (value.operation) {
    case "route":
      if (value.seed.length > 128) throw new TypeError("seed bound");
      return join(value.programId, Uint8Array.of(1), fixed(value.account), fixed(value.asset), fixed(value.destination), integer(value.seed.length, 2), value.seed);
    case "deprecate": {
      if (value.deadlineBatch < 0n || value.deadlineBatch >= 1n << 64n) throw new TypeError("deadline bound");
      const deadline = new Uint8Array(8); new DataView(deadline.buffer).setBigUint64(0, value.deadlineBatch);
      return join(value.programId, Uint8Array.of(2), fixed(value.exitProgram), deadline);
    }
    case "tombstone": return join(value.programId, Uint8Array.of(3));
    case "exit": return join(value.programId, Uint8Array.of(4), fixed(value.account));
    default: throw new TypeError("wind-down operation");
  }
}
export function decodeNativeProgramWindDown(payload: Uint8Array): NativeProgramWindDown {
  if (payload.length < 33) throw new TypeError("wind-down framing");
  const programId = payload.slice(0, 32);
  let value: NativeProgramWindDown;
  if (payload[32] === 1 && payload.length >= 131) value = { programId, operation: "route", account: payload.slice(33, 65), asset: payload.slice(65, 97), destination: payload.slice(97, 129), seed: payload.slice(131) };
  else if (payload[32] === 2 && payload.length === 73) value = { programId, operation: "deprecate", exitProgram: payload.slice(33, 65), deadlineBatch: new DataView(payload.buffer, payload.byteOffset, payload.byteLength).getBigUint64(65) };
  else if (payload[32] === 3 && payload.length === 33) value = { programId, operation: "tombstone" };
  else if (payload[32] === 4 && payload.length === 65) value = { programId, operation: "exit", account: payload.slice(33) };
  else throw new TypeError("wind-down framing");
  if (!Buffer.from(encodeNativeProgramWindDown(value)).equals(payload)) throw new TypeError("noncanonical wind-down");
  return value;
}
