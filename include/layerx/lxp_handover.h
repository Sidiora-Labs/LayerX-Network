#ifndef LAYERX_LXP_HANDOVER_H
#define LAYERX_LXP_HANDOVER_H

#include "layerx/lxp_batch.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_storage.h"

enum {
    LXP_GOVERNANCE_HANDOVER = 0x00070009,
    LXP_HANDOVER_CERTIFICATE_BYTES = 392,
    LXP_HANDOVER_MAX_EVIDENCE_BYTES = 1048576,
    LXP_HANDOVER_MAX_RECOVERY_BYTES = 16777216
};

typedef struct lxp_handover_certificate {
    uint32_t network_id;
    uint16_t protocol_version;
    uint64_t old_epoch;
    uint64_t new_epoch;
    uint8_t old_sequencer_id[32];
    uint8_t old_public_key[32];
    uint8_t new_sequencer_id[32];
    uint8_t new_public_key[32];
    uint64_t predecessor_batch;
    uint64_t predecessor_last_sequence;
    uint8_t predecessor_header_hash[32];
    uint8_t predecessor_state_root[32];
    uint8_t predecessor_checkpoint_id[32];
    uint64_t activation_batch;
    uint8_t finality_evidence_digest[32];
    uint8_t governance_signature[64];
} lxp_handover_certificate;

typedef struct lxp_handover_evidence {
    lxp_handover_certificate certificate;
    lxp_byte_span predecessor_header;
    uint8_t predecessor_signature[64];
    lxp_byte_span checkpoint_payload;
    lxp_byte_span finality_proof;
} lxp_handover_evidence;

struct lxp_kernel;
struct lxp_module_ctx;
typedef lxp_result (*lxp_handover_finality_verify_fn)(void *context,
    const lxp_handover_evidence *evidence, lxp_arena *arena);

typedef struct lxp_handover_state {
    bool enabled;
    uint8_t governance_public_key[32];
    uint32_t network_id;
    lxp_sequencer_authorization genesis_authorization;
    lxp_handover_finality_verify_fn verify_finality;
    void *finality_context;
    bool pending;
    lxp_handover_certificate pending_certificate;
    uint8_t pending_evidence_digest[32];
} lxp_handover_state;

enum { LXP_HANDOVER_MAX_TRANSITIONS = 512 };
typedef struct lxp_handover_trust_chain {
    uint32_t network_id;
    uint8_t governance_public_key[32];
    lxp_sequencer_authorization genesis_authorization;
    lxp_sequencer_authorization current_authorization;
    lxp_batch_header predecessor;
    uint8_t predecessor_signature[64];
    uint64_t epoch;
    lxp_handover_certificate transitions[LXP_HANDOVER_MAX_TRANSITIONS];
    uint8_t evidence_digests[LXP_HANDOVER_MAX_TRANSITIONS][32];
    size_t transition_count;
} lxp_handover_trust_chain;

typedef lxp_result (*lxp_handover_trust_finality_fn)(void *context,
    const lxp_batch_header *predecessor, const uint8_t signature[64],
    const lxp_handover_evidence *evidence, lxp_arena *arena);

lxp_result lxp_handover_trust_initialize(lxp_handover_trust_chain *chain,
    const lxp_genesis_manifest *manifest);
lxp_result lxp_handover_trust_accept(lxp_handover_trust_chain *chain,
    const lxp_batch_body *body, lxp_handover_trust_finality_fn verify_finality,
    void *context, lxp_arena *arena);
lxp_result lxp_handover_trust_scan_log(lxp_handover_trust_chain *chain,
    const lxp_log *log, uint64_t *offset,
    lxp_handover_trust_finality_fn verify_finality, void *context, lxp_arena *arena);
lxp_result lxp_handover_trust_authorization(const lxp_handover_trust_chain *chain,
    uint64_t batch_number, lxp_sequencer_authorization *authorization, uint64_t *epoch);
lxp_result lxp_handover_trust_authorization_sequence(const lxp_handover_trust_chain *chain,
    uint64_t global_sequence, lxp_sequencer_authorization *authorization, uint64_t *epoch);
lxp_result lxp_handover_trust_matches_kernel(const lxp_handover_trust_chain *chain,
    const struct lxp_kernel *kernel);

lxp_result lxp_handover_sequencer_id(const uint8_t public_key[32],
                                    uint8_t identifier[32]);
lxp_result lxp_handover_genesis_authority(const lxp_genesis_manifest *manifest,
                                         uint8_t public_key[32], bool *present);
lxp_result lxp_handover_certificate_encode(const lxp_handover_certificate *certificate,
    uint8_t bytes[LXP_HANDOVER_CERTIFICATE_BYTES]);
lxp_result lxp_handover_certificate_decode(lxp_byte_span encoded,
                                           lxp_handover_certificate *certificate);
lxp_result lxp_handover_certificate_sign(lxp_handover_certificate *certificate,
                                         const uint8_t private_key[32]);
lxp_result lxp_handover_certificate_verify(const lxp_handover_certificate *certificate,
                                           const uint8_t public_key[32]);
lxp_result lxp_handover_finality_digest(lxp_byte_span checkpoint_payload,
                                        lxp_byte_span finality_proof,
                                        uint8_t digest[32]);
lxp_result lxp_handover_evidence_encode(const lxp_handover_evidence *evidence,
                                        lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_handover_evidence_decode(lxp_byte_span encoded,
                                        lxp_handover_evidence *evidence);
lxp_result lxp_handover_evidence_verify_binding(const lxp_handover_evidence *evidence,
    const uint8_t governance_public_key[32], const lxp_sequencer_authorization *previous,
    uint64_t previous_epoch, uint64_t previous_batch, uint64_t next_sequence,
    const uint8_t previous_state_root[32], lxp_arena *arena);
bool lxp_handover_recovery_is_envelope(lxp_byte_span encoded);
lxp_result lxp_handover_recovery_encode(lxp_byte_span canonical_recovery,
    lxp_byte_span canonical_evidence, lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_handover_recovery_decode(lxp_byte_span encoded,
    lxp_byte_span *canonical_recovery, lxp_byte_span *canonical_evidence);
lxp_result lxp_handover_kernel_initialize(struct lxp_kernel *kernel,
    const lxp_genesis_manifest *manifest, lxp_handover_finality_verify_fn verify,
    void *context);
lxp_result lxp_handover_history_resolve(const struct lxp_kernel *kernel,
    uint64_t batch_number, lxp_sequencer_authorization *authorization,
    uint64_t *epoch, lxp_arena *arena);
lxp_result lxp_handover_history_resolve_sequence(const struct lxp_kernel *kernel,
    uint64_t global_sequence, lxp_sequencer_authorization *authorization,
    uint64_t *epoch, lxp_arena *arena);
lxp_result lxp_handover_history_latest(const struct lxp_kernel *kernel,
    lxp_byte_span *evidence);
lxp_result lxp_handover_prepare(struct lxp_kernel *kernel,
    const lxp_activity *activity, uint64_t batch_number, lxp_arena *arena);
lxp_result lxp_handover_stage(struct lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority);
lxp_result lxp_handover_incoming(const struct lxp_kernel *kernel,
    const lxp_batch_body *body, lxp_arena *arena,
    lxp_sequencer_authorization *authorization, lxp_handover_state *prospective);

#endif
