import { readFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const LOOPBACK = new Set(["127.0.0.1", "localhost", "::1"]);

export class LayerXApplicationStateError extends Error {
  constructor(state, detail) {
    super(detail);
    this.name = "LayerXApplicationStateError";
    this.state = state;
  }
}

export const requiredEnvironment = (name) => {
  if (!/^[A-Z][A-Z0-9_]{0,127}$/u.test(name)) throw new Error("invalid_environment_binding");
  const value = process.env[name];
  if (value === undefined || value.length === 0) throw new Error(`missing_${name.toLowerCase()}`);
  return value;
};

export const optionalEnvironment = (name) => {
  if (name === undefined) return undefined;
  if (!/^[A-Z][A-Z0-9_]{0,127}$/u.test(name)) throw new Error("invalid_environment_binding");
  const value = process.env[name];
  return value === undefined || value.length === 0 ? undefined : value;
};

export async function loadApplicationConfig(moduleUrl, application) {
  const directory = dirname(fileURLToPath(moduleUrl));
  const document = exactObject(JSON.parse(await readFile(resolve(directory, "layerx.example.json"), "utf8")));
  if (document.version !== 1 || document.application !== application) throw new Error("invalid_application_config");
  const selected = parseArguments();
  const environments = exactObject(document.environments);
  const config = exactObject(environments[selected.environment]);
  return Object.freeze({ name: selected.environment, action: selected.action, directory, ...config });
}

export class ReceiptAuthorityClient {
  constructor(baseUrl, token) {
    this.baseUrl = secureBaseUrl(baseUrl);
    this.token = token;
  }

  async resolve(canonicalReceipt) {
    const activityId = receiptActivityId(canonicalReceipt);
    const evidence = await this.resolveReference(activityId);
    if (!equalBytes(evidence.canonicalReceipt, canonicalReceipt)) {
      throw new LayerXApplicationStateError("unknown", "receipt_authority_mismatch");
    }
    return evidence.authorizedBatch;
  }

  async resolveReference(reference) {
    if (!/^[A-Za-z0-9._:-]{1,256}$/u.test(reference)) {
      throw new LayerXApplicationStateError("refused", "invalid_receipt_reference");
    }
    const headers = new Headers({ accept: "application/json" });
    if (this.token !== undefined) headers.set("authorization", `Bearer ${this.token}`);
    let response;
    try {
      response = await fetch(new URL(`/v1/receipts/${encodeURIComponent(reference)}`, this.baseUrl), { headers });
    } catch {
      throw new LayerXApplicationStateError("unknown", "receipt_authority_unreachable");
    }
    const body = await response.json().catch(() => undefined);
    if (!response.ok) throw authorityHttpState(response.status);
    const envelope = exactObject(body);
    const result = exactObject(envelope.result ?? envelope);
    const canonicalReceipt = decodeReceipt(result.receipt);
    const authority = exactObject(result.authority);
    return {
      canonicalReceipt,
      authorizedBatch: {
        batchId: hex32(authority.batch_id),
        asset: hex32(authority.asset),
        previousStateRoot: hex32(authority.previous_state_root),
        resultingStateRoot: hex32(authority.resulting_state_root),
        sequencerPublicKey: hex32(authority.sequencer_public_key),
      },
    };
  }
}

export function exactObject(value) {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid_application_data");
  return value;
}

export function secureBaseUrl(value) {
  const url = secureUrl(value.endsWith("/") ? value : `${value}/`);
  return url;
}

export function secureUrl(value) {
  const url = new URL(value);
  if (url.username !== "" || url.password !== "" || url.hash !== "" || url.search !== "") {
    throw new Error("invalid_service_url");
  }
  if (url.protocol !== "https:" && !(url.protocol === "http:" && LOOPBACK.has(url.hostname))) {
    throw new Error("insecure_service_url");
  }
  return url;
}

export function hex32(value) {
  const digits = typeof value === "string" && value.startsWith("0x") ? value.slice(2) : value;
  if (typeof digits !== "string" || !/^[0-9a-fA-F]{64}$/u.test(digits)) throw new Error("invalid_receipt_authority");
  return Uint8Array.from({ length: 32 }, (_, index) => Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16));
}

export function equalBytes(left, right) {
  if (!(left instanceof Uint8Array) || !(right instanceof Uint8Array) || left.length !== right.length) return false;
  let difference = 0;
  for (let index = 0; index < left.length; index += 1) difference |= left[index] ^ right[index];
  return difference === 0;
}

function parseArguments() {
  const arguments_ = process.argv.slice(2);
  if ((arguments_.length !== 2 && arguments_.length !== 4) || arguments_[0] !== "--environment" || !["emulator", "testnet"].includes(arguments_[1])) {
    throw new Error("usage_--environment_emulator_or_testnet");
  }
  if (arguments_.length === 4 && (arguments_[2] !== "--action" || !["deploy", "list", "buy"].includes(arguments_[3]))) {
    throw new Error("usage_--action_deploy_list_or_buy");
  }
  return { environment: arguments_[1], action: arguments_[3] ?? "run" };
}

function receiptActivityId(receipt) {
  if (!(receipt instanceof Uint8Array) || receipt.length < 42) {
    throw new LayerXApplicationStateError("refused", "invalid_canonical_receipt");
  }
  const header = [0x00, 0x01, 0x52, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x20];
  for (let index = 0; index < header.length; index += 1) {
    if (receipt[index] !== header[index]) throw new LayerXApplicationStateError("refused", "invalid_canonical_receipt");
  }
  return Buffer.from(receipt.subarray(10, 42)).toString("hex");
}

function decodeReceipt(value) {
  if (typeof value !== "string") throw new LayerXApplicationStateError("unknown", "receipt_authority_omitted_receipt");
  if (/^(?:[0-9a-fA-F]{2})+$/u.test(value)) return Uint8Array.from(Buffer.from(value, "hex"));
  if (/^[A-Za-z0-9+/]+={0,2}$/u.test(value)) return Uint8Array.from(Buffer.from(value, "base64"));
  throw new LayerXApplicationStateError("unknown", "receipt_authority_returned_invalid_receipt");
}

function authorityHttpState(status) {
  if (status === 404 || status === 408 || status === 409 || status === 425) {
    return new LayerXApplicationStateError("pending", `receipt_authority_http_${status}`);
  }
  if (status === 400 || status === 401 || status === 403 || status === 410 || status === 422) {
    return new LayerXApplicationStateError("refused", `receipt_authority_http_${status}`);
  }
  return new LayerXApplicationStateError("unknown", `receipt_authority_http_${status}`);
}
