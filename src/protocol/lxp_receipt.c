#include "layerx/lxp_receipt.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_module.h"

#include <openssl/evp.h>
#include <string.h>

enum {
    LXP_RECEIPT_STRUCTURE_TAG = 0x5201,
    LXP_PROGRAM_OUTCOME_TAG_V1 = 0x50524731,
    LXP_PROGRAM_OUTCOME_TAG_V2 = 0x50524732,
    LXP_PROGRAM_OUTCOME_TAG_V3 = 0x50524733
};

static bool valid_program_terminal(uint8_t terminal)
{
    return terminal == LXP_PROGRAM_TERMINAL_SUCCESS ||
           terminal == LXP_PROGRAM_TERMINAL_FAILURE ||
           terminal == LXP_PROGRAM_TERMINAL_RESOURCE;
}

bool lxp_program_metering_schedule_available(uint32_t schedule_version)
{
    return schedule_version == LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
}

lxp_result lxp_program_outcome_validate(const lxp_program_outcome *outcome)
{
    if (outcome == NULL || !outcome->present ||
        !valid_program_terminal(outcome->terminal_kind) ||
        outcome->runtime_version == 0U || outcome->abi_version == 0U ||
        outcome->fee_schedule_version == 0U ||
        outcome->metering_schedule_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_program_metering_schedule_available(
            outcome->metering_schedule_version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if ((outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS &&
         outcome->result_code != LXP_OK) ||
        (outcome->terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS &&
         (outcome->result_code == LXP_OK ||
          lxp_result_is_fatal(outcome->result_code))))
        return LXP_FATAL_INVARIANT;
    if (lxp_ct_is_zero(outcome->terminal_payload_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (outcome->encoding_version != 1U && outcome->encoding_version != 2U &&
        outcome->encoding_version != 3U)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (outcome->encoding_version < 3U &&
        outcome->metering_schedule_version !=
            LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (outcome->encoding_version == 2U &&
        outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS) {
        if (lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) ||
            lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U))
            return LXP_ERR_NON_CANONICAL;
    } else if (outcome->encoding_version >= 2U &&
               outcome->terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS &&
               (!lxp_u128_is_zero(outcome->occupancy_byte_batches) ||
                !lxp_u128_is_zero(outcome->occupancy_fee_units) ||
                !lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) ||
                !lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U) ||
                !lxp_ct_is_zero(outcome->occupancy_transfer_root, 32U))) {
        return LXP_FATAL_INVARIANT;
    } else if (outcome->encoding_version == 3U &&
               (lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) !=
                    lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U))) {
        return LXP_ERR_NON_CANONICAL;
    } else if (outcome->encoding_version == 1U &&
               (!lxp_u128_is_zero(outcome->occupancy_byte_batches) ||
                !lxp_u128_is_zero(outcome->occupancy_fee_units) ||
                !lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) ||
                !lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U) ||
                !lxp_ct_is_zero(outcome->occupancy_transfer_root, 32U))) {
        return LXP_ERR_VERSION_UNSUPPORTED;
    }
    if (outcome->terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS &&
        !lxp_ct_is_zero(outcome->transfer_root, 32U)) {
        return LXP_FATAL_INVARIANT;
    }
    return LXP_OK;
}

lxp_result lxp_program_outcome_validate_for_protocol(
    const lxp_program_outcome *outcome, uint16_t protocol_version)
{
    lxp_result status = lxp_program_outcome_validate(outcome);
    if (status != LXP_OK) return status;
    if (!lxp_protocol_version_supported(protocol_version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if ((protocol_version == LXP_PROTOCOL_VERSION_OCCUPANCY &&
         outcome->encoding_version != 2U &&
         outcome->encoding_version != 3U) ||
        (protocol_version == LXP_PROTOCOL_VERSION_LEGACY &&
         outcome->encoding_version != 1U &&
         outcome->encoding_version != 3U))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (outcome->encoding_version == 3U &&
        protocol_version == LXP_PROTOCOL_VERSION_OCCUPANCY &&
        outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS &&
        (lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) ||
         lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U)))
        return LXP_ERR_NON_CANONICAL;
    if (outcome->encoding_version == 3U &&
        protocol_version == LXP_PROTOCOL_VERSION_LEGACY &&
        (!lxp_u128_is_zero(outcome->occupancy_byte_batches) ||
         !lxp_u128_is_zero(outcome->occupancy_fee_units) ||
         !lxp_ct_is_zero(outcome->occupancy_asset_id, 32U) ||
         !lxp_ct_is_zero(outcome->occupancy_evidence_digest, 32U) ||
         !lxp_ct_is_zero(outcome->occupancy_transfer_root, 32U)))
        return LXP_ERR_VERSION_UNSUPPORTED;
    return LXP_OK;
}

static int effect_compare(const lxp_effect *left, const lxp_effect *right)
{
    if (left->module_id != right->module_id)
        return left->module_id < right->module_id ? -1 : 1;
    if (left->ordinal != right->ordinal)
        return left->ordinal < right->ordinal ? -1 : 1;
    return 0;
}

lxp_result lxp_effect_buffer_init(lxp_effect_buffer *buffer)
{
    if (buffer == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(buffer, 0, sizeof(*buffer));
    return LXP_OK;
}

lxp_result lxp_effect_buffer_add(lxp_effect_buffer *buffer,
                                 const lxp_effect *effect)
{
    if (buffer == NULL || effect == NULL || effect->module_id == 0U ||
        effect->body_length > sizeof(effect->body))
        return LXP_ERR_NON_CANONICAL;
    if (effect->monetary && effect->kind != LXP_EFFECT_TRANSFER)
        return LXP_FATAL_INVARIANT;
    if (effect->kind == LXP_EFFECT_TRANSFER &&
        lxp_ct_is_zero(effect->transfer_set_root, 32U))
        return LXP_FATAL_INVARIANT;
    if (buffer->count == LXP_MAX_EFFECTS) return LXP_ERR_LENGTH_LIMIT;
    if (buffer->count != 0U &&
        effect_compare(&buffer->effects[buffer->count - 1U], effect) >= 0)
        return LXP_ERR_UNSORTED_SEQUENCE;
    buffer->effects[buffer->count++] = *effect;
    return LXP_OK;
}

static lxp_result effect_encode(lxp_codec_writer *writer,
                                const lxp_effect *effect)
{
    lxp_result status = lxp_codec_write_u16(writer, effect->module_id);
    if (status == LXP_OK) status = lxp_codec_write_u16(writer, effect->ordinal);
    if (status == LXP_OK)
        status = lxp_codec_write_u16(writer, effect->event_type);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, (uint8_t)effect->kind);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, effect->monetary ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, effect->transfer_set_root, 32U,
                                       32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, effect->body,
                                       effect->body_length, 256U);
    return status;
}

static lxp_result copy_exact(lxp_codec_reader *reader, uint8_t *destination,
                             size_t length)
{
    lxp_byte_span span;
    lxp_result status = lxp_codec_read_bytes(reader, &span, (uint32_t)length);
    if (status != LXP_OK || span.length != length) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(destination, span.bytes, length);
    return LXP_OK;
}

static lxp_result effect_decode(lxp_codec_reader *reader, lxp_effect *effect)
{
    lxp_byte_span span;
    uint8_t kind;
    uint8_t monetary;
    lxp_result status;
    (void)memset(effect, 0, sizeof(*effect));
    status = lxp_codec_read_u16(reader, &effect->module_id);
    if (status == LXP_OK) status = lxp_codec_read_u16(reader, &effect->ordinal);
    if (status == LXP_OK) status = lxp_codec_read_u16(reader, &effect->event_type);
    if (status == LXP_OK) status = lxp_codec_read_u8(reader, &kind);
    if (status == LXP_OK) status = lxp_codec_read_u8(reader, &monetary);
    if (status == LXP_OK)
        status = copy_exact(reader, effect->transfer_set_root, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_bytes(reader, &span, sizeof(effect->body));
    if (status != LXP_OK || kind < (uint8_t)LXP_EFFECT_STATE ||
        kind > (uint8_t)LXP_EFFECT_EVENT || monetary > 1U ||
        span.length > sizeof(effect->body))
        return LXP_ERR_NON_CANONICAL;
    effect->kind = (lxp_effect_kind)kind;
    effect->monetary = monetary != 0U;
    effect->body_length = (uint16_t)span.length;
    (void)memcpy(effect->body, span.bytes, span.length);
    return LXP_OK;
}

lxp_result lxp_effect_event_root(const lxp_effect_buffer *buffer,
                                 lxp_arena *arena, uint8_t root[32])
{
    uint8_t hashes[LXP_MAX_EFFECTS][32];
    size_t count = 0U;
    size_t i;
    lxp_result status;
    if (buffer == NULL || arena == NULL || root == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < buffer->count; ++i) {
        uint8_t encoded[2U + 2U + 2U + 1U + 2U + 256U];
        size_t length;
        const lxp_effect *effect = &buffer->effects[i];
        if (effect->kind != LXP_EFFECT_EVENT) continue;
        encoded[0] = (uint8_t)(effect->module_id >> 8U);
        encoded[1] = (uint8_t)effect->module_id;
        encoded[2] = (uint8_t)(effect->ordinal >> 8U);
        encoded[3] = (uint8_t)effect->ordinal;
        encoded[4] = (uint8_t)(effect->event_type >> 8U);
        encoded[5] = (uint8_t)effect->event_type;
        encoded[6] = (uint8_t)effect->kind;
        encoded[7] = (uint8_t)(effect->body_length >> 8U);
        encoded[8] = (uint8_t)effect->body_length;
        (void)memcpy(encoded + 9U, effect->body, effect->body_length);
        length = 9U + effect->body_length;
        status = lxp_merkle_leaf_hash(encoded, length, hashes[count]);
        if (status != LXP_OK) return status;
        ++count;
    }
    return lxp_merkle_build((const uint8_t (*)[32])hashes, count, arena, root);
}

lxp_result lxp_receipt_build(lxp_receipt *receipt,
                             const uint8_t activity_id[32],
                             uint64_t global_sequence,
                             const uint8_t previous_state_root[32],
                             const uint8_t resulting_state_root[32],
                             const uint8_t activity_root[32],
                             lxp_result result_code,
                             const lxp_effect_buffer *effects,
                             lxp_u128 fee_charged,
                             const uint8_t batch_id[32], uint16_t module_id,
                             uint32_t module_version,
                             uint32_t parameter_version)
{
    uint8_t activity_id_copy[32];
    uint8_t previous_copy[32];
    uint8_t resulting_copy[32];
    uint8_t activity_root_copy[32];
    uint8_t batch_id_copy[32];
    uint16_t protocol_version;
    lxp_effect_buffer effects_copy;
    if (receipt == NULL || activity_id == NULL || previous_state_root == NULL ||
        resulting_state_root == NULL || activity_root == NULL ||
        effects == NULL || batch_id == NULL || module_id == 0U ||
        module_version == 0U) return LXP_ERR_NON_CANONICAL;
    protocol_version = lxp_protocol_version_supported(
        receipt->protocol_version) ? receipt->protocol_version :
                                     (uint16_t)LXP_PROTOCOL_VERSION;
    (void)memcpy(activity_id_copy, activity_id, 32U);
    (void)memcpy(previous_copy, previous_state_root, 32U);
    (void)memcpy(resulting_copy, resulting_state_root, 32U);
    (void)memcpy(activity_root_copy, activity_root, 32U);
    (void)memcpy(batch_id_copy, batch_id, 32U);
    effects_copy = *effects;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->protocol_version = protocol_version;
    (void)memcpy(receipt->activity_id, activity_id_copy, 32U);
    receipt->global_sequence = global_sequence;
    (void)memcpy(receipt->previous_state_root, previous_copy, 32U);
    (void)memcpy(receipt->resulting_state_root, resulting_copy, 32U);
    (void)memcpy(receipt->activity_root, activity_root_copy, 32U);
    receipt->result_code = result_code;
    receipt->effects = effects_copy;
    receipt->fee_charged = fee_charged;
    (void)memcpy(receipt->batch_id, batch_id_copy, 32U);
    receipt->module_id = module_id;
    receipt->module_version = module_version;
    receipt->parameter_version = parameter_version;
    return LXP_OK;
}

lxp_result lxp_receipt_bind_program_outcome(
    lxp_receipt *receipt, const lxp_program_outcome *outcome)
{
    lxp_result status;
    if (receipt == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_program_outcome_validate_for_protocol(
        outcome, receipt->protocol_version);
    if (status != LXP_OK) return status;
    if (receipt->module_id != LXP_MODULE_PROGRAMS ||
        receipt->program_outcome.present)
        return LXP_FATAL_INVARIANT;
    if (outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS) {
        if (receipt->result_code != outcome->result_code ||
            lxp_ct_memcmp(receipt->transfer_set_root,
                          outcome->transfer_root, 32U) != 0)
            return LXP_FATAL_INVARIANT;
    } else if (receipt->result_code != outcome->result_code ||
               !lxp_ct_is_zero(receipt->transfer_set_root, 32U) ||
               !lxp_ct_is_zero(outcome->transfer_root, 32U)) {
        return LXP_FATAL_INVARIANT;
    }
    receipt->program_outcome = *outcome;
    return LXP_OK;
}

static lxp_result program_outcome_encode(lxp_codec_writer *writer,
                                         const lxp_program_outcome *outcome)
{
    lxp_result status = lxp_program_outcome_validate(outcome);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(
            writer, outcome->encoding_version == 3U ?
                LXP_PROGRAM_OUTCOME_TAG_V3 :
                (outcome->encoding_version == 2U ?
                    LXP_PROGRAM_OUTCOME_TAG_V2 : LXP_PROGRAM_OUTCOME_TAG_V1));
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, outcome->terminal_kind);
    if (status == LXP_OK)
        status = lxp_codec_write_i32(writer, outcome->result_code);
    if (status == LXP_OK)
        status = lxp_codec_write_u16(writer, outcome->runtime_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u16(writer, outcome->abi_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(writer, outcome->fee_schedule_version);
    if (status == LXP_OK && outcome->encoding_version == 3U)
        status = lxp_codec_write_u32(
            writer, outcome->metering_schedule_version);
    if (status == LXP_OK) status = lxp_codec_write_u64(writer, outcome->cpu_fuel);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, outcome->memory_bytes);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, outcome->storage_read_bytes);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, outcome->storage_write_bytes);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(writer, outcome->output_values);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, outcome->output_bytes);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_write_u128(writer,
                                      outcome->occupancy_byte_batches);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_write_u128(writer, outcome->occupancy_fee_units);
    if (status == LXP_OK && outcome->encoding_version >= 2U) {
        size_t index;
        for (index = 0U; index < 7U && status == LXP_OK; ++index)
            status = lxp_codec_write_u64(
                writer, outcome->fee_schedule_prices[index]);
    }
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_write_bytes(
            writer, outcome->occupancy_asset_id, 32U, 32U);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_write_bytes(
            writer, outcome->occupancy_evidence_digest, 32U, 32U);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_write_bytes(
            writer, outcome->occupancy_transfer_root, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(writer, outcome->fee_units);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, outcome->call_graph_root, 32U,
                                       32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, outcome->terminal_payload_root,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, outcome->transfer_root, 32U,
                                       32U);
    return status;
}

static lxp_result program_outcome_decode(lxp_codec_reader *reader,
                                         lxp_program_outcome *outcome)
{
    uint32_t tag;
    size_t index;
    lxp_result status;
    (void)memset(outcome, 0, sizeof(*outcome));
    status = lxp_codec_read_u32(reader, &tag);
    if (status != LXP_OK ||
        (tag != LXP_PROGRAM_OUTCOME_TAG_V1 &&
         tag != LXP_PROGRAM_OUTCOME_TAG_V2 &&
         tag != LXP_PROGRAM_OUTCOME_TAG_V3))
        return LXP_ERR_NON_CANONICAL;
    outcome->present = true;
    outcome->encoding_version = tag == LXP_PROGRAM_OUTCOME_TAG_V3 ? 3U :
        (tag == LXP_PROGRAM_OUTCOME_TAG_V2 ? 2U : 1U);
    outcome->metering_schedule_version =
        LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
    status = lxp_codec_read_u8(reader, &outcome->terminal_kind);
    if (status == LXP_OK)
        status = lxp_codec_read_i32(reader, &outcome->result_code);
    if (status == LXP_OK)
        status = lxp_codec_read_u16(reader, &outcome->runtime_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u16(reader, &outcome->abi_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u32(reader, &outcome->fee_schedule_version);
    if (status == LXP_OK && outcome->encoding_version == 3U)
        status = lxp_codec_read_u32(
            reader, &outcome->metering_schedule_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &outcome->cpu_fuel);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &outcome->memory_bytes);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &outcome->storage_read_bytes);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &outcome->storage_write_bytes);
    if (status == LXP_OK)
        status = lxp_codec_read_u32(reader, &outcome->output_values);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &outcome->output_bytes);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_read_u128(reader,
                                     &outcome->occupancy_byte_batches);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = lxp_codec_read_u128(reader, &outcome->occupancy_fee_units);
    for (index = 0U; status == LXP_OK &&
         outcome->encoding_version >= 2U && index < 7U; ++index)
        status = lxp_codec_read_u64(
            reader, &outcome->fee_schedule_prices[index]);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = copy_exact(reader, outcome->occupancy_asset_id, 32U);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = copy_exact(reader, outcome->occupancy_evidence_digest, 32U);
    if (status == LXP_OK && outcome->encoding_version >= 2U)
        status = copy_exact(reader, outcome->occupancy_transfer_root, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u128(reader, &outcome->fee_units);
    if (status == LXP_OK)
        status = copy_exact(reader, outcome->call_graph_root, 32U);
    if (status == LXP_OK)
        status = copy_exact(reader, outcome->terminal_payload_root, 32U);
    if (status == LXP_OK)
        status = copy_exact(reader, outcome->transfer_root, 32U);
    if (status != LXP_OK) return status;
    return lxp_program_outcome_validate(outcome);
}

lxp_result lxp_receipt_encode(const lxp_receipt *receipt,
                              bool include_signature, lxp_arena *arena,
                              lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    lxp_result status;
    size_t i;
    if (receipt == NULL || arena == NULL || encoded == NULL ||
        receipt->effects.count > LXP_MAX_EFFECTS)
        return LXP_ERR_NON_CANONICAL;
    if (receipt->program_outcome.present &&
        receipt->module_id != LXP_MODULE_PROGRAMS)
        return LXP_FATAL_INVARIANT;
    if (receipt->program_outcome.present) {
        status = lxp_program_outcome_validate_for_protocol(
            &receipt->program_outcome, receipt->protocol_version);
        if (status != LXP_OK) return status;
        if (receipt->program_outcome.terminal_kind ==
            LXP_PROGRAM_TERMINAL_SUCCESS) {
            if (receipt->result_code !=
                    receipt->program_outcome.result_code ||
                lxp_ct_memcmp(receipt->transfer_set_root,
                              receipt->program_outcome.transfer_root,
                              32U) != 0)
                return LXP_FATAL_INVARIANT;
        } else if (receipt->result_code !=
                       receipt->program_outcome.result_code ||
                   !lxp_ct_is_zero(receipt->transfer_set_root, 32U) ||
                   !lxp_ct_is_zero(receipt->program_outcome.transfer_root,
                                   32U)) {
            return LXP_FATAL_INVARIANT;
        }
    }
    status = lxp_codec_writer_init(&writer, arena, LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK)
        status = lxp_codec_write_struct_header_version(
            &writer, LXP_RECEIPT_STRUCTURE_TAG, receipt->protocol_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u16(&writer, receipt->protocol_version);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->activity_id, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, receipt->global_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->previous_state_root,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->resulting_state_root,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->activity_root,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_i32(&writer, receipt->result_code);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer, (uint32_t)receipt->effects.count);
    for (i = 0U; status == LXP_OK && i < receipt->effects.count; ++i)
        status = effect_encode(&writer, &receipt->effects.effects[i]);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->fee_charged);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->batch_id, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u16(&writer, receipt->module_id);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer, receipt->module_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer, receipt->parameter_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(&writer, receipt->operation);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->asset, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->amount);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->from, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->from_balance_before);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->from_balance_after);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, receipt->from_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->to, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->to_balance_before);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, receipt->to_balance_after);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->transfer_set_root,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->authorization_hash,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, receipt->context_hash,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, receipt->timestamp);
    if (status == LXP_OK && receipt->program_outcome.present)
        status = program_outcome_encode(&writer, &receipt->program_outcome);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(&writer, include_signature ? 1U : 0U);
    if (status == LXP_OK && include_signature)
        status = lxp_codec_write_bytes(&writer, receipt->sequencer_signature,
                                       64U, 64U);
    if (status != LXP_OK) return status;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_receipt_decode(const uint8_t *bytes, size_t length,
                              bool require_signature, lxp_receipt *receipt)
{
    lxp_codec_reader reader;
    lxp_receipt decoded;
    uint16_t envelope_version;
    uint32_t effect_count;
    uint8_t signature_present;
    size_t index;
    lxp_result status;
    if (receipt == NULL || (bytes == NULL && length != 0U) ||
        length > LXP_MAX_ACTIVITY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(receipt, 0, sizeof(*receipt));
    (void)memset(&decoded, 0, sizeof(decoded));
    status = lxp_codec_reader_init(&reader, bytes, length);
    if (status == LXP_OK)
        status = lxp_codec_read_struct_header_version(
            &reader, LXP_RECEIPT_STRUCTURE_TAG, &envelope_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u16(&reader, &decoded.protocol_version);
    if (status == LXP_OK &&
        (decoded.protocol_version != envelope_version ||
         !lxp_protocol_version_supported(decoded.protocol_version)))
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.activity_id, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(&reader, &decoded.global_sequence);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.previous_state_root, 32U);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.resulting_state_root, 32U);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.activity_root, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_i32(&reader, &decoded.result_code);
    if (status == LXP_OK)
        status = lxp_codec_read_u32(&reader, &effect_count);
    if (status == LXP_OK && effect_count > LXP_MAX_EFFECTS)
        status = LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; status == LXP_OK && index < effect_count; ++index) {
        lxp_effect effect;
        status = effect_decode(&reader, &effect);
        if (status == LXP_OK)
            status = lxp_effect_buffer_add(&decoded.effects, &effect);
    }
    if (status == LXP_OK)
        status = lxp_codec_read_u128(&reader, &decoded.fee_charged);
    if (status == LXP_OK) status = copy_exact(&reader, decoded.batch_id, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u16(&reader, &decoded.module_id);
    if (status == LXP_OK)
        status = lxp_codec_read_u32(&reader, &decoded.module_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u32(&reader, &decoded.parameter_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u8(&reader, &decoded.operation);
    if (status == LXP_OK) status = copy_exact(&reader, decoded.asset, 32U);
    if (status == LXP_OK) status = lxp_codec_read_u128(&reader, &decoded.amount);
    if (status == LXP_OK) status = copy_exact(&reader, decoded.from, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u128(&reader, &decoded.from_balance_before);
    if (status == LXP_OK)
        status = lxp_codec_read_u128(&reader, &decoded.from_balance_after);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(&reader, &decoded.from_sequence);
    if (status == LXP_OK) status = copy_exact(&reader, decoded.to, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u128(&reader, &decoded.to_balance_before);
    if (status == LXP_OK)
        status = lxp_codec_read_u128(&reader, &decoded.to_balance_after);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.transfer_set_root, 32U);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.authorization_hash, 32U);
    if (status == LXP_OK)
        status = copy_exact(&reader, decoded.context_hash, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(&reader, &decoded.timestamp);
    if (status == LXP_OK && reader.length - reader.offset > 69U)
        status = program_outcome_decode(&reader, &decoded.program_outcome);
    if (status == LXP_OK)
        status = lxp_codec_read_u8(&reader, &signature_present);
    if (status == LXP_OK && signature_present > 1U)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK && require_signature && signature_present == 0U)
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_OK && signature_present != 0U)
        status = copy_exact(&reader, decoded.sequencer_signature, 64U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status != LXP_OK) return status;
    if (decoded.program_outcome.present) {
        lxp_program_outcome outcome = decoded.program_outcome;
        (void)memset(&decoded.program_outcome, 0,
                     sizeof(decoded.program_outcome));
        status = lxp_receipt_bind_program_outcome(&decoded, &outcome);
        if (status != LXP_OK) return status;
    }
    if (decoded.global_sequence == 0U || decoded.module_id == 0U ||
        decoded.module_version == 0U || decoded.timestamp == 0U ||
        lxp_ct_is_zero(decoded.activity_id, 32U) ||
        lxp_ct_is_zero(decoded.resulting_state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    *receipt = decoded;
    return LXP_OK;
}

lxp_result lxp_receipt_digest(const lxp_receipt *receipt, lxp_arena *arena,
                              uint8_t digest[32])
{
    size_t mark = lxp_arena_mark(arena);
    lxp_byte_span encoded;
    lxp_result status = lxp_receipt_encode(receipt, false, arena, &encoded);
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_RECEIPT, encoded.bytes,
                                 encoded.length, digest);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_receipt_sign(lxp_receipt *receipt,
                            const uint8_t private_key[32], lxp_arena *arena)
{
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    uint8_t digest[32];
    size_t signature_length = 64U;
    lxp_result status;
    int signed_ok;
    if (receipt == NULL || private_key == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_receipt_digest(receipt, arena, digest);
    if (status != LXP_OK) return status;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key,
                                       32U);
    context = key == NULL ? NULL : EVP_MD_CTX_new();
    signed_ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, receipt->sequencer_signature,
                       &signature_length, digest, sizeof(digest)) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    lxp_secure_zero(digest, sizeof(digest));
    return signed_ok ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

lxp_result lxp_receipt_verify(const lxp_receipt *receipt,
                              const uint8_t public_key[32], lxp_arena *arena)
{
    uint8_t digest[32];
    lxp_result status;
    if (receipt == NULL || public_key == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_receipt_digest(receipt, arena, digest);
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(public_key,
                                        receipt->sequencer_signature,
                                        digest, sizeof(digest));
    lxp_secure_zero(digest, sizeof(digest));
    return status;
}

lxp_result lxp_verified_receipt_index_init(lxp_verified_receipt_index *index)
{
    if (index == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(index, 0, sizeof(*index));
    return LXP_OK;
}

lxp_result lxp_verified_receipt_index_bind_fallback(
    lxp_verified_receipt_index *index,
    lxp_verified_receipt_fallback_fn fallback, void *context)
{
    if (index == NULL || ((fallback == NULL) != (context == NULL)))
        return LXP_ERR_NON_CANONICAL;
    index->fallback = fallback;
    index->fallback_context = context;
    return LXP_OK;
}

static bool verified_facts_equal(const lxp_verified_receipt_facts *left,
                                 const lxp_verified_receipt_facts *right)
{
    return lxp_ct_memcmp(left->receipt_digest,
                         right->receipt_digest, 32U) == 0 &&
        left->result_code == right->result_code &&
        left->global_sequence == right->global_sequence &&
        left->timestamp == right->timestamp &&
        lxp_ct_memcmp(left->asset, right->asset, 32U) == 0 &&
        lxp_u128_cmp(left->amount, right->amount) == 0 &&
        lxp_ct_memcmp(left->resulting_state_root,
                      right->resulting_state_root, 32U) == 0;
}

lxp_result lxp_verified_receipt_index_add(
    lxp_verified_receipt_index *index, const lxp_receipt *receipt,
    const uint8_t sequencer_public_key[32], lxp_arena *arena)
{
    lxp_verified_receipt_facts facts;
    size_t at;
    lxp_result status;
    if (index == NULL || receipt == NULL || sequencer_public_key == NULL ||
        arena == NULL || index->count > LXP_VERIFIED_RECEIPT_INDEX_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_receipt_verify(receipt, sequencer_public_key, arena);
    if (status != LXP_OK) return status;
    (void)memset(&facts, 0, sizeof(facts));
    status = lxp_receipt_digest(receipt, arena, facts.receipt_digest);
    if (status != LXP_OK) return status;
    facts.result_code = receipt->result_code;
    facts.global_sequence = receipt->global_sequence;
    facts.timestamp = receipt->timestamp;
    (void)memcpy(facts.asset, receipt->asset, 32U);
    facts.amount = receipt->amount;
    (void)memcpy(facts.resulting_state_root, receipt->resulting_state_root, 32U);
    for (at = 0U; at < index->count; ++at) {
        int order = memcmp(index->entries[at].receipt_digest,
                           facts.receipt_digest, 32U);
        if (order == 0)
            return verified_facts_equal(&index->entries[at], &facts) ?
                LXP_OK : LXP_FATAL_INVARIANT;
        if (order > 0) break;
    }
    if (index->count == LXP_VERIFIED_RECEIPT_INDEX_MAX) {
        size_t oldest = 0U;
        size_t candidate;
        for (candidate = 1U; candidate < index->count; ++candidate)
            if (index->entries[candidate].global_sequence <
                    index->entries[oldest].global_sequence)
                oldest = candidate;
        if (oldest + 1U != index->count)
            (void)memmove(&index->entries[oldest],
                          &index->entries[oldest + 1U],
                          (index->count - oldest - 1U) *
                              sizeof(index->entries[0]));
        --index->count;
        for (at = 0U; at < index->count; ++at)
            if (memcmp(index->entries[at].receipt_digest,
                       facts.receipt_digest, 32U) > 0)
                break;
    }
    if (at != index->count)
        (void)memmove(&index->entries[at + 1U], &index->entries[at],
                      (index->count - at) * sizeof(index->entries[0]));
    index->entries[at] = facts;
    ++index->count;
    return LXP_OK;
}

lxp_result lxp_verified_receipt_index_lookup(
    const lxp_verified_receipt_index *index,
    const uint8_t receipt_digest[32], lxp_verified_receipt_facts *facts)
{
    size_t left = 0U;
    size_t right;
    if (index == NULL || receipt_digest == NULL || facts == NULL ||
        index->count > LXP_VERIFIED_RECEIPT_INDEX_MAX ||
        lxp_ct_is_zero(receipt_digest, 32U)) return LXP_ERR_NON_CANONICAL;
    right = index->count;
    while (left < right) {
        size_t middle = left + (right - left) / 2U;
        int order = memcmp(index->entries[middle].receipt_digest,
                           receipt_digest, 32U);
        if (order < 0) left = middle + 1U;
        else right = middle;
    }
    if (left == index->count ||
        memcmp(index->entries[left].receipt_digest, receipt_digest, 32U) != 0)
        return index->fallback == NULL ? LXP_ERR_UNKNOWN_FIELD :
               index->fallback(index->fallback_context,
                               receipt_digest, facts);
    *facts = index->entries[left];
    return LXP_OK;
}
