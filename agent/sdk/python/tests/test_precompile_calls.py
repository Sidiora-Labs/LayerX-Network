from __future__ import annotations

import json
import unittest
from pathlib import Path

from layerx_sdk.bridge import BRIDGE_EVENTS, LAYERX_BRIDGE_PRECOMPILE, bridge_in_call, bridge_out_call, decode_bridge_event
from layerx_sdk.exchange import (
    EXCHANGE_EVENTS,
    LAYERX_EXCHANGE_PRECOMPILE,
    PrecompileAbiError,
    abi_selector,
    decode_exchange_event,
    exchange_cancel_order_call,
    exchange_deposit_margin_call,
    exchange_deposit_margin_token_call,
    exchange_place_order_call,
    exchange_request_settlement_call,
    exchange_withdraw_margin_call,
    precompile_transaction_request,
    send_exchange_deposit_margin,
)
from layerx_sdk.launchpad import (
    LAUNCHPAD_EVENTS,
    LAUNCHPAD_PRECOMPILE,
    decode_launchpad_event,
    launchpad_buy_call,
    launchpad_claim_fees_call,
    launchpad_create_market_call,
    launchpad_sell_call,
    launchpad_set_fee_strategy_call,
    launchpad_token_call,
    send_launchpad_token_write,
)

ROOT = Path(__file__).resolve().parents[4]
TOPICS = {
    "MarginDeposited": "456ba29aa60d5cac1a6dc1c0f3df30b1f18963fd90dafe8c5f4a6de80440118a",
    "MarginWithdrawalRequested": "9cf8600ce0e07d0d2b0b82ed99fc940eb6121143201d8d682c25fc74972c88f0",
    "OrderCancelRequested": "39148489da3c16ee8c589a95e2f0c869816ee4f816b4ba1b85b32bc6d94c0241",
    "OrderPlaced": "88b93538701d726739ade066c2a9f09e5088f608d9b2ebba0cf305f5ee0752ce",
    "SettlementRequested": "70d5b9c37994017669de2a991c65a88ebeb34999f37c6dc6d0c44462d57eb655",
    "BridgeIn": "4352fb2e09bdaa35c4d407ce85dfa93eaec318876eddd6f5a490cce830c3f274",
    "BridgeOut": "3e990eb54009dcdca53d8fa87307210f07097f37dcf6185dee71a42f8e7d524e",
    "AirdropClaimed": "d399c6e7fad358fc300beda3f056717c94a04c7233ce92683de6500ba509022e",
    "AirdropExecuted": "171b2f9dc7a4c7eaa8ca718bcac62fbec15d147f033f38e42971b7ccabe9a469",
    "FeeRecorded": "b4d4d3bd2f97a7d6f1657ee69f7191d7aa7dbd5b6864a2d7a9d14efc1322552f",
    "FeeStrategyChanged": "66c2a2c42cf36fad89e5da817a0b5de0fd78d7481cbfdc59f604148252da2261",
    "FeesBurned": "0d9575a73e2a7da16cfde907df749d23d901528ff2e7c832b731babdecca000b",
    "FeesClaimed": "fe3464cd748424446c37877c28ce5b700222c5bc9f90d908afcc4e5cb22707ff",
    "LpRewardsExecuted": "a9e7850d400945e0434ddd18a194aff01f31efaae577a63486c3dc865c5ab759",
    "MarketCreated": "d8ad483b7300b5831650c4747b4d85390539f25f7d7d8c635eb3f8147daf198e",
    "PauseToggled": "79a5bc58b021076f821571d0fe8b0ae3d9e0a666563bb064fdbf0bf69281331c",
    "Swap": "f3369c7e0aa652773c7246b5481ca4b1ee0b408d90467d2ce93b165b9938fde5",
}
SOURCES = (("layerxexchange", EXCHANGE_EVENTS), ("layerxbridge", BRIDGE_EVENTS), ("launchpad", LAUNCHPAD_EVENTS))


def abi(directory: str) -> list[dict]:
    return json.loads((ROOT / "precompiles" / directory / "abi.json").read_text())


def word(value: int) -> str:
    return f"{value:064x}"


def ident(byte: str) -> str:
    return "0x" + byte * 32


def address(byte: str) -> str:
    return "0x" + byte * 20


def address_word(byte: str) -> str:
    return "0" * 24 + byte * 20


def text(value: str) -> str:
    body = value.encode().hex()
    return word(len(body) // 2) + body.ljust(-(-len(body) // 64) * 64, "0")


FROM = address("12")
SWAP = address_word("aa") + word(100) + word(90) + address_word("bb") + word(1700000000)
WRITES = [
    ("layerxexchange", "placeOrder(bytes32,uint8,uint256,uint256,uint8)",
     exchange_place_order_call(ident("11"), 2, 18446744073709551618, 5000, 0), LAYERX_EXCHANGE_PRECOMPILE,
     "11" * 32 + word(2) + word(18446744073709551618) + word(5000) + word(0)),
    ("layerxexchange", "cancelOrder(bytes32)", exchange_cancel_order_call(ident("31")), LAYERX_EXCHANGE_PRECOMPILE, "31" * 32),
    ("layerxexchange", "requestSettlement(bytes32)", exchange_request_settlement_call(ident("41")), LAYERX_EXCHANGE_PRECOMPILE, "41" * 32),
    ("layerxexchange", "depositMargin(bytes32)", exchange_deposit_margin_call(ident("51"), 7), LAYERX_EXCHANGE_PRECOMPILE, "51" * 32),
    ("layerxexchange", "depositMarginToken(address,uint256,bytes32)",
     exchange_deposit_margin_token_call(address("ab"), 9, ident("51")), LAYERX_EXCHANGE_PRECOMPILE,
     address_word("ab") + word(9) + "51" * 32),
    ("layerxexchange", "withdrawMargin(bytes32,bytes32,uint256)",
     exchange_withdraw_margin_call(ident("51"), ident("61"), 10), LAYERX_EXCHANGE_PRECOMPILE, "51" * 32 + "61" * 32 + word(10)),
    ("layerxbridge", "bridgeIn(uint64,address,bytes32,uint64,bytes32,address,uint256,bytes[])",
     bridge_in_call(1, address("aa"), ident("bb"), 3, ident("cc"), address("dd"), 5, ["0x0102", "0x" + "ee" * 33]),
     LAYERX_BRIDGE_PRECOMPILE,
     word(1) + address_word("aa") + "bb" * 32 + word(3) + "cc" * 32 + address_word("dd") + word(5) + word(256)
     + word(2) + word(64) + word(128) + word(2) + "0102".ljust(64, "0") + word(33) + ("ee" * 33).ljust(128, "0")),
    ("layerxbridge", "bridgeOut(uint64,address,uint256,address)", bridge_out_call(8, address("dd"), 5, address("ee")),
     LAYERX_BRIDGE_PRECOMPILE, word(8) + address_word("dd") + word(5) + address_word("ee")),
    ("launchpad", "buy(address,uint256,uint256,address,uint256)",
     launchpad_buy_call(address("aa"), 100, 90, address("bb"), 1700000000), LAUNCHPAD_PRECOMPILE, SWAP),
    ("launchpad", "sell(address,uint256,uint256,address,uint256)",
     launchpad_sell_call(address("aa"), 100, 90, address("bb"), 1700000000), LAUNCHPAD_PRECOMPILE, SWAP),
    ("launchpad", "createMarket(string,string,uint8)", launchpad_create_market_call("Paxeer Dog", "PDOG", 2), LAUNCHPAD_PRECOMPILE,
     word(96) + word(160) + word(2) + text("Paxeer Dog") + text("PDOG")),
    ("launchpad", "setFeeStrategy(address,uint8)", launchpad_set_fee_strategy_call(address("aa"), 1), LAUNCHPAD_PRECOMPILE,
     address_word("aa") + word(1)),
    ("launchpad", "claimFees(address,address)", launchpad_claim_fees_call(address("aa"), address("bb")), LAUNCHPAD_PRECOMPILE,
     address_word("aa") + address_word("bb")),
] + [
    ("launchpad", f"{write}(address)", launchpad_token_call(write, address("aa")), LAUNCHPAD_PRECOMPILE, address_word("aa"))
    for write in ("claimAirdrop", "executeAirdrop", "executeBurn", "executeLpRewards", "pause", "unpause")
]


class PrecompileCallsTest(unittest.TestCase):
    def test_every_abi_event_is_declared_with_its_topic(self) -> None:
        declared = 0
        for directory, specs in SOURCES:
            events = [entry for entry in abi(directory) if entry["type"] == "event"]
            self.assertEqual(len(specs), len(events), directory)
            for entry in events:
                spec = next(spec for spec in specs if spec.name == entry["name"])
                self.assertEqual(
                    [(item.name, item.type, item.indexed) for item in spec.inputs],
                    [(item["name"], item["type"], bool(item.get("indexed"))) for item in entry["inputs"]],
                )
                self.assertEqual(spec.topic0, "0x" + TOPICS[spec.name])
                declared += 1
        self.assertEqual(declared, 17)

    def test_every_write_encodes_its_abi_calldata(self) -> None:
        self.assertEqual(abi_selector("transfer(address,uint256)"), "0xa9059cbb")
        for directory, signature, call, to, body in WRITES:
            self.assertEqual(call.to, to, signature)
            self.assertEqual(call.data, abi_selector(signature) + body, signature)
            self.assertEqual(call.value, 7 if signature.startswith("depositMargin(") else 0, signature)
            name = signature.split("(")[0]
            entry = next(item for item in abi(directory) if item["type"] == "function" and item["name"] == name)
            self.assertEqual(f"{name}({','.join(item['type'] for item in entry['inputs'])})", signature)
        for directory, _ in SOURCES:
            for entry in abi(directory):
                if entry["type"] == "function" and entry.get("stateMutability") not in ("view", "pure"):
                    self.assertTrue(
                        any(source == directory and sig.startswith(entry["name"] + "(") for source, sig, *_ in WRITES),
                        entry["name"],
                    )
        with self.assertRaises(PrecompileAbiError):
            exchange_deposit_margin_call(ident("51"), 0)
        with self.assertRaises(PrecompileAbiError):
            exchange_place_order_call(ident("11"), 256, 1, 1, 0)

    def test_send_helpers_ask_the_wallet_for_eth_send_transaction(self) -> None:
        sent: list[tuple[str, list[object]]] = []

        def request(method: str, params: list[object]) -> object:
            sent.append((method, params))
            return ident("fe")

        self.assertEqual(send_exchange_deposit_margin(request, FROM, ident("51"), 255), ident("fe"))
        self.assertEqual(send_launchpad_token_write(request, FROM, "pause", address("aa")), ident("fe"))
        self.assertEqual(sent, [
            ("eth_sendTransaction", [{"from": FROM, "to": LAYERX_EXCHANGE_PRECOMPILE,
                                      "data": abi_selector("depositMargin(bytes32)") + "51" * 32, "value": "0xff"}]),
            ("eth_sendTransaction", [{"from": FROM, "to": LAUNCHPAD_PRECOMPILE,
                                      "data": abi_selector("pause(address)") + address_word("aa"), "value": "0x0"}]),
        ])
        self.assertEqual(precompile_transaction_request(FROM, exchange_cancel_order_call(ident("31")))["value"], "0x0")
        with self.assertRaises(PrecompileAbiError) as caught:
            send_exchange_deposit_margin(lambda method, params: None, FROM, ident("51"), 1)
        self.assertEqual(caught.exception.code, "malformed_wallet_answer")

    def test_logs_decode_to_their_typed_fields(self) -> None:
        topics = ["0x" + TOPICS["OrderPlaced"], ident("a1"), ident("11"), "0x" + address_word("12")]
        data = "0x" + word(2) + word(18446744073709551618) + word(5000) + word(0) + word(4)
        order = decode_exchange_event(LAYERX_EXCHANGE_PRECOMPILE, topics, data)
        self.assertEqual(order.event, "OrderPlaced")
        self.assertEqual(order.fields, {
            "intentId": ident("a1"), "marketId": ident("11"), "owner": FROM, "side": 2,
            "price": 18446744073709551618, "quantity": 5000, "timeInForce": 0, "nonce": 4,
        })
        for code, args in (
            ("topic_count", (LAYERX_EXCHANGE_PRECOMPILE, topics[:3], data)),
            ("data_length", (LAYERX_EXCHANGE_PRECOMPILE, topics, data[:-2])),
            ("non_canonical_word", (LAYERX_EXCHANGE_PRECOMPILE, topics, "0x" + word(256) + data[66:])),
            ("unknown_event", (LAUNCHPAD_PRECOMPILE, topics, data)),
        ):
            with self.assertRaises(PrecompileAbiError) as caught:
                decode_exchange_event(*args)
            self.assertEqual(caught.exception.code, code)
        bridge_in = decode_bridge_event(
            LAYERX_BRIDGE_PRECOMPILE,
            ["0x" + TOPICS["BridgeIn"], "0x" + word(1), ident("bb"), "0x" + address_word("12")],
            "0x" + word(3) + address_word("dd") + word(5) + word(128) + text("factory/paxeer/usdc"),
        )
        self.assertEqual(bridge_in.fields, {
            "chain": 1, "txHash": ident("bb"), "recipient": FROM, "logIndex": 3,
            "asset": address("dd"), "amount": 5, "denom": "factory/paxeer/usdc",
        })
        bridge_out = decode_bridge_event(
            LAYERX_BRIDGE_PRECOMPILE,
            ["0x" + TOPICS["BridgeOut"], "0x" + word(8), "0x" + address_word("dd"), "0x" + word(9)],
            "0x" + word(5) + address_word("ee"),
        )
        self.assertEqual(bridge_out.fields, {"chain": 8, "asset": address("dd"), "amount": 5, "recipient": address("ee"), "nonce": 9})
        created = decode_launchpad_event(
            LAUNCHPAD_PRECOMPILE,
            ["0x" + TOPICS["MarketCreated"], "0x" + address_word("aa"), "0x" + address_word("12")],
            "0x" + word(128) + word(192) + word(256) + word(2) + text("factory/pdog") + text("Paxeer Dog") + text("PDOG"),
        )
        self.assertEqual(created.fields, {
            "token": address("aa"), "creator": FROM, "denom": "factory/pdog", "name": "Paxeer Dog", "symbol": "PDOG", "feeStrategy": 2,
        })
        with self.assertRaises(PrecompileAbiError):
            decode_launchpad_event(LAUNCHPAD_PRECOMPILE, ["0x" + TOPICS["PauseToggled"], "0x" + address_word("aa")], "0x" + word(2))
        swap = decode_launchpad_event(
            LAUNCHPAD_PRECOMPILE,
            ["0x" + TOPICS["Swap"], "0x" + address_word("aa"), "0x" + address_word("12"), "0x" + address_word("bb")],
            "0x" + word(1) + word(100) + word(95) + word(1) + word(7),
        )
        self.assertEqual(swap.fields, {
            "token": address("aa"), "trader": FROM, "recipient": address("bb"), "isBuy": True,
            "amountIn": 100, "amountOut": 95, "feeAmount": 1, "price": 7,
        })


if __name__ == "__main__":
    unittest.main()
