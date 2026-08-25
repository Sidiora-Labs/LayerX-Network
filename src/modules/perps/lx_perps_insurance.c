#include "layerx/lx_perps.h"

#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result emit_cover(lxp_module_ctx *ctx, lx_account *from,
                             lx_account *to,
                             const lxp_transfer_asset_state *asset,
                             lxp_u128 amount, uint16_t reason,
                             lxp_transfer_context context,
                             lxp_receipt *receipt)
{
    lxp_transfer_set set;
    if (ctx == NULL || from == NULL || to == NULL || asset == NULL ||
        receipt == NULL || lxp_u128_is_zero(amount))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
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
    set.context.protocol_system_capability = true;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(set.context.authorized_from, from->id, 32U);
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_perps_insurance_cover(
    lxp_module_ctx *ctx, lx_account *insurance_account,
    lx_account *liquidity_account, const lxp_transfer_asset_state *asset,
    lxp_u128 deficit, lxp_transfer_context context, lxp_receipt *receipt)
{
    if (insurance_account == NULL || liquidity_account == NULL ||
        insurance_account->kind != LX_ACCOUNT_SYSTEM_INSURANCE ||
        liquidity_account->kind != LX_ACCOUNT_SYSTEM_LIQUIDITY ||
        lxp_u128_cmp(insurance_account->balance, deficit) < 0)
        return insurance_account != NULL &&
               lxp_u128_cmp(insurance_account->balance, deficit) < 0 ?
            LXP_ERR_INSUFFICIENT_BALANCE : LXP_ERR_NON_CANONICAL;
    return emit_cover(ctx, insurance_account, liquidity_account, asset,
                      deficit, LXP_REASON_INSURANCE, context, receipt);
}

lxp_result lx_perps_deficit_record(
    lx_perps_deficit_store *store, const uint8_t market_id[32],
    const uint8_t insurance_account_id[32], lxp_u128 amount,
    uint64_t global_sequence)
{
    size_t i;
    if (store == NULL || market_id == NULL || insurance_account_id == NULL ||
        store->count > LX_PERPS_DEFICIT_CAPACITY ||
        lxp_u128_is_zero(amount) || global_sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i) {
        lx_perps_deficit *record = &store->deficits[i];
        lxp_u128 total;
        lxp_result status;
        if (memcmp(record->market_id, market_id, 32U) != 0 ||
            memcmp(record->insurance_account_id,
                   insurance_account_id, 32U) != 0)
            continue;
        status = lxp_u128_add(record->amount, amount, &total);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
        record->amount = total;
        record->recorded_at_sequence = global_sequence;
        return LXP_OK;
    }
    if (store->count == LX_PERPS_DEFICIT_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    (void)memset(&store->deficits[store->count], 0,
                 sizeof(store->deficits[store->count]));
    (void)memcpy(store->deficits[store->count].market_id, market_id, 32U);
    (void)memcpy(store->deficits[store->count].insurance_account_id,
                 insurance_account_id, 32U);
    store->deficits[store->count].amount = amount;
    store->deficits[store->count].recorded_at_sequence = global_sequence;
    ++store->count;
    return LXP_OK;
}

static int candidate_compare(const lx_perps_adl_candidate *left,
                             const lx_perps_adl_candidate *right)
{
    return memcmp(left->position->position_id,
                  right->position->position_id, 32U);
}

static void candidate_sort(lx_perps_adl_candidate *candidates, size_t count)
{
    size_t i;
    for (i = 1U; i < count; ++i) {
        lx_perps_adl_candidate value = candidates[i];
        size_t position = i;
        while (position != 0U &&
               candidate_compare(&value, &candidates[position - 1U]) < 0) {
            candidates[position] = candidates[position - 1U];
            --position;
        }
        candidates[position] = value;
    }
}

lxp_result lx_perps_adl_execute(
    lxp_module_ctx *ctx, lx_perps_adl_candidate *candidates,
    size_t candidate_count, lx_account *liquidity_account,
    const lxp_transfer_asset_state *asset, lxp_u128 deficit,
    lxp_transfer_context context, lxp_receipt *receipt,
    lxp_u128 *remaining_deficit)
{
    lx_perps_adl_candidate ordered[LX_PERPS_ADL_CAPACITY];
    lxp_transfer_set set;
    lxp_u128 remaining = deficit;
    size_t i;
    lxp_result status;
    if (ctx == NULL || candidates == NULL || candidate_count == 0U ||
        candidate_count > LX_PERPS_ADL_CAPACITY || liquidity_account == NULL ||
        asset == NULL || receipt == NULL || remaining_deficit == NULL ||
        lxp_u128_is_zero(deficit) ||
        liquidity_account->kind != LX_ACCOUNT_SYSTEM_LIQUIDITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(ordered, candidates,
                 candidate_count * sizeof(ordered[0]));
    for (i = 0U; i < candidate_count; ++i)
        if (ordered[i].position == NULL ||
            ordered[i].margin_account == NULL ||
            ordered[i].margin_account->kind != LX_ACCOUNT_AGENT_MARGIN ||
            !ordered[i].position->open ||
            memcmp(ordered[i].position->margin_account_id,
                   ordered[i].margin_account->id, 32U) != 0)
            return LXP_ERR_NON_CANONICAL;
    candidate_sort(ordered, candidate_count);
    (void)memset(&set, 0, sizeof(set));
    for (i = 0U; i < candidate_count && !lxp_u128_is_zero(remaining); ++i) {
        lxp_u128 available = ordered[i].margin_account->balance;
        lxp_u128 contribution;
        lxp_transfer_leg *leg;
        if (lxp_u128_cmp(available, ordered[i].maximum_contribution) > 0)
            available = ordered[i].maximum_contribution;
        contribution = lxp_u128_cmp(available, remaining) < 0 ? available :
                                                                   remaining;
        if (lxp_u128_is_zero(contribution)) continue;
        if (set.leg_count == LXP_MAX_TRANSFER_SET_LEGS)
            return LXP_ERR_TOO_MANY_LEGS;
        leg = &set.legs[set.leg_count++];
        leg->from = ordered[i].margin_account;
        leg->to = liquidity_account;
        (void)memcpy(leg->asset_id, asset->asset_id, 32U);
        leg->amount = contribution;
        leg->reason = LXP_REASON_ADL;
        leg->supply_mode = LXP_TRANSFER_CONSERVED;
        status = lxp_u128_sub(remaining, contribution, &remaining);
        if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    }
    if (!lxp_u128_is_zero(remaining)) {
        *remaining_deficit = remaining;
        return LXP_ERR_INSUFFICIENT_BALANCE;
    }
    set.context = context;
    set.context.assets = asset;
    set.context.asset_count = 1U;
    set.context.protocol_system_capability = true;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(set.context.authorized_from, set.legs[0].from->id, 32U);
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) {
        *remaining_deficit = deficit;
        return status;
    }
    for (i = 0U; i < candidate_count; ++i)
        if (lxp_u128_is_zero(ordered[i].margin_account->balance)) {
            ordered[i].position->open = false;
            ordered[i].margin_account->has_open_reference = false;
        }
    *remaining_deficit = (lxp_u128){ 0U, 0U };
    return LXP_OK;
}
