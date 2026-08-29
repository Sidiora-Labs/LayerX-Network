import { once } from "node:events";
import * as http from "node:http";

import {
  AgentHttpTransport,
  LayerXKeyCredential,
  ProductionClient,
  SecretBytes,
} from "../src/index.js";

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
      verification_status: { state: "Achieved", level: "SequencerSigned" },
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
