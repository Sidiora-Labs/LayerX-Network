#include "event.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

enum {
    GUEST_KIND = 1,
    OUTCOME_KIND = 2,
    GUEST_DOMAIN_BYTES = 4,
    OUTCOME_DOMAIN_BYTES = 4,
    TOPIC_DOMAIN_BYTES = 30,
    DATA_DOMAIN_BYTES = 29
};

static const uint8_t guest_domain[GUEST_DOMAIN_BYTES] = { 'L', 'X', 'G', 'E' };
static const uint8_t legacy_outcome_domain[OUTCOME_DOMAIN_BYTES] = {
    'L', 'X', 'C', 'O'
};
static const uint8_t outcome_domain[OUTCOME_DOMAIN_BYTES] = { 'L', 'X', 'M', 'O' };
static const uint8_t topic_domain[TOPIC_DOMAIN_BYTES] =
    "LayerX/programs/event-topic/v1";
static const uint8_t data_domain[DATA_DOMAIN_BYTES] =
    "LayerX/programs/event-data/v1";

static void write_u16(uint8_t out[2], uint16_t value)
{
    out[0] = (uint8_t)(value >> 8U);
    out[1] = (uint8_t)value;
}

static void write_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static uint16_t read_u16(const uint8_t in[2])
{
    return (uint16_t)(((uint16_t)in[0] << 8U) | in[1]);
}

static uint32_t read_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static lxp_result digest(const uint8_t *domain, size_t domain_length,
                         const uint8_t *value, size_t value_length,
                         uint8_t out[LXP_PROGRAMS_EVENT_DIGEST_BYTES])
{
    lxp_hash_context context;
    lxp_result status;
    if ((value == NULL && value_length != 0U) || out == NULL)
        return LXP_ERR_NON_CANONICAL;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, domain, domain_length);
    if (status == LXP_OK) status = lxp_hash_update(&context, value, value_length);
    if (status == LXP_OK) status = lxp_hash_final(&context, out);
    return status;
}

static bool required_id(const uint8_t *value)
{
    return value != NULL && !lxp_ct_is_zero(value, LXP_PROGRAMS_EVENT_DIGEST_BYTES);
}

static bool valid_frame(const uint8_t *path, uint8_t depth)
{
    size_t index;
    if (path == NULL || depth > LXP_PROGRAMS_EVENT_FRAME_BYTES) return false;
    for (index = 0U; index < LXP_PROGRAMS_EVENT_FRAME_BYTES; ++index) {
        if ((index < depth && path[index] == 0U) ||
            (index >= depth && path[index] != 0U))
            return false;
    }
    return true;
}

static bool envelope_effect_valid(const lxp_effect *effect)
{
    if (effect->module_id != LXP_MODULE_PROGRAMS ||
        effect->kind != LXP_EFFECT_EVENT)
        return false;
    if (effect->event_type == LX_PROGRAMS_EVENT_GUEST_ENVELOPE)
        return effect->body_length == LXP_PROGRAMS_EVENT_GUEST_BODY_BYTES &&
               memcmp(effect->body, guest_domain, sizeof(guest_domain)) == 0 &&
               effect->body[sizeof(guest_domain)] ==
                   LXP_PROGRAMS_EVENT_ENVELOPE_VERSION &&
               effect->body[sizeof(guest_domain) + 1U] == GUEST_KIND;
    if (effect->event_type == LX_PROGRAMS_EVENT_CALL_OUTCOME) {
        const bool legacy =
            effect->body_length == LXP_PROGRAMS_EVENT_OUTCOME_BODY_V1_BYTES &&
            memcmp(effect->body, legacy_outcome_domain,
                   sizeof(legacy_outcome_domain)) == 0 &&
            effect->body[sizeof(legacy_outcome_domain)] ==
                LXP_PROGRAMS_EVENT_ENVELOPE_VERSION;
        const bool metered =
            effect->body_length == LXP_PROGRAMS_EVENT_OUTCOME_BODY_BYTES &&
            memcmp(effect->body, outcome_domain,
                   sizeof(outcome_domain)) == 0 &&
            effect->body[sizeof(outcome_domain)] ==
                LXP_PROGRAMS_OUTCOME_ENVELOPE_VERSION;
        size_t offset = 6U;
        uint16_t runtime_version;
        uint16_t abi_version;
        uint32_t fee_schedule_version;
        uint32_t metering_schedule_version =
            LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
        lxp_result terminal_result;
        if ((!legacy && !metered) ||
            effect->body[sizeof(outcome_domain) + 1U] != OUTCOME_KIND ||
            !required_id(effect->body + offset) ||
            !required_id(effect->body + offset + 32U) ||
            !required_id(effect->body + offset + 64U))
            return false;
        offset += 96U;
        if (!valid_frame(effect->body + offset,
                         effect->body[offset +
                             LXP_PROGRAMS_EVENT_FRAME_BYTES]))
            return false;
        offset += LXP_PROGRAMS_EVENT_FRAME_BYTES + 1U;
        runtime_version = read_u16(effect->body + offset); offset += 2U;
        abi_version = read_u16(effect->body + offset); offset += 2U;
        fee_schedule_version = read_u32(effect->body + offset); offset += 4U;
        if (metered) {
            metering_schedule_version = read_u32(effect->body + offset);
            offset += 4U;
        }
        terminal_result = (lxp_result)(int32_t)read_u32(
            effect->body + offset);
        offset += 4U;
        if (runtime_version == 0U || abi_version == 0U ||
            fee_schedule_version == 0U ||
            !lxp_program_metering_schedule_available(
                metering_schedule_version) ||
            terminal_result != LXP_OK)
            return false;
        offset += 32U;
        if (!required_id(effect->body + offset) ||
            !required_id(effect->body + offset + 32U) ||
            !required_id(effect->body + offset + 64U))
            return false;
        return offset + 96U == effect->body_length;
    }
    return false;
}

lxp_result lxp_programs_emit_guest_event(
    lxp_module_ctx *ctx, const lxp_programs_guest_event *event)
{
    uint8_t body[LXP_PROGRAMS_EVENT_GUEST_BODY_BYTES];
    uint8_t topic_digest[LXP_PROGRAMS_EVENT_DIGEST_BYTES];
    uint8_t data_digest[LXP_PROGRAMS_EVENT_DIGEST_BYTES];
    size_t offset = 0U;
    lxp_result status;
    if (ctx == NULL || event == NULL || !required_id(event->program_id) ||
        !required_id(event->principal) || !required_id(event->activity_id) ||
        !valid_frame(event->frame_path, event->frame_depth) ||
        event->topic_length > LXP_PROGRAMS_EVENT_MAX_TOPIC_BYTES ||
        event->data_length > LXP_PROGRAMS_EVENT_MAX_DATA_BYTES ||
        (event->topic == NULL && event->topic_length != 0U) ||
        (event->data == NULL && event->data_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    status = digest(topic_domain, sizeof(topic_domain), event->topic,
                    event->topic_length, topic_digest);
    if (status == LXP_OK)
        status = digest(data_domain, sizeof(data_domain), event->data,
                        event->data_length, data_digest);
    if (status != LXP_OK) return status;
    (void)memcpy(body + offset, guest_domain, sizeof(guest_domain));
    offset += sizeof(guest_domain);
    body[offset++] = LXP_PROGRAMS_EVENT_ENVELOPE_VERSION;
    body[offset++] = GUEST_KIND;
    (void)memcpy(body + offset, event->program_id, 32U); offset += 32U;
    (void)memcpy(body + offset, event->principal, 32U); offset += 32U;
    (void)memcpy(body + offset, event->activity_id, 32U); offset += 32U;
    (void)memcpy(body + offset, event->frame_path, LXP_PROGRAMS_EVENT_FRAME_BYTES);
    offset += LXP_PROGRAMS_EVENT_FRAME_BYTES;
    body[offset++] = event->frame_depth;
    write_u32(body + offset, event->event_index); offset += 4U;
    write_u16(body + offset, (uint16_t)event->topic_length); offset += 2U;
    (void)memcpy(body + offset, topic_digest, sizeof(topic_digest));
    offset += sizeof(topic_digest);
    write_u32(body + offset, (uint32_t)event->data_length); offset += 4U;
    (void)memcpy(body + offset, data_digest, sizeof(data_digest));
    offset += sizeof(data_digest);
    if (offset != sizeof(body)) return LXP_FATAL_INVARIANT;
    return lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_GUEST_ENVELOPE,
                              body, sizeof(body));
}

lxp_result lxp_programs_emit_call_outcome(
    lxp_module_ctx *ctx, const lxp_programs_call_outcome *outcome)
{
    uint8_t body[LXP_PROGRAMS_EVENT_OUTCOME_BODY_BYTES];
    size_t offset = 0U;
    if (ctx == NULL || outcome == NULL || !required_id(outcome->program_id) ||
        !required_id(outcome->principal) || !required_id(outcome->activity_id) ||
        !valid_frame(outcome->frame_path, outcome->frame_depth) ||
        outcome->transfer_set_root == NULL || outcome->call_graph_digest == NULL ||
        outcome->terminal_detail_digest == NULL ||
        outcome->event_envelope_digest == NULL ||
        outcome->runtime_version == 0U || outcome->abi_version == 0U ||
        outcome->fee_schedule_version == 0U ||
        !lxp_program_metering_schedule_available(
            outcome->metering_schedule_version) ||
        outcome->terminal_result != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(body + offset, outcome_domain, sizeof(outcome_domain));
    offset += sizeof(outcome_domain);
    body[offset++] = LXP_PROGRAMS_OUTCOME_ENVELOPE_VERSION;
    body[offset++] = OUTCOME_KIND;
    (void)memcpy(body + offset, outcome->program_id, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->principal, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->activity_id, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->frame_path, LXP_PROGRAMS_EVENT_FRAME_BYTES);
    offset += LXP_PROGRAMS_EVENT_FRAME_BYTES;
    body[offset++] = outcome->frame_depth;
    write_u16(body + offset, outcome->runtime_version); offset += 2U;
    write_u16(body + offset, outcome->abi_version); offset += 2U;
    write_u32(body + offset, outcome->fee_schedule_version); offset += 4U;
    write_u32(body + offset, outcome->metering_schedule_version); offset += 4U;
    write_u32(body + offset, (uint32_t)outcome->terminal_result); offset += 4U;
    (void)memcpy(body + offset, outcome->transfer_set_root, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->call_graph_digest, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->terminal_detail_digest, 32U); offset += 32U;
    (void)memcpy(body + offset, outcome->event_envelope_digest, 32U); offset += 32U;
    if (offset != sizeof(body)) return LXP_FATAL_INVARIANT;
    return lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_CALL_OUTCOME,
                              body, sizeof(body));
}

lxp_result lxp_programs_project_committed_events(
    const lxp_effect_buffer *effects, lxp_arena *arena,
    lxp_byte_span *canonical_events)
{
    lxp_codec_writer writer;
    size_t count = 0U;
    size_t index;
    uint16_t previous_ordinal = 0U;
    bool has_previous = false;
    lxp_result status;
    if (effects == NULL || arena == NULL || canonical_events == NULL ||
        effects->count > LXP_MAX_EFFECTS)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < effects->count; ++index) {
        const lxp_effect *effect = &effects->effects[index];
        if (effect->module_id != LXP_MODULE_PROGRAMS ||
            (effect->event_type != LX_PROGRAMS_EVENT_GUEST_ENVELOPE &&
             effect->event_type != LX_PROGRAMS_EVENT_CALL_OUTCOME))
            continue;
        if (!envelope_effect_valid(effect) ||
            (has_previous && effect->ordinal <= previous_ordinal))
            return LXP_ERR_NON_CANONICAL;
        previous_ordinal = effect->ordinal;
        has_previous = true;
        ++count;
    }
    status = lxp_codec_writer_init(&writer, arena, 4U + count * 265U);
    if (status == LXP_OK)
        status = lxp_codec_write_seq(&writer, (uint32_t)count, LXP_MAX_EFFECTS);
    for (index = 0U; status == LXP_OK && index < effects->count; ++index) {
        const lxp_effect *effect = &effects->effects[index];
        if (effect->module_id != LXP_MODULE_PROGRAMS ||
            (effect->event_type != LX_PROGRAMS_EVENT_GUEST_ENVELOPE &&
             effect->event_type != LX_PROGRAMS_EVENT_CALL_OUTCOME))
            continue;
        status = lxp_codec_write_u16(&writer, effect->module_id);
        if (status == LXP_OK)
            status = lxp_codec_write_u16(&writer, effect->ordinal);
        if (status == LXP_OK)
            status = lxp_codec_write_u16(&writer, effect->event_type);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, effect->body,
                                           effect->body_length, 256U);
    }
    if (status != LXP_OK) return status;
    canonical_events->bytes = writer.bytes;
    canonical_events->length = writer.length;
    return LXP_OK;
}
