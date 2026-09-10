#include "layerx/lx_perps.h"

#include "lx_perps_codec.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t oracle_prefix[] = "oracle:";
static const uint8_t funding_prefix[] = "funding:";
static const uint8_t deficit_prefix[] = "deficit:";
static const uint8_t order_prefix[] = "order:";
static const uint8_t position_prefix[] = "position:";

typedef struct book_load_adapter {
    lx_perps_book *book;
    const uint8_t *market_id;
} book_load_adapter;

typedef struct position_iter_adapter {
    lx_perps_position_visit_fn visit;
    void *user;
} position_iter_adapter;

static void single_key(uint8_t *key, const uint8_t *prefix,
                       size_t prefix_length, const uint8_t identifier[32])
{
    (void)memcpy(key, prefix, prefix_length);
    (void)memcpy(key + prefix_length, identifier, 32U);
}

static void pair_key(uint8_t *key, const uint8_t *prefix,
                     size_t prefix_length, const uint8_t market_id[32],
                     const uint8_t identifier[32])
{
    (void)memcpy(key, prefix, prefix_length);
    (void)memcpy(key + prefix_length, market_id, 32U);
    (void)memcpy(key + prefix_length + 32U, identifier, 32U);
}

static lxp_result oracle_encode(const lx_perps_oracle_state *state,
                                uint8_t bytes[LX_PERPS_ORACLE_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (state == NULL || lxp_ct_is_zero(state->market_id, 32U) ||
        state->observation_sequence == 0U ||
        lxp_u128_is_zero(state->price) ||
        lxp_ct_is_zero(state->oracle_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    lx_perps_put_u64(bytes + offset, state->observation_sequence);
    offset += 8U;
    status = lxp_u128_to_be(state->price, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    lx_perps_put_u64(bytes + offset, state->observed_at); offset += 8U;
    lx_perps_put_u64(bytes + offset, state->source_identifier); offset += 8U;
    (void)memcpy(bytes + offset, state->oracle_public_key, 32U);
    offset += 32U;
    return offset == LX_PERPS_ORACLE_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result oracle_decode(const uint8_t *bytes, size_t length,
                                const uint8_t market_id[32],
                                lx_perps_oracle_state *state)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || state == NULL || length != LX_PERPS_ORACLE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(state, 0, sizeof(*state));
    (void)memcpy(state->market_id, market_id, 32U);
    state->observation_sequence = lx_perps_get_u64(bytes + offset);
    offset += 8U;
    status = lxp_u128_from_be(bytes + offset, &state->price);
    if (status != LXP_OK) return status;
    offset += 16U;
    state->observed_at = lx_perps_get_u64(bytes + offset); offset += 8U;
    state->source_identifier = lx_perps_get_u64(bytes + offset); offset += 8U;
    (void)memcpy(state->oracle_public_key, bytes + offset, 32U);
    offset += 32U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (state->observation_sequence == 0U || lxp_u128_is_zero(state->price) ||
        lxp_ct_is_zero(state->oracle_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_perps_oracle_state_put(lxp_module_ctx *ctx,
                                     const lx_perps_oracle_state *state)
{
    uint8_t key[LX_PERPS_ORACLE_KEY_BYTES];
    uint8_t bytes[LX_PERPS_ORACLE_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || state == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = oracle_encode(state, bytes);
    if (status != LXP_OK) return status;
    single_key(key, oracle_prefix, sizeof(oracle_prefix) - 1U,
               state->market_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_oracle_state_lookup(lxp_module_ctx *ctx,
                                        const uint8_t market_id[32],
                                        lx_perps_oracle_state *state)
{
    uint8_t key[LX_PERPS_ORACLE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || state == NULL)
        return LXP_ERR_NON_CANONICAL;
    single_key(key, oracle_prefix, sizeof(oracle_prefix) - 1U, market_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return oracle_decode(bytes, length, market_id, state);
}

static lxp_result funding_encode(const lx_perps_funding_state *state,
                                 uint8_t bytes[LX_PERPS_FUNDING_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (state == NULL || lxp_ct_is_zero(state->market_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_put_i128(bytes + offset, state->funding_index);
    if (status != LXP_OK) return status;
    offset += 17U;
    lx_perps_put_u64(bytes + offset, state->last_funding_timestamp_ms);
    offset += 8U;
    status = lxp_u128_to_be(state->long_open_notional, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_to_be(state->short_open_notional, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    return offset == LX_PERPS_FUNDING_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result funding_decode(const uint8_t *bytes, size_t length,
                                 const uint8_t market_id[32],
                                 lx_perps_funding_state *state)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || state == NULL || length != LX_PERPS_FUNDING_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(state, 0, sizeof(*state));
    (void)memcpy(state->market_id, market_id, 32U);
    status = lx_perps_get_i128(bytes + offset, &state->funding_index);
    if (status != LXP_OK) return status;
    offset += 17U;
    state->last_funding_timestamp_ms = lx_perps_get_u64(bytes + offset);
    offset += 8U;
    status = lxp_u128_from_be(bytes + offset, &state->long_open_notional);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &state->short_open_notional);
    if (status != LXP_OK) return status;
    offset += 16U;
    return offset == length ? LXP_OK : LXP_ERR_TRAILING_BYTES;
}

lxp_result lx_perps_funding_state_put(lxp_module_ctx *ctx,
                                      const lx_perps_funding_state *state)
{
    uint8_t key[LX_PERPS_FUNDING_KEY_BYTES];
    uint8_t bytes[LX_PERPS_FUNDING_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || state == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = funding_encode(state, bytes);
    if (status != LXP_OK) return status;
    single_key(key, funding_prefix, sizeof(funding_prefix) - 1U,
               state->market_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_funding_state_lookup(lxp_module_ctx *ctx,
                                         const uint8_t market_id[32],
                                         lx_perps_funding_state *state)
{
    uint8_t key[LX_PERPS_FUNDING_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || state == NULL)
        return LXP_ERR_NON_CANONICAL;
    single_key(key, funding_prefix, sizeof(funding_prefix) - 1U, market_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return funding_decode(bytes, length, market_id, state);
}

static lxp_result deficit_encode(const lx_perps_deficit *deficit,
                                 uint8_t bytes[LX_PERPS_DEFICIT_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (deficit == NULL || lxp_ct_is_zero(deficit->market_id, 32U) ||
        lxp_ct_is_zero(deficit->insurance_account_id, 32U) ||
        lxp_u128_is_zero(deficit->amount))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes + offset, deficit->insurance_account_id, 32U);
    offset += 32U;
    status = lxp_u128_to_be(deficit->amount, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    lx_perps_put_u64(bytes + offset, deficit->recorded_at_sequence);
    offset += 8U;
    return offset == LX_PERPS_DEFICIT_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result deficit_decode(const uint8_t *bytes, size_t length,
                                 const uint8_t market_id[32],
                                 lx_perps_deficit *deficit)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || deficit == NULL || length != LX_PERPS_DEFICIT_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(deficit, 0, sizeof(*deficit));
    (void)memcpy(deficit->market_id, market_id, 32U);
    (void)memcpy(deficit->insurance_account_id, bytes + offset, 32U);
    offset += 32U;
    status = lxp_u128_from_be(bytes + offset, &deficit->amount);
    if (status != LXP_OK) return status;
    offset += 16U;
    deficit->recorded_at_sequence = lx_perps_get_u64(bytes + offset);
    offset += 8U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    return lxp_u128_is_zero(deficit->amount) ||
           lxp_ct_is_zero(deficit->insurance_account_id, 32U) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_deficit_put(lxp_module_ctx *ctx,
                                const lx_perps_deficit *deficit)
{
    uint8_t key[LX_PERPS_DEFICIT_KEY_BYTES];
    uint8_t bytes[LX_PERPS_DEFICIT_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || deficit == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = deficit_encode(deficit, bytes);
    if (status != LXP_OK) return status;
    single_key(key, deficit_prefix, sizeof(deficit_prefix) - 1U,
               deficit->market_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_deficit_lookup(lxp_module_ctx *ctx,
                                   const uint8_t market_id[32],
                                   lx_perps_deficit *deficit)
{
    uint8_t key[LX_PERPS_DEFICIT_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || deficit == NULL)
        return LXP_ERR_NON_CANONICAL;
    single_key(key, deficit_prefix, sizeof(deficit_prefix) - 1U, market_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return deficit_decode(bytes, length, market_id, deficit);
}

lxp_result lx_perps_deficit_delete(lxp_module_ctx *ctx,
                                   const uint8_t market_id[32])
{
    uint8_t key[LX_PERPS_DEFICIT_KEY_BYTES];
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    single_key(key, deficit_prefix, sizeof(deficit_prefix) - 1U, market_id);
    return lxp_ctx_kv_del(ctx, key, sizeof(key));
}

static lxp_result order_encode(const lx_perps_order *order,
                               uint8_t bytes[LX_PERPS_ORDER_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (order == NULL || lxp_ct_is_zero(order->order_id, 32U) ||
        lxp_ct_is_zero(order->market_id, 32U) ||
        lxp_ct_is_zero(order->owner_account_id, 32U) ||
        lxp_u128_is_zero(order->price) || lxp_u128_is_zero(order->quantity))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes + offset, order->order_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, order->market_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, order->owner_account_id, 32U); offset += 32U;
    status = lx_perps_put_side(bytes + offset, (int)order->side);
    if (status != LXP_OK) return status;
    offset += 1U;
    status = lxp_u128_to_be(order->price, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_to_be(order->quantity, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_to_be(order->remaining, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_to_be(order->initial_margin_required, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    lx_perps_put_u64(bytes + offset, order->global_sequence); offset += 8U;
    bytes[offset++] = order->active ? 1U : 0U;
    return offset == LX_PERPS_ORDER_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result order_decode(const uint8_t *bytes, size_t length,
                               lx_perps_order *order)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || order == NULL || length != LX_PERPS_ORDER_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(order, 0, sizeof(*order));
    (void)memcpy(order->order_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(order->market_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(order->owner_account_id, bytes + offset, 32U); offset += 32U;
    if (!lx_perps_side_valid(bytes[offset])) return LXP_ERR_NON_CANONICAL;
    order->side = bytes[offset] == 1U ? LX_PERPS_SIDE_BUY : LX_PERPS_SIDE_SELL;
    offset += 1U;
    status = lxp_u128_from_be(bytes + offset, &order->price);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &order->quantity);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &order->remaining);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset,
                              &order->initial_margin_required);
    if (status != LXP_OK) return status;
    offset += 16U;
    order->global_sequence = lx_perps_get_u64(bytes + offset); offset += 8U;
    if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
    order->active = bytes[offset++] != 0U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    return lxp_u128_is_zero(order->price) ||
           lxp_u128_is_zero(order->quantity) ||
           lxp_u128_cmp(order->remaining, order->quantity) > 0 ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_order_put(lxp_module_ctx *ctx,
                              const lx_perps_order *order)
{
    uint8_t key[LX_PERPS_ORDER_KEY_BYTES];
    uint8_t bytes[LX_PERPS_ORDER_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || order == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = order_encode(order, bytes);
    if (status != LXP_OK) return status;
    pair_key(key, order_prefix, sizeof(order_prefix) - 1U, order->market_id,
             order->order_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_order_lookup(lxp_module_ctx *ctx,
                                 const uint8_t market_id[32],
                                 const uint8_t order_id[32],
                                 lx_perps_order *order)
{
    uint8_t key[LX_PERPS_ORDER_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || order_id == NULL || order == NULL)
        return LXP_ERR_NON_CANONICAL;
    pair_key(key, order_prefix, sizeof(order_prefix) - 1U, market_id,
             order_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    status = order_decode(bytes, length, order);
    if (status != LXP_OK) return status;
    return memcmp(order->market_id, market_id, 32U) == 0 &&
           memcmp(order->order_id, order_id, 32U) == 0 ?
        LXP_OK : LXP_ERR_NON_CANONICAL;
}

lxp_result lx_perps_order_delete(lxp_module_ctx *ctx,
                                 const uint8_t market_id[32],
                                 const uint8_t order_id[32])
{
    uint8_t key[LX_PERPS_ORDER_KEY_BYTES];
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || order_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    pair_key(key, order_prefix, sizeof(order_prefix) - 1U, market_id,
             order_id);
    return lxp_ctx_kv_del(ctx, key, sizeof(key));
}

static lxp_result visit_order(const uint8_t *key, size_t key_length,
                              const uint8_t *value, size_t value_length,
                              void *user)
{
    book_load_adapter *adapter = (book_load_adapter *)user;
    lx_perps_order order;
    lxp_result status;
    if (key == NULL || key_length != LX_PERPS_ORDER_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = order_decode(value, value_length, &order);
    if (status != LXP_OK) return status;
    if (memcmp(order.market_id, adapter->market_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (!order.active || lxp_u128_is_zero(order.remaining)) return LXP_OK;
    if (adapter->book->count == LX_PERPS_BOOK_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    adapter->book->orders[adapter->book->count++] = order;
    return LXP_OK;
}

lxp_result lx_perps_order_book_load(lxp_module_ctx *ctx,
                                    const uint8_t market_id[32],
                                    lx_perps_book *book)
{
    uint8_t prefix[LX_PERPS_ORDER_KEY_BYTES - 32U];
    book_load_adapter adapter;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || book == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_book_init(book);
    if (status != LXP_OK) return status;
    single_key(prefix, order_prefix, sizeof(order_prefix) - 1U, market_id);
    adapter.book = book;
    adapter.market_id = market_id;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_order,
                           &adapter);
}

static lxp_result position_encode(const lx_perps_position *position,
                                  uint8_t bytes[LX_PERPS_POSITION_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (position == NULL || lxp_ct_is_zero(position->position_id, 32U) ||
        lxp_ct_is_zero(position->market_id, 32U) ||
        lxp_ct_is_zero(position->owner_main_account_id, 32U) ||
        lxp_ct_is_zero(position->margin_account_id, 32U) ||
        lxp_ct_is_zero(position->asset_id, 32U) ||
        lxp_u128_is_zero(position->size) ||
        lxp_u128_is_zero(position->entry_notional))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes + offset, position->position_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, position->market_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, position->owner_main_account_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, position->margin_account_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, position->asset_id, 32U); offset += 32U;
    status = lx_perps_put_side(bytes + offset, (int)position->side);
    if (status != LXP_OK) return status;
    offset += 1U;
    status = lxp_u128_to_be(position->size, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_to_be(position->entry_notional, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lx_perps_put_i128(bytes + offset,
                               position->funding_index_at_entry);
    if (status != LXP_OK) return status;
    offset += 17U;
    bytes[offset++] = position->open ? 1U : 0U;
    return offset == LX_PERPS_POSITION_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result position_decode(const uint8_t *bytes, size_t length,
                                  lx_perps_position *position)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || position == NULL ||
        length != LX_PERPS_POSITION_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(position, 0, sizeof(*position));
    (void)memcpy(position->position_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(position->market_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(position->owner_main_account_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(position->margin_account_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(position->asset_id, bytes + offset, 32U); offset += 32U;
    if (!lx_perps_side_valid(bytes[offset])) return LXP_ERR_NON_CANONICAL;
    position->side = bytes[offset] == 1U ? LX_PERPS_SIDE_BUY :
                                           LX_PERPS_SIDE_SELL;
    offset += 1U;
    status = lxp_u128_from_be(bytes + offset, &position->size);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &position->entry_notional);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lx_perps_get_i128(bytes + offset,
                               &position->funding_index_at_entry);
    if (status != LXP_OK) return status;
    offset += 17U;
    if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
    position->open = bytes[offset++] != 0U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    return lxp_u128_is_zero(position->size) ||
           lxp_u128_is_zero(position->entry_notional) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_position_put(lxp_module_ctx *ctx,
                                 const lx_perps_position *position)
{
    uint8_t key[LX_PERPS_POSITION_KEY_BYTES];
    uint8_t bytes[LX_PERPS_POSITION_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || position == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = position_encode(position, bytes);
    if (status != LXP_OK) return status;
    pair_key(key, position_prefix, sizeof(position_prefix) - 1U,
             position->market_id, position->position_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_position_get(lxp_module_ctx *ctx,
                                 const uint8_t market_id[32],
                                 const uint8_t position_id[32],
                                 lx_perps_position *position)
{
    uint8_t key[LX_PERPS_POSITION_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || position_id == NULL || position == NULL)
        return LXP_ERR_NON_CANONICAL;
    pair_key(key, position_prefix, sizeof(position_prefix) - 1U, market_id,
             position_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    status = position_decode(bytes, length, position);
    if (status != LXP_OK) return status;
    return memcmp(position->market_id, market_id, 32U) == 0 &&
           memcmp(position->position_id, position_id, 32U) == 0 ?
        LXP_OK : LXP_ERR_NON_CANONICAL;
}

static lxp_result visit_position(const uint8_t *key, size_t key_length,
                                 const uint8_t *value, size_t value_length,
                                 void *user)
{
    position_iter_adapter *adapter = (position_iter_adapter *)user;
    lx_perps_position position;
    lxp_result status;
    if (key == NULL || key_length != LX_PERPS_POSITION_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = position_decode(value, value_length, &position);
    if (status != LXP_OK) return status;
    return adapter->visit(&position, adapter->user);
}

lxp_result lx_perps_position_iter(lxp_module_ctx *ctx,
                                  const uint8_t market_id[32],
                                  lx_perps_position_visit_fn visit,
                                  void *user)
{
    uint8_t prefix[LX_PERPS_POSITION_KEY_BYTES - 32U];
    position_iter_adapter adapter;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    single_key(prefix, position_prefix, sizeof(position_prefix) - 1U,
               market_id);
    adapter.visit = visit;
    adapter.user = user;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_position,
                           &adapter);
}

lxp_result lx_perps_source_authority_add(
    lxp_transfer_source_authority *authorities, size_t capacity,
    size_t *count, const uint8_t authorized_from[32],
    lxp_authorization_kind kind, bool protocol_system_capability)
{
    size_t i;
    if (authorities == NULL || count == NULL || authorized_from == NULL ||
        *count > capacity)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < *count; ++i)
        if (memcmp(authorities[i].authorized_from, authorized_from, 32U) == 0)
            return authorities[i].debit_authority_kind == kind &&
                   authorities[i].protocol_system_capability ==
                       protocol_system_capability ?
                LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
    if (*count == capacity) return LXP_ERR_TOO_MANY_LEGS;
    (void)memset(&authorities[*count], 0, sizeof(authorities[*count]));
    (void)memcpy(authorities[*count].authorized_from, authorized_from, 32U);
    authorities[*count].debit_authority_kind = kind;
    authorities[*count].protocol_system_capability =
        protocol_system_capability;
    ++(*count);
    return LXP_OK;
}
