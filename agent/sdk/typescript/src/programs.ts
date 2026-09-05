import { encodeNativeProgramCall, type NativeProgramCall } from "./native-program-call.js";
import type { AuthorizedReceiptBatch, ReceiptVerification, SelectableProtocolVersion } from "./verifier.js";
import { DEFAULT_PROTOCOL_VERSION, isSelectableProtocolVersion, programsModuleVersionForProtocol,
  supportedProgramGuestAbi, verifyReceiptOutcome } from "./verifier.js";
import { PlatformSdkError, type IdempotencyKey, type ProductionClient } from "./production.js";
import { assertFreshSimulationObservation, decodeAndVerifyProgramTerminal, decodeSignedProgramCall,
  type DecodedSignedProgramCall } from "./program-wire.js";

const HEX32 = /^[0-9a-f]{64}$/u;
const DECIMAL_U128 = /^(0|[1-9][0-9]{0,38})$/u;
const MAX_U128 = 340282366920938463463374607431768211455n;
const MAX_CALLDATA = 1_048_576;
const MAX_CAPABILITIES = 5;
const DEFAULT_MAXIMUM_SIMULATION_AGE_MILLISECONDS = 300_000n;
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
  readonly nativeCall?: NativeProgramCall;
  readonly programId: string;
  readonly calldata: Uint8Array;
  readonly budget: ProgramBudget;
  readonly capabilities: readonly ProgramCapability[];
  readonly signedActivity: Uint8Array;
}

export class NativeProgramRequest implements ProgramCall {
  readonly programId: string;
  readonly calldata: Uint8Array;
  readonly budget: ProgramBudget;
  readonly capabilities: readonly ProgramCapability[] = [];
  constructor(readonly nativeCall: NativeProgramCall, feeLimit: bigint, readonly signedActivity: Uint8Array) {
    encodeNativeProgramCall(nativeCall);
    this.programId = hex(nativeCall.programId); this.calldata = nativeCall.calldata;
    this.budget = { fuel: nativeCall.resources[0], feeLimit };
  }
}

export interface ProgramDiscovery { readonly program_id: string; readonly lifecycle: "active" | "deprecated" | "tombstoned"; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly valid_through: string; readonly verification: "server-side-receipt-verification-only" }
export type ProgramSource = Readonly<{ status: "unpublished" }> | Readonly<{ status: "verified"; source_digest: string; environment_digest: string; pipeline: "sha256-source-artifact-reproducible-build-v1" }> | Readonly<{ status: "mismatch"; expected_code_hash: string; reproduced_artifact_digest: string }>;
export interface ProgramInterface { readonly program_id: string; readonly version: number; readonly code_hash: string; readonly abi_version: number; readonly interface: string; readonly interface_digest: string; readonly receipt_digest: string; readonly state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly valid_through: string; readonly source: ProgramSource; readonly verification: "server-side-receipt-verification-only" }
export type ProgramFailure = Readonly<{ kind: "unknown_program" | "reentrancy" | "depth_exceeded" | "fanout_exceeded" | "guest_refused" | "authority" | "resource" | "response" | "fault"; code?: number; limit?: number; attempted?: number }>;
export type ProgramOutcome = Readonly<{ kind: "completed"; code: number; response: string }> | Readonly<{ kind: "legacy_completed"; code: number; values: readonly unknown[] }> | Readonly<{ kind: "refused"; failure: ProgramFailure }>;
export interface ProgramUsage { readonly cpu_fuel: string; readonly memory_bytes: string; readonly storage_read_bytes: string; readonly storage_write_bytes: string; readonly output_values: number; readonly output_bytes: string; readonly fee_units: string }
export interface ProgramAuthorityDocument { readonly batch_id: string; readonly asset: string; readonly previous_state_root: string; readonly resulting_state_root: string; readonly sequencer_public_key: string }
export interface ProgramExecutionDocument { readonly state: "executed" | "refused" | "simulated"; readonly activity_id: string; readonly program_id: string; readonly guest_abi_version: number; readonly module_version: number; readonly batch_id: string; readonly global_sequence: string; readonly result_code: number; readonly state_root: string; readonly receipt: string; readonly receipt_digest: string; readonly terminal_payload: string; readonly call_graph: string; readonly authority: ProgramAuthorityDocument; readonly usage: ProgramUsage; readonly outcome: ProgramOutcome; readonly verification: "receipt-terminal-and-call-graph-verified"; readonly idempotency_key?: string }
export interface ProgramUnknownSubmission { readonly state: "unknown"; readonly activity_id: string; readonly idempotency_key: string; readonly retained_signed_activity?: string }
export type ProgramSubmission = ProgramUnknownSubmission | (ProgramExecutionDocument & Readonly<{ state: "executed" | "refused" }>);
export interface ProgramSimulationEvidence { readonly boundary_id: string; readonly activity_id: string; readonly previous_state_root: string; readonly hypothetical_state_root: string; readonly observed_sequence: string; readonly observed_at: string; readonly committed: false; readonly public_key: string; readonly signature: string }
export interface ProgramSimulation { readonly committed: false; readonly execution: ProgramExecutionDocument & Readonly<{ state: "simulated" }>; readonly simulation_evidence: ProgramSimulationEvidence }
export interface VerifiedProgramReceipt { readonly verification: ReceiptVerification; readonly terminalPayload: Uint8Array; readonly callGraph: Uint8Array }

export class ProgramTrustContext {
  readonly #sequencerPublicKey: Uint8Array;
  readonly #clockMilliseconds: () => bigint;
  readonly #maximumSimulationAgeMilliseconds: bigint;
  readonly #protocolVersion: SelectableProtocolVersion;

  public constructor(
    sequencerPublicKey: Uint8Array,
    clockMilliseconds: () => bigint = () => BigInt(Date.now()),
    maximumSimulationAgeMilliseconds: bigint = DEFAULT_MAXIMUM_SIMULATION_AGE_MILLISECONDS,
    protocolVersion: SelectableProtocolVersion = DEFAULT_PROTOCOL_VERSION,
  ) {
    if (sequencerPublicKey.length !== 32 || sequencerPublicKey.every((value) => value === 0)
      || maximumSimulationAgeMilliseconds <= 0n
      || maximumSimulationAgeMilliseconds > 18446744073709551615n
      || !isSelectableProtocolVersion(protocolVersion)) throw new TypeError("invalid Programs trust context");
    this.#sequencerPublicKey = new Uint8Array(sequencerPublicKey);
    this.#clockMilliseconds = clockMilliseconds;
    this.#maximumSimulationAgeMilliseconds = maximumSimulationAgeMilliseconds;
    this.#protocolVersion = protocolVersion;
    Object.freeze(this);
  }

  public sequencerPublicKey(): Uint8Array { return new Uint8Array(this.#sequencerPublicKey); }
  public protocolVersion(): SelectableProtocolVersion { return this.#protocolVersion; }
  public nowMilliseconds(): bigint {
    const value = this.#clockMilliseconds();
    if (value < 0n || value > 18446744073709551615n) throw new TypeError("invalid trust clock");
    return value;
  }
  public maximumSimulationAgeMilliseconds(): bigint { return this.#maximumSimulationAgeMilliseconds; }
}

function validateCall(call: ProgramCall): void {
  if (!HEX32.test(call.programId) || call.programId === "0".repeat(64) || call.calldata.length > MAX_CALLDATA || call.signedActivity.length === 0 || call.signedActivity.length > MAX_CALLDATA
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
  trust: ProgramTrustContext,
): Promise<VerifiedProgramReceipt> {
  const protocolVersion = trust.protocolVersion();
  if (!HEX32.test(execution.activity_id)
    || !programsModuleVersionForProtocol(protocolVersion, execution.module_version, false)
    || !supportedProgramGuestAbi(execution.guest_abi_version)) throw new TypeError("invalid program execution evidence");
  const receipt = decodeHex(execution.receipt, 1_048_576);
  const terminalPayload = decodeHex(execution.terminal_payload, 1_048_576);
  const callGraph = decodeHex(execution.call_graph, 1_048_576);
  const pinnedKey = trust.sequencerPublicKey();
  if (!equal(authority.sequencerPublicKey, pinnedKey)
    || execution.authority.sequencer_public_key !== hex(pinnedKey)) throw new TypeError("program sequencer authority mismatch");
  const verification = await verifyReceiptOutcome(receipt, authority, { protocolVersion });
  const protocol = verification.receipt;
  const outcome = protocol.programOutcome;
  if (protocol.moduleId !== 9 || protocol.operation !== 3 || protocol.protocolVersion !== protocolVersion
    || !programsModuleVersionForProtocol(protocol.protocolVersion, protocol.moduleVersion, false)
    || protocol.moduleVersion !== execution.module_version
    || hex(protocol.activityId) !== execution.activity_id || outcome === undefined
    || outcome.abiVersion !== execution.guest_abi_version
    || outcome.resultCode !== execution.result_code
    || hex(protocol.batchId) !== execution.batch_id || execution.batch_id !== execution.authority.batch_id
    || protocol.globalSequence.toString() !== execution.global_sequence
    || hex(protocol.previousStateRoot) !== execution.authority.previous_state_root
    || hex(protocol.resultingStateRoot) !== execution.state_root
    || execution.state_root !== execution.authority.resulting_state_root
    || hex(verification.receiptDigest) !== execution.receipt_digest
    || callGraph.length === 0
    || !equal(await digest(terminalPayload), outcome.terminalPayloadRoot)
    || !equal(await digest(callGraph), outcome.callGraphRoot)) {
    throw new TypeError("program receipt binding failed");
  }
  const terminal = await decodeAndVerifyProgramTerminal(terminalPayload, callGraph, execution.program_id, outcome, protocol.protocolVersion);
  if (!sameUsage(terminal.usage, execution.usage) || !sameOutcome(terminal.outcome, execution.outcome)) throw new TypeError("program terminal document binding failed");
  return Object.freeze({ verification, terminalPayload, callGraph });
}

export class ProgramOperations {
  readonly #heads = new Map<string, ProgramHeadObservation>();

  public constructor(private readonly client: ProductionClient, private readonly trust: ProgramTrustContext) {}

  public async discover(programId: string): Promise<ProgramDiscovery> {
    if (!HEX32.test(programId)) throw new TypeError("invalid program id");
    const value = await this.client.agent<unknown, unknown>("program.discover", { program_id: programId, requested_verification_level: "sequencer-signed" });
    const result = discovery(value, programId, this.trust.nowMilliseconds());
    this.#rememberHead(programId, head(result));
    return result;
  }

  public async interface(programId: string): Promise<ProgramInterface> {
    if (!HEX32.test(programId)) throw new TypeError("invalid program id");
    const value = await this.client.agent<unknown, unknown>("program.interface", { program_id: programId, requested_verification_level: "sequencer-signed" });
    const result = await programInterface(value, programId, this.trust.nowMilliseconds());
    this.#rememberHead(programId, head(result));
    return result;
  }

  public async simulate(call: ProgramCall): Promise<ProgramSimulation> {
    validateCall(call);
    if (call.nativeCall !== undefined && this.trust.protocolVersion() !== 3) throw new TypeError("native call requires selected protocol 3");
    const signed = await decodeSignedProgramCall(call);
    const prior = this.#heads.get(call.programId);
    if (prior === undefined) throw new TypeError("a fresh discovered program head is required before simulation");
    requireFreshHead(prior, this.trust.nowMilliseconds());
    const value = await this.client.agent<unknown, unknown>("program.simulate", wireCall(call));
    this.#requireCurrentHead(call.programId, prior);
    requireFreshHead(prior, this.trust.nowMilliseconds());
    const simulation = simulationDocument(value, call.programId, signed.activityId);
    const verified = await verifyProgramReceipt(simulation.execution, wireAuthority(simulation.execution.authority, this.trust), this.trust);
    await verifySimulationEvidence(simulation, verified, prior, signed, this.trust);
    this.#requireCurrentHead(call.programId, prior);
    requireFreshHead(prior, this.trust.nowMilliseconds());
    return simulation;
  }

  #rememberHead(programId: string, candidate: ProgramHeadObservation): void {
    const current = this.#heads.get(programId);
    if (current !== undefined && (candidate.sequence < current.sequence
      || candidate.observedAt < current.observedAt
      || (candidate.sequence === current.sequence && (candidate.stateRoot !== current.stateRoot
        || candidate.validThrough < current.validThrough)))) throw new TypeError("program head rollback or conflict");
    this.#heads.set(programId, candidate);
  }

  #requireCurrentHead(programId: string, expected: ProgramHeadObservation): void {
    const current = this.#heads.get(programId);
    if (current === undefined || current.sequence !== expected.sequence || current.stateRoot !== expected.stateRoot
      || current.observedAt !== expected.observedAt || current.validThrough !== expected.validThrough) {
      throw new TypeError("program head changed during simulation");
    }
  }

  public async submit(call: ProgramCall, idempotencyKey: IdempotencyKey): Promise<ProgramSubmission> {
    validateCall(call);
    if (call.nativeCall !== undefined && this.trust.protocolVersion() !== 3) throw new TypeError("native call requires selected protocol 3");
    if (!HEX32.test(idempotencyKey)) throw new TypeError("invalid program idempotency key");
    const signed = await decodeSignedProgramCall(call, idempotencyKey);
    const retained = hex(signed.canonicalBytes);
    try {
      const value = await this.client.agent<unknown, unknown>("program.call", wireCall(call), { idempotencyKey });
      return await submissionDocument(value, this.trust, { programId: call.programId, activityId: signed.activityId,
        idempotencyKey, retainedSignedActivity: retained });
    } catch (error) {
      if (definitiveServiceRefusal(error)) throw error;
      return Object.freeze({ state: "unknown", activity_id: signed.activityId, idempotency_key: idempotencyKey,
        retained_signed_activity: retained });
    }
  }

  public async receipt(idempotencyKey: string, expectedActivityId: string): Promise<ProgramSubmission> {
    if (!HEX32.test(idempotencyKey) || !HEX32.test(expectedActivityId)) throw new TypeError("invalid program receipt selector");
    const value = await this.client.agent<unknown, unknown>("program.receipt", { idempotency_key: idempotencyKey,
      expected_activity_id: expectedActivityId, requested_verification_level: "sequencer-signed" });
    return await submissionDocument(value, this.trust, { idempotencyKey, activityId: expectedActivityId });
  }

  public async activity(activityId: string): Promise<ProgramSubmission> {
    if (!HEX32.test(activityId)) throw new TypeError("invalid activity id");
    const value = await this.client.agent<unknown, unknown>("program.activity", { activity_id: activityId,
      requested_verification_level: "sequencer-signed" });
    return await submissionDocument(value, this.trust, { activityId });
  }
}

interface SubmissionExpectation { readonly programId?: string; readonly activityId?: string; readonly idempotencyKey?: string; readonly retainedSignedActivity?: string }
interface ProgramHeadObservation { readonly stateRoot: string; readonly sequence: bigint; readonly observedAt: bigint; readonly validThrough: bigint }

async function submissionDocument(value: unknown, trust: ProgramTrustContext, expected: SubmissionExpectation): Promise<ProgramSubmission> {
  const candidate = object(value);
  if (candidate.state === "unknown") {
    exactKeys(candidate, ["state", "activity_id", "idempotency_key"], ["retained_signed_activity"]);
    const activityId = requiredHex32(candidate, "activity_id");
    const idempotencyKey = requiredHex32(candidate, "idempotency_key");
    const retained = candidate.retained_signed_activity === undefined ? undefined : requiredHex(candidate, "retained_signed_activity", MAX_CALLDATA);
    if ((expected.activityId !== undefined && activityId !== expected.activityId)
      || (expected.idempotencyKey !== undefined && idempotencyKey !== expected.idempotencyKey)
      || (expected.retainedSignedActivity !== undefined && retained !== undefined && retained !== expected.retainedSignedActivity)) throw new TypeError("program unknown binding failed");
    const boundRetained = retained ?? expected.retainedSignedActivity;
    return Object.freeze({ state: "unknown", activity_id: activityId, idempotency_key: idempotencyKey,
      ...(boundRetained === undefined ? {} : { retained_signed_activity: boundRetained }) });
  }
  if (candidate.state !== "executed" && candidate.state !== "refused") throw new TypeError("invalid program submission state");
  const execution = executionDocument(candidate, candidate.state);
  if ((expected.programId !== undefined && execution.program_id !== expected.programId)
    || (expected.activityId !== undefined && execution.activity_id !== expected.activityId)
    || (expected.idempotencyKey !== undefined && execution.idempotency_key !== expected.idempotencyKey)) throw new TypeError("program execution binding failed");
  await verifyProgramReceipt(execution, wireAuthority(execution.authority, trust), trust);
  return execution as ProgramSubmission;
}

function simulationDocument(value: unknown, expectedProgramId: string, expectedActivityId: string): ProgramSimulation {
  const candidate = object(value);
  exactKeys(candidate, ["committed", "execution", "simulation_evidence"]);
  if (candidate.committed !== false) throw new TypeError("committed program simulation");
  const execution = executionDocument(object(candidate.execution), "simulated") as ProgramSimulation["execution"];
  if (execution.program_id !== expectedProgramId || execution.activity_id !== expectedActivityId) throw new TypeError("program simulation binding failed");
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
  exactKeys(rawEvidence, ["boundary_id", "activity_id", "previous_state_root", "hypothetical_state_root",
    "observed_sequence", "observed_at", "committed", "public_key", "signature"]);
  if (evidence.activity_id !== execution.activity_id || evidence.hypothetical_state_root !== execution.state_root) throw new TypeError("program simulation evidence binding failed");
  return Object.freeze({ committed: false, execution, simulation_evidence: evidence });
}

function executionDocument(candidate: Readonly<Record<string, unknown>>, state: "executed" | "refused" | "simulated"): ProgramExecutionDocument {
  if (candidate.state !== state) throw new TypeError("invalid program execution state");
  exactKeys(candidate, ["state", "activity_id", "program_id", "guest_abi_version", "module_version", "batch_id",
    "global_sequence", "result_code", "state_root", "receipt", "receipt_digest", "terminal_payload", "call_graph",
    "authority", "usage", "outcome", "verification"], ["idempotency_key"]);
  const usage = object(candidate.usage);
  const authority = object(candidate.authority);
  exactKeys(authority, ["batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key"]);
  exactKeys(usage, ["cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes", "output_values", "output_bytes", "fee_units"]);
  const result: ProgramExecutionDocument = Object.freeze({
    state,
    activity_id: requiredHex32(candidate, "activity_id"),
    program_id: requiredHex32(candidate, "program_id"),
    guest_abi_version: exactInteger(candidate.guest_abi_version, 1, 2),
    module_version: exactInteger(candidate.module_version, 1, 3),
    batch_id: requiredHex32(candidate, "batch_id"),
    global_sequence: decimal(candidate.global_sequence),
    result_code: exactInteger(candidate.result_code, -2147483648, 2147483647),
    state_root: requiredHex32(candidate, "state_root"),
    receipt: requiredHex(candidate, "receipt", MAX_CALLDATA),
    receipt_digest: requiredHex32(candidate, "receipt_digest"),
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
    outcome: programOutcome(candidate.outcome),
    verification: candidate.verification === "receipt-terminal-and-call-graph-verified"
      ? candidate.verification : (() => { throw new TypeError("invalid program verification status"); })(),
    ...(candidate.idempotency_key === undefined ? {} : { idempotency_key: requiredHex32(candidate, "idempotency_key") }),
  });
  if ((state === "refused" && result.outcome.kind !== "refused")
    || (state === "executed" && result.outcome.kind === "refused")) throw new TypeError("program state/outcome mismatch");
  return result;
}

function discovery(value: unknown, programId: string, now: bigint): ProgramDiscovery {
  const candidate = object(value);
  exactKeys(candidate, ["program_id", "lifecycle", "version", "code_hash", "abi_version", "receipt_digest",
    "state_root", "observed_sequence", "observed_at", "valid_through", "verification"]);
  if (requiredHex32(candidate, "program_id") !== programId || candidate.verification !== "registry-receipt-and-current-head-verified") throw new TypeError("unverified program discovery");
  if (candidate.lifecycle !== "active" && candidate.lifecycle !== "deprecated" && candidate.lifecycle !== "tombstoned") throw new TypeError("invalid program lifecycle");
  const result: ProgramDiscovery = Object.freeze({
    program_id: programId, lifecycle: candidate.lifecycle, version: exactInteger(candidate.version, 1, 0xffff_ffff),
    code_hash: requiredHex32(candidate, "code_hash"), abi_version: exactInteger(candidate.abi_version, 1, 2),
    receipt_digest: requiredHex32(candidate, "receipt_digest"), state_root: requiredHex32(candidate, "state_root"),
    observed_sequence: decimal(candidate.observed_sequence), observed_at: decimal(candidate.observed_at), valid_through: decimal(candidate.valid_through),
    verification: "server-side-receipt-verification-only",
  });
  requireFreshHead(head(result), now);
  return result;
}

async function programInterface(value: unknown, programId: string, now: bigint): Promise<ProgramInterface> {
  const candidate = object(value);
  exactKeys(candidate, ["program_id", "version", "code_hash", "abi_version", "interface", "interface_digest",
    "receipt_digest", "state_root", "observed_sequence", "observed_at", "valid_through", "source", "verification"]);
  if (requiredHex32(candidate, "program_id") !== programId || candidate.verification !== "deployment-interface-and-current-head-verified") throw new TypeError("unverified program interface");
  const encodedInterface = requiredHex(candidate, "interface", 952);
  if (encodedInterface.length === 0 || hex(await digest(decodeHex(encodedInterface, 952))) !== requiredHex32(candidate, "interface_digest")) throw new TypeError("program interface digest mismatch");
  const result: ProgramInterface = Object.freeze({
    program_id: programId, version: exactInteger(candidate.version, 1, 0xffff_ffff), code_hash: requiredHex32(candidate, "code_hash"),
    abi_version: exactInteger(candidate.abi_version, 1, 2), interface: encodedInterface, interface_digest: requiredHex32(candidate, "interface_digest"),
    receipt_digest: requiredHex32(candidate, "receipt_digest"), state_root: requiredHex32(candidate, "state_root"),
    observed_sequence: decimal(candidate.observed_sequence), observed_at: decimal(candidate.observed_at), valid_through: decimal(candidate.valid_through),
    source: programSource(candidate.source), verification: "server-side-receipt-verification-only",
  });
  requireFreshHead(head(result), now);
  return result;
}

function wireAuthority(value: ProgramAuthorityDocument, trust: ProgramTrustContext): AuthorizedReceiptBatch {
  const pinned = trust.sequencerPublicKey();
  if (value.sequencer_public_key !== hex(pinned)) throw new TypeError("program sequencer key does not match pin");
  return Object.freeze({ batchId: decodeHex(value.batch_id, 32), asset: decodeHex(value.asset, 32), previousStateRoot: decodeHex(value.previous_state_root, 32), resultingStateRoot: decodeHex(value.resulting_state_root, 32), sequencerPublicKey: pinned });
}

async function verifySimulationEvidence(simulation: ProgramSimulation, verified: VerifiedProgramReceipt,
  prior: ProgramHeadObservation, binding: DecodedSignedProgramCall, trust: ProgramTrustContext): Promise<void> {
  const evidence = simulation.simulation_evidence;
  const publicKey = decodeHex(evidence.public_key, 32);
  const pinned = trust.sequencerPublicKey();
  const now = trust.nowMilliseconds();
  if (!equal(publicKey, pinned)) throw new TypeError("simulation sequencer key does not match pin");
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
  const protocol = verified.verification.receipt;
  const observedSequence = BigInt(evidence.observed_sequence);
  const observedAt = BigInt(evidence.observed_at);
  requireFreshHead(prior, now);
  assertFreshSimulationObservation(observedAt, binding, now, trust.maximumSimulationAgeMilliseconds());
  if (evidence.previous_state_root !== prior.stateRoot || observedSequence !== prior.sequence
    || hex(protocol.previousStateRoot) !== prior.stateRoot
    || protocol.globalSequence !== observedSequence + 1n || observedAt < prior.observedAt
    || observedAt > prior.validThrough) throw new TypeError("stale or mismatched simulation head");
}

function wireCall(call: ProgramCall): Readonly<Record<string, unknown>> {
  const feeLimit = call.budget.feeLimit.toString();
  if (!DECIMAL_U128.test(feeLimit)) throw new TypeError("invalid fee limit");
  if (call.nativeCall !== undefined) {
    const native = call.nativeCall;
    return Object.freeze({ payload_encoding: "native-v1", program_id: call.programId, calldata: hex(call.calldata),
      budget: { fuel: call.budget.fuel.toString(), fee_limit: feeLimit }, signed_activity: hex(call.signedActivity),
      native_call: { guest_abi: native.guestAbi, entrypoint: native.entrypoint, capabilities_hex: hex(native.capabilities),
        access_declaration_hex: hex(native.accessDeclaration), response_capacity: native.responseCapacity,
        resources: native.resources.map(value => value.toString()) } });
  }
  return Object.freeze({ program_id: call.programId, calldata: hex(call.calldata), budget: Object.freeze({ fuel: call.budget.fuel.toString(), fee_limit: feeLimit }), capabilities: call.capabilities, signed_activity: hex(call.signedActivity) });
}

function programSource(value: unknown): ProgramSource {
  const source = object(value);
  if (source.status === "unpublished") {
    exactKeys(source, ["status"]);
    return Object.freeze({ status: "unpublished" });
  }
  if (source.status === "verified") {
    exactKeys(source, ["status", "source_digest", "environment_digest", "pipeline"]);
    if (source.pipeline !== "sha256-source-artifact-reproducible-build-v1") throw new TypeError("invalid program source pipeline");
    return Object.freeze({ status: "verified", source_digest: requiredHex32(source, "source_digest"),
      environment_digest: requiredHex32(source, "environment_digest"), pipeline: source.pipeline });
  }
  if (source.status === "mismatch") {
    exactKeys(source, ["status", "expected_code_hash", "reproduced_artifact_digest"]);
    return Object.freeze({ status: "mismatch", expected_code_hash: requiredHex32(source, "expected_code_hash"),
      reproduced_artifact_digest: requiredHex32(source, "reproduced_artifact_digest") });
  }
  throw new TypeError("invalid program source status");
}

function programOutcome(value: unknown): ProgramOutcome {
  const outcome = object(value);
  if (outcome.kind === "completed") {
    exactKeys(outcome, ["kind", "code", "response"]);
    return Object.freeze({ kind: "completed", code: exactInteger(outcome.code, 0, 0x7fff_ffff),
      response: requiredHex(outcome, "response", MAX_CALLDATA) });
  }
  if (outcome.kind === "legacy_completed") {
    exactKeys(outcome, ["kind", "code", "values"]);
    if (!Array.isArray(outcome.values) || outcome.values.length > Math.floor(MAX_CALLDATA / 5)) throw new TypeError("invalid legacy program values");
    const values = outcome.values.map((raw) => {
      const valueDocument = object(raw);
      exactKeys(valueDocument, ["type", "value"]);
      if (valueDocument.type === "i32") return Object.freeze({ type: "i32", value: exactInteger(valueDocument.value, -2147483648, 2147483647) });
      if (valueDocument.type === "i64" && typeof valueDocument.value === "string" && /^-?(0|[1-9][0-9]*)$/u.test(valueDocument.value)) {
        const parsed = BigInt(valueDocument.value);
        if (parsed >= -9223372036854775808n && parsed <= 9223372036854775807n) return Object.freeze({ type: "i64", value: valueDocument.value });
      }
      throw new TypeError("invalid legacy program value");
    });
    return Object.freeze({ kind: "legacy_completed", code: exactInteger(outcome.code, -2147483648, 2147483647), values: Object.freeze(values) });
  }
  if (outcome.kind === "refused") {
    exactKeys(outcome, ["kind", "failure"]);
    return Object.freeze({ kind: "refused", failure: programFailure(outcome.failure) });
  }
  throw new TypeError("invalid program outcome");
}

function programFailure(value: unknown): ProgramFailure {
  const failure = object(value);
  const kind = failure.kind;
  if (kind === "depth_exceeded" || kind === "fanout_exceeded") {
    exactKeys(failure, ["kind", "limit", "attempted"]);
    return Object.freeze({ kind, limit: exactInteger(failure.limit, 0, 0xffff_ffff),
      attempted: exactInteger(failure.attempted, 0, 0xffff_ffff) });
  }
  if (kind === "guest_refused") {
    exactKeys(failure, ["kind", "code"]);
    return Object.freeze({ kind, code: exactInteger(failure.code, -2147483648, 2147483647) });
  }
  if (kind === "unknown_program" || kind === "reentrancy" || kind === "authority" || kind === "resource"
    || kind === "response" || kind === "fault") {
    exactKeys(failure, ["kind"]);
    return Object.freeze({ kind });
  }
  throw new TypeError("invalid program failure");
}

function head(value: ProgramDiscovery | ProgramInterface): ProgramHeadObservation {
  return Object.freeze({ stateRoot: value.state_root, sequence: BigInt(value.observed_sequence),
    observedAt: BigInt(value.observed_at), validThrough: BigInt(value.valid_through) });
}

function requireFreshHead(value: ProgramHeadObservation, now: bigint): void {
  if (value.observedAt > now || now > value.validThrough || value.validThrough < value.observedAt) throw new TypeError("stale program head");
}

function sameUsage(left: ProgramUsage, right: ProgramUsage): boolean {
  return left.cpu_fuel === right.cpu_fuel && left.memory_bytes === right.memory_bytes
    && left.storage_read_bytes === right.storage_read_bytes && left.storage_write_bytes === right.storage_write_bytes
    && left.output_values === right.output_values && left.output_bytes === right.output_bytes && left.fee_units === right.fee_units;
}

function sameOutcome(left: ProgramOutcome, right: ProgramOutcome): boolean {
  if (left.kind !== right.kind) return false;
  if (left.kind === "completed" && right.kind === "completed") return left.code === right.code && left.response === right.response;
  if (left.kind === "legacy_completed" && right.kind === "legacy_completed") return left.code === right.code && JSON.stringify(left.values) === JSON.stringify(right.values);
  if (left.kind === "refused" && right.kind === "refused") return JSON.stringify(left.failure) === JSON.stringify(right.failure);
  return false;
}

function definitiveServiceRefusal(error: unknown): boolean {
  return error instanceof PlatformSdkError && error.retry === "never" && ["invalid-argument", "idempotency-required",
    "protocol-incompatibility", "unavailable-capability", "core-rejection", "policy-refusal", "capability-refusal",
    "budget-refusal", "idempotency-conflict"].includes(error.code);
}

function object(value: unknown): Readonly<Record<string, unknown>> { if (value === null || typeof value !== "object" || Array.isArray(value)) throw new TypeError("invalid program document"); return value as Readonly<Record<string, unknown>>; }
function exactKeys(value: Readonly<Record<string, unknown>>, required: readonly string[], optional: readonly string[] = []): void { const allowed = new Set([...required, ...optional]); if (required.some((key) => !(key in value)) || Object.keys(value).some((key) => !allowed.has(key))) throw new TypeError("invalid program document fields"); }
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
