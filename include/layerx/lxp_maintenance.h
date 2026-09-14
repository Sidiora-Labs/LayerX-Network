#ifndef LAYERX_LXP_MAINTENANCE_H
#define LAYERX_LXP_MAINTENANCE_H

#include "layerx/programs.h"

enum { LXP_BATCH_MAINTENANCE_MAX_BYTES = 524288 };

typedef struct lxp_batch_maintenance {
    uint16_t protocol_version;
    uint64_t epoch;
    uint64_t batch_number;
    uint64_t timestamp_ms;
    uint64_t global_sequence;
    uint32_t parameter_version;
    lxp_byte_span occupancy;
    lxp_byte_span effects;
} lxp_batch_maintenance;

bool lxp_batch_maintenance_is_envelope(lxp_byte_span encoded);
lxp_result lxp_batch_maintenance_effects_validate(lxp_byte_span effects);
lxp_result lxp_batch_maintenance_encode(const lxp_batch_maintenance *record,
    lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_batch_maintenance_decode(const uint8_t *bytes, size_t length,
    lxp_batch_maintenance *record);
lxp_result lxp_batch_maintenance_occupancy_decode(const uint8_t *bytes,
    size_t length, lxp_programs_occupancy_receipt *record);
lxp_result lxp_batch_maintenance_events(lxp_byte_span encoded,
    const lxp_batch_header *header, lxp_byte_span *events);

#endif
