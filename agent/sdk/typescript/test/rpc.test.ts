import assert from "node:assert/strict";
import { decodeJsonRpcResponse, JsonRpcClient, JsonRpcError, walletAccount } from "../src/rpc.js";

assert.throws(() => new JsonRpcClient("http://example.com"));
assert.throws(() => new JsonRpcClient("https://user:secret@example.com"));
assert.throws(() => decodeJsonRpcResponse({jsonrpc:"2.0", id:"2", result:{}}, "1"));
assert.throws(() => decodeJsonRpcResponse({jsonrpc:"2.0", id:"1", result:{}, error:{}}, "1"));
assert.throws(() => decodeJsonRpcResponse({jsonrpc:"2.0", id:"1", error:{code:-32005, message:"Read unavailable", data:{retry:1}}}, "1"),
  (error: unknown) => error instanceof JsonRpcError && error.code === -32005);
assert.deepEqual(decodeJsonRpcResponse({jsonrpc:"2.0", id:"1", result:{state:"pending"}}, "1"), {state:"pending"});
assert.notEqual(walletAccount("did:layerx:alice", "11".repeat(32), "00".repeat(32)), walletAccount("did:layerx:alice", "00".repeat(32), "00".repeat(32)));
assert.throws(() => walletAccount("did::alice", "11".repeat(32), "00".repeat(32)));

assert.equal(walletAccount("did:layerx:alice", "00".repeat(32), "00".repeat(32)), "575498fd80da9b17115311af107ab11639acf474a69f944c9c9b1d0ea28ed205");
assert.equal(walletAccount("did:layerx:alice", "11".repeat(32), "00".repeat(32)), "c25ada37deae26ed54923dab01b56e9dded08b4ca710b0fd0b1d172f44404324");

const { encodeNativeRegistration, WalletRpc } = await import("../src/wallet.js");
const { readFileSync } = await import("node:fs");
const registration = Buffer.from(readFileSync(new URL("../../../../crates/layerx-crypto/tests/fixtures/payments/1-1.hex",import.meta.url),"utf8").trim(),"hex");
const symbolLength=registration[66]!;
const nameOffset=67+symbolLength;
const nameLength=registration[nameOffset]!;
const decimalsOffset=nameOffset+1+nameLength;
const cap=BigInt(`0x${registration.subarray(decimalsOffset+1,decimalsOffset+17).toString("hex")}`);
assert.equal(Buffer.from(encodeNativeRegistration({salt:registration.subarray(34,66).toString("hex"),symbol:registration.subarray(67,nameOffset).toString(),name:registration.subarray(nameOffset+1,decimalsOffset).toString(),decimals:registration[decimalsOffset]!,supplyCap:cap},"did:layerx:alice")).toString("hex"),registration.toString("hex"));
assert.throws(()=>new WalletRpc("relative",{} as never));

const feeClient = new JsonRpcClient("http://127.0.0.1:1");
assert.throws(() => feeClient.estimateFee(new Uint8Array()));
assert.throws(() => feeClient.estimateFee(new Uint8Array(524289)));
assert.throws(() => feeClient.getAsset("AB".repeat(32)));

const { subscriptionAcknowledgement, subscriptionNotification } = await import("../src/rpc-subscription.js");
assert.equal(subscriptionAcknowledgement({jsonrpc:"2.0",id:"1",result:"sub"},"1"),"sub");
assert.throws(() => subscriptionAcknowledgement({jsonrpc:"2.0",id:"2",result:"sub"},"1"));
assert.throws(() => subscriptionAcknowledgement({jsonrpc:"2.0",id:"1",result:{state:"accepted"}},"1"));
const event = {jsonrpc:"2.0",method:"lx_subscription",params:{subscription:"sub",result:{state:"pending"}}};
assert.deepEqual(subscriptionNotification(event,"sub"),{state:"pending"});
assert.throws(() => subscriptionNotification(event,"other"));
await assert.rejects(feeClient.subscribe("account").next());
await assert.rejects(feeClient.subscribe("receipts","ab".repeat(32)).next());
const cancellation = new AbortController(); cancellation.abort();
await assert.rejects(feeClient.subscribe("receipts",undefined,cancellation.signal).next());
