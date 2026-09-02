#ifndef LAYERX_LXP_GUARANTOR_H
#define LAYERX_LXP_GUARANTOR_H

#include "layerx/lxp_activity.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_replica.h"
#include "layerx/lxp_sequencer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_GUARANTOR_DUTY_NONE = 0,
    LXP_GUARANTOR_DUTY_DOWNLOADED = 1,
    LXP_GUARANTOR_DUTY_SIGNATURES = 2,
    LXP_GUARANTOR_DUTY_REPLAYED = 3,
    LXP_GUARANTOR_DUTY_ROOTS = 4,
    LXP_GUARANTOR_DUTY_STORED = 5,
    LXP_GUARANTOR_DUTY_READY_TO_SIGN = 6,
    LXP_MAX_GUARANTOR_ATTESTATIONS = 32,
    LXP_MAX_VALIDITY_PROOF_BYTES = 1048576,
    LXP_MAX_SETTLEMENT_REFERENCE_BYTES = 1024,
    LXP_GUARANTOR_AVAILABILITY_ALL = 0x1f,
    LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS = 32,
    LXP_MAX_DA_CHALLENGE_INDICES = 16,
    LXP_MAX_DA_CHALLENGE_RECORDS = 128
};

typedef struct lxp_guarantor_bond_view {
    lxp_u128 bonded_amount;
    uint64_t epoch;
    bool bonded;
} lxp_guarantor_bond_view;

typedef struct lxp_guarantor_authority_verdict {
    bool actor_signature;
    bool session_key;
    bool capability_grant;
    bool delegated_authority;
} lxp_guarantor_authority_verdict;

typedef lxp_result (*lxp_guarantor_download_fn)(
    void *context, uint64_t batch_number, lxp_arena *arena,
    lxp_byte_span *canonical_body);
typedef lxp_result (*lxp_guarantor_authority_verify_fn)(
    void *context, const lxp_activity *activity,
    lxp_byte_span canonical_activity,
    lxp_guarantor_authority_verdict *verdict);
typedef lxp_result (*lxp_guarantor_oracle_verify_fn)(
    void *context, lxp_byte_span canonical_oracle, bool *valid);
typedef lxp_result (*lxp_guarantor_store_fn)(
    void *context, uint64_t batch_number, const uint8_t *canonical_body,
    size_t body_length);

typedef enum lxp_guarantor_divergence_component {
    LXP_GUARANTOR_DIVERGENCE_SIGNATURE = 1,
    LXP_GUARANTOR_DIVERGENCE_STATE_ROOT = 2,
    LXP_GUARANTOR_DIVERGENCE_RESULT_CODE = 3,
    LXP_GUARANTOR_DIVERGENCE_FEE = 4,
    LXP_GUARANTOR_DIVERGENCE_EFFECTS = 5,
    LXP_GUARANTOR_DIVERGENCE_BALANCE = 6,
    LXP_GUARANTOR_DIVERGENCE_RECEIPT = 7,
    LXP_GUARANTOR_DIVERGENCE_EVENTS = 8
} lxp_guarantor_divergence_component;

typedef struct lxp_guarantor_divergence {
    uint64_t batch_number;
    uint64_t global_sequence;
    lxp_guarantor_divergence_component component;
    uint8_t expected_hash[32];
    uint8_t produced_hash[32];
} lxp_guarantor_divergence;

typedef struct lxp_guarantor_dissent_record {
    uint8_t guarantor_id[32];
    uint64_t epoch;
    lxp_guarantor_divergence divergence;
    uint8_t signature[64];
} lxp_guarantor_dissent_record;

typedef lxp_result (*lxp_guarantor_dissent_publish_fn)(
    void *context, const lxp_guarantor_dissent_record *dissent);

typedef struct lxp_guarantor_ctx {
    uint8_t guarantor_id[32];
    uint8_t paxeer_private_key[32];
    uint8_t paxeer_public_key[33];
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t paxeer_chain_id;
    uint8_t paxeer_settlement_contract[20];
    lxp_guarantor_bond_view bond_view;
    uint8_t independent_state_root[32];
    lxp_replay_engine *replay_engine;
    const lxp_sequencer_authorization *sequencer_authorization;
    lxp_guarantor_download_fn download;
    void *download_context;
    lxp_guarantor_authority_verify_fn verify_authority;
    void *authority_context;
    lxp_guarantor_oracle_verify_fn verify_oracle;
    void *oracle_context;
    lxp_guarantor_store_fn store_availability;
    void *storage_context;
    lxp_guarantor_dissent_publish_fn publish_dissent;
    void *dissent_context;
    uint64_t attestation_halted_epoch;
    uint8_t last_completed_duty;
    bool possesses_availability;
    bool ready_to_sign;
} lxp_guarantor_ctx;
#define lxp_guarantor_ctx lxp_guarantor_ctx

typedef struct lxp_checkpoint_certificate {
    lxp_batch_header header;
    lxp_byte_span validity_proof;
} lxp_checkpoint_certificate;

typedef struct lxp_guarantor_attestation {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t paxeer_chain_id;
    uint8_t paxeer_settlement_contract[20];
    uint64_t epoch;
    uint8_t checkpoint_id[32];
    uint8_t checkpoint_hash[32];
    uint8_t guarantor_id[32];
    uint64_t batch_number;
    uint8_t data_availability_root[32];
    bool replayed;
    bool da_possessed;
    uint8_t availability_class_mask;
    uint64_t attested_at_ms;
    uint8_t signer[20];
    uint8_t signature[64];
    uint8_t signature_v;
} lxp_guarantor_attestation;
#define lxp_guarantor_attestation lxp_guarantor_attestation

typedef struct lxp_da_challenge {
    uint8_t challenge_id[32];
    uint8_t checkpoint_hash[32];
    lxp_guarantor_attestation signed_commitment;
    uint64_t batch_number;
    uint32_t chunk_index;
    uint32_t chunk_count;
    uint64_t issued_at_ms;
    uint64_t deadline_ms;
} lxp_da_challenge;

typedef struct lxp_da_challenge_response {
    uint8_t challenge_id[32];
    lxp_da_chunk chunk;
    lxp_merkle_proof inclusion_proof;
    uint64_t responded_at_ms;
} lxp_da_challenge_response;

typedef struct lxp_da_failure_evidence {
    lxp_da_challenge challenge;
    lxp_byte_span served_bytes;
    uint8_t served_chunk_hash[32];
    lxp_result failure_code;
} lxp_da_failure_evidence;

typedef struct lxp_da_challenge_record {
    uint8_t challenge_id[32];
    uint8_t guarantor_id[32];
    uint64_t batch_number;
    lxp_result outcome;
    bool answered;
    bool slashable;
} lxp_da_challenge_record;

typedef lxp_result (*lxp_da_evidence_publish_fn)(
    void *context, const lxp_da_failure_evidence *evidence);

typedef struct lxp_da_challenge_registry {
    lxp_da_challenge_record records[LXP_MAX_DA_CHALLENGE_RECORDS];
    size_t count;
    lxp_da_evidence_publish_fn publish_evidence;
    void *publish_context;
} lxp_da_challenge_registry;

typedef struct lxp_guarantor_cert {
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation
        attestations[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t attestation_count;
    size_t threshold;
    bool bonded_economic_guarantee;
    bool validity_proof_present;
} lxp_guarantor_cert;
#define lxp_guarantor_cert lxp_guarantor_cert

typedef struct lxp_guarantor_key_record {
    uint8_t guarantor_id[32];
    uint8_t public_key[33];
    bool bonded;
} lxp_guarantor_key_record;

typedef struct lxp_augmented_receipt {
    lxp_byte_span pre_checkpoint_receipt;
    lxp_byte_span canonical_activity;
    lxp_byte_span state_leaf;
    lxp_merkle_proof activity_inclusion_proof;
    lxp_merkle_proof state_inclusion_proof;
    uint8_t checkpoint_id[32];
    const lxp_guarantor_cert *guarantor_certificate;
    lxp_byte_span paxeer_settlement_reference;
} lxp_augmented_receipt;

typedef struct lxp_guarantor_signer_authorization {
    uint8_t public_key[33];
    uint64_t active_from_epoch;
    uint64_t active_until_epoch;
    uint64_t set_version;
} lxp_guarantor_signer_authorization;

typedef struct lxp_guarantor_bond_state {
    uint8_t guarantor_id[32];
    uint8_t public_key[33];
    lxp_u128 bond_amount;
    uint64_t joined_epoch;
    uint64_t removed_epoch;
    uint64_t ejected_at_version;
    lxp_guarantor_signer_authorization
        signer_authorizations[LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS];
    size_t signer_authorization_count;
    bool jailed;
    bool unresolved_slashing;
    bool active;
} lxp_guarantor_bond_state;
#define lxp_guarantor_bond_state lxp_guarantor_bond_state

typedef struct lxp_guarantor_set {
    uint64_t version;
    uint64_t last_governance_sequence;
    lxp_guarantor_bond_state records[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t count;
} lxp_guarantor_set;
#define lxp_guarantor_set lxp_guarantor_set

typedef struct lxp_finalisation_state {
    uint8_t settlement_anchor[32];
    uint64_t finalized_batch_number;
    uint64_t blocked_checkpoint_batch_number;
    uint64_t availability_incident_batch_number;
    bool checkpoint_finalized;
    bool withdrawal_settlement_enabled;
    bool deposit_settlement_enabled;
    bool dispute_settlement_enabled;
    bool pending_withdrawal_settlement_enabled;
    bool pending_deposit_settlement_enabled;
    bool pending_dispute_settlement_enabled;
    bool unfinalized_checkpoint_blocked;
    bool finalisation_halted;
    bool emergency_data_mode;
    bool emergency_exit_enabled;
} lxp_finalisation_state;

typedef struct lxp_finalisation_requirements {
    uint64_t checkpoint_epoch;
    uint64_t challenge_window_end_ms;
    uint64_t checkpoint_deadline_ms;
    uint64_t now_ms;
    size_t threshold;
    lxp_u128 minimum_bond;
    bool availability_challenges_answered;
    bool equivocation_detected;
} lxp_finalisation_requirements;

typedef enum lxp_equivocation_kind {
    LXP_EQUIVOCATION_GUARANTOR = 1,
    LXP_EQUIVOCATION_SEQUENCER = 2
} lxp_equivocation_kind;

typedef struct lxp_equivocation_evidence {
    lxp_equivocation_kind kind;
    uint8_t offender_public_key[33];
    size_t offender_public_key_length;
    lxp_guarantor_attestation guarantor_first;
    lxp_guarantor_attestation guarantor_second;
    lxp_sealed_header_record sequencer_first;
    lxp_sealed_header_record sequencer_second;
} lxp_equivocation_evidence;
#define lxp_equivocation_evidence lxp_equivocation_evidence

lxp_result lxp_guarantor_verify_signatures(lxp_guarantor_ctx *ctx,
                                           const lxp_batch_body *body,
                                           lxp_arena *arena);
lxp_result lxp_guarantor_recompute_roots(
    const lxp_batch_body *body, const lxp_replay_batch_result *replay,
    lxp_arena *arena, lxp_batch_roots *roots);
lxp_result lxp_guarantor_process_batch(lxp_guarantor_ctx *ctx,
                                       uint64_t batch_number,
                                       lxp_arena *arena,
                                       bool *ready_to_sign);
lxp_result lxp_checkpoint_certificate_hash(
    const lxp_checkpoint_certificate *checkpoint, lxp_arena *arena,
    uint8_t checkpoint_hash[32]);
lxp_result lxp_guarantor_attest(
    const lxp_guarantor_ctx *ctx,
    const lxp_checkpoint_certificate *checkpoint, bool replayed,
    bool da_possessed, uint64_t attested_at_ms, lxp_arena *arena,
    lxp_guarantor_attestation *attestation);
lxp_result lxp_guarantor_attestation_verify(
    const lxp_guarantor_attestation *attestation,
    const uint8_t public_key[33]);
lxp_result lxp_guarantor_cert_assemble(
    const lxp_checkpoint_certificate *checkpoint,
    const lxp_guarantor_attestation *attestations, size_t attestation_count,
    size_t threshold, lxp_guarantor_cert *certificate);
uint64_t lxp_checkpoint_maximum_attestation_delay_ms(void);
lxp_result lxp_guarantor_cert_verify(
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_key_record *keys, size_t key_count,
    lxp_arena *arena, size_t *valid_signatures);
lxp_result lxp_receipt_augment(
    lxp_byte_span pre_checkpoint_receipt,
    lxp_byte_span canonical_activity,
    lxp_byte_span state_leaf,
    const lxp_merkle_proof *activity_inclusion_proof,
    const lxp_merkle_proof *state_inclusion_proof,
    const lxp_guarantor_cert *guarantor_certificate,
    lxp_byte_span paxeer_settlement_reference,
    lxp_augmented_receipt *augmented);
lxp_result lxp_guarantor_set_init(lxp_guarantor_set *set);
lxp_result lxp_guarantor_set_validate(const lxp_guarantor_set *set);
lxp_result lxp_guarantor_set_apply(
    lxp_guarantor_set *set, uint64_t governance_sequence,
    bool ordered_governance_activity,
    const lxp_guarantor_bond_state *bond_state);
lxp_result lxp_guarantor_set_rotate_signer(
    lxp_guarantor_set *set, uint64_t governance_sequence,
    bool ordered_governance_activity, const uint8_t guarantor_id[32],
    const uint8_t public_key[33], uint64_t activation_epoch);
lxp_result lxp_guarantor_signer_authorized(
    const lxp_guarantor_bond_state *bond_state,
    const uint8_t public_key[33], uint64_t checkpoint_epoch,
    bool *authorized);
lxp_result lxp_guarantor_signer_at_epoch(
    const lxp_guarantor_bond_state *bond_state, uint64_t checkpoint_epoch,
    uint8_t public_key[33]);
lxp_result lxp_guarantor_eligible(
    const lxp_guarantor_bond_state *bond_state, uint64_t checkpoint_epoch,
    lxp_u128 minimum_bond, bool *eligible);
lxp_result lxp_checkpoint_finalisable(
    lxp_finalisation_state *state, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, bool *finalisable);
lxp_result lxp_checkpoint_block_on_da(lxp_finalisation_state *state,
                                      uint64_t checkpoint_batch_number,
                                      bool data_available);
lxp_result lxp_da_unavailable_mode(lxp_finalisation_state *state,
                                   uint64_t affected_batch_number,
                                   bool data_available,
                                   bool governance_reconstituted);
lxp_result lxp_equivocation_detect(
    lxp_equivocation_kind kind, const void *first_statement,
    const void *second_statement, const uint8_t *offender_public_key,
    size_t offender_public_key_length,
    lxp_equivocation_evidence *evidence);
lxp_result lxp_equivocation_encode(
    const lxp_equivocation_evidence *evidence, lxp_arena *arena,
    lxp_byte_span *encoded);
lxp_result lxp_equivocation_verify(
    const lxp_equivocation_evidence *evidence, lxp_arena *arena);
lxp_result lxp_slashing_submit(
    const lxp_equivocation_evidence *evidence, lxp_guarantor_set *set,
    lxp_arena *arena);
lxp_result lxp_guarantor_first_divergence(
    uint64_t batch_number, uint64_t first_sequence,
    const lxp_replay_batch_result *published,
    const lxp_replay_batch_result *recomputed,
    lxp_guarantor_divergence *divergence);
lxp_result lxp_guarantor_dissent(
    const lxp_guarantor_ctx *ctx, uint64_t epoch,
    const lxp_guarantor_divergence *divergence,
    lxp_guarantor_dissent_record *dissent);
lxp_result lxp_guarantor_dissent_verify(
    const lxp_guarantor_dissent_record *dissent,
    const uint8_t public_key[33]);
lxp_result lxp_guarantor_withhold(
    lxp_guarantor_ctx *ctx, uint64_t epoch,
    const lxp_guarantor_divergence *divergence,
    lxp_guarantor_dissent_record *dissent);
lxp_result lxp_da_challenge_indices(
    const uint8_t checkpoint_hash[32], uint32_t chunk_count,
    size_t requested_count, uint32_t *indices);
lxp_result lxp_da_challenge_issue(
    const lxp_guarantor_attestation *attestation,
    const uint8_t checkpoint_hash[32], uint32_t chunk_index,
    uint32_t chunk_count, uint64_t issued_at_ms, uint64_t response_window_ms,
    lxp_da_challenge *challenge);
lxp_result lxp_da_challenge_respond(
    const lxp_da_store *store, const lxp_da_challenge *challenge,
    uint64_t responded_at_ms, lxp_arena *arena,
    lxp_da_challenge_response *response);
lxp_result lxp_da_challenge_judge(
    const lxp_da_challenge *challenge,
    const lxp_da_challenge_response *response, uint64_t judged_at_ms,
    bool *satisfied, lxp_da_failure_evidence *evidence);
lxp_result lxp_da_challenge_registry_init(
    lxp_da_challenge_registry *registry,
    lxp_da_evidence_publish_fn publish_evidence, void *publish_context);
lxp_result lxp_da_challenge_record_success(
    lxp_da_challenge_registry *registry,
    const lxp_da_challenge *challenge);
lxp_result lxp_da_challenge_record_failure(
    lxp_da_challenge_registry *registry,
    const lxp_da_failure_evidence *evidence, lxp_guarantor_set *set);

#endif
