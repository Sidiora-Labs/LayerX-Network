import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { decodeNativeProgramDeploy } from "@sidiora/layerx-sdk";
import {
  BUY_OPERATION,
  LIFECYCLE_WINDOW_MS,
  LIST_OPERATION,
  accountNameForDid,
  canonicalNativeProgramDeploy,
  marketplaceCallRequest,
  readLifecycleAnchor,
} from "../deploy-payload.mjs";

const root = resolve(import.meta.dirname, "../../../..");
const wasm = Uint8Array.from(await readFile(resolve(root, "programs/fixtures/pay5/payments-merchant.wasm")));
const codeHash = createHash("sha256").update(wasm).digest("hex");
const programId = "3b7e1c40a9d25f8e6041bd9c73a5e8f210c4d6b98e07f35a1c2d4e6f80a9b3c5";
const listingId = "c41d8a2f60e9b7530d1a8c4f92b6e70a35d8c1f4a6b90e23d7c58f1a4b6d9e02";
const asset = "0100000000000000000000000000000000000000000000000000000000000000";
const seller = "7f2a91c604db85e3a0c7f16d928e40b5c73a61f8d20e94b7c5a3106df82b49ec";
const receiptDigest = "a5c3901e7bd426f80c19a3f56d82b4e70931cf6a8d25e40b7c1936af52d8e0b4";
const stateRoot = "1f6b04c982a37e5d0b48f1c62a9d370e85b4c1f6a2d83e07b95c4f1a6d20e38b";
const did = "did:layerx:marketplace-reference";

const deployment = canonicalNativeProgramDeploy({ programId, abiVersion: 2, codeHash, wasm });
const decoded = decodeNativeProgramDeploy(Uint8Array.from(deployment.payload));

assert.equal(Buffer.from(decoded.programId).toString("hex"), programId);
assert.equal(decoded.guestAbi, 2);
assert.equal(decoded.policy, 0);
assert.ok(Buffer.from(decoded.authority).equals(Buffer.alloc(32)));
assert.equal(Buffer.from(decoded.newHash).toString("hex"), codeHash);
assert.ok(Buffer.from(decoded.wasm).equals(Buffer.from(wasm)));
assert.equal(decoded.interface, undefined);
assert.equal(Buffer.from(deployment.value.programId).toString("hex"), programId);

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

const listRequest = marketplaceCallRequest({ action: "list", listingId, asset, seller, price: "2500" });
assert.equal(listRequest.calldata.length, 1 + 32 + 32 + 32 + 16);
assert.equal(listRequest.calldata[0], LIST_OPERATION);
assert.equal(listRequest.calldata.subarray(1, 33).toString("hex"), listingId);
assert.equal(listRequest.calldata.subarray(33, 65).toString("hex"), asset);
assert.equal(listRequest.calldata.subarray(65, 97).toString("hex"), seller);
assert.equal(listRequest.calldata.subarray(97).readBigUInt64BE(8), 2500n);
assert.deepEqual([...listRequest.capabilities], ["storage-read", "storage-write", "emit-event"]);

const buyRequest = marketplaceCallRequest({ action: "buy", listingId, asset, seller, price: "2500", receiptDigest });
assert.equal(buyRequest.calldata.length, 1 + 32 + 32);
assert.equal(buyRequest.calldata[0], BUY_OPERATION);
assert.equal(buyRequest.calldata.subarray(1, 33).toString("hex"), listingId);
assert.equal(buyRequest.calldata.subarray(33).toString("hex"), receiptDigest);
assert.deepEqual([...buyRequest.capabilities], [
  "storage-read",
  "storage-write",
  "emit-event",
  `receipt-read:${receiptDigest}`,
  `transfer402:${asset}:${seller}:2500`,
]);

assert.throws(
  () => marketplaceCallRequest({ action: "list", listingId, asset, seller, price: "0" }),
  (error) => error.state === "refused" && error.message === "invalid_marketplace_price",
);
assert.throws(
  () => marketplaceCallRequest({ action: "browse", listingId, asset, seller, price: "1" }),
  (error) => error.state === "refused" && error.message === "unsupported_marketplace_action",
);

assert.equal(accountNameForDid(did), `agent:${did}:main`);
assert.throws(
  () => accountNameForDid("marketplace-reference"),
  (error) => error.state === "refused" && error.message === "invalid_signing_key_did",
);

const stateBody = {
  ok: true,
  result: {
    network_mode: "emulator",
    batch_cadence: "instant",
    state_root: stateRoot,
    canonical_state_root: stateRoot,
    receipt_state_root: "83b1d07a4c96e2f5108b3d6a97c40e2158fb7d3c609a2e4b81d75c3f0a6e9b12",
    next_sequence: 41,
    batch_number: 7,
    timestamp_ms: 1_700_000_000_000,
    cells: [{ key: "00".repeat(32), value_hi: 0, value_lo: 12 }],
    accounts: [
      { id: "11".repeat(32), name: "agent:did:layerx:other:main", balance_hi: 0, balance_lo: 5, next_sequence: 2 },
      { id: "22".repeat(32), name: `agent:${did}:main`, balance_hi: 0, balance_lo: 900, next_sequence: 19 },
    ],
  },
  trace: "emu-0000000000000001",
};

const requests = [];
const state = createServer((request, response) => {
  requests.push({ method: request.method, url: request.url, authorization: request.headers.authorization });
  if (request.url !== "/v1/state") {
    response.writeHead(404, { "content-type": "application/json" }).end(JSON.stringify({ ok: false }));
    return;
  }
  response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(stateBody));
});
await new Promise((ready) => state.listen(0, "127.0.0.1", ready));

try {
  const endpoint = `http://127.0.0.1:${state.address().port}`;
  const anchor = await readLifecycleAnchor({ endpoint, token: "reference-token", did });
  assert.deepEqual(requests, [{ method: "GET", url: "/v1/state", authorization: "Bearer reference-token" }]);
  assert.equal(anchor.accountSequence, 19n);
  assert.equal(anchor.notBefore, 1_700_000_000_000n);
  assert.equal(anchor.expiresAt, 1_700_000_000_000n + LIFECYCLE_WINDOW_MS);
  assert.equal(anchor.previousStateRoot, stateRoot);

  await assert.rejects(
    readLifecycleAnchor({ endpoint, token: "reference-token", did: "did:layerx:absent" }),
    (error) => error.state === "refused" && error.message === "program_state_omitted_signing_account",
  );
} finally {
  await new Promise((closed) => state.close(closed));
}

const hostedState = {
  ok: true,
  result: {
    network_mode: "hosted",
    canonical_state_root: stateRoot,
    state_root: stateRoot,
    receipt_state_root: "83b1d07a4c96e2f5108b3d6a97c40e2158fb7d3c609a2e4b81d75c3f0a6e9b12",
    receipt_digest: "2c7e5a10b94df386021c7e5b8a43d6f019bc5e27a806d14f93b2c6e05a1d7f48",
    batch_number: 8_412,
    observed_sequence: 5_190_744,
    timestamp_ms: 1_700_000_500_000,
    verification: "sequencer-signed-batch-header-and-receipt-inclusion",
  },
  trace: "gw-0000000000000002",
};

const hostedAccounts = {
  ok: true,
  result: {
    did,
    accounts: [
      {
        account_id: "33".repeat(32),
        name: `agent:${did}:budget:daily`,
        asset_id: asset,
        balance: "120",
        next_sequence: "4",
        frozen: false,
        canonical_value: "44".repeat(16),
        proof_material: "55".repeat(16),
        observed_head_sequence: "5190744",
        batch_number: "8412",
        verification: "settlement_anchored",
      },
      {
        account_id: "66".repeat(32),
        name: `agent:${did}:main`,
        asset_id: asset,
        balance: "7400",
        next_sequence: "57",
        frozen: false,
        canonical_value: "77".repeat(16),
        proof_material: "88".repeat(16),
        observed_head_sequence: "5190744",
        batch_number: "8412",
        verification: "settlement_anchored",
      },
    ],
    verification: "settlement_anchored",
  },
  trace: "gw-0000000000000003",
};

const hostedRequests = [];
const hosted = createServer((request, response) => {
  hostedRequests.push({ method: request.method, url: request.url, authorization: request.headers.authorization });
  const listing = request.url === `/v1/dids/${did}/accounts`;
  if (request.url !== "/v1/state" && !listing) {
    response.writeHead(404, { "content-type": "application/json" }).end(JSON.stringify({ ok: false }));
    return;
  }
  response.writeHead(200, { "content-type": "application/json" })
    .end(JSON.stringify(listing ? hostedAccounts : hostedState));
});
await new Promise((ready) => hosted.listen(0, "127.0.0.1", ready));

try {
  const endpoint = `http://127.0.0.1:${hosted.address().port}`;
  const anchor = await readLifecycleAnchor({ endpoint, token: "hosted-token", did });
  assert.deepEqual(hostedRequests, [
    { method: "GET", url: "/v1/state", authorization: "Bearer hosted-token" },
    { method: "GET", url: `/v1/dids/${did}/accounts`, authorization: "Bearer hosted-token" },
  ]);
  assert.equal(anchor.accountSequence, 57n);
  assert.equal(anchor.notBefore, 1_700_000_500_000n);
  assert.equal(anchor.expiresAt, 1_700_000_500_000n + LIFECYCLE_WINDOW_MS);
  assert.equal(anchor.previousStateRoot, stateRoot);

  await assert.rejects(
    readLifecycleAnchor({ endpoint, token: "hosted-token", did: "did:layerx:absent" }),
    (error) => error.state === "refused" && error.message === "did_accounts_http_404",
  );
} finally {
  await new Promise((closed) => hosted.close(closed));
}

const withoutMain = createServer((request, response) => {
  const body = request.url === "/v1/state"
    ? hostedState
    : { ...hostedAccounts, result: { did, accounts: [hostedAccounts.result.accounts[0]], verification: "settlement_anchored" } };
  response.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(body));
});
await new Promise((ready) => withoutMain.listen(0, "127.0.0.1", ready));
try {
  await assert.rejects(
    readLifecycleAnchor({ endpoint: `http://127.0.0.1:${withoutMain.address().port}`, token: "hosted-token", did }),
    (error) => error.state === "refused" && error.message === "did_accounts_omitted_signing_account",
  );
} finally {
  await new Promise((closed) => withoutMain.close(closed));
}

const refusing = createServer((request, response) => {
  response.writeHead(503, { "content-type": "application/json" })
    .end(JSON.stringify({ ok: false, error: { code: "principal_state_proof_unavailable", retry: "never" } }));
});
await new Promise((ready) => refusing.listen(0, "127.0.0.1", ready));
try {
  await assert.rejects(
    readLifecycleAnchor({ endpoint: `http://127.0.0.1:${refusing.address().port}`, token: "reference-token", did }),
    (error) => error.state === "pending" && error.message === "program_state_http_503",
  );
} finally {
  await new Promise((closed) => refusing.close(closed));
}

process.stdout.write(`${JSON.stringify({
  test: "marketplace-deploy-payload",
  payloadBytes: deployment.payload.length,
  listCalldataBytes: listRequest.calldata.length,
  buyCalldataBytes: buyRequest.calldata.length,
  stateRequests: requests.length,
  hostedRequests: hostedRequests.length,
})}\n`);
