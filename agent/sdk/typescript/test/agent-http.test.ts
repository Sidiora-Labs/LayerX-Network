import { once } from "node:events";
import * as http from "node:http";

import {
  AgentHttpTransport,
  idempotencyKey,
  LayerXKeyCredential,
  ProgramOperations,
  ProgramTrustContext,
  ProductionClient,
  SecretBytes,
} from "../src/index.js";
import { assertFreshSimulationObservation, decodeAndVerifyProgramTerminal, decodeSignedProgramCall } from "../src/program-wire.js";
import type { ProgramReceiptOutcome } from "../src/verifier.js";

function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

async function rejectsWith(action: () => Promise<unknown>, expected: string, message: string): Promise<void> {
  let failure: unknown;
  try { await action(); } catch (error) { failure = error; }
  assert(failure instanceof Error && failure.message.includes(expected), message);
}

const programId = "11".repeat(32);
const bodies: Buffer[] = [];
const server = http.createServer((request, response) => {
  const chunks: Buffer[] = [];
  request.on("data", (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
  request.on("end", () => {
    bodies.push(Buffer.concat(chunks));
    assert(request.method === "GET", "program discovery did not use GET");
    assert(request.url === `/v1/programs/registry/${programId}`, "program path was not exact");
    assert(request.headers.authorization === `LayerX-Key key_1:lxp_live_${"22".repeat(32)}`, "LayerX-Key authentication changed");
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({
      request_id: "1",
      value: { program_id: programId },
      verification_status: { state: "Unverified", requested: "SequencerSigned", achieved: "Unverified", reason: "server_side_receipt_verification_only" },
    }));
  });
});
server.listen(0, "127.0.0.1");
await once(server, "listening");
const address = server.address();
assert(address !== null && typeof address === "object", "test listener missing");

try {
  const credential = new LayerXKeyCredential(
    "key_1",
    new SecretBytes(Buffer.from(`lxp_live_${"22".repeat(32)}`, "ascii")),
  );
  const client = new ProductionClient(new AgentHttpTransport({
    endpoint: `http://127.0.0.1:${address.port}`,
    credential,
  }));
  const value = await client.agent<{ program_id: string; requested_verification_level: string }, { program_id: string }>(
    "program.discover",
    { program_id: programId, requested_verification_level: "sequencer-signed" },
  );
  assert(value.program_id === programId, "success envelope value changed");
  assert(
    JSON.parse(bodies[0]?.toString("utf8") ?? "null").requested_verification_level === "sequencer-signed",
    "GET verification request body was discarded",
  );
} finally {
  server.close();
  await once(server, "close");
}

const callIdempotency = "33".repeat(32);
const signedActivity = await canonicalProgramCall(programId, callIdempotency);
const signedBinding = await decodeSignedProgramCall({
  programId,
  calldata: new Uint8Array([0xaa]),
  budget: { fuel: 1n, feeLimit: 0n },
  capabilities: [],
  signedActivity,
});
assert(signedBinding.notBefore === 10n && signedBinding.notAfter === 20n, "signed Programs validity window was discarded");
assertFreshSimulationObservation(15n, signedBinding, 15n, 5n);
for (const [observedAt, now, maximumAge] of [[9n, 15n, 10n], [21n, 21n, 10n], [15n, 14n, 10n], [15n, 21n, 5n]] as const) {
  let rejected = false;
  try { assertFreshSimulationObservation(observedAt, signedBinding, now, maximumAge); } catch { rejected = true; }
  assert(rejected, "out-of-window Programs simulation observation was accepted");
}
const malformedServer = http.createServer((_request, response) => {
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(JSON.stringify({ malformed: true }));
});
malformedServer.listen(0, "127.0.0.1");
await once(malformedServer, "listening");
const malformedAddress = malformedServer.address();
assert(malformedAddress !== null && typeof malformedAddress === "object", "malformed-response listener missing");
try {
  const programs = new ProgramOperations(
    new ProductionClient(new AgentHttpTransport({ endpoint: `http://127.0.0.1:${malformedAddress.port}` })),
    new ProgramTrustContext(Uint8Array.from({ length: 32 }, () => 0x44), () => 15n),
  );
  const unknown = await programs.submit({
    programId,
    calldata: new Uint8Array([0xaa]),
    budget: { fuel: 1n, feeLimit: 0n },
    capabilities: [],
    signedActivity,
  }, idempotencyKey(callIdempotency));
  assert(unknown.state === "unknown", "ambiguous call was not tagged unknown");
  assert(unknown.activity_id === await activityId(signedActivity), "unknown call activity ID was not derived from signed bytes");
  assert(unknown.retained_signed_activity === Buffer.from(signedActivity).toString("hex"), "unknown call did not retain signed bytes");
} finally {
  malformedServer.close();
  await once(malformedServer, "close");
}

let zeroKeyRejected = false;
try { new ProgramTrustContext(new Uint8Array(32)); } catch { zeroKeyRejected = true; }
assert(zeroKeyRejected, "all-zero Programs sequencer key was accepted");

let trustNow = 15n;
let headSequence = 10n;
let headRoot = "55".repeat(32);
let simulationMode: "expiry" | "race" = "expiry";
let signalSimulation: (() => void) | undefined;
let releaseSimulation: (() => void) | undefined;
let simulationEntered = Promise.resolve();
let simulationRelease = Promise.resolve();
const trustServer = http.createServer(async (request, response) => {
  if (request.url === `/v1/programs/registry/${programId}`) {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(programEnvelope({
      program_id: programId, lifecycle: "active", version: 1, code_hash: "22".repeat(32), abi_version: 2,
      receipt_digest: "33".repeat(32), state_root: headRoot, observed_sequence: headSequence.toString(),
      observed_at: "10", valid_through: "20", verification: "registry-receipt-and-current-head-verified",
    }, false));
    return;
  }
  assert(request.url === "/v1/programs/simulate", "unexpected Programs trust route");
  if (simulationMode === "expiry") trustNow = 21n;
  else {
    signalSimulation?.();
    await simulationRelease;
  }
  response.writeHead(200, { "Content-Type": "application/json" });
  response.end(programEnvelope({}, true));
});
trustServer.listen(0, "127.0.0.1");
await once(trustServer, "listening");
const trustAddress = trustServer.address();
assert(trustAddress !== null && typeof trustAddress === "object", "Programs trust listener missing");
try {
  const trustPrograms = new ProgramOperations(
    new ProductionClient(new AgentHttpTransport({ endpoint: `http://127.0.0.1:${trustAddress.port}` })),
    new ProgramTrustContext(Uint8Array.from({ length: 32 }, () => 0x44), () => trustNow, 5n),
  );
  await trustPrograms.discover(programId);
  headSequence = 9n;
  await rejectsWith(() => trustPrograms.discover(programId), "rollback or conflict", "Programs cache accepted a sequence rollback");
  headSequence = 10n;
  headRoot = "66".repeat(32);
  await rejectsWith(() => trustPrograms.discover(programId), "rollback or conflict", "Programs cache accepted a conflicting root");
  headRoot = "55".repeat(32);
  await rejectsWith(() => trustPrograms.simulate({
    programId, calldata: new Uint8Array([0xaa]), budget: { fuel: 1n, feeLimit: 0n }, capabilities: [], signedActivity,
  }), "stale program head", "Programs simulation accepted a head that expired in flight");

  trustNow = 15n;
  await trustPrograms.discover(programId);
  simulationMode = "race";
  simulationEntered = new Promise<void>((resolve) => { signalSimulation = resolve; });
  simulationRelease = new Promise<void>((resolve) => { releaseSimulation = resolve; });
  const pendingSimulation = trustPrograms.simulate({
    programId, calldata: new Uint8Array([0xaa]), budget: { fuel: 1n, feeLimit: 0n }, capabilities: [], signedActivity,
  });
  await simulationEntered;
  headSequence = 11n;
  headRoot = "77".repeat(32);
  await trustPrograms.discover(programId);
  releaseSimulation?.();
  await rejectsWith(() => pendingSimulation, "head changed during simulation", "Programs simulation accepted a superseded head");

  simulationEntered = new Promise<void>((resolve) => { signalSimulation = resolve; });
  simulationRelease = new Promise<void>((resolve) => { releaseSimulation = resolve; });
  const sameHeadSimulation = trustPrograms.simulate({
    programId, calldata: new Uint8Array([0xaa]), budget: { fuel: 1n, feeLimit: 0n }, capabilities: [], signedActivity,
  });
  await simulationEntered;
  await trustPrograms.discover(programId);
  releaseSimulation?.();
  await rejectsWith(() => sameHeadSimulation, "invalid program document fields", "Programs simulation treated an identical rediscovered head as changed");
} finally {
  trustServer.close();
  await once(trustServer, "close");
}

const graph = Buffer.from("LayerX/programs/call-graph/v1\0", "utf8");
const terminal = join(
  Buffer.from("LXP/program-execution/v4\0", "utf8"),
  integer(1n, 2), integer(1n, 4), integer(1n, 4), integer(0n, 8),
  integer(1n, 8), integer(2n, 8), integer(3n, 8), integer(4n, 8), integer(0n, 4), integer(0n, 8), integer(10n, 16),
  Buffer.from([0]), Buffer.from(programId, "hex"), integer(2n, 2), Buffer.from([0]), integer(0n, 4),
  sized64(Buffer.from([0xaa, 0xbb])), sized64(graph),
);
const graphRoot = await hash(graph);
const terminalRoot = await hash(terminal);
const receiptOutcome: ProgramReceiptOutcome = {
  encodingVersion: 3, terminalKind: 1, resultCode: 0, runtimeVersion: 1, abiVersion: 2,
  feeScheduleVersion: 1, meteringScheduleVersion: 1, cpuFuel: 1n, memoryBytes: 2n,
  storageReadBytes: 3n, storageWriteBytes: 4n, outputValues: 0, outputBytes: 0n,
  occupancyByteBatches: 0n, occupancyFeeUnits: 0n, feeSchedulePrices: [0n, 0n, 0n, 0n, 0n, 0n, 0n],
  occupancyAssetId: new Uint8Array(32), occupancyEvidenceDigest: new Uint8Array(32),
  occupancyTransferRoot: new Uint8Array(32), feeUnits: 10n, callGraphRoot: graphRoot,
  terminalPayloadRoot: terminalRoot, transferRoot: new Uint8Array(32),
};
const decodedTerminal = await decodeAndVerifyProgramTerminal(terminal, graph, programId, receiptOutcome, 1);
assert(decodedTerminal.outcome.kind === "completed" && decodedTerminal.outcome.response === "aabb", "canonical candidate terminal response was not bound");
assert(decodedTerminal.usage.fee_units === "10", "canonical candidate terminal usage was not bound");

const transferPrincipal = Buffer.from("22".repeat(32), "hex");
const transferAsset = Buffer.from("33".repeat(32), "hex");
const transferDestination = Buffer.from("44".repeat(32), "hex");
const transferAuthorization = join(
  Buffer.from("LayerX/programs/402LXP/transfer-set/v2\0", "utf8"), Buffer.from(programId, "hex"), transferPrincipal,
  Buffer.from("55".repeat(32), "hex"), Buffer.alloc(9),
  sized(Buffer.concat([Buffer.from("LayerX/programs/events/v1\0", "utf8"), Buffer.alloc(4)])),
  integer(0n, 8), integer(1n, 8), Buffer.alloc(9), Buffer.from([1]), transferPrincipal,
  transferAsset, transferDestination, integer(7n, 16), Buffer.from(programId, "hex"),
);
const transferRoot = await merkleTestRoot(join(Buffer.from([0]), transferPrincipal, transferDestination, transferAsset, integer(7n, 16), integer(1n, 2)));
const authorityTerminal = authorityWrapper(terminal, transferAuthorization, transferRoot);
const authorityReceipt = { ...receiptOutcome, terminalPayloadRoot: await hash(authorityTerminal), transferRoot };
await decodeAndVerifyProgramTerminal(authorityTerminal, graph, programId, authorityReceipt, 1);
const mutatedAuthorization = Uint8Array.from(transferAuthorization); const authorizationMutation = mutatedAuthorization.length - 65;
mutatedAuthorization[authorizationMutation] = (mutatedAuthorization[authorizationMutation] ?? 0) ^ 1;
await rejectsTerminal(authorityWrapper(terminal, mutatedAuthorization, transferRoot), graph, programId, authorityReceipt, 1, "mutated transfer authorization was accepted");
const mutatedAuthorityRoot = Uint8Array.from(transferRoot); mutatedAuthorityRoot[0] = (mutatedAuthorityRoot[0] ?? 0) ^ 1;
await rejectsTerminal(authorityWrapper(terminal, transferAuthorization, mutatedAuthorityRoot), graph, programId, authorityReceipt, 1, "mutated transfer wrapper root was accepted");

const occupancyAsset = Buffer.from("66".repeat(32), "hex");
const occupancyPayer = Buffer.from("77".repeat(32), "hex");
const occupancyNamespace = join(Buffer.from([65]), Buffer.from(programId, "hex"), Buffer.from([0]), occupancyPayer);
const occupancyEvidence = join(
  Buffer.from("LXP/storage-occupancy-settlement/v3\0", "utf8"), integer(2n, 8), integer(1n, 4),
  integer(0n, 8), integer(0n, 8), integer(0n, 8), integer(0n, 8), integer(0n, 8), integer(0n, 8), integer(2n, 8),
  integer(3n, 16), integer(6n, 16), integer(6n, 16), integer(0n, 16), integer(1n, 4),
  occupancyNamespace, occupancyPayer, Buffer.from(programId, "hex"), Buffer.from("88".repeat(32), "hex"),
  integer(1n, 8), integer(2n, 8), integer(3n, 8), integer(3n, 8), integer(3n, 16), integer(2n, 8),
  integer(6n, 16), integer(0n, 16), integer(6n, 16), integer(0n, 16), Buffer.from([1]), integer(0n, 16),
  integer(3n, 8), integer(2n, 8), integer(0n, 16), Buffer.from("99".repeat(32), "hex"),
);
const occupancyRoot = await occupancyTestRoot(occupancyPayer, occupancyAsset, 6n);
const occupancyTerminal = occupancyWrapper(terminal, occupancyEvidence);
const occupancyReceipt: ProgramReceiptOutcome = {
  ...receiptOutcome, terminalPayloadRoot: await hash(occupancyTerminal), occupancyByteBatches: 3n, occupancyFeeUnits: 6n,
  occupancyAssetId: occupancyAsset, occupancyEvidenceDigest: await hash(occupancyEvidence), occupancyTransferRoot: occupancyRoot,
};
await decodeAndVerifyProgramTerminal(occupancyTerminal, graph, programId, occupancyReceipt, 2);
await rejectsTerminal(occupancyTerminal, graph, programId, { ...occupancyReceipt, occupancyByteBatches: 4n }, 2, "mutated occupancy counter was accepted");
const mutatedEvidence = Uint8Array.from(occupancyEvidence);
const evidenceMutation = Buffer.from("LXP/storage-occupancy-settlement/v3\0", "utf8").length + 8 + 4 + (7 * 8) + 15;
mutatedEvidence[evidenceMutation] = (mutatedEvidence[evidenceMutation] ?? 0) ^ 1;
await rejectsTerminal(occupancyWrapper(terminal, mutatedEvidence), graph, programId,
  { ...occupancyReceipt, occupancyEvidenceDigest: await hash(mutatedEvidence) }, 2, "semantically mutated occupancy evidence was accepted");
const mutatedOccupancyRoot = Uint8Array.from(occupancyRoot); mutatedOccupancyRoot[0] = (mutatedOccupancyRoot[0] ?? 0) ^ 1;
await rejectsTerminal(occupancyTerminal, graph, programId, { ...occupancyReceipt, occupancyTransferRoot: mutatedOccupancyRoot }, 2, "mutated occupancy root was accepted");
const mutatedOccupancyAsset = Uint8Array.from(occupancyAsset); mutatedOccupancyAsset[0] = (mutatedOccupancyAsset[0] ?? 0) ^ 1;
await rejectsTerminal(occupancyTerminal, graph, programId, { ...occupancyReceipt, occupancyAssetId: mutatedOccupancyAsset }, 2, "mutated occupancy asset was accepted");

const zeroOccupancyEvidence = join(
  Buffer.from("LXP/storage-occupancy-settlement/v3\0", "utf8"), integer(2n, 8), integer(1n, 4),
  ...Array.from({ length: 7 }, () => integer(0n, 8)), ...Array.from({ length: 4 }, () => integer(0n, 16)), integer(0n, 4),
);
const zeroOccupancyTerminal = occupancyWrapper(terminal, zeroOccupancyEvidence);
await decodeAndVerifyProgramTerminal(zeroOccupancyTerminal, graph, programId, {
  ...receiptOutcome, terminalPayloadRoot: await hash(zeroOccupancyTerminal), occupancyAssetId: occupancyAsset,
  occupancyEvidenceDigest: await hash(zeroOccupancyEvidence), occupancyTransferRoot: new Uint8Array(32),
}, 2);
const emptyOccupancyTerminal = occupancyWrapper(terminal, new Uint8Array());
await decodeAndVerifyProgramTerminal(emptyOccupancyTerminal, graph, programId, {
  ...receiptOutcome, terminalPayloadRoot: await hash(emptyOccupancyTerminal),
}, 2);
const wrongWrapperOrder = occupancyWrapper(authorityWrapper(terminal, transferAuthorization, transferRoot), occupancyEvidence);
await rejectsTerminal(wrongWrapperOrder, graph, programId, { ...occupancyReceipt, transferRoot }, 2, "noncanonical attachment wrapper order was accepted");
await rejectsTerminal(authorityWrapper(authorityTerminal, transferAuthorization, transferRoot), graph, programId, authorityReceipt, 1,
  "duplicate transfer authority attachment was accepted");
await rejectsTerminal(occupancyWrapper(occupancyTerminal, occupancyEvidence), graph, programId, occupancyReceipt, 2,
  "duplicate occupancy attachment was accepted");

function programEnvelope(value: Readonly<Record<string, unknown>>, achieved: boolean): string {
  return JSON.stringify({
    request_id: "1",
    value,
    verification_status: achieved
      ? { state: "Achieved", level: "SequencerSigned" }
      : { state: "Unverified", requested: "SequencerSigned", achieved: "Unverified", reason: "server_side_receipt_verification_only" },
  });
}

async function canonicalProgramCall(callee: string, idempotency: string): Promise<Uint8Array> {
  const payload = join(
    Buffer.from("LayerX/programs/call/v1\0", "utf8"),
    Buffer.from(callee, "hex"),
    integer(1n, 8), integer(0n, 16), integer(0n, 2),
    sized(Buffer.from([0xaa])),
  );
  const payloadHash = await hash(join(Buffer.from("LXP/v1/payload-hash\0", "utf8"), payload));
  return join(
    integer(1n, 2), integer(0x1001n, 2), Buffer.from([12]),
    Buffer.from([1]), integer(1n, 2),
    Buffer.from([2]), integer(1n, 4),
    Buffer.from([3]), integer(0x0009_0003n, 4),
    Buffer.from([4]), sized(Buffer.from("did:lxp:test", "utf8")),
    Buffer.from([5]), sized(Buffer.from([1])),
    Buffer.from([6]), integer(0n, 8),
    Buffer.from([7]), integer(10n, 8), integer(20n, 8),
    Buffer.from([8]), sized(Buffer.from(idempotency, "hex")),
    Buffer.from([9]), integer(0n, 16),
    Buffer.from([10]), sized(payloadHash),
    Buffer.from([11]), sized(payload),
    Buffer.from([12]), sized(Buffer.from([2])),
  );
}

async function activityId(signed: Uint8Array): Promise<string> {
  return Buffer.from(await hash(join(Buffer.from("LXP/v1/activity-id\0", "utf8"), signed))).toString("hex");
}

async function hash(value: Uint8Array): Promise<Uint8Array> {
  const encoded = new ArrayBuffer(value.length);
  new Uint8Array(encoded).set(value);
  return new Uint8Array(await globalThis.crypto.subtle.digest("SHA-256", encoded));
}

function authorityWrapper(inner: Uint8Array, authorization: Uint8Array, root: Uint8Array): Uint8Array {
  return join(Buffer.from("LXP/program-execution-with-transfer-authority/v2\0", "utf8"), sized(inner), sized(authorization), root);
}
function occupancyWrapper(inner: Uint8Array, evidence: Uint8Array): Uint8Array {
  return join(Buffer.from("LXP/program-execution-with-occupancy/v1\0", "utf8"), sized(inner), sized(evidence));
}
async function merkleTestRoot(leg: Uint8Array): Promise<Uint8Array> {
  return hash(join(Buffer.from("LXP/v1/merkle-leaf\0", "utf8"), leg));
}
async function occupancyTestRoot(payer: Uint8Array, asset: Uint8Array, amount: bigint): Promise<Uint8Array> {
  const treasury = await hash(join(Buffer.from("LX:ACCOUNT:v1", "utf8"), integer(11n, 4), Buffer.from("system:fees", "utf8")));
  return merkleTestRoot(join(Buffer.from([0]), payer, treasury, asset, integer(amount, 16), integer(23n, 2)));
}
async function rejectsTerminal(payload: Uint8Array, availableGraph: Uint8Array, expectedProgram: string,
  receipt: ProgramReceiptOutcome, protocol: number, message: string): Promise<void> {
  let rejected = false;
  try { await decodeAndVerifyProgramTerminal(payload, availableGraph, expectedProgram, receipt, protocol); } catch { rejected = true; }
  assert(rejected, message);
}

function sized(value: Uint8Array): Uint8Array { return join(integer(BigInt(value.length), 4), value); }
function sized64(value: Uint8Array): Uint8Array { return join(integer(BigInt(value.length), 8), value); }
function join(...values: readonly Uint8Array[]): Uint8Array { return Buffer.concat(values); }
function integer(value: bigint, length: number): Uint8Array {
  const result = new Uint8Array(length);
  let remaining = value;
  for (let index = length - 1; index >= 0; index -= 1) { result[index] = Number(remaining & 0xffn); remaining >>= 8n; }
  return result;
}
