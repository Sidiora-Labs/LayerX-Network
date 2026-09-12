#include "layerx/lxp_fee.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_result resolve(const lxp_param_table *parameters, const char *name,
                          uint64_t epoch, const uint8_t cohort_id[32],
                          uint64_t *value, uint32_t *version)
{
    return lxp_gov_param_enact(
        parameters,
        (lxp_byte_span){(const uint8_t *)name, strlen(name)},
        epoch, cohort_id, value, version);
}

lxp_result lxp_fee_schedule(
    const lxp_param_table *parameters, uint64_t batch_epoch,
    const uint8_t cohort_id[32], lxp_fee_params *schedule,
    uint32_t *parameter_version)
{
    static const char *const keys[6] = {
        "fee.base", "fee.activity", "fee.byte", "fee.exec",
        "fee.storage", "fee.multiplier_bps"
    };
    uint64_t values[6];
    uint32_t version = 0U;
    size_t i;
    lxp_result status;
    if (parameters == NULL || schedule == NULL || parameter_version == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < 6U; ++i) {
        uint32_t resolved_version;
        status = resolve(parameters, keys[i], batch_epoch, cohort_id,
                         &values[i], &resolved_version);
        if (status != LXP_OK) return status;
        if (i == 0U) version = resolved_version;
        else if (resolved_version != version) return LXP_FATAL_INVARIANT;
    }
    if (values[5] > UINT32_MAX) return LXP_ERR_PARAMETER_BOUNDS;
    (void)memset(schedule, 0, sizeof(*schedule));
    schedule->version = 1U;
    schedule->base_fee = (lxp_u128){0U, values[0]};
    schedule->per_activity_type_unit = (lxp_u128){0U, values[1]};
    schedule->per_encoded_byte = (lxp_u128){0U, values[2]};
    schedule->per_execution_unit = (lxp_u128){0U, values[3]};
    schedule->per_storage_unit = (lxp_u128){0U, values[4]};
    schedule->multiplier_basis_points = (uint32_t)values[5];
    for (i = 0U; i < parameters->count; ++i) {
        const lxp_param_entry *entry = &parameters->entries[i];
        if (entry->key_length != 12U || memcmp(entry->key, "fee.encoding", 12U) != 0) continue;
        uint64_t encoding;
        uint32_t encoding_version;
        status = resolve(parameters, "fee.encoding", batch_epoch, cohort_id,
                         &encoding, &encoding_version);
        if (status != LXP_OK) return status;
        if (encoding_version != version) return LXP_FATAL_INVARIANT;
        if (encoding != 1U && encoding != 2U && encoding != 3U) return LXP_ERR_VERSION_UNSUPPORTED;
        schedule->version = (uint16_t)encoding;
        if (encoding == 2U || encoding == 3U) {
            schedule->asset_price_count = encoding == 3U ?
                LXP_ASSET_FEE_PRICE_COUNT_V3 : LXP_ASSET_FEE_PRICE_COUNT;
            for (size_t price = 0U; price < schedule->asset_price_count; ++price) {
                uint64_t value;
                uint32_t price_version;
                status = resolve(parameters, lxp_asset_fee_name_for_version(schedule->version, price), batch_epoch,
                                 cohort_id, &value, &price_version);
                if (status != LXP_OK) return status;
                if (price_version != version) return LXP_FATAL_INVARIANT;
                schedule->asset_prices[price] = (lxp_u128){0U, value};
            }
        }
    }
    *parameter_version = version;
    return LXP_OK;
}

lxp_result lxp_fee_committed_schedule(const lxp_kernel *kernel,
    uint32_t parameter_version, lxp_fee_params *schedule)
{
    const lxp_module_kv_entry *found = NULL;
    if (kernel == NULL || schedule == NULL || kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE ||
            (entry->key_length != 12U && entry->key_length != 32U) ||
            memcmp(entry->key, "fee.schedule", 12U) != 0) continue;
        if (entry->key_length == 32U && !lxp_ct_is_zero(entry->key + 12U, 20U)) continue;
        if (found != NULL) return LXP_FATAL_INVARIANT;
        found = entry;
    }
    if (found != NULL) return lxp_fee_params_decode(found->value, found->value_length, schedule);
    if (parameter_version != 1U) return LXP_ERR_VERSION_UNSUPPORTED;
    (void)memset(schedule, 0, sizeof(*schedule));
    schedule->version = 1U;
    schedule->multiplier_basis_points = 10000U;
    return LXP_OK;
}

lxp_result lxp_fee_replay_schedule_verify(const lxp_kernel *kernel,
    uint32_t parameter_version, const lxp_fee_params *cached)
{
    static const uint8_t parameter_key[32] = "parameter-version";
    const lxp_module_kv_entry *parameter = NULL;
    lxp_fee_params committed;
    uint8_t actual[LXP_FEE_PARAMS_V3_BYTES], expected[LXP_FEE_PARAMS_V3_BYTES];
    size_t actual_length, expected_length;
    lxp_result status;
    if (kernel == NULL || cached == NULL || kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != sizeof(parameter_key) ||
            memcmp(entry->key, parameter_key, sizeof(parameter_key)) != 0) continue;
        if (parameter != NULL) return LXP_FATAL_INVARIANT;
        parameter = entry;
    }
    if (parameter == NULL || parameter->value_length != 32U ||
        !lxp_ct_is_zero(parameter->value, 28U) || parameter_version == 0U ||
        parameter_version > UINT16_MAX ||
        (((uint32_t)parameter->value[28] << 24U) | ((uint32_t)parameter->value[29] << 16U) |
         ((uint32_t)parameter->value[30] << 8U) | parameter->value[31]) != parameter_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_fee_committed_schedule(kernel, parameter_version, &committed);
    if (status == LXP_OK)
        status = lxp_fee_params_encode(&committed, expected, sizeof(expected), &expected_length);
    if (status == LXP_OK)
        status = lxp_fee_params_encode(cached, actual, sizeof(actual), &actual_length);
    if (status == LXP_OK && (actual_length != expected_length ||
        lxp_ct_memcmp(actual, expected, expected_length) != 0))
        status = LXP_ERR_VERSION_UNSUPPORTED;
    return status;
}
