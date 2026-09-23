#include "layerx/lx_spot.h"
#include "../asset/committed.h"

#include "lx_spot_codec.h"

#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_SPOT_MARKET_CREATE, LX_SPOT_ORDER_PLACE, LX_SPOT_ORDER_CANCEL,
    LX_SPOT_MARKET_HALT, LX_SPOT_MARKET_RESUME
};

typedef union spot_typed_command {
    lx_spot_market market;
    lx_spot_order_command order;
    lx_spot_cancel_command cancel;
    lx_spot_market_command state;
} spot_typed_command;

typedef struct spot_decoded {
    uint16_t ordinal;
    size_t payload_length;
    spot_typed_command typed;
} spot_decoded;

typedef struct spot_assets {
    lxp_transfer_asset_state states[2];
} spot_assets;

typedef struct spot_settlement {
    lxp_transfer_set *set;
    lxp_transfer_source_authority sources[3];
    size_t source_count;
} spot_settlement;

static const lxp_u128 u128_zero = { 0U, 0U };
static const lxp_u128 u128_max = { UINT64_MAX, UINT64_MAX };

static lxp_result work_alloc(lxp_module_ctx *ctx, size_t size,
                             size_t alignment, void **memory)
{
    lxp_result status = lxp_ctx_arena_alloc(ctx, size, alignment, memory);
    if (status != LXP_OK) return status;
    (void)memset(*memory, 0, size);
    return LXP_OK;
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

static lxp_result asset_state(lxp_module_ctx *ctx, const uint8_t asset_id[32],
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
    if (ctx->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT) {
        const lx_asset_record *committed;
        lxp_result status =
            lxp_module_committed_asset(ctx, asset_id, &committed);
        return status == LXP_OK ? lx_asset_transfer_state(committed, state) :
                                  status;
    }
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

static lxp_result market_assets(lxp_module_ctx *ctx,
                                const lx_spot_market *market,
                                spot_assets *assets)
{
    lxp_result status = asset_state(ctx, market->base_asset,
                                    &assets->states[0]);
    if (status == LXP_OK)
        status = asset_state(ctx, market->quote_asset, &assets->states[1]);
    if (status != LXP_OK) return status;
    if (!assets->states[0].registered || !assets->states[1].registered)
        return LXP_ERR_ASSET_MISMATCH;
    if (assets->states[0].paused || assets->states[1].paused)
        return LXP_ERR_ASSET_PAUSED;
    return LXP_OK;
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

/* An order names two of the actor's own accounts: its main account or its
 * per-asset account for the asset that leg of the market trades. */
static bool account_owned_by_actor(const lx_account *account,
                                   const lxp_activity *activity,
                                   const uint8_t asset_id[32])
{
    static const char hex[] = "0123456789abcdef";
    static const uint8_t asset_segment[] = ":asset:";
    size_t did_length;
    size_t offset;
    size_t i;
    if (account == NULL || activity == NULL ||
        activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U)
        return false;
    did_length = activity->actor_did.length;
    if ((size_t)account->name_length <= 6U + did_length ||
        memcmp(account->name, "agent:", 6U) != 0 ||
        memcmp(account->name + 6U, activity->actor_did.bytes, did_length) !=
            0)
        return false;
    if (account->has_asset && memcmp(account->asset_id, asset_id, 32U) != 0)
        return false;
    offset = 6U + did_length;
    if (account->kind == LX_ACCOUNT_AGENT_MAIN)
        return (size_t)account->name_length == offset + 5U &&
               memcmp(account->name + offset, ":main", 5U) == 0;
    if (account->kind != LX_ACCOUNT_AGENT_ASSET ||
        (size_t)account->name_length !=
            offset + sizeof(asset_segment) - 1U + 64U ||
        memcmp(account->name + offset, asset_segment,
               sizeof(asset_segment) - 1U) != 0)
        return false;
    offset += sizeof(asset_segment) - 1U;
    for (i = 0U; i < 32U; ++i)
        if (account->name[offset + i * 2U] != (uint8_t)hex[asset_id[i] >> 4U] ||
            account->name[offset + i * 2U + 1U] !=
                (uint8_t)hex[asset_id[i] & 15U])
            return false;
    return true;
}

static lxp_result order_accounts(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lx_spot_market *market,
                                 const uint8_t base_account_id[32],
                                 const uint8_t quote_account_id[32],
                                 lx_account **base_account,
                                 lx_account **quote_account)
{
    lxp_result status = lxp_ctx_account_find(ctx, base_account_id,
                                             base_account);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, quote_account_id, quote_account);
    if (status != LXP_OK) return status;
    if (!account_owned_by_actor(*base_account, activity, market->base_asset) ||
        !account_owned_by_actor(*quote_account, activity,
                                market->quote_asset))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

static lxp_result multiple_check(lxp_u128 value, lxp_u128 unit)
{
    lxp_u256 widened = { { value.lo, value.hi, 0U, 0U } };
    lxp_u128 quotient;
    lxp_u128 residue;
    lxp_result status = lxp_u256_div_floor(widened, unit, &quotient,
                                           &residue);
    if (status != LXP_OK) return status;
    return lxp_u128_is_zero(residue) ? LXP_OK : LXP_ERR_NON_CANONICAL;
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

static lxp_result settlement_open(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  lx_account *sequence_account,
                                  const spot_assets *assets,
                                  spot_settlement *settlement)
{
    void *memory = NULL;
    lxp_result status = work_alloc(ctx, sizeof(lxp_transfer_set),
                                   _Alignof(lxp_transfer_set), &memory);
    if (status != LXP_OK) return status;
    (void)memset(settlement, 0, sizeof(*settlement));
    settlement->set = (lxp_transfer_set *)memory;
    settlement->set->context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    settlement->set->context.expires_at = activity->timestamp_bound.not_after;
    settlement->set->context.actor_sequence = activity->account_sequence;
    settlement->set->context.sequence_account = sequence_account;
    settlement->set->context.debit_authority_kind = LXP_AUTH_OWNER;
    settlement->set->context.assets = assets->states;
    settlement->set->context.asset_count = 2U;
    return LXP_OK;
}

static lxp_result settlement_source(spot_settlement *settlement,
                                    const lx_account *from, bool escrow)
{
    size_t i;
    for (i = 0U; i < settlement->source_count; ++i)
        if (memcmp(settlement->sources[i].authorized_from, from->id, 32U) ==
            0)
            return settlement->sources[i].protocol_system_capability == escrow ?
                LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
    if (settlement->source_count == sizeof(settlement->sources) /
                                        sizeof(settlement->sources[0]))
        return LXP_FATAL_INVARIANT;
    (void)memcpy(settlement->sources[settlement->source_count].authorized_from,
                 from->id, 32U);
    settlement->sources[settlement->source_count].debit_authority_kind =
        escrow ? LXP_AUTH_PROTOCOL_MODULE : LXP_AUTH_OWNER;
    settlement->sources[settlement->source_count].protocol_system_capability =
        escrow;
    ++settlement->source_count;
    return LXP_OK;
}

/* Zero legs are left out, so every recorded source debits at least once. */
static lxp_result settlement_leg(spot_settlement *settlement,
                                 lx_account *from, lx_account *to,
                                 const uint8_t asset_id[32], lxp_u128 amount,
                                 uint16_t reason, bool escrow)
{
    lxp_transfer_leg *leg;
    lxp_result status;
    if (lxp_u128_is_zero(amount)) return LXP_OK;
    if (settlement->set->leg_count == LX_SPOT_DISPATCH_MAX_LEGS)
        return LXP_ERR_LENGTH_LIMIT;
    status = settlement_source(settlement, from, escrow);
    if (status != LXP_OK) return status;
    leg = &settlement->set->legs[settlement->set->leg_count++];
    leg->from = from;
    leg->to = to;
    (void)memcpy(leg->asset_id, asset_id, 32U);
    leg->amount = amount;
    leg->reason = reason;
    leg->supply_mode = LXP_TRANSFER_CONSERVED;
    return LXP_OK;
}

static lxp_result settlement_emit(lxp_module_ctx *ctx,
                                  spot_settlement *settlement)
{
    void *memory = NULL;
    lxp_result status;
    if (settlement->set->leg_count == 0U) return LXP_OK;
    status = work_alloc(ctx, sizeof(lxp_receipt), _Alignof(lxp_receipt),
                        &memory);
    if (status != LXP_OK) return status;
    settlement->set->context.source_authorities = settlement->sources;
    settlement->set->context.source_authority_count =
        settlement->source_count;
    return lxp_ctx_emit_transfer_set(ctx, settlement->set,
                                     (lxp_receipt *)memory);
}

static lxp_result escrow_accounts(lxp_module_ctx *ctx,
                                  const lx_spot_market *market,
                                  lx_account **base_escrow,
                                  lx_account **quote_escrow)
{
    lxp_result status = lxp_ctx_account_find(ctx, market->base_escrow_id,
                                             base_escrow);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, market->quote_escrow_id,
                                      quote_escrow);
    if (status != LXP_OK) return status;
    return (*base_escrow)->kind == LX_ACCOUNT_MODULE_VALUE &&
                   (*quote_escrow)->kind == LX_ACCOUNT_MODULE_VALUE ?
               LXP_OK : LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

static lxp_result market_loaded(lxp_module_ctx *ctx,
                                const uint8_t market_id[32],
                                bool refuse_halted, lx_spot_market *market)
{
    lxp_result status = lx_spot_market_lookup(ctx, market_id, market);
    if (status != LXP_OK) return status;
    return refuse_halted && market->halted ? LXP_ERR_MARKET_HALTED : LXP_OK;
}

static lxp_result validate_market_create(lxp_module_ctx *ctx,
                                         const lxp_authority_resolved *authority,
                                         const lx_spot_market *market)
{
    lx_spot_market existing;
    spot_assets assets;
    size_t count = 0U;
    lxp_result status;
    if (memcmp(authority->actor, market->administrator, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = market_assets(ctx, market, &assets);
    if (status != LXP_OK) return status;
    status = lx_spot_market_lookup(ctx, market->market_id, &existing);
    if (status == LXP_OK) return LXP_ERR_MARKET_ALREADY_EXISTS;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = lx_spot_market_count(ctx, &count);
    if (status != LXP_OK) return status;
    return count < (size_t)LX_SPOT_MARKET_CAPACITY ? LXP_OK :
                                                     LXP_ERR_LENGTH_LIMIT;
}

static lxp_result validate_market_state(lxp_module_ctx *ctx,
                                        const lxp_authority_resolved *authority,
                                        const lx_spot_market_command *command,
                                        bool halt)
{
    lx_spot_market market;
    lxp_result status = lx_spot_market_lookup(ctx, command->market_id,
                                              &market);
    if (status != LXP_OK) return status;
    if (memcmp(authority->actor, market.administrator, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return market.halted == halt ? LXP_ERR_AGREEMENT_STATE : LXP_OK;
}

static lxp_result validate_order_place(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lx_spot_order_command *command)
{
    lx_spot_market market;
    lx_spot_order existing;
    lx_account *base_account;
    lx_account *quote_account;
    lx_account *actor_main;
    lxp_u128 notional;
    lxp_result status = market_loaded(ctx, command->market_id, true, &market);
    if (status != LXP_OK) return status;
    status = multiple_check(command->quantity, market.lot_size);
    if (status == LXP_OK && command->kind == LX_SPOT_ORDER_LIMIT)
        status = multiple_check(command->price, market.tick_size);
    if (status == LXP_OK && command->kind == LX_SPOT_ORDER_LIMIT)
        status = lx_spot_notional(command->price, command->quantity,
                                  &notional);
    if (status != LXP_OK) return status;
    status = actor_account(ctx, activity, &actor_main);
    if (status == LXP_OK)
        status = order_accounts(ctx, activity, &market,
                                command->base_account_id,
                                command->quote_account_id, &base_account,
                                &quote_account);
    if (status != LXP_OK) return status;
    status = lx_spot_order_lookup(ctx, command->market_id, command->order_id,
                                  &existing);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK : status;
}

static lxp_result validate_order_cancel(lxp_module_ctx *ctx,
                                        const lxp_activity *activity,
                                        const lx_spot_cancel_command *command)
{
    lx_spot_market market;
    lx_spot_order order;
    lx_account *base_account;
    lx_account *quote_account;
    lxp_result status = market_loaded(ctx, command->market_id, false,
                                      &market);
    if (status != LXP_OK) return status;
    status = lx_spot_order_lookup(ctx, command->market_id, command->order_id,
                                  &order);
    if (status != LXP_OK) return status;
    return order_accounts(ctx, activity, &market, order.base_account_id,
                          order.quote_account_id, &base_account,
                          &quote_account);
}

static lxp_result execute_market_create(lxp_module_ctx *ctx,
                                        const lx_spot_market *market)
{
    lx_account *escrow;
    bool created = false;
    lxp_result status = lxp_ctx_account_stage_module_value(
        ctx, market->base_escrow_id, market->base_asset, &escrow, &created);
    if (status == LXP_OK)
        status = lxp_ctx_account_stage_module_value(
            ctx, market->quote_escrow_id, market->quote_asset, &escrow,
            &created);
    if (status == LXP_OK) status = lx_spot_market_put(ctx, market);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_SPOT_EVENT_MARKET_CREATED,
                                 market->market_id, market->base_asset,
                                 market->quote_asset, 32U);
}

static lxp_result execute_market_state(lxp_module_ctx *ctx,
                                       const lx_spot_market_command *command,
                                       bool halt)
{
    lx_spot_market market;
    lxp_result status = lx_spot_market_lookup(ctx, command->market_id,
                                              &market);
    if (status != LXP_OK) return status;
    market.halted = halt;
    status = lx_spot_market_put(ctx, &market);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx,
                                 halt ? LX_SPOT_EVENT_MARKET_HALTED :
                                        LX_SPOT_EVENT_MARKET_RESUMED,
                                 market.market_id, market.administrator,
                                 NULL, 0U);
}

typedef struct spot_place {
    lx_spot_market market;
    spot_assets assets;
    spot_settlement settlement;
    lx_spot_book *before;
    lx_spot_book *after;
    lx_spot_fill fills[LX_SPOT_DISPATCH_MAX_FILLS];
    size_t fill_count;
    lx_spot_order order;
    lx_account *actor_main;
    lx_account *base_account;
    lx_account *quote_account;
    lx_account *base_escrow;
    lx_account *quote_escrow;
} spot_place;

static lxp_result place_load(lxp_module_ctx *ctx, const lxp_activity *activity,
                             const lx_spot_order_command *command,
                             spot_place *place)
{
    void *memory = NULL;
    lxp_result status = market_loaded(ctx, command->market_id, true,
                                      &place->market);
    if (status == LXP_OK)
        status = market_assets(ctx, &place->market, &place->assets);
    if (status == LXP_OK)
        status = actor_account(ctx, activity, &place->actor_main);
    if (status == LXP_OK)
        status = order_accounts(ctx, activity, &place->market,
                                command->base_account_id,
                                command->quote_account_id,
                                &place->base_account, &place->quote_account);
    if (status == LXP_OK)
        status = escrow_accounts(ctx, &place->market, &place->base_escrow,
                                 &place->quote_escrow);
    if (status == LXP_OK)
        status = work_alloc(ctx, sizeof(lx_spot_book), _Alignof(lx_spot_book),
                            &memory);
    if (status != LXP_OK) return status;
    place->before = (lx_spot_book *)memory;
    status = work_alloc(ctx, sizeof(lx_spot_book), _Alignof(lx_spot_book),
                        &memory);
    if (status != LXP_OK) return status;
    place->after = (lx_spot_book *)memory;
    status = lx_spot_book_load(ctx, command->market_id, place->before);
    if (status != LXP_OK) return status;
    *place->after = *place->before;
    (void)memset(&place->order, 0, sizeof(place->order));
    (void)memcpy(place->order.order_id, command->order_id, 32U);
    (void)memcpy(place->order.market_id, command->market_id, 32U);
    (void)memcpy(place->order.base_account_id, command->base_account_id, 32U);
    (void)memcpy(place->order.quote_account_id, command->quote_account_id,
                 32U);
    place->order.side = command->side;
    place->order.quantity = command->quantity;
    place->order.remaining = command->quantity;
    place->order.global_sequence = lxp_ctx_global_sequence(ctx);
    if (command->kind == LX_SPOT_ORDER_LIMIT)
        place->order.price = command->price;
    else
        place->order.price =
            command->side == LX_SPOT_SIDE_BID ? u128_max : u128_zero;
    return LXP_OK;
}

/* The escrow the taker posts before its fills settle: the full sell-side
 * amount for a limit order and a market ask, and exactly the traded quote
 * for a market bid, which has no price to reserve against. */
static lxp_result place_escrow_in(const lx_spot_order_command *command,
                                  const spot_place *place, lxp_u128 *amount)
{
    size_t i;
    if (command->side == LX_SPOT_SIDE_ASK) {
        *amount = command->quantity;
        return LXP_OK;
    }
    if (command->kind == LX_SPOT_ORDER_LIMIT)
        return lx_spot_notional(command->price, command->quantity, amount);
    *amount = u128_zero;
    for (i = 0U; i < place->fill_count; ++i) {
        lxp_result status = lxp_u128_add(*amount, place->fills[i].notional,
                                         amount);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result place_fill_legs(lxp_module_ctx *ctx, spot_place *place)
{
    const lx_spot_market *market = &place->market;
    size_t i;
    for (i = 0U; i < place->fill_count; ++i) {
        const lx_spot_fill *fill = &place->fills[i];
        lx_account *maker_base;
        lx_account *maker_quote;
        lx_account *buyer;
        lx_account *seller;
        lxp_result status = lxp_ctx_account_find(
            ctx, fill->maker_base_account_id, &maker_base);
        if (status == LXP_OK)
            status = lxp_ctx_account_find(ctx, fill->maker_quote_account_id,
                                          &maker_quote);
        if (status != LXP_OK) return status;
        buyer = place->order.side == LX_SPOT_SIDE_BID ? place->base_account :
                                                        maker_base;
        seller = place->order.side == LX_SPOT_SIDE_BID ? maker_quote :
                                                         place->quote_account;
        status = settlement_leg(&place->settlement, place->base_escrow, buyer,
                                market->base_asset, fill->quantity,
                                LXP_REASON_ESCROW_CAPTURE, true);
        if (status == LXP_OK)
            status = settlement_leg(&place->settlement, place->quote_escrow,
                                    seller, market->quote_asset,
                                    fill->notional, LXP_REASON_ESCROW_CAPTURE,
                                    true);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

/* What the taker gets back: a bid's price improvement over its limit plus,
 * for an order that does not rest, the escrow of its unfilled remainder. */
static lxp_result place_refund(const lx_spot_order_command *command,
                               const spot_place *place, bool rests,
                               lxp_u128 *refund)
{
    lxp_u128 reserved;
    lxp_u128 improvement;
    lxp_result status;
    size_t i;
    *refund = u128_zero;
    if (command->side == LX_SPOT_SIDE_ASK) {
        if (!rests) *refund = place->order.remaining;
        return LXP_OK;
    }
    if (command->kind == LX_SPOT_ORDER_MARKET) return LXP_OK;
    for (i = 0U; i < place->fill_count; ++i) {
        status = lx_spot_notional(command->price, place->fills[i].quantity,
                                  &reserved);
        if (status == LXP_OK)
            status = lxp_u128_sub(reserved, place->fills[i].notional,
                                  &improvement);
        if (status == LXP_OK) status = lxp_u128_add(*refund, improvement,
                                                    refund);
        if (status != LXP_OK) return status;
    }
    if (rests) return LXP_OK;
    status = lx_spot_notional(command->price, place->order.remaining,
                              &reserved);
    if (status != LXP_OK) return status;
    return lxp_u128_add(*refund, reserved, refund);
}

static lxp_result execute_order_place(lxp_module_ctx *ctx,
                                      const lxp_activity *activity,
                                      const lx_spot_order_command *command)
{
    spot_place *place;
    void *memory = NULL;
    lxp_u128 escrow_in;
    lxp_u128 refund;
    lx_account *owner_source;
    lx_account *sell_escrow;
    const uint8_t *sell_asset;
    uint8_t tail[33];
    bool rests;
    lxp_result status = work_alloc(ctx, sizeof(*place), _Alignof(spot_place),
                                   &memory);
    if (status != LXP_OK) return status;
    place = (spot_place *)memory;
    status = place_load(ctx, activity, command, place);
    if (status != LXP_OK) return status;
    status = lx_spot_book_match(place->after, &place->order, place->fills,
                                LX_SPOT_DISPATCH_MAX_FILLS,
                                &place->fill_count);
    if (status != LXP_OK) return status;
    if (command->kind == LX_SPOT_ORDER_MARKET && place->fill_count == 0U)
        return LXP_ERR_AGREEMENT_STATE;
    rests = command->kind == LX_SPOT_ORDER_LIMIT &&
            command->time_in_force == LX_SPOT_TIF_GTC &&
            !lxp_u128_is_zero(place->order.remaining);
    status = place_escrow_in(command, place, &escrow_in);
    if (status == LXP_OK) status = place_refund(command, place, rests, &refund);
    if (status == LXP_OK)
        status = settlement_open(ctx, activity, place->actor_main,
                                 &place->assets, &place->settlement);
    if (status != LXP_OK) return status;
    owner_source = command->side == LX_SPOT_SIDE_BID ? place->quote_account :
                                                       place->base_account;
    sell_escrow = command->side == LX_SPOT_SIDE_BID ? place->quote_escrow :
                                                      place->base_escrow;
    sell_asset = command->side == LX_SPOT_SIDE_BID ?
        place->market.quote_asset : place->market.base_asset;
    status = settlement_leg(&place->settlement, owner_source, sell_escrow,
                            sell_asset, escrow_in, LXP_REASON_ESCROW_LOCK,
                            false);
    if (status == LXP_OK) status = place_fill_legs(ctx, place);
    if (status == LXP_OK)
        status = settlement_leg(&place->settlement, sell_escrow, owner_source,
                                sell_asset, refund, LXP_REASON_ESCROW_RELEASE,
                                true);
    if (status != LXP_OK) return status;
    if (rests) {
        if (place->after->count == LX_SPOT_BOOK_CAPACITY)
            return LXP_ERR_LENGTH_LIMIT;
        place->order.escrowed = place->order.remaining;
        if (command->side == LX_SPOT_SIDE_BID)
            status = lx_spot_notional(command->price, place->order.remaining,
                                      &place->order.escrowed);
        if (status != LXP_OK) return status;
        place->after->orders[place->after->count++] = place->order;
    }
    status = settlement_emit(ctx, &place->settlement);
    if (status == LXP_OK)
        status = lx_spot_book_persist(ctx, place->before, place->after);
    if (status != LXP_OK) return status;
    tail[0] = (uint8_t)place->fill_count;
    status = lxp_u128_to_be(command->quantity, tail + 1U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(rests ? place->order.remaining : u128_zero,
                                tail + 17U);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_SPOT_EVENT_ORDER_PLACED,
                                 command->order_id, command->market_id,
                                 tail, sizeof(tail));
}

static lxp_result execute_order_cancel(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lx_spot_cancel_command *command)
{
    lx_spot_market market;
    lx_spot_order order;
    spot_assets assets;
    spot_settlement settlement;
    lx_account *actor_main;
    lx_account *base_account;
    lx_account *quote_account;
    lx_account *base_escrow;
    lx_account *quote_escrow;
    uint8_t tail[16];
    bool bid;
    lxp_result status = market_loaded(ctx, command->market_id, false,
                                      &market);
    if (status == LXP_OK)
        status = lx_spot_order_lookup(ctx, command->market_id,
                                      command->order_id, &order);
    if (status == LXP_OK) status = market_assets(ctx, &market, &assets);
    if (status == LXP_OK) status = actor_account(ctx, activity, &actor_main);
    if (status == LXP_OK)
        status = order_accounts(ctx, activity, &market, order.base_account_id,
                                order.quote_account_id, &base_account,
                                &quote_account);
    if (status == LXP_OK)
        status = escrow_accounts(ctx, &market, &base_escrow, &quote_escrow);
    if (status == LXP_OK)
        status = settlement_open(ctx, activity, actor_main, &assets,
                                 &settlement);
    if (status != LXP_OK) return status;
    bid = order.side == LX_SPOT_SIDE_BID;
    status = settlement_leg(&settlement, bid ? quote_escrow : base_escrow,
                            bid ? quote_account : base_account,
                            bid ? market.quote_asset : market.base_asset,
                            order.escrowed, LXP_REASON_ESCROW_RELEASE, true);
    if (status == LXP_OK) status = settlement_emit(ctx, &settlement);
    if (status == LXP_OK)
        status = lx_spot_order_delete(ctx, command->market_id,
                                      command->order_id);
    if (status == LXP_OK) status = lxp_u128_to_be(order.escrowed, tail);
    if (status != LXP_OK) return status;
    return emit_identifier_event(ctx, LX_SPOT_EVENT_ORDER_CANCELLED,
                                 command->order_id, command->market_id,
                                 tail, sizeof(tail));
}

lxp_result lx_spot_dispatch(lxp_module_ctx *ctx, const lxp_activity *activity,
                            const lxp_authority_resolved *authority,
                            uint16_t ordinal, const void *command)
{
    const spot_typed_command *typed = (const spot_typed_command *)command;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        typed == NULL || ctx->module_id != LXP_MODULE_SPOT)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    switch (ordinal) {
    case 1U:
        return execute_market_create(ctx, &typed->market);
    case 2U:
        return execute_order_place(ctx, activity, &typed->order);
    case 3U:
        return execute_order_cancel(ctx, activity, &typed->cancel);
    case 4U:
        return execute_market_state(ctx, &typed->state, true);
    case 5U:
        return execute_market_state(ctx, &typed->state, false);
    default:
        break;
    }
    return LXP_ERR_UNKNOWN_ACTIVITY;
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
    spot_decoded *value;
    void *memory = NULL;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U ||
        ordinal > (uint16_t)LX_SPOT_ORDINAL_COUNT || payload == NULL ||
        payload_length == 0U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = work_alloc(ctx, sizeof(*value), _Alignof(spot_decoded), &memory);
    if (status != LXP_OK) return status;
    value = (spot_decoded *)memory;
    value->ordinal = ordinal;
    value->payload_length = payload_length;
    switch (ordinal) {
    case 1U:
        status = lx_spot_market_decode(payload, payload_length,
                                       &value->typed.market);
        break;
    case 2U:
        status = lx_spot_order_command_decode(payload, payload_length,
                                              &value->typed.order);
        break;
    case 3U:
        status = lx_spot_cancel_command_decode(payload, payload_length,
                                               &value->typed.cancel);
        break;
    default:
        status = lx_spot_market_command_decode(payload, payload_length,
                                               &value->typed.state);
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
    const spot_decoded *value = (const spot_decoded *)decoded;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->ordinal == 0U ||
        value->ordinal > (uint16_t)LX_SPOT_ORDINAL_COUNT)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (ctx->module_id != LXP_MODULE_SPOT ||
        lxp_activity_module_id(activity->activity_type) != LXP_MODULE_SPOT ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
    if (status != LXP_OK) return status;
    switch (value->ordinal) {
    case 1U:
        return validate_market_create(ctx, authority, &value->typed.market);
    case 2U:
        return validate_order_place(ctx, activity, &value->typed.order);
    case 3U:
        return validate_order_cancel(ctx, activity, &value->typed.cancel);
    case 4U:
        return validate_market_state(ctx, authority, &value->typed.state,
                                     true);
    default:
        break;
    }
    return validate_market_state(ctx, authority, &value->typed.state, false);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const spot_decoded *value = (const spot_decoded *)decoded;
    (void)effects;
    if (value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lx_spot_dispatch(ctx, activity, authority, value->ordinal,
                            &value->typed);
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
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_SPOT, root);
}

const lxp_module_iface *lx_spot_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_SPOT, 1U, "spot", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_state_root, NULL
    };
    return &iface;
}
