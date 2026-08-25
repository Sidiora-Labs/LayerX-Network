#include "layerx/lx_asset.h"

#include <string.h>

static lxp_result move(lx_account *from, lx_account *to,
                       lxp_transfer_asset_state *asset, uint64_t amount,
                       bool system_capability)
{
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_result result;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = from; leg.to = to;
    (void)memcpy(leg.asset_id, asset->asset_id, 32U);
    leg.amount = (lxp_u128){ 0U, amount };
    (void)memset(&context, 0, sizeof(context));
    context.assets = asset; context.asset_count = 1U;
    context.protocol_system_capability = system_capability;
    context.actor_sequence = from->next_sequence;
    context.sequence_account = from;
    (void)memcpy(context.authorized_from, from->id, 32U);
    return lxp_apply_transfer(&leg, &context, &result);
}

static int seal(lx_asset_registry *assets, lx_account_registry *accounts,
                lx_asset_custody_attestation *attestation)
{
    lxp_u128 total;
    lx_asset_reserve_report_record report;
    uint8_t encoded[274];
    size_t length;
    return lx_asset_total_units(assets, accounts, attestation->asset_id, &total) !=
               LXP_OK ||
           lx_asset_reserve_reconcile(accounts, attestation, &report) != LXP_OK ||
           lx_asset_reserve_report_encode(&report, encoded, sizeof(encoded),
                                          &length) != LXP_OK || length != 274U ||
           lx_asset_supply_check(assets, accounts, attestation, 1U) != LXP_OK;
}

int main(void)
{
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *reserve;
    lx_account *agent;
    lx_account *other;
    lx_account *withdrawals;
    lxp_transfer_asset_state asset_state;
    lx_asset_custody_attestation attestation;
    lx_asset_reserve_report_record report;
    const char *names[4] = { "system:paxeer-reserve", "agent:did:key:a:main",
                             "agent:did:key:b:main",
                             "system:paxeer-withdrawals" };
    lx_account **opened[4] = { &reserve, &agent, &other, &withdrawals };
    lx_account_open_authority authority[4] = {
        LX_ACCOUNT_OPEN_GENESIS, LX_ACCOUNT_OPEN_CREDIT,
        LX_ACCOUNT_OPEN_CREDIT, LX_ACCOUNT_OPEN_GENESIS
    };
    size_t i;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U; (void)memcpy(asset.symbol, "A", 2U);
    asset.symbol_length = 1U; asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U; asset.custody_reference_length = 1U;
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U, (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK) return 1;
    for (i = 0U; i < 4U; ++i)
        if (lx_asset_account_open(&assets, &accounts, asset.asset_id,
                                  (const uint8_t *)names[i], strlen(names[i]),
                                  1U + i, authority[i], NULL, opened[i]) != LXP_OK)
            return 1;
    if (lxp_ledger_bootstrap_balance(reserve, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK) return 1;
    (void)memset(&attestation, 0, sizeof(attestation));
    (void)memcpy(attestation.asset_id, asset.asset_id, 32U);
    attestation.custody_amount = (lxp_u128){ 0U, 100U };
    attestation.checkpoint_id[0] = 2U;
    attestation.state_root[0] = 3U;
    attestation.finalized = true;
    if (seal(&assets, &accounts, &attestation) != 0 ||
        move(reserve, agent, &asset_state, 30U, true) != LXP_OK ||
        seal(&assets, &accounts, &attestation) != 0 ||
        move(agent, other, &asset_state, 10U, false) != LXP_OK ||
        seal(&assets, &accounts, &attestation) != 0 ||
        move(agent, withdrawals, &asset_state, 20U, false) != LXP_OK ||
        seal(&assets, &accounts, &attestation) != 0 ||
        move(withdrawals, reserve, &asset_state, 20U, true) != LXP_OK)
        return 1;
    attestation.settled_out = (lxp_u128){ 0U, 20U };
    if (seal(&assets, &accounts, &attestation) != 0 ||
        lx_asset_reserve_report(&accounts, &attestation, &report) != LXP_OK ||
        report.agent_main.lo != 10U || report.withdrawals.lo != 0U ||
        report.reserve.lo != 90U || report.raw_total.lo != 100U ||
        report.effective_total.lo != 80U || report.expected_backing.lo != 80U)
        return 1;
    if (lxp_ledger_bootstrap_balance(other, asset.asset_id,
                                     (lxp_u128){ 0U, 11U },
                                     other->next_sequence) != LXP_OK ||
        lx_asset_supply_check(&assets, &accounts, &attestation, 1U) !=
            LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    accounts.count = LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
    if (lx_asset_reserve_report(&accounts, &attestation, &report) !=
        LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    accounts.count = 4U;
    assets.count = LX_ASSET_REGISTRY_CAPACITY + 1U;
    if (lx_asset_supply_check(&assets, &accounts, &attestation, 1U) !=
        LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    assets.count = 1U;
    if (lx_asset_supply_check(&assets, &accounts, &attestation,
                              LX_ASSET_REGISTRY_CAPACITY + 1U) !=
        LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    return 0;
}
