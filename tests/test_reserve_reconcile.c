#include "layerx/lxp_reserve.h"

#include <string.h>

int main(void)
{
    static const char *names[] = {
        "system:paxeer-reserve",
        "agent:did:key:a:main",
        "agent:did:key:a:escrow:order-1",
        "agent:did:key:a:budget:daily",
        "agent:did:key:a:stream:meter-1",
        "agent:did:key:a:margin:btc-usd",
        "system:liquidity:btc-usd",
        "system:insurance",
        "system:fees",
        "system:paxeer-withdrawals"
    };
    static const uint64_t balances[] = {70U, 10U, 3U, 2U, 1U,
                                        2U, 3U, 2U, 2U, 5U};
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *opened[sizeof(names) / sizeof(names[0])];
    lx_asset_custody_attestation attestation;
    lxp_outstanding_claims claims;
    lxp_reserve_report_view report;
    lxp_u128 total;
    uint8_t encoded[322];
    size_t encoded_length = 0U;
    size_t i;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    (void)memcpy(asset.symbol, "A", 2U);
    asset.symbol_length = 1U;
    asset.name[0] = (uint8_t)'A';
    asset.name_length = 1U;
    asset.issuer_kind = 2U;
    asset.issuer_did32[0] = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U, (lxp_u128){0U, 0U}) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK) return 1;
    for (i = 0U; i < sizeof(names) / sizeof(names[0]); ++i) {
        if (lx_asset_account_open(
                &assets, &accounts, asset.asset_id,
                (const uint8_t *)names[i], strlen(names[i]), i + 1U,
                LX_ACCOUNT_OPEN_GENESIS, NULL, &opened[i]) != LXP_OK ||
            lxp_ledger_bootstrap_balance(
                opened[i], asset.asset_id,
                (lxp_u128){0U, balances[i]}, 0U) != LXP_OK)
            return 1;
    }
    (void)memset(&attestation, 0, sizeof(attestation));
    (void)memcpy(attestation.asset_id, asset.asset_id, 32U);
    attestation.custody_amount = (lxp_u128){0U, 100U};
    attestation.checkpoint_id[0] = 2U;
    attestation.state_root[0] = 3U;
    attestation.finalized = true;
    (void)memset(&claims, 0, sizeof(claims));
    (void)memcpy(claims.asset_id, asset.asset_id, 32U);
    claims.amount = (lxp_u128){0U, 5U};
    if (lx_asset_total_units(
            &assets, &accounts, asset.asset_id, &total) != LXP_OK ||
        total.lo != 100U ||
        lxp_reserve_reconcile(
            &accounts, &attestation, &claims, &report) != LXP_OK ||
        !report.zero_tolerance_match ||
        report.contributing_class_mask != LXP_RESERVE_ALL_CLASSES ||
        report.accounts.agent_main.lo != 10U ||
        report.accounts.escrow.lo != 3U ||
        report.accounts.budget.lo != 2U ||
        report.accounts.stream.lo != 1U ||
        report.accounts.margin.lo != 2U ||
        report.accounts.liquidity.lo != 3U ||
        report.accounts.insurance.lo != 2U ||
        report.accounts.fees.lo != 2U ||
        report.accounts.withdrawals.lo != 5U ||
        report.accounts.reserve.lo != 70U ||
        report.required_backing.lo != 30U ||
        report.available_custody.lo != 100U ||
        report.excess_backing.lo != 70U ||
        lx_asset_reserve_report_encode(
            &report.accounts, encoded, sizeof(encoded),
            &encoded_length) != LXP_OK || encoded_length != 322U ||
        lxp_supply_invariant_check(
            &assets, &accounts, &attestation, 1U, &claims, 1U) != LXP_OK)
        return 1;

    claims.amount.lo = 4U;
    if (lxp_reserve_reconcile(
            &accounts, &attestation, &claims, &report) !=
            LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    claims.amount.lo = 5U;
    if (lxp_ledger_bootstrap_balance(
            opened[1], asset.asset_id, (lxp_u128){0U, 11U},
            opened[1]->next_sequence) != LXP_OK ||
        lxp_supply_invariant_check(
            &assets, &accounts, &attestation, 1U, &claims, 1U) !=
            LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    return 0;
}
