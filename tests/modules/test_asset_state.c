#include "layerx/lx_asset.h"

#include <string.h>

static void make_asset(lx_asset_record *record, uint8_t id, const char *symbol)
{
    (void)memset(record, 0, sizeof(*record));
    record->asset_id[0] = id;
    record->symbol_length = (uint8_t)strlen(symbol);
    (void)memcpy(record->symbol, symbol, record->symbol_length + 1U);
    record->decimals = 6U;
    record->custody_kind = LX_ASSET_CUSTODY_PAXEER;
    record->custody_reference[0] = id;
    record->custody_reference_length = 1U;
}

static int prepare(lx_asset_registry *assets, lx_account_registry *accounts,
                   bool reverse)
{
    lx_asset_record first;
    lx_asset_record second;
    lx_account *account;
    const char *names[3] = { "agent:did:key:a:main", "agent:did:key:b:main",
                             "agent:did:key:a:escrow:h1" };
    uint64_t balances[3] = { 10U, 20U, 7U };
    size_t order[3] = { 0U, 1U, 2U };
    size_t i;
    make_asset(&first, 1U, "ONE");
    make_asset(&second, 2U, "TWO");
    if (lx_asset_registry_init(assets, 0U) != LXP_OK ||
        lx_account_registry_init(accounts) != LXP_OK) return 1;
    if (reverse) {
        lx_asset_record swap = first;
        first = second;
        second = swap;
        order[0] = 2U; order[1] = 1U; order[2] = 0U;
    }
    if (lx_asset_register(assets, &first, 0U, (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_asset_register(assets, &second, 1U, (lxp_u128){ 0U, 0U }) != LXP_OK)
        return 1;
    for (i = 0U; i < 3U; ++i) {
        size_t index = order[i];
        uint8_t asset_id[32] = { index == 2U ? 2U : 1U };
        if (lx_asset_account_open(assets, accounts, asset_id,
                                  (const uint8_t *)names[index],
                                  strlen(names[index]), 4U + i,
                                  LX_ACCOUNT_OPEN_CREDIT, NULL, &account) != LXP_OK ||
            lxp_ledger_bootstrap_balance(account, asset_id,
                (lxp_u128){ 0U, balances[index] }, 0U) != LXP_OK) return 1;
    }
    return 0;
}

int main(void)
{
    lx_asset_registry assets_a;
    lx_asset_registry assets_b;
    lx_account_registry accounts_a;
    lx_account_registry accounts_b;
    uint8_t root_a[32];
    uint8_t root_b[32];
    uint8_t asset_one[32] = { 1U };
    lxp_u128 total;
    lxp_u128 balance;

    if (prepare(&assets_a, &accounts_a, false) != 0 ||
        prepare(&assets_b, &accounts_b, true) != 0 ||
        lx_asset_state_root(&assets_a, &accounts_a, root_a) != LXP_OK ||
        lx_asset_state_root(&assets_b, &accounts_b, root_b) != LXP_OK ||
        memcmp(root_a, root_b, 32U) != 0 ||
        lx_asset_total_units(&assets_a, &accounts_a, asset_one, &total) != LXP_OK ||
        total.hi != 0U || total.lo != 30U ||
        lx_asset_balance_get(&accounts_a, accounts_a.accounts[0].id,
                             accounts_a.accounts[0].asset_id, &balance) != LXP_OK ||
        balance.lo != 10U || balance.hi != 0U)
        return 1;
    accounts_a.count = LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
    if (lx_asset_balance_get(&accounts_a, accounts_a.accounts[0].id,
                             accounts_a.accounts[0].asset_id, &balance) !=
            LXP_ERR_NON_CANONICAL ||
        lx_asset_total_units(&assets_a, &accounts_a, asset_one, &total) !=
            LXP_ERR_NON_CANONICAL ||
        lx_asset_state_root(&assets_a, &accounts_a, root_a) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    accounts_a.count = 3U;
    assets_a.count = LX_ASSET_REGISTRY_CAPACITY + 1U;
    if (lx_asset_state_root(&assets_a, &accounts_a, root_a) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}
