import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  BRIDGE_EVENTS,
  EXCHANGE_EVENTS,
  LAUNCHPAD_EVENTS,
  LAUNCHPAD_PRECOMPILE,
  LAYERX_BRIDGE_PRECOMPILE,
  LAYERX_EXCHANGE_PRECOMPILE,
  PrecompileAbiError,
  abiEventTopic,
  abiSelector,
  bridgeInCall,
  bridgeOutCall,
  decodeBridgeEvent,
  decodeExchangeEvent,
  decodeLaunchpadEvent,
  exchangeCancelOrderCall,
  exchangeDepositMarginCall,
  exchangeDepositMarginTokenCall,
  exchangePlaceOrderCall,
  exchangeRequestSettlementCall,
  exchangeWithdrawMarginCall,
  launchpadBuyCall,
  launchpadClaimFeesCall,
  launchpadCreateMarketCall,
  launchpadSellCall,
  launchpadSetFeeStrategyCall,
  launchpadTokenCall,
  precompileEventSignature,
  precompileTransactionRequest,
  sendExchangeDepositMargin,
  sendLaunchpadTokenWrite,
  type Eip1193Requester,
  type PrecompileCall,
  type PrecompileEventSpec,
  type PrecompileLog,
} from "../src/index.js";

interface AbiEntry {
  readonly type: string;
  readonly name?: string;
  readonly stateMutability?: string;
  readonly inputs?: readonly { readonly type: string; readonly indexed?: boolean; readonly name: string }[];
}

function abi(directory: string): readonly AbiEntry[] {
  return JSON.parse(readFileSync(
    new URL(`../../../../../precompiles/${directory}/abi.json`, import.meta.url),
    "utf8",
  )) as readonly AbiEntry[];
}

const sources: readonly (readonly [string, readonly PrecompileEventSpec[]])[] = [
  ["layerxexchange", EXCHANGE_EVENTS],
  ["layerxbridge", BRIDGE_EVENTS],
  ["launchpad", LAUNCHPAD_EVENTS],
];

const TOPICS: Readonly<Record<string, string>> = {
  MarginDeposited: "456ba29aa60d5cac1a6dc1c0f3df30b1f18963fd90dafe8c5f4a6de80440118a",
  MarginWithdrawalRequested: "9cf8600ce0e07d0d2b0b82ed99fc940eb6121143201d8d682c25fc74972c88f0",
  OrderCancelRequested: "39148489da3c16ee8c589a95e2f0c869816ee4f816b4ba1b85b32bc6d94c0241",
  OrderPlaced: "88b93538701d726739ade066c2a9f09e5088f608d9b2ebba0cf305f5ee0752ce",
  SettlementRequested: "70d5b9c37994017669de2a991c65a88ebeb34999f37c6dc6d0c44462d57eb655",
  BridgeIn: "4352fb2e09bdaa35c4d407ce85dfa93eaec318876eddd6f5a490cce830c3f274",
  BridgeOut: "3e990eb54009dcdca53d8fa87307210f07097f37dcf6185dee71a42f8e7d524e",
  AirdropClaimed: "d399c6e7fad358fc300beda3f056717c94a04c7233ce92683de6500ba509022e",
  AirdropExecuted: "171b2f9dc7a4c7eaa8ca718bcac62fbec15d147f033f38e42971b7ccabe9a469",
  FeeRecorded: "b4d4d3bd2f97a7d6f1657ee69f7191d7aa7dbd5b6864a2d7a9d14efc1322552f",
  FeeStrategyChanged: "66c2a2c42cf36fad89e5da817a0b5de0fd78d7481cbfdc59f604148252da2261",
  FeesBurned: "0d9575a73e2a7da16cfde907df749d23d901528ff2e7c832b731babdecca000b",
  FeesClaimed: "fe3464cd748424446c37877c28ce5b700222c5bc9f90d908afcc4e5cb22707ff",
  LpRewardsExecuted: "a9e7850d400945e0434ddd18a194aff01f31efaae577a63486c3dc865c5ab759",
  MarketCreated: "d8ad483b7300b5831650c4747b4d85390539f25f7d7d8c635eb3f8147daf198e",
  PauseToggled: "79a5bc58b021076f821571d0fe8b0ae3d9e0a666563bb064fdbf0bf69281331c",
  Swap: "f3369c7e0aa652773c7246b5481ca4b1ee0b408d90467d2ce93b165b9938fde5",
};

let declared = 0;
for (const [directory, specs] of sources) {
  const events = abi(directory).filter((entry) => entry.type === "event");
  assert.equal(specs.length, events.length, directory);
  for (const entry of events) {
    const spec = specs.find((candidate) => candidate.name === entry.name);
    assert.ok(spec, `${directory} ${entry.name ?? ""}`);
    assert.deepEqual(
      spec.inputs.map((input) => [input.name, input.type, input.indexed]),
      (entry.inputs ?? []).map((input) => [input.name, input.type, input.indexed === true]),
    );
    assert.equal(abiEventTopic(precompileEventSignature(spec)), `0x${TOPICS[spec.name] ?? ""}`, spec.name);
    declared += 1;
  }
}
assert.equal(declared, 17);

const word = (value: bigint | number): string => BigInt(value).toString(16).padStart(64, "0");
const id = (byte: string): string => `0x${byte.repeat(32)}`;
const address = (byte: string): string => `0x${byte.repeat(20)}`;
const addressWord = (byte: string): string => "0".repeat(24) + byte.repeat(20);
const text = (value: string): string => {
  const body = Buffer.from(value, "utf8").toString("hex");
  return word(body.length / 2) + body.padEnd(Math.ceil(body.length / 64) * 64, "0");
};

const writes: readonly (readonly [string, string, PrecompileCall, string, string])[] = [
  ["layerxexchange", "placeOrder(bytes32,uint8,uint256,uint256,uint8)",
    exchangePlaceOrderCall({ marketId: id("11"), side: 2, price: 18446744073709551618n, quantity: 5000n, timeInForce: 0 }),
    LAYERX_EXCHANGE_PRECOMPILE, "11".repeat(32) + word(2) + word(18446744073709551618n) + word(5000) + word(0)],
  ["layerxexchange", "cancelOrder(bytes32)", exchangeCancelOrderCall(id("31")), LAYERX_EXCHANGE_PRECOMPILE, "31".repeat(32)],
  ["layerxexchange", "requestSettlement(bytes32)", exchangeRequestSettlementCall(id("41")), LAYERX_EXCHANGE_PRECOMPILE, "41".repeat(32)],
  ["layerxexchange", "depositMargin(bytes32)", exchangeDepositMarginCall(id("51"), 7n), LAYERX_EXCHANGE_PRECOMPILE, "51".repeat(32)],
  ["layerxexchange", "depositMarginToken(address,uint256,bytes32)",
    exchangeDepositMarginTokenCall(address("ab"), 9n, id("51")), LAYERX_EXCHANGE_PRECOMPILE,
    addressWord("ab") + word(9) + "51".repeat(32)],
  ["layerxexchange", "withdrawMargin(bytes32,bytes32,uint256)",
    exchangeWithdrawMarginCall(id("51"), id("61"), 10n), LAYERX_EXCHANGE_PRECOMPILE, "51".repeat(32) + "61".repeat(32) + word(10)],
  ["layerxbridge", "bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])",
    bridgeInCall({ chain: 1n, vault: address("aa"), txHash: id("bb"), logIndex: 3n, recipient: id("cc"), asset: address("dd"), amount: 5n, signatures: ["0x0102", "0x" + "ee".repeat(33)] }),
    LAYERX_BRIDGE_PRECOMPILE,
    word(1) + addressWord("aa") + "bb".repeat(32) + word(3) + "cc".repeat(32) + addressWord("dd") + word(5) + word(256)
      + word(2) + word(64) + word(128) + word(2) + "0102".padEnd(64, "0") + word(33) + ("ee".repeat(33)).padEnd(128, "0")],
  ["layerxbridge", "bridgeOut(uint64,address,uint256,address)",
    bridgeOutCall(8n, address("dd"), 5n, address("ee")), LAYERX_BRIDGE_PRECOMPILE, word(8) + addressWord("dd") + word(5) + addressWord("ee")],
  ["launchpad", "buy(address,uint256,uint256,address,uint256)",
    launchpadBuyCall({ token: address("aa"), amountIn: 100n, minOut: 90n, recipient: address("bb"), deadline: 1700000000n }), LAUNCHPAD_PRECOMPILE,
    addressWord("aa") + word(100) + word(90) + addressWord("bb") + word(1700000000)],
  ["launchpad", "sell(address,uint256,uint256,address,uint256)",
    launchpadSellCall({ token: address("aa"), amountIn: 100n, minOut: 90n, recipient: address("bb"), deadline: 1700000000n }), LAUNCHPAD_PRECOMPILE,
    addressWord("aa") + word(100) + word(90) + addressWord("bb") + word(1700000000)],
  ["launchpad", "createMarket(string,string,uint8)", launchpadCreateMarketCall("Paxeer Dog", "PDOG", 2), LAUNCHPAD_PRECOMPILE,
    word(96) + word(160) + word(2) + text("Paxeer Dog") + text("PDOG")],
  ["launchpad", "setFeeStrategy(address,uint8)", launchpadSetFeeStrategyCall(address("aa"), 1), LAUNCHPAD_PRECOMPILE, addressWord("aa") + word(1)],
  ["launchpad", "claimFees(address,address)", launchpadClaimFeesCall(address("aa"), address("bb")), LAUNCHPAD_PRECOMPILE, addressWord("aa") + addressWord("bb")],
  ...(["claimAirdrop", "executeAirdrop", "executeBurn", "executeLpRewards", "pause", "unpause"] as const).map(
    (write) => ["launchpad", `${write}(address)`, launchpadTokenCall(write, address("aa")), LAUNCHPAD_PRECOMPILE, addressWord("aa")] as const,
  ),
];

for (const [directory, signature, call, to, body] of writes) {
  assert.equal(call.to, to, signature);
  assert.equal(call.data, abiSelector(signature) + body, signature);
  assert.equal(call.value, signature.startsWith("depositMargin(") ? 7n : 0n, signature);
  const name = signature.slice(0, signature.indexOf("("));
  const entry = abi(directory).find((candidate) => candidate.type === "function" && candidate.name === name);
  assert.ok(entry, signature);
  assert.equal(`${name}(${(entry.inputs ?? []).map((input) => input.type).join(",")})`, signature);
}
for (const [directory] of sources) {
  const nonView = abi(directory).filter(
    (entry) => entry.type === "function" && entry.stateMutability !== "view" && entry.stateMutability !== "pure",
  );
  for (const entry of nonView) {
    assert.ok(writes.some(([source, signature]) => source === directory && signature.startsWith(`${entry.name ?? ""}(`)), entry.name);
  }
}
assert.equal(abiSelector("transfer(address,uint256)"), "0xa9059cbb");

const from = address("12");
const sent: unknown[] = [];
const wallet: Eip1193Requester = {
  request: async ({ method, params }) => {
    sent.push({ method, params });
    return id("fe");
  },
};
assert.equal(await sendExchangeDepositMargin(wallet, from, id("51"), 255n), id("fe"));
assert.equal(await sendLaunchpadTokenWrite(wallet, from, "pause", address("aa")), id("fe"));
assert.deepEqual(sent, [
  { method: "eth_sendTransaction", params: [{ from, to: LAYERX_EXCHANGE_PRECOMPILE, data: abiSelector("depositMargin(bytes32)") + "51".repeat(32), value: "0xff" }] },
  { method: "eth_sendTransaction", params: [{ from, to: LAUNCHPAD_PRECOMPILE, data: abiSelector("pause(address)") + addressWord("aa"), value: "0x0" }] },
]);
assert.deepEqual(precompileTransactionRequest(from, exchangeCancelOrderCall(id("31"))).params, [
  { from, to: LAYERX_EXCHANGE_PRECOMPILE, data: abiSelector("cancelOrder(bytes32)") + "31".repeat(32), value: "0x0" },
]);
await assert.rejects(
  sendExchangeDepositMargin({ request: async () => null }, from, id("51"), 1n),
  (error: unknown) => error instanceof PrecompileAbiError && error.code === "malformed_wallet_answer",
);
assert.throws(() => exchangeDepositMarginCall(id("51"), 0n), PrecompileAbiError);
assert.throws(() => exchangePlaceOrderCall({ marketId: id("11"), side: 256, price: 1n, quantity: 1n, timeInForce: 0 }), PrecompileAbiError);

const topic = (name: string): string => `0x${TOPICS[name] ?? ""}`;
const order: PrecompileLog = {
  address: LAYERX_EXCHANGE_PRECOMPILE,
  topics: [topic("OrderPlaced"), id("a1"), id("11"), `0x${addressWord("12")}`],
  data: `0x${word(2)}${word(18446744073709551618n)}${word(5000)}${word(0)}${word(4)}`,
};
assert.deepEqual(decodeExchangeEvent(order), {
  event: "OrderPlaced",
  precompile: LAYERX_EXCHANGE_PRECOMPILE,
  topic0: topic("OrderPlaced"),
  fields: { intentId: id("a1"), marketId: id("11"), owner: from, side: 2n, price: 18446744073709551618n, quantity: 5000n, timeInForce: 0n, nonce: 4n },
});
const refused = (log: PrecompileLog, code: PrecompileAbiError["code"], decode = decodeExchangeEvent): void => {
  assert.throws(() => decode(log), (error: unknown) => error instanceof PrecompileAbiError && error.code === code);
};
refused({ ...order, topics: order.topics.slice(0, 3) }, "topic_count");
refused({ ...order, data: order.data.slice(0, -2) }, "data_length");
refused({ ...order, data: `0x${word(256)}${order.data.slice(66)}` }, "non_canonical_word");
refused({ ...order, address: LAUNCHPAD_PRECOMPILE }, "unknown_event");
refused({ ...order, topics: [id("00"), ...order.topics.slice(1)] }, "unknown_event");

const bridgeIn = decodeBridgeEvent({
  address: LAYERX_BRIDGE_PRECOMPILE,
  topics: [topic("BridgeIn"), `0x${word(1)}`, id("bb"), `0x${addressWord("12")}`],
  data: `0x${word(3)}${addressWord("dd")}${word(5)}${word(128)}${text("factory/paxeer/usdc")}`,
});
assert.equal(bridgeIn.event, "BridgeIn");
assert.deepEqual(bridgeIn.fields, { chain: 1n, txHash: id("bb"), recipient: from, logIndex: 3n, asset: address("dd"), amount: 5n, denom: "factory/paxeer/usdc" });
const bridgeOut = decodeBridgeEvent({
  address: LAYERX_BRIDGE_PRECOMPILE,
  topics: [topic("BridgeOut"), `0x${word(8)}`, `0x${addressWord("dd")}`, `0x${word(9)}`],
  data: `0x${word(5)}${addressWord("ee")}`,
});
assert.deepEqual(bridgeOut.fields, { chain: 8n, asset: address("dd"), amount: 5n, recipient: address("ee"), nonce: 9n });

const created = decodeLaunchpadEvent({
  address: LAUNCHPAD_PRECOMPILE,
  topics: [topic("MarketCreated"), `0x${addressWord("aa")}`, `0x${addressWord("12")}`],
  data: `0x${word(128)}${word(192)}${word(256)}${word(2)}${text("factory/pdog")}${text("Paxeer Dog")}${text("PDOG")}`,
});
assert.deepEqual(created.fields, { token: address("aa"), creator: from, denom: "factory/pdog", name: "Paxeer Dog", symbol: "PDOG", feeStrategy: 2n });
refused({
  address: LAUNCHPAD_PRECOMPILE,
  topics: [topic("PauseToggled"), `0x${addressWord("aa")}`],
  data: `0x${word(2)}`,
}, "non_canonical_word", decodeLaunchpadEvent);
const swap = decodeLaunchpadEvent({
  address: LAUNCHPAD_PRECOMPILE,
  topics: [topic("Swap"), `0x${addressWord("aa")}`, `0x${addressWord("12")}`, `0x${addressWord("bb")}`],
  data: `0x${word(1)}${word(100)}${word(95)}${word(1)}${word(7)}`,
});
assert.deepEqual(swap.fields, { token: address("aa"), trader: from, recipient: address("bb"), isBuy: true, amountIn: 100n, amountOut: 95n, feeAmount: 1n, price: 7n });
