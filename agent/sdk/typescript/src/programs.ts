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

export interface ProgramDiscovery { readonly program_id: string; readonly lifecycle: "active" | "deprecated" | "tombstoned"; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: number; readonly observed_at: number; readonly valid_through: number; readonly verification: "registry-receipt-and-current-head-verified" }
export interface ProgramInterface { readonly program_id: string; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly interface: string; readonly interface_digest: string; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: number; readonly observed_at: number; readonly valid_through: number; readonly source: Readonly<Record<string, unknown>>; readonly verification: "deployment-interface-and-current-head-verified" }
export type ProgramFailure = Readonly<{ kind: "unknown_program" | "reentrancy" | "depth_exceeded" | "fanout_exceeded" | "guest_refused" | "authority" | "resource" | "response" | "fault"; code?: number; limit?: number; attempted?: number }>;
export type ProgramOutcome = Readonly<{ kind: "completed"; code: number; response: string }> | Readonly<{ kind: "legacy_completed"; code: number; values: readonly unknown[] }> | Readonly<{ kind: "refused"; failure: ProgramFailure }>;
export interface ProgramUsage { readonly cpu_fuel: number; readonly memory_bytes: number; readonly storage_read_bytes: number; readonly storage_write_bytes: number; readonly output_values: number; readonly output_bytes: number; readonly fee_units: string }
export interface ProgramExecutionDocument { readonly state: "executed" | "simulated"; readonly activity_id: string; readonly program_id: string; readonly guest_abi_version: number; readonly module_version: number; readonly result_code: number; readonly state_root: string; readonly receipt: string; readonly terminal_payload: string; readonly call_graph: string; readonly usage: ProgramUsage; readonly outcome: ProgramOutcome; readonly idempotency_key?: string }
export type ProgramSubmission = Readonly<{ state: "unknown"; activity_id: string; idempotency_key: string; retained_signed_activity?: string }> | ProgramExecutionDocument;
export interface ProgramSimulation { readonly committed: false; readonly execution: ProgramExecutionDocument; readonly simulation_evidence: Readonly<Record<string, unknown>> }
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
  public discover(programId: string): Promise<ProgramDiscovery> { if (!HEX32.test(programId)) throw new TypeError("invalid program id"); return this.client.agent("program.discover", { program_id: programId, requested_verification_level: "sequencer-signed" }); }
  public interface(programId: string): Promise<ProgramInterface> { if (!HEX32.test(programId)) throw new TypeError("invalid program id"); return this.client.agent("program.interface", { program_id: programId, requested_verification_level: "sequencer-signed" }); }
  public simulate(call: ProgramCall): Promise<ProgramSimulation> { validateCall(call); return this.client.agent("program.simulate", wireCall(call)); }
  public submit(call: ProgramCall, idempotencyKey: IdempotencyKey): Promise<ProgramSubmission> { validateCall(call); return this.client.agent("program.call", wireCall(call), { idempotencyKey }); }
  public async receipt(idempotencyKey: string, expectedActivityId: string): Promise<ProgramSubmission> { if (!HEX32.test(idempotencyKey) || !HEX32.test(expectedActivityId)) throw new TypeError("invalid program receipt selector"); const result = await this.client.agent<ProgramSubmission>("program.receipt", { idempotency_key: idempotencyKey, expected_activity_id: expectedActivityId, requested_verification_level: "sequencer-signed" }); if (result.activity_id !== expectedActivityId) throw new TypeError("program receipt selector binding failed"); return result; }
  public activity(activityId: string): Promise<ProgramSubmission> { if (!HEX32.test(activityId)) throw new TypeError("invalid activity id"); return this.client.agent("program.activity", { activity_id: activityId, requested_verification_level: "sequencer-signed" }); }
}

function wireCall(call: ProgramCall): Readonly<Record<string, unknown>> {
  const feeLimit = call.budget.feeLimit.toString();
  if (!DECIMAL_U128.test(feeLimit)) throw new TypeError("invalid fee limit");
  return Object.freeze({ program_id: call.programId, calldata: hex(call.calldata), budget: Object.freeze({ fuel: call.budget.fuel.toString(), fee_limit: feeLimit }), capabilities: call.capabilities, signed_activity: hex(call.signedActivity) });
}

function hex(value: Uint8Array): string { return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(""); }
function decodeHex(value: string, maximum: number): Uint8Array { if (value.length % 2 !== 0 || value.length > maximum * 2 || !/^[0-9a-f]*$/u.test(value)) throw new TypeError("invalid hexadecimal evidence"); return Uint8Array.from(value.match(/.{2}/gu) ?? [], (pair) => Number.parseInt(pair, 16)); }
function equal(left: Uint8Array, right: Uint8Array): boolean { if (left.length !== right.length) return false; let difference = 0; for (let index = 0; index < left.length; index += 1) difference |= (left[index] ?? 0) ^ (right[index] ?? 0); return difference === 0; }
async function digest(value: Uint8Array): Promise<Uint8Array> { const copy = new Uint8Array(value); return new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", copy.buffer)); }

export function platform_sdk_programs(): string { return "receipt-verified-program-operations-v1"; }
