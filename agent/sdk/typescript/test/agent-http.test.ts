import { once } from "node:events";
import * as http from "node:http";

import {
  AgentHttpTransport,
  idempotencyKey,
  LayerXKeyCredential,
  PlatformSdkError,
  ProgramOperations,
  ProgramTrustContext,
  ProductionClient,
  SecretBytes,
  type ProductionTransport,
} from "../src/index.js";
import { decodeAndVerifyProgramTerminal } from "../src/program-wire.js";
import type { ProgramReceiptOutcome } from "../src/verifier.js";

function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
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
      request_id: "request-1",
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
const ambiguous: ProductionTransport = {
  async call<TRequest, TResponse>(): Promise<TResponse> {
    throw new PlatformSdkError({ code: "decode-failure", retry: "never" });
  },
};
const programs = new ProgramOperations(
  new ProductionClient(ambiguous),
  new ProgramTrustContext(Uint8Array.from({ length: 32 }, () => 0x44), () => 1n),
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
    Buffer.from([7]), integer(0n, 8), integer(1n, 8),
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

function sized(value: Uint8Array): Uint8Array { return join(integer(BigInt(value.length), 4), value); }
function sized64(value: Uint8Array): Uint8Array { return join(integer(BigInt(value.length), 8), value); }
function join(...values: readonly Uint8Array[]): Uint8Array { return Buffer.concat(values); }
function integer(value: bigint, length: number): Uint8Array {
  const result = new Uint8Array(length);
  let remaining = value;
  for (let index = length - 1; index >= 0; index -= 1) { result[index] = Number(remaining & 0xffn); remaining >>= 8n; }
  return result;
}
