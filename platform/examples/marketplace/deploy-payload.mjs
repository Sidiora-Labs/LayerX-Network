import { decodeNativeProgramDeploy, encodeNativeProgramDeploy } from "@sidiora/layerx-sdk";
import { LayerXApplicationStateError, exactObject, hex32, secureBaseUrl } from "../support/runtime.mjs";

export const IMMUTABLE_POLICY = 0;
export const LIFECYCLE_WINDOW_MS = 300_000n;

const MAX_U64 = 0xffffffffffffffffn;

export function canonicalNativeProgramDeploy({ programId, abiVersion, codeHash, wasm }) {
  if (![1, 2, 3].includes(abiVersion)) {
    throw new LayerXApplicationStateError("refused", "program_build_reported_unsupported_guest_abi");
  }
  const value = {
    programId: hex32(programId),
    guestAbi: abiVersion,
    policy: IMMUTABLE_POLICY,
    authority: new Uint8Array(32),
    newHash: hex32(codeHash),
    wasm: Uint8Array.from(wasm),
  };
  let payload;
  let decoded;
  try {
    payload = Uint8Array.from(encodeNativeProgramDeploy(value));
    decoded = decodeNativeProgramDeploy(payload);
  } catch {
    throw new LayerXApplicationStateError("refused", "program_deployment_is_not_canonical");
  }
  return Object.freeze({ payload, value: decoded });
}

export function accountNameForDid(did) {
  if (typeof did !== "string" || !/^did:[a-z0-9]+:[A-Za-z0-9._-]{1,128}$/u.test(did)) {
    throw new LayerXApplicationStateError("refused", "invalid_signing_key_did");
  }
  return `agent:${did}:main`;
}

export async function readLifecycleAnchor({ endpoint, token, did, fetchImplementation = fetch }) {
  const accountName = accountNameForDid(did);
  const state = await readState(endpoint, token, fetchImplementation);
  const accounts = state.accounts;
  if (!Array.isArray(accounts)) {
    throw new LayerXApplicationStateError("unknown", "program_state_omitted_accounts");
  }
  const account = accounts.find((entry) => exactObject(entry).name === accountName);
  if (account === undefined) {
    throw new LayerXApplicationStateError("refused", "program_state_omitted_signing_account");
  }
  const notBefore = unsigned64(state.timestamp_ms, "program_state_timestamp_ms");
  const expiresAt = notBefore + LIFECYCLE_WINDOW_MS;
  if (expiresAt > MAX_U64) {
    throw new LayerXApplicationStateError("unknown", "invalid_program_state_timestamp_ms");
  }
  return Object.freeze({
    accountSequence: unsigned64(account.next_sequence, "program_state_next_sequence"),
    notBefore,
    expiresAt,
    previousStateRoot: Buffer.from(hex32(state.canonical_state_root)).toString("hex"),
  });
}

export const LIST_OPERATION = 1;
export const BUY_OPERATION = 2;

export function marketplaceCallRequest({ action, listingId, asset, seller, price, receiptDigest }) {
  const listing = Buffer.from(hex32(listingId));
  if (action === "list") {
    return Object.freeze({
      calldata: Buffer.concat([
        Buffer.from([LIST_OPERATION]),
        listing,
        Buffer.from(hex32(asset)),
        Buffer.from(hex32(seller)),
        Buffer.from(u128(price)),
      ]),
      capabilities: Object.freeze(["storage-read", "storage-write", "emit-event"]),
    });
  }
  if (action !== "buy") throw new LayerXApplicationStateError("refused", "unsupported_marketplace_action");
  const digest = Buffer.from(hex32(receiptDigest)).toString("hex");
  u128(price);
  return Object.freeze({
    calldata: Buffer.concat([Buffer.from([BUY_OPERATION]), listing, Buffer.from(hex32(receiptDigest))]),
    capabilities: Object.freeze([
      "storage-read",
      "storage-write",
      "emit-event",
      `receipt-read:${digest}`,
      `transfer402:${Buffer.from(hex32(asset)).toString("hex")}:${Buffer.from(hex32(seller)).toString("hex")}:${price}`,
    ]),
  });
}

export function u128(value) {
  if (typeof value !== "string" || !/^(0|[1-9][0-9]{0,38})$/u.test(value)) {
    throw new LayerXApplicationStateError("refused", "invalid_marketplace_price");
  }
  let number = BigInt(value);
  if (number <= 0n || number > 0xffffffffffffffffffffffffffffffffn) {
    throw new LayerXApplicationStateError("refused", "invalid_marketplace_price");
  }
  const bytes = new Uint8Array(16);
  for (let index = 15; index >= 0; index -= 1) {
    bytes[index] = Number(number & 0xffn);
    number >>= 8n;
  }
  return bytes;
}

async function readState(endpoint, token, fetchImplementation) {
  let response;
  try {
    response = await fetchImplementation(new URL("v1/state", secureBaseUrl(endpoint)), {
      headers: { accept: "application/json", authorization: `Bearer ${token}` },
    });
  } catch {
    throw new LayerXApplicationStateError("unknown", "program_state_unreachable");
  }
  const body = await response.json().catch(() => undefined);
  if (!response.ok) throw stateHttpFailure(response.status);
  const envelope = exactObject(body);
  return exactObject(envelope.result ?? envelope);
}

function stateHttpFailure(status) {
  if (status === 408 || status === 409 || status === 425 || status === 503) {
    return new LayerXApplicationStateError("pending", `program_state_http_${status}`);
  }
  if (status === 400 || status === 401 || status === 403 || status === 404 || status === 410 || status === 422) {
    return new LayerXApplicationStateError("refused", `program_state_http_${status}`);
  }
  return new LayerXApplicationStateError("unknown", `program_state_http_${status}`);
}

function unsigned64(value, name) {
  const text = typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value.toString() : value;
  if (typeof text !== "string" || !/^(0|[1-9][0-9]{0,19})$/u.test(text)) {
    throw new LayerXApplicationStateError("unknown", `invalid_${name}`);
  }
  const parsed = BigInt(text);
  if (parsed > MAX_U64) throw new LayerXApplicationStateError("unknown", `invalid_${name}`);
  return parsed;
}
