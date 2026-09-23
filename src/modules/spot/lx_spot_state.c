#include "layerx/lx_spot.h"

#include "lx_spot_codec.h"

#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t market_prefix[] = "market:";
static const uint8_t order_prefix[] = "order:";

_Static_assert(sizeof(market_prefix) - 1U + 32U == LX_SPOT_MARKET_KEY_BYTES,
               "spot market key length");
_Static_assert(sizeof(order_prefix) - 1U + 64U == LX_SPOT_ORDER_KEY_BYTES,
               "spot order key length");

typedef struct book_load_adapter {
    lx_spot_book *book;
    const uint8_t *market_id;
} book_load_adapter;

static bool spot_ctx(const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->module_id == LXP_MODULE_SPOT;
}

static void market_key(uint8_t key[LX_SPOT_MARKET_KEY_BYTES],
                       const uint8_t market_id[32])
{
    (void)memcpy(key, market_prefix, sizeof(market_prefix) - 1U);
    (void)memcpy(key + sizeof(market_prefix) - 1U, market_id, 32U);
}

static void order_key(uint8_t key[LX_SPOT_ORDER_KEY_BYTES],
                      const uint8_t market_id[32], const uint8_t order_id[32])
{
    (void)memcpy(key, order_prefix, sizeof(order_prefix) - 1U);
    (void)memcpy(key + sizeof(order_prefix) - 1U, market_id, 32U);
    (void)memcpy(key + sizeof(order_prefix) - 1U + 32U, order_id, 32U);
}

static lxp_result market_record_encode(const lx_spot_market *market,
                                       uint8_t bytes[LX_SPOT_MARKET_BYTES])
{
    lxp_result status = lx_spot_market_encode(market, bytes);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + LX_SPOT_MARKET_PAYLOAD_BYTES, market->base_escrow_id,
                 32U);
    (void)memcpy(bytes + LX_SPOT_MARKET_PAYLOAD_BYTES + 32U,
                 market->quote_escrow_id, 32U);
    bytes[LX_SPOT_MARKET_PAYLOAD_BYTES + 64U] = market->halted ? 1U : 0U;
    return LXP_OK;
}

static lxp_result market_record_decode(const uint8_t *bytes, size_t length,
                                       lx_spot_market *market)
{
    lx_spot_market decoded;
    lxp_result status;
    if (bytes == NULL || length != LX_SPOT_MARKET_BYTES ||
        bytes[LX_SPOT_MARKET_BYTES - 1U] > 1U)
        return LXP_ERR_NON_CANONICAL;
    status = lx_spot_market_decode(bytes, LX_SPOT_MARKET_PAYLOAD_BYTES,
                                   &decoded);
    if (status != LXP_OK) return status;
    if (memcmp(decoded.base_escrow_id, bytes + LX_SPOT_MARKET_PAYLOAD_BYTES,
               32U) != 0 ||
        memcmp(decoded.quote_escrow_id,
               bytes + LX_SPOT_MARKET_PAYLOAD_BYTES + 32U, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    decoded.halted = bytes[LX_SPOT_MARKET_BYTES - 1U] != 0U;
    *market = decoded;
    return LXP_OK;
}

static lxp_result order_encode(const lx_spot_order *order,
                               uint8_t bytes[LX_SPOT_ORDER_BYTES])
{
    lxp_result status;
    if (order == NULL || lx_spot_zero_id(order->order_id) ||
        lx_spot_zero_id(order->market_id) ||
        !lx_spot_side_valid((uint8_t)order->side) ||
        lxp_u128_is_zero(order->remaining) ||
        lxp_u128_is_zero(order->escrowed) ||
        lxp_u128_cmp(order->remaining, order->quantity) > 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, order->order_id, 32U);
    (void)memcpy(bytes + 32U, order->market_id, 32U);
    (void)memcpy(bytes + 64U, order->base_account_id, 32U);
    (void)memcpy(bytes + 96U, order->quote_account_id, 32U);
    bytes[128] = (uint8_t)order->side;
    status = lxp_u128_to_be(order->price, bytes + 129U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(order->quantity, bytes + 145U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(order->remaining, bytes + 161U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(order->escrowed, bytes + 177U);
    if (status != LXP_OK) return status;
    lx_spot_put_u64(bytes + 193U, order->global_sequence);
    return LXP_OK;
}

static lxp_result order_decode(const uint8_t *bytes, size_t length,
                               lx_spot_order *order)
{
    lx_spot_order decoded;
    lxp_result status;
    if (bytes == NULL || order == NULL || length != LX_SPOT_ORDER_BYTES ||
        !lx_spot_side_valid(bytes[128]))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded, 0, sizeof(decoded));
    (void)memcpy(decoded.order_id, bytes, 32U);
    (void)memcpy(decoded.market_id, bytes + 32U, 32U);
    (void)memcpy(decoded.base_account_id, bytes + 64U, 32U);
    (void)memcpy(decoded.quote_account_id, bytes + 96U, 32U);
    decoded.side = (lx_spot_side)bytes[128];
    status = lxp_u128_from_be(bytes + 129U, &decoded.price);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 145U, &decoded.quantity);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 161U, &decoded.remaining);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 177U, &decoded.escrowed);
    if (status != LXP_OK) return status;
    decoded.global_sequence = lx_spot_get_u64(bytes + 193U);
    if (lxp_u128_is_zero(decoded.remaining) ||
        lxp_u128_is_zero(decoded.escrowed) ||
        lxp_u128_cmp(decoded.remaining, decoded.quantity) > 0)
        return LXP_ERR_NON_CANONICAL;
    *order = decoded;
    return LXP_OK;
}

lxp_result lx_spot_market_lookup(lxp_module_ctx *ctx,
                                 const uint8_t market_id[32],
                                 lx_spot_market *market)
{
    uint8_t key[LX_SPOT_MARKET_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t value_length = 0U;
    lxp_result status;
    if (!spot_ctx(ctx) || market_id == NULL || market == NULL)
        return LXP_ERR_NON_CANONICAL;
    market_key(key, market_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &value, &value_length);
    if (status != LXP_OK) return status;
    return market_record_decode(value, value_length, market);
}

lxp_result lx_spot_market_put(lxp_module_ctx *ctx,
                              const lx_spot_market *market)
{
    uint8_t key[LX_SPOT_MARKET_KEY_BYTES];
    uint8_t bytes[LX_SPOT_MARKET_BYTES];
    lxp_result status;
    if (!spot_ctx(ctx) || market == NULL) return LXP_ERR_NON_CANONICAL;
    status = market_record_encode(market, bytes);
    if (status != LXP_OK) return status;
    market_key(key, market->market_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

static lxp_result visit_market(const uint8_t *key, size_t key_length,
                               const uint8_t *value, size_t value_length,
                               void *user)
{
    size_t *count = (size_t *)user;
    (void)value;
    if (key == NULL || key_length != LX_SPOT_MARKET_KEY_BYTES ||
        value_length != LX_SPOT_MARKET_BYTES)
        return LXP_ERR_NON_CANONICAL;
    ++*count;
    return LXP_OK;
}

lxp_result lx_spot_market_count(lxp_module_ctx *ctx, size_t *count)
{
    if (!spot_ctx(ctx) || count == NULL) return LXP_ERR_NON_CANONICAL;
    *count = 0U;
    return lxp_ctx_kv_iter(ctx, market_prefix, sizeof(market_prefix) - 1U,
                           visit_market, count);
}

lxp_result lx_spot_order_lookup(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                const uint8_t order_id[32],
                                lx_spot_order *order)
{
    uint8_t key[LX_SPOT_ORDER_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t value_length = 0U;
    lxp_result status;
    if (!spot_ctx(ctx) || market_id == NULL || order_id == NULL ||
        order == NULL)
        return LXP_ERR_NON_CANONICAL;
    order_key(key, market_id, order_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &value, &value_length);
    if (status != LXP_OK) return status;
    return order_decode(value, value_length, order);
}

lxp_result lx_spot_order_put(lxp_module_ctx *ctx, const lx_spot_order *order)
{
    uint8_t key[LX_SPOT_ORDER_KEY_BYTES];
    uint8_t bytes[LX_SPOT_ORDER_BYTES];
    lxp_result status;
    if (!spot_ctx(ctx) || order == NULL) return LXP_ERR_NON_CANONICAL;
    status = order_encode(order, bytes);
    if (status != LXP_OK) return status;
    order_key(key, order->market_id, order->order_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_spot_order_delete(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                const uint8_t order_id[32])
{
    uint8_t key[LX_SPOT_ORDER_KEY_BYTES];
    if (!spot_ctx(ctx) || market_id == NULL || order_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    order_key(key, market_id, order_id);
    return lxp_ctx_kv_del(ctx, key, sizeof(key));
}

static lxp_result visit_order(const uint8_t *key, size_t key_length,
                              const uint8_t *value, size_t value_length,
                              void *user)
{
    book_load_adapter *adapter = (book_load_adapter *)user;
    lx_spot_order order;
    lxp_result status;
    if (key == NULL || key_length != LX_SPOT_ORDER_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = order_decode(value, value_length, &order);
    if (status != LXP_OK) return status;
    if (memcmp(order.market_id, adapter->market_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (adapter->book->count == LX_SPOT_BOOK_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    adapter->book->orders[adapter->book->count++] = order;
    return LXP_OK;
}

lxp_result lx_spot_book_load(lxp_module_ctx *ctx, const uint8_t market_id[32],
                             lx_spot_book *book)
{
    uint8_t prefix[LX_SPOT_ORDER_KEY_BYTES - 32U];
    book_load_adapter adapter;
    if (!spot_ctx(ctx) || market_id == NULL || book == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(book, 0, sizeof(*book));
    (void)memcpy(prefix, order_prefix, sizeof(order_prefix) - 1U);
    (void)memcpy(prefix + sizeof(order_prefix) - 1U, market_id, 32U);
    adapter.book = book;
    adapter.market_id = market_id;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_order,
                           &adapter);
}

static const lx_spot_order *book_find(const lx_spot_book *book,
                                      const uint8_t order_id[32])
{
    size_t i;
    for (i = 0U; i < book->count; ++i)
        if (memcmp(book->orders[i].order_id, order_id, 32U) == 0)
            return &book->orders[i];
    return NULL;
}

static bool order_unchanged(const lx_spot_order *left,
                            const lx_spot_order *right)
{
    return lxp_u128_cmp(left->remaining, right->remaining) == 0 &&
           lxp_u128_cmp(left->escrowed, right->escrowed) == 0;
}

lxp_result lx_spot_book_persist(lxp_module_ctx *ctx,
                                const lx_spot_book *before,
                                const lx_spot_book *after)
{
    size_t i;
    lxp_result status;
    if (!spot_ctx(ctx) || before == NULL || after == NULL ||
        before->count > LX_SPOT_BOOK_CAPACITY ||
        after->count > LX_SPOT_BOOK_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < before->count; ++i) {
        const lx_spot_order *current =
            book_find(after, before->orders[i].order_id);
        if (current == NULL) {
            status = lx_spot_order_delete(ctx, before->orders[i].market_id,
                                          before->orders[i].order_id);
            if (status != LXP_OK) return status;
            continue;
        }
        if (order_unchanged(&before->orders[i], current)) continue;
        status = lx_spot_order_put(ctx, current);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < after->count; ++i) {
        if (book_find(before, after->orders[i].order_id) != NULL) continue;
        status = lx_spot_order_put(ctx, &after->orders[i]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}
