#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"

#include <string.h>
#include "test_asset_payloads.h"

int main(void)
{
    lx_asset_registry registry;
    lx_asset_record record;
    lx_asset_record decoded;
    lx_asset_record *found;
    uint8_t encoded[256];
    size_t encoded_length;
    lxp_u128 amount;
    lxp_transfer_asset_state asset_state;
    lx_account_registry accounts;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:a:main";
    const char *to_name = "agent:did:key:b:main";
    uint8_t from_id[32];
    uint8_t to_id[32];
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_result transfer_result;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    uint64_t parameters = 1U;
    const lxp_module_iface *iface = lx_asset_module_iface();
    const lxp_module_registration *registration;

    (void)memset(&record, 0, sizeof(record));
    record.asset_id[0] = 1U;
    (void)memcpy(record.symbol, "USDC", 5U);
    record.symbol_length = 4U;
    record.decimals = 6U;
    (void)memcpy(record.name, "USD Coin", 8U);
    record.name_length = 8U;
    record.issuer_kind = 2U;
    record.issuer_did32[0] = 1U;
    record.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    (void)memcpy(record.custody_reference, "paxeer:usdc", 11U);
    record.custody_reference_length = 11U;
    if (lx_asset_registry_init(&registry, 4U) != LXP_OK ||
        lx_asset_register(&registry, &record, 4U, (lxp_u128){ 0U, 3U }) !=
            LXP_OK || registry.next_sequence != 5U ||
        registry.fees_charged.lo != 3U ||
        lx_asset_register(&registry, &record, 5U, (lxp_u128){ 0U, 3U }) !=
            LXP_ERR_ASSET_ALREADY_REGISTERED || registry.next_sequence != 6U ||
        registry.fees_charged.lo != 6U || registry.count != 1U ||
        lx_asset_lookup(&registry, record.asset_id, &found) != LXP_OK ||
        found->decimals != 6U ||
        lx_asset_record_encode(found, encoded, sizeof(encoded), &encoded_length) !=
            LXP_OK || lx_asset_record_decode(encoded, encoded_length, &decoded) !=
            LXP_OK || memcmp(decoded.asset_id, record.asset_id, 32U) != 0 ||
        strcmp(decoded.symbol, "USDC") != 0 ||
        decoded.custody_reference_length != 11U) return 1;
    encoded[encoded_length - 17U] = 0U;
    if (lx_asset_record_decode(encoded, encoded_length, &decoded) !=
        LXP_ERR_NON_CANONICAL) return 1;
    encoded[encoded_length - 17U] = 2U;
    record.issuer_kind = 0U;
    if (lx_asset_record_encode(&record, encoded, sizeof(encoded),
                               &encoded_length) != LXP_ERR_NON_CANONICAL)
        return 1;
    record.issuer_kind = 2U;
    if (lx_asset_amount_decode((const uint8_t *)"1000000", 7U, &amount) !=
            LXP_OK || amount.lo != 1000000U || amount.hi != 0U ||
        lx_asset_amount_decode((const uint8_t *)"1.0", 3U, &amount) !=
            LXP_ERR_INVALID_AMOUNT ||
        lx_asset_amount_decode((const uint8_t *)"1e6", 3U, &amount) !=
            LXP_ERR_INVALID_AMOUNT ||
        lx_asset_amount_decode((const uint8_t *)"01", 2U, &amount) !=
            LXP_ERR_INVALID_AMOUNT) return 1;
    if (iface == NULL || iface->module_id != LXP_MODULE_ASSET ||
        iface->activity_type_count != 11U ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, iface) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ASSET_GRANT_REVOKE, 0U,
                                       &registration) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ASSET_WITHDRAW, 0U,
                                       &registration) != LXP_OK ||
        registration->activity_type_count != 11U) return 1;
    if (test_asset_payloads(&kernel) != 0) return 1;
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  from_id) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  to_id) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)from_name, strlen(from_name),
                        from_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)to_name, strlen(to_name),
                        to_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, record.asset_id,
                                     (lxp_u128){ 0U, 10U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, record.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_pause(&registry, record.asset_id) != LXP_OK ||
        lx_asset_transfer_state(found, &asset_state) != LXP_OK) return 1;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = from;
    leg.to = to;
    (void)memcpy(leg.asset_id, record.asset_id, 32U);
    leg.amount = (lxp_u128){ 0U, 1U };
    (void)memset(&context, 0, sizeof(context));
    context.assets = &asset_state;
    context.asset_count = 1U;
    (void)memcpy(context.authorized_from, from_id, 32U);
    if (lxp_apply_transfer(&leg, &context, &transfer_result) !=
        LXP_ERR_ASSET_PAUSED) return 1;
    if (lx_asset_unpause(&registry, record.asset_id) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
}
