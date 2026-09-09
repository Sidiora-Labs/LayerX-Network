#include "layerx/lxp_fee.h"
#include "layerx/lxp_module.h"

#include <stddef.h>
#include <string.h>

static const uint16_t asset_ordinals[LXP_ASSET_FEE_PRICE_COUNT] = {1U, 4U, 5U, 6U, 7U, 8U, 10U, 11U};

const char *lxp_asset_fee_name(size_t index)
{
    static const char *const names[LXP_ASSET_FEE_PRICE_COUNT] = {
        "fee.asset.register", "fee.asset.account_open", "fee.asset.send",
        "fee.asset.receive", "fee.asset.grant_issue", "fee.asset.grant_revoke",
        "fee.asset.mint", "fee.asset.burn"
    };
    return index < LXP_ASSET_FEE_PRICE_COUNT ? names[index] : NULL;
}

static bool fee_version_valid(const lxp_fee_params *parameters)
{
    if (parameters->version == 2U)
        return parameters->asset_price_count == LXP_ASSET_FEE_PRICE_COUNT;
    if (parameters->version != 1U || parameters->asset_price_count != 0U) return false;
    for (size_t i = 0U; i < LXP_ASSET_FEE_PRICE_COUNT; ++i)
        if (!lxp_u128_is_zero(parameters->asset_prices[i])) return false;
    return true;
}

lxp_result lxp_fee_params_encode(const lxp_fee_params *parameters,
    uint8_t *bytes, size_t capacity, size_t *length)
{
    const lxp_u128 components[] = {parameters == NULL ? (lxp_u128){0U, 0U} : parameters->base_fee,
        parameters == NULL ? (lxp_u128){0U, 0U} : parameters->per_activity_type_unit,
        parameters == NULL ? (lxp_u128){0U, 0U} : parameters->per_encoded_byte,
        parameters == NULL ? (lxp_u128){0U, 0U} : parameters->per_execution_unit,
        parameters == NULL ? (lxp_u128){0U, 0U} : parameters->per_storage_unit};
    if (parameters == NULL || bytes == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    if (!fee_version_valid(parameters)) return LXP_ERR_VERSION_UNSUPPORTED;
    size_t required = parameters->version == 2U ? 215U : 86U;
    if (capacity < required) return LXP_ERR_LENGTH_LIMIT;
    bytes[0] = 0U; bytes[1] = (uint8_t)parameters->version;
    for (size_t i = 0U; i < 5U; ++i) (void)lxp_u128_to_be(components[i], bytes + 2U + 16U * i);
    for (size_t i = 0U; i < 4U; ++i)
        bytes[82U + i] = (uint8_t)(parameters->multiplier_basis_points >> (24U - 8U * i));
    if (parameters->version == 2U) {
        bytes[86] = LXP_ASSET_FEE_PRICE_COUNT;
        for (size_t i = 0U; i < LXP_ASSET_FEE_PRICE_COUNT; ++i)
            (void)lxp_u128_to_be(parameters->asset_prices[i], bytes + 87U + 16U * i);
    }
    *length = required;
    return LXP_OK;
}

lxp_result lxp_fee_params_decode(const uint8_t *bytes, size_t length,
    lxp_fee_params *parameters)
{
    lxp_fee_params decoded = {0};
    if (bytes == NULL || parameters == NULL || length < 2U || bytes[0] != 0U)
        return LXP_ERR_NON_CANONICAL;
    if (bytes[1] != 1U && bytes[1] != 2U) return LXP_ERR_VERSION_UNSUPPORTED;
    if (length != (bytes[1] == 2U ? 215U : 86U)) return LXP_ERR_NON_CANONICAL;
    decoded.version = bytes[1];
    (void)lxp_u128_from_be(bytes + 2U, &decoded.base_fee);
    (void)lxp_u128_from_be(bytes + 18U, &decoded.per_activity_type_unit);
    (void)lxp_u128_from_be(bytes + 34U, &decoded.per_encoded_byte);
    (void)lxp_u128_from_be(bytes + 50U, &decoded.per_execution_unit);
    (void)lxp_u128_from_be(bytes + 66U, &decoded.per_storage_unit);
    for (size_t i = 0U; i < 4U; ++i)
        decoded.multiplier_basis_points = (decoded.multiplier_basis_points << 8U) | bytes[82U + i];
    if (decoded.version == 2U) {
        decoded.asset_price_count = bytes[86];
        for (size_t i = 0U; i < LXP_ASSET_FEE_PRICE_COUNT; ++i)
            (void)lxp_u128_from_be(bytes + 87U + 16U * i, &decoded.asset_prices[i]);
    }
    if (!fee_version_valid(&decoded)) return LXP_ERR_VERSION_UNSUPPORTED;
    *parameters = decoded;
    return LXP_OK;
}

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
    if (!fee_version_valid(parameters)) return LXP_ERR_VERSION_UNSUPPORTED;
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
    if (parameters->version == 2U && lxp_activity_module_id(activity_type) == LXP_MODULE_ASSET) {
        size_t index;
        for (index = 0U; index < LXP_ASSET_FEE_PRICE_COUNT; ++index)
            if (asset_ordinals[index] == lxp_activity_type_ordinal(activity_type)) break;
        if (index == LXP_ASSET_FEE_PRICE_COUNT) return LXP_ERR_UNKNOWN_ACTIVITY;
        status = lxp_u128_add(total, parameters->asset_prices[index], &total);
    } else {
        status = add_component(&total, parameters->per_activity_type_unit, activity_type);
    }
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
        (actor_main->kind != LX_ACCOUNT_AGENT_MAIN &&
         actor_main->kind != LX_ACCOUNT_AGENT_ASSET) ||
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
