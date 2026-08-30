import type { ProgramCall, ProgramOutcome, ProgramUsage } from "./programs.js";
import type { ProgramReceiptOutcome } from "./verifier.js";

const ACTIVITY_DOMAIN = bytes("LXP/v1/activity-id\0");
const PAYLOAD_DOMAIN = bytes("LXP/v1/payload-hash\0");
const CALL_DOMAIN = bytes("LayerX/programs/call/v1\0");
const EXECUTION_V2 = bytes("LXP/program-execution/v2\0");
const EXECUTION_V3 = bytes("LXP/program-execution/v3\0");
const EXECUTION_V4 = bytes("LXP/program-execution/v4\0");
const OCCUPANCY = bytes("LXP/program-execution-with-occupancy/v1\0");
const AUTHORITY = bytes("LXP/program-execution-with-transfer-authority/v2\0");
const FAILURE = bytes("LXP/programs/failure-detail/v1\0");
const RESOURCE = bytes("LXP/programs/resource-detail/v1\0");
const SETTLEMENT = bytes("LXP/programs/settlement-failure/v1\0");
const CALLBACK = bytes("LXP/programs/callback-failure/v1\0");
const MAX_U64 = 0xffff_ffff_ffff_ffffn;
const MAX_U128 = 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffffn;
const MAX_TRACE_EVIDENCE_BYTES = 34 + 65_536 * 52;
const MAX_GRAPH_EVIDENCE_BYTES = bytes("LayerX/programs/call-graph/v1\0").length + 32 + 16 + 8 + 64 * 68;
const MAX_CALL_RESPONSE_BYTES = 1_048_576;
const CAPABILITY_TAGS = Object.freeze({ storage_read: 1, storage_write: 2, transfer: 3, emit_event: 4, compose: 5 } as const);

export interface DecodedSignedProgramCall {
  readonly activityId: string;
  readonly idempotencyKey: string;
  readonly canonicalBytes: Uint8Array;
}

export interface DecodedProgramTerminal {
  readonly outcome: ProgramOutcome;
  readonly usage: ProgramUsage;
}

export async function decodeSignedProgramCall(
  call: ProgramCall,
  expectedIdempotencyKey?: string,
): Promise<DecodedSignedProgramCall> {
  const canonical = new Uint8Array(call.signedActivity);
  const reader = new Reader(canonical);
  if (reader.u16() !== 1 || reader.u16() !== 0x1001 || reader.byte() !== 12) fail("signed activity header");
  field(reader, 1); const protocolVersion = reader.u16();
  if (protocolVersion !== 1 && protocolVersion !== 2) fail("signed activity protocol");
  field(reader, 2); reader.u32();
  field(reader, 3); if (reader.u32() !== 0x0009_0003) fail("signed activity type");
  field(reader, 4); const actor = reader.sizedU32(255);
  field(reader, 5); const authority = reader.sizedU32(524_288);
  field(reader, 6); reader.u64();
  field(reader, 7); const notBefore = reader.u64(); const notAfter = reader.u64();
  field(reader, 8); const idempotency = reader.sizedU32(32, 32);
  field(reader, 9); reader.u128();
  field(reader, 10); const payloadHash = reader.sizedU32(32, 32);
  field(reader, 11); const payload = reader.sizedU32(524_288);
  field(reader, 12); const signature = reader.sizedU32(128);
  reader.end();
  if (notAfter < notBefore) fail("signed activity bounds");
  if (!equal(payloadHash, await sha256(PAYLOAD_DOMAIN, payload))) fail("signed activity payload hash");
  decodeCallPayload(payload, call);
  const idempotencyKey = hex(idempotency);
  if (expectedIdempotencyKey !== undefined && idempotencyKey !== expectedIdempotencyKey) fail("signed activity idempotency");
  return Object.freeze({
    activityId: hex(await sha256(ACTIVITY_DOMAIN, canonical)),
    idempotencyKey,
    canonicalBytes: canonical,
  });
}

export async function decodeAndVerifyProgramTerminal(
  terminalPayload: Uint8Array,
  callGraph: Uint8Array,
  expectedProgramId: string,
  receipt: ProgramReceiptOutcome,
  protocolVersion: number,
): Promise<DecodedProgramTerminal> {
  if (callGraph.length === 0 || !equal(await sha256(callGraph), receipt.callGraphRoot)) fail("program call graph root");
  let inner = terminalPayload;
  let authorization: Uint8Array | undefined;
  let authorityRoot: Uint8Array | undefined;
  let occupancy: Uint8Array | undefined;
  if (starts(inner, AUTHORITY)) {
    const wrapper = new Reader(inner.subarray(AUTHORITY.length));
    inner = wrapper.sizedU32(1_048_576);
    authorization = wrapper.sizedU32(1_048_576);
    authorityRoot = wrapper.fixed(32);
    wrapper.end();
  }
  if (starts(inner, OCCUPANCY)) {
    const wrapper = new Reader(inner.subarray(OCCUPANCY.length));
    inner = wrapper.sizedU32(1_048_576);
    occupancy = wrapper.sizedU32(65_536);
    wrapper.end();
  }
  if (starts(inner, AUTHORITY) || starts(inner, OCCUPANCY)) fail("program terminal wrapper order");

  let outcome: ProgramOutcome;
  let usage: ProgramUsage | undefined;
  let candidate = false;
  let successfulExecution = false;
  if (starts(inner, EXECUTION_V2) || starts(inner, EXECUTION_V3)) {
    if (receipt.terminalKind !== 1 || receipt.abiVersion !== 1) fail("legacy terminal kind");
    const traced = starts(inner, EXECUTION_V3);
    const decoded = decodeLegacy(inner.subarray((traced ? EXECUTION_V3 : EXECUTION_V2).length), traced);
    bindExecutionMetadata(decoded.runtime, 1, 0, decoded.metering, decoded.usage, receipt);
    outcome = Object.freeze({ kind: "legacy_completed", code: receipt.resultCode, values: decoded.values });
    usage = decoded.usage;
    successfulExecution = true;
  } else if (starts(inner, EXECUTION_V4)) {
    candidate = true;
    const decoded = decodeCandidate(inner.subarray(EXECUTION_V4.length));
    if (decoded.kind !== receipt.terminalKind || receipt.abiVersion !== 2 || decoded.program !== expectedProgramId) fail("candidate terminal binding");
    bindExecutionMetadata(decoded.runtime, 2, decoded.fee, decoded.metering, decoded.usage, receipt);
    if (!equal(decoded.graph, callGraph)) fail("candidate call graph");
    if (decoded.outcome === "success") {
      outcome = Object.freeze({ kind: "completed", code: decoded.code, response: hex(decoded.response) });
      successfulExecution = true;
    } else if (decoded.outcome === "failure") {
      outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "guest_refused", code: receipt.resultCode }) });
    } else {
      outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "resource" }) });
    }
    usage = decoded.usage;
  } else if (starts(inner, FAILURE)) {
    if (receipt.terminalKind !== 2) fail("failure terminal kind");
    decodeFailure(inner.subarray(FAILURE.length));
    outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "guest_refused", code: receipt.resultCode }) });
  } else if (starts(inner, RESOURCE)) {
    if (receipt.terminalKind !== 3) fail("resource terminal kind");
    const reader = new Reader(inner.subarray(RESOURCE.length));
    decodeResource(reader, false);
    reader.end();
    outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "resource" }) });
  } else if (starts(inner, SETTLEMENT)) {
    if (receipt.terminalKind !== 2 || inner.length !== SETTLEMENT.length + 1 || !validTransferError(inner[SETTLEMENT.length] ?? 0)) fail("settlement terminal");
    outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "guest_refused", code: receipt.resultCode }) });
  } else if (starts(inner, CALLBACK)) {
    if (receipt.terminalKind !== 2 || inner.length !== CALLBACK.length + 5) fail("callback terminal");
    outcome = Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "guest_refused", code: receipt.resultCode }) });
  } else {
    fail("unknown terminal domain");
  }

  const zero = new Uint8Array(32);
  const occupancyRequired = protocolVersion === 2 && successfulExecution;
  if ((occupancy !== undefined) !== occupancyRequired) fail("occupancy attachment presence");
  if (occupancy !== undefined) {
    if (occupancy.length === 0) {
      if (!equal(receipt.occupancyEvidenceDigest, zero) || !equal(receipt.occupancyTransferRoot, zero)
        || receipt.occupancyByteBatches !== 0n || receipt.occupancyFeeUnits !== 0n) fail("empty occupancy attachment");
    } else if (!equal(await sha256(occupancy), receipt.occupancyEvidenceDigest)) {
      fail("occupancy evidence digest");
    }
  } else if (!equal(receipt.occupancyEvidenceDigest, zero) || !equal(receipt.occupancyTransferRoot, zero)
    || receipt.occupancyByteBatches !== 0n || receipt.occupancyFeeUnits !== 0n) {
    fail("unexpected occupancy commitment");
  }
  const transferPresent = !equal(receipt.transferRoot, zero);
  if (candidate ? (authorization !== undefined) !== transferPresent : authorization !== undefined) fail("transfer authority presence");
  if (authorization !== undefined && (authorization.length === 0 || authorityRoot === undefined || !equal(authorityRoot, receipt.transferRoot))) fail("transfer authority root");
  if (protocolVersion !== 1 && protocolVersion !== 2) fail("program receipt protocol");
  const boundUsage = usage ?? receiptUsage(receipt);
  return Object.freeze({ outcome, usage: boundUsage });
}

function decodeCallPayload(payload: Uint8Array, call: ProgramCall): void {
  const reader = new Reader(payload);
  if (!equal(reader.fixed(CALL_DOMAIN.length), CALL_DOMAIN)) fail("program call domain");
  if (hex(reader.fixed(32)) !== call.programId || reader.u64() !== call.budget.fuel || reader.u128() !== call.budget.feeLimit) fail("program call budget");
  const count = reader.u16();
  if (count !== call.capabilities.length || count > 5) fail("program call capabilities");
  let prior = 0;
  for (let index = 0; index < count; index += 1) {
    const tag = reader.byte();
    const expected = CAPABILITY_TAGS[call.capabilities[index] ?? "storage_read"];
    if (tag !== expected || tag <= prior) fail("program call capability tag");
    prior = tag;
  }
  const calldata = reader.sizedU32(1_048_576);
  reader.end();
  if (!equal(calldata, call.calldata)) fail("program call calldata");
}

interface DecodedExecution {
  readonly runtime: number;
  readonly fee: number;
  readonly metering: number;
  readonly usage: ProgramUsage;
}

function decodeLegacy(encoded: Uint8Array, traced: boolean): DecodedExecution & { readonly values: readonly unknown[] } {
  const reader = new Reader(encoded);
  const runtime = reader.u16(); const abi = reader.u16(); const metering = reader.u32();
  if (runtime === 0 || abi !== 1 || metering === 0) fail("legacy metadata");
  const count = reader.u128();
  if (count > BigInt(Math.floor(reader.remaining() / 5))) fail("legacy value count");
  const values: unknown[] = [];
  for (let index = 0n; index < count; index += 1n) {
    const tag = reader.byte();
    if (tag === 1) values.push(Object.freeze({ type: "i32", value: reader.i32() }));
    else if (tag === 2) values.push(Object.freeze({ type: "i64", value: reader.i64().toString() }));
    else fail("legacy value tag");
  }
  const usage = usageValue(reader.u64(), reader.u64(), reader.u64(), reader.u64(), reader.u32(), 0n, reader.u128());
  if (traced) {
    if (reader.byte() !== 1 || reader.sizedU64(MAX_TRACE_EVIDENCE_BYTES).length > MAX_TRACE_EVIDENCE_BYTES) fail("legacy trace");
  }
  reader.end();
  return { runtime, fee: 0, metering, usage, values: Object.freeze(values) };
}

type Candidate = DecodedExecution & {
  readonly program: string;
  readonly graph: Uint8Array;
  readonly kind: 1 | 2 | 3;
} & (
  | { readonly outcome: "success"; readonly code: number; readonly response: Uint8Array }
  | { readonly outcome: "failure" }
  | { readonly outcome: "resource" }
);

function decodeCandidate(encoded: Uint8Array): Candidate {
  const reader = new Reader(encoded);
  const runtime = reader.u16(); const fee = reader.u32(); const metering = reader.u32();
  if (runtime === 0 || fee === 0 || metering === 0) fail("candidate metadata");
  const count = reader.u64();
  if (count > BigInt(Math.floor(reader.remaining() / 5))) fail("candidate value count");
  for (let index = 0n; index < count; index += 1n) {
    const tag = reader.byte();
    if (tag === 1) reader.i32(); else if (tag === 2) reader.i64(); else fail("candidate value tag");
  }
  const usage = usageValue(reader.u64(), reader.u64(), reader.u64(), reader.u64(), reader.u32(), reader.u64(), reader.u128());
  const traceTag = reader.byte();
  if (traceTag === 1) reader.sizedU64(MAX_TRACE_EVIDENCE_BYTES); else if (traceTag !== 0) fail("candidate trace tag");
  const program = hex(reader.fixed(32));
  if (reader.u16() !== 2) fail("candidate ABI");
  const tag = reader.byte();
  let variant:
    | { readonly outcome: "success"; readonly code: number; readonly response: Uint8Array }
    | { readonly outcome: "failure" }
    | { readonly outcome: "resource" };
  let kind: 1 | 2 | 3;
  if (tag === 0) {
    const code = reader.i32();
    if (code < 0) fail("candidate result code");
    variant = { outcome: "success", code, response: reader.sizedU64(MAX_CALL_RESPONSE_BYTES) };
    kind = 1;
  } else if (tag === 1) {
    decodeProgramFailure(reader.sizedU64(4_136));
    variant = { outcome: "failure" };
    kind = 2;
  } else if (tag === 2) {
    decodeResource(reader, true, usage);
    variant = { outcome: "resource" };
    kind = 3;
  } else fail("candidate outcome tag");
  const graph = reader.sizedU64(MAX_GRAPH_EVIDENCE_BYTES);
  reader.end();
  return { runtime, fee, metering, usage, program, graph, kind, ...variant } as Candidate;
}

function decodeFailure(encoded: Uint8Array): void {
  const reader = new Reader(encoded);
  const tag = reader.byte(); const payload = new Reader(reader.sizedU32(1_048_576)); reader.end();
  if (tag === 1) decodeProgramFailure(payload.rest());
  else if (tag === 2) decodeComposition(payload);
  else if (tag === 3) decodeEntrypoint(payload);
  else if (tag === 4) decodeAbi(payload);
  else fail("failure terminal tag");
  payload.end();
}

function decodeComposition(reader: Reader): void {
  const tag = reader.byte();
  if ([1, 9, 10, 11, 20, 21, 22].includes(tag)) return;
  if (tag === 2) { if (![1, 2].includes(reader.byte()) || ![1, 2].includes(reader.byte())) fail("composition revision"); return; }
  if (tag === 23) { reader.fixed(76); reader.fixed(76); return; }
  if (tag === 3 || tag === 4) { reader.fixed(32); return; }
  if (tag === 5 || tag === 6 || tag === 7) { reader.u32(); reader.u32(); return; }
  if (tag === 8) { reader.fixed(32); reader.u32(); reader.u32(); return; }
  if (tag === 12) { reader.i32(); return; }
  if (tag === 13) { reader.u64(); reader.u64(); return; }
  if (tag === 14) { reader.fixed(32); reader.i32(); return; }
  if (tag === 15) { decodeProgramFailure(reader.rest()); return; }
  if (tag === 16) { decodeAbiFields(reader); return; }
  if (tag === 17) { decodeFault(reader); return; }
  if (tag === 18) { decodeMeterFailure(reader); return; }
  if (tag === 19) { decodeResponseFailure(reader); return; }
  fail("composition failure tag");
}

function decodeEntrypoint(reader: Reader): void {
  const tag = reader.byte();
  if (tag === 1) { reader.u64(); reader.u64(); }
  else if ([2, 3, 4].includes(tag)) return;
  else if (tag === 5 || tag === 6) reader.i32();
  else if (tag === 7) decodeFault(reader);
  else if (tag === 8) decodeMeterFailure(reader);
  else fail("entrypoint failure tag");
}

function decodeAbi(reader: Reader): void {
  const tag = reader.byte();
  if ([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 14, 15].includes(tag)) return;
  if (tag === 11) { const storage = reader.byte(); if (storage < 1 || storage > 11) fail("storage failure tag"); return; }
  if (tag === 12) { decodeMeterFailure(reader); return; }
  fail("ABI failure tag");
}

function decodeAbiFields(reader: Reader): void { decodeAbi(reader); }

function decodeMeterFailure(reader: Reader): void {
  const tag = reader.byte();
  if (tag === 1) {
    const resource = reader.byte(); const limit = reader.u64(); const attempted = reader.u64();
    if (resource < 1 || resource > 7 || attempted <= limit) fail("meter budget failure");
  } else if (tag === 2) {
    const resource = reader.byte(); if (resource < 1 || resource > 7) fail("meter counter failure");
  } else if (tag !== 3) fail("meter failure tag");
}

function decodeFault(reader: Reader): void {
  const tag = reader.byte();
  if (tag === 1 || tag === 2 || tag === 16) { new TextDecoder("utf-8", { fatal: true }).decode(reader.sizedU32(1_048_576)); }
  else if ([3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15].includes(tag)) return;
  else if (tag === 14) decodeMeterFailure(reader);
  else fail("execution fault tag");
}

function decodeResponseFailure(reader: Reader): void {
  const tag = reader.byte();
  if (tag === 1 || tag === 2) { reader.u64(); reader.u64(); }
  else if (tag === 3 || tag === 4) return;
  else if (tag === 5) { reader.i32(); reader.i32(); }
  else if (tag === 6) decodeMeterFailure(reader);
  else fail("response failure tag");
}

function decodeProgramFailure(encoded: Uint8Array): void {
  const reader = new Reader(encoded);
  const program = reader.fixed(32); const refusalClass = reader.u32(); const reason = reader.sizedU32(4_096);
  reader.end();
  if (program.every((value) => value === 0) || ![1, 2, 3, 4, 5, 254, 255].includes(refusalClass)
    || ((refusalClass === 254 || refusalClass === 255) && reason.length !== 0)) fail("program failure payload");
}

function decodeResource(reader: Reader, candidate: boolean, usage?: ProgramUsage): void {
  const tag = reader.byte(); const resource = reader.byte();
  if (candidate ? resource > 6 : resource < 1 || resource > 7) fail("resource kind");
  if (candidate ? tag === 0 : tag === 1) {
    const limit = reader.u64(); const attempted = reader.u64();
    if (attempted <= limit) fail("resource refusal bounds");
    if (candidate && usage !== undefined && usageFor(usage, resource) > limit) fail("resource refusal usage");
  } else if (!(candidate ? tag === 1 : tag === 2)) fail("resource refusal tag");
}

function usageFor(usage: ProgramUsage, resource: number): bigint {
  return [usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes, usage.storage_write_bytes,
    usage.output_values.toString(), usage.output_bytes, "0"][resource] !== undefined
    ? BigInt([usage.cpu_fuel, usage.memory_bytes, usage.storage_read_bytes, usage.storage_write_bytes,
      usage.output_values.toString(), usage.output_bytes, "0"][resource] ?? "0") : 0n;
}

function bindExecutionMetadata(runtime: number, abi: number, fee: number, metering: number, usage: ProgramUsage, receipt: ProgramReceiptOutcome): void {
  if (runtime !== receipt.runtimeVersion || abi !== receipt.abiVersion || fee !== receipt.feeScheduleVersion
    || metering !== receipt.meteringScheduleVersion || !sameUsage(usage, receiptUsage(receipt))) fail("terminal receipt metadata");
}

function receiptUsage(receipt: ProgramReceiptOutcome): ProgramUsage {
  return usageValue(receipt.cpuFuel, receipt.memoryBytes, receipt.storageReadBytes, receipt.storageWriteBytes,
    receipt.outputValues, receipt.outputBytes, receipt.feeUnits);
}

function usageValue(cpu: bigint, memory: bigint, read: bigint, write: bigint, values: number, output: bigint, fee: bigint): ProgramUsage {
  return Object.freeze({ cpu_fuel: cpu.toString(), memory_bytes: memory.toString(), storage_read_bytes: read.toString(),
    storage_write_bytes: write.toString(), output_values: values, output_bytes: output.toString(), fee_units: fee.toString() });
}

function sameUsage(left: ProgramUsage, right: ProgramUsage): boolean {
  return left.cpu_fuel === right.cpu_fuel && left.memory_bytes === right.memory_bytes
    && left.storage_read_bytes === right.storage_read_bytes && left.storage_write_bytes === right.storage_write_bytes
    && left.output_values === right.output_values && left.output_bytes === right.output_bytes && left.fee_units === right.fee_units;
}

function validTransferError(tag: number): boolean { return tag >= 1 && tag <= 12; }
function field(reader: Reader, expected: number): void { if (reader.byte() !== expected) fail("signed activity field tag"); }
function starts(value: Uint8Array, prefix: Uint8Array): boolean { return value.length >= prefix.length && equal(value.subarray(0, prefix.length), prefix); }
function bytes(value: string): Uint8Array { return new TextEncoder().encode(value); }
function hex(value: Uint8Array): string { return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(""); }
function equal(left: Uint8Array, right: Uint8Array): boolean { if (left.length !== right.length) return false; let difference = 0; for (let index = 0; index < left.length; index += 1) difference |= (left[index] ?? 0) ^ (right[index] ?? 0); return difference === 0; }
function concatenate(...values: readonly Uint8Array[]): Uint8Array { const result = new Uint8Array(values.reduce((sum, value) => sum + value.length, 0)); let offset = 0; for (const value of values) { result.set(value, offset); offset += value.length; } return result; }
async function sha256(...values: readonly Uint8Array[]): Promise<Uint8Array> { const input = concatenate(...values); const encoded = new ArrayBuffer(input.length); new Uint8Array(encoded).set(input); return new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", encoded)); }
function fail(boundary: string): never { throw new TypeError(`invalid ${boundary}`); }

class Reader {
  private offset = 0;
  public constructor(private readonly value: Uint8Array) {}
  public remaining(): number { return this.value.length - this.offset; }
  public fixed(length: number): Uint8Array { const end = this.offset + length; if (!Number.isSafeInteger(length) || length < 0 || end > this.value.length) fail("canonical bytes"); const result = this.value.subarray(this.offset, end); this.offset = end; return result; }
  public byte(): number { return this.fixed(1)[0] ?? fail("canonical byte"); }
  public u16(): number { const value = this.fixed(2); return ((value[0] ?? 0) << 8) | (value[1] ?? 0); }
  public u32(): number { const value = this.fixed(4); return ((value[0] ?? 0) * 0x1000000) + ((value[1] ?? 0) << 16) + ((value[2] ?? 0) << 8) + (value[3] ?? 0); }
  public u64(): bigint { return this.unsigned(8); }
  public u128(): bigint { return this.unsigned(16); }
  public i32(): number { const value = this.u32(); return value > 0x7fff_ffff ? value - 0x1_0000_0000 : value; }
  public i64(): bigint { const value = this.u64(); return value > 0x7fff_ffff_ffff_ffffn ? value - 0x1_0000_0000_0000_0000n : value; }
  public sizedU32(maximum: number, exact?: number): Uint8Array { const length = this.u32(); if (length > maximum || (exact !== undefined && length !== exact)) fail("canonical u32 length"); return this.fixed(length); }
  public sizedU64(maximum: number): Uint8Array { const length = this.u64(); if (length > BigInt(maximum)) fail("canonical u64 length"); return this.fixed(Number(length)); }
  public rest(): Uint8Array { return this.fixed(this.remaining()); }
  public end(): void { if (this.remaining() !== 0) fail("trailing canonical bytes"); }
  private unsigned(length: number): bigint { let result = 0n; for (const value of this.fixed(length)) result = (result << 8n) | BigInt(value); if ((length === 8 && result > MAX_U64) || (length === 16 && result > MAX_U128)) fail("canonical integer"); return result; }
}
