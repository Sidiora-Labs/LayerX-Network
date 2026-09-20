import { decodeNativeProgramCall, encodeNativeProgramCall } from "./native-program-call.js";
import { decodeNativeCapabilitySet } from "./native-capabilities.js";
import type { ProgramCall, ProgramOutcome, ProgramUsage } from "./programs.js";
import type { ProgramReceiptOutcome, ProtocolReceipt } from "./verifier.js";

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
const PRE_RUNTIME_FAILURE = bytes("LXP/v1/context-hash\0LXP/programs/pre-runtime-failure/v1\0");
const EMPTY_CALL_GRAPH = bytes("LXP/v1/context-hash\0LXP/programs/empty-call-graph/v1\0");
const TRANSFER_SET_V1 = bytes("LayerX/programs/402LXP/transfer-set/v1\0");
const TRANSFER_SET_V2 = bytes("LayerX/programs/402LXP/transfer-set/v2\0");
const ACCOUNT_BOUND_SET = bytes("LayerX/programs/402LXP/account-bound-set/v1\0");
const PROGRAM_AUTHORITY = bytes("LayerX/programs/402LXP/program-authority/v1\0");
const PROGRAM_FUNDING = bytes("LayerX/programs/402LXP/program-funding/v1\0");
const PROGRAM_ACCOUNT = bytes("LayerX/programs/program-account/v1\0");
const EVENT_ENVELOPE = bytes("LayerX/programs/events/v1\0");
const OCCUPANCY_V1 = bytes("LXP/storage-occupancy-settlement/v1\0");
const OCCUPANCY_V2 = bytes("LXP/storage-occupancy-settlement/v2\0");
const OCCUPANCY_V3 = bytes("LXP/storage-occupancy-settlement/v3\0");
const OCCUPANCY_MANDATE = bytes("LXP/storage-occupancy-mandate/v1\0");
const MERKLE_LEAF = bytes("LXP/v1/merkle-leaf\0");
const MERKLE_INTERNAL = bytes("LXP/v1/merkle-internal\0");
const ACCOUNT_DERIVATION = bytes("LX:ACCOUNT:v1");
const DID_DERIVATION = bytes("LXP/v1/did-id\0");
const FEE_TREASURY_LABEL = bytes("system:fees");
const STATE_COMMITMENT_PROTOCOL_VERSION = 3;
const MAX_OCCUPANCY_PAYERS = 256;
const MAX_PAYER_DID_BYTES = 255;
const MAX_U64 = 0xffff_ffff_ffff_ffffn;
const MAX_U128 = 0xffff_ffff_ffff_ffff_ffff_ffff_ffff_ffffn;
const MAX_TRACE_EVIDENCE_BYTES = 34 + 65_536 * 52;
const MAX_GRAPH_EVIDENCE_BYTES = bytes("LayerX/programs/call-graph/v1\0").length + 32 + 16 + 8 + 64 * 68;
const MAX_CALL_RESPONSE_BYTES = 1_048_576;
const CAPABILITY_TAGS = Object.freeze({ storage_read: 1, storage_write: 2, transfer: 3, emit_event: 4, compose: 5 } as const);

export interface DecodedSignedProgramCall {
  readonly activityId: string;
  readonly idempotencyKey: string;
  readonly notBefore: bigint;
  readonly notAfter: bigint;
  readonly canonicalBytes: Uint8Array;
}

export interface DecodedProgramTerminal {
  readonly outcome: ProgramOutcome;
  readonly usage: ProgramUsage;
  readonly transferVerification: "reconstructed" | "recorded_terminal_root_not_locally_reconstructable";
  readonly occupancyPaymentAccounts: readonly string[];
}

export interface OccupancyPayer {
  readonly did: Uint8Array | string;
  readonly account?: Uint8Array;
}

export async function bindSignedProgramLifecycle(canonical: Uint8Array, payload: Uint8Array | undefined, ordinal: 1 | 2 | 7, expectedIdempotencyKey?: string): Promise<DecodedSignedProgramCall> {
  canonical = new Uint8Array(canonical);
  payload = payload === undefined ? undefined : new Uint8Array(payload);
  if (canonical.length === 0 || canonical.length > 1_048_576 || ![1, 2, 7].includes(ordinal)) fail("activity bounds");
  const reader = new Reader(canonical);
  if (reader.u16() !== 3 || reader.u16() !== 0x1001 || reader.byte() !== 12) fail("signed activity header");
  field(reader, 1); if (reader.u16() !== 3) fail("signed activity protocol");
  field(reader, 2); reader.u32();
  field(reader, 3); if (reader.u32() !== 0x0009_0000 + ordinal) fail("signed activity type");
  field(reader, 4); reader.sizedU32(255);
  field(reader, 5); reader.sizedU32(524_288);
  field(reader, 6); reader.u64();
  field(reader, 7); const notBefore = reader.u64(), notAfter = reader.u64();
  field(reader, 8); const idempotencyKey = hex(reader.sizedU32(32, 32));
  field(reader, 9); reader.u128();
  field(reader, 10); const digest = reader.sizedU32(32, 32);
  field(reader, 11); const retainedPayload = reader.sizedU32(524_288);
  field(reader, 12); reader.sizedU32(128); reader.end();
  if (payload === undefined) {
    const codecs = await import("./program-lifecycle.js");
    payload = ordinal === 1 ? codecs.encodeNativeProgramDeploy(codecs.decodeNativeProgramDeploy(retainedPayload))
      : ordinal === 2 ? codecs.encodeNativeProgramUpgrade(codecs.decodeNativeProgramUpgrade(retainedPayload))
      : codecs.encodeNativeProgramWindDown(codecs.decodeNativeProgramWindDown(retainedPayload));
  }
  if (notAfter < notBefore || !equal(payload, retainedPayload) || !equal(digest, await sha256(PAYLOAD_DOMAIN, payload))
    || expectedIdempotencyKey !== undefined && expectedIdempotencyKey !== idempotencyKey) fail("lifecycle binding");
  return Object.freeze({ activityId: hex(await sha256(ACTIVITY_DOMAIN, canonical)), idempotencyKey, notBefore, notAfter, canonicalBytes: canonical.slice() });
}

export async function decodeSignedProgramCall(
  call: ProgramCall,
  expectedIdempotencyKey?: string,
): Promise<DecodedSignedProgramCall> {
  const canonical = new Uint8Array(call.signedActivity);
  const reader = new Reader(canonical);
  const envelopeVersion = reader.u16();
  if ((envelopeVersion !== 1 && envelopeVersion !== 2 && envelopeVersion !== 3) || reader.u16() !== 0x1001 || reader.byte() !== 12) fail("signed activity header");
  field(reader, 1); const protocolVersion = reader.u16();
  if (protocolVersion !== envelopeVersion) fail("signed activity protocol");
  field(reader, 2); reader.u32();
  field(reader, 3); if (reader.u32() !== 0x0009_0003) fail("signed activity type");
  field(reader, 4); const actor = reader.sizedU32(255);
  field(reader, 5); const authority = reader.sizedU32(524_288);
  field(reader, 6); reader.u64();
  field(reader, 7); const notBefore = reader.u64(); const notAfter = reader.u64();
  field(reader, 8); const idempotency = reader.sizedU32(32, 32);
  field(reader, 9); const envelopeFeeLimit = reader.u128();
  field(reader, 10); const payloadHash = reader.sizedU32(32, 32);
  field(reader, 11); const payload = reader.sizedU32(524_288);
  field(reader, 12); const signature = reader.sizedU32(128);
  reader.end();
  if (notAfter < notBefore) fail("signed activity bounds");
  if (!equal(payloadHash, await sha256(PAYLOAD_DOMAIN, payload))) fail("signed activity payload hash");
  if (call.nativeCall !== undefined) {
    if (protocolVersion !== 3 || envelopeFeeLimit !== call.budget.feeLimit
      || hex(call.nativeCall.programId) !== call.programId || !equal(call.nativeCall.calldata, call.calldata)
      || call.nativeCall.resources[0] !== call.budget.fuel || call.capabilities.length !== 0
      || !equal(payload, encodeNativeProgramCall(call.nativeCall))) fail("native signed activity binding");
  } else decodeCallPayload(payload, call);
  const idempotencyKey = hex(idempotency);
  if (expectedIdempotencyKey !== undefined && idempotencyKey !== expectedIdempotencyKey) fail("signed activity idempotency");
  return Object.freeze({
    activityId: hex(await sha256(ACTIVITY_DOMAIN, canonical)),
    idempotencyKey,
    notBefore,
    notAfter,
    canonicalBytes: canonical,
  });
}

export function assertFreshSimulationObservation(
  observedAt: bigint,
  binding: Pick<DecodedSignedProgramCall, "notBefore" | "notAfter">,
  now: bigint,
  maximumAgeMilliseconds: bigint,
): void {
  if (maximumAgeMilliseconds <= 0n || observedAt < binding.notBefore || observedAt > binding.notAfter
    || observedAt > now || now - observedAt > maximumAgeMilliseconds) {
    fail("simulation observation bounds");
  }
}

export async function decodeAndVerifyProgramTerminal(
  terminalPayload: Uint8Array,
  callGraph: Uint8Array,
  expectedProgramId: string,
  receipt: ProgramReceiptOutcome,
  protocolVersion: number,
  binding?: Readonly<{ protocol: ProtocolReceipt; signedActivity: Uint8Array }>,
  occupancyPayers: readonly OccupancyPayer[] = [],
): Promise<DecodedProgramTerminal> {
  if (callGraph.length === 0 || !equal(await sha256(callGraph), receipt.callGraphRoot)) fail("program call graph root");
  if (terminalPayload.length === 0 || terminalPayload.length > 1_048_576 || !equal(await sha256(terminalPayload), receipt.terminalPayloadRoot)) fail("program terminal root");
  if (starts(terminalPayload, PRE_RUNTIME_FAILURE)) {
    if (binding === undefined) fail("native refusal requires signed activity binding");
    await verifyPreRuntimeFailure(terminalPayload, callGraph, expectedProgramId, receipt, protocolVersion, binding);
    return Object.freeze({
      outcome: Object.freeze({ kind: "refused", failure: Object.freeze({ kind: "guest_refused", code: receipt.resultCode }) }),
      usage: receiptUsage(receipt), transferVerification: "reconstructed", occupancyPaymentAccounts: Object.freeze([]),
    });
  }
  let inner = terminalPayload;
  if (receipt.encodingVersion === 4) {
    const domain = bytes("LXP/programs/terminal-applied-legs/v1\0");
    const wrapper = new Reader(inner);
    if (!equal(wrapper.fixed(domain.length), domain)) fail("applied terminal domain");
    inner = wrapper.sizedU32(1_048_576);
    if (inner.length === 0) fail("empty applied terminal detail");
    const legs = wrapper.sizedU32(256 * 115);
    wrapper.end();
    if (!equal(await sha256(legs), receipt.appliedLegsDigest)) fail("applied legs digest");
    await verifyAppliedLegs(legs, receipt.transferRoot);
  }
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
      if (receipt.resultCode !== 0) fail("candidate response code requires successful execution");
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
  const occupancyRequired = (protocolVersion === 2 || protocolVersion === 3) && successfulExecution;
  if ((occupancy !== undefined) !== occupancyRequired) fail("occupancy attachment presence");
  let occupancyPaymentAccounts: readonly string[] = Object.freeze([]);
  if (occupancy !== undefined) {
    if (occupancy.length === 0) {
      if (!equal(receipt.occupancyEvidenceDigest, zero) || !equal(receipt.occupancyTransferRoot, zero)
        || receipt.occupancyByteBatches !== 0n || receipt.occupancyFeeUnits !== 0n) fail("empty occupancy attachment");
    } else {
      if (!equal(await sha256(occupancy), receipt.occupancyEvidenceDigest)) fail("occupancy evidence digest");
      const settlement = await decodeOccupancySettlement(occupancy);
      if (settlement.byteBatches !== receipt.occupancyByteBatches || settlement.feeUnits !== receipt.occupancyFeeUnits) {
        fail("occupancy receipt binding");
      }
      occupancyPaymentAccounts = await verifyOccupancyTransferRoot(settlement, protocolVersion,
        receipt.occupancyAssetId, receipt.occupancyTransferRoot, occupancyPayers);
    }
  } else if (!equal(receipt.occupancyEvidenceDigest, zero) || !equal(receipt.occupancyTransferRoot, zero)
    || receipt.occupancyByteBatches !== 0n || receipt.occupancyFeeUnits !== 0n) {
    fail("unexpected occupancy commitment");
  }
  const transferPresent = !equal(receipt.transferRoot, zero);
  const recorded = receipt.encodingVersion !== 4 && authorization === undefined && transferPresent;
  const authorityRequired = candidate || receipt.encodingVersion === 4 && successfulExecution;
  if (!recorded && (authorityRequired ? (authorization !== undefined) !== transferPresent : authorization !== undefined)) fail("transfer authority presence");
  if (authorization !== undefined) {
    if (authorization.length === 0 || authorityRoot === undefined || !equal(authorityRoot, receipt.transferRoot)) fail("transfer authority root");
    await verifyAuthorizationRoot(authorization, authorityRoot, receipt.encodingVersion === 4);
  }
  if (protocolVersion !== 1 && protocolVersion !== 2 && protocolVersion !== 3) fail("program receipt protocol");
  const boundUsage = usage ?? receiptUsage(receipt);
  return Object.freeze({ outcome, usage: boundUsage, occupancyPaymentAccounts,
    transferVerification: recorded ? "recorded_terminal_root_not_locally_reconstructable" : "reconstructed" });
}

async function verifyPreRuntimeFailure(
  terminal: Uint8Array, graph: Uint8Array, expectedProgram: string, outcome: ProgramReceiptOutcome,
  version: number, binding: Readonly<{ protocol: ProtocolReceipt; signedActivity: Uint8Array }>,
): Promise<void> {
  const protocol = binding.protocol, zero = new Uint8Array(32);
  const reader = new Reader(terminal.subarray(PRE_RUNTIME_FAILURE.length));
  const activity = reader.fixed(32), payloadHash = reader.fixed(32), code = reader.i32();
  const moduleVersion = reader.u32(), parameterVersion = reader.u32();
  let encoding = 3, appliedDigest: Uint8Array = zero;
  if (terminal.length !== PRE_RUNTIME_FAILURE.length + 76) {
    encoding = reader.byte();
    if (encoding !== 4) fail("native refusal encoding");
    appliedDigest = reader.fixed(32);
  }
  reader.end();
  const emptyDigest = encoding === 4 ? await sha256(new Uint8Array()) : zero;
  if (code >= 0 || code !== protocol.resultCode || code !== outcome.resultCode
    || !equal(activity, protocol.activityId) || moduleVersion !== protocol.moduleVersion
    || parameterVersion !== protocol.parameterVersion || encoding !== outcome.encodingVersion
    || protocol.protocolVersion !== version || protocol.moduleId !== 9 || protocol.operation !== 3
    || protocol.programOutcome === undefined || !sameReceiptOutcome(outcome, protocol.programOutcome)
    || outcome.terminalKind !== 2 || outcome.runtimeVersion !== 1
    || !((version === 2 && encoding === 3) || (version === 3 && encoding === 4))
    || outcome.memoryBytes !== 0n || outcome.storageReadBytes !== 0n
    || outcome.outputValues !== 0 || outcome.outputBytes !== 0n) fail("native refusal receipt binding");
  if (!equal(appliedDigest, emptyDigest) || !equal(outcome.appliedLegsDigest, emptyDigest)
    || !equal(outcome.transferRoot, zero)) fail("native refusal transfer commitments");
  if (!equal(outcome.occupancyAssetId, zero) || !equal(outcome.occupancyEvidenceDigest, zero)
    || !equal(outcome.occupancyTransferRoot, zero) || outcome.occupancyByteBatches !== 0n
    || outcome.occupancyFeeUnits !== 0n) fail("native refusal occupancy commitments");
  if (!equal(graph, EMPTY_CALL_GRAPH)) fail("native refusal call graph");
  const retained = await bindRetainedProgramCall(binding.signedActivity, hex(activity), expectedProgram, version);
  if (!equal(retained.payloadHash, payloadHash) || retained.guestAbi !== outcome.abiVersion) fail("native refusal payload binding");
}

function sameReceiptOutcome(left: ProgramReceiptOutcome, right: ProgramReceiptOutcome): boolean {
  const keys = Object.keys(left) as Array<keyof ProgramReceiptOutcome>;
  return keys.length === Object.keys(right).length && keys.every((key) => {
    const value = left[key], other = right[key];
    if (value instanceof Uint8Array) return other instanceof Uint8Array && equal(value, other);
    if (Array.isArray(value)) return Array.isArray(other) && value.length === other.length && value.every((item, index) => item === other[index]);
    return value === other;
  });
}

export async function bindRetainedProgramCall(
  signedActivity: Uint8Array, expectedActivity: string, expectedProgram: string, version: number,
): Promise<Readonly<{ payloadHash: Uint8Array; guestAbi: number; idempotencyKey: string }>> {
  const canonical = new Uint8Array(signedActivity);
  if (canonical.length === 0 || canonical.length > 1_048_576
    || ![1, 2, 3].includes(version)
    || hex(await sha256(ACTIVITY_DOMAIN, canonical)) !== expectedActivity) fail("program signed activity mismatch");
  const call = new Reader(canonical);
  if (call.u16() !== version || call.u16() !== 0x1001 || call.byte() !== 12) fail("native refusal activity header");
  field(call, 1); if (call.u16() !== version) fail("native refusal activity protocol");
  field(call, 2); call.u32();
  field(call, 3); if (call.u32() !== 0x0009_0003) fail("native refusal activity type");
  field(call, 4); call.sizedU32(255);
  field(call, 5); call.sizedU32(524_288);
  field(call, 6); call.u64();
  field(call, 7); const notBefore = call.u64(), notAfter = call.u64();
  field(call, 8); const idempotencyKey = hex(call.sizedU32(32, 32));
  field(call, 9); call.u128();
  field(call, 10); const declaredHash = call.sizedU32(32, 32);
  field(call, 11); const payload = call.sizedU32(524_288);
  field(call, 12); call.sizedU32(128); call.end();
  if (notAfter < notBefore || !equal(await sha256(PAYLOAD_DOMAIN, payload), declaredHash)) fail("retained call payload hash");
  let program: string, guestAbi: number;
  if (version === 3 || version === 2 && !starts(payload, CALL_DOMAIN)) {
    const native = decodeNativeProgramCall(payload);
    if (!equal(encodeNativeProgramCall(native), payload)) fail("retained native call encoding");
    program = hex(native.programId); guestAbi = native.guestAbi;
  } else {
    const legacy = new Reader(payload);
    if (!equal(legacy.fixed(CALL_DOMAIN.length), CALL_DOMAIN)) fail("retained call domain");
    program = hex(legacy.fixed(32)); guestAbi = 1;
    if (legacy.u64() === 0n) fail("retained call budget");
    legacy.u128();
    const count = legacy.u16();
    if (count > 5) fail("retained call capabilities");
    let prior = 0;
    for (let index = 0; index < count; index++) {
      const tag = legacy.byte();
      if (tag <= prior || tag > 5) fail("retained call capability tag");
      prior = tag;
    }
    legacy.sizedU32(1_048_576); legacy.end();
  }
  if (program !== expectedProgram || program === "0".repeat(64)) fail("native refusal payload binding");
  return Object.freeze({ payloadHash: declaredHash, guestAbi, idempotencyKey });
}

async function verifyAppliedLegs(encoded: Uint8Array, expected: Uint8Array): Promise<void> {
  if (encoded.length % 115 !== 0 || encoded.length > 256 * 115) fail("applied legs bounds");
  const legs: Uint8Array[] = [];
  for (let offset = 0; offset < encoded.length; offset += 115) {
    const leg = encoded.subarray(offset, offset + 115);
    if (leg[0] !== 0 || leg[113] !== 0 || leg[114] !== 1 || allZero(leg.subarray(1, 33))
      || allZero(leg.subarray(33, 65)) || allZero(leg.subarray(65, 97)) || allZero(leg.subarray(97, 113))) fail("applied leg canonical fields");
    legs.push(leg);
  }
  if (!equal(await merkleRoot(legs), expected)) fail("applied transfer root");
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
  if (runtime !== receipt.runtimeVersion || abi !== receipt.abiVersion || (abi === 2 && fee !== receipt.feeScheduleVersion)
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

interface ProgramAuthorityBinding {
  readonly owner: Uint8Array; readonly frame: Uint8Array; readonly source: Uint8Array;
  readonly asset: Uint8Array; readonly to: Uint8Array; readonly amount: bigint;
}
interface ProgramFundingBinding { readonly owner: Uint8Array; readonly destination: Uint8Array; readonly asset: Uint8Array }
interface OccupancyChargeBinding { readonly payer: Uint8Array; readonly amountDue: bigint; readonly paid: boolean; readonly arrearsAfter: bigint }
interface OccupancySettlementBinding { readonly byteBatches: bigint; readonly feeUnits: bigint; readonly charges: readonly OccupancyChargeBinding[] }
interface OccupancyPayerDisposition { readonly payer: Uint8Array; readonly due: bigint; readonly paid: bigint; readonly arrears: bigint }
interface OccupancyPaymentAccountBinding { readonly payer: Uint8Array; readonly asset: Uint8Array; readonly account: Uint8Array }
interface StorageNamespaceBinding { readonly canonical: Uint8Array; readonly wire: Uint8Array; readonly program: Uint8Array; readonly principal?: Uint8Array }

async function verifyAuthorizationRoot(encoded: Uint8Array, expected: Uint8Array, requireV2: boolean): Promise<void> {
  let names: Reader | undefined;
  if (starts(encoded, ACCOUNT_BOUND_SET)) {
    names = new Reader(encoded.subarray(ACCOUNT_BOUND_SET.length));
    encoded = names.sizedU32(1_048_576);
    if (starts(encoded, ACCOUNT_BOUND_SET)) fail("nested account-bound transfer set");
  }
  if (requireV2 && !starts(encoded, TRANSFER_SET_V2)) fail("V2 transfer authority required");
  const reader = new Reader(encoded);
  const candidate = starts(encoded, TRANSFER_SET_V2);
  const domain = candidate ? TRANSFER_SET_V2 : TRANSFER_SET_V1;
  if (!starts(encoded, domain) || !equal(reader.fixed(domain.length), domain)) fail("transfer authorization domain");
  nonzero(reader.fixed(32), "transfer program");
  const principal = reader.fixed(32); nonzero(principal, "transfer principal");
  nonzero(reader.fixed(32), "transfer invocation authority");
  decodeFrame(reader);
  decodeEventEnvelope(reader.fixed(reader.u32()));
  const calls = reader.u64();
  if (calls > 64n) fail("transfer call count");
  for (let index = 0; index < Number(calls); index += 1) {
    nonzero(reader.fixed(32), "transfer caller");
    nonzero(reader.fixed(32), "transfer callee");
    nonzero(reader.fixed(32), "transfer call principal");
    decodeFrame(reader); decodeFrame(reader);
    await decodeCapabilitySet(reader.fixed(reader.u32()), candidate);
  }
  const legCount = reader.u64();
  if (legCount === 0n || legCount > 256n) fail("transfer leg count");
  const kernelLegs: Uint8Array[] = [];
  let total = 0n;
  for (let index = 0; index < Number(legCount); index += 1) {
    const frame = decodeFrame(reader);
    let source = principal;
    let authority: ProgramAuthorityBinding | undefined;
    let funding: ProgramFundingBinding | undefined;
    if (candidate) {
      const sourceTag = reader.byte();
      if (sourceTag === 1) {
        source = reader.fixed(32); nonzero(source, "transfer principal source");
        if (!equal(source, principal)) fail("transfer principal authority");
      } else if (sourceTag === 2) {
        const encodedAuthority = reader.sizedU32(1_048_576);
        authority = await decodeProgramAuthority(encodedAuthority);
        source = authority.source;
      } else if (sourceTag === 3) {
        source = reader.fixed(32); nonzero(source, "transfer funding principal");
        if (!equal(source, principal)) fail("transfer funding authority");
        funding = await decodeProgramFunding(reader.sizedU32(1_048_576));
      } else fail("transfer source tag");
    }
    const asset = reader.fixed(32); const to = reader.fixed(32); const amount = reader.u128(); const program = reader.fixed(32);
    nonzero(asset, "transfer asset"); nonzero(to, "transfer destination"); nonzero(program, "transfer leg program");
    if (amount === 0n) fail("transfer amount");
    if (authority !== undefined && (!equal(authority.owner, program) || !equal(authority.frame, frame)
      || !equal(authority.asset, asset) || !equal(authority.to, to) || authority.amount !== amount)) fail("program transfer authority");
    if (funding !== undefined && (!equal(funding.owner, program) || !equal(funding.destination, to) || !equal(funding.asset, asset))) fail("program funding authority");
    if (names !== undefined) {
      const name = names.fixed(names.u16());
      if (authority !== undefined) {
        if (name.length !== 0) fail("program source account name");
      } else {
        source = await principalPaymentAccount(principal, asset, name);
      }
    }
    total = checkedU128Add(total, amount, "transfer total");
    kernelLegs.push(concatenate(Uint8Array.of(0), source, to, asset, bigEndian(amount, 16), bigEndian(1n, 2)));
  }
  reader.end();
  names?.end();
  if (!equal(await merkleRoot(kernelLegs), expected)) fail("transfer authorization root");
}

async function principalPaymentAccount(principal: Uint8Array, asset: Uint8Array, name: Uint8Array): Promise<Uint8Array> {
  if (name.length > 512 || name.some((byte) => !(byte >= 97 && byte <= 122)
    && !(byte >= 48 && byte <= 57) && ![46, 95, 45, 58].includes(byte))) fail("principal account name");
  const text = new TextDecoder().decode(name);
  if (!text.startsWith("agent:") || text.includes("::")) fail("principal account name");
  const tail = text.slice(6);
  let did: string;
  if (tail.endsWith(":main")) did = tail.slice(0, -5);
  else {
    const suffix = `:asset:${hex(asset)}`;
    if (!tail.endsWith(suffix)) fail("principal account asset");
    did = tail.slice(0, -suffix.length);
  }
  if (did.length === 0 || did.startsWith(":") || did.endsWith(":")) fail("principal account DID");
  const didBytes = bytes(did);
  if (!equal(await sha256(bytes("LXP/v1/did-id\0"), bigEndian(BigInt(didBytes.length), 2), didBytes), principal)) fail("principal account owner");
  return sha256(ACCOUNT_DERIVATION, bigEndian(BigInt(name.length), 4), name);
}

async function decodeProgramAuthority(encoded: Uint8Array): Promise<ProgramAuthorityBinding> {
  const reader = new Reader(encoded);
  if (!equal(reader.fixed(PROGRAM_AUTHORITY.length), PROGRAM_AUTHORITY)) fail("program authority domain");
  const owner = reader.fixed(32); nonzero(owner, "program authority owner");
  const seedLength = reader.u16(); if (seedLength > 128) fail("program authority seed");
  const seed = reader.fixed(seedLength); const source = reader.fixed(32); const frame = decodeFrame(reader);
  const asset = reader.fixed(32); const to = reader.fixed(32); const amount = reader.u128(); reader.end();
  nonzero(asset, "program authority asset"); nonzero(to, "program authority destination");
  if (amount === 0n || !equal(await deriveProgramAccount(owner, seed), source)) fail("program authority derivation");
  return { owner, frame, source, asset, to, amount };
}

async function decodeProgramFunding(encoded: Uint8Array): Promise<ProgramFundingBinding> {
  const reader = new Reader(encoded);
  if (!equal(reader.fixed(PROGRAM_FUNDING.length), PROGRAM_FUNDING)) fail("program funding domain");
  const owner = reader.fixed(32); nonzero(owner, "program funding owner");
  const seedLength = reader.u16(); if (seedLength > 128) fail("program funding seed");
  const seed = reader.fixed(seedLength); const destination = reader.fixed(32); const asset = reader.fixed(32); reader.end();
  nonzero(destination, "program funding destination"); nonzero(asset, "program funding asset");
  if (!equal(await deriveProgramAccount(owner, seed), destination)) fail("program funding derivation");
  return { owner, destination, asset };
}

async function deriveProgramAccount(owner: Uint8Array, seed: Uint8Array): Promise<Uint8Array> {
  return sha256(PROGRAM_ACCOUNT, owner, bigEndian(BigInt(seed.length), 4), seed);
}

function decodeEventEnvelope(encoded: Uint8Array): void {
  const reader = new Reader(encoded);
  if (!equal(reader.fixed(EVENT_ENVELOPE.length), EVENT_ENVELOPE)) fail("program event domain");
  const count = reader.u32(); if (count > 64) fail("program event count");
  for (let index = 0; index < count; index += 1) {
    nonzero(reader.fixed(32), "event program"); nonzero(reader.fixed(32), "event principal"); decodeFrame(reader);
    reader.sizedU32(64); reader.sizedU32(65_536);
  }
  reader.end();
}

function decodeFrame(reader: Reader): Uint8Array {
  const path = reader.fixed(8); const depth = reader.byte();
  if (depth > 8 || path.subarray(0, depth).some((value) => value === 0) || path.subarray(depth).some((value) => value !== 0)) fail("call frame");
  return concatenate(path, Uint8Array.of(depth));
}

async function decodeCapabilitySet(encoded: Uint8Array, v2: boolean): Promise<void> {
  const grants = await decodeNativeCapabilitySet(encoded);
  if (!v2 && grants.some((grant) => grant.kind === "program_spend" || grant.kind === "balance_view")) fail("capability tag");
}

async function decodeOccupancySettlement(encoded: Uint8Array): Promise<OccupancySettlementBinding> {
  if (encoded.length > 65_536) fail("occupancy evidence length");
  if (starts(encoded, OCCUPANCY_V1) || starts(encoded, OCCUPANCY_V2)) return decodeLegacyOccupancy(encoded);
  const reader = new Reader(encoded);
  if (!equal(reader.fixed(OCCUPANCY_V3.length), OCCUPANCY_V3)) fail("occupancy evidence domain");
  const batch = reader.u64(); const occupancyPrice = decodeOccupancySchedule(reader, true);
  const declaredUnits = reader.u128(); const declaredFee = reader.u128(); const declaredPaid = reader.u128(); const declaredArrears = reader.u128();
  const count = reader.u32(); if (count > 256) fail("occupancy position count");
  let byteBatches = 0n; let feeUnits = 0n; let paidUnits = 0n; let arrearsUnits = 0n;
  let priorNamespace: Uint8Array | undefined; const charges: OccupancyChargeBinding[] = [];
  for (let index = 0; index < count; index += 1) {
    const namespace = decodeStorageNamespace(reader);
    if (priorNamespace !== undefined && compareBytes(priorNamespace, namespace.canonical) >= 0) fail("occupancy namespace order");
    priorNamespace = namespace.canonical;
    const payer = reader.fixed(32); nonzero(payer, "occupancy payer");
    if (namespace.principal !== undefined && !equal(namespace.principal, payer)) fail("occupancy payer scope");
    const rootProgram = reader.fixed(32); nonzero(rootProgram, "occupancy root program");
    const activity = reader.fixed(32); const fromBatch = reader.u64(); const toBatch = reader.u64();
    const recordedBytes = reader.u64(); const finalBytes = reader.u64(); const units = reader.u128(); const price = reader.u64();
    const accrued = reader.u128(); const priorArrears = reader.u128(); const amountDue = reader.u128(); const authorizedAdded = reader.u128();
    const disposition = reader.byte(); if (disposition < 1 || disposition > 5) fail("occupancy disposition");
    const arrearsAfter = reader.u128(); const maximumBytes = reader.u64(); const maximumPrice = reader.u64(); reader.u128(); const mandate = reader.fixed(32);
    if (toBatch < fromBatch) fail("occupancy batch interval");
    const expectedUnits = checkedU128Multiply(recordedBytes, toBatch - fromBatch, "occupancy units");
    const expectedFee = checkedU128Multiply(expectedUnits, price, "occupancy fee");
    const expectedDue = checkedU128Add(priorArrears, expectedFee, "occupancy due");
    const migration = disposition === 5;
    if (toBatch !== batch || (!migration && price !== occupancyPrice) || units !== expectedUnits || accrued !== expectedFee
      || amountDue !== expectedDue || finalBytes > maximumBytes || (!migration && (allZero(mandate) || allZero(activity)))
      || (migration && (price !== 0n || accrued !== 0n || priorArrears !== 0n || amountDue !== 0n || arrearsAfter !== 0n
        || !allZero(mandate) || !allZero(activity) || !equal(rootProgram, namespace.program)))
      || (disposition === 4) !== (price > maximumPrice) || (disposition === 1 && arrearsAfter !== 0n)
      || (disposition !== 1 && arrearsAfter !== amountDue)) fail("occupancy charge semantics");
    if (authorizedAdded !== 0n) {
      const expectedMandate = await sha256(OCCUPANCY_MANDATE, payer, rootProgram, activity, namespace.wire,
        bigEndian(maximumBytes, 8), bigEndian(maximumPrice, 8), bigEndian(authorizedAdded, 16));
      if (!equal(mandate, expectedMandate)) fail("occupancy mandate");
    }
    byteBatches = checkedU128Add(byteBatches, units, "occupancy usage"); feeUnits = checkedU128Add(feeUnits, accrued, "occupancy fees");
    if (disposition === 1) paidUnits = checkedU128Add(paidUnits, amountDue, "occupancy paid");
    else arrearsUnits = checkedU128Add(arrearsUnits, arrearsAfter, "occupancy arrears");
    charges.push({ payer, amountDue, paid: disposition === 1, arrearsAfter });
  }
  reader.end();
  if (byteBatches !== declaredUnits || feeUnits !== declaredFee || paidUnits !== declaredPaid || arrearsUnits !== declaredArrears) fail("occupancy declared usage");
  return { byteBatches, feeUnits, charges };
}

function decodeLegacyOccupancy(encoded: Uint8Array): OccupancySettlementBinding {
  const versioned = starts(encoded, OCCUPANCY_V2); const domain = versioned ? OCCUPANCY_V2 : OCCUPANCY_V1;
  const reader = new Reader(encoded); if (!equal(reader.fixed(domain.length), domain)) fail("legacy occupancy domain");
  const batch = reader.u64(); const occupancyPrice = decodeOccupancySchedule(reader, versioned);
  const declaredUnits = reader.u128(); const declaredFee = reader.u128(); const count = reader.u64();
  if (count > 256n) fail("legacy occupancy count");
  let byteBatches = 0n; let feeUnits = 0n; const charges: OccupancyChargeBinding[] = [];
  for (let index = 0; index < Number(count); index += 1) {
    const namespace = decodeStorageNamespace(reader); const payer = reader.fixed(32); nonzero(payer, "legacy occupancy payer");
    const fromBatch = reader.u64(); const toBatch = reader.u64(); const recordedBytes = reader.u64(); reader.u64();
    const units = reader.u128(); const price = reader.u64(); const accrued = reader.u128();
    if (toBatch < fromBatch) fail("legacy occupancy batch interval");
    const expectedUnits = checkedU128Multiply(recordedBytes, toBatch - fromBatch, "legacy occupancy units");
    if (toBatch !== batch || units !== expectedUnits || price !== occupancyPrice
      || accrued !== checkedU128Multiply(units, price, "legacy occupancy fee")) fail("legacy occupancy charge");
    byteBatches = checkedU128Add(byteBatches, units, "legacy occupancy usage"); feeUnits = checkedU128Add(feeUnits, accrued, "legacy occupancy fees");
    charges.push({ payer, amountDue: accrued, paid: true, arrearsAfter: 0n });
    void namespace;
  }
  reader.end(); if (byteBatches !== declaredUnits || feeUnits !== declaredFee) fail("legacy occupancy declared usage");
  return { byteBatches, feeUnits, charges };
}

function decodeOccupancySchedule(reader: Reader, versioned: boolean): bigint {
  const version = versioned ? reader.u32() : 1; if (version === 0) fail("occupancy schedule version");
  let occupancyPrice = 0n; for (let index = 0; index < 7; index += 1) occupancyPrice = reader.u64(); return occupancyPrice;
}

function decodeStorageNamespace(reader: Reader): StorageNamespaceBinding {
  const length = reader.byte(); if (length !== 33 && length !== 65) fail("storage namespace length");
  const canonical = reader.fixed(length); const program = canonical.subarray(0, 32); nonzero(program, "storage namespace program");
  const tag = canonical[32]; let principal: Uint8Array | undefined;
  if (tag === 0 && length === 65) { principal = canonical.subarray(33); nonzero(principal, "storage namespace principal"); }
  else if (!(tag === 1 && length === 33) && !(tag === 2 && length === 65)) fail("storage namespace tag");
  return { canonical, wire: concatenate(Uint8Array.of(length), canonical), program, ...(principal === undefined ? {} : { principal }) };
}

function occupancyPayerDispositions(settlement: OccupancySettlementBinding): OccupancyPayerDisposition[] {
  const payers = new Map<string, { payer: Uint8Array; due: bigint; paid: bigint; arrears: bigint }>();
  for (const charge of settlement.charges) {
    const key = hex(charge.payer); const existing = payers.get(key) ?? { payer: charge.payer, due: 0n, paid: 0n, arrears: 0n };
    existing.due = checkedU128Add(existing.due, charge.amountDue, "occupancy payer due");
    if (charge.paid) existing.paid = checkedU128Add(existing.paid, charge.amountDue, "occupancy payer paid");
    existing.arrears = checkedU128Add(existing.arrears, charge.arrearsAfter, "occupancy payer arrears"); payers.set(key, existing);
  }
  return [...payers.values()].sort((left, right) => compareBytes(left.payer, right.payer));
}

async function occupancyTransferRoot(settlement: OccupancySettlementBinding, asset: Uint8Array): Promise<Uint8Array> {
  if (asset.length !== 32) fail("occupancy asset length");
  nonzero(asset, "occupancy asset");
  const treasury = await sha256(ACCOUNT_DERIVATION, bigEndian(11n, 4), FEE_TREASURY_LABEL); const legs: Uint8Array[] = [];
  for (const entry of occupancyPayerDispositions(settlement).filter((value) => value.due !== 0n || value.arrears !== 0n)) {
    if (entry.paid !== 0n) legs.push(concatenate(Uint8Array.of(0), entry.payer, treasury, asset, bigEndian(entry.paid, 16), bigEndian(23n, 2)));
  }
  return merkleRoot(legs);
}

async function verifyOccupancyTransferRoot(
  settlement: OccupancySettlementBinding, protocolVersion: number, asset: Uint8Array,
  committedRoot: Uint8Array, payers: readonly OccupancyPayer[],
): Promise<readonly string[]> {
  if (protocolVersion !== STATE_COMMITMENT_PROTOCOL_VERSION) {
    if (!equal(await occupancyTransferRoot(settlement, asset), committedRoot)) fail("occupancy receipt binding");
    return Object.freeze([]);
  }
  if (asset.length !== 32) fail("occupancy asset length");
  nonzero(asset, "occupancy asset");
  const dispositions = occupancyPayerDispositions(settlement);
  const accounts = await provenPayingAccounts(dispositions, payers, asset);
  if (accounts.length > 2 * MAX_OCCUPANCY_PAYERS) fail("occupancy payment account bound");
  const paying: { paid: bigint; proven: OccupancyPaymentAccountBinding[] }[] = [];
  let selections = 1;
  for (const entry of dispositions) {
    if (entry.paid === 0n) continue;
    const proven: OccupancyPaymentAccountBinding[] = [];
    for (const account of accounts) {
      if (equal(account.payer, entry.payer) && equal(account.asset, asset)
        && !proven.some((known) => equal(known.account, account.account))) proven.push(account);
    }
    if (proven.length === 0) fail("occupancy payment account");
    selections *= proven.length;
    if (!Number.isSafeInteger(selections) || selections > MAX_OCCUPANCY_PAYERS) fail("occupancy payment account bound");
    paying.push({ paid: entry.paid, proven });
  }
  if (paying.length > MAX_OCCUPANCY_PAYERS) fail("occupancy payment account bound");
  const treasury = await sha256(ACCOUNT_DERIVATION, bigEndian(11n, 4), FEE_TREASURY_LABEL);
  for (let selection = 0; selection < selections; selection += 1) {
    let remaining = selection;
    const chosen: string[] = [];
    const legs: Uint8Array[] = [];
    for (const entry of paying) {
      const account = entry.proven[remaining % entry.proven.length] ?? fail("occupancy payment account");
      remaining = Math.floor(remaining / entry.proven.length);
      legs.push(concatenate(Uint8Array.of(0), account.account, treasury, asset, bigEndian(entry.paid, 16), bigEndian(23n, 2)));
      chosen.push(hex(account.account));
    }
    if (equal(await merkleRoot(legs), committedRoot)) return Object.freeze(chosen);
  }
  return fail("occupancy transfer root");
}

async function provenPayingAccounts(
  dispositions: readonly OccupancyPayerDisposition[], payers: readonly OccupancyPayer[], asset: Uint8Array,
): Promise<OccupancyPaymentAccountBinding[]> {
  const paying = dispositions.filter((entry) => entry.paid !== 0n).map((entry) => entry.payer);
  const accounts: OccupancyPaymentAccountBinding[] = [];
  if (paying.length === 0 || paying.length > MAX_OCCUPANCY_PAYERS) return accounts;
  for (const payer of payers) {
    const did = typeof payer.did === "string" ? bytes(payer.did) : payer.did;
    let main: OccupancyPaymentAccountBinding;
    try { main = await occupancyPaymentAccount(did, asset, false); } catch { continue; }
    if (!paying.some((value) => equal(value, main.payer))) continue;
    if (payer.account === undefined) {
      accounts.push(main, await occupancyPaymentAccount(did, asset, true));
      continue;
    }
    const offered = payer.account;
    const scoped = await occupancyPaymentAccount(did, asset, true);
    const proven = [main, scoped].find((candidate) => equal(candidate.account, offered));
    if (proven === undefined) fail("occupancy payment account");
    accounts.push(proven);
  }
  return accounts;
}

async function occupancyPaymentAccount(did: Uint8Array, asset: Uint8Array, assetScoped: boolean): Promise<OccupancyPaymentAccountBinding> {
  if (did.length === 0 || did.length > MAX_PAYER_DID_BYTES || asset.length !== 32 || allZero(asset)) fail("occupancy payer did");
  const payer = await sha256(DID_DERIVATION, bigEndian(BigInt(did.length), 2), did);
  nonzero(payer, "occupancy payer did");
  const name = assetScoped
    ? concatenate(bytes("agent:"), did, bytes(":asset:"), bytes(hex(asset)))
    : concatenate(bytes("agent:"), did, bytes(":main"));
  return { payer, asset, account: await sha256(ACCOUNT_DERIVATION, bigEndian(BigInt(name.length), 4), name) };
}

async function merkleRoot(legs: readonly Uint8Array[]): Promise<Uint8Array> {
  if (legs.length === 0) return new Uint8Array(32);
  let level = await Promise.all(legs.map((leg) => sha256(MERKLE_LEAF, leg)));
  while (level.length > 1) {
    const next: Uint8Array[] = [];
    for (let index = 0; index < level.length; index += 2) next.push(await sha256(MERKLE_INTERNAL, level[index] ?? fail("merkle level"), level[index + 1] ?? level[index] ?? fail("merkle level")));
    level = next;
  }
  return level[0] ?? fail("merkle root");
}

function checkedU128Add(left: bigint, right: bigint, boundary: string): bigint { const value = left + right; if (value > MAX_U128) fail(boundary); return value; }
function checkedU128Multiply(left: bigint, right: bigint, boundary: string): bigint { const value = left * right; if (value > MAX_U128) fail(boundary); return value; }
function bigEndian(value: bigint, length: number): Uint8Array { const result = new Uint8Array(length); let remaining = value; for (let index = length - 1; index >= 0; index -= 1) { result[index] = Number(remaining & 0xffn); remaining >>= 8n; } if (remaining !== 0n) fail("canonical integer encoding"); return result; }
function nonzero(value: Uint8Array, boundary: string): void { if (allZero(value)) fail(boundary); }
function allZero(value: Uint8Array): boolean { return value.every((byte) => byte === 0); }
function compareBytes(left: Uint8Array, right: Uint8Array): number { const length = Math.min(left.length, right.length); for (let index = 0; index < length; index += 1) { const order = (left[index] ?? 0) - (right[index] ?? 0); if (order !== 0) return order; } return left.length - right.length; }

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
