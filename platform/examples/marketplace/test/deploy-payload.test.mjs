import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { decodeNativeProgramDeploy } from "@sidiora/layerx-sdk";
import { canonicalNativeProgramDeploy } from "../deploy-payload.mjs";

const root = resolve(import.meta.dirname, "../../../..");
const wasm = Uint8Array.from(await readFile(resolve(root, "programs/fixtures/pay5/payments-merchant.wasm")));
const codeHash = createHash("sha256").update(wasm).digest("hex");
const programId = "3b7e1c40a9d25f8e6041bd9c73a5e8f210c4d6b98e07f35a1c2d4e6f80a9b3c5";

const deployment = canonicalNativeProgramDeploy({ programId, abiVersion: 2, codeHash, wasm });
const decoded = decodeNativeProgramDeploy(Uint8Array.from(deployment.payload));

assert.equal(Buffer.from(decoded.programId).toString("hex"), programId);
assert.equal(decoded.guestAbi, 2);
assert.equal(decoded.policy, 0);
assert.ok(Buffer.from(decoded.authority).equals(Buffer.alloc(32)));
assert.equal(Buffer.from(decoded.newHash).toString("hex"), codeHash);
assert.ok(Buffer.from(decoded.wasm).equals(Buffer.from(wasm)));
assert.equal(decoded.interface, undefined);
assert.equal(Buffer.from(decoded.programId).toString("hex"), Buffer.from(deployment.value.programId).toString("hex"));

assert.throws(
  () => canonicalNativeProgramDeploy({ programId, abiVersion: 2, codeHash: "00".repeat(32), wasm }),
  (error) => error.state === "refused" && error.message === "program_deployment_is_not_canonical",
);
assert.throws(
  () => canonicalNativeProgramDeploy({ programId, abiVersion: 4, codeHash, wasm }),
  (error) => error.state === "refused" && error.message === "program_build_reported_unsupported_guest_abi",
);
assert.throws(
  () => canonicalNativeProgramDeploy({ programId, abiVersion: 2, codeHash, wasm: wasm.slice(4) }),
  (error) => error.state === "refused" && error.message === "program_deployment_is_not_canonical",
);

process.stdout.write(`${JSON.stringify({
  test: "marketplace-deploy-payload",
  payloadBytes: deployment.payload.length,
  programId,
  codeHash,
})}\n`);
