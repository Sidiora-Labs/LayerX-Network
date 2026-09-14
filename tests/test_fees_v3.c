#include "layerx/lxp_fee.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_module_ctx.h"

#include <stdio.h>
#include <string.h>

static int parameter(lxp_param_table *table, const char *key, uint64_t value)
{
    return lxp_param_set_bounds(table,
        (lxp_byte_span){(const uint8_t *)key, strlen(key)}, 1U,
        0U, UINT64_MAX, value, 1U) == LXP_OK ? 0 : 1;
}

static int named_withdrawal(void)
{
    static const char *const base[] = {"fee.base", "fee.activity", "fee.byte",
        "fee.exec", "fee.storage", "fee.multiplier_bps"};
    static const uint16_t ordinals[] = {1U,2U,3U,4U,5U,6U,7U,8U,10U,11U,9U};
    lxp_param_table table;
    lxp_fee_params schedule;
    uint32_t version;
    lxp_u128 fee;
    if (lxp_param_table_init(&table) != LXP_OK) return 1;
    for (size_t i = 0U; i < 6U; ++i)
        if (parameter(&table, base[i], i == 5U ? 10000U : 0U) != 0) return 1;
    if (parameter(&table, "fee.encoding", 3U) != 0) return 1;
    for (size_t i = 0U; i < LXP_ASSET_FEE_PRICE_COUNT; ++i)
        if (parameter(&table, lxp_asset_fee_name(i), 101U + i) != 0) return 1;
    if (lxp_fee_schedule(&table, 2U, NULL, &schedule, &version) != LXP_ERR_PARAMETER_BOUNDS ||
        parameter(&table, "fee.asset.withdraw", 111U) != 0 ||
        lxp_fee_schedule(&table, 2U, NULL, &schedule, &version) != LXP_OK ||
        schedule.version != 3U || schedule.asset_price_count != LXP_ASSET_FEE_PRICE_COUNT_V3)
        return 1;
    for (size_t i = 0U; i < LXP_ASSET_FEE_PRICE_COUNT_V3; ++i)
        if (lxp_fee_compute(&schedule, (((uint32_t)LXP_MODULE_ASSET << 16U) | ordinals[i]),
                (lxp_fee_meter){0}, &fee) != LXP_OK || fee.hi != 0U || fee.lo != 101U + i)
            return 1;
    if (lxp_fee_compute(&schedule, (((uint32_t)LXP_MODULE_ASSET << 16U) | 12U),
            (lxp_fee_meter){0}, &fee) != LXP_ERR_UNKNOWN_ACTIVITY ||
        lxp_asset_fee_name(LXP_ASSET_FEE_PRICE_COUNT) != NULL ||
        lxp_asset_fee_name_for_version(3U, LXP_ASSET_FEE_PRICE_COUNT_V3) != NULL ||
        strcmp(lxp_asset_fee_name_for_version(3U, LXP_ASSET_FEE_PRICE_COUNT), "fee.asset.withdraw") != 0)
        return 1;
    return 0;
}

static int canonical_withdrawal(const char *output)
{
    lxp_fee_params schedule = {0}, decoded;
    uint8_t bytes[LXP_FEE_PARAMS_V3_BYTES + 1U];
    uint8_t restored[LXP_FEE_PARAMS_V3_BYTES];
    size_t length, restored_length;
    lxp_u128 fee;
    schedule.version = 3U;
    schedule.multiplier_basis_points = 10000U;
    schedule.asset_price_count = LXP_ASSET_FEE_PRICE_COUNT_V3;
    schedule.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].lo = 17U;
    if (lxp_fee_params_encode(&schedule, bytes, sizeof(bytes), &length) != LXP_OK ||
        length != 255U || bytes[1] != 3U || bytes[86] != 11U ||
        lxp_fee_params_decode(bytes, length, &decoded) != LXP_OK ||
        memcmp(&schedule, &decoded, sizeof(schedule)) != 0 ||
        lxp_fee_params_encode(&decoded, restored, sizeof(restored), &restored_length) != LXP_OK ||
        restored_length != length || memcmp(restored, bytes, length) != 0 ||
        lxp_fee_compute(&decoded, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) != LXP_OK ||
        fee.hi != 0U || fee.lo != 17U)
        return 1;
    for (size_t i = 0U; i < length; ++i)
        if (lxp_fee_params_decode(bytes, i, &decoded) == LXP_OK) return 1;
    bytes[length] = 0U;
    if (lxp_fee_params_decode(bytes, length + 1U, &decoded) != LXP_ERR_NON_CANONICAL ||
        lxp_fee_params_encode(&schedule, restored, length - 1U, &restored_length) != LXP_ERR_LENGTH_LIMIT)
        return 1;
    for (unsigned count = 0U; count <= UINT8_MAX; ++count) {
        bytes[86] = (uint8_t)count;
        if ((lxp_fee_params_decode(bytes, length, &decoded) == LXP_OK) != (count == 11U)) return 1;
    }
    bytes[86] = 11U;
    for (unsigned version = 0U; version <= UINT8_MAX; ++version) {
        bytes[1] = (uint8_t)version;
        if ((lxp_fee_params_decode(bytes, length, &decoded) == LXP_OK) != (version == 3U)) return 1;
    }
    bytes[1] = 3U;
    schedule.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].hi = 1U;
    if (lxp_fee_params_encode(&schedule, restored, sizeof(restored), &restored_length) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_fee_compute(&schedule, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) != LXP_ERR_VERSION_UNSUPPORTED)
        return 1;
    schedule.asset_prices[LXP_ASSET_FEE_PRICE_COUNT] = (lxp_u128){0U, UINT64_MAX};
    if (lxp_fee_params_encode(&schedule, restored, sizeof(restored), &restored_length) != LXP_OK ||
        lxp_fee_params_decode(restored, restored_length, &decoded) != LXP_OK ||
        decoded.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].hi != 0U ||
        decoded.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].lo != UINT64_MAX)
        return 1;
    if (lxp_fee_compute(&decoded, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) != LXP_OK ||
        fee.hi != 0U || fee.lo != UINT64_MAX) return 1;
    decoded.base_fee = (lxp_u128){UINT64_MAX, UINT64_MAX};
    if (lxp_fee_compute(&decoded, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) != LXP_ERR_OVERFLOW)
        return 1;
    if (output != NULL) {
        FILE *file = fopen(output, "wb");
        if (file == NULL) return 1;
        bool failed = fwrite(bytes, 1U, length, file) != length;
        if (fclose(file) != 0) failed = true;
        if (failed) return 1;
    }
    return 0;
}

static lxp_result commit_governance_value(lxp_kernel *kernel,
    const uint8_t key[32], const uint8_t *value, size_t length)
{
    static lxp_module_ctx context;
    uint8_t storage[4096];
    lxp_arena arena;
    lxp_result status = lxp_arena_init(&arena, storage, sizeof(storage));
    if (status == LXP_OK)
        status = lxp_module_ctx_init(&context, kernel, LXP_MODULE_GOVERNANCE,
            900U, 1U, 1U, 1000U, &arena, true);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(&context, key, 32U, value, length);
    if (status == LXP_OK) status = lxp_module_ctx_commit(&context);
    if (status == LXP_OK) status = lxp_state_root(kernel, kernel->current_state_root);
    return status;
}

static int committed_replay_binding(void)
{
    static const uint8_t parameter_key[32] = "parameter-version";
    static const uint8_t fee_key[32] = "fee.schedule";
    static lxp_kernel kernel;
    static lxp_state_store store;
    static lxp_state_journal journal;
    static lxp_param_table parameters;
    lxp_fee_params schedule = {0}, stale;
    uint8_t parameter_value[32] = {0};
    uint8_t encoded[LXP_FEE_PARAMS_V3_BYTES];
    size_t length;
    int result = 1;
    if (lxp_state_store_init(&store, 0U) != LXP_OK) return 1;
    if (lxp_param_table_init(&parameters) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 1U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lxp_governance_module_iface()) != LXP_OK)
        goto done;
    schedule.version = 1U;
    schedule.multiplier_basis_points = 10000U;
    if (lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
        goto done;
    parameter_value[31] = 1U;
    if (commit_governance_value(&kernel, parameter_key, parameter_value, sizeof(parameter_value)) != LXP_OK)
        goto done;
    for (uint16_t version = 1U; version <= 3U; ++version) {
        schedule.version = version;
        schedule.asset_price_count = version == 1U ? 0U :
            version == 2U ? LXP_ASSET_FEE_PRICE_COUNT : LXP_ASSET_FEE_PRICE_COUNT_V3;
        if (lxp_fee_params_encode(&schedule, encoded, sizeof(encoded), &length) != LXP_OK ||
            commit_governance_value(&kernel, fee_key, encoded, length) != LXP_OK ||
            lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_OK ||
            lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
            goto done;
        stale = schedule;
        stale.base_fee.lo = 1U;
        if (lxp_fee_replay_schedule_verify(&kernel, 1U, &stale) != LXP_ERR_VERSION_UNSUPPORTED)
            goto done;
    }
    stale = schedule;
    schedule.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].lo = 17U;
    if (lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_fee_params_encode(&schedule, encoded, sizeof(encoded), &length) != LXP_OK ||
        commit_governance_value(&kernel, fee_key, encoded, length) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 1U, &stale) != LXP_ERR_VERSION_UNSUPPORTED)
        goto done;
    parameter_value[31] = 2U;
    if (commit_governance_value(&kernel, parameter_key, parameter_value, sizeof(parameter_value)) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &stale) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_fee_replay_schedule_verify(&kernel, 0U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_fee_replay_schedule_verify(&kernel, UINT16_MAX + 1U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
        goto done;
    parameter_value[0] = 1U;
    if (commit_governance_value(&kernel, parameter_key, parameter_value, sizeof(parameter_value)) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
        goto done;
    parameter_value[0] = 0U;
    if (commit_governance_value(&kernel, parameter_key, parameter_value, sizeof(parameter_value) - 1U) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        commit_governance_value(&kernel, parameter_key, parameter_value, sizeof(parameter_value)) != LXP_OK ||
        commit_governance_value(&kernel, fee_key, encoded, length - 1U) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_ERR_NON_CANONICAL ||
        commit_governance_value(&kernel, fee_key, encoded, length) != LXP_OK ||
        lxp_fee_replay_schedule_verify(&kernel, 2U, &schedule) != LXP_OK)
        goto done;
    result = 0;
done:
    if (lxp_state_store_destroy(&store) != LXP_OK) return 1;
    return result;
}

int main(int argc, char **argv)
{
    if (argc > 2) return 1;
    return named_withdrawal() != 0 || canonical_withdrawal(argc == 2 ? argv[1] : NULL) != 0 ||
        committed_replay_binding() != 0;
}
