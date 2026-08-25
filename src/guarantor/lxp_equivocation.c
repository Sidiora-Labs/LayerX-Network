#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

enum {
    LXP_ATTESTATION_ENCODED_BYTES = 306,
    LXP_SEQUENCER_STATEMENT_BYTES = LXP_BATCH_HEADER_ENCODED_SIZE + 64
};

static lxp_result encode_attestation(
    lxp_codec_writer *writer, const lxp_guarantor_attestation *attestation)
{
    lxp_result status = lxp_codec_write_u16(
        writer, attestation->protocol_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(writer, attestation->network_id);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, attestation->paxeer_chain_id);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(
            writer, attestation->paxeer_settlement_contract, 20U, 20U);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, attestation->epoch);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(
            writer, attestation->checkpoint_id, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, attestation->checkpoint_hash,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, attestation->guarantor_id,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, attestation->batch_number);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(
            writer, attestation->data_availability_root, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, attestation->replayed ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer,
                                    attestation->da_possessed ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer,
                                    attestation->availability_class_mask);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, attestation->attested_at_ms);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, attestation->signer, 20U, 20U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, attestation->signature, 32U,
                                       32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, attestation->signature + 32U,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, attestation->signature_v);
    return status;
}

static lxp_result encode_sequencer(lxp_codec_writer *writer,
                                   const lxp_sealed_header_record *statement,
                                   lxp_arena *arena)
{
    lxp_byte_span header;
    size_t mark = lxp_arena_mark(arena);
    lxp_result status = lxp_batch_header_encode(&statement->header, arena,
                                                &header);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, header.bytes, header.length,
                                       LXP_BATCH_HEADER_ENCODED_SIZE);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, statement->signature, 64U,
                                       64U);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static int guarantor_contradiction(
    const lxp_guarantor_attestation *first,
    const lxp_guarantor_attestation *second)
{
    int different = memcmp(first->checkpoint_hash,
                           second->checkpoint_hash, 32U) != 0;
    return first->protocol_version == second->protocol_version &&
           first->network_id == second->network_id &&
           first->paxeer_chain_id == second->paxeer_chain_id &&
           memcmp(first->paxeer_settlement_contract,
                  second->paxeer_settlement_contract, 20U) == 0 &&
           first->epoch == second->epoch &&
           first->batch_number == second->batch_number &&
           memcmp(first->guarantor_id, second->guarantor_id, 32U) == 0 &&
           memcmp(first->checkpoint_id,
                  first->checkpoint_hash, 32U) == 0 &&
           memcmp(second->checkpoint_id,
                  second->checkpoint_hash, 32U) == 0 && different;
}

static int sequencer_contradiction(const lxp_sealed_header_record *first,
                                   const lxp_sealed_header_record *second)
{
    return first->header.protocol_version == second->header.protocol_version &&
        first->header.network_id == second->header.network_id &&
        first->header.epoch == second->header.epoch &&
        first->header.batch_number == second->header.batch_number &&
        memcmp(first->header.sequencer_id,
               second->header.sequencer_id, 32U) == 0 &&
        memcmp(first->header_hash, second->header_hash, 32U) != 0;
}

lxp_result lxp_equivocation_detect(
    lxp_equivocation_kind kind, const void *first_statement,
    const void *second_statement, const uint8_t *offender_public_key,
    size_t offender_public_key_length,
    lxp_equivocation_evidence *evidence)
{
    if (first_statement == NULL || second_statement == NULL ||
        offender_public_key == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(evidence, 0, sizeof(*evidence));
    if (kind == LXP_EQUIVOCATION_GUARANTOR) {
        const lxp_guarantor_attestation *first = first_statement;
        const lxp_guarantor_attestation *second = second_statement;
        if (offender_public_key_length != 33U ||
            !guarantor_contradiction(first, second))
            return LXP_ERR_NON_CANONICAL;
        evidence->guarantor_first = *first;
        evidence->guarantor_second = *second;
    } else if (kind == LXP_EQUIVOCATION_SEQUENCER) {
        const lxp_sealed_header_record *first = first_statement;
        const lxp_sealed_header_record *second = second_statement;
        if (offender_public_key_length != 32U ||
            !sequencer_contradiction(first, second))
            return LXP_ERR_NON_CANONICAL;
        evidence->sequencer_first = *first;
        evidence->sequencer_second = *second;
    } else {
        return LXP_ERR_NON_CANONICAL;
    }
    evidence->kind = kind;
    evidence->offender_public_key_length = offender_public_key_length;
    (void)memcpy(evidence->offender_public_key, offender_public_key,
                 offender_public_key_length);
    return LXP_OK;
}

lxp_result lxp_equivocation_encode(
    const lxp_equivocation_evidence *evidence, lxp_arena *arena,
    lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    size_t capacity;
    lxp_result status;
    if (evidence == NULL || arena == NULL || encoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    capacity = 5U + 1U + 4U + evidence->offender_public_key_length +
        (evidence->kind == LXP_EQUIVOCATION_GUARANTOR ?
         2U * LXP_ATTESTATION_ENCODED_BYTES :
         2U * LXP_SEQUENCER_STATEMENT_BYTES + 16U);
    status = lxp_codec_writer_init(&writer, arena, capacity);
    if (status == LXP_OK)
        status = lxp_codec_write_struct_header(&writer, 0x1904U);
    if (status == LXP_OK) status = lxp_codec_write_u8(&writer, 3U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(&writer, (uint8_t)evidence->kind);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer,
            evidence->offender_public_key,
            evidence->offender_public_key_length, 33U);
    if (status == LXP_OK && evidence->kind == LXP_EQUIVOCATION_GUARANTOR)
        status = encode_attestation(&writer, &evidence->guarantor_first);
    if (status == LXP_OK && evidence->kind == LXP_EQUIVOCATION_GUARANTOR)
        status = encode_attestation(&writer, &evidence->guarantor_second);
    if (status == LXP_OK && evidence->kind == LXP_EQUIVOCATION_SEQUENCER)
        status = encode_sequencer(&writer, &evidence->sequencer_first, arena);
    if (status == LXP_OK && evidence->kind == LXP_EQUIVOCATION_SEQUENCER)
        status = encode_sequencer(&writer, &evidence->sequencer_second, arena);
    if (status != LXP_OK) return status;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

static lxp_result verify_sequencer_statement(
    const lxp_sealed_header_record *statement, const uint8_t public_key[32],
    lxp_arena *arena)
{
    lxp_sequencer_authorization authorization;
    uint8_t hash[32];
    lxp_result status;
    (void)memset(&authorization, 0, sizeof(authorization));
    (void)memcpy(authorization.sequencer_id, statement->header.sequencer_id,
                 32U);
    (void)memcpy(authorization.public_key, public_key, 32U);
    authorization.first_batch_number = statement->header.batch_number;
    authorization.last_batch_number = statement->header.batch_number;
    authorization.authorized = 1U;
    status = lxp_batch_header_hash(&statement->header, arena, hash);
    if (status == LXP_OK &&
        memcmp(hash, statement->header_hash, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(
            &statement->header, statement->signature, 64U, &authorization,
            arena);
    return status;
}

lxp_result lxp_equivocation_verify(
    const lxp_equivocation_evidence *evidence, lxp_arena *arena)
{
    lxp_result status;
    if (evidence == NULL || arena == NULL) return LXP_ERR_NON_CANONICAL;
    if (evidence->kind == LXP_EQUIVOCATION_GUARANTOR) {
        if (evidence->offender_public_key_length != 33U ||
            !guarantor_contradiction(&evidence->guarantor_first,
                                     &evidence->guarantor_second))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_guarantor_attestation_verify(
            &evidence->guarantor_first, evidence->offender_public_key);
        if (status == LXP_OK)
            status = lxp_guarantor_attestation_verify(
                &evidence->guarantor_second,
                evidence->offender_public_key);
        return status;
    }
    if (evidence->kind == LXP_EQUIVOCATION_SEQUENCER) {
        if (evidence->offender_public_key_length != 32U ||
            !sequencer_contradiction(&evidence->sequencer_first,
                                     &evidence->sequencer_second))
            return LXP_ERR_NON_CANONICAL;
        status = verify_sequencer_statement(
            &evidence->sequencer_first, evidence->offender_public_key, arena);
        if (status == LXP_OK)
            status = verify_sequencer_statement(
                &evidence->sequencer_second,
                evidence->offender_public_key, arena);
        return status;
    }
    return LXP_ERR_NON_CANONICAL;
}
