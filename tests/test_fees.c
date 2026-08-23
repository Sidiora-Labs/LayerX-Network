#include "layerx/lxp_fee.h"

#include <stdint.h>
#include <string.h>

static int add_parameter(lxp_param_table *table, const char *key,
                         uint64_t value)
{
    return lxp_param_set_bounds(
        table, (lxp_byte_span){(const uint8_t *)key, strlen(key)}, 1U,
        0U, UINT32_MAX, value, 1U) == LXP_OK ? 0 : 1;
}

int main(void)
{
    static const uint8_t actor_name[] = "agent:alice:main";
    static const uint8_t treasury_name[] = "system:fees";
    uint8_t actor_id[32];
    uint8_t treasury_id[32];
    uint8_t fee_asset[32] = {0x40U, 0x32U};
    uint8_t proposal_id[32] = {1U};
    lxp_param_table parameters;
    lxp_fee_params historical_schedule;
    lxp_fee_params current_schedule;
    uint32_t historical_version;
    uint32_t current_version;
    lx_account_registry registry;
    lx_account *actor;
    lx_account *opened_treasury;
    lx_account *treasury;
    lxp_transfer_asset_state asset;
    lxp_transfer_context context;
    lxp_transfer_result transfer_result;
    lxp_receipt receipt;
    lxp_u128 initial_actor = {0U, UINT64_C(1000000000000)};
    lxp_u128 charged_total = {0U, 0U};
    lxp_u128 expected_actor;
    size_t i;

    if (lxp_param_table_init(&parameters) != LXP_OK ||
        add_parameter(&parameters, "fee.base", 10U) != 0 ||
        add_parameter(&parameters, "fee.activity", 2U) != 0 ||
        add_parameter(&parameters, "fee.byte", 3U) != 0 ||
        add_parameter(&parameters, "fee.exec", 4U) != 0 ||
        add_parameter(&parameters, "fee.storage", 5U) != 0 ||
        add_parameter(&parameters, "fee.multiplier_bps", 10001U) != 0 ||
        lxp_fee_schedule(&parameters, 2U, NULL, &historical_schedule,
                         &historical_version) != LXP_OK ||
        lxp_param_apply_ordered(
            &parameters,
            (lxp_byte_span){(const uint8_t *)"fee.base", 8U},
            20U, 5U, proposal_id, true) != LXP_OK ||
        lxp_fee_schedule(&parameters, 6U, NULL, &current_schedule,
                         &current_version) != LXP_OK ||
        historical_schedule.base_fee.lo != 10U ||
        current_schedule.base_fee.lo != 20U ||
        historical_version == current_version)
        return 1;
    if (lx_account_registry_init(&registry) != LXP_OK ||
        lx_account_id_from_string(actor_name, sizeof(actor_name) - 1U,
                                  actor_id) != LXP_OK ||
        lx_account_id_from_string(treasury_name, sizeof(treasury_name) - 1U,
                                  treasury_id) != LXP_OK ||
        lx_account_open(&registry, actor_name, sizeof(actor_name) - 1U,
                        actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL,
                        &actor) != LXP_OK ||
        lx_account_open(&registry, treasury_name, sizeof(treasury_name) - 1U,
                        treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS, NULL,
                        &opened_treasury) != LXP_OK ||
        lxp_fee_treasury_account(&registry, &treasury) != LXP_OK ||
        treasury != opened_treasury ||
        lxp_ledger_bootstrap_balance(actor, fee_asset, initial_actor, 1U) !=
            LXP_OK ||
        lxp_ledger_bootstrap_balance(treasury, fee_asset,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK)
        return 1;
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, fee_asset, 32U);
    asset.registered = true;
    (void)memset(&context, 0, sizeof(context));
    context.assets = &asset;
    context.asset_count = 1U;
    (void)memcpy(context.authorized_from, actor->id, 32U);
    context.origin_module_id = 1U;
    context.debit_authority_kind = LXP_AUTH_OWNER;

    for (i = 0U; i < 1000U; ++i) {
        lxp_fee_meter meter = {
            .canonical_encoded_bytes = 100U + i,
            .execution_units = 200U + i * 2U,
            .storage_units = 50U + i * 3U,
            .exact_program_fee_present = false,
            .program_fee_schedule_version = 0U,
            .exact_program_fee_units = {0U, 0U}
        };
        lxp_u128 fee;
        lxp_u128 updated;
        const lxp_fee_params *schedule = i < 500U ?
            &historical_schedule : &current_schedule;
        if (lxp_fee_compute(schedule, (uint32_t)(i % 17U + 1U), meter,
                            &fee) != LXP_OK)
            return 1;
        (void)memset(&receipt, 0, sizeof(receipt));
        context.actor_sequence = actor->next_sequence;
        if (lxp_fee_charge(actor, treasury, fee_asset, fee, fee, &context,
                           &receipt, &transfer_result) != LXP_OK ||
            lxp_u128_cmp(receipt.fee_charged, fee) != 0 ||
            lxp_u128_add(charged_total, fee, &updated) != LXP_OK)
            return 1;
        charged_total = updated;
    }
    if (lxp_u128_sub(initial_actor, charged_total, &expected_actor) != LXP_OK ||
        lxp_u128_cmp(actor->balance, expected_actor) != 0 ||
        lxp_u128_cmp(treasury->balance, charged_total) != 0)
        return 1;
    (void)memset(&receipt, 0, sizeof(receipt));
    receipt.fee_charged = (lxp_u128){0U, 77U};
    context.actor_sequence = actor->next_sequence;
    if (lxp_fee_charge(actor, treasury, fee_asset, (lxp_u128){0U, 10U},
                       (lxp_u128){0U, 9U}, &context, &receipt,
                       &transfer_result) != LXP_ERR_FEE_LIMIT ||
        receipt.fee_charged.lo != 77U ||
        lxp_u128_cmp(actor->balance, expected_actor) != 0 ||
        lxp_u128_cmp(treasury->balance, charged_total) != 0)
        return 1;
    return 0;
}
