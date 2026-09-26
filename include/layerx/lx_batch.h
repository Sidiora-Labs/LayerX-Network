#ifndef LAYERX_LX_BATCH_H
#define LAYERX_LX_BATCH_H

#include "layerx/lx_oracle.h"
#include "layerx/lx_web.h"

#include <stddef.h>
#include <stdint.h>

enum { LX_ORACLE_LEAF_BYTES = 176 };

typedef struct lx_batch_header {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t epoch;
    uint64_t batch_number;
    uint64_t first_sequence;
    uint64_t last_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t activity_merkle_root[32];
    uint8_t receipt_merkle_root[32];
    uint8_t event_merkle_root[32];
    uint8_t data_availability_root[32];
    uint8_t oracle_root[32];
    uint8_t web_root[32];
    uint64_t timestamp;
    uint8_t sequencer_id[32];
} lx_batch_header;

typedef struct lx_oracle_availability_bundle {
    uint8_t leaves[LX_ORACLE_STORE_CAPACITY][LX_ORACLE_LEAF_BYTES];
    size_t count;
} lx_oracle_availability_bundle;

lxp_result lx_oracle_leaf_encode(const lx_oracle_accepted *accepted,
                                 uint8_t *bytes, size_t capacity,
                                 size_t *length);
lxp_result lx_oracle_root_compute(const lx_oracle_store *store,
                                  lxp_arena *arena, uint8_t root[32]);
lxp_result lx_batch_header_set_oracle_root(lx_batch_header *header,
                                           const lx_oracle_store *store,
                                           lxp_arena *arena);
lxp_result lx_batch_header_set_web_root(lx_batch_header *header,
                                        const lx_web_store *store,
                                        lxp_arena *arena);
lxp_result lx_oracle_availability_bundle_build(
    const lx_oracle_store *store, lx_oracle_availability_bundle *bundle);
lxp_result lx_oracle_root_from_availability(
    const lx_oracle_availability_bundle *bundle, lxp_arena *arena,
    uint8_t root[32]);

#endif
