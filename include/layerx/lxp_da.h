#ifndef LAYERX_LXP_DA_H
#define LAYERX_LXP_DA_H

#include "layerx/lxp_batch.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LXP_DA_CLASS_COUNT = 5,
    LXP_DA_MAX_CHUNKS = 4096,
    LXP_DA_MAX_CHUNK_BYTES = 65536,
    LXP_DA_CANONICAL_CHUNK_BYTES = LXP_DA_MAX_CHUNK_BYTES,
    LAYERX_DA_MAX_BATCHES_PER_FETCH = 8,
    LXP_DA_MAX_MODULE_ROOTS = 256,
    LXP_DA_MAX_ACCOUNT_FRONTIER_BYTES = 1048576,
    LXP_DA_STORE_PATH_BYTES = 4096
};

typedef enum lxp_da_class {
    LXP_DA_ACTIVITIES = 1,
    LXP_DA_RECEIPTS = 2,
    LXP_DA_ORACLE_INPUTS = 3,
    LXP_DA_STATE_DIFF = 4,
    LXP_DA_RECOVERY_METADATA = 5
} lxp_da_class;
#define lxp_da_class lxp_da_class

typedef struct lxp_da_chunk {
    uint64_t batch_number;
    uint32_t chunk_index;
    lxp_da_class availability_class;
    uint64_t class_offset;
    uint32_t length;
    lxp_byte_span bytes;
    uint8_t chunk_hash[32];
} lxp_da_chunk;
#define lxp_da_chunk lxp_da_chunk

typedef struct lxp_da_bundle {
    lxp_da_chunk *chunks;
    size_t chunk_count;
    uint64_t batch_number;
    size_t total_bytes;
} lxp_da_bundle;

typedef struct lxp_da_module_root {
    uint16_t module_id;
    uint8_t state_root[32];
} lxp_da_module_root;

typedef struct lxp_da_recovery_input {
    const lxp_da_module_root *module_roots;
    size_t module_root_count;
    lxp_byte_span account_tree_frontier;
    uint64_t next_global_sequence;
    uint64_t receipt_watermark;
    uint64_t projection_watermark;
} lxp_da_recovery_input;

typedef struct lxp_da_store {
    char directory[LXP_DA_STORE_PATH_BYTES];
} lxp_da_store;

typedef enum lxp_da_lookup_kind {
    LXP_DA_LOOKUP_CHECKPOINT_ID = 1,
    LXP_DA_LOOKUP_BATCH_NUMBER = 2,
    LXP_DA_LOOKUP_SEQUENCE_RANGE = 3,
    LXP_DA_LOOKUP_ACTIVITY_ID = 4
} lxp_da_lookup_kind;

typedef struct lxp_da_retrieval_request {
    lxp_da_lookup_kind lookup_kind;
    uint8_t checkpoint_id[32];
    uint64_t batch_number;
    uint64_t first_global_sequence;
    uint64_t last_global_sequence;
    uint8_t activity_id[32];
} lxp_da_retrieval_request;
#define lxp_da_retrieval_request lxp_da_retrieval_request

struct lxp_guarantor_ctx;
struct lxp_checkpoint_certificate;
struct lxp_guarantor_attestation;
struct lxp_replay_engine;
struct lxp_replay_batch_result;

typedef lxp_result (*lxp_da_chunk_fetch_fn)(
    void *context, const lxp_da_retrieval_request *request,
    uint32_t chunk_index, lxp_arena *arena, lxp_byte_span *response);

struct lxp_kernel;
lxp_result lxp_da_recovery_from_kernel(
    const struct lxp_kernel *kernel, uint64_t receipt_watermark,
    uint64_t projection_watermark, lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_da_receipt_section_encode(
    const lxp_byte_span *receipts, size_t receipt_count,
    const lxp_byte_span *events, size_t event_count,
    lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_da_receipt_section_decode(
    lxp_byte_span encoded, lxp_arena *arena,
    lxp_byte_span **receipts, size_t *receipt_count,
    lxp_byte_span **events, size_t *event_count);
lxp_result lxp_da_body_from_kernels(
    const lxp_batch_header *header,
    const struct lxp_kernel *before, const struct lxp_kernel *after,
    const lxp_byte_span *activities, size_t activity_count,
    const lxp_byte_span *receipts, size_t receipt_count,
    const lxp_byte_span *events, size_t event_count,
    const lxp_byte_span *oracles, size_t oracle_count,
    lxp_arena *arena, lxp_batch_body *body);

lxp_result lxp_da_recovery_metadata_encode(
    const lxp_da_recovery_input *input, lxp_arena *arena,
    lxp_byte_span *encoded);
lxp_result lxp_da_bundle_build(const lxp_batch_body *body, size_t chunk_size,
                               lxp_arena *arena, lxp_da_bundle *bundle);
lxp_result lxp_batch_availability_root(const lxp_batch_body *body,
                                       lxp_arena *arena, uint8_t root[32]);
lxp_result lxp_da_bundle_body(const lxp_da_bundle *bundle,
                              const lxp_batch_header *header,
                              lxp_arena *arena, lxp_batch_body *body);
lxp_result lxp_da_chunk_hash(lxp_da_chunk *chunk);
lxp_result lxp_da_bundle_root(const lxp_da_bundle *bundle, lxp_arena *arena,
                              uint8_t root[32]);
lxp_result lxp_da_store_init(lxp_da_store *store, const char *directory);
lxp_result lxp_da_store_bundle(const lxp_da_store *store,
                               const lxp_da_bundle *bundle,
                               lxp_arena *arena);
lxp_result lxp_da_store_read_bundle(const lxp_da_store *store,
                                    uint64_t batch_number,
                                    lxp_arena *arena,
                                    lxp_da_bundle *bundle,
                                    uint8_t root[32]);
lxp_result lxp_da_store_read_verified(const lxp_da_store *store,
                                     uint64_t batch_number,
                                     const uint8_t expected_root[32],
                                     lxp_arena *arena,
                                     lxp_da_bundle *bundle);
lxp_result lxp_da_log_read_body(const lxp_log *log, uint64_t batch_number,
                               lxp_arena *arena, lxp_batch_body *body);
lxp_result lxp_da_log_store_body(lxp_log *log, const lxp_batch_body *body,
                                lxp_arena *arena);
lxp_result lxp_da_serve_chunk_proof(const lxp_da_store *store,
                                   uint64_t batch_number,
                                   uint32_t chunk_index,
                                   const uint8_t expected_root[32],
                                   lxp_arena *arena,
                                   lxp_byte_span *bytes,
                                   lxp_byte_span *proof_material);
lxp_result lxp_da_possession_attest(
    const lxp_da_store *store, const struct lxp_guarantor_ctx *ctx,
    const struct lxp_checkpoint_certificate *checkpoint,
    uint64_t attested_at_ms, lxp_arena *arena,
    struct lxp_guarantor_attestation *attestation);
lxp_result lxp_da_possession_verify(
    const lxp_da_store *store,
    const struct lxp_guarantor_attestation *attestation,
    const uint8_t expected_data_availability_root[32], lxp_arena *arena);
lxp_result lxp_da_serve_chunk(const lxp_da_store *store,
                              uint64_t batch_number, uint32_t chunk_index,
                              lxp_arena *arena, lxp_byte_span *response);
lxp_result lxp_da_fetch(const lxp_da_retrieval_request *request,
                        lxp_da_chunk_fetch_fn fetch_chunk,
                        void *fetch_context, lxp_arena *arena,
                        lxp_da_bundle *bundle, uint8_t root[32]);
lxp_result lxp_da_verify_served_bytes(
    const lxp_da_bundle *bundle, const lxp_batch_header *header,
    struct lxp_replay_engine *engine,
    const uint8_t starting_state_root[32], lxp_arena *arena,
    struct lxp_replay_batch_result *replayed);
lxp_result lxp_da_withhold_sim(const lxp_da_bundle *bundle,
                               lxp_da_class withheld_class,
                               lxp_arena *arena,
                               lxp_da_bundle *served_bundle,
                               uint8_t *available_class_mask);

#endif
