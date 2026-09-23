#ifndef LAYERX_LX_SPOT_H
#define LAYERX_LX_SPOT_H

#include "layerx/lx_perps.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_transfer.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_SPOT_MARKET_CREATE = (LXP_MODULE_SPOT << 16) | 0x0001,
    LX_SPOT_ORDER_PLACE = (LXP_MODULE_SPOT << 16) | 0x0002,
    LX_SPOT_ORDER_CANCEL = (LXP_MODULE_SPOT << 16) | 0x0003,
    LX_SPOT_MARKET_HALT = (LXP_MODULE_SPOT << 16) | 0x0004,
    LX_SPOT_MARKET_RESUME = (LXP_MODULE_SPOT << 16) | 0x0005,
    LX_SPOT_ORDINAL_COUNT = 5,
    LX_SPOT_MARKET_CAPACITY = 64,
    LX_SPOT_BOOK_CAPACITY = 256,
    LX_SPOT_DISPATCH_MAX_FILLS = 8,
    LX_SPOT_DISPATCH_MAX_LEGS = 2 * LX_SPOT_DISPATCH_MAX_FILLS + 2,
    LX_SPOT_MARKET_PAYLOAD_BYTES = 160,
    LX_SPOT_MARKET_KEY_BYTES = 39,
    LX_SPOT_MARKET_BYTES = 225,
    LX_SPOT_ORDER_PAYLOAD_BYTES = 163,
    LX_SPOT_ORDER_KEY_BYTES = 70,
    LX_SPOT_ORDER_BYTES = 201,
    LX_SPOT_CANCEL_PAYLOAD_BYTES = 64,
    LX_SPOT_MARKET_ID_PAYLOAD_BYTES = 32,
    LX_SPOT_EVENT_MARKET_CREATED = (LXP_MODULE_SPOT << 8) | 0x01,
    LX_SPOT_EVENT_ORDER_PLACED = (LXP_MODULE_SPOT << 8) | 0x02,
    LX_SPOT_EVENT_ORDER_CANCELLED = (LXP_MODULE_SPOT << 8) | 0x03,
    LX_SPOT_EVENT_MARKET_HALTED = (LXP_MODULE_SPOT << 8) | 0x04,
    LX_SPOT_EVENT_MARKET_RESUMED = (LXP_MODULE_SPOT << 8) | 0x05
};

_Static_assert((int)LX_SPOT_BOOK_CAPACITY <= (int)LX_PERPS_BOOK_CAPACITY,
               "spot book fits the shared matching engine");
_Static_assert((int)LX_SPOT_DISPATCH_MAX_LEGS <= (int)LXP_MAX_TRANSFER_SET_LEGS,
               "spot settlement fits one transfer set");

typedef enum lx_spot_side {
    LX_SPOT_SIDE_BID = LX_PERPS_SIDE_BUY,
    LX_SPOT_SIDE_ASK = LX_PERPS_SIDE_SELL
} lx_spot_side;

typedef enum lx_spot_order_kind {
    LX_SPOT_ORDER_LIMIT = 1,
    LX_SPOT_ORDER_MARKET = 2
} lx_spot_order_kind;

typedef enum lx_spot_time_in_force {
    LX_SPOT_TIF_GTC = 1,
    LX_SPOT_TIF_IOC = 2
} lx_spot_time_in_force;

typedef struct lx_spot_market {
    uint8_t market_id[32];
    uint8_t base_asset[32];
    uint8_t quote_asset[32];
    uint8_t administrator[32];
    lxp_u128 tick_size;
    lxp_u128 lot_size;
    uint8_t base_escrow_id[32];
    uint8_t quote_escrow_id[32];
    bool halted;
} lx_spot_market;

typedef struct lx_spot_order_command {
    uint8_t market_id[32];
    uint8_t order_id[32];
    uint8_t base_account_id[32];
    uint8_t quote_account_id[32];
    lx_spot_side side;
    lx_spot_order_kind kind;
    lx_spot_time_in_force time_in_force;
    lxp_u128 price;
    lxp_u128 quantity;
} lx_spot_order_command;

typedef struct lx_spot_cancel_command {
    uint8_t market_id[32];
    uint8_t order_id[32];
} lx_spot_cancel_command;

typedef struct lx_spot_market_command {
    uint8_t market_id[32];
} lx_spot_market_command;

/* A resting order. escrowed is what the module's escrow account still holds
 * for it: base units for an ask, quote units (price times remaining) for a
 * bid. */
typedef struct lx_spot_order {
    uint8_t order_id[32];
    uint8_t market_id[32];
    uint8_t base_account_id[32];
    uint8_t quote_account_id[32];
    lx_spot_side side;
    lxp_u128 price;
    lxp_u128 quantity;
    lxp_u128 remaining;
    lxp_u128 escrowed;
    uint64_t global_sequence;
} lx_spot_order;

typedef struct lx_spot_book {
    lx_spot_order orders[LX_SPOT_BOOK_CAPACITY];
    size_t count;
} lx_spot_book;

typedef struct lx_spot_fill {
    uint8_t maker_order_id[32];
    uint8_t maker_base_account_id[32];
    uint8_t maker_quote_account_id[32];
    lx_spot_side maker_side;
    lxp_u128 price;
    lxp_u128 quantity;
    lxp_u128 notional;
} lx_spot_fill;

const lxp_module_iface *lx_spot_module_iface(void);

lxp_result lx_spot_market_encode(const lx_spot_market *market,
                                 uint8_t payload[LX_SPOT_MARKET_PAYLOAD_BYTES]);
lxp_result lx_spot_market_decode(const uint8_t *payload, size_t length,
                                 lx_spot_market *market);
lxp_result lx_spot_order_command_encode(
    const lx_spot_order_command *command,
    uint8_t payload[LX_SPOT_ORDER_PAYLOAD_BYTES]);
lxp_result lx_spot_order_command_decode(const uint8_t *payload, size_t length,
                                        lx_spot_order_command *command);
lxp_result lx_spot_cancel_command_encode(
    const lx_spot_cancel_command *command,
    uint8_t payload[LX_SPOT_CANCEL_PAYLOAD_BYTES]);
lxp_result lx_spot_cancel_command_decode(const uint8_t *payload, size_t length,
                                         lx_spot_cancel_command *command);
lxp_result lx_spot_market_command_encode(
    const lx_spot_market_command *command,
    uint8_t payload[LX_SPOT_MARKET_ID_PAYLOAD_BYTES]);
lxp_result lx_spot_market_command_decode(const uint8_t *payload,
                                         size_t length,
                                         lx_spot_market_command *command);

lxp_result lx_spot_escrow_id(const uint8_t market_id[32],
                             const uint8_t asset_id[32], uint8_t out[32]);
lxp_result lx_spot_notional(lxp_u128 price, lxp_u128 quantity,
                            lxp_u128 *notional);

lxp_result lx_spot_market_lookup(lxp_module_ctx *ctx,
                                 const uint8_t market_id[32],
                                 lx_spot_market *market);
lxp_result lx_spot_market_put(lxp_module_ctx *ctx,
                              const lx_spot_market *market);
lxp_result lx_spot_market_count(lxp_module_ctx *ctx, size_t *count);
lxp_result lx_spot_order_lookup(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                const uint8_t order_id[32],
                                lx_spot_order *order);
lxp_result lx_spot_order_put(lxp_module_ctx *ctx, const lx_spot_order *order);
lxp_result lx_spot_order_delete(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                const uint8_t order_id[32]);
lxp_result lx_spot_book_load(lxp_module_ctx *ctx, const uint8_t market_id[32],
                             lx_spot_book *book);
lxp_result lx_spot_book_persist(lxp_module_ctx *ctx,
                                const lx_spot_book *before,
                                const lx_spot_book *after);

/* Matches incoming against the book through the perps price-time engine
 * (best price, then earliest global sequence, then order id). Every fill
 * trades at the maker's price and draws down the maker's remaining quantity
 * and escrow; fully filled makers leave the book. incoming->remaining is
 * reduced by the filled quantity; its escrow is left to the caller. */
lxp_result lx_spot_book_match(lx_spot_book *book, lx_spot_order *incoming,
                              lx_spot_fill *fills, size_t fill_capacity,
                              size_t *fill_count);

/* Executes one decoded spot activity against ctx. It is the dispatch entry
 * behind the module interface's execute hook. */
lxp_result lx_spot_dispatch(lxp_module_ctx *ctx, const lxp_activity *activity,
                            const lxp_authority_resolved *authority,
                            uint16_t ordinal, const void *command);

#endif
