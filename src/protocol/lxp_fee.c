#include "layerx/lxp_fee.h"
#include "layerx/lxp_module.h"

#include <stddef.h>
#include <string.h>

static lxp_result multiply_units(lxp_u128 price, uint64_t units,
                                 lxp_u128 *amount)
{
    lxp_u128 remainder;
    return lxp_u128_mul_div_floor(price, (lxp_u128){ 0U, units },
                                  (lxp_u128){ 0U, 1U }, amount, &remainder);
}

static lxp_result add_component(lxp_u128 *total, lxp_u128 price,
                                uint64_t units)
{
    lxp_u128 component;
    lxp_u128 sum;
    lxp_result status = multiply_units(price, units, &component);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(*total, component, &sum);
    if (status != LXP_OK) return status;
    *total = sum;
    return LXP_OK;
}

lxp_result lxp_fee_compute(const lxp_fee_params *parameters,
                           uint32_t activity_type, lxp_fee_meter meter,
                           lxp_u128 *fee)
{
    lxp_u128 total;
    lxp_result status;
    if (parameters == NULL || fee == NULL) return LXP_ERR_NON_CANONICAL;
    if (parameters->version != 1U) return LXP_ERR_VERSION_UNSUPPORTED;
    if (meter.exact_program_fee_present) {
        if (lxp_activity_module_id(activity_type) != LXP_MODULE_PROGRAMS ||
            lxp_activity_type_ordinal(activity_type) != 3U)
            return LXP_ERR_NON_CANONICAL;
        if (meter.program_fee_schedule_version == 0U)
            return LXP_ERR_VERSION_UNSUPPORTED;
        *fee = meter.exact_program_fee_units;
        return LXP_OK;
    }
    total = parameters->base_fee;
    status = add_component(&total, parameters->per_activity_type_unit,
                           activity_type);
    if (status == LXP_OK)
        status = add_component(&total, parameters->per_encoded_byte,
                               meter.canonical_encoded_bytes);
    if (status == LXP_OK)
        status = add_component(&total, parameters->per_execution_unit,
                               meter.execution_units);
    if (status == LXP_OK)
        status = add_component(&total, parameters->per_storage_unit,
                               meter.storage_units);
    if (status != LXP_OK) return status;
    return lxp_u128_mul_bps_ceil(total, parameters->multiplier_basis_points,
                                 fee);
}

lxp_result lxp_fee_limit_check(lxp_u128 computed_fee, lxp_u128 fee_limit,
                               lxp_u128 actor_spendable_fee_balance)
{
    if (lxp_u128_cmp(actor_spendable_fee_balance, fee_limit) < 0)
        return LXP_ERR_FEE_UNPAYABLE;
    return lxp_u128_cmp(computed_fee, fee_limit) <= 0 ? LXP_OK :
           LXP_ERR_FEE_LIMIT;
}

lxp_result lxp_fee_treasury_account(lx_account_registry *registry,
                                    lx_account **treasury)
{
    static const uint8_t name[] = "system:fees";
    uint8_t account_id[32];
    lxp_result status;
    if (registry == NULL || treasury == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_account_id_from_string(name, sizeof(name) - 1U, account_id);
    if (status == LXP_OK)
        status = lx_account_lookup(registry, name, sizeof(name) - 1U,
                                   account_id, treasury);
    if (status == LXP_OK && (*treasury)->kind != LX_ACCOUNT_SYSTEM_FEES)
        status = LXP_FATAL_INVARIANT;
    return status;
}

lxp_result lxp_fee_charge(
    lx_account *actor_main, lx_account *treasury, const uint8_t asset_id[32],
    lxp_u128 fee, lxp_u128 fee_limit, lxp_transfer_context *context,
    lxp_receipt *receipt, lxp_transfer_result *transfer_result)
{
    lxp_transfer_leg leg;
    lxp_u128 original_receipt_fee;
    lxp_result status;
    if (actor_main == NULL || treasury == NULL || asset_id == NULL ||
        context == NULL || receipt == NULL || transfer_result == NULL ||
        actor_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        treasury->kind != LX_ACCOUNT_SYSTEM_FEES || lxp_u128_is_zero(fee))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_fee_limit_check(fee, fee_limit, actor_main->balance);
    if (status != LXP_OK) return status;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = actor_main;
    leg.to = treasury;
    (void)memcpy(leg.asset_id, asset_id, 32U);
    leg.amount = fee;
    leg.reason = LXP_REASON_PROTOCOL_FEE;
    leg.supply_mode = LXP_TRANSFER_CONSERVED;
    original_receipt_fee = receipt->fee_charged;
    status = lxp_apply_transfer(&leg, context, transfer_result);
    if (status != LXP_OK) {
        receipt->fee_charged = original_receipt_fee;
        return status;
    }
    receipt->fee_charged = fee;
    return LXP_OK;
}
