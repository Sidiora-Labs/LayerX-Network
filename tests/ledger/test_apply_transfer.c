#include "layerx/lxp_module.h"
#include "layerx/lxp_transfer.h"

#include <string.h>

static int unchanged(const lx_account *from, const lx_account *to,
                     uint64_t from_value, uint64_t to_value)
{
    return from->balance.hi == 0U && from->balance.lo == from_value &&
           to->balance.hi == 0U && to->balance.lo == to_value;
}

int main(void)
{
    lx_account_registry registry;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:alice:main";
    const char *to_name = "agent:did:key:bob:main";
    uint8_t from_id[32];
    uint8_t to_id[32];
    uint8_t asset_id[32] = { 1U };
    uint8_t other_asset[32] = { 2U };
    lxp_transfer_asset_state assets[1];
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_result result;
    lxp_u128 maximum = { UINT64_MAX, UINT64_MAX };

    if (lx_account_registry_init(&registry) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  from_id) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  to_id) != LXP_OK ||
        lx_account_open(&registry, (const uint8_t *)from_name, strlen(from_name),
                        from_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) != LXP_OK ||
        lx_account_open(&registry, (const uint8_t *)to_name, strlen(to_name),
                        to_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 100U },
                                     4U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 5U }, 0U) !=
            LXP_OK) return 1;
    (void)memset(&assets, 0, sizeof(assets));
    (void)memcpy(assets[0].asset_id, asset_id, 32U);
    assets[0].registered = true;
    (void)memset(&context, 0, sizeof(context));
    context.assets = assets;
    context.asset_count = 1U;
    (void)memcpy(context.authorized_from, from_id, 32U);
    context.actor_sequence = 4U;
    context.batch_timestamp = 10U;
    context.expires_at = 10U;
    leg.from = from;
    leg.to = to;
    (void)memcpy(leg.asset_id, asset_id, 32U);

    leg.amount = (lxp_u128){ 0U, 0U };
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ZERO_AMOUNT ||
        !unchanged(from, to, 100U, 5U)) return 1;
    (void)memcpy(leg.asset_id, other_asset, 32U);
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ZERO_AMOUNT)
        return 1;
    leg.amount = (lxp_u128){ 0U, 1U };
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ASSET_MISMATCH ||
        !unchanged(from, to, 100U, 5U)) return 1;
    (void)memcpy(leg.asset_id, asset_id, 32U);
    assets[0].registered = false;
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ASSET_MISMATCH)
        return 1;
    assets[0].registered = true;
    assets[0].paused = true;
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ASSET_PAUSED)
        return 1;
    assets[0].paused = false;
    from->frozen = true;
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_ACCOUNT_FROZEN)
        return 1;
    from->frozen = false;
    leg.amount = (lxp_u128){ 0U, 101U };
    if (lxp_apply_transfer(&leg, &context, &result) !=
            LXP_ERR_INSUFFICIENT_BALANCE || !unchanged(from, to, 100U, 5U))
        return 1;
    if (lxp_ledger_bootstrap_balance(to, asset_id, maximum, 0U) != LXP_OK)
        return 1;
    leg.amount = (lxp_u128){ 0U, 1U };
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_ERR_OVERFLOW ||
        from->balance.lo != 100U || to->balance.hi != UINT64_MAX ||
        to->balance.lo != UINT64_MAX) return 1;
    if (lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U }, 0U) !=
        LXP_OK) return 1;
    context.has_client_balance = true;
    if (lxp_apply_transfer(&leg, &context, &result) !=
        LXP_ERR_CLIENT_SUPPLIED_BALANCE) return 1;
    context.has_client_balance = false;
    leg.amount = (lxp_u128){ 0U, 100U };
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_OK ||
        !unchanged(from, to, 0U, 100U) || result.from_balance_before.lo != 100U ||
        result.from_balance_after.lo != 0U || result.to_balance_before.lo != 0U ||
        result.to_balance_after.lo != 100U || from->next_sequence != 5U)
        return 1;
    leg.reason = LXP_REASON_PAYMENT;
    leg.supply_mode = LXP_TRANSFER_CONSERVED;
    if (lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 100U },
                                     UINT64_MAX) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 40U }, 0U) !=
            LXP_OK) return 1;
    context.actor_sequence = UINT64_MAX;
    leg.amount = (lxp_u128){ 0U, 10U };
    if (lxp_apply_transfer(&leg, &context, &result) !=
            LXP_ERR_SEQUENCE_EXHAUSTED || !unchanged(from, to, 100U, 40U) ||
        from->next_sequence != UINT64_MAX || to->next_sequence != 0U) return 1;
    leg.from = to;
    leg.to = from;
    (void)memcpy(context.authorized_from, to_id, 32U);
    context.actor_sequence = 0U;
    if (lxp_apply_transfer(&leg, &context, &result) != LXP_OK ||
        !unchanged(to, from, 30U, 110U) || to->next_sequence != 1U ||
        from->next_sequence != UINT64_MAX) return 1;
    leg.from = from;
    leg.to = to;
    (void)memcpy(context.authorized_from, from_id, 32U);
    if (lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 110U },
                                     3U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 30U },
                                     UINT64_MAX) != LXP_OK) return 1;
    context.sequence_account = to;
    context.actor_sequence = 3U;
    if (lxp_apply_transfer(&leg, &context, &result) !=
            LXP_ERR_SEQUENCE_EXHAUSTED || !unchanged(from, to, 110U, 30U) ||
        from->next_sequence != 3U || to->next_sequence != UINT64_MAX) return 1;
    context.sequence_account = NULL;
    context.protocol_system_capability = true;
    context.origin_module_id = LXP_MODULE_PROGRAMS;
    context.debit_authority_kind = LXP_AUTH_OCCUPANCY_RESPONSIBILITY;
    leg.reason = LXP_REASON_STORAGE_OCCUPANCY;
    if (from->kind != LX_ACCOUNT_AGENT_MAIN ||
        lxp_apply_transfer(&leg, &context, &result) != LXP_OK ||
        !unchanged(from, to, 100U, 40U) || from->next_sequence != 3U ||
        to->next_sequence != UINT64_MAX) return 1;
    if (lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 100U },
                                     UINT64_MAX) != LXP_OK) return 1;
    if (lxp_apply_transfer(&leg, &context, &result) !=
            LXP_ERR_SEQUENCE_EXHAUSTED || !unchanged(from, to, 100U, 40U) ||
        from->next_sequence != UINT64_MAX) return 1;
    return 0;
}
