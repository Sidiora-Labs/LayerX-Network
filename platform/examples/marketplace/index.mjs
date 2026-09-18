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
} from "../support/runtime.mjs";
import {
  accountNameForDid,
  canonicalNativeProgramDeploy,
  marketplaceCallRequest,
  readLifecycleAnchor,
} from "./deploy-payload.mjs";

export function platform_ref_marketplace() {
  return "programs-shared-listing-receipt-settled-marketplace";
}

const config = await loadApplicationConfig(import.meta.url, "marketplace");
const cli = requiredEnvironment(config.cliBinaryEnvironment);
const token = requiredEnvironment(config.tokenEnvironment);
const authority = new ReceiptAuthorityClient(config.receiptAuthorityUrl, token);

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

const CALL_FUEL = 1_000_000;

const idempotency = (suffix) => {
  const prefix = requiredEnvironment(config.idempotencyEnvironment);
  const value = `${prefix}-${suffix}`;
  if (!/^[A-Za-z0-9_-]{16,128}$/u.test(value)) throw new Error("invalid_marketplace_idempotency_key");
  return value;
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

const signingKeyDid = async () => {
  const listed = await runCli(["key", "list"]);
  const keys = Array.isArray(listed.data) ? listed.data : undefined;
  if (keys === undefined) throw new LayerXApplicationStateError("unknown", "layerx_cli_omitted_key_list");
  const selected = keys.find((entry) => exactObject(entry).default === true) ?? keys[0];
  if (selected === undefined) throw new LayerXApplicationStateError("refused", "layerx_cli_has_no_signing_key");
  const did = exactObject(selected).did;
  accountNameForDid(did);
  return did;
};

const anchor = async () => readLifecycleAnchor({
  endpoint: config.endpoint,
  token,
  did: await signingKeyDid(),
});

const deploy = async () => {
  const manifest = resolve(config.directory, "program/Cargo.toml");
  const built = await runCli(["program", "build", "--manifest-path", manifest]);
  const artifact = built.data?.artifact;
  const codeHash = built.data?.code_hash;
  const abiVersion = built.data?.abi_version;
  if (typeof artifact !== "string" || typeof codeHash !== "string" || !/^[0-9a-f]{64}$/u.test(codeHash)
    || !Number.isSafeInteger(abiVersion)) {
    throw new LayerXApplicationStateError("unknown", "program_build_omitted_artifact_identity");
  }
  const artifactPath = resolve(config.directory, artifact);
  let wasm;
  try {
    wasm = await readFile(artifactPath);
  } catch {
    throw new LayerXApplicationStateError("unknown", "program_build_artifact_unreadable");
  }
  const deployment = canonicalNativeProgramDeploy({
    programId: requiredEnvironment(config.programIdEnvironment),
    abiVersion,
    codeHash,
    wasm,
  });
  const anchored = await anchor();
  return verifyOutcome(await runCli([
    "program",
    "deploy",
    artifactPath,
    "--program-id",
    toHex(deployment.value.programId),
    "--idempotency-key",
    idempotency("deploy"),
    "--account-sequence",
    anchored.accountSequence.toString(),
    "--not-before-ms",
    anchored.notBefore.toString(),
    "--expires-at-ms",
    anchored.expiresAt.toString(),
    "--previous-state-root",
    anchored.previousStateRoot,
  ]));
};

const call = async (action) => {
  const programId = toHex(hex32(requiredEnvironment(config.programIdEnvironment)));
  const request = marketplaceCallRequest({
    action,
    listingId: requiredEnvironment(config.listingIdEnvironment),
    asset: requiredEnvironment(config.assetEnvironment),
    seller: requiredEnvironment(config.sellerEnvironment),
    price: requiredEnvironment(config.priceEnvironment),
    receiptDigest: action === "buy" ? requiredEnvironment(config.receiptDigestEnvironment) : undefined,
  });
  const discovered = await runCli(["program", "discover", programId]);
  const abiVersion = discovered.data?.abi_version;
  if (!Number.isSafeInteger(abiVersion)) {
    throw new LayerXApplicationStateError("unknown", "program_discovery_omitted_abi_version");
  }
  const anchored = await anchor();
  return verifyOutcome(await runCli([
    "program",
    "call",
    programId,
    "--abi-version",
    abiVersion.toString(),
    "--calldata",
    toHex(request.calldata),
    "--fuel",
    CALL_FUEL.toString(),
    "--fee-limit",
    "0",
    ...request.capabilities.flatMap((capability) => ["--capability", capability]),
    "--idempotency-key",
    idempotency(action),
    "--account-sequence",
    anchored.accountSequence.toString(),
    "--not-before-ms",
    anchored.notBefore.toString(),
    "--expires-at-ms",
    anchored.expiresAt.toString(),
  ]));
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
