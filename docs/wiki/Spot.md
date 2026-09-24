# Spot module

Module ID `10` (`LXP_MODULE_SPOT`). Activity types are `0x000Axxxx`
(`include/layerx/lx_spot.h`). Spot reuses the perps price-time matching
engine for two-asset limit and market orders on a book of markets it owns,
settling both legs of every fill immediately through the asset module.

Sources: `include/layerx/lx_spot.h`, `src/modules/spot/`,
`tests/modules/test_spot_book.c`.

---

## Activities

| Constant | Value | Role |
| --- | --- | --- |
| `LX_SPOT_MARKET_CREATE` | `0x000A0001` | Create a market for a base/quote asset pair |
| `LX_SPOT_ORDER_PLACE` | `0x000A0002` | Place a limit or market order |
| `LX_SPOT_ORDER_CANCEL` | `0x000A0003` | Cancel a resting order |
| `LX_SPOT_MARKET_HALT` | `0x000A0004` | Halt trading on a market |
| `LX_SPOT_MARKET_RESUME` | `0x000A0005` | Resume a halted market |

Up to `LX_SPOT_MARKET_CAPACITY` (64) markets and `LX_SPOT_BOOK_CAPACITY`
(256) resting orders per market are supported; capacities are compile-time
constants.

---

## Markets

`MARKET_CREATE`'s 160-byte payload (`LX_SPOT_MARKET_PAYLOAD_BYTES`) is
`market_id32 || base_asset32 || quote_asset32 || tick_size:u128be ||
lot_size:u128be || administrator32` (`src/modules/spot/lx_spot_market.c:52-67`).
A market that already exists refuses with `LXP_ERR_MARKET_ALREADY_EXISTS`.
On decode, `base_escrow_id` and `quote_escrow_id` are derived with
`lx_spot_escrow_id(market_id, asset)` rather than carried on the wire
(`src/modules/spot/lx_spot_market.c:87-92`). `MARKET_HALT` and
`MARKET_RESUME` take a 32-byte market-id-only payload
(`LX_SPOT_MARKET_ID_PAYLOAD_BYTES`) and flip the market's `halted` flag;
placing an order against a halted market refuses with
`LXP_ERR_MARKET_HALTED`.

## Orders

`ORDER_PLACE`'s 163-byte payload (`LX_SPOT_ORDER_PAYLOAD_BYTES`) is
`market_id32 || order_id32 || base_account_id32 || quote_account_id32 ||
side:u8 || kind:u8 || time_in_force:u8 || price:u128be || quantity:u128be`
(`src/modules/spot/lx_spot_market.c:117-134`). `side` aliases the perps
buy/sell enum (`LX_SPOT_SIDE_BID`/`LX_SPOT_SIDE_ASK`). `kind` is `1` limit
or `2` market; a limit order needs a nonzero price and time-in-force `1`
GTC or `2` IOC, a market order needs a zero price and is always IOC
(`src/modules/spot/lx_spot_market.c:97-115`). `ORDER_CANCEL`'s 64-byte
payload is `market_id32 || order_id32`.

Incoming orders match against the resting book through
`lx_spot_book_match`, the same price-time engine perps uses: best price,
then earliest global sequence, then order id. Every fill trades at the
maker's price and draws down the maker's remaining quantity and escrow; a
fully filled maker leaves the book. A resting order escrows what it still
needs to settle - base units for an ask, quote units (price times
remaining) for a bid (`include/layerx/lx_spot.h:92-106`). One activity
produces at most `LX_SPOT_DISPATCH_MAX_FILLS` (8) fills and
`LX_SPOT_DISPATCH_MAX_LEGS` (18) transfer legs, sized to fit one transfer
set (`LXP_MAX_TRANSFER_SET_LEGS`).

## Settlement

Every fill settles both legs immediately through the asset module as
`402LXP` transfer legs; spot never holds a shadow balance. Refusals include
`LXP_ERR_ASSET_MISMATCH`, `LXP_ERR_ASSET_PAUSED`,
`LXP_ERR_UNAUTHORIZED_DEBIT`, `LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE`,
`LXP_ERR_SEQUENCE_REUSED`, `LXP_ERR_NON_CANONICAL`, `LXP_ERR_LENGTH_LIMIT`,
`LXP_ERR_UNKNOWN_FIELD` and `LXP_ERR_UNKNOWN_ACTIVITY`
(`src/modules/spot/lx_spot_dispatch.c`). `tests/modules/test_spot_book.c`
(Makefile target `test-spot-book`) covers book matching and a conservation
check.

The module runtime is bound alongside perps and service through
`lxp_daemon_module_runtimes_bind` (`cmd/layerxd/lxp_daemon_modules.h`) and
is enabled in genesis through `platform/hosted/node/genesis-modules.conf`.

Spot is a LayerX kernel module executing inside `layerxd`. It is a
different surface from the Paxeer-side `layerxexchange` precompile and Go
module (EVM address `0x1015`), which takes orders from the Paxeer chain;
see [Unified Network Architecture](UnifiedNetwork.md). The protocol
specification in `spec/layerx-beta/spec.kvx` describes this module under
module id 7; the shipped module id is `10` because ids 7-9 were already
assigned to governance, bridge and programs.

---

## Start here

- [Modules](Modules.md)
- [Perps](Perps.md)
- [Asset](Asset.md)
- [Home](Home.md)
