#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <limits.h>
#include <string.h>

enum {
    PROGRAMS_METERING_RECORD_BYTES = LX_PROGRAMS_METERING_RECORD_BYTES
};

static const uint8_t metering_active_key[] = "progmet/active/v1";
static const uint8_t metering_history_prefix[] = "progmet/history/v1/";
static const uint8_t metering_record_magic[5] = {'L', 'X', 'M', 'R', '1'};
static const uint64_t metering_v1_coefficients[
    LX_PROGRAMS_METERING_COEFFICIENTS] = {1U, 1U, 1U, 1U, 1U,
                                          8U, 8U, 64U, 8U};

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static bool schedule_valid_for_encoding(
    const lx_programs_metering_schedule *schedule, bool allow_empty_authority)
{
    size_t index;
    if (schedule == NULL || schedule->version == 0U ||
        schedule->activation_batch == 0U ||
        (schedule->authority_kind != LX_PROGRAMS_METERING_AUTHORITY_GENESIS &&
         schedule->authority_kind !=
             LX_PROGRAMS_METERING_AUTHORITY_GOVERNANCE) ||
        (!allow_empty_authority &&
         lxp_ct_is_zero(schedule->authority_digest, 32U)))
        return false;
    for (index = 0U; index < LX_PROGRAMS_METERING_COEFFICIENTS; ++index)
        if (schedule->coefficients[index] == 0U ||
            (schedule->version == 1U &&
             schedule->coefficients[index] !=
                 metering_v1_coefficients[index]))
            return false;
    return true;
}

static bool schedule_valid(const lx_programs_metering_schedule *schedule)
{
    return schedule_valid_for_encoding(schedule, false);
}

static bool schedule_equal(const lx_programs_metering_schedule *left,
                           const lx_programs_metering_schedule *right)
{
    return left != NULL && right != NULL && left->version == right->version &&
        left->activation_batch == right->activation_batch &&
        left->authority_kind == right->authority_kind &&
        memcmp(left->coefficients, right->coefficients,
               sizeof(left->coefficients)) == 0 &&
        lxp_ct_memcmp(left->authority_digest,
                      right->authority_digest, 32U) == 0;
}

static bool schedule_lineage_valid(
    const lx_programs_metering_schedule *schedule)
{
    return schedule != NULL &&
        ((schedule->version == 1U && schedule->activation_batch == 1U &&
          schedule->authority_kind ==
              LX_PROGRAMS_METERING_AUTHORITY_GENESIS) ||
         (schedule->version > 1U && schedule->activation_batch > 1U &&
          schedule->authority_kind ==
              LX_PROGRAMS_METERING_AUTHORITY_GOVERNANCE));
}

static lxp_result record_encode(
    const lx_programs_metering_schedule *schedule,
    uint8_t encoded[PROGRAMS_METERING_RECORD_BYTES])
{
    size_t offset = 0U;
    size_t index;
    if (!schedule_valid(schedule) || encoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded + offset, metering_record_magic,
                 sizeof(metering_record_magic));
    offset += sizeof(metering_record_magic);
    write_u32(encoded + offset, schedule->version);
    offset += 4U;
    for (index = 0U; index < LX_PROGRAMS_METERING_COEFFICIENTS; ++index) {
        write_u64(encoded + offset, schedule->coefficients[index]);
        offset += 8U;
    }
    write_u64(encoded + offset, schedule->activation_batch);
    offset += 8U;
    encoded[offset++] = schedule->authority_kind;
    (void)memcpy(encoded + offset, schedule->authority_digest, 32U);
    offset += 32U;
    return offset == PROGRAMS_METERING_RECORD_BYTES ? LXP_OK :
                                                     LXP_FATAL_INVARIANT;
}

static lxp_result record_decode(
    const uint8_t *encoded, size_t length,
    lx_programs_metering_schedule *schedule)
{
    size_t offset = 0U;
    size_t index;
    if (encoded == NULL || schedule == NULL ||
        length != PROGRAMS_METERING_RECORD_BYTES ||
        memcmp(encoded, metering_record_magic,
               sizeof(metering_record_magic)) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(schedule, 0, sizeof(*schedule));
    offset += sizeof(metering_record_magic);
    schedule->version = read_u32(encoded + offset);
    offset += 4U;
    for (index = 0U; index < LX_PROGRAMS_METERING_COEFFICIENTS; ++index) {
        schedule->coefficients[index] = read_u64(encoded + offset);
        offset += 8U;
    }
    schedule->activation_batch = read_u64(encoded + offset);
    offset += 8U;
    schedule->authority_kind = encoded[offset++];
    (void)memcpy(schedule->authority_digest, encoded + offset, 32U);
    return schedule_valid(schedule) ? LXP_OK : LXP_ERR_NON_CANONICAL;
}

static void history_key(
    uint32_t version,
    uint8_t key[sizeof(metering_history_prefix) - 1U + 4U])
{
    (void)memcpy(key, metering_history_prefix,
                 sizeof(metering_history_prefix) - 1U);
    write_u32(key + sizeof(metering_history_prefix) - 1U, version);
}

static lxp_result kernel_record(
    const lxp_kernel *kernel, const uint8_t *key, size_t key_length,
    lx_programs_metering_schedule *schedule)
{
    size_t index;
    if (kernel == NULL || key == NULL || key_length == 0U ||
        schedule == NULL || kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == key_length &&
            memcmp(entry->key, key, key_length) == 0)
            return record_decode(entry->value, entry->value_length, schedule);
    }
    return LXP_ERR_VERSION_UNSUPPORTED;
}

lxp_result lxp_programs_metering_schedule_at(
    const lxp_kernel *kernel, uint32_t recorded_version,
    uint64_t receipt_batch_number,
    lx_programs_metering_schedule *schedule)
{
    uint8_t key[sizeof(metering_history_prefix) - 1U + 4U];
    lxp_result status;
    if (recorded_version == 0U || receipt_batch_number == 0U)
        return LXP_ERR_VERSION_UNSUPPORTED;
    history_key(recorded_version, key);
    status = kernel_record(kernel, key, sizeof(key), schedule);
    if (status == LXP_OK && schedule->version != recorded_version)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK && !schedule_lineage_valid(schedule))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK &&
        !lxp_program_metering_schedule_available(schedule->version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK && schedule->activation_batch > receipt_batch_number)
        return LXP_ERR_NOT_YET_VALID;
    return status;
}

lxp_result lxp_programs_metering_schedule_current(
    const lxp_kernel *kernel, uint64_t batch_number,
    lx_programs_metering_schedule *schedule)
{
    lx_programs_metering_schedule active;
    lx_programs_metering_schedule historical;
    lx_programs_metering_schedule selected;
    uint8_t key[sizeof(metering_history_prefix) - 1U + 4U];
    uint32_t version;
    uint64_t prior_activation = 0U;
    bool selected_present = false;
    lxp_result status;
    if (batch_number == 0U) return LXP_ERR_NON_CANONICAL;
    status = kernel_record(kernel, metering_active_key,
                           sizeof(metering_active_key) - 1U, &active);
    if (status != LXP_OK) return status;
    if (active.version == 0U || kernel == NULL ||
        active.version > kernel->module_kv_count)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    for (version = 1U; version <= active.version; ++version) {
        history_key(version, key);
        status = kernel_record(kernel, key, sizeof(key), &historical);
        if (status != LXP_OK) return status;
        if (historical.version != version ||
            !schedule_lineage_valid(&historical) ||
            historical.activation_batch <= prior_activation)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        if (!lxp_program_metering_schedule_available(historical.version))
            return LXP_ERR_VERSION_UNSUPPORTED;
        prior_activation = historical.activation_batch;
        if (historical.activation_batch <= batch_number) {
            selected = historical;
            selected_present = true;
        }
    }
    if (!schedule_equal(&active, &historical))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (!selected_present) return LXP_ERR_NOT_YET_VALID;
    *schedule = selected;
    return LXP_OK;
}

lxp_result lxp_programs_metering_resolve_runtime(
    void *context, uint32_t recorded_version, uint64_t batch_number,
    lx_programs_metering_schedule *schedule)
{
    if (recorded_version == 0U)
        return lxp_programs_metering_schedule_current(
            (const lxp_kernel *)context, batch_number, schedule);
    return lxp_programs_metering_schedule_at(
        (const lxp_kernel *)context, recorded_version, batch_number, schedule);
}

static int manifest_order(uint16_t module_id, const uint8_t key[32],
                          const lxp_genesis_module_value *right)
{
    if (module_id < right->module_id) return -1;
    if (module_id > right->module_id) return 1;
    return memcmp(key, right->key, 32U);
}

static lxp_result manifest_insert(
    lxp_genesis_manifest *manifest, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length)
{
    uint8_t padded_key[32] = {0U};
    size_t location = 0U;
    if (manifest == NULL || key == NULL || key_length == 0U ||
        key_length > sizeof(padded_key) || value == NULL ||
        value_length == 0U || value_length > LXP_GENESIS_MODULE_VALUE_BYTES ||
        manifest->module_value_count == LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(padded_key, key, key_length);
    while (location < manifest->module_value_count &&
           manifest_order(LXP_MODULE_PROGRAMS, padded_key,
                          &manifest->module_values[location]) > 0)
        ++location;
    if (location < manifest->module_value_count &&
        manifest_order(LXP_MODULE_PROGRAMS, padded_key,
                       &manifest->module_values[location]) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memmove(&manifest->module_values[location + 1U],
                  &manifest->module_values[location],
                  (manifest->module_value_count - location) *
                      sizeof(manifest->module_values[0]));
    (void)memset(&manifest->module_values[location], 0,
                 sizeof(manifest->module_values[location]));
    manifest->module_values[location].module_id = LXP_MODULE_PROGRAMS;
    (void)memcpy(manifest->module_values[location].key, padded_key, 32U);
    (void)memcpy(manifest->module_values[location].value, value, value_length);
    manifest->module_values[location].value_length = value_length;
    ++manifest->module_value_count;
    return LXP_OK;
}

static bool manifest_contains(const lxp_genesis_manifest *manifest,
                              const uint8_t *key, size_t key_length)
{
    uint8_t padded_key[32] = {0U};
    size_t index;
    if (manifest == NULL || key == NULL || key_length > sizeof(padded_key))
        return false;
    (void)memcpy(padded_key, key, key_length);
    for (index = 0U; index < manifest->module_value_count; ++index)
        if (manifest->module_values[index].module_id == LXP_MODULE_PROGRAMS &&
            memcmp(manifest->module_values[index].key, padded_key, 32U) == 0)
            return true;
    return false;
}

lxp_result lxp_programs_metering_genesis_append(
    lxp_genesis_manifest *manifest,
    const lx_programs_metering_schedule *schedule)
{
    uint8_t encoded[PROGRAMS_METERING_RECORD_BYTES];
    uint8_t key[sizeof(metering_history_prefix) - 1U + 4U];
    uint8_t signer_digest[32];
    size_t original_count;
    lxp_result status;
    if (manifest == NULL || schedule == NULL || schedule->version != 1U ||
        schedule->activation_batch != 1U ||
        schedule->authority_kind != LX_PROGRAMS_METERING_AUTHORITY_GENESIS ||
        manifest->module_value_count > LXP_GENESIS_MAX_MODULE_VALUES - 2U ||
        lxp_ct_is_zero(manifest->signer_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_payload(manifest->signer_public_key, 32U,
                              signer_digest);
    if (status == LXP_OK && lxp_ct_memcmp(
            signer_digest, schedule->authority_digest, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = record_encode(schedule, encoded);
    if (status != LXP_OK) return status;
    history_key(schedule->version, key);
    if (manifest_contains(manifest, metering_active_key,
                          sizeof(metering_active_key) - 1U) ||
        manifest_contains(manifest, key, sizeof(key)))
        return LXP_ERR_SEQUENCE_REUSED;
    original_count = manifest->module_value_count;
    status = manifest_insert(manifest, metering_active_key,
                             sizeof(metering_active_key) - 1U,
                             encoded, sizeof(encoded));
    if (status == LXP_OK)
        status = manifest_insert(manifest, key, sizeof(key),
                                 encoded, sizeof(encoded));
    if (status != LXP_OK) manifest->module_value_count = original_count;
    return status;
}

lxp_result lxp_programs_metering_genesis_validate(
    const lxp_genesis_manifest *manifest)
{
    const lxp_genesis_module_value *active = NULL;
    const lxp_genesis_module_value *history = NULL;
    uint8_t history_v1_key[sizeof(metering_history_prefix) - 1U + 4U];
    uint8_t signer_digest[32];
    size_t index;
    lxp_result status;
    if (manifest == NULL || lxp_ct_is_zero(manifest->signer_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_payload(manifest->signer_public_key, 32U,
                              signer_digest);
    if (status != LXP_OK) return status;
    history_key(1U, history_v1_key);
    for (index = 0U; index < manifest->module_value_count; ++index) {
        const lxp_genesis_module_value *value = &manifest->module_values[index];
        lx_programs_metering_schedule decoded;
        bool is_active;
        bool is_history;
        if (value->module_id != LXP_MODULE_PROGRAMS) continue;
        is_active = memcmp(value->key, metering_active_key,
                           sizeof(metering_active_key) - 1U) == 0 &&
            lxp_ct_is_zero(value->key + sizeof(metering_active_key) - 1U,
                           32U - (sizeof(metering_active_key) - 1U));
        is_history = memcmp(value->key, history_v1_key,
                            sizeof(history_v1_key)) == 0 &&
            lxp_ct_is_zero(value->key + sizeof(history_v1_key),
                           32U - sizeof(history_v1_key));
        if (!is_active && !is_history) continue;
        if ((is_active && active != NULL) || (is_history && history != NULL))
            return LXP_ERR_SEQUENCE_REUSED;
        status = record_decode(value->value, value->value_length, &decoded);
        if (status != LXP_OK || decoded.version != 1U ||
            decoded.activation_batch != 1U ||
            decoded.authority_kind !=
                LX_PROGRAMS_METERING_AUTHORITY_GENESIS ||
            lxp_ct_memcmp(decoded.authority_digest,
                          signer_digest, 32U) != 0)
            return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
        if (is_active) active = value;
        else history = value;
    }
    if (active == NULL || history == NULL ||
        memcmp(active->value, history->value,
               PROGRAMS_METERING_RECORD_BYTES) != 0)
        return LXP_ERR_UNKNOWN_FIELD;
    return LXP_OK;
}

static int kernel_entry_order(uint16_t module_id, const uint8_t *key,
                              size_t key_length,
                              const lxp_module_kv_entry *right)
{
    size_t common;
    int order;
    if (module_id < right->module_id) return -1;
    if (module_id > right->module_id) return 1;
    common = key_length < right->key_length ? key_length : right->key_length;
    order = memcmp(key, right->key, common);
    if (order != 0) return order;
    return key_length < right->key_length ? -1 :
           key_length > right->key_length ? 1 : 0;
}

static lxp_result kernel_insert(
    lxp_kernel *kernel, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length)
{
    size_t location = 0U;
    if (kernel == NULL || key == NULL || key_length == 0U ||
        key_length > LXP_MODULE_MAX_KEY_BYTES || value == NULL ||
        value_length > LXP_MODULE_MAX_VALUE_BYTES ||
        kernel->module_kv_count == LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    while (location < kernel->module_kv_count &&
           kernel_entry_order(LXP_MODULE_PROGRAMS, key, key_length,
                              &kernel->module_kv[location]) > 0)
        ++location;
    if (location < kernel->module_kv_count &&
        kernel_entry_order(LXP_MODULE_PROGRAMS, key, key_length,
                           &kernel->module_kv[location]) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memmove(&kernel->module_kv[location + 1U],
                  &kernel->module_kv[location],
                  (kernel->module_kv_count - location) *
                      sizeof(kernel->module_kv[0]));
    (void)memset(&kernel->module_kv[location], 0,
                 sizeof(kernel->module_kv[location]));
    kernel->module_kv[location].module_id = LXP_MODULE_PROGRAMS;
    kernel->module_kv[location].key_length = (uint16_t)key_length;
    kernel->module_kv[location].value_length = (uint32_t)value_length;
    (void)memcpy(kernel->module_kv[location].key, key, key_length);
    (void)memcpy(kernel->module_kv[location].value, value, value_length);
    ++kernel->module_kv_count;
    return LXP_OK;
}

lxp_result lxp_programs_metering_genesis_materialize(
    const lxp_genesis_manifest *manifest, lxp_kernel *kernel)
{
    size_t index;
    const lxp_genesis_module_value *active = NULL;
    const lxp_genesis_module_value *history = NULL;
    uint8_t history_v1_key[sizeof(metering_history_prefix) - 1U + 4U];
    uint8_t signer_digest[32];
    lxp_result status = LXP_OK;
    if (manifest == NULL || kernel == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_metering_genesis_validate(manifest);
    if (status != LXP_OK) return status;
    status = lxp_hash_payload(manifest->signer_public_key, 32U,
                              signer_digest);
    if (kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV - 2U)
        return LXP_ERR_LENGTH_LIMIT;
    history_key(1U, history_v1_key);
    for (index = 0U; status == LXP_OK &&
         index < manifest->module_value_count; ++index) {
        const lxp_genesis_module_value *value = &manifest->module_values[index];
        lx_programs_metering_schedule decoded;
        if (value->module_id != LXP_MODULE_PROGRAMS ||
            value->value_length != PROGRAMS_METERING_RECORD_BYTES)
            continue;
        if (memcmp(value->key, metering_active_key,
                   sizeof(metering_active_key) - 1U) == 0 &&
            lxp_ct_is_zero(value->key + sizeof(metering_active_key) - 1U,
                           32U - (sizeof(metering_active_key) - 1U)))
            active = value;
        else if (memcmp(value->key, history_v1_key,
                        sizeof(history_v1_key)) == 0 &&
                 lxp_ct_is_zero(value->key + sizeof(history_v1_key),
                                32U - sizeof(history_v1_key)))
            history = value;
        else continue;
        status = record_decode(value->value, value->value_length, &decoded);
        if (status == LXP_OK &&
            (decoded.version != 1U || decoded.activation_batch != 1U ||
             decoded.authority_kind !=
                 LX_PROGRAMS_METERING_AUTHORITY_GENESIS ||
             lxp_ct_memcmp(decoded.authority_digest,
                           signer_digest, 32U) != 0))
            status = LXP_ERR_NON_CANONICAL;
    }
    if (status == LXP_OK && (active == NULL || history == NULL ||
        memcmp(active->value, history->value,
               PROGRAMS_METERING_RECORD_BYTES) != 0))
        status = LXP_ERR_UNKNOWN_FIELD;
    if (status == LXP_OK) {
        lx_programs_metering_schedule existing_active;
        lx_programs_metering_schedule existing_history;
        lx_programs_metering_schedule expected;
        lxp_result active_status;
        lxp_result history_status;
        active_status = kernel_record(
            kernel, metering_active_key, sizeof(metering_active_key) - 1U,
            &existing_active);
        history_status = kernel_record(
            kernel, history_v1_key, sizeof(history_v1_key),
            &existing_history);
        if (active_status == LXP_OK && history_status == LXP_OK) {
            if (record_decode(active->value, active->value_length,
                              &expected) != LXP_OK ||
                !schedule_equal(&existing_active, &existing_history) ||
                !schedule_equal(&existing_active, &expected))
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            else status = LXP_OK;
        } else if (active_status == LXP_ERR_VERSION_UNSUPPORTED &&
                   history_status == LXP_ERR_VERSION_UNSUPPORTED) {
            status = LXP_OK;
        } else {
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        }
        if (status == LXP_OK && active_status == LXP_OK) return LXP_OK;
    }
    if (status == LXP_OK) {
        lx_programs_metering_schedule existing;
        status = kernel_record(kernel, metering_active_key,
                               sizeof(metering_active_key) - 1U, &existing);
        if (status == LXP_OK) status = LXP_ERR_SEQUENCE_REUSED;
        else if (status == LXP_ERR_VERSION_UNSUPPORTED) status = LXP_OK;
    }
    if (status == LXP_OK) {
        lx_programs_metering_schedule existing;
        status = kernel_record(kernel, history_v1_key,
                               sizeof(history_v1_key), &existing);
        if (status == LXP_OK) status = LXP_ERR_SEQUENCE_REUSED;
        else if (status == LXP_ERR_VERSION_UNSUPPORTED) status = LXP_OK;
    }
    if (status == LXP_OK)
        status = kernel_insert(kernel, metering_active_key,
                               sizeof(metering_active_key) - 1U,
                               active->value, active->value_length);
    if (status == LXP_OK)
        status = kernel_insert(kernel, history_v1_key,
                               sizeof(history_v1_key),
                               history->value, history->value_length);
    return status;
}

lxp_result lxp_programs_metering_genesis_project(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    lxp_kernel *kernel)
{
    lxp_result status;
    if (manifest == NULL || arena == NULL || kernel == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_genesis_verify_signature(manifest, arena);
    return status == LXP_OK ?
        lxp_programs_metering_genesis_materialize(manifest, kernel) : status;
}
