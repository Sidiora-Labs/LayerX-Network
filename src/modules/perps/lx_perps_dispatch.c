#include "layerx/lx_perps.h"

#include "lx_perps_codec.h"

#include "layerx/lx_asset.h"
#include "layerx/lx_oracle.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_PERPS_MARKET_CREATE, LX_PERPS_MARKET_HALT, LX_PERPS_ORACLE_PUSH,
    LX_PERPS_ORDER_PLACE, LX_PERPS_ORDER_CANCEL, LX_PERPS_POSITION_OPEN,
    LX_PERPS_POSITION_INCREASE, LX_PERPS_POSITION_CLOSE,
    LX_PERPS_FUNDING_TICK, LX_PERPS_LIQUIDATE, LX_PERPS_ADL
};

typedef union perps_typed_command {
    lx_perps_market market;
    lx_perps_halt_command halt;
    lx_perps_oracle_command oracle;
    lx_perps_order_command order;
    lx_perps_cancel_command cancel;
    lx_perps_open_command open;
    lx_perps_increase_command increase;
    lx_perps_close_command close;
    lx_perps_tick_command tick;
    lx_perps_liquidate_command liquidate;
    lx_perps_adl_command adl;
} perps_typed_command;

typedef struct perps_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
    perps_typed_command *typed;
} perps_decoded;

static const lxp_u128 u128_one = { 0U, 1U };
static const lxp_u128 u128_two = { 0U, 2U };
static const lxp_u128 u128_zero = { 0U, 0U };

static lxp_result work_alloc(lxp_module_ctx *ctx, size_t size,
                             size_t alignment, void **memory)
{
    lxp_result status = lxp_ctx_arena_alloc(ctx, size, alignment, memory);
    if (status != LXP_OK) return status;
    (void)memset(*memory, 0, size);
    return LXP_OK;
}

static lxp_result receipt_alloc(lxp_module_ctx *ctx, lxp_receipt **receipt)
{
    void *memory = NULL;
    lxp_result status = work_alloc(ctx, sizeof(lxp_receipt),
                                   _Alignof(lxp_receipt), &memory);
    if (status != LXP_OK) return status;
    *receipt = (lxp_receipt *)memory;
    return LXP_OK;
}

static lxp_result book_alloc(lxp_module_ctx *ctx, lx_perps_book **book)
{
    void *memory = NULL;
    lxp_result status = work_alloc(ctx, sizeof(lx_perps_book),
                                   _Alignof(lx_perps_book), &memory);
    if (status != LXP_OK) return status;
    *book = (lx_perps_book *)memory;
    return LXP_OK;
}

static lxp_result store_alloc(lxp_module_ctx *ctx,
                              lx_perps_position_store **store)
{
    void *memory = NULL;
    lxp_result status = work_alloc(ctx, sizeof(lx_perps_position_store),
                                   _Alignof(lx_perps_position_store),
                                   &memory);
    if (status != LXP_OK) return status;
    *store = (lx_perps_position_store *)memory;
    return LXP_OK;
}

static void owner_context(lxp_module_ctx *ctx, const lxp_activity *activity,
                          lx_account *sequence_account,
                          lxp_transfer_context *context)
{
    (void)memset(context, 0, sizeof(*context));
    context->batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    context->expires_at = activity->timestamp_bound.not_after;
    context->actor_sequence = activity->account_sequence;
    context->sequence_account = sequence_account;
    context->debit_authority_kind = LXP_AUTH_OWNER;
}

static void protocol_context(lxp_module_ctx *ctx,
                             const lxp_activity *activity,
                             lxp_transfer_context *context)
{
    (void)memset(context, 0, sizeof(*context));
    context->batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    context->expires_at = activity->timestamp_bound.not_after;
    context->protocol_system_capability = true;
    context->debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
}

static lxp_result actor_account(lxp_module_ctx *ctx,
                                const lxp_activity *activity,
                                lx_account **account)
{
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t account_id[32];
    size_t length;
    lxp_result status;
    if (ctx == NULL || activity == NULL || account == NULL ||
        activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U ||
        activity->actor_did.length + 11U > LX_ACCOUNT_NAME_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, activity->actor_did.bytes,
                 activity->actor_did.length);
    (void)memcpy(name + 6U + activity->actor_did.length, ":main", 5U);
    length = 6U + activity->actor_did.length + 5U;
    status = lx_account_id_from_string(name, length, account_id);
    if (status != LXP_OK) return status;
    return lxp_ctx_account_find(ctx, account_id, account);
}

static bool margin_owned_by_actor(const lx_account *account,
                                  const lxp_activity *activity)
{
    static const uint8_t segment[] = ":margin:";
    size_t prefix;
    if (account == NULL || activity == NULL ||
        activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U ||
        account->kind != LX_ACCOUNT_AGENT_MARGIN)
        return false;
    prefix = 6U + activity->actor_did.length;
    if ((size_t)account->name_length <= prefix + sizeof(segment) - 1U)
        return false;
    return memcmp(account->name, "agent:", 6U) == 0 &&
           memcmp(account->name + 6U, activity->actor_did.bytes,
                  activity->actor_did.length) == 0 &&
           memcmp(account->name + prefix, segment, sizeof(segment) - 1U) == 0;
}

static lxp_result system_account(lxp_module_ctx *ctx,
                                 const uint8_t account_id[32],
                                 lx_account_kind expected,
                                 lx_account **account)
{
    lxp_result status = lxp_ctx_account_find(ctx, account_id, account);
    if (status != LXP_OK) return status;
    return (*account)->kind == expected ?
        LXP_OK : LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

static lxp_result market_accounts_check(lxp_module_ctx *ctx,
                                        const lx_perps_market *market)
{
    lx_account *account;
    lxp_result status = system_account(ctx, market->liquidity_account_id,
                                       LX_ACCOUNT_SYSTEM_LIQUIDITY, &account);
    if (status == LXP_OK)
        status = system_account(ctx, market->long_funding_account_id,
                                LX_ACCOUNT_SYSTEM_FUNDING_LONG, &account);
    if (status == LXP_OK)
        status = system_account(ctx, market->short_funding_account_id,
                                LX_ACCOUNT_SYSTEM_FUNDING_SHORT, &account);
    if (status == LXP_OK)
        status = system_account(ctx, market->insurance_account_id,
                                LX_ACCOUNT_SYSTEM_INSURANCE, &account);
    return status;
}

static const lx_asset_record *runtime_asset_record(
    const lx_asset_runtime *runtime, const uint8_t asset_id[32])
{
    size_t i;
    if (runtime == NULL || runtime->assets == NULL ||
        runtime->asset_count == 0U ||
        runtime->asset_count > LX_ASSET_REGISTRY_CAPACITY)
        return NULL;
    for (i = 0U; i < runtime->asset_count; ++i)
        if (memcmp(runtime->assets[i].asset_id, asset_id, 32U) == 0)
            return &runtime->assets[i];
    return NULL;
}

static lxp_result quote_asset_state(lxp_module_ctx *ctx,
                                    const uint8_t asset_id[32],
                                    lxp_transfer_asset_state *state)
{
    static const uint8_t prefix[] = "asset:";
    const lx_asset_runtime *runtime;
    const lx_asset_record *base;
    lx_asset_record record;
    size_t i;
    if (ctx == NULL || ctx->kernel == NULL || asset_id == NULL ||
        state == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_ASSET ||
            entry->key_length != sizeof(prefix) - 1U + 32U ||
            memcmp(entry->key, prefix, sizeof(prefix) - 1U) != 0 ||
            memcmp(entry->key + sizeof(prefix) - 1U, asset_id, 32U) != 0)
            continue;
        if (lx_asset_record_decode(entry->value, entry->value_length,
                                   &record) != LXP_OK)
            return LXP_ERR_ASSET_MISMATCH;
        return lx_asset_transfer_state(&record, state);
    }
    runtime = (const lx_asset_runtime *)
        ctx->kernel->module_runtime[LXP_MODULE_ASSET];
    base = runtime_asset_record(runtime, asset_id);
    if (base == NULL) return LXP_ERR_ASSET_MISMATCH;
    return lx_asset_transfer_state(base, state);
}

static void oracle_market_project(const lx_perps_market *market,
                                  lx_oracle_market *projected)
{
    size_t i;
    (void)memset(projected, 0, sizeof(*projected));
    (void)memcpy(projected->market_id, market->market_id, 32U);
    for (i = 0U; i < market->permitted_oracle_key_count &&
                 i < (size_t)LX_ORACLE_MAX_KEYS; ++i)
        (void)memcpy(projected->permitted_keys[i],
                     market->permitted_oracle_keys[i], 32U);
    projected->permitted_key_count = market->permitted_oracle_key_count;
    projected->maximum_staleness = market->maximum_oracle_staleness_ms;
    projected->minimum_price = market->minimum_price;
    projected->maximum_price = market->maximum_price;
    projected->maximum_deviation_basis_points =
        market->maximum_deviation_basis_points;
    projected->halted = market->halted;
}

static void oracle_observation_project(const lx_perps_oracle_state *state,
                                       lx_oracle_observation *observation)
{
    (void)memset(observation, 0, sizeof(*observation));
    (void)memcpy(observation->market_id, state->market_id, 32U);
    observation->observation_sequence = state->observation_sequence;
    observation->price = state->price;
    observation->observed_at = state->observed_at;
    observation->source_identifier = state->source_identifier;
    (void)memcpy(observation->oracle_public_key, state->oracle_public_key,
                 32U);
}

static lxp_result oracle_price_current(lxp_module_ctx *ctx,
                                       const lx_perps_market *market,
                                       lxp_u128 *price)
{
    lx_perps_oracle_state state;
    uint64_t timestamp;
    lxp_result status = lx_perps_oracle_state_lookup(ctx, market->market_id,
                                                     &state);
    if (status != LXP_OK)
        return status == LXP_ERR_UNKNOWN_FIELD ? LXP_ERR_ORACLE_STALE : status;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (state.observed_at > timestamp) return LXP_ERR_TIMESTAMP_REGRESSION;
    if (timestamp - state.observed_at > market->maximum_oracle_staleness_ms)
        return LXP_ERR_ORACLE_STALE;
    *price = state.price;
    return LXP_OK;
}

static lxp_result multiple_check(lxp_u128 value, lxp_u128 unit)
{
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_u256 product;
    lxp_result status;
    if (lxp_u128_is_zero(unit)) return LXP_ERR_PARAMETER_BOUNDS;
    status = lxp_u128_mul(value, u128_one, &product);
    if (status != LXP_OK) return status;
    status = lxp_u256_div_floor(product, unit, &quotient, &remainder);
    if (status != LXP_OK) return status;
    return lxp_u128_is_zero(remainder) ? LXP_OK : LXP_ERR_PARAMETER_BOUNDS;
}

static lxp_result price_check(const lx_perps_market *market, lxp_u128 price)
{
    if (lxp_u128_cmp(price, market->minimum_price) < 0 ||
        lxp_u128_cmp(price, market->maximum_price) > 0)
        return LXP_ERR_ORACLE_BOUNDS;
    return multiple_check(price, market->tick_size);
}

static lxp_result entry_price_of(const lx_perps_market *market,
                                 lxp_u128 size, lxp_u128 notional,
                                 lxp_u128 *price)
{
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_result status;
    if (lxp_u128_is_zero(size)) return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul_div_floor(notional, market->price_scale, size,
                                    &quotient, &remainder);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    if (!lxp_u128_is_zero(remainder)) return LXP_ERR_PARAMETER_BOUNDS;
    *price = quotient;
    return LXP_OK;
}

static lxp_result notional_check(const lx_perps_market *market,
                                 lxp_u128 size, lxp_u128 notional)
{
    lxp_u128 price;
    lxp_result status = multiple_check(size, market->lot_size);
    if (status != LXP_OK) return status;
    status = entry_price_of(market, size, notional, &price);
    if (status != LXP_OK) return status;
    return price_check(market, price);
}

static lxp_result margin_requirement(const lx_perps_market *market,
                                     lxp_u128 notional, lxp_u128 *required)
{
    lxp_result status = lxp_u128_mul_bps_ceil(
        notional, market->initial_margin_ratio_bps, required);
    return status == LXP_OK ? LXP_OK : LXP_ERR_OVERFLOW;
}

static lxp_result funding_open_interest_add(lx_perps_funding_state *funding,
                                            lx_perps_side side,
                                            lxp_u128 notional)
{
    lxp_u128 *target = side == LX_PERPS_SIDE_BUY ?
        &funding->long_open_notional : &funding->short_open_notional;
    return lxp_u128_add(*target, notional, target) == LXP_OK ?
        LXP_OK : LXP_ERR_OVERFLOW;
}

static lxp_result funding_open_interest_remove(
    lx_perps_funding_state *funding, lx_perps_side side, lxp_u128 notional)
{
    lxp_u128 *target = side == LX_PERPS_SIDE_BUY ?
        &funding->long_open_notional : &funding->short_open_notional;
    if (lxp_u128_cmp(*target, notional) < 0) return LXP_ERR_CONSERVATION;
    return lxp_u128_sub(*target, notional, target) == LXP_OK ?
        LXP_OK : LXP_ERR_CONSERVATION;
}

static bool book_mid_price(const lx_perps_book *book,
                           const uint8_t market_id[32], lxp_u128 *mid)
{
    lxp_u128 best_bid = { 0U, 0U };
    lxp_u128 best_ask = { 0U, 0U };
    lxp_u128 spread;
    lxp_u128 half;
    lxp_u128 remainder;
    bool has_bid = false;
    bool has_ask = false;
    size_t i;
    for (i = 0U; i < book->count; ++i) {
        const lx_perps_order *order = &book->orders[i];
        if (!order->active || lxp_u128_is_zero(order->remaining) ||
            memcmp(order->market_id, market_id, 32U) != 0)
            continue;
        if (order->side == LX_PERPS_SIDE_BUY) {
            if (!has_bid || lxp_u128_cmp(order->price, best_bid) > 0) {
                best_bid = order->price;
                has_bid = true;
            }
        } else if (!has_ask || lxp_u128_cmp(order->price, best_ask) < 0) {
            best_ask = order->price;
            has_ask = true;
        }
    }
    if (!has_bid || !has_ask) return false;
    if (lxp_u128_cmp(best_ask, best_bid) < 0) {
        lxp_u128 swap = best_bid;
        best_bid = best_ask;
        best_ask = swap;
    }
    if (lxp_u128_sub(best_ask, best_bid, &spread) != LXP_OK) return false;
    if (lxp_u128_mul_div_floor(spread, u128_one, u128_two, &half,
                               &remainder) != LXP_OK)
        return false;
    if (lxp_u128_add(best_bid, half, mid) != LXP_OK) return false;
    return !lxp_u128_is_zero(*mid);
}

static const lx_perps_order *book_find(const lx_perps_book *book,
                                       const uint8_t order_id[32])
{
    size_t i;
    for (i = 0U; i < book->count; ++i)
        if (memcmp(book->orders[i].order_id, order_id, 32U) == 0)
            return &book->orders[i];
    return NULL;
}

static bool order_unchanged(const lx_perps_order *left,
                            const lx_perps_order *right)
{
    return left->active == right->active &&
           lxp_u128_cmp(left->remaining, right->remaining) == 0 &&
           lxp_u128_cmp(left->price, right->price) == 0 &&
           lxp_u128_cmp(left->quantity, right->quantity) == 0;
}

static lxp_result book_persist(lxp_module_ctx *ctx,
                               const lx_perps_book *before,
                               const lx_perps_book *after)
{
    size_t i;
    lxp_result status;
    for (i = 0U; i < before->count; ++i) {
        const lx_perps_order *current =
            book_find(after, before->orders[i].order_id);
        if (current == NULL) {
            status = lx_perps_order_delete(ctx, before->orders[i].market_id,
                                           before->orders[i].order_id);
            if (status != LXP_OK) return status;
            continue;
        }
        if (order_unchanged(&before->orders[i], current)) continue;
        status = lx_perps_order_put(ctx, current);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < after->count; ++i) {
        if (book_find(before, after->orders[i].order_id) != NULL) continue;
        status = lx_perps_order_put(ctx, &after->orders[i]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result emit_identifier_event(lxp_module_ctx *ctx,
                                        uint16_t event_type,
                                        const uint8_t first[32],
                                        const uint8_t second[32],
                                        const uint8_t *tail,
                                        size_t tail_length)
{
    uint8_t body[128];
    size_t length = 0U;
    if (tail_length > sizeof(body) - 64U) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(body, first, 32U);
    length += 32U;
    (void)memcpy(body + length, second, 32U);
    length += 32U;
    if (tail_length != 0U) {
        (void)memcpy(body + length, tail, tail_length);
        length += tail_length;
    }
    return lxp_ctx_emit_event(ctx, event_type, body, length);
}

static lxp_result oracle_command_bind(const perps_decoded *value,
                                      const lxp_activity *activity,
                                      const lxp_authority_resolved *authority,
                                      lx_perps_oracle_command *command)
{
    if (value->typed == NULL || activity->authority.bytes == NULL ||
        activity->authority.length != 32U ||
        activity->signature.bytes == NULL ||
        activity->signature.length != 64U ||
        memcmp(authority->verified_key, activity->authority.bytes, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_ORACLE;
    *command = value->typed->oracle;
    (void)memcpy(command->oracle_public_key, activity->authority.bytes, 32U);
    (void)memcpy(command->signature, activity->signature.bytes, 64U);
    return LXP_OK;
}

static lxp_result oracle_admission_check(lxp_module_ctx *ctx,
                                         const perps_decoded *value,
                                         const lxp_activity *activity,
                                         const lxp_authority_resolved *authority,
                                         lx_perps_oracle_command *command)
{
    lx_perps_market market;
    lx_perps_oracle_state previous;
    lx_oracle_market projected;
    lx_oracle_observation observation;
    lx_oracle_observation latest;
    lxp_result status = oracle_command_bind(value, activity, authority,
                                            command);
    if (status != LXP_OK) return status;
    status = lx_perps_market_lookup(ctx, command->market_id, &market);
    if (status != LXP_OK) return status;
    oracle_market_project(&market, &projected);
    (void)memset(&observation, 0, sizeof(observation));
    (void)memcpy(observation.market_id, command->market_id, 32U);
    observation.observation_sequence = command->observation_sequence;
    observation.price = command->price;
    observation.observed_at = command->observed_at;
    observation.source_identifier = command->source_identifier;
    (void)memcpy(observation.oracle_public_key, command->oracle_public_key,
                 32U);
    (void)memcpy(observation.signature, command->signature, 64U);
    status = lx_oracle_key_set_check(&projected, &observation, value->payload,
                                     value->payload_length);
    if (status == LXP_OK)
        status = lx_oracle_staleness_check(&projected, &observation,
                                           lxp_ctx_batch_timestamp_ms(ctx));
    if (status == LXP_OK)
        status = lx_oracle_bounds_check(&projected, &observation);
    if (status != LXP_OK) return status;
    status = lx_perps_oracle_state_lookup(ctx, command->market_id, &previous);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (command->observation_sequence <= previous.observation_sequence)
        return LXP_ERR_ORACLE_SEQUENCE;
    oracle_observation_project(&previous, &latest);
    return lx_oracle_deviation_check(&projected, &latest, &observation);
}

static lxp_result market_loaded(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                bool refuse_halted,
                                lx_perps_market *market)
{
    lxp_result status = lx_perps_market_lookup(ctx, market_id, market);
    if (status != LXP_OK) return status;
    return refuse_halted && market->halted ? LXP_ERR_MARKET_HALTED : LXP_OK;
}

static lxp_result position_loaded(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const uint8_t market_id[32],
                                  const uint8_t position_id[32],
                                  lx_perps_position *position,
                                  lx_account **owner_main,
                                  lx_account **margin_account)
{
    lxp_result status = lx_perps_position_get(ctx, market_id, position_id,
                                              position);
    if (status != LXP_OK) return status;
    if (!position->open) return LXP_ERR_AGREEMENT_STATE;
    status = actor_account(ctx, activity, owner_main);
    if (status != LXP_OK) return status;
    if (memcmp(position->owner_main_account_id, (*owner_main)->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_ctx_account_find(ctx, position->margin_account_id,
                                  margin_account);
    if (status != LXP_OK) return status;
    return (*margin_account)->kind == LX_ACCOUNT_AGENT_MARGIN ?
        LXP_OK : LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

static lxp_result validate_market_create(lxp_module_ctx *ctx,
                                         const lxp_authority_resolved *authority,
                                         const lx_perps_market *market)
{
    lx_perps_market existing;
    lxp_result status;
    if (memcmp(authority->actor, market->administrator, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = market_accounts_check(ctx, market);
    if (status != LXP_OK) return status;
    status = lx_perps_market_lookup(ctx, market->market_id, &existing);
    if (status == LXP_OK) return LXP_ERR_MARKET_ALREADY_EXISTS;
    return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK : status;
}

static lxp_result validate_market_halt(lxp_module_ctx *ctx,
                                       const lxp_authority_resolved *authority,
                                       const lx_perps_halt_command *command)
{
    lx_perps_market market;
    lxp_result status = lx_perps_market_lookup(ctx, command->market_id,
                                               &market);
    if (status != LXP_OK) return status;
    if (memcmp(authority->actor, market.administrator, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return market.halted == command->halted ? LXP_ERR_AGREEMENT_STATE : LXP_OK;
}

static lxp_result validate_order_place(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lx_perps_order_command *command)
{
    lx_perps_market market;
    lx_perps_order existing;
    lx_account *owner;
    lxp_u128 price;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &price);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, command->owner_account_id, &owner);
    if (status != LXP_OK) return status;
    if (!margin_owned_by_actor(owner, activity))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_perps_order_lookup(ctx, command->market_id, command->order_id,
                                   &existing);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK : status;
}

static lxp_result validate_order_cancel(lxp_module_ctx *ctx,
                                        const lxp_activity *activity,
                                        const lx_perps_cancel_command *command)
{
    lx_perps_market market;
    lx_perps_order order;
    lx_account *owner;
    lxp_result status = market_loaded(ctx, command->market_id, false, &market);
    if (status != LXP_OK) return status;
    status = lx_perps_order_lookup(ctx, command->market_id, command->order_id,
                                   &order);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, order.owner_account_id, &owner);
    if (status != LXP_OK) return status;
    return margin_owned_by_actor(owner, activity) ?
        LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
}

static lxp_result validate_position_open(lxp_module_ctx *ctx,
                                         const lxp_activity *activity,
                                         const lx_perps_open_command *command)
{
    lx_perps_market market;
    lx_perps_position existing;
    lx_account *owner_main;
    lx_account *margin_account;
    lxp_u128 price;
    lxp_u128 required;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &price);
    if (status != LXP_OK) return status;
    status = actor_account(ctx, activity, &owner_main);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, command->margin_account_id,
                                  &margin_account);
    if (status != LXP_OK) return status;
    if (!margin_owned_by_actor(margin_account, activity))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = notional_check(&market, command->size, command->entry_notional);
    if (status != LXP_OK) return status;
    status = margin_requirement(&market, command->entry_notional, &required);
    if (status != LXP_OK) return status;
    if (lxp_u128_cmp(command->margin_amount, required) < 0)
        return LXP_ERR_MARGIN_INSUFFICIENT;
    status = lx_perps_position_get(ctx, command->market_id,
                                   command->position_id, &existing);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK : status;
}

static lxp_result validate_position_increase(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lx_perps_increase_command *command)
{
    lx_perps_market market;
    lx_perps_position position;
    lx_account *owner_main;
    lx_account *margin_account;
    lxp_u128 price;
    lxp_u128 total_notional;
    lxp_u128 total_size;
    lxp_u128 required;
    lxp_u128 held;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &price);
    if (status != LXP_OK) return status;
    status = position_loaded(ctx, activity, command->market_id,
                             command->position_id, &position, &owner_main,
                             &margin_account);
    if (status != LXP_OK) return status;
    status = notional_check(&market, command->size_delta,
                            command->notional_delta);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(position.size, command->size_delta, &total_size);
    if (status == LXP_OK)
        status = lxp_u128_add(position.entry_notional,
                              command->notional_delta, &total_notional);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    status = margin_requirement(&market, total_notional, &required);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(margin_account->balance, command->margin_amount,
                          &held);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    return lxp_u128_cmp(held, required) < 0 ? LXP_ERR_MARGIN_INSUFFICIENT :
                                              LXP_OK;
}

static lxp_result validate_position_close(lxp_module_ctx *ctx,
                                          const lxp_activity *activity,
                                          const lx_perps_close_command *command)
{
    lx_perps_market market;
    lx_perps_position position;
    lx_account *owner_main;
    lx_account *margin_account;
    lxp_result status = market_loaded(ctx, command->market_id, false, &market);
    if (status != LXP_OK) return status;
    return position_loaded(ctx, activity, command->market_id,
                           command->position_id, &position, &owner_main,
                           &margin_account);
}

static lxp_result validate_funding_tick(lxp_module_ctx *ctx,
                                        const lx_perps_tick_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_account *account;
    lxp_u128 price;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &price);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = system_account(ctx, market.long_funding_account_id,
                            LX_ACCOUNT_SYSTEM_FUNDING_LONG, &account);
    if (status != LXP_OK) return status;
    return system_account(ctx, market.short_funding_account_id,
                          LX_ACCOUNT_SYSTEM_FUNDING_SHORT, &account);
}

static lxp_result validate_liquidate(lxp_module_ctx *ctx,
                                     const lxp_activity *activity,
                                     const lx_perps_liquidate_command *command)
{
    lx_perps_market market;
    lx_perps_position position;
    lx_account *liquidator;
    lx_account *margin_account;
    lxp_u128 price;
    bool liquidatable = false;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &price);
    if (status != LXP_OK) return status;
    status = actor_account(ctx, activity, &liquidator);
    if (status != LXP_OK) return status;
    if (memcmp(liquidator->id, command->liquidator_account_id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_perps_position_get(ctx, command->market_id,
                                   command->position_id, &position);
    if (status != LXP_OK) return status;
    if (!position.open) return LXP_ERR_AGREEMENT_STATE;
    status = lxp_ctx_account_find(ctx, position.margin_account_id,
                                  &margin_account);
    if (status != LXP_OK) return status;
    status = market_accounts_check(ctx, &market);
    if (status != LXP_OK) return status;
    status = lx_perps_maintenance_check(&market, &position, price,
                                        market.price_scale,
                                        margin_account->balance,
                                        &liquidatable);
    if (status != LXP_OK) return status;
    return liquidatable ? LXP_OK : LXP_ERR_MARGIN_INSUFFICIENT;
}

static lxp_result validate_adl(lxp_module_ctx *ctx,
                               const lx_perps_adl_command *command)
{
    lx_perps_market market;
    lx_perps_deficit deficit;
    lx_perps_position position;
    lx_account *account;
    size_t i;
    lxp_result status;
    if (command->position_count == 0U ||
        command->position_count > (size_t)LX_PERPS_DISPATCH_MAX_ADL)
        return LXP_ERR_LENGTH_LIMIT;
    status = market_loaded(ctx, command->market_id, false, &market);
    if (status != LXP_OK) return status;
    status = lx_perps_deficit_lookup(ctx, command->market_id, &deficit);
    if (status != LXP_OK) return status;
    status = system_account(ctx, market.liquidity_account_id,
                            LX_ACCOUNT_SYSTEM_LIQUIDITY, &account);
    if (status != LXP_OK) return status;
    for (i = 0U; i < command->position_count; ++i) {
        status = lx_perps_position_get(ctx, command->market_id,
                                       command->position_ids[i], &position);
        if (status != LXP_OK) return status;
        if (!position.open) return LXP_ERR_AGREEMENT_STATE;
        status = lxp_ctx_account_find(ctx, position.margin_account_id,
                                      &account);
        if (status != LXP_OK) return status;
        if (account->kind != LX_ACCOUNT_AGENT_MARGIN)
            return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    }
    return LXP_OK;
}

static lxp_result execute_market_create(lxp_module_ctx *ctx,
                                        const lx_perps_market *market)
{
    lx_perps_funding_state funding;
    lxp_result status = lx_perps_market_create_execute(ctx, market);
    if (status != LXP_OK) return status;
    (void)memset(&funding, 0, sizeof(funding));
    (void)memcpy(funding.market_id, market->market_id, 32U);
    funding.last_funding_timestamp_ms = lxp_ctx_batch_timestamp_ms(ctx);
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_MARKET_CREATED,
                                 market->market_id, market->quote_asset,
                                 NULL, 0U);
}

static lxp_result execute_market_halt(lxp_module_ctx *ctx,
                                      const lx_perps_halt_command *command)
{
    lx_perps_market market;
    uint8_t flag;
    lxp_result status = lx_perps_market_lookup(ctx, command->market_id,
                                               &market);
    if (status != LXP_OK) return status;
    market.halted = command->halted;
    status = lx_perps_market_put(ctx, &market);
    if (status != LXP_OK) return status;
    flag = command->halted ? 1U : 0U;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_MARKET_HALTED,
                                 market.market_id, market.administrator,
                                 &flag, 1U);
}

static lxp_result execute_oracle_push(lxp_module_ctx *ctx,
                                      const perps_decoded *value,
                                      const lxp_activity *activity,
                                      const lxp_authority_resolved *authority)
{
    lx_perps_oracle_command command;
    lx_perps_oracle_state state;
    uint8_t tail[24];
    lxp_result status = oracle_admission_check(ctx, value, activity, authority,
                                               &command);
    if (status != LXP_OK) return status;
    (void)memset(&state, 0, sizeof(state));
    (void)memcpy(state.market_id, command.market_id, 32U);
    state.observation_sequence = command.observation_sequence;
    state.price = command.price;
    state.observed_at = command.observed_at;
    state.source_identifier = command.source_identifier;
    (void)memcpy(state.oracle_public_key, command.oracle_public_key, 32U);
    status = lx_perps_oracle_state_put(ctx, &state);
    if (status != LXP_OK) return status;
    lx_perps_put_u64(tail, state.observation_sequence);
    status = lxp_u128_to_be(state.price, tail + 8U);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_ORACLE_ACCEPTED,
                                 state.market_id, state.oracle_public_key,
                                 tail, sizeof(tail));
}

static lxp_result execute_order_place(lxp_module_ctx *ctx,
                                      const lxp_activity *activity,
                                      const lx_perps_order_command *command)
{
    lx_perps_market market;
    lx_perps_book *before = NULL;
    lx_perps_book *after = NULL;
    lx_perps_fill fills[LX_PERPS_DISPATCH_MAX_FILLS];
    lx_perps_order order;
    lx_account *owner;
    size_t fill_count = 0U;
    size_t leg_count = 0U;
    uint8_t tail[17];
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, command->owner_account_id, &owner);
    if (status != LXP_OK) return status;
    if (!margin_owned_by_actor(owner, activity))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = book_alloc(ctx, &before);
    if (status == LXP_OK) status = book_alloc(ctx, &after);
    if (status != LXP_OK) return status;
    status = lx_perps_order_book_load(ctx, command->market_id, before);
    if (status != LXP_OK) return status;
    *after = *before;
    (void)memset(fills, 0, sizeof(fills));
    (void)memset(&order, 0, sizeof(order));
    (void)memcpy(order.order_id, command->order_id, 32U);
    (void)memcpy(order.market_id, command->market_id, 32U);
    (void)memcpy(order.owner_account_id, command->owner_account_id, 32U);
    order.side = command->side;
    order.price = command->price;
    order.quantity = command->quantity;
    status = lx_perps_order_place_execute(ctx, after, &market, &order,
                                          owner->balance, fills,
                                          LX_PERPS_DISPATCH_MAX_FILLS,
                                          &fill_count, &leg_count);
    if (status != LXP_OK) return status;
    status = book_persist(ctx, before, after);
    if (status != LXP_OK) return status;
    tail[0] = (uint8_t)fill_count;
    status = lxp_u128_to_be(order.quantity, tail + 1U);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_ORDER_PLACED,
                                 command->order_id, command->market_id,
                                 tail, sizeof(tail));
}

static lxp_result execute_order_cancel(lxp_module_ctx *ctx,
                                       const lx_perps_cancel_command *command)
{
    lx_perps_book *book = NULL;
    lx_perps_order order;
    size_t leg_count = 0U;
    lxp_result status = lx_perps_order_lookup(ctx, command->market_id,
                                              command->order_id, &order);
    if (status != LXP_OK) return status;
    status = book_alloc(ctx, &book);
    if (status != LXP_OK) return status;
    status = lx_perps_order_book_load(ctx, command->market_id, book);
    if (status != LXP_OK) return status;
    status = lx_perps_order_cancel_execute(ctx, book, command->order_id,
                                           order.owner_account_id,
                                           &leg_count);
    if (status != LXP_OK) return status;
    status = lx_perps_order_delete(ctx, command->market_id,
                                   command->order_id);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_ORDER_CANCELLED,
                                 command->order_id, command->market_id,
                                 NULL, 0U);
}

static lxp_result execute_position_open(lxp_module_ctx *ctx,
                                        const lxp_activity *activity,
                                        const lx_perps_open_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_position_store *store = NULL;
    lx_perps_position_request request;
    lxp_transfer_asset_state asset;
    lxp_receipt *receipt = NULL;
    lx_account *owner_main;
    lx_account *margin_account;
    uint8_t side_byte;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = actor_account(ctx, activity, &owner_main);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, command->margin_account_id,
                                  &margin_account);
    if (status != LXP_OK) return status;
    if (!margin_owned_by_actor(margin_account, activity))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = store_alloc(ctx, &store);
    if (status == LXP_OK) status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    request.store = store;
    request.owner_main = owner_main;
    request.margin_account = margin_account;
    request.asset = &asset;
    (void)memcpy(request.position.position_id, command->position_id, 32U);
    (void)memcpy(request.position.market_id, command->market_id, 32U);
    (void)memcpy(request.position.owner_main_account_id, owner_main->id, 32U);
    (void)memcpy(request.position.margin_account_id, margin_account->id, 32U);
    (void)memcpy(request.position.asset_id, asset.asset_id, 32U);
    request.position.side = command->side;
    request.position.size = command->size;
    request.position.entry_notional = command->entry_notional;
    request.position.funding_index_at_entry = funding.funding_index;
    request.margin_amount = command->margin_amount;
    owner_context(ctx, activity, owner_main, &request.context);
    status = lx_perps_position_open_execute(ctx, &request, receipt);
    if (status != LXP_OK) return status;
    status = lx_perps_position_put(ctx, &store->positions[0]);
    if (status != LXP_OK) return status;
    status = funding_open_interest_add(&funding, command->side,
                                       command->entry_notional);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    status = lx_perps_put_side(&side_byte, (int)command->side);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_POSITION_OPENED,
                                 command->position_id, command->market_id,
                                 &side_byte, 1U);
}

static lxp_result execute_position_increase(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lx_perps_increase_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_position position;
    lx_perps_position_store *store = NULL;
    lx_perps_position_request request;
    lxp_transfer_asset_state asset;
    lxp_receipt *receipt = NULL;
    lx_account *owner_main;
    lx_account *margin_account;
    uint8_t side_byte;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = position_loaded(ctx, activity, command->market_id,
                             command->position_id, &position, &owner_main,
                             &margin_account);
    if (status != LXP_OK) return status;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = store_alloc(ctx, &store);
    if (status == LXP_OK) status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    store->positions[0] = position;
    store->count = 1U;
    (void)memset(&request, 0, sizeof(request));
    request.store = store;
    request.owner_main = owner_main;
    request.margin_account = margin_account;
    request.asset = &asset;
    request.position = position;
    request.margin_amount = command->margin_amount;
    request.size_delta = command->size_delta;
    request.notional_delta = command->notional_delta;
    owner_context(ctx, activity, owner_main, &request.context);
    status = lx_perps_position_increase_execute(ctx, &request, receipt);
    if (status != LXP_OK) return status;
    status = lx_perps_position_put(ctx, &store->positions[0]);
    if (status != LXP_OK) return status;
    status = funding_open_interest_add(&funding, position.side,
                                       command->notional_delta);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    status = lx_perps_put_side(&side_byte, (int)position.side);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_POSITION_INCREASED,
                                 command->position_id, command->market_id,
                                 &side_byte, 1U);
}

static lxp_result close_settlement_emit(lxp_module_ctx *ctx,
                                        const lxp_activity *activity,
                                        const lx_perps_market *market,
                                        const lx_perps_position *position,
                                        lx_account *owner_main,
                                        lx_account *margin_account,
                                        const lxp_transfer_asset_state *asset,
                                        lxp_i128 owed, lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority authorities[2];
    lx_account *pool;
    size_t authority_count = 0U;
    lxp_result status;
    if (lxp_u128_is_zero(margin_account->balance))
        return LXP_ERR_ACCOUNT_NOT_EMPTY;
    status = system_account(ctx,
                            position->side == LX_PERPS_SIDE_BUY ?
                                market->long_funding_account_id :
                                market->short_funding_account_id,
                            position->side == LX_PERPS_SIDE_BUY ?
                                LX_ACCOUNT_SYSTEM_FUNDING_LONG :
                                LX_ACCOUNT_SYSTEM_FUNDING_SHORT,
                            &pool);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(authorities, 0, sizeof(authorities));
    owner_context(ctx, activity, owner_main, &set.context);
    set.context.assets = asset;
    set.context.asset_count = 1U;
    (void)memcpy(set.context.authorized_from, margin_account->id, 32U);
    set.leg_count = 2U;
    set.legs[0].from = margin_account;
    set.legs[0].to = owner_main;
    (void)memcpy(set.legs[0].asset_id, asset->asset_id, 32U);
    set.legs[0].amount = margin_account->balance;
    set.legs[0].reason = LXP_REASON_MARGIN_RELEASE;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.legs[1].from = owed.negative ? pool : owner_main;
    set.legs[1].to = owed.negative ? owner_main : pool;
    (void)memcpy(set.legs[1].asset_id, asset->asset_id, 32U);
    set.legs[1].amount = owed.magnitude;
    set.legs[1].reason = LXP_REASON_FUNDING;
    set.legs[1].supply_mode = LXP_TRANSFER_CONSERVED;
    status = lx_perps_source_authority_add(authorities, 2U, &authority_count,
                                           margin_account->id,
                                           LXP_AUTH_PROTOCOL_MODULE, true);
    if (status == LXP_OK)
        status = owed.negative ?
            lx_perps_source_authority_add(authorities, 2U, &authority_count,
                                          pool->id, LXP_AUTH_PROTOCOL_MODULE,
                                          true) :
            lx_perps_source_authority_add(authorities, 2U, &authority_count,
                                          owner_main->id, LXP_AUTH_OWNER,
                                          false);
    if (status != LXP_OK) return status;
    set.context.source_authorities = authorities;
    set.context.source_authority_count = authority_count;
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

static lxp_result execute_position_close(lxp_module_ctx *ctx,
                                         const lxp_activity *activity,
                                         const lx_perps_close_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_position position;
    lx_perps_position_store *store = NULL;
    lxp_transfer_asset_state asset;
    lxp_transfer_context context;
    lxp_receipt *receipt = NULL;
    lx_account *owner_main;
    lx_account *margin_account;
    lxp_i128 owed;
    uint8_t tail[17];
    lxp_result status = market_loaded(ctx, command->market_id, false, &market);
    if (status != LXP_OK) return status;
    status = position_loaded(ctx, activity, command->market_id,
                             command->position_id, &position, &owner_main,
                             &margin_account);
    if (status != LXP_OK) return status;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_owed(&position, funding.funding_index, &owed);
    if (status != LXP_OK) return status;
    status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(owed.magnitude)) {
        status = store_alloc(ctx, &store);
        if (status != LXP_OK) return status;
        store->positions[0] = position;
        store->count = 1U;
        owner_context(ctx, activity, owner_main, &context);
        status = lx_perps_position_close_execute(ctx, store,
                                                 command->position_id,
                                                 margin_account, owner_main,
                                                 &asset, context, receipt);
    } else {
        status = close_settlement_emit(ctx, activity, &market, &position,
                                       owner_main, margin_account, &asset,
                                       owed, receipt);
        if (status == LXP_OK && !lxp_u128_is_zero(margin_account->balance))
            status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK) margin_account->has_open_reference = false;
    }
    if (status != LXP_OK) return status;
    position.open = false;
    status = lx_perps_position_put(ctx, &position);
    if (status != LXP_OK) return status;
    status = funding_open_interest_remove(&funding, position.side,
                                          position.entry_notional);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    status = lx_perps_put_i128(tail, owed);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_POSITION_CLOSED,
                                 command->position_id, command->market_id,
                                 tail, sizeof(tail));
}

static lxp_result execute_funding_tick(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lx_perps_tick_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_funding_tick_request request;
    lx_perps_book *book = NULL;
    lxp_transfer_asset_state asset;
    lxp_receipt *receipt = NULL;
    lx_account *long_account;
    lx_account *short_account;
    lxp_u128 oracle_price;
    lxp_u128 mid;
    lxp_u128 matched;
    lxp_i128 rate;
    uint8_t tail[34];
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &oracle_price);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = system_account(ctx, market.long_funding_account_id,
                            LX_ACCOUNT_SYSTEM_FUNDING_LONG, &long_account);
    if (status == LXP_OK)
        status = system_account(ctx, market.short_funding_account_id,
                                LX_ACCOUNT_SYSTEM_FUNDING_SHORT,
                                &short_account);
    if (status != LXP_OK) return status;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = book_alloc(ctx, &book);
    if (status == LXP_OK) status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    status = lx_perps_order_book_load(ctx, command->market_id, book);
    if (status != LXP_OK) return status;
    rate.negative = false;
    rate.magnitude = u128_zero;
    matched = lxp_u128_cmp(funding.long_open_notional,
                           funding.short_open_notional) < 0 ?
        funding.long_open_notional : funding.short_open_notional;
    if (!lxp_u128_is_zero(matched) &&
        book_mid_price(book, market.market_id, &mid)) {
        status = lx_perps_funding_rate(&market, mid, oracle_price,
                                       market.maximum_funding_rate_bps,
                                       &rate);
        if (status != LXP_OK) return status;
    }
    (void)memset(&request, 0, sizeof(request));
    request.market = &market;
    request.long_funding_account = long_account;
    request.short_funding_account = short_account;
    request.asset = &asset;
    request.funding_rate_bps = rate;
    request.open_notional = matched;
    request.last_funding_timestamp_ms = &funding.last_funding_timestamp_ms;
    request.funding_index = &funding.funding_index;
    protocol_context(ctx, activity, &request.context);
    status = lx_perps_funding_tick_execute(ctx, &request, receipt);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    status = lx_perps_put_i128(tail, funding.funding_index);
    if (status == LXP_OK) status = lx_perps_put_i128(tail + 17U, rate);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_FUNDING_SETTLED,
                                 market.market_id, market.quote_asset,
                                 tail, sizeof(tail));
}

static lxp_result liquidation_loss(const lx_perps_market *market,
                                   const lx_perps_position *position,
                                   lxp_u128 mark_price, lxp_u128 *loss)
{
    lxp_u128 entry_price;
    lxp_i128 pnl;
    lxp_result status = entry_price_of(market, position->size,
                                       position->entry_notional,
                                       &entry_price);
    if (status != LXP_OK) return status;
    status = lx_perps_pnl_compute(position->side, entry_price, mark_price,
                                  position->size, market->price_scale, &pnl);
    if (status != LXP_OK) return status;
    *loss = pnl.negative ? pnl.magnitude : u128_zero;
    return LXP_OK;
}

static lxp_result deficit_accumulate(lxp_module_ctx *ctx,
                                     const uint8_t market_id[32],
                                     const uint8_t insurance_account_id[32],
                                     lxp_u128 amount)
{
    lx_perps_deficit deficit;
    lxp_u128 total;
    lxp_result status = lx_perps_deficit_lookup(ctx, market_id, &deficit);
    if (status == LXP_ERR_UNKNOWN_FIELD) {
        (void)memset(&deficit, 0, sizeof(deficit));
        (void)memcpy(deficit.market_id, market_id, 32U);
        (void)memcpy(deficit.insurance_account_id, insurance_account_id, 32U);
        deficit.amount = amount;
        deficit.recorded_at_sequence = lxp_ctx_global_sequence(ctx);
        return lx_perps_deficit_put(ctx, &deficit);
    }
    if (status != LXP_OK) return status;
    if (memcmp(deficit.insurance_account_id, insurance_account_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_add(deficit.amount, amount, &total);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    deficit.amount = total;
    deficit.recorded_at_sequence = lxp_ctx_global_sequence(ctx);
    return lx_perps_deficit_put(ctx, &deficit);
}

static lxp_result execute_liquidate(lxp_module_ctx *ctx,
                                    const lxp_activity *activity,
                                    const lx_perps_liquidate_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_position position;
    lx_perps_liquidation_request request;
    lxp_transfer_asset_state asset;
    lxp_receipt *receipt = NULL;
    lx_account *margin_account;
    lx_account *owner_main;
    lx_account *liquidity;
    lx_account *insurance;
    lx_account *liquidator;
    lxp_u128 mark_price;
    lxp_u128 loss;
    lxp_u128 capacity;
    lxp_u128 covered;
    lxp_u128 residue;
    uint8_t tail[16];
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &mark_price);
    if (status != LXP_OK) return status;
    status = lx_perps_position_get(ctx, command->market_id,
                                   command->position_id, &position);
    if (status != LXP_OK) return status;
    if (!position.open) return LXP_ERR_AGREEMENT_STATE;
    status = lxp_ctx_account_find(ctx, position.margin_account_id,
                                  &margin_account);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, position.owner_main_account_id,
                                      &owner_main);
    if (status == LXP_OK)
        status = system_account(ctx, market.liquidity_account_id,
                                LX_ACCOUNT_SYSTEM_LIQUIDITY, &liquidity);
    if (status == LXP_OK)
        status = system_account(ctx, market.insurance_account_id,
                                LX_ACCOUNT_SYSTEM_INSURANCE, &insurance);
    if (status == LXP_OK) status = actor_account(ctx, activity, &liquidator);
    if (status != LXP_OK) return status;
    if (memcmp(liquidator->id, command->liquidator_account_id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = liquidation_loss(&market, &position, mark_price, &loss);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(margin_account->balance, insurance->balance,
                          &capacity);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    covered = lxp_u128_cmp(loss, capacity) < 0 ? loss : capacity;
    status = lxp_u128_sub(loss, covered, &residue);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    request.position = &position;
    request.market = &market;
    request.margin_account = margin_account;
    request.market_liquidity_account = liquidity;
    request.liquidator_main_account = liquidator;
    request.insurance_account = insurance;
    request.owner_main_account = owner_main;
    request.asset = &asset;
    request.mark_price = mark_price;
    request.price_scale = market.price_scale;
    request.trading_loss = covered;
    request.liquidation_fee_bps = market.liquidation_fee_bps;
    request.liquidator_share_bps = market.liquidator_share_bps;
    protocol_context(ctx, activity, &request.context);
    status = lx_perps_liquidate_execute(ctx, &request, receipt);
    if (status != LXP_OK) return status;
    status = lx_perps_position_put(ctx, &position);
    if (status != LXP_OK) return status;
    status = funding_open_interest_remove(&funding, position.side,
                                          position.entry_notional);
    if (status == LXP_OK) status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(residue)) {
        status = deficit_accumulate(ctx, market.market_id, insurance->id,
                                    residue);
        if (status != LXP_OK) return status;
    }
    status = lxp_u128_to_be(covered, tail);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_LIQUIDATED,
                                 command->position_id, command->market_id,
                                 tail, sizeof(tail));
}

static lxp_result execute_adl(lxp_module_ctx *ctx,
                              const lxp_activity *activity,
                              const lx_perps_adl_command *command)
{
    lx_perps_market market;
    lx_perps_funding_state funding;
    lx_perps_deficit deficit;
    lx_perps_position positions[LX_PERPS_DISPATCH_MAX_ADL];
    lx_perps_adl_candidate candidates[LX_PERPS_DISPATCH_MAX_ADL];
    lxp_transfer_asset_state asset;
    lxp_transfer_context context;
    lxp_receipt *receipt = NULL;
    lx_account *liquidity;
    lx_account *margin_account;
    lxp_u128 mark_price;
    lxp_u128 remaining = { 0U, 0U };
    lxp_u128 entry_price;
    lxp_i128 pnl;
    size_t i;
    uint8_t tail[17];
    lxp_result status;
    if (command->position_count == 0U ||
        command->position_count > (size_t)LX_PERPS_DISPATCH_MAX_ADL)
        return LXP_ERR_LENGTH_LIMIT;
    status = market_loaded(ctx, command->market_id, false, &market);
    if (status != LXP_OK) return status;
    status = oracle_price_current(ctx, &market, &mark_price);
    if (status != LXP_OK) return status;
    status = lx_perps_deficit_lookup(ctx, command->market_id, &deficit);
    if (status != LXP_OK) return status;
    status = system_account(ctx, market.liquidity_account_id,
                            LX_ACCOUNT_SYSTEM_LIQUIDITY, &liquidity);
    if (status != LXP_OK) return status;
    status = quote_asset_state(ctx, market.quote_asset, &asset);
    if (status != LXP_OK) return status;
    status = lx_perps_funding_state_lookup(ctx, command->market_id, &funding);
    if (status != LXP_OK) return status;
    status = receipt_alloc(ctx, &receipt);
    if (status != LXP_OK) return status;
    (void)memset(positions, 0, sizeof(positions));
    (void)memset(candidates, 0, sizeof(candidates));
    for (i = 0U; i < command->position_count; ++i) {
        status = lx_perps_position_get(ctx, command->market_id,
                                       command->position_ids[i],
                                       &positions[i]);
        if (status != LXP_OK) return status;
        if (!positions[i].open) return LXP_ERR_AGREEMENT_STATE;
        status = lxp_ctx_account_find(ctx, positions[i].margin_account_id,
                                      &margin_account);
        if (status != LXP_OK) return status;
        status = entry_price_of(&market, positions[i].size,
                                positions[i].entry_notional, &entry_price);
        if (status != LXP_OK) return status;
        status = lx_perps_pnl_compute(positions[i].side, entry_price,
                                      mark_price, positions[i].size,
                                      market.price_scale, &pnl);
        if (status != LXP_OK) return status;
        if (pnl.negative || lxp_u128_is_zero(pnl.magnitude))
            return LXP_ERR_AGREEMENT_STATE;
        candidates[i].position = &positions[i];
        candidates[i].margin_account = margin_account;
        candidates[i].maximum_contribution =
            lxp_u128_cmp(pnl.magnitude, margin_account->balance) < 0 ?
                pnl.magnitude : margin_account->balance;
    }
    protocol_context(ctx, activity, &context);
    status = lx_perps_adl_execute(ctx, candidates, command->position_count,
                                  liquidity, &asset, deficit.amount, context,
                                  receipt, &remaining);
    if (status != LXP_OK) return status;
    status = lx_perps_deficit_delete(ctx, command->market_id);
    if (status != LXP_OK) return status;
    for (i = 0U; i < command->position_count; ++i) {
        status = lx_perps_position_put(ctx, &positions[i]);
        if (status != LXP_OK) return status;
        if (positions[i].open) continue;
        status = funding_open_interest_remove(&funding, positions[i].side,
                                              positions[i].entry_notional);
        if (status != LXP_OK) return status;
    }
    status = lx_perps_funding_state_put(ctx, &funding);
    if (status != LXP_OK) return status;
    tail[0] = (uint8_t)command->position_count;
    status = lxp_u128_to_be(deficit.amount, tail + 1U);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_PERPS_EVENT_DELEVERAGED,
                                 market.market_id, liquidity->id,
                                 tail, sizeof(tail));
}

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t manifest_length)
{
    if (ctx == NULL || (manifest == NULL && manifest_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, manifest_length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t payload_length,
                                void **decoded)
{
    perps_decoded *value;
    void *memory = NULL;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 11U ||
        payload == NULL || payload_length == 0U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(perps_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (perps_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = payload_length;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value->typed),
                                 _Alignof(perps_typed_command), &memory);
    if (status != LXP_OK) return status;
    value->typed = (perps_typed_command *)memory;
    (void)memset(value->typed, 0, sizeof(*value->typed));
    switch (ordinal) {
    case 1U:
        status = lx_perps_market_decode(payload, payload_length,
                                        &value->typed->market);
        break;
    case 2U:
        status = lx_perps_halt_command_decode(payload, payload_length,
                                              &value->typed->halt);
        break;
    case 3U:
        status = lx_perps_oracle_command_decode(payload, payload_length,
                                                &value->typed->oracle);
        break;
    case 4U:
        status = lx_perps_order_command_decode(payload, payload_length,
                                               &value->typed->order);
        break;
    case 5U:
        status = lx_perps_cancel_command_decode(payload, payload_length,
                                                &value->typed->cancel);
        break;
    case 6U:
        status = lx_perps_open_command_decode(payload, payload_length,
                                              &value->typed->open);
        break;
    case 7U:
        status = lx_perps_increase_command_decode(payload, payload_length,
                                                  &value->typed->increase);
        break;
    case 8U:
        status = lx_perps_close_command_decode(payload, payload_length,
                                               &value->typed->close);
        break;
    case 9U:
        status = lx_perps_tick_command_decode(payload, payload_length,
                                              &value->typed->tick);
        break;
    case 10U:
        status = lx_perps_liquidate_command_decode(payload, payload_length,
                                                   &value->typed->liquidate);
        break;
    default:
        status = lx_perps_adl_command_decode(payload, payload_length,
                                             &value->typed->adl);
        break;
    }
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const perps_decoded *value = (const perps_decoded *)decoded;
    lx_perps_oracle_command oracle;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->typed == NULL || value->ordinal == 0U ||
        value->ordinal > 11U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (ctx->module_id != LXP_MODULE_PERPS ||
        lxp_activity_module_id(activity->activity_type) != LXP_MODULE_PERPS ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
    if (status != LXP_OK) return status;
    switch (value->ordinal) {
    case 1U:
        return validate_market_create(ctx, authority, &value->typed->market);
    case 2U:
        return validate_market_halt(ctx, authority, &value->typed->halt);
    case 3U:
        return oracle_admission_check(ctx, value, activity, authority,
                                      &oracle);
    case 4U:
        return validate_order_place(ctx, activity, &value->typed->order);
    case 5U:
        return validate_order_cancel(ctx, activity, &value->typed->cancel);
    case 6U:
        return validate_position_open(ctx, activity, &value->typed->open);
    case 7U:
        return validate_position_increase(ctx, activity,
                                          &value->typed->increase);
    case 8U:
        return validate_position_close(ctx, activity, &value->typed->close);
    case 9U:
        return validate_funding_tick(ctx, &value->typed->tick);
    case 10U:
        return validate_liquidate(ctx, activity, &value->typed->liquidate);
    default:
        break;
    }
    return validate_adl(ctx, &value->typed->adl);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const perps_decoded *value = (const perps_decoded *)decoded;
    (void)effects;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->typed == NULL || value->ordinal == 0U ||
        value->ordinal > 11U || ctx->module_id != LXP_MODULE_PERPS)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    switch (value->ordinal) {
    case 1U:
        return execute_market_create(ctx, &value->typed->market);
    case 2U:
        return execute_market_halt(ctx, &value->typed->halt);
    case 3U:
        return execute_oracle_push(ctx, value, activity, authority);
    case 4U:
        return execute_order_place(ctx, activity, &value->typed->order);
    case 5U:
        return execute_order_cancel(ctx, &value->typed->cancel);
    case 6U:
        return execute_position_open(ctx, activity, &value->typed->open);
    case 7U:
        return execute_position_increase(ctx, activity,
                                         &value->typed->increase);
    case 8U:
        return execute_position_close(ctx, activity, &value->typed->close);
    case 9U:
        return execute_funding_tick(ctx, activity, &value->typed->tick);
    case 10U:
        return execute_liquidate(ctx, activity, &value->typed->liquidate);
    default:
        break;
    }
    return execute_adl(ctx, activity, &value->typed->adl);
}

static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_PERPS, root);
}

const lxp_module_iface *lx_perps_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_PERPS, 1U, "perps", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_state_root, NULL
    };
    return &iface;
}
