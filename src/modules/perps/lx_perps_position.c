#include "layerx/lx_perps.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_perps_authority_check(const lx_account *account,
                                    lxp_authorization_kind kind,
                                    uint16_t origin_module_id,
                                    uint16_t reason)
{
    if (account == NULL) return LXP_ERR_NON_CANONICAL;
    if (account->kind != LX_ACCOUNT_AGENT_MARGIN) return LXP_OK;
    if (kind != LXP_AUTH_PROTOCOL_MODULE ||
        origin_module_id != LXP_MODULE_PERPS ||
        (reason != LXP_REASON_MARGIN_RELEASE &&
         reason != LXP_REASON_TRADING_LOSS &&
         reason != LXP_REASON_LIQUIDATION_FEE &&
         reason != LXP_REASON_ADL))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

lxp_result lx_perps_position_lookup(lx_perps_position_store *store,
                                    const uint8_t position_id[32],
                                    lx_perps_position **position)
{
    size_t i;
    if (store == NULL || position_id == NULL || position == NULL ||
        store->count > LX_PERPS_POSITION_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->positions[i].position_id, position_id, 32U) == 0) {
            *position = &store->positions[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

static lxp_result emit_margin(lxp_module_ctx *ctx, lx_account *from,
                              lx_account *to,
                              const lxp_transfer_asset_state *asset,
                              lxp_u128 amount, lxp_transfer_context context,
                              uint16_t reason, lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    if (ctx == NULL || from == NULL || to == NULL || asset == NULL ||
        receipt == NULL || lxp_u128_is_zero(amount) || !asset->registered ||
        asset->paused)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.leg_count = 1U;
    set.legs[0].from = from;
    set.legs[0].to = to;
    (void)memcpy(set.legs[0].asset_id, asset->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = reason;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.context = context;
    set.context.assets = asset;
    set.context.asset_count = 1U;
    if (memcmp(set.context.authorized_from, from->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    (void)memcpy(source.authorized_from, from->id, 32U);
    source.debit_authority_kind = set.context.debit_authority_kind;
    source.protocol_system_capability =
        set.context.protocol_system_capability;
    set.context.source_authorities = &source;
    set.context.source_authority_count = 1U;
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_perps_margin_post(lxp_module_ctx *ctx,
                                lx_account *owner_main,
                                lx_account *margin_account,
                                const lxp_transfer_asset_state *asset,
                                lxp_u128 amount,
                                lxp_transfer_context context,
                                lxp_receipt *receipt)
{
    if (owner_main == NULL || margin_account == NULL ||
        owner_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        margin_account->kind != LX_ACCOUNT_AGENT_MARGIN)
        return LXP_ERR_NON_CANONICAL;
    context.debit_authority_kind = context.debit_authority_kind == 0 ?
        LXP_AUTH_OWNER : context.debit_authority_kind;
    (void)memcpy(context.authorized_from, owner_main->id, 32U);
    return emit_margin(ctx, owner_main, margin_account, asset, amount, context,
                       LXP_REASON_MARGIN_POST, receipt);
}

lxp_result lx_perps_margin_release(lxp_module_ctx *ctx,
                                   lx_account *margin_account,
                                   lx_account *owner_main,
                                   const lxp_transfer_asset_state *asset,
                                   lxp_u128 amount,
                                   lxp_transfer_context context,
                                   lxp_receipt *receipt)
{
    if (owner_main == NULL || margin_account == NULL ||
        owner_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        margin_account->kind != LX_ACCOUNT_AGENT_MARGIN)
        return LXP_ERR_NON_CANONICAL;
    context.protocol_system_capability = true;
    context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(context.authorized_from, margin_account->id, 32U);
    return emit_margin(ctx, margin_account, owner_main, asset, amount, context,
                       LXP_REASON_MARGIN_RELEASE, receipt);
}

static lxp_result position_validate(const lx_perps_position_request *request)
{
    const lx_perps_position *position;
    if (request == NULL || request->store == NULL ||
        request->store->count > LX_PERPS_POSITION_CAPACITY ||
        request->owner_main == NULL || request->margin_account == NULL ||
        request->asset == NULL || lxp_u128_is_zero(request->margin_amount))
        return LXP_ERR_NON_CANONICAL;
    position = &request->position;
    if (lxp_ct_is_zero(position->position_id, 32U) ||
        lxp_ct_is_zero(position->market_id, 32U) ||
        memcmp(position->owner_main_account_id,
               request->owner_main->id, 32U) != 0 ||
        memcmp(position->margin_account_id,
               request->margin_account->id, 32U) != 0 ||
        memcmp(position->asset_id, request->asset->asset_id, 32U) != 0 ||
        (position->side != LX_PERPS_SIDE_BUY &&
         position->side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(position->size) ||
        lxp_u128_is_zero(position->entry_notional))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_perps_position_open_execute(
    lxp_module_ctx *ctx, const lx_perps_position_request *request,
    lxp_receipt *receipt)
{
    lx_perps_position *existing;
    lx_perps_position position;
    lxp_result status = position_validate(request);
    if (status != LXP_OK || ctx == NULL || receipt == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    if (request->store->count == LX_PERPS_POSITION_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (lx_perps_position_lookup(request->store,
                                 request->position.position_id,
                                 &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    status = lx_perps_margin_post(ctx, request->owner_main,
                                  request->margin_account, request->asset,
                                  request->margin_amount, request->context,
                                  receipt);
    if (status != LXP_OK) return status;
    position = request->position;
    position.open = true;
    request->store->positions[request->store->count++] = position;
    request->margin_account->has_open_reference = true;
    return LXP_OK;
}

lxp_result lx_perps_position_increase_execute(
    lxp_module_ctx *ctx, const lx_perps_position_request *request,
    lxp_receipt *receipt)
{
    lx_perps_position *position;
    lxp_u128 next_size;
    lxp_u128 next_notional;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        receipt == NULL || lxp_u128_is_zero(request->size_delta) ||
        lxp_u128_is_zero(request->notional_delta))
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_position_lookup(request->store,
                                      request->position.position_id,
                                      &position);
    if (status != LXP_OK) return status;
    if (!position->open) return LXP_ERR_MARKET_HALTED;
    status = lxp_u128_add(position->size, request->size_delta, &next_size);
    if (status == LXP_OK)
        status = lxp_u128_add(position->entry_notional,
                              request->notional_delta, &next_notional);
    if (status != LXP_OK) return status;
    status = lx_perps_margin_post(ctx, request->owner_main,
                                  request->margin_account, request->asset,
                                  request->margin_amount, request->context,
                                  receipt);
    if (status != LXP_OK) return status;
    position->size = next_size;
    position->entry_notional = next_notional;
    return LXP_OK;
}

lxp_result lx_perps_position_close_execute(
    lxp_module_ctx *ctx, lx_perps_position_store *store,
    const uint8_t position_id[32], lx_account *margin_account,
    lx_account *owner_main, const lxp_transfer_asset_state *asset,
    lxp_transfer_context context, lxp_receipt *receipt)
{
    lx_perps_position *position;
    lxp_u128 amount;
    lxp_result status;
    if (ctx == NULL || store == NULL || position_id == NULL ||
        margin_account == NULL || owner_main == NULL || asset == NULL ||
        receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_position_lookup(store, position_id, &position);
    if (status != LXP_OK) return status;
    if (!position->open || memcmp(position->margin_account_id,
                                  margin_account->id, 32U) != 0)
        return LXP_ERR_ACCOUNT_NOT_EMPTY;
    amount = margin_account->balance;
    if (lxp_u128_is_zero(amount)) return LXP_ERR_ACCOUNT_NOT_EMPTY;
    status = lx_perps_margin_release(ctx, margin_account, owner_main, asset,
                                     amount, context, receipt);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(margin_account->balance))
        return LXP_FATAL_INVARIANT;
    position->open = false;
    margin_account->has_open_reference = false;
    return LXP_OK;
}

lxp_result lx_perps_fill_notional(const lx_perps_market *market,
                                  lxp_u128 price, lxp_u128 quantity,
                                  lxp_u128 *notional)
{
    lxp_u128 remainder;
    if (market == NULL || notional == NULL || lxp_u128_is_zero(price) ||
        lxp_u128_is_zero(quantity))
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(market->price_scale))
        return LXP_ERR_PARAMETER_BOUNDS;
    if (lxp_u128_mul_div_floor(price, quantity, market->price_scale,
                               notional, &remainder) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    if (!lxp_u128_is_zero(remainder) || lxp_u128_is_zero(*notional))
        return LXP_ERR_PARAMETER_BOUNDS;
    return LXP_OK;
}

lxp_result lx_perps_price_deviation_check(const lx_perps_market *market,
                                          lxp_u128 oracle_price,
                                          lxp_u128 price)
{
    lxp_u128 difference;
    lxp_u128 allowed;
    lxp_result status;
    if (market == NULL || lxp_u128_is_zero(oracle_price) ||
        lxp_u128_is_zero(price))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_cmp(price, oracle_price) >= 0 ?
        lxp_u128_sub(price, oracle_price, &difference) :
        lxp_u128_sub(oracle_price, price, &difference);
    if (status == LXP_OK)
        status = lxp_u128_mul_bps_floor(
            oracle_price, market->maximum_deviation_basis_points, &allowed);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    return lxp_u128_cmp(difference, allowed) <= 0 ? LXP_OK :
                                                    LXP_ERR_ORACLE_DEVIATION;
}

static lxp_result fill_margin_check(const lx_perps_fill_request *request,
                                    lxp_u128 notional)
{
    lxp_u128 required;
    if (lxp_u128_mul_bps_ceil(notional,
                              request->market->initial_margin_ratio_bps,
                              &required) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    return lxp_u128_cmp(request->margin_account->balance, required) < 0 ?
        LXP_ERR_MARGIN_INSUFFICIENT : LXP_OK;
}

static lxp_result fill_interest_add(lx_perps_funding_state *funding,
                                    lx_perps_side side, lxp_u128 notional)
{
    lxp_u128 *target = side == LX_PERPS_SIDE_BUY ?
        &funding->long_open_notional : &funding->short_open_notional;
    return lxp_u128_add(*target, notional, target) == LXP_OK ?
        LXP_OK : LXP_ERR_OVERFLOW;
}

static lxp_result fill_interest_remove(lx_perps_funding_state *funding,
                                       lx_perps_side side, lxp_u128 notional)
{
    lxp_u128 *target = side == LX_PERPS_SIDE_BUY ?
        &funding->long_open_notional : &funding->short_open_notional;
    if (lxp_u128_cmp(*target, notional) < 0) return LXP_ERR_CONSERVATION;
    return lxp_u128_sub(*target, notional, target) == LXP_OK ?
        LXP_OK : LXP_ERR_CONSERVATION;
}

/* Moves the entry funding index so the funding a position already accrued
 * stays owed after its entry notional changes from old_notional to
 * new_notional. Rounds toward the side that pays. */
static lxp_result fill_funding_rebase(const lx_perps_position *position,
                                      lxp_i128 funding_index,
                                      lxp_u128 new_notional,
                                      lxp_i128 *rebased)
{
    lxp_i128 delta;
    lxp_i128 scaled;
    lxp_u128 remainder;
    bool trader_pays;
    if (lxp_i128_sub(funding_index, position->funding_index_at_entry,
                     &delta) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    if (lxp_u128_is_zero(delta.magnitude)) {
        *rebased = funding_index;
        return LXP_OK;
    }
    trader_pays = position->side == LX_PERPS_SIDE_BUY ? !delta.negative :
                                                        delta.negative;
    if (lxp_u128_mul_div_floor(delta.magnitude, position->entry_notional,
                               new_notional, &scaled.magnitude,
                               &remainder) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    if (trader_pays && !lxp_u128_is_zero(remainder) &&
        lxp_u128_add(scaled.magnitude, (lxp_u128){ 0U, 1U },
                     &scaled.magnitude) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    scaled.negative = delta.negative && !lxp_u128_is_zero(scaled.magnitude);
    return lxp_i128_sub(funding_index, scaled, rebased) == LXP_OK ?
        LXP_OK : LXP_ERR_OVERFLOW;
}

static lxp_result fill_party_note(const lx_perps_fill_request *request,
                                  bool closed)
{
    lx_perps_fill_settlement *settlement = request->settlement;
    size_t i;
    for (i = 0U; i < settlement->party_count; ++i) {
        lx_perps_fill_party *party = &settlement->parties[i];
        if (party->margin_account != request->margin_account) continue;
        if (party->owner_main != request->owner_main)
            return LXP_ERR_NON_CANONICAL;
        party->closed = party->closed || closed;
        return LXP_OK;
    }
    if (settlement->party_count == LX_PERPS_FILL_PARTIES)
        return LXP_ERR_LENGTH_LIMIT;
    settlement->parties[settlement->party_count].owner_main =
        request->owner_main;
    settlement->parties[settlement->party_count].margin_account =
        request->margin_account;
    settlement->parties[settlement->party_count].closed = closed;
    ++settlement->party_count;
    return LXP_OK;
}

/* Queues the funding a position owes or is owed when its exposure goes to
 * zero, the same legs POSITION_CLOSE settles. The protocol cannot debit a
 * main account on behalf of a counterparty that did not sign this activity,
 * so a maker that owes funding refuses the fill. */
static lxp_result fill_funding_settle(const lx_perps_fill_request *request,
                                      const lx_perps_position *position)
{
    lx_perps_fill_settlement *settlement = request->settlement;
    lxp_transfer_leg *leg;
    lx_account *pool;
    lxp_i128 owed;
    lxp_result status = lx_perps_funding_owed(
        position, request->funding->funding_index, &owed);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(owed.magnitude)) return LXP_OK;
    if (!owed.negative && !request->owner_authorized)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (settlement->funding_leg_count == LX_PERPS_FILL_PARTIES)
        return LXP_ERR_LENGTH_LIMIT;
    pool = position->side == LX_PERPS_SIDE_BUY ?
        request->long_funding_account : request->short_funding_account;
    leg = &settlement->funding_legs[settlement->funding_leg_count];
    (void)memset(leg, 0, sizeof(*leg));
    leg->from = owed.negative ? pool : request->owner_main;
    leg->to = owed.negative ? request->owner_main : pool;
    (void)memcpy(leg->asset_id, request->asset->asset_id, 32U);
    leg->amount = owed.magnitude;
    leg->reason = LXP_REASON_FUNDING;
    leg->supply_mode = LXP_TRANSFER_CONSERVED;
    ++settlement->funding_leg_count;
    return LXP_OK;
}

static lxp_result fill_open(const lx_perps_fill_request *request,
                            lx_perps_position *slot, lxp_u128 quantity)
{
    lx_perps_position position;
    lxp_u128 notional;
    lxp_result status = lx_perps_fill_notional(request->market,
                                               request->price, quantity,
                                               &notional);
    if (status != LXP_OK) return status;
    status = fill_margin_check(request, notional);
    if (status != LXP_OK) return status;
    status = fill_interest_add(request->funding, request->side, notional);
    if (status != LXP_OK) return status;
    (void)memset(&position, 0, sizeof(position));
    (void)memcpy(position.position_id, request->margin_account->id, 32U);
    (void)memcpy(position.market_id, request->market->market_id, 32U);
    (void)memcpy(position.owner_main_account_id, request->owner_main->id,
                 32U);
    (void)memcpy(position.margin_account_id, request->margin_account->id,
                 32U);
    (void)memcpy(position.asset_id, request->asset->asset_id, 32U);
    position.side = request->side;
    position.size = quantity;
    position.entry_notional = notional;
    position.funding_index_at_entry = request->funding->funding_index;
    position.open = true;
    *slot = position;
    return LXP_OK;
}

static lxp_result fill_increase(const lx_perps_fill_request *request,
                                lx_perps_position *position)
{
    lxp_u128 notional;
    lxp_u128 next_size;
    lxp_u128 next_notional;
    lxp_i128 rebased;
    lxp_result status = lx_perps_fill_notional(request->market,
                                               request->price,
                                               request->quantity, &notional);
    if (status != LXP_OK) return status;
    if (lxp_u128_add(position->size, request->quantity, &next_size) !=
            LXP_OK ||
        lxp_u128_add(position->entry_notional, notional, &next_notional) !=
            LXP_OK)
        return LXP_ERR_OVERFLOW;
    status = fill_margin_check(request, next_notional);
    if (status != LXP_OK) return status;
    status = fill_funding_rebase(position, request->funding->funding_index,
                                 next_notional, &rebased);
    if (status != LXP_OK) return status;
    status = fill_interest_add(request->funding, position->side, notional);
    if (status != LXP_OK) return status;
    position->size = next_size;
    position->entry_notional = next_notional;
    position->funding_index_at_entry = rebased;
    return fill_party_note(request, false);
}

static lxp_result fill_reduce(const lx_perps_fill_request *request,
                              lx_perps_position *position)
{
    lxp_u128 removed;
    lxp_u128 remainder;
    lxp_u128 next_size;
    lxp_u128 next_notional;
    lxp_i128 rebased;
    lxp_result status;
    int comparison = lxp_u128_cmp(request->quantity, position->size);
    if (comparison < 0) {
        if (lxp_u128_mul_div_floor(position->entry_notional,
                                   request->quantity, position->size,
                                   &removed, &remainder) != LXP_OK ||
            lxp_u128_sub(position->size, request->quantity, &next_size) !=
                LXP_OK ||
            lxp_u128_sub(position->entry_notional, removed,
                         &next_notional) != LXP_OK ||
            lxp_u128_is_zero(next_notional))
            return LXP_FATAL_INVARIANT;
        status = fill_funding_rebase(position,
                                     request->funding->funding_index,
                                     next_notional, &rebased);
        if (status != LXP_OK) return status;
        if (!lxp_u128_is_zero(removed)) {
            status = fill_interest_remove(request->funding, position->side,
                                          removed);
            if (status != LXP_OK) return status;
        }
        position->size = next_size;
        position->entry_notional = next_notional;
        position->funding_index_at_entry = rebased;
        return fill_party_note(request, false);
    }
    status = fill_funding_settle(request, position);
    if (status != LXP_OK) return status;
    status = fill_interest_remove(request->funding, position->side,
                                  position->entry_notional);
    if (status != LXP_OK) return status;
    if (comparison == 0) {
        position->open = false;
        return fill_party_note(request, true);
    }
    if (lxp_u128_sub(request->quantity, position->size, &next_size) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    status = fill_open(request, position, next_size);
    if (status != LXP_OK) return status;
    return fill_party_note(request, false);
}

lxp_result lx_perps_position_apply_fill(const lx_perps_fill_request *request)
{
    lx_perps_position *position = NULL;
    lxp_result status;
    if (request == NULL || request->store == NULL ||
        request->store->count > LX_PERPS_POSITION_CAPACITY ||
        request->market == NULL || request->funding == NULL ||
        request->settlement == NULL || request->owner_main == NULL ||
        request->margin_account == NULL ||
        request->long_funding_account == NULL ||
        request->short_funding_account == NULL || request->asset == NULL ||
        request->owner_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->margin_account->kind != LX_ACCOUNT_AGENT_MARGIN ||
        (request->side != LX_PERPS_SIDE_BUY &&
         request->side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(request->price) ||
        lxp_u128_is_zero(request->quantity))
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_position_lookup(request->store,
                                      request->margin_account->id, &position);
    if (status == LXP_ERR_UNKNOWN_FIELD) {
        if (request->store->count == LX_PERPS_POSITION_CAPACITY)
            return LXP_ERR_ARENA_EXHAUSTED;
        status = fill_open(request,
                           &request->store->positions[request->store->count],
                           request->quantity);
        if (status != LXP_OK) return status;
        ++request->store->count;
        return fill_party_note(request, false);
    }
    if (status != LXP_OK) return status;
    if (memcmp(position->market_id, request->market->market_id, 32U) != 0 ||
        memcmp(position->owner_main_account_id, request->owner_main->id,
               32U) != 0 ||
        memcmp(position->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (!position->open) {
        status = fill_open(request, position, request->quantity);
        if (status != LXP_OK) return status;
        return fill_party_note(request, false);
    }
    return position->side == request->side ? fill_increase(request, position) :
                                             fill_reduce(request, position);
}

lxp_result lx_perps_fill_settle(lxp_module_ctx *ctx,
                                lx_perps_position_store *store,
                                const uint8_t market_id[32],
                                lx_perps_fill_settlement *settlement,
                                const lxp_transfer_asset_state *asset,
                                lxp_transfer_context context,
                                lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority authorities[LX_PERPS_FILL_SOURCES];
    size_t authority_count = 0U;
    size_t i;
    lxp_result status;
    if (ctx == NULL || store == NULL || market_id == NULL ||
        settlement == NULL || asset == NULL || receipt == NULL ||
        settlement->party_count > LX_PERPS_FILL_PARTIES ||
        settlement->funding_leg_count > LX_PERPS_FILL_PARTIES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(authorities, 0, sizeof(authorities));
    for (i = 0U; i < settlement->party_count; ++i) {
        const lx_perps_fill_party *party = &settlement->parties[i];
        lx_perps_position *position = NULL;
        lxp_transfer_leg *leg;
        if (!party->closed) continue;
        status = lx_perps_position_lookup(store, party->margin_account->id,
                                          &position);
        if (status != LXP_OK) return status;
        if (memcmp(position->market_id, market_id, 32U) != 0)
            return LXP_ERR_NON_CANONICAL;
        if (position->open ||
            lxp_u128_is_zero(party->margin_account->balance))
            continue;
        leg = &set.legs[set.leg_count++];
        leg->from = party->margin_account;
        leg->to = party->owner_main;
        (void)memcpy(leg->asset_id, asset->asset_id, 32U);
        leg->amount = party->margin_account->balance;
        leg->reason = LXP_REASON_MARGIN_RELEASE;
        leg->supply_mode = LXP_TRANSFER_CONSERVED;
        status = lx_perps_source_authority_add(
            authorities, LX_PERPS_FILL_SOURCES, &authority_count,
            party->margin_account->id, LXP_AUTH_PROTOCOL_MODULE, true);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < settlement->funding_leg_count; ++i) {
        const lxp_transfer_leg *funding = &settlement->funding_legs[i];
        bool pool = funding->from->kind == LX_ACCOUNT_SYSTEM_FUNDING_LONG ||
                    funding->from->kind == LX_ACCOUNT_SYSTEM_FUNDING_SHORT;
        set.legs[set.leg_count++] = *funding;
        status = lx_perps_source_authority_add(
            authorities, LX_PERPS_FILL_SOURCES, &authority_count,
            funding->from->id,
            pool ? LXP_AUTH_PROTOCOL_MODULE : LXP_AUTH_OWNER, pool);
        if (status != LXP_OK) return status;
    }
    if (set.leg_count != 0U) {
        set.context = context;
        set.context.assets = asset;
        set.context.asset_count = 1U;
        (void)memcpy(set.context.authorized_from, set.legs[0].from->id, 32U);
        set.context.source_authorities = authorities;
        set.context.source_authority_count = authority_count;
        status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < settlement->party_count; ++i) {
        const lx_perps_fill_party *party = &settlement->parties[i];
        lx_perps_position *position = NULL;
        status = lx_perps_position_lookup(store, party->margin_account->id,
                                          &position);
        if (status != LXP_OK) return status;
        if (position->open) {
            party->margin_account->has_open_reference = true;
        } else if (party->closed) {
            if (!lxp_u128_is_zero(party->margin_account->balance))
                return LXP_FATAL_INVARIANT;
            party->margin_account->has_open_reference = false;
        }
    }
    return LXP_OK;
}
