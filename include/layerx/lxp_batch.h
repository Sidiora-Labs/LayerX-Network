#ifndef LAYERX_LXP_BATCH_H
#define LAYERX_LXP_BATCH_H

#include "layerx/lxp_arena.h"
#include "layerx/lxp_codec.h"
#include "layerx/lxp_storage.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LXP_BATCH_HEADER_ENCODED_SIZE = 354,
    LXP_MAX_BATCH_BODY_BYTES = 16777216,
    LXP_MAX_BATCH_REPLICAS = 32,
    LXP_MAX_BATCH_CHUNK_BYTES = 65536
};

typedef struct lxp_batch_header {
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
    uint64_t timestamp_ms;
    uint8_t sequencer_id[32];
} lxp_batch_header;
#define lxp_batch_header lxp_batch_header

typedef struct lxp_exec_clock {
    uint64_t sealed_timestamp_ms;
    uint8_t bound;
} lxp_exec_clock;
#define lxp_exec_clock lxp_exec_clock

typedef struct lxp_batch_root_inputs {
    const lxp_byte_span *activities;
    size_t activity_count;
    const lxp_byte_span *receipts;
    size_t receipt_count;
    const lxp_byte_span *events;
    size_t event_count;
    const lxp_byte_span *oracle_inputs;
    size_t oracle_input_count;
    const lxp_byte_span *availability_chunks;
    size_t availability_chunk_count;
} lxp_batch_root_inputs;

typedef struct lxp_batch_roots {
    uint8_t activity_merkle_root[32];
    uint8_t receipt_merkle_root[32];
    uint8_t event_merkle_root[32];
    uint8_t oracle_root[32];
    uint8_t data_availability_root[32];
} lxp_batch_roots;

typedef struct lxp_batch_seal_input {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t epoch;
    uint64_t batch_number;
    uint64_t first_sequence;
    uint64_t last_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint64_t timestamp_ms;
    uint8_t sequencer_id[32];
} lxp_batch_seal_input;

typedef struct lxp_sequencer_authorization {
    uint8_t sequencer_id[32];
    uint8_t public_key[32];
    uint64_t first_batch_number;
    uint64_t last_batch_number;
    uint8_t authorized;
} lxp_sequencer_authorization;

typedef struct lxp_batch_body {
    lxp_batch_header header;
    uint8_t sequencer_signature[64];
    lxp_byte_span activities;
    lxp_byte_span receipts;
    lxp_byte_span events;
    lxp_byte_span oracle_inputs;
    lxp_byte_span state_diff;
    lxp_byte_span recovery_metadata;
} lxp_batch_body;
#define lxp_batch_body lxp_batch_body

typedef struct lxp_batch_replica_target {
    uint8_t replica_id[32];
    uint64_t resume_offset;
    uint8_t complete;
} lxp_batch_replica_target;

typedef struct lxp_batch_eligibility_state {
    uint64_t batch_number;
    uint8_t replica_ids[LXP_MAX_BATCH_REPLICAS][32];
    uint8_t acknowledged[LXP_MAX_BATCH_REPLICAS];
    size_t replica_count;
    size_t threshold;
    size_t acknowledgement_count;
} lxp_batch_eligibility_state;

typedef lxp_result (*lxp_batch_chunk_send_fn)(
    void *context, const uint8_t replica_id[32], uint64_t batch_number,
    uint64_t offset, const uint8_t *bytes, size_t length,
    uint64_t total_length);

lxp_result lxp_batch_header_encode(const lxp_batch_header *header,
                                   lxp_arena *arena,
                                   lxp_byte_span *encoded);
lxp_result lxp_batch_header_decode(const uint8_t *bytes, size_t length,
                                   lxp_batch_header *header);
lxp_result lxp_batch_header_hash(const lxp_batch_header *header,
                                 lxp_arena *arena, uint8_t digest[32]);
lxp_result lxp_batch_timestamp_select(lxp_batch_header *header,
                                      uint64_t timestamp_ms);
lxp_result lxp_batch_timestamp_validate(const lxp_batch_header *previous,
                                        const lxp_batch_header *candidate,
                                        uint64_t maximum_forward_drift_ms);
lxp_result lxp_exec_clock_bind(lxp_exec_clock *clock,
                               const lxp_batch_header *sealed_header);
lxp_result lxp_exec_clock_read(const lxp_exec_clock *clock,
                               uint64_t *timestamp_ms);
lxp_result lxp_batch_roots_compute(const lxp_batch_root_inputs *inputs,
                                   lxp_arena *arena,
                                   lxp_batch_roots *roots);
lxp_result lxp_batch_seal(lxp_batch_header *header,
                          const lxp_batch_seal_input *input,
                          const lxp_batch_roots *roots, lxp_log *log,
                          lxp_arena *arena);
lxp_result lxp_batch_sign(const lxp_batch_header *header,
                          const uint8_t private_key[32],
                          const lxp_sequencer_authorization *authorization,
                          uint8_t signature[64], lxp_arena *arena);
lxp_result lxp_batch_verify_signature(
    const lxp_batch_header *header, const uint8_t *signature,
    size_t signature_length,
    const lxp_sequencer_authorization *authorization, lxp_arena *arena);
lxp_result lxp_batch_body_encode(const lxp_batch_body *body, lxp_arena *arena,
                                 lxp_byte_span *encoded);
lxp_result lxp_batch_body_decode(const uint8_t *bytes, size_t length,
                                 lxp_batch_body *body);
lxp_result lxp_batch_publish(const lxp_batch_body *body,
                             lxp_batch_replica_target *replicas,
                             size_t replica_count, size_t chunk_size,
                             lxp_batch_chunk_send_fn send_chunk,
                             void *send_context, lxp_arena *arena);
lxp_result lxp_batch_eligibility_init(
    lxp_batch_eligibility_state *state, uint64_t batch_number,
    const uint8_t (*replica_ids)[32], size_t replica_count, size_t threshold);
lxp_result lxp_batch_eligibility_reset(lxp_batch_eligibility_state *state,
                                       uint64_t batch_number);
lxp_result lxp_replica_ack(lxp_batch_eligibility_state *state,
                           const uint8_t replica_id[32], lxp_log *log);
lxp_result lxp_batch_eligibility(const lxp_batch_eligibility_state *state,
                                 bool *eligible);

#endif
