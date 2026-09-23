from __future__ import annotations

import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from layerx_sdk import (
    HISTORY_DEFAULT_LIMIT,
    PX_MAXIMUM_JOINED_ASSETS,
    PxClient,
    PxPaxeerAssetBalance,
    PxRpcError,
    decode_history_asset_metadata,
    decode_layerx_history_page,
    decode_paxeer_history_page,
    decode_px_account_balances,
    decode_px_account_join,
    decode_px_asset_table,
    decode_px_network_head,
    decode_px_resolved_identities,
    decode_unified_history_page,
    history_params,
    px_account_key,
    px_optional_quantity,
    px_quantity,
)

ACCOUNT = "bb" * 32
DID = "did:layerx:" + "aa" * 32
EVM = "0x" + "11" * 20
NATIVE = "cc" * 32
UNJOINED = "dd" * 32

IDENTITIES = {
    "evm_address": EVM,
    "pax_address": "pax1qq7p8m4nfz0k7h8s2v9d3l6c5x4b3n2m1q0w9e",
    "layerx_did": DID,
    "layerx_account": ACCOUNT,
    "bound": True,
}
CUSTODY = {
    "asset_id": NATIVE,
    "denom": "ulxp",
    "pointer": "0x" + "22" * 20,
    "enabled": True,
    "paused": False,
    "minimum_deposit": "1000",
    "custody_cap": "100000000000",
    "custodied": "500000",
    "released": "0",
    "pending": "0",
}
BALANCES = {
    "account": IDENTITIES,
    "balances": [
        {
            "asset_id": NATIVE,
            "denom": "ulxp",
            "custody": CUSTODY,
            "paxeer": {"denom": "ulxp", "amount": "0x1e"},
            "layerx": {"balance": "12"},
        },
        {"asset_id": UNJOINED, "denom": None, "custody": None, "paxeer": None, "layerx": None},
    ],
    "joined_limit": 16,
}


def gateway_handler(observed: list[dict[str, object]]) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            length = int(self.headers["Content-Length"])
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            observed.append({
                "path": self.path,
                "content_type": self.headers["Content-Type"],
                "jsonrpc": body["jsonrpc"],
                "method": body["method"],
                "params": body["params"],
            })
            if body["method"] == "px_getBalances":
                answer: dict[str, object] = {"jsonrpc": "2.0", "id": body["id"], "result": BALANCES}
            else:
                answer = {
                    "jsonrpc": "2.0",
                    "id": body["id"],
                    "error": {
                        "code": -32001,
                        "message": "Paxeer read unavailable",
                        "data": {"code": "paxeer_unreachable"},
                    },
                }
            encoded = json.dumps(answer, separators=(",", ":")).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, _format: str, *args: object) -> None:
            del args

    return Handler


class UnifiedNetworkReadTest(unittest.TestCase):
    def test_account_key_accepts_the_three_public_spellings(self) -> None:
        self.assertEqual(px_account_key(EVM.upper()), EVM)
        self.assertEqual(px_account_key(f" {DID.upper()} "), DID)
        self.assertEqual(px_account_key(ACCOUNT.upper()), ACCOUNT)
        for refused in ("", "0x", "0x" + "11" * 19, "did:layerx:zz", "did:paxeer:" + "aa" * 32, "bb" * 31, "agent:did:layerx:alice:main"):
            with self.assertRaises(ValueError):
                px_account_key(refused)

    def test_quantities_decode_exactly_and_never_as_floats(self) -> None:
        self.assertEqual(px_quantity("500000"), 500000)
        self.assertEqual(px_quantity("0x1e"), 30)
        self.assertEqual(px_quantity("0X1E"), 30)
        self.assertEqual(px_quantity(0), 0)
        self.assertEqual(px_quantity(16), 16)
        self.assertEqual(px_quantity("340282366920938463463374607431768211455"), 340282366920938463463374607431768211455)
        for refused in ("", "0x", "-1", "1.5", "007", "0xgg", "0x" + "f" * 33, "340282366920938463463374607431768211456", 1.5, -1, None, True, {}):
            with self.assertRaises(ValueError):
                px_quantity(refused)
        self.assertIsNone(px_optional_quantity(None))
        self.assertEqual(px_optional_quantity("0x00"), 0)

    def test_balances_decode_keeps_an_unknown_half_unknown(self) -> None:
        decoded = decode_px_account_balances(BALANCES)
        self.assertEqual(decoded.account.evm_address, EVM)
        self.assertEqual(decoded.account.layerx_did, DID)
        self.assertEqual(decoded.account.layerx_account, ACCOUNT)
        self.assertIs(decoded.account.bound, True)
        self.assertEqual(decoded.joined_limit, 16)
        self.assertEqual(len(decoded.balances), 2)

        joined = decoded.balances[0]
        self.assertEqual(joined.asset_id, NATIVE)
        self.assertEqual(joined.denom, "ulxp")
        self.assertEqual(joined.paxeer, PxPaxeerAssetBalance("ulxp", 30))
        self.assertEqual(joined.custody.custodied, 500000)
        self.assertEqual(joined.custody.pending, 0)
        self.assertIs(joined.custody.enabled, True)
        self.assertIs(joined.custody.paused, False)
        self.assertEqual(joined.layerx, {"balance": "12"})

        unknown = decoded.balances[1]
        self.assertIsNone(unknown.denom)
        self.assertNotEqual(unknown.denom, "")
        self.assertIsNone(unknown.custody)
        self.assertIsNone(unknown.paxeer)
        self.assertNotEqual(unknown.paxeer, 0)
        self.assertIsNone(unknown.layerx)
        self.assertNotEqual(unknown.layerx, 0)

        with self.assertRaises(ValueError):
            decode_px_account_balances({**BALANCES, "balances": [{"asset_id": NATIVE, "denom": None, "custody": None, "paxeer": None}]})
        with self.assertRaises(ValueError):
            decode_px_account_balances({**BALANCES, "joined_limit": None})
        with self.assertRaises(ValueError):
            decode_px_account_balances({"account": IDENTITIES, "joined_limit": 16})
        with self.assertRaises(ValueError):
            decode_px_account_balances({**BALANCES, "balances": [BALANCES["balances"][1]] * (PX_MAXIMUM_JOINED_ASSETS + 1)})
        with self.assertRaises(ValueError):
            decode_px_resolved_identities({**IDENTITIES, "bound": "true"})
        with self.assertRaises(ValueError):
            decode_px_resolved_identities({**IDENTITIES, "evm_address": "0x" + "11" * 32})
        silent = decode_px_resolved_identities({**IDENTITIES, "evm_address": None, "layerx_account": None, "bound": False})
        self.assertIsNone(silent.evm_address)
        self.assertIsNone(silent.layerx_account)
        self.assertEqual(PX_MAXIMUM_JOINED_ASSETS, 1024)

    def test_join_asset_table_and_network_head_decode(self) -> None:
        join = decode_px_account_join({
            "account": IDENTITIES,
            "paxeer": {"address": EVM, "balance": "0x0de0b6b3a7640000", "nonce": "0x07"},
            "layerx": {"sequence": "3"},
        })
        self.assertEqual(join.paxeer.balance, 1000000000000000000)
        self.assertEqual(join.paxeer.nonce, 7)
        self.assertEqual(join.layerx, {"sequence": "3"})
        half = decode_px_account_join({"account": IDENTITIES, "paxeer": None, "layerx": None})
        self.assertIsNone(half.paxeer)
        self.assertIsNone(half.layerx)

        assets = decode_px_asset_table({
            "assets": [
                {"asset_id": NATIVE, "layerx": {"symbol": "LXP"}, "paxeer": CUSTODY},
                {"asset_id": UNJOINED, "layerx": None, "paxeer": None},
            ],
            "joined_limit": 16,
        })
        self.assertEqual(assets.joined_limit, 16)
        self.assertEqual(assets.assets[0].paxeer.denom, "ulxp")
        self.assertIsNone(assets.assets[1].paxeer)
        self.assertIsNone(assets.assets[1].layerx)

        network = decode_px_network_head({
            "network_id": "layerx-beta",
            "paxeer": {"chain_id": "0x1a4", "latest_block": "0x2b67"},
            "layerx": {"node_info": {"network": "layerx-beta"}},
            "anchor": {
                "latest_finalized_batch": 41,
                "status": 2,
                "status_name": "final",
                "status_ladder": {"0": "unknown", "1": "submitted", "2": "final"},
            },
        })
        self.assertEqual(network.network_id, "layerx-beta")
        self.assertEqual(network.paxeer.chain_id, 420)
        self.assertEqual(network.paxeer.latest_block, 11111)
        self.assertEqual(network.layerx, {"node_info": {"network": "layerx-beta"}})
        self.assertEqual(network.anchor.latest_finalized_batch, 41)
        self.assertEqual(network.anchor.status, 2)
        self.assertEqual(network.anchor.status_name, "final")
        self.assertEqual(network.anchor.status_ladder["1"], "submitted")

        quiet = decode_px_network_head({
            "network_id": "layerx-beta",
            "paxeer": {"chain_id": "0x1a4", "latest_block": "0x2b67"},
            "layerx": {"node_info": None},
            "anchor": {"latest_finalized_batch": None, "status": None, "status_name": None, "status_ladder": None},
        })
        self.assertIsNone(quiet.anchor.latest_finalized_batch)
        self.assertNotEqual(quiet.anchor.latest_finalized_batch, 0)
        self.assertIsNone(quiet.anchor.status)
        self.assertNotEqual(quiet.anchor.status, 0)
        self.assertIsNone(quiet.anchor.status_name)
        self.assertNotEqual(quiet.anchor.status_name, "")
        self.assertIsNone(quiet.anchor.status_ladder)
        self.assertEqual(quiet.layerx, {"node_info": None})
        self.assertIsNone(decode_px_network_head({
            "network_id": "layerx-beta",
            "paxeer": {"chain_id": "0x1a4", "latest_block": "0x2b67"},
            "layerx": None,
            "anchor": None,
        }).anchor)
        with self.assertRaises(ValueError):
            decode_px_network_head({"network_id": "layerx-beta", "paxeer": {"chain_id": "0x1a4"}, "layerx": None, "anchor": None})
        with self.assertRaises(ValueError):
            decode_px_network_head({"network_id": "", "paxeer": {"chain_id": "0x1a4", "latest_block": "0x1"}, "layerx": None, "anchor": None})

    def test_gateway_reads_and_json_rpc_errors_over_loopback(self) -> None:
        observed: list[dict[str, object]] = []
        server = ThreadingHTTPServer(("127.0.0.1", 0), gateway_handler(observed))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            client = PxClient(f"http://127.0.0.1:{server.server_port}/rpc")
            table = client.get_balances(ACCOUNT.upper())
            self.assertEqual(len(table.balances), 2)
            self.assertEqual(table.balances[0].paxeer.amount, 30)
            self.assertIsNone(table.balances[1].paxeer)
            self.assertIsNone(table.balances[1].denom)
            self.assertEqual(observed[0]["path"], "/rpc")
            self.assertEqual(observed[0]["content_type"], "application/json")
            self.assertEqual(observed[0]["jsonrpc"], "2.0")
            self.assertEqual(observed[0]["method"], "px_getBalances")
            self.assertEqual(observed[0]["params"], [ACCOUNT])

            with self.assertRaises(PxRpcError) as caught:
                client.get_network()
            self.assertEqual(caught.exception.code, -32001)
            self.assertEqual(caught.exception.message, "Paxeer read unavailable")
            self.assertEqual(caught.exception.data, {"code": "paxeer_unreachable"})
            for call in (client.list_assets, lambda: client.resolve_account(DID), lambda: client.get_account(EVM)):
                with self.assertRaises(PxRpcError):
                    call()
            self.assertEqual(
                [entry["method"] for entry in observed],
                ["px_getBalances", "px_getNetwork", "px_listAssets", "px_resolveAccount", "px_getAccount"],
            )
            self.assertEqual(observed[1]["params"], [])
            self.assertEqual(observed[3]["params"], [DID])
            self.assertEqual(observed[4]["params"], [EVM])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_client_refuses_unsafe_endpoints_and_unknown_methods(self) -> None:
        for endpoint in (
            "http://example.com/rpc",
            "https://user:secret@example.com/rpc",
            "wss://example.com/rpc",
            "https://example.com/rpc#head",
            "https://example.com/rpc?trace=1",
        ):
            with self.assertRaises(ValueError):
                PxClient(endpoint)
        with self.assertRaises(ValueError):
            PxClient("https://example.com/rpc", timeout=0)
        with self.assertRaises(ValueError):
            PxClient("https://example.com/rpc").call("px_unknown", [])
        with self.assertRaises(ValueError):
            PxClient("https://example.com/rpc").get_balances("not-an-account")


# Recorded gateway answers for the three history reads, as the gateway joins the
# indexer's rows with its /v1/assets records and the LayerX registry.
LXP = "11" * 32
ALICE = "aa" * 32
EVE = "0x" + "ee" * 20
FRANK = "0x" + "ff" * 20
POINTER = "0x" + "cc" * 20
LXP_METADATA = {
    "asset": LXP, "chain": "layerx", "kind": "layerx", "address": None, "denom": None, "symbol": "LXP",
    "decimals": 6, "native_id": LXP, "pointer": None, "metadata": {"supply": "100"},
}
POINTER_METADATA = {
    "asset": "evm:" + POINTER, "chain": "paxeer", "kind": "pointer", "address": POINTER, "denom": "ulxp",
    "symbol": "LXP", "decimals": 6, "native_id": LXP, "pointer": POINTER, "metadata": {},
}
NATIVE_METADATA = {
    "asset": "evm:native", "chain": "paxeer", "kind": "native", "address": None, "denom": None, "symbol": None,
    "decimals": 18, "native_id": None, "pointer": None, "metadata": {},
}


def history_row(
    row_id: str, height: str, chain: str, kind: str, direction: str, account: str, counterparty: str | None,
    asset: str, amount: str, metadata: dict[str, object] | None,
) -> dict[str, object]:
    return {
        "id": row_id, "height_or_seq": height, "chain": chain, "kind": kind, "direction": direction,
        "account": account, "counterparty": counterparty, "asset": asset, "amount": amount,
        "tx_id": f"{kind}-{height}", "ordinal": "0", "final": chain == "layerx",
        "decoded": {"position": height}, "asset_metadata": metadata,
    }


CUSTODY_ROW = history_row("8", "11", "paxeer", "custody_deposit", "in", ALICE, EVE, LXP, "9", LXP_METADATA)
POINTER_ROW = history_row(
    "6", "11", "paxeer", "pointer_transfer", "in", EVE, FRANK, "evm:" + POINTER, "3", POINTER_METADATA,
)
CREDIT_ROW = history_row("5", "2", "layerx", "lxp_credit", "in", ALICE, None, LXP, "7", LXP_METADATA)
NATIVE_ROW = history_row(
    "3", "10", "paxeer", "native_transfer", "out", EVE, FRANK, "evm:native", "1000000000000000000", NATIVE_METADATA,
)
UNKNOWN_ROW = history_row(
    "11", "12", "paxeer", "erc20_transfer", "in", EVE, FRANK, "evm:0x" + "00" * 18 + "0abc", str((1 << 256) - 1), None,
)
RECORDED_LAYERX = {"items": [CUSTODY_ROW, CREDIT_ROW], "next_cursor": "5", "account": ALICE}
RECORDED_PAXEER = {"items": [UNKNOWN_ROW, POINTER_ROW, NATIVE_ROW], "next_cursor": None, "account": EVE}
RECORDED_UNIFIED = {
    "items": [{**CUSTODY_ROW, "side": "layerx"}, {**POINTER_ROW, "side": "paxeer"}, {**CREDIT_ROW, "side": "layerx"}],
    "next_cursor": "5",
    "account": IDENTITIES,
    "accounts": [{"side": "paxeer", "account": EVE}, {"side": "layerx", "account": ALICE}],
}


def history_handler(observed: list[dict[str, object]]) -> type[BaseHTTPRequestHandler]:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            length = int(self.headers["Content-Length"])
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            observed.append({"method": body["method"], "params": body["params"]})
            result = {
                "lx_getHistory": RECORDED_LAYERX,
                "px_getHistory": RECORDED_PAXEER,
            }.get(body["method"], RECORDED_UNIFIED)
            encoded = json.dumps({"jsonrpc": "2.0", "id": body["id"], "result": result}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)

        def log_message(self, _format: str, *args: object) -> None:
            del args

    return Handler


class HistoryReadTest(unittest.TestCase):
    def test_history_params_are_positional_and_bounded(self) -> None:
        self.assertEqual(history_params(ALICE), [ALICE, None, None, None])
        self.assertEqual(
            history_params(EVE, cursor="42", limit=7, kind="erc20_transfer"), [EVE, "42", 7, "erc20_transfer"],
        )
        for refused in (
            {"cursor": ""}, {"cursor": "1&limit=500"}, {"cursor": "x" * 33}, {"limit": 0}, {"limit": 101},
            {"limit": True}, {"kind": "Transfer"}, {"kind": ""},
        ):
            with self.assertRaises(ValueError):
                history_params(EVE, **refused)  # type: ignore[arg-type]
        self.assertEqual(HISTORY_DEFAULT_LIMIT, 50)

    def test_recorded_pages_decode_rows_and_joined_asset_metadata(self) -> None:
        layerx = decode_layerx_history_page(RECORDED_LAYERX)
        self.assertEqual(layerx.account, ALICE)
        self.assertEqual(layerx.next_cursor, "5")
        self.assertEqual([row.id for row in layerx.items], [8, 5])
        self.assertEqual(layerx.items[0].chain, "paxeer")
        self.assertIsNone(layerx.items[0].side)
        credit = layerx.items[1]
        self.assertIsNone(credit.counterparty)
        self.assertEqual(credit.amount, 7)
        self.assertTrue(credit.final)
        assert credit.asset_metadata is not None
        self.assertEqual(credit.asset_metadata.symbol, "LXP")
        self.assertEqual(credit.asset_metadata.decimals, 6)
        self.assertEqual(credit.asset_metadata.native_id, LXP)
        self.assertEqual(credit.asset_metadata.metadata, {"supply": "100"})

        paxeer = decode_paxeer_history_page(RECORDED_PAXEER)
        self.assertIsNone(paxeer.next_cursor)
        self.assertIsNone(paxeer.items[0].asset_metadata)
        self.assertEqual(paxeer.items[0].amount, (1 << 256) - 1)
        pointer = paxeer.items[1].asset_metadata
        assert pointer is not None
        self.assertEqual(pointer.pointer, POINTER)
        self.assertEqual(pointer.denom, "ulxp")
        native = paxeer.items[2].asset_metadata
        assert native is not None
        self.assertEqual(native.decimals, 18)
        self.assertIsNone(native.symbol)
        self.assertEqual(paxeer.items[2].direction, "out")

        unified = decode_unified_history_page(RECORDED_UNIFIED)
        self.assertEqual(
            [(row.id, row.side, row.chain) for row in unified.items],
            [(8, "layerx", "paxeer"), (6, "paxeer", "paxeer"), (5, "layerx", "layerx")],
        )
        self.assertEqual([(side.side, side.account) for side in unified.accounts], [("paxeer", EVE), ("layerx", ALICE)])
        self.assertEqual(unified.next_cursor, "5")
        self.assertEqual(unified.account["evm_address"], EVM)
        self.assertEqual(decode_layerx_history_page({"items": [], "next_cursor": None, "account": ALICE}).items, ())

    def test_malformed_pages_are_refused(self) -> None:
        items = RECORDED_UNIFIED["items"]
        assert isinstance(items, list)
        refusals = (
            lambda: decode_unified_history_page({**RECORDED_UNIFIED, "items": [items[2], items[0]]}),
            lambda: decode_unified_history_page({**RECORDED_UNIFIED, "items": [CUSTODY_ROW]}),
            lambda: decode_layerx_history_page({**RECORDED_LAYERX, "account": EVE}),
            lambda: decode_paxeer_history_page({**RECORDED_PAXEER, "next_cursor": "1&x"}),
            lambda: decode_layerx_history_page({"items": [], "next_cursor": "5", "account": ALICE}),
            lambda: decode_layerx_history_page({**RECORDED_LAYERX, "items": [{**CREDIT_ROW, "amount": "-7"}]}),
            lambda: decode_layerx_history_page({**RECORDED_LAYERX, "items": [{**CREDIT_ROW, "direction": "sideways"}]}),
            lambda: decode_layerx_history_page({
                **RECORDED_LAYERX, "items": [{**CREDIT_ROW, "asset_metadata": {**LXP_METADATA, "asset": "evm:native"}}],
            }),
            lambda: decode_layerx_history_page(RECORDED_LAYERX, 1),
            lambda: decode_history_asset_metadata({**NATIVE_METADATA, "decimals": 256}),
        )
        for refusal in refusals:
            with self.assertRaises(ValueError):
                refusal()

    def test_history_reads_round_trip_over_loopback(self) -> None:
        observed: list[dict[str, object]] = []
        server = ThreadingHTTPServer(("127.0.0.1", 0), history_handler(observed))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            client = PxClient(f"http://127.0.0.1:{server.server_port}/rpc")
            unified = client.get_unified_history(DID.upper().replace("DID:LAYERX:", "did:layerx:"), limit=3)
            self.assertEqual([row.id for row in unified.items], [8, 6, 5])
            paxeer = client.get_history(EVE.upper().replace("0X", "0x"), kind="erc20_transfer")
            self.assertEqual(len(paxeer.items), 3)
            layerx = client.get_layerx_history(ALICE, cursor="9", limit=2)
            self.assertEqual(layerx.next_cursor, "5")
            with self.assertRaises(ValueError):
                client.get_unified_history(EVE, limit=2)
            with self.assertRaises(ValueError):
                client.get_history(ALICE)
            with self.assertRaises(ValueError):
                client.get_layerx_history(EVE)
            self.assertEqual(observed, [
                {"method": "px_getUnifiedHistory", "params": [DID, None, 3, None]},
                {"method": "px_getHistory", "params": [EVE, None, None, "erc20_transfer"]},
                {"method": "lx_getHistory", "params": [ALICE, "9", 2, None]},
                {"method": "px_getUnifiedHistory", "params": [EVE, None, 2, None]},
            ])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
