#include "layerx/lxp_paxeer.h"

#include "layerx/lxp_crypto.h"

#include <stdint.h>
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

const uint8_t lxp_paxeer_anchor_address[20] = {
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 0U, 0U, 0U,
    0U, 0U, 0U, 0U, 0U, 0U, 0U, 0U, 0x10U, 0x14U};

lxp_result lxp_paxeer_abi_selector(const char *signature, uint8_t selector[4])
{
    uint8_t hash[32];
    lxp_result status;
    if (signature == NULL || selector == NULL || signature[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    status = lxp_keccak256((const uint8_t *)signature, strlen(signature), hash);
    if (status != LXP_OK) return status;
    (void)memcpy(selector, hash, 4U);
    return LXP_OK;
}

static void put_be(uint8_t *out, size_t width, uint64_t value)
{
    size_t i;
    for (i = 0U; i < width; ++i) {
        out[width - 1U - i] = (uint8_t)value;
        value >>= 8U;
    }
}

lxp_result lxp_paxeer_checkpoint_certificate_encode(
    const lxp_guarantor_cert *certificate, lxp_arena *arena,
    lxp_byte_span *encoded)
{
    lxp_byte_span header;
    void *memory = NULL;
    uint8_t *out;
    size_t length, cursor = 0U, i;
    lxp_result status;
    if (certificate == NULL || arena == NULL || encoded == NULL ||
        certificate->attestation_count == 0U ||
        certificate->attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        certificate->threshold == 0U ||
        certificate->threshold > certificate->attestation_count ||
        certificate->checkpoint.validity_proof.length >
            LXP_MAX_VALIDITY_PROOF_BYTES ||
        (certificate->checkpoint.validity_proof.bytes == NULL &&
         certificate->checkpoint.validity_proof.length != 0U))
        return LXP_ERR_NON_CANONICAL;
    for (i = 1U; i < certificate->attestation_count; ++i)
        if (memcmp(certificate->attestations[i - 1U].guarantor_id,
                   certificate->attestations[i].guarantor_id, 32U) >= 0)
            return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_header_encode(&certificate->checkpoint.header, arena,
                                     &header);
    if (status != LXP_OK) return status;
    if (header.length != LXP_BATCH_HEADER_ENCODED_SIZE)
        return LXP_ERR_NON_CANONICAL;
    length = 2U + 4U + header.length + 4U +
             certificate->checkpoint.validity_proof.length + 1U +
             certificate->attestation_count *
                 LXP_PAXEER_CERTIFICATE_ATTESTATION_BYTES +
             1U + 2U;
    status = lxp_arena_alloc(arena, length, _Alignof(uint64_t), &memory);
    if (status != LXP_OK) return status;
    out = memory;
    put_be(out + cursor, 2U, LXP_PAXEER_CERTIFICATE_WIRE_VERSION); cursor += 2U;
    put_be(out + cursor, 4U, header.length); cursor += 4U;
    (void)memcpy(out + cursor, header.bytes, header.length); cursor += header.length;
    put_be(out + cursor, 4U, certificate->checkpoint.validity_proof.length); cursor += 4U;
    if (certificate->checkpoint.validity_proof.length != 0U)
        (void)memcpy(out + cursor, certificate->checkpoint.validity_proof.bytes,
                     certificate->checkpoint.validity_proof.length);
    cursor += certificate->checkpoint.validity_proof.length;
    out[cursor++] = (uint8_t)certificate->attestation_count;
    for (i = 0U; i < certificate->attestation_count; ++i) {
        const lxp_guarantor_attestation *a = &certificate->attestations[i];
        if (lxp_ct_is_zero(a->signer, 20U) ||
            (a->signature_v != 27U && a->signature_v != 28U))
            return LXP_ERR_NON_CANONICAL;
        put_be(out + cursor, 2U, a->protocol_version); cursor += 2U;
        put_be(out + cursor, 4U, a->network_id); cursor += 4U;
        put_be(out + cursor, 8U, a->paxeer_chain_id); cursor += 8U;
        (void)memcpy(out + cursor, a->paxeer_settlement_contract, 20U); cursor += 20U;
        put_be(out + cursor, 8U, a->epoch); cursor += 8U;
        (void)memcpy(out + cursor, a->checkpoint_id, 32U); cursor += 32U;
        (void)memcpy(out + cursor, a->checkpoint_hash, 32U); cursor += 32U;
        (void)memcpy(out + cursor, a->guarantor_id, 32U); cursor += 32U;
        put_be(out + cursor, 8U, a->batch_number); cursor += 8U;
        (void)memcpy(out + cursor, a->data_availability_root, 32U); cursor += 32U;
        out[cursor++] = a->replayed ? 1U : 0U;
        out[cursor++] = a->da_possessed ? 1U : 0U;
        out[cursor++] = a->availability_class_mask;
        put_be(out + cursor, 8U, a->attested_at_ms); cursor += 8U;
        (void)memcpy(out + cursor, a->signer, 20U); cursor += 20U;
        (void)memcpy(out + cursor, a->signature, 64U); cursor += 64U;
        out[cursor++] = a->signature_v;
    }
    out[cursor++] = (uint8_t)certificate->threshold;
    put_be(out + cursor, 2U, 0U); cursor += 2U;
    if (cursor != length) return LXP_FATAL_INVARIANT;
    *encoded = (lxp_byte_span){out, length};
    return LXP_OK;
}

lxp_result lxp_paxeer_abi_encode_bytes(
    const uint8_t selector[4], const lxp_byte_span *arguments,
    size_t argument_count, lxp_arena *arena, lxp_byte_span *calldata)
{
    void *memory = NULL;
    uint8_t *out;
    size_t length, offset, cursor, i;
    lxp_result status;
    if (selector == NULL || arguments == NULL || argument_count == 0U ||
        argument_count > 8U || arena == NULL || calldata == NULL)
        return LXP_ERR_NON_CANONICAL;
    length = 4U + 32U * argument_count;
    for (i = 0U; i < argument_count; ++i) {
        size_t padded;
        if ((arguments[i].bytes == NULL && arguments[i].length != 0U) ||
            arguments[i].length > UINT32_MAX)
            return LXP_ERR_NON_CANONICAL;
        padded = (arguments[i].length + 31U) / 32U * 32U;
        if (length > SIZE_MAX - 32U - padded) return LXP_ERR_OVERFLOW;
        length += 32U + padded;
    }
    status = lxp_arena_alloc(arena, length, _Alignof(uint64_t), &memory);
    if (status != LXP_OK) return status;
    out = memory;
    (void)memset(out, 0, length);
    (void)memcpy(out, selector, 4U);
    offset = 32U * argument_count;
    cursor = 4U + offset;
    for (i = 0U; i < argument_count; ++i) {
        size_t padded = (arguments[i].length + 31U) / 32U * 32U;
        put_be(out + 4U + 32U * i + 24U, 8U, offset);
        put_be(out + cursor + 24U, 8U, arguments[i].length);
        if (arguments[i].length != 0U)
            (void)memcpy(out + cursor + 32U, arguments[i].bytes,
                         arguments[i].length);
        cursor += 32U + padded;
        offset += 32U + padded;
    }
    if (cursor != length) return LXP_FATAL_INVARIANT;
    *calldata = (lxp_byte_span){out, length};
    return LXP_OK;
}

lxp_result lxp_checkpoint_submit_calldata(
    const lxp_guarantor_cert *certificate, const uint8_t header_signature[64],
    lxp_arena *arena, lxp_byte_span *calldata)
{
    lxp_byte_span arguments[3];
    uint8_t selector[4];
    lxp_result status;
    if (certificate == NULL || header_signature == NULL || arena == NULL ||
        calldata == NULL || lxp_ct_is_zero(header_signature, 64U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_paxeer_abi_selector(LXP_PAXEER_ANCHOR_SUBMIT_CHECKPOINT,
                                     selector);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(&certificate->checkpoint.header,
                                         arena, &arguments[0]);
    if (status == LXP_OK &&
        arguments[0].length != LXP_BATCH_HEADER_ENCODED_SIZE)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_paxeer_checkpoint_certificate_encode(certificate, arena,
                                                          &arguments[2]);
    if (status != LXP_OK) return status;
    arguments[1] = (lxp_byte_span){header_signature, 64U};
    return lxp_paxeer_abi_encode_bytes(selector, arguments, 3U, arena,
                                       calldata);
}
