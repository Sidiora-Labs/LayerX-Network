#include "layerx/lxp_kernel.h"
#include "layerx/programs.h"

#include <stdio.h>
#include <string.h>

static const uint8_t record_v1[122] = {
    'L', 'X', 'M', 'R', '1',
    0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 8U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 8U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 64U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 8U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 1U,
    1U,
    0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U,
    0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U,
    0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U,
    0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U, 0xa5U
};

static void put_record(lxp_module_kv_entry *entry, const uint8_t *key,
                       size_t key_length)
{
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_PROGRAMS;
    entry->key_length = (uint16_t)key_length;
    entry->value_length = sizeof(record_v1);
    (void)memcpy(entry->key, key, key_length);
    (void)memcpy(entry->value, record_v1, sizeof(record_v1));
}

int main(void)
{
    static const uint8_t active_key[] = "progmet/active/v1";
    static const uint8_t history_key[] = {
        'p', 'r', 'o', 'g', 'm', 'e', 't', '/', 'h', 'i', 's', 't', 'o',
        'r', 'y', '/', 'v', '1', '/', 0U, 0U, 0U, 1U
    };
    static const uint64_t expected[9] = {1U, 1U, 1U, 1U, 1U,
                                         8U, 8U, 64U, 8U};
    static const uint8_t history_key_v2[] = {
        'p', 'r', 'o', 'g', 'm', 'e', 't', '/', 'h', 'i', 's', 't', 'o',
        'r', 'y', '/', 'v', '1', '/', 0U, 0U, 0U, 2U
    };
    uint8_t record_v2[sizeof(record_v1)];
    lxp_kernel kernel;
    lx_programs_metering_schedule schedule;
    (void)memset(&kernel, 0, sizeof(kernel));
    put_record(&kernel.module_kv[0], active_key, sizeof(active_key) - 1U);
    put_record(&kernel.module_kv[1], history_key, sizeof(history_key));
    kernel.module_kv_count = 2U;
    if (lxp_programs_metering_schedule_current(
            &kernel, 1U, &schedule) != LXP_OK || schedule.version != 1U ||
        schedule.activation_batch != 1U || schedule.authority_kind != 1U ||
        memcmp(schedule.coefficients, expected, sizeof(expected)) != 0)
        return 1;
    if (lxp_programs_metering_schedule_at(
            &kernel, 1U, 1U, &schedule) != LXP_OK)
        return 2;
    if (lxp_programs_metering_schedule_at(
            &kernel, 2U, 1U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
        return 3;
    (void)memcpy(record_v2, record_v1, sizeof(record_v2));
    record_v2[8] = 2U;
    record_v2[88] = 10U;
    record_v2[89] = LX_PROGRAMS_METERING_AUTHORITY_GOVERNANCE;
    put_record(&kernel.module_kv[2], history_key_v2, sizeof(history_key_v2));
    (void)memcpy(kernel.module_kv[2].value, record_v2, sizeof(record_v2));
    (void)memcpy(kernel.module_kv[0].value, record_v2, sizeof(record_v2));
    kernel.module_kv_count = 3U;
    if (lxp_programs_metering_schedule_current(
            &kernel, 9U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_programs_metering_schedule_current(
            &kernel, 10U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_programs_metering_schedule_at(
            &kernel, 2U, 9U, &schedule) != LXP_ERR_VERSION_UNSUPPORTED)
        return 4;
    kernel.module_kv[1].value[9U + 7U * 8U + 7U] = 63U;
    if (lxp_programs_metering_schedule_current(
            &kernel, 1U, &schedule) == LXP_OK)
        return 5;
    (void)puts("program metering schedule vectors ok");
    return 0;
}
