#ifndef LAYERX_LXP_PAXEER_H
#define LAYERX_LXP_PAXEER_H

#include "layerx/lxp_guarantor.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_PAXEER_CUSTODY_INPUT_COUNT = 6
};

typedef enum lxp_paxeer_custody_input_kind {
    LXP_PAXEER_INPUT_FINALISED_CHECKPOINT_CERTIFICATE = 1,
    LXP_PAXEER_INPUT_STATE_PROOF = 2,
    LXP_PAXEER_INPUT_WITHDRAWAL_NULLIFIER = 3,
    LXP_PAXEER_INPUT_GUARANTOR_SIGNATURES = 4,
    LXP_PAXEER_INPUT_CHALLENGE_WINDOW_STATE = 5,
    LXP_PAXEER_INPUT_EMERGENCY_EXIT_ELIGIBILITY = 6
} lxp_paxeer_custody_input_kind;

typedef struct lxp_paxeer_custody_abi {
    lxp_paxeer_custody_input_kind inputs[LXP_PAXEER_CUSTODY_INPUT_COUNT];
    size_t input_count;
} lxp_paxeer_custody_abi;
#define lxp_paxeer_custody_abi lxp_paxeer_custody_abi

typedef struct lxp_checkpoint_registry_state {
    lxp_finalisation_state finalisation;
    uint8_t checkpoint_id[32];
    uint8_t last_header_hash[32];
    uint64_t registration_count;
    size_t registered_header_length;
} lxp_checkpoint_registry_state;
#define lxp_checkpoint_registry_state lxp_checkpoint_registry_state

typedef struct lxp_paxeer_guarantor_attestation {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t paxeer_chain_id;
    uint8_t settlement_contract[20];
    uint64_t epoch;
    uint8_t checkpoint_id[32];
    uint8_t checkpoint_hash[32];
    uint8_t guarantor_id[32];
    uint64_t batch_number;
    uint8_t data_availability_root[32];
    bool replayed;
    bool data_available;
    uint8_t availability_class_mask;
    uint64_t attested_at_ms;
    uint8_t signer[20];
    uint8_t r[32];
    uint8_t s[32];
    uint8_t v;
} lxp_paxeer_guarantor_attestation;

typedef struct lxp_checkpoint_registration {
    lxp_batch_header header;
    lxp_byte_span header_commitments;
    lxp_byte_span validity_proof;
    lxp_paxeer_guarantor_attestation
        attestations[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t attestation_count;
    uint8_t checkpoint_id[32];
    uint8_t resulting_state_root[32];
    uint64_t batch_number;
} lxp_checkpoint_registration;

typedef struct lxp_paxeer_bond_state {
    lxp_guarantor_set guarantors;
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t paxeer_chain_id;
    uint8_t paxeer_settlement_contract[20];
    lxp_u128 custodied_value;
    lxp_u128 minimum_bond;
    uint32_t minimum_bond_bps;
    uint64_t mirror_version;
} lxp_paxeer_bond_state;
#define lxp_paxeer_bond_state lxp_paxeer_bond_state

typedef enum lxp_paxeer_membership_sync_availability {
    LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE = 1
} lxp_paxeer_membership_sync_availability;

lxp_result lxp_paxeer_custody_abi_init(lxp_paxeer_custody_abi *abi);
lxp_result lxp_paxeer_guarantor_attestation_from_core(
    const lxp_guarantor_attestation *source,
    lxp_paxeer_guarantor_attestation *target);
lxp_result lxp_paxeer_verify_cert(
    lxp_checkpoint_registry_state *state,
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *guarantor_set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, bool *finalised);
lxp_result lxp_checkpoint_register(
    lxp_checkpoint_registry_state *state,
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *guarantor_set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, lxp_checkpoint_registration *registration);
lxp_result lxp_paxeer_bond_init(lxp_paxeer_bond_state *state,
                                 uint16_t protocol_version,
                                 uint32_t network_id,
                                 uint64_t paxeer_chain_id,
                                 const uint8_t paxeer_contract[20],
                                 lxp_u128 custodied_value,
                                 uint32_t minimum_bond_bps);
lxp_result lxp_paxeer_membership_sync_status(
    const lxp_paxeer_bond_state *state,
    lxp_paxeer_membership_sync_availability *availability);
lxp_result lxp_paxeer_bond_deposit(
    lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32],
    lxp_u128 amount);
lxp_result lxp_paxeer_bond_state_read(
    const lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32],
    lxp_guarantor_bond_state *bond, bool *threshold_eligible);
lxp_result lxp_paxeer_slash_submit(
    lxp_paxeer_bond_state *state, const uint8_t *evidence_bytes,
    size_t evidence_length, const lxp_equivocation_evidence *evidence,
    lxp_arena *arena);

#endif
