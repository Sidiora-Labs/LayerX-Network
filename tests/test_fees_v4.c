#include "layerx/lxp_fee.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_module_ctx.h"

#include <stdio.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); return 1; } } while (0)

static lxp_fee_params module_schedule(void)
{
    lxp_fee_params schedule = {0};
    schedule.version = 4U;
    schedule.multiplier_basis_points = 10000U;
    schedule.asset_price_count = LXP_ASSET_FEE_PRICE_COUNT_V3;
    schedule.asset_prices[LXP_ASSET_FEE_PRICE_COUNT].lo = 17U;
    schedule.module_price_count = LXP_MODULE_FEE_PRICE_COUNT;
    for (size_t i = 0U; i < 6U; ++i) schedule.module_prices[i].lo = 4U;
    return schedule;
}

static int canonical(const char *output)
{
    lxp_fee_params schedule = module_schedule(), decoded;
    uint8_t bytes[LXP_FEE_PARAMS_V4_BYTES + 1U], restored[LXP_FEE_PARAMS_V4_BYTES];
    size_t length, restored_length;
    REQUIRE(lxp_fee_params_encode(&schedule, bytes, sizeof(bytes), &length) == LXP_OK);
    REQUIRE(length == 368U && bytes[1] == 4U && bytes[86] == 11U && bytes[255] == 7U);
    REQUIRE(lxp_fee_params_decode(bytes, length, &decoded) == LXP_OK && memcmp(&schedule, &decoded, sizeof(schedule)) == 0);
    REQUIRE(lxp_fee_params_encode(&decoded, restored, sizeof(restored), &restored_length) == LXP_OK);
    REQUIRE(restored_length == length && memcmp(bytes, restored, length) == 0);
    for (size_t i = 0U; i < length; ++i) REQUIRE(lxp_fee_params_decode(bytes, i, &decoded) != LXP_OK);
    bytes[length] = 0U;
    REQUIRE(lxp_fee_params_decode(bytes, length + 1U, &decoded) != LXP_OK);
    REQUIRE(lxp_fee_params_encode(&schedule, restored, length - 1U, &restored_length) == LXP_ERR_LENGTH_LIMIT);
    const size_t offsets[] = {0U, 1U, 86U, 255U};
    const uint8_t valid[] = {0U, 4U, 11U, 7U};
    for (size_t i = 0U; i < 4U; ++i) {
        for (unsigned value = 0U; value <= UINT8_MAX; ++value) {
            bytes[offsets[i]] = (uint8_t)value;
            REQUIRE((lxp_fee_params_decode(bytes, length, &decoded) == LXP_OK) == (value == valid[i]));
        }
        bytes[offsets[i]] = valid[i];
    }
    lxp_byte_span head = {bytes, LXP_FEE_PARAMS_V4_HEAD_BYTES};
    lxp_byte_span prices = {bytes + head.length, LXP_FEE_PARAMS_V4_PRICES_BYTES};
    REQUIRE(lxp_fee_stored_schedule_decode(head, prices, &decoded) == LXP_OK);
    REQUIRE(memcmp(&schedule, &decoded, sizeof(schedule)) == 0);
    REQUIRE(lxp_fee_stored_schedule_decode(head, (lxp_byte_span){NULL, 0U}, &decoded) != LXP_OK);
    REQUIRE(lxp_fee_stored_schedule_decode((lxp_byte_span){NULL, 0U}, prices, &decoded) != LXP_OK);
    REQUIRE(lxp_fee_stored_schedule_decode(prices, head, &decoded) != LXP_OK);
    REQUIRE(lxp_fee_stored_schedule_decode((lxp_byte_span){bytes, length}, prices, &decoded) != LXP_OK);
    for (size_t i = 0U; i < prices.length; ++i)
        REQUIRE(lxp_fee_stored_schedule_decode(head, (lxp_byte_span){prices.bytes, i}, &decoded) != LXP_OK);
    REQUIRE(lxp_fee_stored_schedule_decode(head, (lxp_byte_span){prices.bytes, prices.length + 1U}, &decoded) != LXP_OK);
    for (uint16_t version = 1U; version <= 3U; ++version) {
        lxp_fee_params legacy = {0};
        legacy.version = version;
        legacy.multiplier_basis_points = 10000U;
        legacy.asset_price_count = version == 1U ? 0U : version == 2U ? 10U : 11U;
        REQUIRE(lxp_fee_params_encode(&legacy, restored, sizeof(restored), &restored_length) == LXP_OK);
        REQUIRE(restored_length == (version == 1U ? 86U : version == 2U ? 247U : 255U));
        REQUIRE(lxp_fee_stored_schedule_decode((lxp_byte_span){restored, restored_length}, prices, &decoded) != LXP_OK);
        REQUIRE(lxp_fee_stored_schedule_decode((lxp_byte_span){restored, restored_length}, (lxp_byte_span){NULL, 0U}, &decoded) == LXP_OK);
        for (size_t i = 0U; i < LXP_MODULE_FEE_PRICE_COUNT; ++i) {
            legacy.module_prices[i].lo = 1U;
            REQUIRE(lxp_fee_params_encode(&legacy, restored, sizeof(restored), &restored_length) != LXP_OK);
            legacy.module_prices[i].lo = 0U;
        }
        legacy.module_price_count = 7U;
        REQUIRE(lxp_fee_params_encode(&legacy, restored, sizeof(restored), &restored_length) != LXP_OK);
    }
    if (output != NULL) {
        FILE *file = fopen(output, "wb");
        REQUIRE(file != NULL);
        bool failed = fwrite(bytes, 1U, length, file) != length;
        if (fclose(file) != 0) failed = true;
        REQUIRE(!failed);
    }
    return 0;
}

static int arithmetic(void)
{
    lxp_fee_params schedule = module_schedule();
    lxp_u128 fee;
    for (uint16_t module = LXP_MODULE_ESCROW; module <= LXP_MODULE_BRIDGE; ++module) {
        REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)module << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_OK);
        REQUIRE(fee.hi == 0U && fee.lo == (module == LXP_MODULE_BRIDGE ? 0U : 4U));
    }
    REQUIRE(lxp_fee_compute(&schedule, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) == LXP_OK && fee.lo == 17U);
    REQUIRE(lxp_fee_compute(&schedule, LX_ASSET_SEND, (lxp_fee_meter){0}, &fee) == LXP_OK && lxp_u128_is_zero(fee));
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_GOVERNANCE << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_OK);
    REQUIRE(lxp_fee_limit_check(fee, (lxp_u128){0U, 4U}, (lxp_u128){0U, 0U}) == LXP_ERR_FEE_UNPAYABLE);
    REQUIRE(lxp_fee_limit_check(fee, (lxp_u128){0U, 3U}, (lxp_u128){0U, 4U}) == LXP_ERR_FEE_LIMIT);
    schedule.base_fee.lo = 3U;
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_BRIDGE << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_OK && fee.lo == 3U);
    schedule.module_prices[6].lo = 9U;
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_BRIDGE << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_OK && fee.lo == 12U);
    lxp_fee_meter exact = {.exact_program_fee_present = true, .program_fee_schedule_version = 7U,
        .exact_program_fee_units = {0U, 23U}};
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_PROGRAMS << 16U) | 3U, exact, &fee) == LXP_OK && fee.lo == 23U);
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_GOVERNANCE << 16U) | 3U, exact, &fee) != LXP_OK);
    schedule.module_prices[5] = (lxp_u128){UINT64_MAX, UINT64_MAX};
    fee = (lxp_u128){1U, 9U};
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_GOVERNANCE << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_ERR_OVERFLOW);
    REQUIRE(fee.hi == 1U && fee.lo == 9U);
    schedule.base_fee.lo = 0U;
    schedule.multiplier_basis_points = 10001U;
    REQUIRE(lxp_fee_compute(&schedule, ((uint32_t)LXP_MODULE_GOVERNANCE << 16U) | 1U, (lxp_fee_meter){0}, &fee) == LXP_ERR_OVERFLOW);
    schedule = module_schedule();
    schedule.asset_prices[10].hi = 1U;
    REQUIRE(lxp_fee_compute(&schedule, LX_ASSET_WITHDRAW, (lxp_fee_meter){0}, &fee) == LXP_ERR_VERSION_UNSUPPORTED);
    return 0;
}

static int named_parameters(void)
{
    static const char *const base[] = {"fee.base", "fee.activity", "fee.byte", "fee.exec", "fee.storage", "fee.multiplier_bps"};
    lxp_param_table table;
    lxp_fee_params schedule;
    uint32_t version;
    REQUIRE(lxp_param_table_init(&table) == LXP_OK);
    for (size_t i = 0U; i < 6U; ++i)
        REQUIRE(lxp_param_set_bounds(&table, (lxp_byte_span){(const uint8_t *)base[i], strlen(base[i])},
            1U, 0U, UINT64_MAX, i == 5U ? 10000U : 0U, 1U) == LXP_OK);
    REQUIRE(lxp_param_set_bounds(&table, (lxp_byte_span){(const uint8_t *)"fee.encoding", 12U}, 1U, 0U, UINT64_MAX, 4U, 1U) == LXP_OK);
    for (size_t i = 0U; i < 11U; ++i) {
        const char *name = lxp_asset_fee_name_for_version(4U, i);
        REQUIRE(name != NULL);
        REQUIRE(lxp_param_set_bounds(&table, (lxp_byte_span){(const uint8_t *)name, strlen(name)},
            1U, 0U, UINT64_MAX, i == 10U ? 17U : 0U, 1U) == LXP_OK);
    }
    for (size_t i = 0U; i < 7U; ++i) {
        REQUIRE(lxp_fee_schedule(&table, 2U, NULL, &schedule, &version) != LXP_OK);
        const char *name = lxp_module_fee_name(i);
        REQUIRE(name != NULL);
        REQUIRE(lxp_param_set_bounds(&table, (lxp_byte_span){(const uint8_t *)name, strlen(name)},
            1U, 0U, UINT64_MAX, i == 6U ? 0U : 4U, 1U) == LXP_OK);
    }
    REQUIRE(lxp_module_fee_name(7U) == NULL);
    REQUIRE(lxp_fee_schedule(&table, 2U, NULL, &schedule, &version) == LXP_OK);
    lxp_fee_params expected = module_schedule();
    REQUIRE(version == 1U && memcmp(&schedule, &expected, sizeof(schedule)) == 0);
    return 0;
}

static lxp_result put(lxp_kernel *kernel, const uint8_t key[32], const uint8_t *value, size_t length)
{
    static lxp_module_ctx context;
    uint8_t storage[4096];
    lxp_arena arena;
    lxp_result status = lxp_arena_init(&arena, storage, sizeof(storage));
    if (status == LXP_OK) status = lxp_module_ctx_init(&context, kernel, LXP_MODULE_GOVERNANCE, 900U, 1U, 1U, 1000U, &arena, true);
    if (status == LXP_OK) status = lxp_ctx_kv_put(&context, key, 32U, value, length);
    if (status == LXP_OK) status = lxp_module_ctx_commit(&context);
    if (status == LXP_OK) status = lxp_state_root(kernel, kernel->current_state_root);
    return status;
}

static int committed(void)
{
    static lxp_kernel kernel;
    static lxp_state_store store;
    static lxp_state_journal journal;
    static lxp_param_table parameters;
    static const uint8_t parameter_key[32] = "parameter-version", head_key[32] = "fee.schedule", prices_key[32] = "fee.module-prices";
    lxp_fee_params schedule = module_schedule(), changed;
    uint8_t bytes[LXP_FEE_PARAMS_V4_BYTES], parameter[32] = {0}, root[32];
    size_t length;
    REQUIRE(lxp_state_store_init(&store, 0U) == LXP_OK);
    REQUIRE(lxp_param_table_init(&parameters) == LXP_OK && lxp_kernel_create(&kernel, &store, &journal, &parameters, 1U) == LXP_OK);
    REQUIRE(lxp_kernel_register_module(&kernel, lxp_governance_module_iface()) == LXP_OK);
    parameter[31] = 1U;
    REQUIRE(put(&kernel, parameter_key, parameter, sizeof(parameter)) == LXP_OK);
    REQUIRE(lxp_fee_params_encode(&schedule, bytes, sizeof(bytes), &length) == LXP_OK);
    REQUIRE(put(&kernel, prices_key, bytes + 256U, 112U) == LXP_OK);
    REQUIRE(lxp_fee_committed_schedule(&kernel, 1U, &changed) != LXP_OK);
    REQUIRE(put(&kernel, head_key, bytes, 256U) == LXP_OK);
    REQUIRE(lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) == LXP_OK);
    (void)memcpy(root, kernel.current_state_root, 32U);
    for (size_t i = 0U; i < LXP_MODULE_FEE_PRICE_COUNT; ++i) {
        changed = schedule; ++changed.module_prices[i].lo;
        REQUIRE(lxp_fee_replay_schedule_verify(&kernel, 1U, &changed) != LXP_OK);
        bytes[271U + 16U * i] ^= 1U;
        REQUIRE(put(&kernel, prices_key, bytes + 256U, 112U) == LXP_OK);
        REQUIRE(memcmp(root, kernel.current_state_root, 32U) != 0);
        REQUIRE(lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_OK);
        bytes[271U + 16U * i] ^= 1U;
        REQUIRE(put(&kernel, prices_key, bytes + 256U, 112U) == LXP_OK);
        REQUIRE(lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) == LXP_OK);
    }
    REQUIRE(put(&kernel, prices_key, bytes + 256U, 111U) == LXP_OK);
    REQUIRE(lxp_fee_replay_schedule_verify(&kernel, 1U, &schedule) != LXP_OK);
    REQUIRE(put(&kernel, prices_key, bytes + 256U, 112U) == LXP_OK);
    bytes[1] = 3U;
    REQUIRE(put(&kernel, head_key, bytes, 255U) == LXP_OK);
    REQUIRE(lxp_fee_committed_schedule(&kernel, 1U, &changed) != LXP_OK);
    REQUIRE(lxp_state_store_destroy(&store) == LXP_OK);
    return 0;
}

int main(int argc, char **argv)
{
    REQUIRE(argc <= 2);
    REQUIRE(canonical(argc == 2 ? argv[1] : NULL) == 0);
    REQUIRE(arithmetic() == 0);
    REQUIRE(named_parameters() == 0);
    REQUIRE(committed() == 0);
    return 0;
}
