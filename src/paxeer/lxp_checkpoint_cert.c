#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"
#include "lxp_checkpoint_settlement.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

uint64_t lxp_checkpoint_maximum_attestation_delay_ms(void)
{
    return LXP_CHECKPOINT_MAXIMUM_ATTESTATION_DELAY_SECONDS * UINT64_C(1000);
}

static lxp_result attestation_within_freshness_window(
    const lxp_batch_header *header,
    const lxp_guarantor_attestation *attestation)
{
    const uint64_t maximum_delay_ms =
        lxp_checkpoint_maximum_attestation_delay_ms();
    if (attestation->attested_at_ms < header->timestamp_ms)
        return LXP_ERR_NOT_YET_VALID;
    if (attestation->attested_at_ms - header->timestamp_ms > maximum_delay_ms)
        return LXP_ERR_EXPIRED;
    return LXP_OK;
}

lxp_result lxp_guarantor_attestation_verify(
    const lxp_guarantor_attestation *attestation,
    const uint8_t public_key[33]);

lxp_result lxp_checkpoint_certificate_hash(
    const lxp_checkpoint_certificate *checkpoint, lxp_arena *arena,
    uint8_t checkpoint_hash[32])
{
    lxp_byte_span header;
    lxp_hash_context hash;
    size_t tag_length = 0U;
    const uint8_t *tag;
    uint8_t length[4];
    size_t mark;
    lxp_result status;
    if (checkpoint == NULL || arena == NULL || checkpoint_hash == NULL ||
        checkpoint->validity_proof.length > LXP_MAX_VALIDITY_PROOF_BYTES ||
        (checkpoint->validity_proof.bytes == NULL &&
         checkpoint->validity_proof.length != 0U))
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_header_encode(&checkpoint->header, arena, &header);
    tag = lxp_domain_tag(LXP_DOMAIN_CHECKPOINT_CERTIFICATE, &tag_length);
    length[0] = (uint8_t)(checkpoint->validity_proof.length >> 24U);
    length[1] = (uint8_t)(checkpoint->validity_proof.length >> 16U);
    length[2] = (uint8_t)(checkpoint->validity_proof.length >> 8U);
    length[3] = (uint8_t)checkpoint->validity_proof.length;
    if (status == LXP_OK && tag == NULL) status = LXP_ERR_INVALID_TAG;
    if (status == LXP_OK) {
        lxp_hash_init(&hash);
        status = lxp_hash_update(&hash, tag, tag_length);
    }
    if (status == LXP_OK)
        status = lxp_hash_update(&hash, header.bytes, header.length);
    if (status == LXP_OK) status = lxp_hash_update(&hash, length, sizeof(length));
    if (status == LXP_OK)
        status = lxp_hash_update(&hash, checkpoint->validity_proof.bytes,
                                 checkpoint->validity_proof.length);
    if (status == LXP_OK) status = lxp_hash_final(&hash, checkpoint_hash);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static int attestation_compare(const void *left, const void *right)
{
    const lxp_guarantor_attestation *a =
        (const lxp_guarantor_attestation *)left;
    const lxp_guarantor_attestation *b =
        (const lxp_guarantor_attestation *)right;
    return memcmp(a->guarantor_id, b->guarantor_id, 32U);
}

lxp_result lxp_guarantor_cert_assemble(
    const lxp_checkpoint_certificate *checkpoint,
    const lxp_guarantor_attestation *attestations, size_t attestation_count,
    size_t threshold, lxp_guarantor_cert *certificate)
{
    size_t i;
    if (checkpoint == NULL || attestations == NULL || certificate == NULL ||
        attestation_count == 0U ||
        attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS || threshold == 0U ||
        threshold > attestation_count ||
        checkpoint->validity_proof.length > LXP_MAX_VALIDITY_PROOF_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(certificate, 0, sizeof(*certificate));
    certificate->checkpoint = *checkpoint;
    (void)memcpy(certificate->attestations, attestations,
                 attestation_count * sizeof(*attestations));
    certificate->attestation_count = attestation_count;
    certificate->threshold = threshold;
    certificate->bonded_economic_guarantee = true;
    certificate->validity_proof_present =
        checkpoint->validity_proof.length != 0U;
    qsort(certificate->attestations, attestation_count,
          sizeof(certificate->attestations[0]), attestation_compare);
    for (i = 0U; i < attestation_count; ++i) {
        if (!certificate->attestations[i].replayed ||
            !certificate->attestations[i].da_possessed ||
            certificate->attestations[i].protocol_version !=
                certificate->checkpoint.header.protocol_version ||
            certificate->attestations[i].network_id !=
                certificate->checkpoint.header.network_id ||
            certificate->attestations[i].epoch !=
                certificate->checkpoint.header.epoch ||
            certificate->attestations[i].batch_number !=
                certificate->checkpoint.header.batch_number ||
            certificate->attestations[i].availability_class_mask !=
                LXP_GUARANTOR_AVAILABILITY_ALL ||
            (i != 0U && memcmp(certificate->attestations[i - 1U].guarantor_id,
                               certificate->attestations[i].guarantor_id,
                               32U) == 0))
            return LXP_ERR_ATTESTATION_THRESHOLD;
        if (memcmp(certificate->attestations[0].checkpoint_id,
                   certificate->attestations[i].checkpoint_id, 32U) != 0 ||
            memcmp(certificate->attestations[0].checkpoint_hash,
                   certificate->attestations[i].checkpoint_hash, 32U) != 0 ||
            certificate->attestations[0].paxeer_chain_id !=
                certificate->attestations[i].paxeer_chain_id ||
            memcmp(certificate->attestations[0].paxeer_settlement_contract,
                   certificate->attestations[i].paxeer_settlement_contract,
                   20U) != 0 ||
            memcmp(certificate->checkpoint.header.data_availability_root,
                   certificate->attestations[i].data_availability_root,
                   32U) != 0)
            return LXP_ERR_ROOT_MISMATCH;
    }
    return LXP_OK;
}

static const lxp_guarantor_key_record *key_for(
    const lxp_guarantor_key_record *keys, size_t key_count,
    const uint8_t guarantor_id[32])
{
    size_t i;
    for (i = 0U; i < key_count; ++i)
        if (memcmp(keys[i].guarantor_id, guarantor_id, 32U) == 0)
            return &keys[i];
    return NULL;
}

lxp_result lxp_guarantor_cert_verify(
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_key_record *keys, size_t key_count,
    lxp_arena *arena, size_t *valid_signatures)
{
    uint8_t checkpoint_hash[32];
    size_t valid = 0U;
    size_t i;
    lxp_result status;
    if (certificate == NULL || keys == NULL || key_count == 0U ||
        arena == NULL || valid_signatures == NULL ||
        certificate->threshold == 0U ||
        certificate->attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        certificate->attestation_count < certificate->threshold ||
        !certificate->bonded_economic_guarantee)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    *valid_signatures = 0U;
    status = lxp_checkpoint_certificate_hash(&certificate->checkpoint, arena,
                                              checkpoint_hash);
    for (i = 0U; status == LXP_OK &&
                 i < certificate->attestation_count; ++i) {
        const lxp_guarantor_attestation *attestation =
            &certificate->attestations[i];
        const lxp_guarantor_key_record *key;
        if (i != 0U && memcmp(certificate->attestations[i - 1U].guarantor_id,
                              attestation->guarantor_id, 32U) >= 0)
            return LXP_ERR_ATTESTATION_THRESHOLD;
        if (memcmp(attestation->checkpoint_id, checkpoint_hash, 32U) != 0 ||
            memcmp(attestation->checkpoint_hash, checkpoint_hash, 32U) != 0 ||
            (i != 0U &&
             certificate->attestations[0].paxeer_chain_id !=
                 attestation->paxeer_chain_id) ||
            (i != 0U &&
             memcmp(certificate->attestations[0].paxeer_settlement_contract,
                    attestation->paxeer_settlement_contract, 20U) != 0) ||
            attestation->protocol_version !=
                certificate->checkpoint.header.protocol_version ||
            attestation->network_id !=
                certificate->checkpoint.header.network_id ||
            attestation->epoch != certificate->checkpoint.header.epoch ||
            attestation->batch_number !=
                certificate->checkpoint.header.batch_number ||
            memcmp(attestation->data_availability_root,
                   certificate->checkpoint.header.data_availability_root,
                   32U) != 0)
            return LXP_ERR_ROOT_MISMATCH;
        status = attestation_within_freshness_window(
            &certificate->checkpoint.header, attestation);
        if (status != LXP_OK) return status;
        key = key_for(keys, key_count, attestation->guarantor_id);
        if (key != NULL && key->bonded &&
            lxp_guarantor_attestation_verify(attestation, key->public_key) ==
                LXP_OK) ++valid;
    }
    if (status != LXP_OK) return status;
    *valid_signatures = valid;
    return valid >= certificate->threshold ? LXP_OK :
           LXP_ERR_ATTESTATION_THRESHOLD;
}

lxp_result lxp_receipt_augment(
    lxp_byte_span pre_checkpoint_receipt,
    lxp_byte_span canonical_activity,
    lxp_byte_span state_leaf,
    const lxp_merkle_proof *activity_inclusion_proof,
    const lxp_merkle_proof *state_inclusion_proof,
    const lxp_guarantor_cert *guarantor_certificate,
    lxp_byte_span paxeer_settlement_reference,
    lxp_augmented_receipt *augmented)
{
    if ((pre_checkpoint_receipt.bytes == NULL &&
         pre_checkpoint_receipt.length != 0U) ||
        pre_checkpoint_receipt.length == 0U ||
        canonical_activity.bytes == NULL || canonical_activity.length == 0U ||
        state_leaf.bytes == NULL || state_leaf.length == 0U ||
        activity_inclusion_proof == NULL || state_inclusion_proof == NULL ||
        guarantor_certificate == NULL || augmented == NULL ||
        paxeer_settlement_reference.bytes == NULL ||
        paxeer_settlement_reference.length == 0U ||
        paxeer_settlement_reference.length >
            LXP_MAX_SETTLEMENT_REFERENCE_BYTES ||
        guarantor_certificate->attestation_count <
            guarantor_certificate->threshold)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(augmented, 0, sizeof(*augmented));
    augmented->pre_checkpoint_receipt = pre_checkpoint_receipt;
    augmented->canonical_activity = canonical_activity;
    augmented->state_leaf = state_leaf;
    augmented->activity_inclusion_proof = *activity_inclusion_proof;
    augmented->state_inclusion_proof = *state_inclusion_proof;
    (void)memcpy(augmented->checkpoint_id,
                 guarantor_certificate->attestations[0].checkpoint_id, 32U);
    augmented->guarantor_certificate = guarantor_certificate;
    augmented->paxeer_settlement_reference = paxeer_settlement_reference;
    return LXP_OK;
}
