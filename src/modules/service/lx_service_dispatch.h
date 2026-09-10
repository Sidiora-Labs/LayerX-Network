#ifndef LAYERX_SRC_MODULES_SERVICE_DISPATCH_H
#define LAYERX_SRC_MODULES_SERVICE_DISPATCH_H

#include "layerx/lx_service.h"

#include <stddef.h>
#include <stdint.h>

typedef struct lx_service_offer_publish_payload {
    uint8_t offer_id[32];
    uint8_t asset_id[32];
    lxp_u128 price;
    uint8_t terms_hash[32];
    uint8_t deliverable_specification_hash[32];
    uint64_t delivery_deadline;
    uint64_t acceptance_window;
    uint64_t dispute_window;
    lx_service_default_outcome default_outcome;
    uint64_t offer_expiry;
} lx_service_offer_publish_payload;

typedef struct lx_service_identifier_payload {
    uint8_t identifier[32];
} lx_service_identifier_payload;

typedef struct lx_service_agreement_propose_payload {
    uint8_t agreement_id[32];
    uint8_t offer_id[32];
    uint8_t terms_hash[32];
    uint8_t escrow_id[32];
} lx_service_agreement_propose_payload;

typedef struct lx_service_commit_task_payload {
    uint8_t commitment_id[32];
    uint8_t agreement_id[32];
    uint8_t task_hash[32];
    uint8_t escrow_id[32];
    uint64_t deadline;
    uint64_t resource_bound;
} lx_service_commit_task_payload;

typedef struct lx_service_commit_abandon_payload {
    uint8_t commitment_id[32];
    uint16_t abandon_reason;
} lx_service_commit_abandon_payload;

typedef struct lx_service_progress_payload {
    uint8_t report_id[32];
    uint8_t commitment_id[32];
    uint8_t note_hash[32];
    uint8_t availability_reference[32];
    uint32_t progress_bps;
} lx_service_progress_payload;

typedef struct lx_service_deliver_payload {
    uint8_t delivery_id[32];
    uint8_t agreement_id[32];
    lx_service_deliverable deliverables[LX_SERVICE_MAX_DELIVERABLES];
    size_t deliverable_count;
} lx_service_deliver_payload;

typedef struct lx_service_reject_payload {
    uint8_t agreement_id[32];
    uint16_t rejection_reason;
    uint8_t contested_hashes[LX_SERVICE_MAX_DELIVERABLES][32];
    size_t contested_hash_count;
} lx_service_reject_payload;

typedef struct lx_service_dispute_open_payload {
    uint8_t dispute_id[32];
    uint8_t agreement_id[32];
    uint8_t evidence_hashes[LX_SERVICE_MAX_DELIVERABLES][32];
    size_t evidence_hash_count;
} lx_service_dispute_open_payload;

typedef struct lx_service_dispute_resolve_payload {
    uint8_t dispute_id[32];
    uint16_t ruling;
    uint32_t provider_basis_points;
    uint8_t escrow_resolution_id[32];
} lx_service_dispute_resolve_payload;

typedef union lx_service_payload {
    lx_service_offer_publish_payload offer_publish;
    lx_service_identifier_payload identifier;
    lx_service_agreement_propose_payload agreement_propose;
    lx_service_commit_task_payload commit_task;
    lx_service_commit_abandon_payload commit_abandon;
    lx_service_progress_payload progress;
    lx_service_deliver_payload deliver;
    lx_service_reject_payload reject;
    lx_service_dispute_open_payload dispute_open;
    lx_service_dispute_resolve_payload dispute_resolve;
    lx_service_execution execution;
} lx_service_payload;

typedef struct lx_service_decoded {
    uint16_t ordinal;
    size_t payload_length;
    lx_service_payload payload;
} lx_service_decoded;

void lx_service_put_u16(uint8_t bytes[2], uint16_t value);
uint16_t lx_service_get_u16(const uint8_t bytes[2]);
void lx_service_put_u32(uint8_t bytes[4], uint32_t value);
uint32_t lx_service_get_u32(const uint8_t bytes[4]);
void lx_service_put_u64(uint8_t bytes[8], uint64_t value);
uint64_t lx_service_get_u64(const uint8_t bytes[8]);

lxp_result lx_service_key(const uint8_t prefix[LX_SERVICE_KEY_PREFIX_BYTES],
                          const uint8_t identifier[32],
                          uint8_t key[LX_SERVICE_KEY_BYTES]);

extern const uint8_t lx_service_offer_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_agreement_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_commitment_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_execution_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_delivery_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_deliverable_prefix[
    LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_dispute_prefix[LX_SERVICE_KEY_PREFIX_BYTES];
extern const uint8_t lx_service_progress_prefix[LX_SERVICE_KEY_PREFIX_BYTES];

lxp_result lx_service_hashes_check(
    const uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32], size_t count);
void lx_service_hashes_sort(uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32],
                            size_t count);

lxp_result lx_service_payload_decode(uint16_t ordinal, const uint8_t *payload,
                                     size_t length,
                                     lx_service_decoded *decoded);

lxp_result lx_service_emit(lxp_module_ctx *ctx, uint16_t event_type,
                           const uint8_t primary[32],
                           const uint8_t secondary[32], uint8_t code,
                           uint64_t sequence);

#endif
