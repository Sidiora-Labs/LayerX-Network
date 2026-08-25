#include "layerx/lx_perps.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static int u128_compare(lxp_u128 left, lxp_u128 right)
{
    return lxp_u128_cmp(left, right);
}

static int order_key_compare(const lx_perps_order *left,
                             const lx_perps_order *right)
{
    int comparison = memcmp(left->market_id, right->market_id, 32U);
    if (comparison != 0) return comparison;
    if (left->side != right->side)
        return left->side < right->side ? -1 : 1;
    comparison = u128_compare(left->price, right->price);
    if (comparison != 0) return comparison;
    if (left->global_sequence != right->global_sequence)
        return left->global_sequence < right->global_sequence ? -1 : 1;
    return memcmp(left->order_id, right->order_id, 32U);
}

static void sort_book(lx_perps_book *book)
{
    size_t i;
    for (i = 1U; i < book->count; ++i) {
        lx_perps_order value = book->orders[i];
        size_t position = i;
        while (position != 0U &&
               order_key_compare(&value, &book->orders[position - 1U]) < 0) {
            book->orders[position] = book->orders[position - 1U];
            --position;
        }
        book->orders[position] = value;
    }
}

lxp_result lx_perps_book_init(lx_perps_book *book)
{
    if (book == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(book, 0, sizeof(*book));
    return LXP_OK;
}

lxp_result lx_perps_book_iter(const lx_perps_book *book,
                              lx_perps_order_visit_fn visit, void *user)
{
    size_t i;
    if (book == NULL || visit == NULL ||
        book->count > LX_PERPS_BOOK_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < book->count; ++i) {
        lxp_result status = visit(&book->orders[i], user);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static bool crosses(const lx_perps_order *incoming,
                    const lx_perps_order *resting)
{
    if (incoming->side == LX_PERPS_SIDE_BUY)
        return lxp_u128_cmp(incoming->price, resting->price) >= 0;
    return lxp_u128_cmp(incoming->price, resting->price) <= 0;
}

static bool better_match(const lx_perps_order *incoming,
                         const lx_perps_order *candidate,
                         const lx_perps_order *current)
{
    int price_comparison = lxp_u128_cmp(candidate->price, current->price);
    if (price_comparison != 0)
        return incoming->side == LX_PERPS_SIDE_BUY ? price_comparison < 0 :
                                                    price_comparison > 0;
    if (candidate->global_sequence != current->global_sequence)
        return candidate->global_sequence < current->global_sequence;
    return memcmp(candidate->order_id, current->order_id, 32U) < 0;
}

static size_t best_match(const lx_perps_book *book,
                         const lx_perps_order *incoming)
{
    size_t best = book->count;
    size_t i;
    for (i = 0U; i < book->count; ++i) {
        const lx_perps_order *candidate = &book->orders[i];
        if (!candidate->active || candidate->side == incoming->side ||
            memcmp(candidate->market_id, incoming->market_id, 32U) != 0 ||
            !crosses(incoming, candidate))
            continue;
        if (best == book->count ||
            better_match(incoming, candidate, &book->orders[best]))
            best = i;
    }
    return best;
}

static void compact_book(lx_perps_book *book)
{
    size_t read_index;
    size_t write_index = 0U;
    for (read_index = 0U; read_index < book->count; ++read_index) {
        if (!book->orders[read_index].active ||
            lxp_u128_is_zero(book->orders[read_index].remaining))
            continue;
        if (write_index != read_index)
            book->orders[write_index] = book->orders[read_index];
        ++write_index;
    }
    if (write_index < book->count)
        (void)memset(&book->orders[write_index], 0,
                     (book->count - write_index) * sizeof(book->orders[0]));
    book->count = write_index;
    sort_book(book);
}

lxp_result lx_perps_book_match(lx_perps_book *book,
                               lx_perps_order *incoming,
                               lx_perps_fill *fills, size_t fill_capacity,
                               size_t *fill_count)
{
    size_t count = 0U;
    if (book == NULL || incoming == NULL || fill_count == NULL ||
        book->count > LX_PERPS_BOOK_CAPACITY ||
        (fills == NULL && fill_capacity != 0U))
        return LXP_ERR_NON_CANONICAL;
    while (!lxp_u128_is_zero(incoming->remaining)) {
        size_t index = best_match(book, incoming);
        lx_perps_order *maker;
        lx_perps_fill fill;
        lxp_u128 quantity;
        lxp_u128 maker_remaining;
        lxp_u128 taker_remaining;
        if (index == book->count) break;
        if (count == fill_capacity) return LXP_ERR_LENGTH_LIMIT;
        maker = &book->orders[index];
        quantity = lxp_u128_cmp(maker->remaining, incoming->remaining) < 0 ?
            maker->remaining : incoming->remaining;
        if (lxp_u128_sub(maker->remaining, quantity, &maker_remaining) !=
                LXP_OK ||
            lxp_u128_sub(incoming->remaining, quantity, &taker_remaining) !=
                LXP_OK)
            return LXP_FATAL_INVARIANT;
        (void)memset(&fill, 0, sizeof(fill));
        (void)memcpy(fill.maker_order_id, maker->order_id, 32U);
        (void)memcpy(fill.taker_order_id, incoming->order_id, 32U);
        fill.price = maker->price;
        fill.quantity = quantity;
        fill.maker_sequence = maker->global_sequence;
        fill.taker_sequence = incoming->global_sequence;
        fills[count++] = fill;
        maker->remaining = maker_remaining;
        incoming->remaining = taker_remaining;
        if (lxp_u128_is_zero(maker->remaining)) maker->active = false;
    }
    compact_book(book);
    *fill_count = count;
    return LXP_OK;
}

static lxp_result order_margin(const lx_perps_market *market,
                               const lx_perps_order *order,
                               lxp_u128 *required)
{
    lxp_u256 product;
    lxp_u128 notional;
    lxp_u128 residue;
    lxp_result status;
    status = lxp_u128_mul(order->price, order->quantity, &product);
    if (status != LXP_OK) return status;
    status = lxp_u256_div_floor(product, (lxp_u128){ 0U, 1U },
                                &notional, &residue);
    if (status != LXP_OK || !lxp_u128_is_zero(residue))
        return LXP_ERR_OVERFLOW;
    return lxp_u128_mul_bps_ceil(notional,
                                 market->initial_margin_ratio_bps, required);
}

static lxp_result order_validate(const lx_perps_market *market,
                                 const lx_perps_order *order,
                                 lxp_u128 available_margin,
                                 lxp_u128 *required)
{
    lxp_u256 product;
    lxp_u128 quotient;
    lxp_u128 residue;
    lxp_result status;
    if (market == NULL || order == NULL || required == NULL ||
        lxp_ct_is_zero(order->order_id, 32U) ||
        lxp_ct_is_zero(order->owner_account_id, 32U) ||
        memcmp(market->market_id, order->market_id, 32U) != 0 ||
        (order->side != LX_PERPS_SIDE_BUY &&
         order->side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(order->price) || lxp_u128_is_zero(order->quantity))
        return LXP_ERR_NON_CANONICAL;
    if (market->halted) return LXP_ERR_MARKET_HALTED;
    status = lxp_u128_mul(order->price, (lxp_u128){ 0U, 1U }, &product);
    if (status != LXP_OK) return status;
    status = lxp_u256_div_floor(product, market->tick_size, &quotient,
                                &residue);
    if (status != LXP_OK || !lxp_u128_is_zero(residue))
        return LXP_ERR_PARAMETER_BOUNDS;
    status = lxp_u128_mul(order->quantity, (lxp_u128){ 0U, 1U }, &product);
    if (status != LXP_OK) return status;
    status = lxp_u256_div_floor(product, market->lot_size, &quotient, &residue);
    if (status != LXP_OK || !lxp_u128_is_zero(residue))
        return LXP_ERR_PARAMETER_BOUNDS;
    status = order_margin(market, order, required);
    if (status != LXP_OK) return status;
    return lxp_u128_cmp(*required, available_margin) <= 0 ? LXP_OK :
                                                            LXP_ERR_MARGIN_INSUFFICIENT;
}

static bool order_id_exists(const lx_perps_book *book,
                            const uint8_t order_id[32])
{
    size_t i;
    for (i = 0U; i < book->count; ++i)
        if (memcmp(book->orders[i].order_id, order_id, 32U) == 0)
            return true;
    return false;
}

lxp_result lx_perps_order_place_execute(
    lxp_module_ctx *ctx, lx_perps_book *book, const lx_perps_market *market,
    const lx_perps_order *order, lxp_u128 available_margin,
    lx_perps_fill *fills, size_t fill_capacity, size_t *fill_count,
    size_t *transfer_leg_count)
{
    lx_perps_book candidate;
    lx_perps_order incoming;
    lxp_u128 required;
    lxp_result status;
    if (ctx == NULL || book == NULL || order == NULL || fill_count == NULL ||
        book->count > LX_PERPS_BOOK_CAPACITY ||
        transfer_leg_count == NULL || ctx->module_id != LXP_MODULE_PERPS)
        return LXP_ERR_NON_CANONICAL;
    *transfer_leg_count = 0U;
    status = order_validate(market, order, available_margin, &required);
    if (status != LXP_OK) return status;
    if (order_id_exists(book, order->order_id)) return LXP_ERR_SEQUENCE_REUSED;
    candidate = *book;
    incoming = *order;
    incoming.global_sequence = lxp_ctx_global_sequence(ctx);
    incoming.remaining = incoming.quantity;
    incoming.initial_margin_required = required;
    incoming.active = true;
    status = lx_perps_book_match(&candidate, &incoming, fills, fill_capacity,
                                 fill_count);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(incoming.remaining)) {
        if (candidate.count == LX_PERPS_BOOK_CAPACITY)
            return LXP_ERR_ARENA_EXHAUSTED;
        candidate.orders[candidate.count++] = incoming;
        sort_book(&candidate);
    }
    *book = candidate;
    return LXP_OK;
}

lxp_result lx_perps_order_cancel_execute(
    lxp_module_ctx *ctx, lx_perps_book *book, const uint8_t order_id[32],
    const uint8_t owner_account_id[32], size_t *transfer_leg_count)
{
    size_t i;
    if (ctx == NULL || book == NULL || order_id == NULL ||
        book->count > LX_PERPS_BOOK_CAPACITY ||
        owner_account_id == NULL || transfer_leg_count == NULL ||
        ctx->module_id != LXP_MODULE_PERPS)
        return LXP_ERR_NON_CANONICAL;
    *transfer_leg_count = 0U;
    for (i = 0U; i < book->count; ++i) {
        lx_perps_order *order = &book->orders[i];
        if (memcmp(order->order_id, order_id, 32U) != 0) continue;
        if (memcmp(order->owner_account_id, owner_account_id, 32U) != 0)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        order->active = false;
        compact_book(book);
        return LXP_OK;
    }
    return LXP_ERR_UNKNOWN_FIELD;
}
