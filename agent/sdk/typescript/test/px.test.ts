import assert from "node:assert/strict";
import { once } from "node:events";
import * as http from "node:http";

import {
  JsonRpcError,
  PxClient,
  PX_MAXIMUM_JOINED_ASSETS,
  decodePxAccountBalances,
  decodePxAccountJoin,
  decodePxAssetTable,
  decodePxNetworkHead,
  decodePxResolvedIdentities,
  pxAccountKey,
  pxOptionalQuantity,
  pxQuantity,
} from "../src/index.js";

const ACCOUNT = "bb".repeat(32);
const DID = `did:layerx:${"aa".repeat(32)}`;
const EVM = `0x${"11".repeat(20)}`;
const NATIVE = "cc".repeat(32);
const UNJOINED = "dd".repeat(32);

assert.equal(pxAccountKey(EVM.toUpperCase()), EVM);
assert.equal(pxAccountKey(` ${DID.toUpperCase()} `), DID);
assert.equal(pxAccountKey(ACCOUNT.toUpperCase()), ACCOUNT);
for (const refused of ["", "0x", `0x${"11".repeat(19)}`, "did:layerx:zz", `did:paxeer:${"aa".repeat(32)}`, "bb".repeat(31), "agent:did:layerx:alice:main"]) {
  assert.throws(() => pxAccountKey(refused));
}

assert.equal(pxQuantity("500000"), 500000n);
assert.equal(pxQuantity("0x1e"), 30n);
assert.equal(pxQuantity("0X1E"), 30n);
assert.equal(pxQuantity(0), 0n);
assert.equal(pxQuantity(16), 16n);
assert.equal(pxQuantity("340282366920938463463374607431768211455"), 340282366920938463463374607431768211455n);
for (const refused of ["", "0x", "-1", "1.5", "007", "0xgg", `0x${"f".repeat(33)}`, "340282366920938463463374607431768211456", 1.5, -1, null, true, {}]) {
  assert.throws(() => pxQuantity(refused));
}
assert.equal(pxOptionalQuantity(null), null);
assert.equal(pxOptionalQuantity("0x00"), 0n);

const identities = {
  evm_address: EVM,
  pax_address: "pax1qq7p8m4nfz0k7h8s2v9d3l6c5x4b3n2m1q0w9e",
  layerx_did: DID,
  layerx_account: ACCOUNT,
  bound: true,
};
const custody = {
  asset_id: NATIVE,
  denom: "ulxp",
  pointer: `0x${"22".repeat(20)}`,
  enabled: true,
  paused: false,
  minimum_deposit: "1000",
  custody_cap: "100000000000",
  custodied: "500000",
  released: "0",
  pending: "0",
};
const balances = {
  account: identities,
  balances: [
    { asset_id: NATIVE, denom: "ulxp", custody, paxeer: { denom: "ulxp", amount: "0x1e" }, layerx: { balance: "12" } },
    { asset_id: UNJOINED, denom: null, custody: null, paxeer: null, layerx: null },
  ],
  joined_limit: 16,
};

const decoded = decodePxAccountBalances(balances);
assert.equal(decoded.account.evmAddress, EVM);
assert.equal(decoded.account.layerxDid, DID);
assert.equal(decoded.account.layerxAccount, ACCOUNT);
assert.equal(decoded.account.bound, true);
assert.equal(decoded.joinedLimit, 16n);
assert.equal(decoded.balances.length, 2);

const joined = decoded.balances[0]!;
assert.equal(joined.assetId, NATIVE);
assert.equal(joined.denom, "ulxp");
assert.equal(joined.paxeer?.amount, 30n);
assert.equal(joined.paxeer?.denom, "ulxp");
assert.equal(joined.custody?.custodied, 500000n);
assert.equal(joined.custody?.pending, 0n);
assert.equal(joined.custody?.enabled, true);
assert.equal(joined.custody?.paused, false);
assert.deepEqual(joined.layerx, { balance: "12" });

const unknownRow = decoded.balances[1]!;
assert.equal(unknownRow.denom, null);
assert.notEqual(unknownRow.denom, "");
assert.equal(unknownRow.custody, null);
assert.equal(unknownRow.paxeer, null);
assert.notEqual(unknownRow.paxeer, 0n);
assert.equal(unknownRow.layerx, null);
assert.notEqual(unknownRow.layerx, 0n);

assert.throws(() => decodePxAccountBalances({ ...balances, balances: [{ asset_id: NATIVE, denom: null, custody: null, paxeer: null }] }));
assert.throws(() => decodePxAccountBalances({ ...balances, joined_limit: null }));
assert.throws(() => decodePxAccountBalances({ account: identities, joined_limit: 16 }));
assert.throws(() => decodePxResolvedIdentities({ ...identities, bound: "true" }));
assert.throws(() => decodePxResolvedIdentities({ ...identities, evm_address: `0x${"11".repeat(32)}` }));
assert.equal(decodePxResolvedIdentities({ ...identities, evm_address: null, layerx_account: null, bound: false }).evmAddress, null);
assert.equal(PX_MAXIMUM_JOINED_ASSETS, 1024);
assert.throws(() => decodePxAccountBalances({ ...balances, balances: new Array<unknown>(PX_MAXIMUM_JOINED_ASSETS + 1).fill(balances.balances[1]) }));

const join = decodePxAccountJoin({
  account: identities,
  paxeer: { address: EVM, balance: "0x0de0b6b3a7640000", nonce: "0x07" },
  layerx: { sequence: "3" },
});
assert.equal(join.paxeer?.balance, 1000000000000000000n);
assert.equal(join.paxeer?.nonce, 7n);
assert.deepEqual(join.layerx, { sequence: "3" });
const halfJoin = decodePxAccountJoin({ account: identities, paxeer: null, layerx: null });
assert.equal(halfJoin.paxeer, null);
assert.equal(halfJoin.layerx, null);

const assets = decodePxAssetTable({
  assets: [
    { asset_id: NATIVE, layerx: { symbol: "LXP" }, paxeer: custody },
    { asset_id: UNJOINED, layerx: null, paxeer: null },
  ],
  joined_limit: 16,
});
assert.equal(assets.joinedLimit, 16n);
assert.equal(assets.assets[0]!.paxeer?.denom, "ulxp");
assert.equal(assets.assets[1]!.paxeer, null);
assert.equal(assets.assets[1]!.layerx, null);

const network = decodePxNetworkHead({
  network_id: "layerx-beta",
  paxeer: { chain_id: "0x1a4", latest_block: "0x2b67" },
  layerx: { node_info: { network: "layerx-beta" } },
  anchor: { latest_finalized_batch: 41, status: 2, status_name: "final", status_ladder: { "0": "unknown", "1": "submitted", "2": "final" } },
});
assert.equal(network.networkId, "layerx-beta");
assert.equal(network.paxeer.chainId, 420n);
assert.equal(network.paxeer.latestBlock, 11111n);
assert.deepEqual(network.layerx, { node_info: { network: "layerx-beta" } });
assert.equal(network.anchor?.latestFinalizedBatch, 41n);
assert.equal(network.anchor?.status, 2n);
assert.equal(network.anchor?.statusName, "final");
assert.equal(network.anchor?.statusLadder?.["1"], "submitted");

const silentNetwork = decodePxNetworkHead({
  network_id: "layerx-beta",
  paxeer: { chain_id: "0x1a4", latest_block: "0x2b67" },
  layerx: { node_info: null },
  anchor: { latest_finalized_batch: null, status: null, status_name: null, status_ladder: null },
});
assert.equal(silentNetwork.anchor?.latestFinalizedBatch, null);
assert.notEqual(silentNetwork.anchor?.latestFinalizedBatch, 0n);
assert.equal(silentNetwork.anchor?.status, null);
assert.notEqual(silentNetwork.anchor?.status, 0n);
assert.equal(silentNetwork.anchor?.statusName, null);
assert.notEqual(silentNetwork.anchor?.statusName, "");
assert.equal(silentNetwork.anchor?.statusLadder, null);
assert.deepEqual(silentNetwork.layerx, { node_info: null });
assert.equal(decodePxNetworkHead({
  network_id: "layerx-beta",
  paxeer: { chain_id: "0x1a4", latest_block: "0x2b67" },
  layerx: null,
  anchor: null,
}).anchor, null);
assert.throws(() => decodePxNetworkHead({ network_id: "layerx-beta", paxeer: { chain_id: "0x1a4" }, layerx: null, anchor: null }));
assert.throws(() => decodePxNetworkHead({ network_id: "", paxeer: { chain_id: "0x1a4", latest_block: "0x1" }, layerx: null, anchor: null }));

const requests: { readonly method: unknown; readonly params: unknown }[] = [];
const gateway = http.createServer((request, response) => {
  const chunks: Buffer[] = [];
  request.on("data", (chunk: Buffer) => chunks.push(Buffer.from(chunk)));
  request.on("end", () => {
    const body = JSON.parse(Buffer.concat(chunks).toString("utf8")) as { jsonrpc: string; id: string; method: string; params: unknown };
    assert.equal(request.method, "POST");
    assert.equal(request.headers["content-type"], "application/json");
    assert.equal(request.url, "/rpc");
    assert.equal(body.jsonrpc, "2.0");
    requests.push({ method: body.method, params: body.params });
    const answer = body.method === "px_getBalances"
      ? { jsonrpc: "2.0", id: body.id, result: balances }
      : { jsonrpc: "2.0", id: body.id, error: { code: -32001, message: "Paxeer read unavailable", data: { code: "paxeer_unreachable" } } };
    const encoded = Buffer.from(JSON.stringify(answer), "utf8");
    response.writeHead(200, { "Content-Type": "application/json", "Content-Length": encoded.length });
    response.end(encoded);
  });
});
gateway.listen(0, "127.0.0.1");
await once(gateway, "listening");
const address = gateway.address();
assert(address !== null && typeof address === "object", "gateway listener missing");
try {
  const client = new PxClient({ endpoint: `http://127.0.0.1:${address.port}/rpc` });
  const table = await client.getBalances(ACCOUNT.toUpperCase());
  assert.equal(table.balances.length, 2);
  assert.equal(table.balances[0]!.paxeer?.amount, 30n);
  assert.equal(table.balances[1]!.paxeer, null);
  assert.equal(table.balances[1]!.denom, null);
  assert.deepEqual(requests[0], { method: "px_getBalances", params: [ACCOUNT] });

  await assert.rejects(client.getNetwork(), (error: unknown) => error instanceof JsonRpcError
    && error.code === -32001
    && error.message === "Paxeer read unavailable"
    && (error.data as { code: string }).code === "paxeer_unreachable");
  await assert.rejects(client.listAssets(), (error: unknown) => error instanceof JsonRpcError && error.code === -32001);
  await assert.rejects(client.resolveAccount(DID), (error: unknown) => error instanceof JsonRpcError && error.code === -32001);
  await assert.rejects(client.getAccount(EVM), (error: unknown) => error instanceof JsonRpcError && error.code === -32001);
  assert.deepEqual(requests.map((entry) => entry.method), ["px_getBalances", "px_getNetwork", "px_listAssets", "px_resolveAccount", "px_getAccount"]);
  assert.deepEqual(requests[1]!.params, []);
  assert.deepEqual(requests[3]!.params, [DID]);
  assert.deepEqual(requests[4]!.params, [EVM]);
} finally {
  gateway.close();
  await once(gateway, "close");
}

assert.throws(() => new PxClient({ endpoint: "http://example.com/rpc" }));
assert.throws(() => new PxClient({ endpoint: "https://user:secret@example.com/rpc" }));
assert.throws(() => new PxClient({ endpoint: "wss://example.com/rpc" }));
assert.throws(() => new PxClient({ endpoint: "https://example.com/rpc#head" }));
await assert.rejects(new PxClient({ endpoint: "http://127.0.0.1:1/rpc" }).getNetwork());
await assert.rejects(new PxClient({ endpoint: "https://example.com/rpc" }).getBalances("not-an-account"));
