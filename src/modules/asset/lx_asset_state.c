#include "layerx/lx_asset.h"

#include "layerx/lxp_hash.h"
#include "layerx/lxp_merkle.h"

#include <stdlib.h>
#include <string.h>

typedef struct state_entry {
    uint8_t key[64];
    uint8_t value[384];
    size_t value_length;
} state_entry;

enum { LX_ASSET_STATE_MAX_ENTRIES = LX_ASSET_REGISTRY_CAPACITY +
                                     LX_ACCOUNT_REGISTRY_CAPACITY };

static int entry_order(const void *left, const void *right)
{
    const state_entry *const *a = (const state_entry *const *)left;
    const state_entry *const *b = (const state_entry *const *)right;
    int order = memcmp((*a)->key, (*b)->key, 64U);
    if (order != 0) return order;
    if (*a < *b) return -1;
    return *a > *b ? 1 : 0;
}

lxp_result lx_asset_balance_get(const lx_account_registry *accounts,
                                const uint8_t account_id[32],
                                const uint8_t asset_id[32], lxp_u128 *balance)
{
    size_t i;
    if (accounts == NULL || account_id == NULL || asset_id == NULL ||
        balance == NULL || accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < accounts->count; ++i) {
        if (memcmp(accounts->accounts[i].id, account_id, 32U) == 0) {
            if (!accounts->accounts[i].has_asset ||
                memcmp(accounts->accounts[i].asset_id, asset_id, 32U) != 0)
                return LXP_ERR_ASSET_MISMATCH;
            *balance = accounts->accounts[i].balance;
            return LXP_OK;
        }
    }
    return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

lxp_result lx_asset_account_open(lx_asset_registry *assets,
                                 lx_account_registry *accounts,
                                 const uint8_t asset_id[32],
                                 const uint8_t *name, size_t name_length,
                                 uint64_t global_sequence,
                                 lx_account_open_authority authority,
                                 lxp_log *activity_log, lx_account **account)
{
    lx_asset_record *asset;
    uint8_t account_id[32];
    lxp_result status;
    if (assets == NULL || accounts == NULL || asset_id == NULL || account == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_asset_lookup(assets, asset_id, &asset);
    if (status != LXP_OK) return status;
    status = lx_account_id_from_string(name, name_length, account_id);
    if (status == LXP_OK)
        status = lx_account_open(accounts, name, name_length, account_id,
                                 global_sequence, authority, activity_log,
                                 account);
    if (status != LXP_OK) return status;
    if ((*account)->has_asset)
        return memcmp((*account)->asset_id, asset->asset_id, 32U) == 0 ? LXP_OK :
               LXP_ERR_ASSET_MISMATCH;
    return lxp_ledger_bootstrap_balance(*account, asset_id,
                                        (lxp_u128){ 0U, 0U }, 0U);
}

static lxp_result sum_units(const lx_account_registry *accounts,
                            const uint8_t asset_id[32], lxp_u128 *total)
{
    lxp_u128 sum = { 0U, 0U };
    size_t i;
    if (accounts == NULL || asset_id == NULL || total == NULL ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < accounts->count; ++i) {
        lxp_u128 next;
        if (!accounts->accounts[i].has_asset ||
            memcmp(accounts->accounts[i].asset_id, asset_id, 32U) != 0) continue;
        uint8_t issuance_name[LX_ASSET_ISSUANCE_NAME_BYTES];
        uint8_t issuance_id[32];
        if (lx_asset_issuance_name(asset_id, issuance_name, issuance_id) != LXP_OK)
            return LXP_FATAL_SUPPLY_MISMATCH;
        if (accounts->accounts[i].kind == LX_ACCOUNT_MODULE_VALUE &&
            memcmp(accounts->accounts[i].id, issuance_id, 32U) == 0) continue;
        if (lxp_u128_add(sum, accounts->accounts[i].balance, &next) != LXP_OK)
            return LXP_FATAL_SUPPLY_MISMATCH;
        sum = next;
    }
    *total = sum;
    return LXP_OK;
}

lxp_result lx_asset_total_units(lx_asset_registry *assets,
                                const lx_account_registry *accounts,
                                const uint8_t asset_id[32], lxp_u128 *total)
{
    lx_asset_record *asset;
    lxp_result status;
    if (accounts == NULL || total == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_asset_lookup(assets, asset_id, &asset);
    if (status == LXP_OK) status = sum_units(accounts, asset_id, total);
    if (status == LXP_OK) asset->total_units = *total;
    return status;
}

lxp_result lx_asset_state_root(const lx_asset_registry *assets,
                               const lx_account_registry *accounts,
                               uint8_t root[32])
{
    state_entry *entries = NULL;
    const state_entry **order = NULL;
    uint8_t (*hashes)[32] = NULL;
    size_t capacity;
    size_t count = 0U;
    size_t i;
    lxp_result status = LXP_OK;
    if (assets == NULL || accounts == NULL || root == NULL ||
        assets->count > LX_ASSET_REGISTRY_CAPACITY ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    capacity = assets->count + accounts->count;
    if (capacity > (size_t)LX_ASSET_STATE_MAX_ENTRIES)
        return LXP_ERR_LENGTH_LIMIT;
    if (capacity == 0U)
        return lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, NULL, 0U, root);
    entries = (state_entry *)calloc(capacity, sizeof(*entries));
    order = (const state_entry **)calloc(capacity, sizeof(*order));
    hashes = (uint8_t (*)[32])calloc(capacity, sizeof(*hashes));
    if (entries == NULL || order == NULL || hashes == NULL) {
        free(entries); free(order); free(hashes);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    for (i = 0U; status == LXP_OK && i < assets->count; ++i) {
        lxp_u128 total;
        size_t encoded_length = 0U;
        uint8_t total_bytes[16];
        lx_asset_record record = assets->assets[i];
        (void)memcpy(entries[count].key, assets->assets[i].asset_id, 32U);
        status = sum_units(accounts, record.asset_id, &total);
        if (status == LXP_OK) {
            record.total_units = total;
            status = lx_asset_record_encode(&record, entries[count].value,
                                            sizeof(entries[count].value) - 16U,
                                            &encoded_length);
        }
        if (status == LXP_OK) status = lxp_u128_to_be(total, total_bytes);
        if (status != LXP_OK) break;
        (void)memcpy(entries[count].value + encoded_length, total_bytes, 16U);
        entries[count].value_length = encoded_length + 16U;
        ++count;
    }
    for (i = 0U; status == LXP_OK && i < accounts->count; ++i) {
        uint8_t balance[16];
        if (!accounts->accounts[i].has_asset) continue;
        (void)memcpy(entries[count].key, accounts->accounts[i].asset_id, 32U);
        (void)memcpy(entries[count].key + 32U, accounts->accounts[i].id, 32U);
        status = lxp_u128_to_be(accounts->accounts[i].balance, balance);
        if (status != LXP_OK) break;
        (void)memcpy(entries[count].value, balance, sizeof(balance));
        entries[count].value_length = sizeof(balance);
        ++count;
    }
    if (status == LXP_OK && count == 0U)
        status = lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, NULL, 0U, root);
    if (status != LXP_OK || count == 0U) {
        free(entries); free(order); free(hashes);
        return status;
    }
    for (i = 0U; i < count; ++i) order[i] = &entries[i];
    qsort(order, count, sizeof(*order), entry_order);
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        uint8_t leaf[64U + 384U];
        (void)memcpy(leaf, order[i]->key, 64U);
        (void)memcpy(leaf + 64U, order[i]->value, order[i]->value_length);
        status = lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, leaf,
                                 64U + order[i]->value_length, hashes[i]);
    }
    while (status == LXP_OK && count > 1U) {
        size_t next = (count + 1U) / 2U;
        for (i = 0U; status == LXP_OK && i < next; ++i) {
            size_t right = i * 2U + 1U;
            if (right >= count) right = i * 2U;
            status = lxp_merkle_node_hash(hashes[i * 2U], hashes[right],
                                          hashes[i]);
        }
        count = next;
    }
    if (status == LXP_OK) (void)memcpy(root, hashes[0], 32U);
    free(entries); free(order); free(hashes);
    return status;
}
