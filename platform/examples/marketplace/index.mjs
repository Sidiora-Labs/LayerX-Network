import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { PlatformSdkError, verifyReceipt } from "@sidiora/layerx-sdk";
import {
  LayerXApplicationStateError,
  ReceiptAuthorityClient,
  exactObject,
  hex32,
  loadApplicationConfig,
  requiredEnvironment,
  secureBaseUrl,
} from "../support/runtime.mjs";

export function platform_ref_marketplace() {
  return "programs-shared-listing-receipt-settled-marketplace";
}

const config = await loadApplicationConfig(import.meta.url, "marketplace");
const cli = requiredEnvironment(config.cliBinaryEnvironment);
const token = requiredEnvironment(config.tokenEnvironment);
const authority = new ReceiptAuthorityClient(config.receiptAuthorityUrl, token);
const endpoint = secureBaseUrl(config.endpoint);

const runCli = (arguments_) => new Promise((resolvePromise, reject) => {
  const child = spawn(cli, ["--json", ...arguments_], {
    cwd: config.directory,
    stdio: ["pipe", "pipe", "pipe"],
    env: process.env,
  });
  const stdout = [];
  const stderr = [];
  child.stdout.on("data", (chunk) => stdout.push(chunk));
  child.stderr.on("data", (chunk) => stderr.push(chunk));
  child.once("error", () => reject(new LayerXApplicationStateError("unknown", "layerx_cli_unreachable")));
  child.once("close", (code, signal) => {
    const output = Buffer.concat(code === 0 ? stdout : stderr).toString("utf8").trim();
    let value;
    try {
      value = exactObject(JSON.parse(output));
    } catch {
      reject(new LayerXApplicationStateError("unknown", `layerx_cli_${signal ?? code ?? "failed"}`));
      return;
    }
    if (code !== 0 || value.ok !== true) {
      const detail = String(value.error?.detail ?? "layerx_cli_failed");
      reject(classifyCliFailure(detail));
      return;
    }
    resolvePromise(value);
  });
  child.stdin.end();
});

const classifyCliFailure = (detail) => {
  const match = /HTTP (\d{3})/u.exec(detail);
  const status = match === null ? undefined : Number(match[1]);
  if (status === 202 || status === 408 || status === 409 || status === 425) {
    return new LayerXApplicationStateError("pending", detail);
  }
  if (status === 400 || status === 401 || status === 403 || status === 410 || status === 422) {
    return new LayerXApplicationStateError("refused", detail);
  }
  return new LayerXApplicationStateError("unknown", detail);
};

const toHex = (bytes) => Buffer.from(bytes).toString("hex");

const capabilityTags = Object.freeze({
  "storage-read": 1,
  "storage-write": 2,
  transfer: 3,
  "emit-event": 4,
});

const u128 = (value) => {
  if (!/^(0|[1-9][0-9]{0,38})$/u.test(value)) throw new Error("invalid_marketplace_price");
  let number = BigInt(value);
  if (number <= 0n || number > 0xffffffffffffffffffffffffffffffffn) throw new Error("invalid_marketplace_price");
  const bytes = new Uint8Array(16);
  for (let index = 15; index >= 0; index -= 1) {
    bytes[index] = Number(number & 0xffn);
    number >>= 8n;
  }
  return bytes;
};

const idempotency = (suffix) => {
  const prefix = requiredEnvironment(config.idempotencyEnvironment);
  const value = `${prefix}-${suffix}`;
  if (!/^[A-Za-z0-9_-]{16,128}$/u.test(value)) throw new Error("invalid_marketplace_idempotency_key");
  return value;
};

const post = async (path, body, idempotencyKey) => {
  let response;
  try {
    response = await fetch(new URL(path, endpoint), {
      method: "POST",
      headers: {
        accept: "application/json",
        authorization: `Bearer ${token}`,
        "content-type": "application/json",
        "idempotency-key": idempotencyKey,
      },
      body: JSON.stringify(body),
    });
  } catch {
    throw new LayerXApplicationStateError("unknown", "program_gateway_unreachable");
  }
  const value = await response.json().catch(() => undefined);
  if (!response.ok) throw classifyHttpFailure(response.status);
  if (value === undefined) throw new LayerXApplicationStateError("unknown", "program_gateway_returned_non_json");
  return exactObject(value);
};

const classifyHttpFailure = (status) => {
  if (status === 202 || status === 408 || status === 409 || status === 425) {
    return new LayerXApplicationStateError("pending", `program_gateway_http_${status}`);
  }
  if (status === 400 || status === 401 || status === 403 || status === 410 || status === 422) {
    return new LayerXApplicationStateError("refused", `program_gateway_http_${status}`);
  }
  return new LayerXApplicationStateError("unknown", `program_gateway_http_${status}`);
};

const canonicalCallPayload = (programId, calldata, capabilities) => {
  const domain = Buffer.from("LayerX/programs/call/v1\0", "utf8");
  const fuel = Buffer.alloc(8);
  fuel.writeBigUInt64BE(1_000_000n);
  const feeLimit = Buffer.alloc(16);
  const tags = Buffer.from(capabilities.map((name) => capabilityTags[name]).sort((left, right) => left - right));
  const count = Buffer.alloc(2);
  count.writeUInt16BE(tags.length);
  const length = Buffer.alloc(4);
  length.writeUInt32BE(calldata.length);
  return Buffer.concat([domain, programId, fuel, feeLimit, count, tags, length, calldata]);
};

const canonicalReceipt = (value) => {
  const found = findReceipt(value);
  if (found === undefined) return undefined;
  if (/^(?:[0-9a-fA-F]{2})+$/u.test(found)) return Uint8Array.from(Buffer.from(found, "hex"));
  if (/^[A-Za-z0-9+/]+={0,2}$/u.test(found)) return Uint8Array.from(Buffer.from(found, "base64"));
  throw new LayerXApplicationStateError("unknown", "program_response_invalid_receipt");
};

const findReceipt = (value) => {
  if (value === null || typeof value !== "object") return undefined;
  if (!Array.isArray(value) && typeof value.receipt === "string") return value.receipt;
  for (const child of Object.values(value)) {
    const found = findReceipt(child);
    if (found !== undefined) return found;
  }
  return undefined;
};

const verifyOutcome = async (output) => {
  const receipt = canonicalReceipt(output);
  if (receipt === undefined) {
    const state = findState(output);
    if (state === "pending" || state === "unknown" || state === "refused") return { state, output };
    throw new LayerXApplicationStateError("unknown", "program_response_omitted_receipt");
  }
  const authorizedBatch = await authority.resolve(receipt);
  let verification;
  try {
    verification = await verifyReceipt(receipt, authorizedBatch);
  } catch (error) {
    if (error instanceof PlatformSdkError && error.code === "verification-failure") {
      throw new LayerXApplicationStateError("refused", "program_receipt_verification_failed");
    }
    throw error;
  }
  return {
    state: verification.receipt.resultCode < 0 ? "refused" : "completed",
    receiptDigest: toHex(verification.receiptDigest),
    verification: verification.level,
    resultCode: verification.receipt.resultCode,
    output,
  };
};

const findState = (value) => {
  if (value === null || typeof value !== "object") return undefined;
  if (!Array.isArray(value) && ["pending", "unknown", "refused"].includes(value.state)) return value.state;
  for (const child of Object.values(value)) {
    const found = findState(child);
    if (found !== undefined) return found;
  }
  return undefined;
};

const deploy = async () => {
  const manifest = resolve(config.directory, "program/Cargo.toml");
  const built = await runCli(["program", "build", "--manifest-path", manifest]);
  const artifact = built.data?.artifact;
  const codeHash = built.data?.code_hash;
  if (typeof artifact !== "string" || typeof codeHash !== "string" || !/^[0-9a-f]{64}$/u.test(codeHash)) {
    throw new LayerXApplicationStateError("unknown", "program_build_omitted_artifact_identity");
  }
  let wasm;
  try {
    wasm = await readFile(resolve(config.directory, artifact));
  } catch {
    throw new LayerXApplicationStateError("unknown", "program_build_artifact_unreadable");
  }
  const deployment = await post("/v1/programs/deploy", {
    abi_version: 1,
    code_hash: codeHash,
    wasm_hex: wasm.toString("hex"),
    upgrade_policy: { kind: "immutable" },
    source_uri: "platform/examples/marketplace/program",
  }, idempotency("deploy"));
  return verifyOutcome(deployment);
};

const call = async (action) => {
  const programId = Buffer.from(hex32(requiredEnvironment(config.programIdEnvironment)));
  const listing = hex32(requiredEnvironment(config.listingIdEnvironment));
  const calldata = Buffer.from(action === "list"
    ? Buffer.concat([
      Buffer.from([1]),
      Buffer.from(listing),
      Buffer.from(hex32(requiredEnvironment(config.assetEnvironment))),
      Buffer.from(hex32(requiredEnvironment(config.sellerEnvironment))),
      Buffer.from(u128(requiredEnvironment(config.priceEnvironment))),
    ])
    : Buffer.concat([
      Buffer.from([2]),
      Buffer.from(listing),
      Buffer.from(hex32(requiredEnvironment(config.receiptDigestEnvironment))),
    ]));
  const capabilities = action === "list"
    ? ["storage-read", "storage-write", "emit-event"]
    : ["storage-read", "storage-write", "transfer", "emit-event"];
  const canonicalPayload = canonicalCallPayload(programId, calldata, capabilities);
  return verifyOutcome(await post("/v1/programs/call", {
    program_id: toHex(programId),
    calldata: toHex(calldata),
    budget: { fuel: 1_000_000, fee_limit: "0" },
    capabilities,
    canonical_payload: toHex(canonicalPayload),
    contract_major: 1,
  }, idempotency(action)));
};

try {
  const result = config.action === "deploy" ? await deploy() : await call(config.action);
  process.stdout.write(`${JSON.stringify({ application: "marketplace", environment: config.name, action: config.action, ...result })}\n`);
  if (result.state !== "completed") process.exitCode = result.state === "pending" ? 2 : result.state === "unknown" ? 3 : 4;
} catch (error) {
  const stateError = error instanceof LayerXApplicationStateError
    ? error
    : error instanceof PlatformSdkError
      ? new LayerXApplicationStateError("unknown", error.code)
      : undefined;
  if (stateError === undefined) throw error;
  process.stdout.write(`${JSON.stringify({ application: "marketplace", environment: config.name, action: config.action, state: stateError.state, detail: stateError.message })}\n`);
  process.exitCode = stateError.state === "pending" ? 2 : stateError.state === "unknown" ? 3 : 4;
}
