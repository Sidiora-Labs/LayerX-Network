#include "layerx/lxp_transfer.h"
#include "layerx/lxp_module.h"
#include "layerx/lx_escrow.h"
#include "layerx/lx_budget.h"
#include "layerx/lx_stream.h"
#include "layerx/lx_perps.h"

#include <string.h>

static const lxp_transfer_asset_state *asset_find(
    const lxp_transfer_context *context, const uint8_t asset_id[32])
{
    size_t i;
    for (i = 0U; i < context->asset_count; ++i)
        if (memcmp(context->assets[i].asset_id, asset_id, 32U) == 0)
            return &context->assets[i];
    return NULL;
}

static bool privileged_system(lx_account_kind kind)
{
    switch (kind) {
    case LX_ACCOUNT_SYSTEM_LIQUIDITY:
    case LX_ACCOUNT_SYSTEM_FUNDING_LONG:
    case LX_ACCOUNT_SYSTEM_FUNDING_SHORT:
    case LX_ACCOUNT_SYSTEM_INSURANCE:
    case LX_ACCOUNT_SYSTEM_FEES:
    case LX_ACCOUNT_SYSTEM_PAXEER_RESERVE:
    case LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS:
        return true;
    default:
        return false;
    }
}

static lxp_result custody_spend_check(const lxp_transfer_leg *leg,
                                      const lxp_transfer_context *context,
                                      lxp_authorization_kind authority_kind)
{
    lxp_result status = lx_escrow_authority_check(
        leg->from, authority_kind,
        context->origin_module_id, leg->reason);
    if (status != LXP_OK) return status;
    status = lx_budget_authority_check(leg->from,
                                       authority_kind,
                                       context->origin_module_id,
                                       leg->reason);
    if (status != LXP_OK) return status;
    status = lx_stream_authority_check(leg->from,
                                       authority_kind,
                                       context->origin_module_id,
                                       leg->reason);
    if (status != LXP_OK) return status;
    return lx_perps_authority_check(leg->from,
                                    authority_kind,
                                    context->origin_module_id,
                                    leg->reason);
}

static lxp_result source_authority(
    const lxp_transfer_leg *leg, const lxp_transfer_context *context,
    const uint8_t **authorized_from, lxp_authorization_kind *authority_kind,
    bool *protocol_system_capability)
{
    size_t i;
    size_t matches = 0U;
    if (leg == NULL || leg->from == NULL || context == NULL ||
        authorized_from == NULL || authority_kind == NULL ||
        protocol_system_capability == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (context->source_authority_count == 0U) {
        if (context->source_authorities != NULL) return LXP_ERR_NON_CANONICAL;
        *authorized_from = context->authorized_from;
        *authority_kind = context->debit_authority_kind;
        *protocol_system_capability = context->protocol_system_capability;
        return LXP_OK;
    }
    if (context->source_authorities == NULL ||
        context->source_authority_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < context->source_authority_count; ++i) {
        const lxp_transfer_source_authority *candidate =
            &context->source_authorities[i];
        if (memcmp(candidate->authorized_from, leg->from->id, 32U) != 0)
            continue;
        *authorized_from = candidate->authorized_from;
        *authority_kind = candidate->debit_authority_kind;
        *protocol_system_capability = candidate->protocol_system_capability;
        ++matches;
    }
    return matches == 1U ? LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
}

lxp_result lxp_ledger_bootstrap_balance(lx_account *account,
                                        const uint8_t asset_id[32],
                                        lxp_u128 balance,
                                        uint64_t next_sequence)
{
    if (account == NULL || asset_id == NULL) return LXP_ERR_NON_CANONICAL;
    account->balance = balance;
    (void)memcpy(account->asset_id, asset_id, 32U);
    account->has_asset = true;
    account->next_sequence = next_sequence;
    return LXP_OK;
}

lxp_result lxp_ledger_restore_account_snapshot(lx_account *account,
                                               lxp_u128 balance,
                                               const uint8_t asset_id[32],
                                               bool has_asset,
                                               uint64_t next_sequence)
{
    if (account == NULL || asset_id == NULL) return LXP_ERR_NON_CANONICAL;
    account->balance = balance;
    (void)memcpy(account->asset_id, asset_id, 32U);
    account->has_asset = has_asset;
    account->next_sequence = next_sequence;
    return LXP_OK;
}

lxp_result lxp_state_balance_get(const lx_account *account,
                                 const uint8_t asset_id[32],
                                 lxp_u128 *balance)
{
    if (account == NULL || asset_id == NULL || balance == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!account->has_asset || memcmp(account->asset_id, asset_id, 32U) != 0)
        return LXP_ERR_ASSET_MISMATCH;
    *balance = account->balance;
    return LXP_OK;
}

lxp_result lxp_precondition_check(const lxp_transfer_leg *legs,
                                  size_t leg_count,
                                  const lxp_transfer_context *context)
{
    const lxp_transfer_leg *leg;
    const lxp_transfer_asset_state *asset;
    const uint8_t *authorized_from;
    lxp_authorization_kind authority_kind;
    bool protocol_system_capability;
    lxp_u128 computed;
    bool occupancy_mandate;
    if (legs == NULL || context == NULL) return LXP_ERR_NON_CANONICAL;
    if (context->has_client_balance) return LXP_ERR_CLIENT_SUPPLIED_BALANCE;
    if (leg_count == 0U || leg_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_TOO_MANY_LEGS;
    leg = &legs[0];
    if (leg->from == NULL || leg->to == NULL) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(leg->amount)) return LXP_ERR_ZERO_AMOUNT;
    asset = asset_find(context, leg->asset_id);
    if (asset == NULL || !asset->registered) return LXP_ERR_ASSET_MISMATCH;
    if (asset->paused) return LXP_ERR_ASSET_PAUSED;
    if (!leg->from->has_asset ||
        memcmp(leg->from->asset_id, leg->asset_id, 32U) != 0 ||
        (leg->to->has_asset &&
         memcmp(leg->to->asset_id, leg->asset_id, 32U) != 0))
        return LXP_ERR_ASSET_MISMATCH;
    if (leg->from->frozen || leg->to->frozen) return LXP_ERR_ACCOUNT_FROZEN;
    {
        lxp_result custody_status = source_authority(
            leg, context, &authorized_from, &authority_kind,
            &protocol_system_capability);
        if (custody_status != LXP_OK) return custody_status;
        custody_status = custody_spend_check(leg, context, authority_kind);
        if (custody_status != LXP_OK) return custody_status;
    }
    occupancy_mandate = authority_kind ==
                            LXP_AUTH_OCCUPANCY_RESPONSIBILITY &&
                        protocol_system_capability &&
                        context->origin_module_id == LXP_MODULE_PROGRAMS &&
                        leg->reason == LXP_REASON_STORAGE_OCCUPANCY &&
                        leg->from->kind == LX_ACCOUNT_AGENT_MAIN &&
                        memcmp(authorized_from, leg->from->id, 32U) == 0;
    if (authority_kind ==
            LXP_AUTH_OCCUPANCY_RESPONSIBILITY && !occupancy_mandate)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if ((privileged_system(leg->from->kind) &&
         !protocol_system_capability) ||
        (!privileged_system(leg->from->kind) &&
         !(protocol_system_capability &&
           (leg->from->kind == LX_ACCOUNT_AGENT_MARGIN ||
            occupancy_mandate)) &&
         memcmp(authorized_from, leg->from->id, 32U) != 0))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (!protocol_system_capability && context->actor_sequence <
        (context->sequence_account != NULL ? context->sequence_account->next_sequence :
                                             leg->from->next_sequence))
        return LXP_ERR_SEQUENCE_REUSED;
    if (!protocol_system_capability && context->actor_sequence >
        (context->sequence_account != NULL ? context->sequence_account->next_sequence :
                                             leg->from->next_sequence))
        return LXP_ERR_SEQUENCE_GAP;
    if (context->idempotency_seen) return LXP_ERR_IDEMPOTENT_REPLAY;
    if (context->expires_at != 0U &&
        context->batch_timestamp > context->expires_at) return LXP_ERR_EXPIRED;
    if (lxp_u128_cmp(leg->from->balance, leg->amount) < 0)
        return LXP_ERR_INSUFFICIENT_BALANCE;
    if (lxp_u128_sub(leg->from->balance, leg->amount, &computed) != LXP_OK)
        return LXP_ERR_UNDERFLOW;
    if (leg->from != leg->to &&
        lxp_u128_add(leg->to->balance, leg->amount, &computed) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    return LXP_OK;
}

lxp_result lxp_balance_apply_leg(lxp_transfer_leg *leg,
                                 lxp_transfer_result *result)
{
    lxp_u128 from_after;
    lxp_u128 to_after;
    lxp_result status;
    if (leg == NULL || result == NULL || leg->from == NULL || leg->to == NULL)
        return LXP_ERR_NON_CANONICAL;
    result->from_balance_before = leg->from->balance;
    result->to_balance_before = leg->to->balance;
    if (leg->from == leg->to) {
        result->from_balance_after = leg->from->balance;
        result->to_balance_after = leg->to->balance;
        return LXP_OK;
    }
    status = lxp_u128_sub(leg->from->balance, leg->amount, &from_after);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(leg->to->balance, leg->amount, &to_after);
    if (status != LXP_OK) return status;
    leg->from->balance = from_after;
    leg->to->balance = to_after;
    if (!leg->to->has_asset) {
        (void)memcpy(leg->to->asset_id, leg->asset_id, 32U);
        leg->to->has_asset = true;
    }
    result->from_balance_after = from_after;
    result->to_balance_after = to_after;
    return LXP_OK;
}

lxp_result lxp_balance_restore_snapshot(lxp_ledger_journal *journal)
{
    size_t i;
    if (journal == NULL || !journal->open) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < journal->count; ++i) {
        lxp_result status = lxp_ledger_restore_account_snapshot(
            journal->entries[i].account, journal->entries[i].balance_before,
            journal->entries[i].asset_id, journal->entries[i].has_asset,
            journal->entries[i].next_sequence);
        if (status != LXP_OK) return status;
    }
    journal->open = false;
    return LXP_OK;
}

lxp_result lxp_apply_transfer(lxp_transfer_leg *leg,
                              lxp_transfer_context *context,
                              lxp_transfer_result *result)
{
    size_t index;
    lxp_result status;
    if (context == NULL) return LXP_ERR_NON_CANONICAL;
    if (context->source_authority_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    if (context->debit_authority_kind == LXP_AUTH_PROGRAM_SPEND)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    for (index = 0U; index < context->source_authority_count; ++index)
        if (context->source_authorities == NULL ||
            context->source_authorities[index].debit_authority_kind ==
                LXP_AUTH_PROGRAM_SPEND)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (context->program_spend_token != 0U)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (leg != NULL && leg->supply_mode != LXP_TRANSFER_CONSERVED)
        return LXP_ERR_CONSERVATION;
    status = lxp_precondition_check(leg, 1U, context);
    if (status != LXP_OK) return status;
    status = lxp_balance_apply_leg(leg, result);
    if (status == LXP_OK && context->debit_authority_kind !=
                            LXP_AUTH_OCCUPANCY_RESPONSIBILITY)
        ++(context->sequence_account != NULL ? context->sequence_account :
                                              leg->from)->next_sequence;
    return status;
}
