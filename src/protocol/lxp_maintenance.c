#include "layerx/lxp_maintenance.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t receipt_domain[] = "LXP/batch-maintenance/v1";
static const uint8_t effects_domain[] = "LXP/batch-maintenance-effects/v1";

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
        ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void write_integer(uint8_t *bytes, uint64_t value, size_t width)
{
    for (size_t i = 0U; i < width; ++i)
        bytes[i] = (uint8_t)(value >> ((width - i - 1U) * 8U));
}

bool lxp_batch_maintenance_is_envelope(lxp_byte_span encoded)
{
    return encoded.bytes != NULL && encoded.length >= sizeof(receipt_domain) &&
        memcmp(encoded.bytes, receipt_domain, sizeof(receipt_domain)) == 0;
}

lxp_result lxp_batch_maintenance_effects_validate(lxp_byte_span effects)
{
    size_t offset = sizeof(effects_domain);
    uint16_t previous_module = 0U;
    uint16_t count;
    if (effects.bytes == NULL || effects.length < offset + 2U ||
        effects.length > LXP_BATCH_MAINTENANCE_MAX_BYTES ||
        memcmp(effects.bytes, effects_domain, offset) != 0)
        return LXP_ERR_NON_CANONICAL;
    count = read_u16(effects.bytes + offset);
    offset += 2U;
    if (count > LXP_KERNEL_MAX_MODULE_KV) return LXP_ERR_LENGTH_LIMIT;
    for (uint16_t frame = 0U; frame < count; ++frame) {
        uint16_t module;
        uint16_t effect_count;
        if (effects.length - offset < 8U) return LXP_ERR_TRUNCATED;
        module = read_u16(effects.bytes + offset);
        if ((module != LXP_MODULE_ESCROW && module != LXP_MODULE_BUDGET &&
             module != LXP_MODULE_SERVICE) || module < previous_module ||
            read_u32(effects.bytes + offset + 2U) != 1U)
            return LXP_ERR_NON_CANONICAL;
        previous_module = module;
        effect_count = read_u16(effects.bytes + offset + 6U);
        offset += 8U;
        if (effect_count == 0U || effect_count > LXP_MAX_EFFECTS)
            return LXP_ERR_LENGTH_LIMIT;
        for (uint16_t index = 0U; index < effect_count; ++index) {
            const uint8_t *effect;
            uint8_t kind;
            uint8_t monetary;
            uint16_t length;
            if (effects.length - offset < 40U) return LXP_ERR_TRUNCATED;
            effect = effects.bytes + offset;
            kind = effect[4U];
            monetary = effect[5U];
            length = read_u16(effect + 38U);
            if (read_u16(effect) != index || kind < LXP_EFFECT_STATE ||
                kind > LXP_EFFECT_EVENT || monetary > 1U ||
                (monetary != 0U && kind != LXP_EFFECT_TRANSFER) ||
                ((kind == LXP_EFFECT_TRANSFER) ==
                 lxp_ct_is_zero(effect + 6U, 32U)) || length > 256U)
                return LXP_ERR_NON_CANONICAL;
            offset += 40U;
            if (length > effects.length - offset) return LXP_ERR_TRUNCATED;
            if (kind == LXP_EFFECT_STATE) {
                const uint8_t *body = effects.bytes + offset;
                uint16_t key_length;
                if (read_u16(effect + 2U) != 0U || length < 36U)
                    return LXP_ERR_NON_CANONICAL;
                key_length = read_u16(body);
                if (key_length == 0U || key_length > 64U ||
                    length != 35U + key_length || body[2U + key_length] > 1U)
                    return LXP_ERR_NON_CANONICAL;
                if (body[2U + key_length] != 0U) {
                    uint8_t empty_digest[32];
                    lxp_result status = lxp_hash_sha256(NULL, 0U, empty_digest);
                    if (status != LXP_OK) return status;
                    if (lxp_ct_memcmp(body + 3U + key_length, empty_digest, 32U) != 0)
                        return LXP_ERR_NON_CANONICAL;
                }
            }
            offset += length;
        }
    }
    return offset == effects.length ? LXP_OK : LXP_ERR_TRAILING_BYTES;
}

static lxp_result validate(const lxp_batch_maintenance *record)
{
    lxp_programs_occupancy_receipt occupancy;
    lxp_result status;
    if (record == NULL) return LXP_ERR_NON_CANONICAL;
    if (record->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (record->epoch == 0U || record->batch_number == 0U || record->timestamp_ms == 0U ||
        record->global_sequence == UINT64_MAX || record->parameter_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_occupancy_receipt_decode(record->occupancy.bytes,
        record->occupancy.length, &occupancy);
    if (status != LXP_OK) return status;
    if (occupancy.batch_number != record->batch_number ||
        occupancy.global_sequence != record->global_sequence ||
        occupancy.parameter_version != record->parameter_version)
        return LXP_ERR_CONTEXT_MISMATCH;
    return lxp_batch_maintenance_effects_validate(record->effects);
}

lxp_result lxp_batch_maintenance_encode(const lxp_batch_maintenance *record,
    lxp_arena *arena, lxp_byte_span *encoded)
{
    size_t offset = sizeof(receipt_domain);
    size_t length;
    void *memory;
    uint8_t *bytes;
    lxp_result status;
    if (arena == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    status = validate(record);
    if (status != LXP_OK) return status;
    if (record->occupancy.length > LXP_BATCH_MAINTENANCE_MAX_BYTES ||
        record->effects.length > LXP_BATCH_MAINTENANCE_MAX_BYTES - record->occupancy.length)
        return LXP_ERR_LENGTH_LIMIT;
    length = offset + 46U + record->occupancy.length + record->effects.length;
    if (length > LXP_BATCH_MAINTENANCE_MAX_BYTES) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_arena_alloc(arena, length, 1U, &memory);
    if (status != LXP_OK) return status;
    bytes = memory;
    (void)memcpy(bytes, receipt_domain, offset);
    write_integer(bytes + offset, record->protocol_version, 2U); offset += 2U;
    write_integer(bytes + offset, record->epoch, 8U); offset += 8U;
    write_integer(bytes + offset, record->batch_number, 8U); offset += 8U;
    write_integer(bytes + offset, record->timestamp_ms, 8U); offset += 8U;
    write_integer(bytes + offset, record->global_sequence, 8U); offset += 8U;
    write_integer(bytes + offset, record->parameter_version, 4U); offset += 4U;
    write_integer(bytes + offset, record->occupancy.length, 4U); offset += 4U;
    (void)memcpy(bytes + offset, record->occupancy.bytes, record->occupancy.length);
    offset += record->occupancy.length;
    write_integer(bytes + offset, record->effects.length, 4U); offset += 4U;
    (void)memcpy(bytes + offset, record->effects.bytes, record->effects.length);
    *encoded = (lxp_byte_span){bytes, length};
    return LXP_OK;
}

lxp_result lxp_batch_maintenance_decode(const uint8_t *bytes, size_t length,
    lxp_batch_maintenance *record)
{
    size_t offset = sizeof(receipt_domain);
    uint32_t span_length;
    if (record == NULL || !lxp_batch_maintenance_is_envelope((lxp_byte_span){bytes, length}) ||
        length < offset + 46U || length > LXP_BATCH_MAINTENANCE_MAX_BYTES)
        return LXP_ERR_NON_CANONICAL;
    record->protocol_version = read_u16(bytes + offset); offset += 2U;
    record->epoch = read_u64(bytes + offset); offset += 8U;
    record->batch_number = read_u64(bytes + offset); offset += 8U;
    record->timestamp_ms = read_u64(bytes + offset); offset += 8U;
    record->global_sequence = read_u64(bytes + offset); offset += 8U;
    record->parameter_version = read_u32(bytes + offset); offset += 4U;
    span_length = read_u32(bytes + offset); offset += 4U;
    if (span_length > length - offset - 4U) return LXP_ERR_TRUNCATED;
    record->occupancy = (lxp_byte_span){bytes + offset, span_length};
    offset += span_length;
    span_length = read_u32(bytes + offset); offset += 4U;
    if (span_length != length - offset) return LXP_ERR_NON_CANONICAL;
    record->effects = (lxp_byte_span){bytes + offset, span_length};
    return validate(record);
}

lxp_result lxp_batch_maintenance_occupancy_decode(const uint8_t *bytes,
    size_t length, lxp_programs_occupancy_receipt *record)
{
    if (lxp_batch_maintenance_is_envelope((lxp_byte_span){bytes, length})) {
        lxp_batch_maintenance envelope;
        lxp_result status = lxp_batch_maintenance_decode(bytes, length, &envelope);
        if (status != LXP_OK) return status;
        return lxp_programs_occupancy_receipt_decode(envelope.occupancy.bytes,
            envelope.occupancy.length, record);
    }
    return lxp_programs_occupancy_receipt_decode(bytes, length, record);
}

lxp_result lxp_batch_maintenance_events(lxp_byte_span encoded,
    const lxp_batch_header *header, lxp_byte_span *events)
{
    lxp_programs_occupancy_receipt occupancy;
    lxp_result status;
    if (events == NULL) return LXP_ERR_NON_CANONICAL;
    *events = (lxp_byte_span){NULL, 0U};
    if (encoded.length == 0U) return LXP_OK;
    status = lxp_batch_maintenance_occupancy_decode(encoded.bytes, encoded.length, &occupancy);
    if (status != LXP_OK) return status;
    if (lxp_batch_maintenance_is_envelope(encoded)) {
        lxp_batch_maintenance envelope;
        status = lxp_batch_maintenance_decode(encoded.bytes, encoded.length, &envelope);
        if (status != LXP_OK) return status;
        if (header != NULL && (envelope.protocol_version != header->protocol_version ||
            envelope.epoch != header->epoch || envelope.batch_number != header->batch_number ||
            envelope.timestamp_ms != header->timestamp_ms ||
            envelope.global_sequence != header->last_sequence))
            return LXP_ERR_CONTEXT_MISMATCH;
        *events = envelope.effects;
    }
    return LXP_OK;
}
