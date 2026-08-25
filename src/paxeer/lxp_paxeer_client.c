#include "layerx/lxp_paxeer.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

lxp_result lxp_paxeer_guarantor_attestation_from_core(
    const lxp_guarantor_attestation *source,
    lxp_paxeer_guarantor_attestation *target)
{
    if (source == NULL || target == NULL ||
        lxp_ct_is_zero(source->signer, 20U) ||
        (source->signature_v != 27U && source->signature_v != 28U) ||
        memcmp(source->checkpoint_id, source->checkpoint_hash, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(target, 0, sizeof(*target));
    target->protocol_version = source->protocol_version;
    target->network_id = source->network_id;
    target->paxeer_chain_id = source->paxeer_chain_id;
    (void)memcpy(target->settlement_contract,
                 source->paxeer_settlement_contract, 20U);
    target->epoch = source->epoch;
    (void)memcpy(target->checkpoint_id, source->checkpoint_id, 32U);
    (void)memcpy(target->checkpoint_hash, source->checkpoint_hash, 32U);
    (void)memcpy(target->guarantor_id, source->guarantor_id, 32U);
    target->batch_number = source->batch_number;
    (void)memcpy(target->data_availability_root,
                 source->data_availability_root, 32U);
    target->replayed = source->replayed;
    target->data_available = source->da_possessed;
    target->availability_class_mask = source->availability_class_mask;
    target->attested_at_ms = source->attested_at_ms;
    (void)memcpy(target->signer, source->signer, 20U);
    (void)memcpy(target->r, source->signature, 32U);
    (void)memcpy(target->s, source->signature + 32U, 32U);
    target->v = source->signature_v;
    return LXP_OK;
}

lxp_result lxp_paxeer_custody_abi_init(lxp_paxeer_custody_abi *abi)
{
    static const lxp_paxeer_custody_input_kind inputs[] = {
        LXP_PAXEER_INPUT_FINALISED_CHECKPOINT_CERTIFICATE,
        LXP_PAXEER_INPUT_STATE_PROOF,
        LXP_PAXEER_INPUT_WITHDRAWAL_NULLIFIER,
        LXP_PAXEER_INPUT_GUARANTOR_SIGNATURES,
        LXP_PAXEER_INPUT_CHALLENGE_WINDOW_STATE,
        LXP_PAXEER_INPUT_EMERGENCY_EXIT_ELIGIBILITY
    };
    if (abi == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(abi, 0, sizeof(*abi));
    (void)memcpy(abi->inputs, inputs, sizeof(inputs));
    abi->input_count = LXP_PAXEER_CUSTODY_INPUT_COUNT;
    return LXP_OK;
}

lxp_result lxp_paxeer_verify_cert(
    lxp_checkpoint_registry_state *state,
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *guarantor_set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, bool *finalised)
{
    lxp_finalisation_state candidate;
    lxp_result status;
    if (state == NULL || certificate == NULL || guarantor_set == NULL ||
        requirements == NULL || arena == NULL || finalised == NULL)
        return LXP_ERR_NON_CANONICAL;
    *finalised = false;
    candidate = state->finalisation;
    status = lxp_checkpoint_finalisable(&candidate, certificate,
                                        guarantor_set, requirements, arena,
                                        finalised);
    if (status != LXP_OK) return status;
    if (!*finalised) return LXP_ERR_ATTESTATION_THRESHOLD;
    state->finalisation = candidate;
    return LXP_OK;
}

lxp_result lxp_checkpoint_register(
    lxp_checkpoint_registry_state *state,
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *guarantor_set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, lxp_checkpoint_registration *registration)
{
    lxp_checkpoint_registry_state candidate;
    lxp_byte_span header;
    uint8_t checkpoint_id[32];
    uint8_t header_hash[32];
    bool finalised = false;
    size_t i;
    lxp_result status;
    if (state == NULL || certificate == NULL || guarantor_set == NULL ||
        requirements == NULL || arena == NULL || registration == NULL)
        return LXP_ERR_NON_CANONICAL;
    candidate = *state;
    status = lxp_paxeer_verify_cert(&candidate, certificate, guarantor_set,
                                     requirements, arena, &finalised);
    if (status != LXP_OK || !finalised) return status;
    status = lxp_batch_header_encode(&certificate->checkpoint.header, arena,
                                     &header);
    if (status != LXP_OK) return status;
    if (header.length != LXP_BATCH_HEADER_ENCODED_SIZE)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_checkpoint_certificate_hash(&certificate->checkpoint, arena,
                                              checkpoint_id);
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_BATCH_HEADER, header.bytes,
                                 header.length, header_hash);
    if (status != LXP_OK) return status;
    (void)memcpy(candidate.checkpoint_id, checkpoint_id,
                 sizeof(candidate.checkpoint_id));
    (void)memcpy(candidate.last_header_hash, header_hash,
                 sizeof(candidate.last_header_hash));
    candidate.registered_header_length = header.length;
    if (candidate.registration_count == UINT64_MAX) return LXP_ERR_OVERFLOW;
    ++candidate.registration_count;
    (void)memset(registration, 0, sizeof(*registration));
    registration->header = certificate->checkpoint.header;
    registration->header_commitments = header;
    registration->validity_proof = certificate->checkpoint.validity_proof;
    for (i = 0U; i < certificate->attestation_count; ++i) {
        status = lxp_paxeer_guarantor_attestation_from_core(
            &certificate->attestations[i], &registration->attestations[i]);
        if (status != LXP_OK) return status;
    }
    registration->attestation_count = certificate->attestation_count;
    (void)memcpy(registration->checkpoint_id, checkpoint_id,
                 sizeof(registration->checkpoint_id));
    (void)memcpy(registration->resulting_state_root,
                 certificate->checkpoint.header.resulting_state_root,
                 sizeof(registration->resulting_state_root));
    registration->batch_number = certificate->checkpoint.header.batch_number;
    *state = candidate;
    return LXP_OK;
}
