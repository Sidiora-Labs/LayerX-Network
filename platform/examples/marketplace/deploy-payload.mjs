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

export const IDENTITY_SEQUENCE_SELECTOR = "identity";

export async function readLifecycleAnchor({ endpoint, token, did, fetchImplementation = fetch }) {
  accountNameForDid(did);
  const state = await readJson(endpoint, "v1/state", token, fetchImplementation, "program_state");
  const identitySequence = await readIdentitySequence({
    endpoint,
    token,
    did,
    networkMode: state.network_mode,
    fetchImplementation,
  });
  const notBefore = unsigned64(state.timestamp_ms, "program_state_timestamp_ms");
  const expiresAt = notBefore + LIFECYCLE_WINDOW_MS;
  if (expiresAt > MAX_U64) {
    throw new LayerXApplicationStateError("unknown", "invalid_program_state_timestamp_ms");
  }
  return Object.freeze({
    identitySequence,
    notBefore,
    expiresAt,
    previousStateRoot: Buffer.from(hex32(state.canonical_state_root)).toString("hex"),
  });
}

async function readIdentitySequence({ endpoint, token, did, networkMode, fetchImplementation }) {
  if (networkMode === "emulator") {
    const snapshot = await readJson(
      endpoint,
      `v1/dids/${did}/sequence`,
      token,
      fetchImplementation,
      "identity_sequence",
    );
    return identityNextSequence(snapshot, did);
  }
  if (networkMode !== "hosted") {
    throw new LayerXApplicationStateError("unknown", "program_state_omitted_network_mode");
  }
  const snapshot = await callRpc(
    endpoint,
    token,
    "lx_getSequence",
    [did, IDENTITY_SEQUENCE_SELECTOR],
    fetchImplementation,
  );
  if (snapshot.verification !== "authenticated_node_snapshot") {
    throw new LayerXApplicationStateError("unknown", "identity_sequence_snapshot_unauthenticated");
  }
  return identityNextSequence(snapshot, did);
}

function identityNextSequence(snapshot, did) {
  if (snapshot.did !== did) {
    throw new LayerXApplicationStateError("unknown", "identity_sequence_named_another_did");
  }
  return unsigned64(snapshot.next_sequence, "identity_next_sequence");
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

async function readJson(endpoint, path, token, fetchImplementation, source) {
  let response;
  try {
    response = await fetchImplementation(new URL(path, secureBaseUrl(endpoint)), {
      headers: { accept: "application/json", authorization: `Bearer ${token}` },
    });
  } catch {
    throw new LayerXApplicationStateError("unknown", `${source}_unreachable`);
  }
  const body = await response.json().catch(() => undefined);
  if (!response.ok) throw httpFailure(response.status, source);
  const envelope = exactObject(body);
  return exactObject(envelope.result ?? envelope);
}

async function callRpc(endpoint, token, method, params, fetchImplementation) {
  let response;
  try {
    response = await fetchImplementation(new URL("rpc", secureBaseUrl(endpoint)), {
      method: "POST",
      headers: {
        accept: "application/json",
        "content-type": "application/json",
        authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
    });
  } catch {
    throw new LayerXApplicationStateError("unknown", "identity_sequence_unreachable");
  }
  const body = await response.json().catch(() => undefined);
  if (!response.ok) throw httpFailure(response.status, "identity_sequence");
  const envelope = exactObject(body);
  if (envelope.jsonrpc !== "2.0" || envelope.result === undefined) {
    throw new LayerXApplicationStateError("unknown", "identity_sequence_rpc_refused");
  }
  return exactObject(envelope.result);
}

function httpFailure(status, source) {
  if (status === 408 || status === 409 || status === 425 || status === 429 || status === 503) {
    return new LayerXApplicationStateError("pending", `${source}_http_${status}`);
  }
  if (status === 400 || status === 401 || status === 403 || status === 404 || status === 410 || status === 422) {
    return new LayerXApplicationStateError("refused", `${source}_http_${status}`);
  }
  return new LayerXApplicationStateError("unknown", `${source}_http_${status}`);
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
