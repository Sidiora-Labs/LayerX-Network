import assert from "node:assert/strict";
import { decodeAssetListSnapshot, decodeAssetSnapshot, decodeIdentitySequenceSnapshot, decodeJsonRpcResponse, JsonRpcClient, JsonRpcError, walletAccount } from "../src/rpc.js";

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

const asset = {asset_id:"11".repeat(32),symbol:"USD",name:"Test Dollar",decimals:6,custody_kind:0,custody_reference:"",paused:false,supply_cap:"1000000",issuer_did:"22".repeat(32),issuer_kind:1,total_units:"100",salt:"33".repeat(32)};
assert.deepEqual(decodeAssetSnapshot({asset,observed_head_sequence:"9",state_root:"44".repeat(32),verification:"authenticated_committed_snapshot"}),{asset:{assetId:"11".repeat(32),symbol:"USD",name:"Test Dollar",decimals:6,custodyKind:0,custodyReference:"",paused:false,supplyCap:1000000n,issuerDid:"22".repeat(32),issuerKind:1,totalUnits:100n,salt:"33".repeat(32)},observedHeadSequence:9n,stateRoot:"44".repeat(32)});
assert.throws(()=>decodeAssetListSnapshot({assets:[{...asset,asset_id:"22".repeat(32)},asset],observed_head_sequence:"9",state_root:"44".repeat(32),verification:"authenticated_committed_snapshot"}));
assert.deepEqual(decodeIdentitySequenceSnapshot({did:"did:layerx:alice",next_sequence:"7",observed_head_sequence:"11",state_root:"44".repeat(32),verification:"authenticated_node_snapshot"}),{did:"did:layerx:alice",nextSequence:7n,observedHeadSequence:11n,stateRoot:"44".repeat(32)});
assert.throws(()=>decodeIdentitySequenceSnapshot({did:"did:layerx:alice",next_sequence:"07",observed_head_sequence:"11",state_root:"44".repeat(32),verification:"authenticated_node_snapshot"}));

const { subscriptionAcknowledgement, subscriptionCursor, subscriptionNotification, subscriptionSelector, unsubscribeAcknowledgement } = await import("../src/rpc-subscription.js");
assert.equal(subscriptionAcknowledgement({jsonrpc:"2.0",id:"1",result:"sub"},"1"),"sub");
assert.throws(() => subscriptionAcknowledgement({jsonrpc:"2.0",id:"2",result:"sub"},"1"));
assert.throws(() => subscriptionAcknowledgement({jsonrpc:"2.0",id:"1",result:{state:"accepted"}},"1"));
const event = {jsonrpc:"2.0",method:"lx_subscription",params:{subscription:"sub",result:{state:"pending"},cursor:"41"}};
assert.deepEqual(subscriptionNotification(event,"sub"),{result:{state:"pending"},cursor:41n});
assert.throws(() => subscriptionNotification(event,"other"));
assert.throws(() => subscriptionNotification({jsonrpc:"2.0",method:"lx_subscription",params:{subscription:"sub",result:{state:"pending"}}},"sub"));
for (const cursor of ["041","",41,"-1","+1","18446744073709551616"]) {
  assert.throws(() => subscriptionNotification({jsonrpc:"2.0",method:"lx_subscription",params:{subscription:"sub",result:{state:"pending"},cursor}},"sub"));
  assert.throws(() => subscriptionCursor(cursor));
}
assert.equal(subscriptionCursor("0"),0n);
assert.equal(subscriptionCursor("18446744073709551615"),18446744073709551615n);
assert.deepEqual(subscriptionSelector("receipts"),["receipts"]);
assert.deepEqual(subscriptionSelector("receipts",undefined,41n),["receipts","41"]);
assert.deepEqual(subscriptionSelector("checkpoints",undefined,0n),["checkpoints","0"]);
assert.deepEqual(subscriptionSelector("account","ab".repeat(32),7n),["account","ab".repeat(32),"7"]);
assert.throws(() => subscriptionSelector("account",undefined,7n));
assert.throws(() => subscriptionSelector("receipts",undefined,-1n));
assert.throws(() => subscriptionSelector("receipts",undefined,18446744073709551616n));
assert.equal(unsubscribeAcknowledgement({jsonrpc:"2.0",id:"1:unsubscribe",result:true},"1:unsubscribe"),true);
assert.throws(() => unsubscribeAcknowledgement({jsonrpc:"2.0",id:"1:unsubscribe",result:false},"1:unsubscribe"));
assert.throws(() => unsubscribeAcknowledgement({jsonrpc:"2.0",id:"1:unsubscribe",result:"true"},"1:unsubscribe"));
assert.throws(() => unsubscribeAcknowledgement({jsonrpc:"2.0",id:"2:unsubscribe",result:true},"1:unsubscribe"));
assert.throws(() => unsubscribeAcknowledgement({jsonrpc:"2.0",id:"1:unsubscribe",error:{code:-32602,message:"Unknown subscription"}},"1:unsubscribe"),
  (error: unknown) => error instanceof JsonRpcError && error.code === -32602);
await assert.rejects(feeClient.subscribe("account").next());
await assert.rejects(feeClient.subscribe("receipts","ab".repeat(32)).next());
await assert.rejects(feeClient.subscribeFrom("receipts",-1n).next());
const cancellation = new AbortController(); cancellation.abort();
await assert.rejects(feeClient.subscribe("receipts",undefined,cancellation.signal).next());
await assert.rejects(feeClient.subscribeFrom("receipts",41n,undefined,cancellation.signal).next());

const { SubscriptionContinuity } = await import("../src/rpc-subscription.js");
const receiptContinuity = new SubscriptionContinuity("receipts", 40n);
receiptContinuity.accept({cursor: 41n, result: {}});
for (const cursor of [40n, 41n, 43n]) assert.throws(() => receiptContinuity.accept({cursor, result: {}}));
receiptContinuity.accept({cursor: 42n, result: {}});
const accountContinuity = new SubscriptionContinuity("account", 40n);
accountContinuity.accept({cursor: 45n, result: {}});
assert.throws(() => accountContinuity.accept({cursor: 45n, result: {}}));
assert.throws(() => new SubscriptionContinuity("checkpoints", 40n).accept({cursor: 40n, result: {}}));

const { decodeMirrorProcessResult } = await import("../src/node-mirror.js");
const mirrorOutput = JSON.stringify({ok:true,verification:{level:"receipt-verified",batchNumber:"3",headerDigest:"11".repeat(32),evidenceDigest:"22".repeat(32),sourceId:"source",target:"mirror",canonicalPosition:"3",provenance:"Canonical",latestBatch:null,batchLag:"0",failoverCount:0,agreeingSources:1,checkpointLevel:"unavailable"}});
assert.equal(decodeMirrorProcessResult(0,null,mirrorOutput,3n).batchNumber,3n);
assert.throws(() => decodeMirrorProcessResult(1,null,mirrorOutput,3n));
assert.throws(() => decodeMirrorProcessResult(null,"SIGTERM",mirrorOutput,3n));
assert.throws(() => decodeMirrorProcessResult(0,null,mirrorOutput,4n));
