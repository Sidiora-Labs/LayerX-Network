#ifndef LAYERX_LX_SERVICE_H
#define LAYERX_LX_SERVICE_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_SERVICE_STORE_CAPACITY = 128,
    LX_SERVICE_MAX_DELIVERABLES = 16,
    LX_SERVICE_OFFER_PUBLISH = 0x00050001,
    LX_SERVICE_OFFER_WITHDRAW = 0x00050002,
    LX_SERVICE_AGREEMENT_PROPOSE = 0x00050003,
    LX_SERVICE_AGREEMENT_ACCEPT = 0x00050004,
    LX_SERVICE_COMMIT_TASK = 0x00050005,
    LX_SERVICE_COMMIT_ABANDON = 0x00050006,
    LX_SERVICE_TOOL_EXEC_ATTEST = 0x00050007,
    LX_SERVICE_PROGRESS_REPORT = 0x00050008,
    LX_SERVICE_DELIVER = 0x00050009,
    LX_SERVICE_ACCEPT = 0x0005000a,
    LX_SERVICE_REJECT = 0x0005000b,
    LX_SERVICE_DISPUTE_OPEN = 0x0005000c,
    LX_SERVICE_DISPUTE_RESOLVE = 0x0005000d
};

typedef enum lx_service_default_outcome {
    LX_SERVICE_DEFAULT_ACCEPT = 1,
    LX_SERVICE_DEFAULT_REJECT = 2
} lx_service_default_outcome;

typedef enum lx_service_agreement_state {
    LX_SERVICE_AGREEMENT_FORMED = 1,
    LX_SERVICE_AGREEMENT_COMMITTED = 2,
    LX_SERVICE_AGREEMENT_DELIVERED = 3,
    LX_SERVICE_AGREEMENT_ACCEPTED = 4,
    LX_SERVICE_AGREEMENT_REJECTED = 5,
    LX_SERVICE_AGREEMENT_DISPUTED = 6,
    LX_SERVICE_AGREEMENT_RESOLVED = 7
} lx_service_agreement_state;

typedef struct lx_service_offer {
    uint8_t offer_id[32];
    uint8_t activity_id[32];
    uint8_t offering_agent[32];
    uint8_t asset_id[32];
    lxp_u128 price;
    uint8_t terms_hash[32];
    uint8_t deliverable_specification_hash[32];
    uint64_t delivery_deadline;
    uint64_t acceptance_window;
    uint64_t dispute_window;
    lx_service_default_outcome default_outcome;
    uint64_t offer_expiry;
    uint64_t global_sequence;
    bool withdrawn;
    bool accepted;
} lx_service_offer;

typedef struct lx_service_agreement {
    uint8_t agreement_id[32];
    uint8_t offer_id[32];
    uint8_t provider[32];
    uint8_t buyer[32];
    uint8_t terms_hash[32];
    uint8_t escrow_id[32];
    uint64_t delivery_deadline;
    uint64_t acceptance_window_end;
    uint64_t dispute_window_end;
    lx_service_default_outcome default_outcome;
    lx_service_agreement_state state;
    uint64_t accepted_sequence;
    uint16_t rejection_reason;
    uint8_t contested_hashes[LX_SERVICE_MAX_DELIVERABLES][32];
    size_t contested_hash_count;
    bool default_applied;
    uint64_t outcome_sequence;
    uint64_t outcome_timestamp;
} lx_service_agreement;

typedef struct lx_service_commitment {
    uint8_t commitment_id[32];
    uint8_t activity_id[32];
    uint8_t provider[32];
    uint8_t agreement_id[32];
    uint8_t task_hash[32];
    uint64_t deadline;
    uint64_t resource_bound;
    uint8_t escrow_id[32];
    uint64_t global_sequence;
    bool abandoned;
    uint16_t abandon_reason;
} lx_service_commitment;

typedef struct lx_service_attestor_grant {
    uint8_t principal[32];
    uint8_t public_key[32];
    uint16_t module_id;
    uint32_t activity_type;
    uint64_t not_before;
    uint64_t not_after;
    bool revoked;
} lx_service_attestor_grant;

typedef struct lx_service_execution {
    uint8_t attestation_id[32];
    uint8_t activity_id[32];
    uint8_t agreement_id[32];
    uint8_t commitment_id[32];
    uint8_t tool_id[32];
    uint8_t input_commitment_hash[32];
    uint8_t output_commitment_hash[32];
    uint64_t execution_start;
    uint64_t execution_end;
    uint64_t resource_units;
    uint8_t attestor_identity[32];
    uint8_t availability_reference[32];
    uint8_t public_key[32];
    uint8_t signature[64];
    uint64_t global_sequence;
    uint16_t canonical_payload_length;
    uint8_t canonical_payload[384];
} lx_service_execution;

typedef struct lx_service_deliverable {
    uint8_t hash[32];
    uint64_t artifact_size;
    uint8_t availability_reference[32];
} lx_service_deliverable;

typedef struct lx_service_delivery {
    uint8_t delivery_id[32];
    uint8_t activity_id[32];
    uint8_t agreement_id[32];
    uint8_t provider[32];
    lx_service_deliverable deliverables[LX_SERVICE_MAX_DELIVERABLES];
    size_t deliverable_count;
    uint64_t global_sequence;
} lx_service_delivery;

typedef struct lx_service_dispute {
    uint8_t dispute_id[32];
    uint8_t activity_id[32];
    uint8_t agreement_id[32];
    uint8_t raiser[32];
    uint8_t evidence_hashes[LX_SERVICE_MAX_DELIVERABLES][32];
    size_t evidence_hash_count;
    uint64_t global_sequence;
    bool resolved;
    uint16_t ruling;
    uint32_t provider_basis_points;
    uint8_t escrow_resolution_id[32];
    uint64_t resolution_sequence;
} lx_service_dispute;

typedef struct lx_service_store {
    lx_service_offer offers[LX_SERVICE_STORE_CAPACITY];
    size_t offer_count;
    lx_service_agreement agreements[LX_SERVICE_STORE_CAPACITY];
    size_t agreement_count;
    lx_service_commitment commitments[LX_SERVICE_STORE_CAPACITY];
    size_t commitment_count;
    lx_service_execution executions[LX_SERVICE_STORE_CAPACITY];
    size_t execution_count;
    lx_service_delivery deliveries[LX_SERVICE_STORE_CAPACITY];
    size_t delivery_count;
    lx_service_dispute disputes[LX_SERVICE_STORE_CAPACITY];
    size_t dispute_count;
} lx_service_store;

lxp_result lx_service_store_validate(const lx_service_store *store);

typedef struct lx_service_offer_request {
    lx_service_store *store;
    lx_service_offer offer;
    const lxp_authority_resolved *authority;
    bool attempts_balance_mutation;
} lx_service_offer_request;

typedef struct lx_service_agreement_request {
    lx_service_store *store;
    const uint8_t *offer_id;
    uint8_t agreement_id[32];
    uint8_t buyer[32];
    uint8_t terms_hash[32];
    uint8_t escrow_id[32];
    const lxp_authority_resolved *authority;
    bool attempts_balance_mutation;
} lx_service_agreement_request;

typedef struct lx_service_commit_request {
    lx_service_store *store;
    lx_service_commitment commitment;
    const lxp_authority_resolved *authority;
    bool attempts_balance_mutation;
    uint16_t abandon_reason;
} lx_service_commit_request;

typedef struct lx_service_attest_request {
    lx_service_store *store;
    lx_service_execution execution;
    const lx_service_attestor_grant *grant;
    bool attempts_balance_mutation;
} lx_service_attest_request;

typedef struct lx_service_delivery_request {
    lx_service_store *store;
    lx_service_delivery delivery;
    const lxp_authority_resolved *authority;
    bool attempts_balance_mutation;
} lx_service_delivery_request;

typedef struct lx_service_outcome_request {
    lx_service_store *store;
    const uint8_t *agreement_id;
    const lxp_authority_resolved *authority;
    uint16_t rejection_reason;
    uint8_t contested_hashes[LX_SERVICE_MAX_DELIVERABLES][32];
    size_t contested_hash_count;
    bool attempts_balance_mutation;
} lx_service_outcome_request;

typedef struct lx_service_runtime {
    lx_service_store *store;
} lx_service_runtime;

typedef struct lx_service_dispute_request {
    lx_service_store *store;
    lx_service_dispute dispute;
    const lxp_authority_resolved *authority;
    bool attempts_balance_mutation;
} lx_service_dispute_request;

const lxp_module_iface *lx_service_module_iface(void);
lxp_result lx_service_offer_lookup(lx_service_store *store,
                                   const uint8_t offer_id[32],
                                   lx_service_offer **offer);
lxp_result lx_service_agreement_lookup(lx_service_store *store,
                                       const uint8_t agreement_id[32],
                                       lx_service_agreement **agreement);
lxp_result lx_service_offer_publish_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request);
lxp_result lx_service_offer_withdraw_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request);
lxp_result lx_service_agreement_accept_execute(
    lxp_module_ctx *ctx, const lx_service_agreement_request *request);
lxp_result lx_service_commitment_put(lx_service_store *store,
                                     const lx_service_commitment *commitment);
lxp_result lx_service_commit_task_execute(
    lxp_module_ctx *ctx, const lx_service_commit_request *request,
    lx_service_commitment *result);
lxp_result lx_service_commit_abandon_execute(
    lxp_module_ctx *ctx, const lx_service_commit_request *request,
    lx_service_commitment *result);
lxp_result lx_service_execution_encode(const lx_service_execution *execution,
                                       uint8_t *bytes, size_t capacity,
                                       size_t *length);
lxp_result lx_service_execution_decode(const uint8_t *bytes, size_t length,
                                       lx_service_execution *execution);
lxp_result lx_service_attestation_bytes(
    const lx_service_execution *execution, uint8_t *bytes, size_t capacity,
    size_t *length);
lxp_result lx_service_attestor_verify(
    const lx_service_store *store, const lx_service_execution *execution,
    const lx_service_attestor_grant *grant, uint64_t batch_timestamp);
lxp_result lx_service_execution_put(lx_service_store *store,
                                    const lx_service_execution *execution);
lxp_result lx_service_tool_exec_attest_execute(
    lxp_module_ctx *ctx, const lx_service_attest_request *request,
    lx_service_execution *result);
lxp_result lx_service_deliverable_check(
    const lx_service_store *store, const lx_service_agreement *agreement,
    const lx_service_delivery *delivery);
lxp_result lx_service_delivery_put(lx_service_store *store,
                                   const lx_service_delivery *delivery);
lxp_result lx_service_deliver_execute(
    lxp_module_ctx *ctx, const lx_service_delivery_request *request,
    lx_service_delivery *result);
lxp_result lx_service_accept_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request);
lxp_result lx_service_reject_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request);
lxp_result lx_service_acceptance_default(lx_service_store *store,
                                         uint64_t batch_timestamp,
                                         uint64_t global_sequence);
lxp_result lx_service_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                  uint64_t timestamp);
lxp_result lx_service_dispute_open_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result);
lxp_result lx_service_dispute_resolve_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result);
lxp_result lx_service_effect_audit(uint32_t activity_type,
                                   const lxp_effect_buffer *effects);

#endif
