#ifndef LAYERX_LX_WEB_H
#define LAYERX_LX_WEB_H

#include "layerx/lxp_activity.h"
#include "layerx/lxp_authority.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_result.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define LX_WEB_PREIMAGE_DOMAIN "PAXEERX_WEB_V1"
#define LX_WEB_REQUEST_TOPIC "PAXEERX_WEB_REQUEST_V1"

enum {
    LX_WEB_MODULE_ID = 11,
    LX_WEB_OBSERVATION_ACTIVITY = 0x000B0001,
    LX_WEB_ATTESTOR_SET_ACTIVITY = 0x000B0002,
    LX_WEB_ORIGIN_EVM = 1,
    LX_WEB_ORIGIN_PROGRAM = 2,
    LX_WEB_KIND_FETCH = 1,
    LX_WEB_KIND_SEARCH = 2,
    LX_WEB_DOMAIN_BYTES = 14,
    LX_WEB_PREIMAGE_BYTES = 188,
    LX_WEB_MAX_RESPONSE_BYTES = 4096,
    LX_WEB_MAX_ATTESTORS = 16,
    LX_WEB_SIGNATURE_BYTES = 65,
    LX_WEB_SIGNER_BYTES = 20,
    LX_WEB_OBSERVATION_HEADER_BYTES = 146,
    LX_WEB_OBSERVATION_MAX_BYTES = LX_WEB_OBSERVATION_HEADER_BYTES +
        LX_WEB_MAX_RESPONSE_BYTES + 1 +
        LX_WEB_MAX_ATTESTORS * LX_WEB_SIGNATURE_BYTES,
    LX_WEB_ATTESTOR_ENTRY_BYTES = LX_WEB_SIGNER_BYTES + 32,
    LX_WEB_ATTESTOR_SET_MAX_BYTES = 2 +
        LX_WEB_MAX_ATTESTORS * LX_WEB_ATTESTOR_ENTRY_BYTES,
    LX_WEB_PENDING_CAPACITY = 256,
    LX_WEB_STORE_CAPACITY = 64,
    LX_WEB_LEAF_BYTES = 213,
    LX_WEB_REQUEST_TOPIC_BYTES = 22,
    LX_WEB_REQUEST_RECORD_HEADER_BYTES = 13
};

/* One attested answer to a program web request. The fields up to
 * full_length are the origin-2 preimage fields; the preimage itself is
 * rebuilt from them and from keccak256 of the stored response. */
typedef struct lx_web_observation {
    uint8_t origin;
    uint32_t network_id;
    uint8_t program_id[32];
    uint64_t request_id;
    uint8_t kind;
    uint8_t payload_hash[32];
    uint8_t content_digest[32];
    uint32_t full_length;
    uint32_t response_length;
    uint8_t response[LX_WEB_MAX_RESPONSE_BYTES];
    size_t signature_count;
    uint8_t signatures[LX_WEB_MAX_ATTESTORS][LX_WEB_SIGNATURE_BYTES];
} lx_web_observation;

typedef struct lx_web_attestor {
    uint8_t signer[LX_WEB_SIGNER_BYTES];
    uint8_t payout_account[32];
} lx_web_attestor;

typedef struct lx_web_attestor_set {
    lx_web_attestor attestors[LX_WEB_MAX_ATTESTORS];
    size_t count;
    uint32_t threshold;
    uint64_t updated_sequence;
} lx_web_attestor_set;

typedef struct lx_web_pending_request {
    uint8_t program_id[32];
    uint64_t request_id;
    uint8_t kind;
    uint8_t payload_hash[32];
    uint64_t recorded_sequence;
    bool fulfilled;
} lx_web_pending_request;

typedef struct lx_web_committed {
    lx_web_observation observation;
    uint8_t attestation_digest[32];
    uint8_t signers[LX_WEB_MAX_ATTESTORS][LX_WEB_SIGNER_BYTES];
    size_t signer_count;
    uint64_t global_sequence;
} lx_web_committed;

typedef struct lx_web_store {
    uint32_t network_id;
    lx_web_pending_request pending[LX_WEB_PENDING_CAPACITY];
    size_t pending_count;
    lx_web_committed committed[LX_WEB_STORE_CAPACITY];
    size_t committed_count;
} lx_web_store;

typedef struct lx_web_intake_request {
    lx_web_store *store;
    const lx_web_attestor_set *attestors;
    const uint8_t *payload;
    size_t payload_length;
} lx_web_intake_request;

typedef struct lx_web_availability_bundle {
    uint8_t leaves[LX_WEB_STORE_CAPACITY][LX_WEB_LEAF_BYTES];
    size_t count;
} lx_web_availability_bundle;

typedef lxp_result (*lx_web_poll_fn)(void *context,
                                    lx_web_observation *observation,
                                    bool *available);
typedef lxp_result (*lx_web_submit_fn)(void *context,
                                      const uint8_t *activity,
                                      size_t activity_length);

typedef struct lx_web_adapter_config {
    lx_web_poll_fn poll_observations;
    void *poll_context;
    lx_web_submit_fn submit_activity;
    void *submit_context;
    uint8_t submitter_private_key[32];
    uint32_t network_id;
    const uint8_t *actor_did;
    size_t actor_did_length;
    uint64_t next_account_sequence;
    lxp_u128 fee_limit;
    lxp_timestamp_bound timestamp_bound;
    size_t maximum_observations;
} lx_web_adapter_config;

lxp_result lx_web_observation_encode(const lx_web_observation *observation,
                                     uint8_t *bytes, size_t capacity,
                                     size_t *length);
lxp_result lx_web_activity_encode(const lx_web_observation *observation,
                                  const uint8_t submitter_private_key[32],
                                  uint32_t network_id,
                                  const uint8_t *actor_did,
                                  size_t actor_did_length,
                                  uint64_t account_sequence,
                                  lxp_u128 fee_limit,
                                  lxp_timestamp_bound timestamp_bound,
                                  lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lx_web_adapter_run(lx_web_adapter_config *config,
                              size_t *submitted);

lxp_result lx_web_preimage_encode(uint8_t origin,
                                  const uint8_t network_id[32],
                                  const uint8_t requester[32],
                                  uint64_t request_id, uint8_t kind,
                                  const uint8_t payload_hash[32],
                                  const uint8_t content_digest[32],
                                  const uint8_t *response,
                                  size_t response_length,
                                  uint32_t full_length,
                                  uint8_t preimage[LX_WEB_PREIMAGE_BYTES]);
lxp_result lx_web_observation_digest(const lx_web_observation *observation,
                                     uint8_t digest[32]);
lxp_result lx_web_observation_decode(const uint8_t *bytes, size_t length,
                                     lx_web_observation *observation);
lxp_result lx_web_request_record_decode(const uint8_t *data, size_t length,
                                        uint64_t *request_id, uint8_t *kind,
                                        lxp_byte_span *payload);
lxp_result lx_web_pending_add(lx_web_store *store,
                              const uint8_t program_id[32],
                              uint64_t request_id, uint8_t kind,
                              const uint8_t *payload, size_t payload_length,
                              uint64_t recorded_sequence);
lxp_result lx_web_intake(lxp_module_ctx *ctx,
                         const lx_web_intake_request *request,
                         lx_web_committed *committed);
lxp_result lx_web_committed_lookup(const lx_web_store *store,
                                   const uint8_t program_id[32],
                                   uint64_t request_id,
                                   const lx_web_committed **committed);

lxp_result lx_web_attestor_set_validate(const lx_web_attestor_set *set);
lxp_result lx_web_attestor_set_encode(const lx_web_attestor_set *set,
                                      uint8_t *bytes, size_t capacity,
                                      size_t *length);
lxp_result lx_web_attestor_set_decode(const uint8_t *bytes, size_t length,
                                      lx_web_attestor_set *set);
lxp_result lx_web_attestor_lookup(const lx_web_attestor_set *set,
                                  const uint8_t signer[LX_WEB_SIGNER_BYTES],
                                  const lx_web_attestor **attestor);
lxp_result lx_web_attestor_set_execute(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lxp_authority_resolved *authority,
                                       lx_web_attestor_set *set);

lxp_result lx_web_leaf_encode(const lx_web_committed *committed,
                              uint8_t *bytes, size_t capacity,
                              size_t *length);
lxp_result lx_web_availability_bundle_build(
    const lx_web_store *store, lx_web_availability_bundle *bundle);
lxp_result lx_web_root_from_availability(
    const lx_web_availability_bundle *bundle, lxp_arena *arena,
    uint8_t root[32]);
lxp_result lx_web_root(const lx_web_store *store, lxp_arena *arena,
                       uint8_t root[32]);

#endif
