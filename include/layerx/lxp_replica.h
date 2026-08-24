#ifndef LAYERX_LXP_REPLICA_H
#define LAYERX_LXP_REPLICA_H

#include "layerx/lxp_batch.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct lxp_replica {
    lxp_log *log;
    lxp_batch_header head;
    uint64_t durable_batch_count;
    bool has_head;
    bool halted;
    bool execution_enabled;
    bool acknowledgements_enabled;
    bool serving_current_state;
    bool serving_finalised_history;
} lxp_replica;

enum { LXP_MAX_DIVERGENCE_VALUE_BYTES = 1024 };
typedef enum lxp_divergence_component {
    LXP_DIVERGENCE_RECEIPT = 1,
    LXP_DIVERGENCE_STATE_DIFF = 2,
    LXP_DIVERGENCE_STATE_ROOT = 3
} lxp_divergence_component;

typedef struct lxp_divergence_state {
    uint64_t batch_number;
    uint64_t global_sequence;
    lxp_divergence_component component;
    uint8_t expected[LXP_MAX_DIVERGENCE_VALUE_BYTES];
    size_t expected_length;
    uint8_t produced[LXP_MAX_DIVERGENCE_VALUE_BYTES];
    size_t produced_length;
    bool detected;
} lxp_divergence_state;

typedef struct lxp_divergence_report_record {
    lxp_divergence_state divergence;
    uint8_t replica_id[32];
    uint8_t signature[64];
} lxp_divergence_report_record;

enum {
    LXP_MAX_REPLAY_TRANSITIONS = 16,
    LXP_MAX_REPLAY_FIELD_BYTES = 1048576
};

typedef struct lxp_replay_activity_output {
    int32_t result_code;
    lxp_u128 fee_charged;
    lxp_byte_span effects;
    lxp_byte_span resulting_balance;
    lxp_byte_span canonical_receipt;
    lxp_byte_span canonical_events;
    uint8_t resulting_state_root[32];
} lxp_replay_activity_output;

typedef lxp_result (*lxp_replay_transition_fn)(
    void *context, uint16_t transition_version, uint32_t parameter_version,
    uint64_t sealed_timestamp_ms, uint64_t global_sequence,
    lxp_byte_span canonical_activity, const uint8_t previous_state_root[32],
    lxp_arena *arena, lxp_replay_activity_output *output);
typedef lxp_result (*lxp_replay_parameter_version_fn)(
    void *context, uint64_t epoch, uint32_t *parameter_version);
typedef lxp_result (*lxp_replay_batch_finalize_fn)(
    void *context, const lxp_batch_header *header, uint32_t parameter_version,
    uint64_t system_sequence, const uint8_t previous_state_root[32],
    lxp_arena *arena, lxp_replay_activity_output *output);

typedef struct lxp_replay_transition_registration {
    uint16_t version;
    lxp_replay_transition_fn transition;
} lxp_replay_transition_registration;

typedef struct lxp_replay_engine {
    lxp_replay_transition_registration
        transitions[LXP_MAX_REPLAY_TRANSITIONS];
    size_t transition_count;
    lxp_replay_parameter_version_fn parameter_version;
    lxp_replay_batch_finalize_fn batch_finalize;
    void *batch_finalize_context;
    void *context;
} lxp_replay_engine;
#define lxp_replay_engine lxp_replay_engine

typedef struct lxp_replay_batch_result {
    lxp_replay_activity_output *outputs;
    lxp_byte_span *encoded_receipts;
    lxp_byte_span *encoded_events;
    size_t activity_count;
    lxp_replay_activity_output batch_maintenance_output;
    lxp_byte_span encoded_batch_maintenance_receipt;
    size_t receipt_count;
    lxp_byte_span canonical_receipt_section;
    lxp_byte_span canonical_event_section;
    uint8_t resulting_state_root[32];
    lxp_batch_roots roots;
} lxp_replay_batch_result;

lxp_result lxp_replica_init(lxp_replica *replica, lxp_log *log);
lxp_result lxp_replica_validate_header(
    const lxp_batch_body *body, uint32_t configured_network_id,
    const lxp_sequencer_authorization *authorization, lxp_arena *arena);
lxp_result lxp_replica_chain_link(const lxp_batch_header *previous,
                                  const lxp_batch_header *candidate);
lxp_result lxp_replica_ingest_batch(
    lxp_replica *replica, const uint8_t *canonical_body, size_t body_length,
    uint32_t configured_network_id,
    const lxp_sequencer_authorization *authorization, lxp_arena *arena,
    bool *acknowledge);
lxp_result lxp_replay_engine_init(
    lxp_replay_engine *engine,
    lxp_replay_parameter_version_fn parameter_version, void *context);
lxp_result lxp_replay_engine_register(lxp_replay_engine *engine,
                                      uint16_t version,
                                      lxp_replay_transition_fn transition);
/* Occupancy-protocol transitions may be registered only after the mandatory
 * batch finalizer has been bound; an unbound v2 engine fails closed. */
lxp_result lxp_replay_engine_register_batch_finalizer(
    lxp_replay_engine *engine, lxp_replay_batch_finalize_fn finalize,
    void *context);
lxp_result lxp_replay_section_encode(const lxp_byte_span *items, size_t count,
                                     lxp_arena *arena,
                                     lxp_byte_span *encoded);
lxp_result lxp_replay_section_decode(const lxp_byte_span *section,
                                     lxp_arena *arena,
                                     lxp_byte_span **items, size_t *count);
lxp_result lxp_replay_batch(lxp_replay_engine *engine,
                            const lxp_batch_body *body,
                            const uint8_t starting_state_root[32],
                            lxp_arena *arena,
                            lxp_replay_batch_result *result);
lxp_result lxp_replay_verify_roots(const lxp_replay_batch_result *recomputed,
                                   const lxp_batch_body *published);
lxp_result lxp_divergence_detect(lxp_divergence_state *state,
                                 uint64_t batch_number,
                                 uint64_t global_sequence,
                                 lxp_divergence_component component,
                                 lxp_byte_span expected,
                                 lxp_byte_span produced);
lxp_result lxp_divergence_report(
    const lxp_divergence_state *state, const uint8_t replica_id[32],
    const uint8_t private_key[32], lxp_divergence_report_record *report);
lxp_result lxp_divergence_report_verify(
    const lxp_divergence_report_record *report,
    const uint8_t public_key[32]);
lxp_result lxp_replica_halt(lxp_replica *replica);

#endif
