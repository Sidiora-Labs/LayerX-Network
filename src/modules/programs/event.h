#ifndef LAYERX_PROGRAMS_EVENT_H
#define LAYERX_PROGRAMS_EVENT_H

#include "layerx/programs.h"
#include "layerx/lxp_receipt.h"

#include <stddef.h>
#include <stdint.h>

/*
 * Private Programs CALL publication seam.  The CALL controller owns when these
 * helpers are invoked; this header deliberately does not extend programs.h.
 */
enum {
    LXP_PROGRAMS_EVENT_ENVELOPE_VERSION = 1,
    LXP_PROGRAMS_OUTCOME_ENVELOPE_VERSION = 2,
    LXP_PROGRAMS_EVENT_FRAME_BYTES = 8,
    LXP_PROGRAMS_EVENT_DIGEST_BYTES = 32,
    LXP_PROGRAMS_EVENT_MAX_TOPIC_BYTES = 64,
    LXP_PROGRAMS_EVENT_MAX_DATA_BYTES = 65536,
    LXP_PROGRAMS_EVENT_GUEST_BODY_BYTES = 185,
    LXP_PROGRAMS_EVENT_OUTCOME_BODY_V1_BYTES = 251,
    LXP_PROGRAMS_EVENT_OUTCOME_BODY_BYTES = 255
};

typedef struct lxp_programs_guest_event {
    const uint8_t *program_id;
    const uint8_t *principal;
    const uint8_t *activity_id;
    const uint8_t *frame_path;
    uint8_t frame_depth;
    uint32_t event_index;
    const uint8_t *topic;
    size_t topic_length;
    const uint8_t *data;
    size_t data_length;
} lxp_programs_guest_event;

typedef struct lxp_programs_call_outcome {
    const uint8_t *program_id;
    const uint8_t *principal;
    const uint8_t *activity_id;
    const uint8_t *frame_path;
    uint8_t frame_depth;
    uint16_t runtime_version;
    uint16_t abi_version;
    uint32_t fee_schedule_version;
    uint32_t metering_schedule_version;
    lxp_result terminal_result;
    const uint8_t *transfer_set_root;
    const uint8_t *call_graph_digest;
    const uint8_t *terminal_detail_digest;
    const uint8_t *event_envelope_digest;
} lxp_programs_call_outcome;

lxp_result lxp_programs_emit_guest_event(
    lxp_module_ctx *ctx, const lxp_programs_guest_event *event);
lxp_result lxp_programs_emit_call_outcome(
    lxp_module_ctx *ctx, const lxp_programs_call_outcome *outcome);
#endif
