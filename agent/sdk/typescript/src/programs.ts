import type { AuthorizedReceiptBatch, ReceiptVerification } from "./verifier.js";
import { verifyReceiptOutcome } from "./verifier.js";
import type { IdempotencyKey, ProductionClient } from "./production.js";

const HEX32 = /^[0-9a-f]{64}$/u;
const DECIMAL_U128 = /^(0|[1-9][0-9]{0,38})$/u;
const MAX_U128 = 340282366920938463463374607431768211455n;
const MAX_CALLDATA = 1_048_576;
const MAX_CAPABILITIES = 5;
const CAPABILITY_ORDER = Object.freeze({
  storage_read: 1,
  storage_write: 2,
  transfer: 3,
  emit_event: 4,
  compose: 5,
} as const);

export interface ProgramBudget { readonly fuel: bigint; readonly feeLimit: bigint }
export type ProgramCapability = keyof typeof CAPABILITY_ORDER;
export interface ProgramCall {
  readonly programId: string;
  readonly calldata: Uint8Array;
  readonly budget: ProgramBudget;
  readonly capabilities: readonly ProgramCapability[];
  readonly signedActivity: Uint8Array;
}

export interface ProgramDiscovery { readonly program_id: string; readonly lifecycle: "active" | "deprecated" | "tombstoned"; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly valid_through: string; readonly verification: "registry-receipt-and-current-head-verified" }
export interface ProgramInterface { readonly program_id: string; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly interface: string; readonly interface_digest: string; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly valid_through: string; readonly source: Readonly<Record<string, unknown>>; readonly verification: "deployment-interface-and-current-head-verified" }
export type ProgramFailure = Readonly<{ kind: "unknown_program" | "reentrancy" | "depth_exceeded" | "fanout_exceeded" | "guest_refused" | "authority" | "resource" | "response" | "fault"; code?: number; limit?: number; attempted?: number }>;
export type ProgramOutcome = Readonly<{ kind: "completed"; code: number; response: string }> | Readonly<{ kind: "legacy_completed"; code: number; values: readonly unknown[] }> | Readonly<{ kind: "refused"; failure: ProgramFailure }>;
export interface ProgramUsage { readonly cpu_fuel: string; readonly memory_bytes: string; readonly storage_read_bytes: string; readonly storage_write_bytes: string; readonly output_values: number; readonly output_bytes: string; readonly fee_units: string }
export interface ProgramAuthorityDocument { readonly batch_id: string; readonly asset: string; readonly previous_state_root: string; readonly resulting_state_root: string; readonly sequencer_public_key: string }
export interface ProgramExecutionDocument { readonly state: "executed" | "refused" | "simulated"; readonly activity_id: string; readonly program_id: string; readonly guest_abi_version: number; readonly module_version: number; readonly global_sequence: string; readonly result_code: number; readonly state_root: string; readonly receipt: string; readonly terminal_payload: string; readonly call_graph: string; readonly authority: ProgramAuthorityDocument; readonly usage: ProgramUsage; readonly outcome: ProgramOutcome; readonly idempotency_key?: string }
export interface ProgramUnknownSubmission { readonly state: "unknown"; readonly activity_id: string; readonly idempotency_key: string; readonly retained_signed_activity: string }
export type ProgramSubmission = ProgramUnknownSubmission | (ProgramExecutionDocument & Readonly<{ state: "executed" | "refused" }>);
export interface ProgramSimulationEvidence { readonly boundary_id: string; readonly activity_id: string; readonly previous_state_root: string; readonly hypothetical_state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly committed: false; readonly public_key: string; readonly signature: string }
export interface ProgramSimulation { readonly committed: false; readonly execution: ProgramExecutionDocument & Readonly<{ state: "simulated" }>; readonly simulation_evidence: ProgramSimulationEvidence }
export interface VerifiedProgramReceipt { readonly verification: ReceiptVerification; readonly terminalPayload: Uint8Array; readonly callGraph: Uint8Array }

function validateCall(call: ProgramCall): void {
  if (!HEX32.test(call.programId) || call.calldata.length > MAX_CALLDATA || call.signedActivity.length === 0 || call.signedActivity.length > MAX_CALLDATA
    || call.budget.fuel <= 0n || call.budget.fuel > 18446744073709551615n
    || call.budget.feeLimit < 0n || call.budget.feeLimit > MAX_U128
    || call.capabilities.length > MAX_CAPABILITIES) throw new TypeError("invalid bounded program call");
  let prior = 0;
  for (const capability of call.capabilities) {
    const current = CAPABILITY_ORDER[capability];
    if (current === undefined || current <= prior) throw new TypeError("program capabilities must be canonical");
    prior = current;
  }
}

export async function verifyProgramReceipt(
  execution: ProgramExecutionDocument,
  authority: AuthorizedReceiptBatch,
): Promise<VerifiedProgramReceipt> {
  if (!HEX32.test(execution.activity_id) || execution.module_version < 1 || execution.module_version > 3
    || ![1, 2].includes(execution.guest_abi_version)) throw new TypeError("invalid program execution evidence");
  const receipt = decodeHex(execution.receipt, 1_048_576);
  const terminalPayload = decodeHex(execution.terminal_payload, 1_048_576);
  const callGraph = decodeHex(execution.call_graph, 1_048_576);
  const verification = await verifyReceiptOutcome(receipt, authority);
  const protocol = verification.receipt;
  const outcome = protocol.programOutcome;
  if (protocol.moduleId !== 9 || protocol.operation !== 3 || protocol.moduleVersion < 1
    || protocol.moduleVersion > 3 || protocol.moduleVersion !== execution.module_version
    || hex(protocol.activityId) !== execution.activity_id || outcome === undefined
    || outcome.abiVersion !== execution.guest_abi_version
    || outcome.resultCode !== execution.result_code
    || callGraph.length === 0
    || !equal(await digest(terminalPayload), outcome.terminalPayloadRoot)
    || !equal(await digest(callGraph), outcome.callGraphRoot)) {
    throw new TypeError("program receipt binding failed");
  }
  return Object.freeze({ verification, terminalPayload, callGraph });
}

export class ProgramOperations {
  public constructor(private readonly client: ProductionClient) {}
  public async discover(programId: string): Promise<ProgramDiscovery> { if (!HEX32.test(programId)) throw new TypeError("invalid program id"); const value = await this.client.agent<unknown, unknown>("program.discover", { program_id: programId, requested_verification_level: "sequencer-signed" }); return discovery(value, programId); }
  public async interface(programId: string): Promise<ProgramInterface> { if (!HEX32.test(programId)) throw new TypeError("invalid program id"); const value = await this.client.agent<unknown, unknown>("program.interface", { program_id: programId, requested_verification_level: "sequencer-signed" }); return programInterface(value, programId); }
  public async simulate(call: ProgramCall): Promise<ProgramSimulation> { validateCall(call); const value = await this.client.agent<unknown, unknown>("program.simulate", wireCall(call)); const simulation = simulationDocument(value, call.programId); await verifyProgramReceipt(simulation.execution, wireAuthority(simulation.execution.authority)); await verifySimulationEvidence(simulation); return simulation; }
  public async submit(call: ProgramCall, idempotencyKey: IdempotencyKey): Promise<ProgramSubmission> { validateCall(call); if (!HEX32.test(idempotencyKey)) throw new TypeError("invalid program idempotency key"); const value = await this.client.agent<unknown, unknown>("program.call", wireCall(call), { idempotencyKey }); return await submissionDocument(value, { programId: call.programId, idempotencyKey, retainedSignedActivity: hex(call.signedActivity) }); }
  public async receipt(idempotencyKey: string, expectedActivityId: string): Promise<ProgramSubmission> { if (!HEX32.test(idempotencyKey) || !HEX32.test(expectedActivityId)) throw new TypeError("invalid program receipt selector"); const value = await this.client.agent<unknown, unknown>("program.receipt", { idempotency_key: idempotencyKey, expected_activity_id: expectedActivityId, requested_verification_level: "sequencer-signed" }); return await submissionDocument(value, { idempotencyKey, activityId: expectedActivityId }); }
  public async activity(activityId: string): Promise<ProgramSubmission> { if (!HEX32.test(activityId)) throw new TypeError("invalid activity id"); const value = await this.client.agent<unknown, unknown>("program.activity", { activity_id: activityId, requested_verification_level: "sequencer-signed" }); return await submissionDocument(value, { activityId }); }
}

interface SubmissionExpectation { readonly programId?: string; readonly activityId?: string; readonly idempotencyKey?: string; readonly retainedSignedActivity?: string }

async function submissionDocument(value: unknown, expected: SubmissionExpectation): Promise<ProgramSubmission> {
  const candidate = object(value);
  if (candidate.state === "unknown") {
    const activityId = requiredHex32(candidate, "activity_id");
    const idempotencyKey = requiredHex32(candidate, "idempotency_key");
    const retained = requiredHex(candidate, "retained_signed_activity", MAX_CALLDATA);
    if ((expected.activityId !== undefined && activityId !== expected.activityId)
      || (expected.idempotencyKey !== undefined && idempotencyKey !== expected.idempotencyKey)
      || (expected.retainedSignedActivity !== undefined && retained !== expected.retainedSignedActivity)) throw new TypeError("program unknown binding failed");
    return Object.freeze({ state: "unknown", activity_id: activityId, idempotency_key: idempotencyKey, retained_signed_activity: retained });
  }
  if (candidate.state !== "executed" && candidate.state !== "refused") throw new TypeError("invalid program submission state");
  const execution = executionDocument(candidate, candidate.state);
  if ((expected.programId !== undefined && execution.program_id !== expected.programId)
    || (expected.activityId !== undefined && execution.activity_id !== expected.activityId)
    || (expected.idempotencyKey !== undefined && execution.idempotency_key !== expected.idempotencyKey)) throw new TypeError("program execution binding failed");
  await verifyProgramReceipt(execution, wireAuthority(execution.authority));
  return execution as ProgramSubmission;
}

function simulationDocument(value: unknown, expectedProgramId: string): ProgramSimulation {
  const candidate = object(value);
  if (candidate.committed !== false) throw new TypeError("committed program simulation");
  const execution = executionDocument(object(candidate.execution), "simulated") as ProgramSimulation["execution"];
  if (execution.program_id !== expectedProgramId) throw new TypeError("program simulation binding failed");
  const rawEvidence = object(candidate.simulation_evidence);
  const evidence: ProgramSimulationEvidence = Object.freeze({
    boundary_id: requiredHex32(rawEvidence, "boundary_id"),
    activity_id: requiredHex32(rawEvidence, "activity_id"),
    previous_state_root: requiredHex32(rawEvidence, "previous_state_root"),
    hypothetical_state_root: requiredHex32(rawEvidence, "hypothetical_state_root"),
    observed_sequence: decimal(rawEvidence.observed_sequence),
    observed_at: decimal(rawEvidence.observed_at),
    committed: rawEvidence.committed === false ? false : (() => { throw new TypeError("invalid simulation evidence"); })(),
    public_key: requiredHex32(rawEvidence, "public_key"),
    signature: requiredHex(rawEvidence, "signature", 64, 64),
  });
  if (evidence.activity_id !== execution.activity_id || evidence.hypothetical_state_root !== execution.state_root) throw new TypeError("program simulation evidence binding failed");
  return Object.freeze({ committed: false, execution, simulation_evidence: evidence });
}

function executionDocument(candidate: Readonly<Record<string, unknown>>, state: "executed" | "refused" | "simulated"): ProgramExecutionDocument {
  if (candidate.state !== state) throw new TypeError("invalid program execution state");
  const usage = object(candidate.usage);
  const authority = object(candidate.authority);
  const result: ProgramExecutionDocument = Object.freeze({
    state,
    activity_id: requiredHex32(candidate, "activity_id"),
    program_id: requiredHex32(candidate, "program_id"),
    guest_abi_version: exactInteger(candidate.guest_abi_version, 1, 2),
    module_version: exactInteger(candidate.module_version, 1, 3),
    global_sequence: decimal(candidate.global_sequence),
    result_code: exactInteger(candidate.result_code, -2147483648, 2147483647),
    state_root: requiredHex32(candidate, "state_root"),
    receipt: requiredHex(candidate, "receipt", MAX_CALLDATA),
    terminal_payload: requiredHex(candidate, "terminal_payload", MAX_CALLDATA),
    call_graph: requiredHex(candidate, "call_graph", MAX_CALLDATA),
    authority: Object.freeze({
      batch_id: requiredHex32(authority, "batch_id"),
      asset: requiredHex32(authority, "asset"),
      previous_state_root: requiredHex32(authority, "previous_state_root"),
      resulting_state_root: requiredHex32(authority, "resulting_state_root"),
      sequencer_public_key: requiredHex32(authority, "sequencer_public_key"),
    }),
    usage: Object.freeze({
      cpu_fuel: decimal(usage.cpu_fuel), memory_bytes: decimal(usage.memory_bytes),
      storage_read_bytes: decimal(usage.storage_read_bytes), storage_write_bytes: decimal(usage.storage_write_bytes),
      output_values: exactInteger(usage.output_values, 0, 0xffff_ffff), output_bytes: decimal(usage.output_bytes), fee_units: decimal(usage.fee_units, true),
    }),
    outcome: candidate.outcome as ProgramOutcome,
    ...(candidate.idempotency_key === undefined ? {} : { idempotency_key: requiredHex32(candidate, "idempotency_key") }),
  });
  if (state === "refused" ? result.outcome.kind !== "refused" : result.outcome.kind === "refused") throw new TypeError("program state/outcome mismatch");
  return result;
}

function discovery(value: unknown, programId: string): ProgramDiscovery {
  const candidate = object(value);
  if (requiredHex32(candidate, "program_id") !== programId || candidate.verification !== "registry-receipt-and-current-head-verified") throw new TypeError("unverified program discovery");
  if (candidate.lifecycle !== "active" && candidate.lifecycle !== "deprecated" && candidate.lifecycle !== "tombstoned") throw new TypeError("invalid program lifecycle");
  return Object.freeze({
    program_id: programId, lifecycle: candidate.lifecycle, version: exactInteger(candidate.version, 1, 0xffff_ffff),
    code_hash: requiredHex32(candidate, "code_hash"), abi_version: exactInteger(candidate.abi_version, 1, 2),
    receipt_digest: requiredHex32(candidate, "receipt_digest"), state_root: requiredHex32(candidate, "state_root"),
    observed_sequence: decimal(candidate.observed_sequence), observed_at: decimal(candidate.observed_at), valid_through: decimal(candidate.valid_through),
    verification: "registry-receipt-and-current-head-verified",
  });
}

function programInterface(value: unknown, programId: string): ProgramInterface {
  const candidate = object(value);
  if (requiredHex32(candidate, "program_id") !== programId || candidate.verification !== "deployment-interface-and-current-head-verified") throw new TypeError("unverified program interface");
  if (typeof candidate.interface !== "string") throw new TypeError("invalid program interface");
  return Object.freeze({
    program_id: programId, version: exactInteger(candidate.version, 1, 0xffff_ffff), code_hash: requiredHex32(candidate, "code_hash"),
    abi_version: exactInteger(candidate.abi_version, 1, 2), interface: candidate.interface, interface_digest: requiredHex32(candidate, "interface_digest"),
    receipt_digest: requiredHex32(candidate, "receipt_digest"), state_root: requiredHex32(candidate, "state_root"),
    observed_sequence: decimal(candidate.observed_sequence), observed_at: decimal(candidate.observed_at), valid_through: decimal(candidate.valid_through),
    source: object(candidate.source), verification: "deployment-interface-and-current-head-verified",
  });
}

function wireAuthority(value: ProgramAuthorityDocument): AuthorizedReceiptBatch {
  return Object.freeze({ batchId: decodeHex(value.batch_id, 32), asset: decodeHex(value.asset, 32), previousStateRoot: decodeHex(value.previous_state_root, 32), resultingStateRoot: decodeHex(value.resulting_state_root, 32), sequencerPublicKey: decodeHex(value.sequencer_public_key, 32) });
}

async function verifySimulationEvidence(simulation: ProgramSimulation): Promise<void> {
  const evidence = simulation.simulation_evidence;
  const publicKey = decodeHex(evidence.public_key, 32);
  const expectedBoundary = await digest(concat(new TextEncoder().encode("LayerX/emulator/simulation-boundary/v1\0"), publicKey));
  if (!equal(expectedBoundary, decodeHex(evidence.boundary_id, 32))) throw new TypeError("simulation boundary mismatch");
  const signed = concat(
    new TextEncoder().encode("LayerX/agent/program-simulation-evidence/v1\0"), decodeHex(evidence.boundary_id, 32),
    decodeHex(evidence.activity_id, 32), decodeHex(evidence.previous_state_root, 32), decodeHex(evidence.hypothetical_state_root, 32),
    u64(BigInt(evidence.observed_sequence)), u64(BigInt(evidence.observed_at)), new Uint8Array([0]),
  );
  const evidenceDigest = await digest(signed);
  const key = await globalThis.crypto.subtle.importKey("raw", buffer(publicKey), { name: "Ed25519" }, false, ["verify"]);
  if (!await globalThis.crypto.subtle.verify("Ed25519", key, buffer(decodeHex(evidence.signature, 64)), buffer(evidenceDigest))) throw new TypeError("simulation evidence signature mismatch");
}

function wireCall(call: ProgramCall): Readonly<Record<string, unknown>> {
  const feeLimit = call.budget.feeLimit.toString();
  if (!DECIMAL_U128.test(feeLimit)) throw new TypeError("invalid fee limit");
  return Object.freeze({ program_id: call.programId, calldata: hex(call.calldata), budget: Object.freeze({ fuel: call.budget.fuel.toString(), fee_limit: feeLimit }), capabilities: call.capabilities, signed_activity: hex(call.signedActivity) });
}

function object(value: unknown): Readonly<Record<string, unknown>> { if (value === null || typeof value !== "object" || Array.isArray(value)) throw new TypeError("invalid program document"); return value as Readonly<Record<string, unknown>>; }
function requiredHex32(value: Readonly<Record<string, unknown>>, field: string): string { const candidate = value[field]; if (typeof candidate !== "string" || !HEX32.test(candidate)) throw new TypeError("invalid program identity"); return candidate; }
function requiredHex(value: Readonly<Record<string, unknown>>, field: string, maximum: number, exact?: number): string { const candidate = value[field]; if (typeof candidate !== "string" || candidate.length % 2 !== 0 || candidate.length > maximum * 2 || (exact !== undefined && candidate.length !== exact * 2) || !/^[0-9a-f]*$/u.test(candidate)) throw new TypeError("invalid program hexadecimal field"); return candidate; }
function decimal(value: unknown, u128 = false): string { if (typeof value !== "string" || !/^(0|[1-9][0-9]*)$/u.test(value)) throw new TypeError("invalid program decimal"); const parsed = BigInt(value); if (parsed > (u128 ? MAX_U128 : 18446744073709551615n)) throw new TypeError("program decimal overflow"); return value; }
function exactInteger(value: unknown, minimum: number, maximum: number): number { if (typeof value !== "number" || !Number.isSafeInteger(value) || value < minimum || value > maximum) throw new TypeError("invalid program integer"); return value; }
function hex(value: Uint8Array): string { return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(""); }
function decodeHex(value: string, maximum: number): Uint8Array { if (value.length % 2 !== 0 || value.length > maximum * 2 || !/^[0-9a-f]*$/u.test(value)) throw new TypeError("invalid hexadecimal evidence"); return Uint8Array.from(value.match(/.{2}/gu) ?? [], (pair) => Number.parseInt(pair, 16)); }
function equal(left: Uint8Array, right: Uint8Array): boolean { if (left.length !== right.length) return false; let difference = 0; for (let index = 0; index < left.length; index += 1) difference |= (left[index] ?? 0) ^ (right[index] ?? 0); return difference === 0; }
async function digest(value: Uint8Array): Promise<Uint8Array> { const copy = new Uint8Array(value); return new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", copy.buffer)); }
function concat(...values: readonly Uint8Array[]): Uint8Array { const output = new Uint8Array(values.reduce((length, value) => length + value.length, 0)); let offset = 0; for (const value of values) { output.set(value, offset); offset += value.length; } return output; }
function u64(value: bigint): Uint8Array { if (value < 0n || value > 18446744073709551615n) throw new TypeError("u64 overflow"); const encoded = new Uint8Array(8); let remaining = value; for (let index = 7; index >= 0; index -= 1) { encoded[index] = Number(remaining & 0xffn); remaining >>= 8n; } return encoded; }
function buffer(value: Uint8Array): ArrayBuffer { const encoded = new ArrayBuffer(value.length); new Uint8Array(encoded).set(value); return encoded; }

export function platform_sdk_programs(): string { return "receipt-verified-program-operations-v1"; }
