#include "layerx/lxp_transfer.h"

#include <string.h>

static lxp_result open_system(lx_account_registry *registry, const char *name,
                              uint64_t balance, const uint8_t asset_id[32],
                              lx_account **account)
{
    uint8_t id[32];
    lxp_result status = lx_account_id_from_string((const uint8_t *)name,
                                                  strlen(name), id);
    if (status == LXP_OK)
        status = lx_account_open(registry, (const uint8_t *)name, strlen(name), id,
                                 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, account);
    if (status == LXP_OK)
        status = lxp_ledger_bootstrap_balance(*account, asset_id,
                                              (lxp_u128){ 0U, balance }, 0U);
    return status;
}

static int balances(lx_account *const accounts[4], uint64_t a, uint64_t b,
                    uint64_t c, uint64_t d)
{
    return accounts[0]->balance.lo == a && accounts[1]->balance.lo == b &&
           accounts[2]->balance.lo == c && accounts[3]->balance.lo == d;
}

int main(void)
{
    lx_account_registry registry;
    lx_account *accounts[4];
    const char *names[4] = { "system:liquidity:btc-usd", "system:insurance",
                             "system:fees", "system:paxeer-reserve" };
    uint8_t asset_id[32] = { 7U };
    lxp_transfer_asset_state asset;
    lxp_transfer_leg legs[4];
    lxp_transfer_context context;
    lxp_transfer_set_result result;
    uint8_t first_root[32];
    size_t i;

    if (lx_account_registry_init(&registry) != LXP_OK) return 1;
    for (i = 0U; i < 4U; ++i)
        if (open_system(&registry, names[i], 100U, asset_id, &accounts[i]) !=
            LXP_OK) return 1;
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, asset_id, 32U);
    asset.registered = true;
    (void)memset(&context, 0, sizeof(context));
    context.assets = &asset;
    context.asset_count = 1U;
    context.protocol_system_capability = true;
    (void)memset(legs, 0, sizeof(legs));
    for (i = 0U; i < 4U; ++i) {
        legs[i].from = accounts[i];
        legs[i].to = accounts[(i + 1U) % 4U];
        (void)memcpy(legs[i].asset_id, asset_id, 32U);
        legs[i].amount = (lxp_u128){ 0U, (i + 1U) * 10U };
        legs[i].reason = (uint16_t)(20U + i);
    }
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) != LXP_OK ||
        !result.receipt_emitted || result.leg_count != 4U ||
        !balances(accounts, 130U, 90U, 90U, 90U)) return 1;
    (void)memcpy(first_root, result.transfer_set_root, sizeof(first_root));

    if (lxp_ledger_bootstrap_balance(accounts[0], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(accounts[1], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(accounts[2], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(accounts[3], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK)
        return 1;
    legs[3].amount = (lxp_u128){ 0U, 1000U };
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) !=
            LXP_ERR_INSUFFICIENT_BALANCE || result.failed_leg != 3U ||
        result.failure != LXP_ERR_INSUFFICIENT_BALANCE ||
        result.receipt_emitted || !balances(accounts, 100U, 100U, 100U, 100U))
        return 1;
    legs[3].amount = (lxp_u128){ 0U, 40U };
    legs[0].supply_mode = LXP_TRANSFER_CREDIT_ONLY;
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) !=
            LXP_ERR_CONSERVATION || !balances(accounts, 100U, 100U, 100U, 100U))
        return 1;
    legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    context.inject_failure = true;
    context.failure_after_leg = 1U;
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) != LXP_ERR_IO ||
        result.failed_leg != 1U || !balances(accounts, 100U, 100U, 100U, 100U))
        return 1;
    context.inject_failure = false;
    {
        lxp_transfer_leg swapped[4];
        uint8_t swapped_root[32];
        (void)memcpy(swapped, legs, sizeof(swapped));
        swapped[0] = legs[1];
        swapped[1] = legs[0];
        if (lxp_transfer_set_root(swapped, 4U, swapped_root) != LXP_OK ||
            memcmp(first_root, swapped_root, 32U) == 0) return 1;
    }
    if (lxp_ledger_bootstrap_balance(accounts[3], asset_id,
                                     (lxp_u128){ 0U, 100U },
                                     UINT64_MAX) != LXP_OK) return 1;
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) !=
            LXP_ERR_SEQUENCE_EXHAUSTED ||
        result.failure != LXP_ERR_SEQUENCE_EXHAUSTED ||
        result.failed_leg != 3U || result.leg_count != 3U ||
        result.receipt_emitted || !balances(accounts, 100U, 100U, 100U, 100U) ||
        accounts[0]->next_sequence != 0U ||
        accounts[3]->next_sequence != UINT64_MAX) return 1;
    if (lxp_ledger_bootstrap_balance(accounts[3], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(accounts[0], asset_id,
                                     (lxp_u128){ 0U, 100U },
                                     UINT64_MAX) != LXP_OK) return 1;
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) !=
            LXP_ERR_SEQUENCE_EXHAUSTED ||
        result.failure != LXP_ERR_SEQUENCE_EXHAUSTED ||
        result.failed_leg != 0U || result.leg_count != 0U ||
        result.receipt_emitted || !balances(accounts, 100U, 100U, 100U, 100U) ||
        accounts[0]->next_sequence != UINT64_MAX) return 1;
    if (lxp_ledger_bootstrap_balance(accounts[0], asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK)
        return 1;
    if (lxp_apply_transfer_set(legs, 4U, &context, &result) != LXP_OK ||
        !result.receipt_emitted || result.leg_count != 4U ||
        !balances(accounts, 130U, 90U, 90U, 90U) ||
        accounts[0]->next_sequence != 1U ||
        accounts[3]->next_sequence != 0U) return 1;
    return 0;
}
