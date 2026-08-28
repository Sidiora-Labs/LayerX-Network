#include "sandbox.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <limits.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

enum {
    SANDBOX_VERSION = 1,
    SANDBOX_EXECUTE = 1,
    SANDBOX_FUND = 2,
    SANDBOX_ACTIVATE = 3,
    SANDBOX_FIXED_BYTES = 236,
    SANDBOX_USAGE_EVENT = 0x0909,
    SANDBOX_USAGE_CHUNK_BYTES = 204,
    SANDBOX_USAGE_RECEIPT_CAPACITY = 4096,
    SANDBOX_MAX_GUEST_EFFECTS = 64,
    SANDBOX_USAGE_EFFECT_RESERVE = SANDBOX_MAX_GUEST_EFFECTS + 1 +
        (SANDBOX_USAGE_RECEIPT_CAPACITY + SANDBOX_USAGE_CHUNK_BYTES - 1) /
        SANDBOX_USAGE_CHUNK_BYTES
};

typedef struct programs_sandbox_activity {
    lxp_module_ctx *ctx;
    uint8_t operation;
    uint8_t lease_id[32];
    uint8_t tenant[32];
    uint8_t host_program[32];
    uint8_t funded_amount[16];
    uint8_t expiry[8];
    uint8_t funding_transfer_root[32];
    const uint8_t *current_lease;
    uint32_t current_lease_length;
    uint64_t expected_sequence;
    uint8_t expected_lease_digest[32];
    uint8_t escrow_account[32];
    uint8_t asset[32];
    uint8_t fee_destination[32];
    uint32_t fee_schedule_version;
    uint64_t fee_schedule[7];
    const uint8_t *call_payload;
    uint32_t call_length;
    void *call;
    const uint8_t *lifecycle[3];
    uint32_t lifecycle_length[3];
    void *transfer;
    lxp_module_savepoint guest_savepoint;
    uint8_t guest_sealed;
    struct {
        uint8_t *bytes;
        uint32_t length;
        uint32_t written;
        uint8_t begun;
        uint8_t applied;
        uint32_t capacity;
    } staged[3];
    struct {
        uint8_t occupancy[16];
        uint8_t occupancy_fee[16];
        uint8_t transfer_root[32];
        uint8_t usage[64];
        uint8_t usage_written[8];
        uint8_t *receipt;
        uint32_t receipt_length;
        uint32_t receipt_written;
        uint8_t begun;
        uint8_t published;
        uint32_t capacity;
    } usage_result;
    uint8_t host_reserved;
    size_t reserved_effect_frontier;
    size_t sealed_guest_effect_frontier;
    uint64_t reservation_token;
} programs_sandbox_activity;

extern int32_t layerx_programs_sandbox_admit_host(
    uint64_t token, uint64_t observed_batch,
    uint64_t maximum_fee_hi, uint64_t maximum_fee_lo);
extern void layerx_programs_sandbox_cancel_host(uint64_t token);

static _Thread_local programs_sandbox_activity *active_sandbox;
static lxp_result sandbox_state_key(programs_sandbox_activity *value,
                                    uint16_t kind, uint8_t key[34]);
static programs_sandbox_activity *sandbox_for_call(uint64_t call_token);
static void write_u64(uint8_t bytes[8], uint64_t value);
static lxp_result emit_usage_receipt(programs_sandbox_activity *value);

lxp_result layerx_programs_call_sandbox_admit(
    uint64_t call_token, uint64_t observed_batch,
    uint64_t maximum_fee_hi, uint64_t maximum_fee_lo)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    size_t kind;
    if (value == NULL || value->operation != SANDBOX_EXECUTE ||
        value->host_reserved || value->ctx->effects == NULL ||
        value->ctx->effects->count > LXP_MAX_EFFECTS ||
        SANDBOX_USAGE_EFFECT_RESERVE >
            LXP_MAX_EFFECTS - value->ctx->effects->count)
        return LXP_ERR_NON_CANONICAL;
    if (layerx_programs_sandbox_admit_host(call_token, observed_batch,
            maximum_fee_hi, maximum_fee_lo) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    /* Bind cleanup before any subsequent C allocation can refuse admission. */
    value->reservation_token = call_token;
    for (kind = 0U; kind < 3U; ++kind) {
        value->staged[kind].bytes = malloc(LXP_MODULE_MAX_VALUE_BYTES);
        if (value->staged[kind].bytes == NULL) break;
        value->staged[kind].capacity = LXP_MODULE_MAX_VALUE_BYTES;
    }
    if (kind == 3U) {
        value->usage_result.receipt = malloc(SANDBOX_USAGE_RECEIPT_CAPACITY);
        if (value->usage_result.receipt != NULL) {
            value->usage_result.capacity = SANDBOX_USAGE_RECEIPT_CAPACITY;
            value->reserved_effect_frontier = value->ctx->effects->count;
            if (lxp_module_staged_reserve(value->ctx, 3U) == LXP_OK) {
                value->host_reserved = 1U;
                return LXP_OK;
            }
        }
        free(value->usage_result.receipt);
        value->usage_result.receipt = NULL;
        value->usage_result.capacity = 0U;
    }
    while (kind != 0U) {
        --kind;
        free(value->staged[kind].bytes);
        value->staged[kind].bytes = NULL;
        value->staged[kind].capacity = 0U;
    }
    layerx_programs_sandbox_cancel_host(value->reservation_token);
    value->reservation_token = 0U;
    return LXP_ERR_ARENA_EXHAUSTED;
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | (uint64_t)bytes[index];
    return value;
}

lxp_result lxp_programs_sandbox_decode(lxp_module_ctx *ctx,
                                       const uint8_t *payload,
                                       size_t payload_length, void **decoded)
{
    programs_sandbox_activity *value;
    void *allocation;
    size_t cursor = 4U;
    size_t index;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL ||
        payload_length < 4U)
        return LXP_ERR_TRUNCATED;
    if (payload[0] != SANDBOX_VERSION || payload[1] < SANDBOX_EXECUTE ||
        payload[1] > SANDBOX_ACTIVATE ||
        payload[2] != 0U || payload[3] != 0U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_sandbox_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (programs_sandbox_activity *)allocation;
    (void)memset(value, 0, sizeof(*value));
    value->ctx = ctx;
    value->operation = payload[1];
    (void)memcpy(value->lease_id, payload + cursor, 32U); cursor += 32U;
    if (value->operation != SANDBOX_EXECUTE) {
        size_t section;
        if (value->operation == SANDBOX_FUND) {
            if (payload_length < 248U) return LXP_ERR_TRUNCATED;
            cursor = 36U;
            (void)memcpy(value->tenant, payload + 36U, 32U);
            (void)memcpy(value->host_program, payload + 68U, 32U);
            value->expected_sequence = 0U;
            (void)memcpy(value->escrow_account, payload + 100U, 32U);
            (void)memcpy(value->asset, payload + 132U, 32U);
            (void)memcpy(value->funded_amount, payload + 164U, 16U);
            (void)memcpy(value->expiry, payload + 180U, 8U);
            value->fee_schedule_version = read_u32(payload + 188U);
            for (section = 0U; section < 7U; ++section)
                value->fee_schedule[section] = read_u64(payload + 192U + section * 8U);
            cursor = 248U;
            if (payload_length - cursor < 4U) return LXP_ERR_TRUNCATED;
            value->lifecycle_length[0] = read_u32(payload + cursor);
            cursor += 4U;
            if (value->lifecycle_length[0] == 0U ||
                (size_t)value->lifecycle_length[0] > payload_length - cursor)
                return LXP_ERR_NON_CANONICAL;
            value->lifecycle[0] = payload + cursor;
            cursor += value->lifecycle_length[0];
            if (payload_length - cursor < 4U) return LXP_ERR_TRUNCATED;
            value->call_length = read_u32(payload + cursor); cursor += 4U;
            if ((size_t)value->call_length != payload_length - cursor ||
                value->call_length == 0U)
                return LXP_ERR_NON_CANONICAL;
            value->call_payload = payload + cursor;
            status = lxp_programs_transfer_decode(ctx, value->call_payload,
                                                  value->call_length,
                                                  &value->transfer);
        } else {
            if (payload_length < 76U) return LXP_ERR_TRUNCATED;
            (void)memcpy(value->expected_lease_digest, payload + 36U, 32U);
            value->expected_sequence = read_u64(payload + 68U);
            if (payload_length != 76U)
                return LXP_ERR_NON_CANONICAL;
            status = LXP_OK;
        }
        if (status != LXP_OK) return status;
        *decoded = value;
        return LXP_OK;
    }
    if (payload_length < SANDBOX_FIXED_BYTES) return LXP_ERR_TRUNCATED;
    value->expected_sequence = read_u64(payload + cursor); cursor += 8U;
    (void)memcpy(value->expected_lease_digest, payload + cursor, 32U); cursor += 32U;
    (void)memcpy(value->escrow_account, payload + cursor, 32U); cursor += 32U;
    (void)memcpy(value->asset, payload + cursor, 32U); cursor += 32U;
    (void)memcpy(value->fee_destination, payload + cursor, 32U); cursor += 32U;
    value->fee_schedule_version = read_u32(payload + cursor); cursor += 4U;
    for (index = 0U; index < 7U; ++index) {
        value->fee_schedule[index] = read_u64(payload + cursor);
        cursor += 8U;
    }
    value->call_length = read_u32(payload + cursor); cursor += 4U;
    if ((size_t)value->call_length != payload_length - cursor ||
        value->call_length == 0U ||
        lxp_ct_is_zero(value->lease_id, 32U) ||
        lxp_ct_is_zero(value->expected_lease_digest, 32U) ||
        lxp_ct_is_zero(value->escrow_account, 32U) ||
        lxp_ct_is_zero(value->asset, 32U) ||
        lxp_ct_is_zero(value->fee_destination, 32U) ||
        value->fee_schedule_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    value->call_payload = payload + cursor;
    status = lxp_programs_call_decode(ctx, value->call_payload,
                                      value->call_length, &value->call);
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

static void inner_activity(const lxp_activity *activity, lxp_activity *inner)
{
    *inner = *activity;
    inner->activity_type = LX_PROGRAMS_CALL;
}

lxp_result lxp_programs_sandbox_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_sandbox_activity *value = decoded;
    lxp_activity inner;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_SANDBOX)
        return LXP_ERR_NON_CANONICAL;
    if (value->operation != SANDBOX_EXECUTE) {
        if (value->operation == SANDBOX_FUND) {
            if (value->transfer == NULL) return LXP_ERR_NON_CANONICAL;
            inner_activity(activity, &inner);
            inner.activity_type = LX_PROGRAMS_TRANSFER;
            inner.payload = (lxp_byte_span){ value->call_payload,
                                             value->call_length };
            return lxp_programs_transfer_validate(ctx, &inner, authority,
                                                   value->transfer);
        }
        return value->operation == SANDBOX_ACTIVATE ? LXP_OK :
                                                     LXP_ERR_NON_CANONICAL;
    }
    if (value->call == NULL) return LXP_ERR_NON_CANONICAL;
    inner_activity(activity, &inner);
    return lxp_programs_call_validate(ctx, &inner, authority, value->call);
}

lxp_result lxp_programs_sandbox_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    programs_sandbox_activity *value = (programs_sandbox_activity *)decoded;
    lxp_activity inner;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_SANDBOX || value->ctx != ctx ||
        active_sandbox != NULL)
        return LXP_ERR_NON_CANONICAL;
    if (value->operation != SANDBOX_EXECUTE) {
        size_t kind;
        const uint8_t *current;
        size_t current_length;
        uint8_t key[34];
        active_sandbox = value;
        status = LXP_OK;
        if (value->operation == SANDBOX_FUND) {
            for (kind = 0U; status == LXP_OK && kind < 3U; ++kind) {
                status = sandbox_state_key(value, (uint16_t)kind, key);
                if (status == LXP_OK)
                    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &current,
                                            &current_length);
                if (status == LXP_ERR_UNKNOWN_FIELD) status = LXP_OK;
                else if (status == LXP_OK) status = LXP_ERR_IDEMPOTENT_REPLAY;
            }
        } else {
            status = sandbox_state_key(value, 0U, key);
            if (status == LXP_OK)
                status = lxp_ctx_kv_get(ctx, key, sizeof(key), &current,
                                        &current_length);
            if (status == LXP_OK) {
                if (current_length > UINT32_MAX) status = LXP_ERR_LENGTH_LIMIT;
                else {
                    value->current_lease = current;
                    value->current_lease_length = (uint32_t)current_length;
                }
            }
        }
        if (status == LXP_OK)
            status = layerx_programs_sandbox_lifecycle_validate(
                (uint64_t)(uintptr_t)value, value->operation);
        if (status == LXP_OK && value->operation == SANDBOX_FUND)
            for (kind = 0U; kind < 3U; ++kind)
                if (value->staged[kind].begun || value->staged[kind].applied)
                    status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK && value->operation == SANDBOX_FUND) {
            inner_activity(activity, &inner);
            inner.activity_type = LX_PROGRAMS_TRANSFER;
            inner.payload = (lxp_byte_span){ value->call_payload,
                                             value->call_length };
            status = lxp_programs_transfer_execute_root(
                ctx, &inner, authority, value->transfer, effects,
                value->funding_transfer_root);
        }
        if (status == LXP_OK && value->operation == SANDBOX_FUND)
            status = layerx_programs_sandbox_lifecycle_validate(
                (uint64_t)(uintptr_t)value, 0x82U);
        if (status == LXP_OK)
            for (kind = 0U; kind < (value->operation == SANDBOX_FUND ? 3U : 1U);
                 ++kind)
                if (!value->staged[kind].applied)
                    status = LXP_FATAL_INVARIANT;
        active_sandbox = NULL;
        return status;
    }
    if (value->call == NULL) return LXP_ERR_NON_CANONICAL;
    inner_activity(activity, &inner);
    active_sandbox = value;
    status = lxp_module_savepoint_begin(ctx, &value->guest_savepoint);
    if (status == LXP_OK)
        status = lxp_programs_call_execute(ctx, &inner, authority, value->call,
                                           effects);
    if (!lxp_result_is_fatal(status) && value->usage_result.published) {
        size_t kind;
        for (kind = 0U; kind < 3U; ++kind)
            if (!value->staged[kind].applied) status = LXP_FATAL_INVARIANT;
    }
    if (status != LXP_OK && !lxp_result_is_fatal(status) &&
        value->usage_result.published) {
        const lxp_program_outcome *outcome = lxp_ctx_program_outcome(ctx);
        if (outcome != NULL && outcome->present &&
            outcome->result_code == status &&
            outcome->terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS)
            status = LXP_OK;
    }
    if (status == LXP_OK && !value->usage_result.published)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) status = emit_usage_receipt(value);
    if (value->guest_savepoint.active) status = LXP_FATAL_INVARIANT;
    {
        size_t kind;
        for (kind = 0U; kind < 3U; ++kind) {
            free(value->staged[kind].bytes);
            value->staged[kind].bytes = NULL;
        }
    }
    free(value->usage_result.receipt);
    value->usage_result.receipt = NULL;
    if (value->reservation_token != 0U)
        layerx_programs_sandbox_cancel_host(value->reservation_token);
    active_sandbox = NULL;
    return status;
}

static lxp_result emit_usage_receipt(programs_sandbox_activity *value)
{
    uint8_t body[256];
    uint32_t offset = 0U;
    uint32_t chunks;
    uint32_t index;
    lxp_result status = LXP_OK;
    if (value == NULL || !value->guest_sealed ||
        !value->usage_result.published || value->usage_result.receipt == NULL)
        return LXP_FATAL_INVARIANT;
    chunks = (value->usage_result.receipt_length +
              SANDBOX_USAGE_CHUNK_BYTES - 1U) / SANDBOX_USAGE_CHUNK_BYTES;
    if (chunks == 0U || chunks > UINT16_MAX || value->ctx->effects == NULL ||
        chunks + SANDBOX_MAX_GUEST_EFFECTS + 1U > SANDBOX_USAGE_EFFECT_RESERVE ||
        value->ctx->effects->count != value->sealed_guest_effect_frontier + 1U ||
        chunks > LXP_MAX_EFFECTS - value->ctx->effects->count)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; status == LXP_OK && index < chunks; ++index) {
        uint32_t remaining = value->usage_result.receipt_length - offset;
        uint16_t length = (uint16_t)(remaining < SANDBOX_USAGE_CHUNK_BYTES ?
                          remaining : SANDBOX_USAGE_CHUNK_BYTES);
        (void)memset(body, 0, sizeof(body));
        (void)memcpy(body, "LXUR", 4U);
        (void)memcpy(body + 4U, value->ctx->activity_id, 32U);
        write_u64(body + 36U, value->expected_sequence);
        body[44] = (uint8_t)(index >> 8U); body[45] = (uint8_t)index;
        body[46] = (uint8_t)(chunks >> 8U); body[47] = (uint8_t)chunks;
        body[48] = (uint8_t)(value->usage_result.receipt_length >> 24U);
        body[49] = (uint8_t)(value->usage_result.receipt_length >> 16U);
        body[50] = (uint8_t)(value->usage_result.receipt_length >> 8U);
        body[51] = (uint8_t)value->usage_result.receipt_length;
        (void)memcpy(body + 52U, value->usage_result.receipt + offset, length);
        status = lxp_ctx_emit_event(value->ctx, SANDBOX_USAGE_EVENT,
                                    body, 52U + length);
        offset += length;
    }
    return status;
}

lxp_result layerx_programs_call_sandbox_guest_seal(uint64_t token,
                                                    uint8_t success)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    lxp_result status;
    if (value == NULL || value->guest_sealed || success > 1U ||
        !value->guest_savepoint.active)
        return LXP_ERR_NON_CANONICAL;
    if (success != 0U &&
        (value->ctx->effects->count < value->reserved_effect_frontier ||
         value->ctx->effects->count - value->reserved_effect_frontier >
             SANDBOX_MAX_GUEST_EFFECTS))
        return LXP_ERR_LENGTH_LIMIT;
    status = success != 0U ?
        lxp_module_savepoint_accept(value->ctx, &value->guest_savepoint) :
        lxp_module_savepoint_discard(value->ctx, &value->guest_savepoint);
    if (status == LXP_OK) {
        value->sealed_guest_effect_frontier = value->ctx->effects->count;
        status = lxp_module_staged_release(value->ctx, 3U);
    }
    if (status == LXP_OK) value->guest_sealed = 1U;
    return status;
}

lxp_result layerx_programs_sandbox_lifecycle_length(
    uint64_t token, uint16_t section)
{
    programs_sandbox_activity *value = (programs_sandbox_activity *)(uintptr_t)token;
    if (value == NULL || value != active_sandbox || section > 13U)
        return LXP_ERR_NON_CANONICAL;
    if (section <= 2U)
        return value->lifecycle_length[section] > (uint32_t)INT32_MAX ?
            LXP_ERR_LENGTH_LIMIT : (lxp_result)value->lifecycle_length[section];
    if (section == 13U)
        return value->current_lease_length > (uint32_t)INT32_MAX ?
            LXP_ERR_LENGTH_LIMIT : (lxp_result)value->current_lease_length;
    return section == 5U ? 16 :
           section == 6U || section == 9U || section == 10U || section == 12U ?
           8 : 32;
}

lxp_result layerx_programs_sandbox_lifecycle_byte(
    uint64_t token, uint16_t section, uint32_t offset)
{
    programs_sandbox_activity *value = (programs_sandbox_activity *)(uintptr_t)token;
    const uint8_t *bytes;
    uint32_t length;
    uint8_t scalar[8];
    if (value == NULL || value != active_sandbox || section > 13U)
        return LXP_ERR_NON_CANONICAL;
    if (section <= 2U) {
        bytes = value->lifecycle[section];
        length = value->lifecycle_length[section];
    } else if (section == 3U) { bytes = value->tenant; length = 32U; }
    else if (section == 4U) { bytes = value->host_program; length = 32U; }
    else if (section == 5U) { bytes = value->funded_amount; length = 16U; }
    else if (section == 6U) { bytes = value->expiry; length = 8U; }
    else if (section == 7U) { bytes = value->funding_transfer_root; length = 32U; }
    else if (section == 8U) { bytes = value->ctx->activity_id; length = 32U; }
    else if (section == 9U) {
        write_u64(scalar, lxp_ctx_global_sequence(value->ctx)); bytes = scalar; length = 8U;
    } else if (section == 10U) {
        write_u64(scalar, value->ctx->batch_number); bytes = scalar; length = 8U;
    } else if (section == 11U) {
        bytes = value->expected_lease_digest; length = 32U;
    } else if (section == 12U) {
        write_u64(scalar, value->expected_sequence); bytes = scalar; length = 8U;
    } else { bytes = value->current_lease; length = value->current_lease_length; }
    return offset < length ? (lxp_result)bytes[offset] : LXP_ERR_NON_CANONICAL;
}

static programs_sandbox_activity *sandbox_for_call(uint64_t call_token)
{
    return active_sandbox != NULL &&
           ((uint64_t)(uintptr_t)active_sandbox->call == call_token ||
            (uint64_t)(uintptr_t)active_sandbox == call_token) ?
           active_sandbox : NULL;
}

static void write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

lxp_result layerx_programs_call_sandbox_usage_result_begin(
    uint64_t token, uint64_t occupancy_hi, uint64_t occupancy_lo,
    uint64_t occupancy_fee_hi, uint64_t occupancy_fee_lo,
    uint64_t transfer0, uint64_t transfer1, uint64_t transfer2,
    uint64_t transfer3, uint32_t receipt_length)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    if (value == NULL || value->operation != SANDBOX_EXECUTE ||
        !value->host_reserved || value->usage_result.receipt == NULL ||
        value->usage_result.begun || receipt_length == 0U ||
        receipt_length > value->usage_result.capacity)
        return LXP_ERR_NON_CANONICAL;
    write_u64(value->usage_result.occupancy, occupancy_hi);
    write_u64(value->usage_result.occupancy + 8U, occupancy_lo);
    write_u64(value->usage_result.occupancy_fee, occupancy_fee_hi);
    write_u64(value->usage_result.occupancy_fee + 8U, occupancy_fee_lo);
    write_u64(value->usage_result.transfer_root, transfer0);
    write_u64(value->usage_result.transfer_root + 8U, transfer1);
    write_u64(value->usage_result.transfer_root + 16U, transfer2);
    write_u64(value->usage_result.transfer_root + 24U, transfer3);
    value->usage_result.receipt_length = receipt_length;
    value->usage_result.begun = 1U;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_usage_result_receipt_byte(
    uint64_t token, uint32_t offset, uint8_t byte)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    if (value == NULL || !value->usage_result.begun ||
        value->usage_result.published ||
        offset != value->usage_result.receipt_written ||
        offset >= value->usage_result.receipt_length)
        return LXP_ERR_NON_CANONICAL;
    value->usage_result.receipt[offset] = byte;
    ++value->usage_result.receipt_written;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_usage_result_field(
    uint64_t token, uint16_t index, uint64_t field)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    if (value == NULL || !value->usage_result.begun ||
        value->usage_result.published || index >= 8U ||
        value->usage_result.usage_written[index])
        return LXP_ERR_NON_CANONICAL;
    write_u64(value->usage_result.usage + (size_t)index * 8U, field);
    value->usage_result.usage_written[index] = 1U;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_usage_result_publish(uint64_t token)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    if (value == NULL || !value->usage_result.begun ||
        value->usage_result.published ||
        value->usage_result.receipt_written != value->usage_result.receipt_length ||
        memcmp(value->usage_result.usage_written,
               (const uint8_t[8]){1U,1U,1U,1U,1U,1U,1U,1U}, 8U) != 0)
        return LXP_ERR_NON_CANONICAL;
    value->usage_result.published = 1U;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_usage_result_length(
    uint64_t token, uint16_t section)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    if (value == NULL || !value->usage_result.published || section > 4U)
        return LXP_ERR_NON_CANONICAL;
    return section < 2U ? 16 : section == 2U ? 32 : section == 4U ? 64 :
           (lxp_result)value->usage_result.receipt_length;
}

lxp_result layerx_programs_call_sandbox_usage_result_byte(
    uint64_t token, uint16_t section, uint32_t offset)
{
    programs_sandbox_activity *value = sandbox_for_call(token);
    const uint8_t *bytes;
    uint32_t length;
    if (value == NULL || !value->usage_result.published || section > 4U)
        return LXP_ERR_NON_CANONICAL;
    bytes = section == 0U ? value->usage_result.occupancy :
            section == 1U ? value->usage_result.occupancy_fee :
            section == 2U ? value->usage_result.transfer_root :
            section == 4U ? value->usage_result.usage :
                            value->usage_result.receipt;
    length = section < 2U ? 16U : section == 2U ? 32U : section == 4U ? 64U :
             value->usage_result.receipt_length;
    return offset < length ? (lxp_result)bytes[offset] : LXP_ERR_NON_CANONICAL;
}

lxp_result layerx_programs_call_sandbox_context(uint64_t call_token)
{
    return sandbox_for_call(call_token) == NULL ? LXP_ERR_UNKNOWN_FIELD : LXP_OK;
}

lxp_result layerx_programs_call_sandbox_context_byte(
    uint64_t call_token, uint16_t section, uint32_t offset)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    const uint8_t *bytes;
    if (value == NULL || section > LX_PROGRAMS_SANDBOX_CONTEXT_EXPECTED_LEASE_DIGEST ||
        offset >= 32U)
        return LXP_ERR_NON_CANONICAL;
    bytes = section == LX_PROGRAMS_SANDBOX_CONTEXT_LEASE_ID ? value->lease_id :
        section == LX_PROGRAMS_SANDBOX_CONTEXT_ESCROW_ACCOUNT ? value->escrow_account :
        section == LX_PROGRAMS_SANDBOX_CONTEXT_ASSET ? value->asset :
        section == LX_PROGRAMS_SANDBOX_CONTEXT_FEE_DESTINATION ? value->fee_destination :
        value->expected_lease_digest;
    return (lxp_result)bytes[offset];
}

lxp_result layerx_programs_call_sandbox_expected_sequence(
    uint64_t call_token, uint64_t expected_sequence)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    return value != NULL && value->expected_sequence == expected_sequence ?
        LXP_OK : LXP_ERR_NON_CANONICAL;
}

lxp_result layerx_programs_call_sandbox_fee_schedule(
    uint64_t call_token, uint32_t version, uint64_t cpu,
    uint64_t memory_byte, uint64_t storage_read_byte,
    uint64_t storage_write_byte, uint64_t output_value,
    uint64_t output_byte, uint64_t occupancy_byte_batch)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    const uint64_t supplied[7] = { cpu, memory_byte, storage_read_byte,
        storage_write_byte, output_value, output_byte, occupancy_byte_batch };
    return value != NULL && value->fee_schedule_version == version &&
        memcmp(value->fee_schedule, supplied, sizeof(supplied)) == 0 ?
        LXP_OK : LXP_ERR_NON_CANONICAL;
}

static lxp_result sandbox_state_key(programs_sandbox_activity *value,
                                    uint16_t kind, uint8_t key[34])
{
    if (value == NULL || kind > 2U || key == NULL)
        return LXP_ERR_NON_CANONICAL;
    key[0] = (uint8_t)'s'; key[1] = (uint8_t)kind;
    (void)memcpy(key + 2U, value->lease_id, 32U);
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_state_length(uint64_t call_token,
                                                      uint16_t kind)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    const uint8_t *bytes;
    size_t length;
    uint8_t key[34];
    lxp_result status = sandbox_state_key(value, kind, key);
    if (status == LXP_OK)
        status = lxp_ctx_kv_get(value->ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    if (length > (size_t)INT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    return (lxp_result)length;
}

lxp_result layerx_programs_call_sandbox_state_byte(
    uint64_t call_token, uint16_t kind, uint32_t offset)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    const uint8_t *bytes;
    size_t length;
    uint8_t key[34];
    lxp_result status = sandbox_state_key(value, kind, key);
    if (status == LXP_OK)
        status = lxp_ctx_kv_get(value->ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return (size_t)offset < length ? (lxp_result)bytes[offset] :
                                    LXP_ERR_NON_CANONICAL;
}

lxp_result layerx_programs_call_sandbox_state_stage_begin(
    uint64_t call_token, uint16_t kind, uint32_t length)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    if (value == NULL || kind > 2U || !value->host_reserved ||
        value->staged[kind].bytes == NULL || length == 0U ||
        length > value->staged[kind].capacity || value->staged[kind].begun)
        return LXP_ERR_NON_CANONICAL;
    value->staged[kind].length = length;
    value->staged[kind].begun = 1U;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_state_stage_byte(
    uint64_t call_token, uint16_t kind, uint32_t offset, uint8_t byte)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    if (value == NULL || kind > 2U || !value->staged[kind].begun ||
        value->staged[kind].applied ||
        offset != value->staged[kind].written ||
        offset >= value->staged[kind].length)
        return LXP_ERR_NON_CANONICAL;
    value->staged[kind].bytes[offset] = byte;
    ++value->staged[kind].written;
    return LXP_OK;
}

lxp_result layerx_programs_call_sandbox_state_stage_apply(
    uint64_t call_token, uint16_t kind)
{
    programs_sandbox_activity *value = sandbox_for_call(call_token);
    uint8_t key[34];
    lxp_result status;
    if (value == NULL || kind > 2U || !value->staged[kind].begun ||
        value->staged[kind].applied ||
        value->staged[kind].written != value->staged[kind].length)
        return LXP_ERR_NON_CANONICAL;
    status = sandbox_state_key(value, kind, key);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(value->ctx, key, sizeof(key),
                                value->staged[kind].bytes,
                                value->staged[kind].length);
    if (status == LXP_OK) {
        value->staged[kind].applied = 1U;
    }
    return status;
}
