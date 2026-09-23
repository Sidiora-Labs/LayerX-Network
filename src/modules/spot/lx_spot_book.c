#include "layerx/lx_spot.h"

#include <stdlib.h>
#include <string.h>

static void perps_order_project(const lx_spot_order *order,
                                lx_perps_order *projected)
{
    (void)memset(projected, 0, sizeof(*projected));
    (void)memcpy(projected->order_id, order->order_id, 32U);
    (void)memcpy(projected->market_id, order->market_id, 32U);
    (void)memcpy(projected->owner_account_id, order->base_account_id, 32U);
    projected->side = (lx_perps_side)order->side;
    projected->price = order->price;
    projected->quantity = order->quantity;
    projected->remaining = order->remaining;
    projected->global_sequence = order->global_sequence;
    projected->active = true;
}

static lx_spot_order *book_find(lx_spot_book *book,
                                const uint8_t order_id[32])
{
    size_t i;
    for (i = 0U; i < book->count; ++i)
        if (memcmp(book->orders[i].order_id, order_id, 32U) == 0)
            return &book->orders[i];
    return NULL;
}

static lxp_result maker_draw(lx_spot_order *maker, const lx_perps_fill *fill,
                             lx_spot_fill *out)
{
    lxp_u128 notional;
    lxp_u128 escrow_draw;
    lxp_result status;
    if (lxp_u128_cmp(fill->price, maker->price) != 0)
        return LXP_FATAL_INVARIANT;
    status = lx_spot_notional(fill->price, fill->quantity, &notional);
    if (status != LXP_OK) return status;
    escrow_draw = maker->side == LX_SPOT_SIDE_ASK ? fill->quantity : notional;
    if (lxp_u128_sub(maker->remaining, fill->quantity, &maker->remaining) !=
            LXP_OK ||
        lxp_u128_sub(maker->escrowed, escrow_draw, &maker->escrowed) !=
            LXP_OK)
        return LXP_FATAL_INVARIANT;
    if (lxp_u128_is_zero(maker->remaining) !=
        lxp_u128_is_zero(maker->escrowed))
        return LXP_FATAL_INVARIANT;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->maker_order_id, maker->order_id, 32U);
    (void)memcpy(out->maker_base_account_id, maker->base_account_id, 32U);
    (void)memcpy(out->maker_quote_account_id, maker->quote_account_id, 32U);
    out->maker_side = maker->side;
    out->price = fill->price;
    out->quantity = fill->quantity;
    out->notional = notional;
    return LXP_OK;
}

lxp_result lx_spot_book_match(lx_spot_book *book, lx_spot_order *incoming,
                              lx_spot_fill *fills, size_t fill_capacity,
                              size_t *fill_count)
{
    lx_perps_book *engine;
    lx_perps_fill perps_fills[LX_SPOT_DISPATCH_MAX_FILLS];
    lx_perps_order taker;
    size_t count = 0U;
    size_t i;
    size_t write_index = 0U;
    lxp_result status;
    if (book == NULL || incoming == NULL || fill_count == NULL ||
        book->count > LX_SPOT_BOOK_CAPACITY ||
        (fills == NULL && fill_capacity != 0U) ||
        fill_capacity > LX_SPOT_DISPATCH_MAX_FILLS ||
        (incoming->side != LX_SPOT_SIDE_BID &&
         incoming->side != LX_SPOT_SIDE_ASK))
        return LXP_ERR_NON_CANONICAL;
    engine = (lx_perps_book *)malloc(sizeof(*engine));
    if (engine == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    status = lx_perps_book_init(engine);
    for (i = 0U; status == LXP_OK && i < book->count; ++i) {
        if (memcmp(book->orders[i].market_id, incoming->market_id, 32U) != 0)
            status = LXP_ERR_NON_CANONICAL;
        else
            perps_order_project(&book->orders[i], &engine->orders[i]);
    }
    engine->count = book->count;
    perps_order_project(incoming, &taker);
    if (status == LXP_OK)
        status = lx_perps_book_match(engine, &taker, perps_fills,
                                     fill_capacity, &count);
    free(engine);
    if (status != LXP_OK) return status;
    for (i = 0U; i < count; ++i) {
        lx_spot_order *maker = book_find(book, perps_fills[i].maker_order_id);
        if (maker == NULL) return LXP_FATAL_INVARIANT;
        status = maker_draw(maker, &perps_fills[i], &fills[i]);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < book->count; ++i) {
        if (lxp_u128_is_zero(book->orders[i].remaining)) continue;
        if (write_index != i) book->orders[write_index] = book->orders[i];
        ++write_index;
    }
    if (write_index < book->count)
        (void)memset(&book->orders[write_index], 0,
                     (book->count - write_index) * sizeof(book->orders[0]));
    book->count = write_index;
    incoming->remaining = taker.remaining;
    *fill_count = count;
    return LXP_OK;
}
