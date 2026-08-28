#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include "artifact.h"

#include <limits.h>
#include <string.h>

enum {
    PROGRAM_KEY_PREFIX_LENGTH = 8,
    PROGRAM_RECORD_LENGTH = 71,
    PROGRAM_POLICY_IMMUTABLE = 0,
    PROGRAM_POLICY_AUTHORITY = 1,
    PROGRAM_DEPLOY_FIXED_LENGTH = 72,
    PROGRAM_UPGRADE_FIXED_LENGTH = 76,
    PROGRAM_EVENT_DEPLOYED = 1,
    PROGRAM_EVENT_UPGRADED = 2
};

static const uint8_t program_prefix[PROGRAM_KEY_PREFIX_LENGTH] = {
    'p', 'r', 'o', 'g', 'r', 'a', 'm', 0
};

typedef struct programs_lifecycle_decoded {
    uint16_t ordinal;
    uint16_t abi_version;
    uint8_t policy_or_flags;
    uint8_t program_id[32];
    uint8_t authority[32];
    uint8_t old_hash[32];
    uint8_t new_hash[32];
    const uint8_t *migration_hook;
    uint16_t migration_hook_length;
    const uint8_t *wasm;
    uint32_t wasm_length;
} programs_lifecycle_decoded;

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static void write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void program_key(const uint8_t program_id[32], uint8_t key[40])
{
    (void)memcpy(key, program_prefix, PROGRAM_KEY_PREFIX_LENGTH);
    (void)memcpy(key + PROGRAM_KEY_PREFIX_LENGTH, program_id, 32U);
}

static lxp_result validate_wasm(const programs_lifecycle_decoded *value)
{
    static const uint8_t wasm_header[8] = {
        0x00U, 0x61U, 0x73U, 0x6dU, 0x01U, 0x00U, 0x00U, 0x00U
    };
    uint8_t digest[32];
    lxp_result status;
    if (value->wasm_length < sizeof(wasm_header) ||
        memcmp(value->wasm, wasm_header, sizeof(wasm_header)) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_sha256(value->wasm, value->wasm_length, digest);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(digest, value->new_hash, sizeof(digest)) == 0 ?
        LXP_OK : LXP_ERR_PAYLOAD_HASH_MISMATCH;
}

static lxp_result decode_deploy(const uint8_t *payload, size_t length,
                                programs_lifecycle_decoded *value)
{
    uint32_t wasm_length;
    if (length < 32U + PROGRAM_DEPLOY_FIXED_LENGTH) return LXP_ERR_TRUNCATED;
    value->abi_version = read_u16(payload + 32U);
    value->policy_or_flags = payload[34U];
    if (payload[35U] != 0U || value->policy_or_flags > PROGRAM_POLICY_AUTHORITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value->authority, payload + 36U, 32U);
    (void)memcpy(value->new_hash, payload + 68U, 32U);
    wasm_length = read_u32(payload + 100U);
    if (wasm_length == 0U || (size_t)wasm_length != length - 104U)
        return LXP_ERR_NON_CANONICAL;
    if ((value->policy_or_flags == PROGRAM_POLICY_IMMUTABLE) !=
        lxp_ct_is_zero(value->authority, 32U)) return LXP_ERR_NON_CANONICAL;
    value->wasm = payload + 104U;
    value->wasm_length = wasm_length;
    return LXP_OK;
}

static lxp_result decode_upgrade(const uint8_t *payload, size_t length,
                                 programs_lifecycle_decoded *value)
{
    uint16_t hook_length;
    uint32_t wasm_length;
    size_t variable_length;
    if (length < 32U + PROGRAM_UPGRADE_FIXED_LENGTH) return LXP_ERR_TRUNCATED;
    value->abi_version = read_u16(payload + 32U);
    value->policy_or_flags = payload[34U];
    if (payload[35U] != 0U || (value->policy_or_flags & 0xfeU) != 0U)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value->old_hash, payload + 36U, 32U);
    (void)memcpy(value->new_hash, payload + 68U, 32U);
    hook_length = read_u16(payload + 100U);
    wasm_length = read_u32(payload + 102U);
    variable_length = (size_t)hook_length + (size_t)wasm_length;
    if (wasm_length == 0U || variable_length != length - 106U ||
        ((value->policy_or_flags & 1U) == 0U) != (hook_length == 0U))
        return LXP_ERR_NON_CANONICAL;
    value->migration_hook = payload + 106U;
    value->migration_hook_length = hook_length;
    value->wasm = payload + 106U + hook_length;
    value->wasm_length = wasm_length;
    return LXP_OK;
}

lxp_result lxp_programs_lifecycle_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                         const uint8_t *payload, size_t length,
                                         void **decoded)
{
    programs_lifecycle_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL ||
        (ordinal != 1U && ordinal != 2U) || length < 32U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_lifecycle_decoded), &memory);
    if (status != LXP_OK) return status;
    value = (programs_lifecycle_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    (void)memcpy(value->program_id, payload, 32U);
    if (lxp_ct_is_zero(value->program_id, 32U)) return LXP_ERR_NON_CANONICAL;
    status = ordinal == 1U ? decode_deploy(payload, length, value) :
                            decode_upgrade(payload, length, value);
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

lxp_result lxp_programs_lifecycle_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_lifecycle_decoded *value =
        (const programs_lifecycle_decoded *)decoded;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (value->abi_version != 1U) return LXP_ERR_VERSION_UNSUPPORTED;
    status = validate_wasm(value);
    if (status != LXP_OK) return status;
    return lxp_ctx_charge_gas(ctx, (uint64_t)value->wasm_length + 1U);
}

static lxp_result execute_deploy(lxp_module_ctx *ctx,
                                 const lxp_authority_resolved *authority,
                                 const programs_lifecycle_decoded *value)
{
    const lxp_module_registration *registration;
    uint8_t key[40];
    uint8_t record[PROGRAM_RECORD_LENGTH];
    const uint8_t *existing;
    size_t existing_length;
    lxp_result status;
    program_key(value->program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &existing, &existing_length);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    record[0] = value->policy_or_flags;
    (void)memcpy(record + 1U, value->authority, 32U);
    (void)memcpy(record + 33U, value->new_hash, 32U);
    write_u16(record + 65U, value->abi_version);
    write_u32(record + 67U, 1U);
    status = lxp_programs_artifact_store(ctx, value->program_id,
                                         value->new_hash, value->wasm,
                                         value->wasm_length);
    if (status != LXP_OK) return status;
    status = lxp_kernel_module_by_id(ctx->kernel, LXP_MODULE_PROGRAMS,
                                     ctx->epoch, &registration);
    if (status != LXP_OK) return status;
    if (registration->abi_version == LX_PROGRAMS_ACCOUNT_ABI_VERSION) {
        status = lxp_programs_account_owner_bind(
            ctx, value->program_id, authority->principal);
        if (status != LXP_OK) return status;
    } else if (registration->abi_version != LX_PROGRAMS_ABI_VERSION) {
        return LXP_ERR_VERSION_UNSUPPORTED;
    }
    status = lxp_ctx_kv_put(ctx, key, sizeof(key), record, sizeof(record));
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, PROGRAM_EVENT_DEPLOYED,
                              value->new_hash, 32U);
}

static lxp_result execute_upgrade(lxp_module_ctx *ctx,
                                  const lxp_authority_resolved *authority,
                                  const programs_lifecycle_decoded *value)
{
    uint8_t key[40];
    uint8_t record[PROGRAM_RECORD_LENGTH];
    uint8_t event[64];
    const uint8_t *current;
    size_t current_length;
    uint32_t version;
    lx_programs_metering_schedule metering_schedule;
    lxp_result status;
    program_key(value->program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &current, &current_length);
    if (status != LXP_OK) return LXP_ERR_UNKNOWN_FIELD;
    if (current_length != sizeof(record)) return LXP_FATAL_INVARIANT;
    status = lxp_programs_program_active(ctx, value->program_id);
    if (status != LXP_OK) return status;
    if (current[0] != PROGRAM_POLICY_AUTHORITY)
        return LXP_ERR_AUTH_SCOPE;
    if (lxp_ct_memcmp(current + 1U, authority->principal, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    if (lxp_ct_memcmp(current + 33U, value->old_hash, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if ((value->policy_or_flags & 1U) != 0U) {
        status = lxp_programs_metering_schedule_current(
            ctx->kernel, lxp_ctx_batch_number(ctx), &metering_schedule);
        if (status != LXP_OK) return status;
        status = layerx_programs_migration_execute_activity(
            (uint64_t)(uintptr_t)value, value->wasm_length,
            value->migration_hook_length, value->abi_version,
            metering_schedule.version,
            metering_schedule.coefficients[0],
            metering_schedule.coefficients[1],
            metering_schedule.coefficients[2],
            metering_schedule.coefficients[3],
            metering_schedule.coefficients[4],
            metering_schedule.coefficients[5],
            metering_schedule.coefficients[6],
            metering_schedule.coefficients[7],
            metering_schedule.coefficients[8],
            read_u64(value->new_hash), read_u64(value->new_hash + 8U),
            read_u64(value->new_hash + 16U), read_u64(value->new_hash + 24U));
        if (status != LXP_OK) return status;
    }
    version = read_u32(current + 67U);
    if (version == UINT32_MAX) return LXP_ERR_OVERFLOW;
    (void)memcpy(record, current, sizeof(record));
    (void)memcpy(record + 33U, value->new_hash, 32U);
    write_u16(record + 65U, value->abi_version);
    write_u32(record + 67U, version + 1U);
    status = lxp_programs_artifact_store(ctx, value->program_id,
                                         value->new_hash, value->wasm,
                                         value->wasm_length);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_put(ctx, key, sizeof(key), record, sizeof(record));
    if (status != LXP_OK) return status;
    (void)memcpy(event, value->old_hash, 32U);
    (void)memcpy(event + 32U, value->new_hash, 32U);
    status = lxp_ctx_emit_event(ctx, PROGRAM_EVENT_UPGRADED, event, sizeof(event));
    if (status != LXP_OK) return status;
    return layerx_programs_module_cache_invalidate_upgrade(
        read_u64(value->old_hash), read_u64(value->old_hash + 8U),
        read_u64(value->old_hash + 16U), read_u64(value->old_hash + 24U));
}

lxp_result layerx_programs_migration_activity_byte(uint64_t token,
                                                   uint16_t section,
                                                   uint32_t offset)
{
    const programs_lifecycle_decoded *value =
        (const programs_lifecycle_decoded *)(uintptr_t)token;
    const uint8_t *bytes;
    size_t length;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    if (section == 0U) {
        bytes = value->wasm;
        length = value->wasm_length;
    } else if (section == 1U) {
        bytes = value->migration_hook;
        length = value->migration_hook_length;
    } else {
        return LXP_ERR_UNKNOWN_FIELD;
    }
    if (bytes == NULL || (size_t)offset >= length)
        return LXP_ERR_TRUNCATED;
    return (lxp_result)bytes[offset];
}

lxp_result lxp_programs_lifecycle_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    const programs_lifecycle_decoded *value =
        (const programs_lifecycle_decoded *)decoded;
    (void)activity;
    (void)effects;
    if (ctx == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return value->ordinal == 1U ? execute_deploy(ctx, authority, value) :
                                 execute_upgrade(ctx, authority, value);
}

void lxp_programs_lifecycle_release(lxp_module_ctx *ctx, void *decoded)
{
    (void)ctx;
    (void)decoded;
}
