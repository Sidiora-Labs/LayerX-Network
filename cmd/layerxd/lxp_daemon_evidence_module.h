#ifndef LXP_DAEMON_EVIDENCE_MODULE_H
#define LXP_DAEMON_EVIDENCE_MODULE_H

static lxp_result module_read_checkpoint(
    const lxp_daemon_evidence_store *store,
    const lxp_daemon_signed_header_evidence *signed_header,
    const lxp_batch_header *header, const uint8_t *checkpoint_id,
    lxp_arena *arena, lxp_daemon_finality_evidence *checkpoint)
{
    uint8_t no_identifier[32] = {0};
    lxp_result status = lxp_daemon_finality_evidence_lookup(
        store, checkpoint_id == NULL ? no_identifier : checkpoint_id,
        checkpoint_id == NULL ? header->batch_number : 0U, arena, checkpoint);
    if (status != LXP_OK) return status;
    if (checkpoint->batch_number != header->batch_number ||
        lxp_ct_memcmp(checkpoint->resulting_state_root, header->resulting_state_root, 32U) != 0 ||
        checkpoint->checkpoint_payload.bytes == NULL ||
        checkpoint->checkpoint_payload.length < 6U + signed_header->canonical_header.length ||
        checkpoint->checkpoint_payload.length > 2U * LXP_KERNEL_MAX_BLOB_BYTES ||
        checkpoint->finality_proof.bytes == NULL || checkpoint->finality_proof.length == 0U ||
        checkpoint->finality_proof.length > 128U * 1024U ||
        read_u16(checkpoint->checkpoint_payload.bytes) != 1U ||
        read_u32(checkpoint->checkpoint_payload.bytes + 2U) != signed_header->canonical_header.length ||
        lxp_ct_memcmp(checkpoint->checkpoint_payload.bytes + 6U,
            signed_header->canonical_header.bytes, signed_header->canonical_header.length) != 0 ||
        (checkpoint_id != NULL && lxp_ct_memcmp(checkpoint_id, checkpoint->checkpoint_id, 32U) != 0))
        return LXP_ERR_CONTEXT_MISMATCH;
    return LXP_OK;
}

lxp_result lxp_daemon_module_evidence_wire_encode(
    const lxp_daemon_evidence_store *store, const lxp_kernel *kernel,
    const lxp_daemon_signed_header_evidence *signed_header,
    uint16_t module_id, lxp_byte_span key, uint8_t selector_kind,
    uint64_t selector_batch, const uint8_t selector_checkpoint_id[32],
    uint8_t requested_rank, lxp_arena *arena,
    lxp_byte_span *canonical_value, lxp_byte_span *proof_material)
{
    lxp_daemon_finality_evidence checkpoint;
    lxp_batch_header header;
    lxp_state_witness *witness = NULL;
    uint8_t *wire = NULL;
    uint8_t root[32];
    size_t wire_length = 0U;
    size_t proof_length;
    void *allocation;
    uint8_t *value;
    evidence_writer writer;
    bool with_checkpoint = selector_kind == 3U || requested_rank == 4U;
    lxp_result status;
    if (store == NULL || !store->initialized || kernel == NULL || kernel->state == NULL ||
        signed_header == NULL || arena == NULL || canonical_value == NULL || proof_material == NULL ||
        key.bytes == NULL || key.length == 0U || key.length > LXP_STATE_WITNESS_MAX_KEY ||
        module_id > LXP_MODULE_RESERVED_COUNT || requested_rank > 4U ||
        !authorizations_equal(&store->authorization, &signed_header->authorization) ||
        (selector_kind == 1U && (selector_batch != 0U || selector_checkpoint_id != NULL)) ||
        (selector_kind == 2U && (selector_batch == 0U || selector_checkpoint_id != NULL)) ||
        (selector_kind == 3U && (selector_batch != 0U || selector_checkpoint_id == NULL ||
            lxp_ct_is_zero(selector_checkpoint_id, 32U))) ||
        selector_kind < 1U || selector_kind > 3U)
        return LXP_ERR_NON_CANONICAL;
    status = verify_signed_header(signed_header, store->network_id, arena, &header);
    if (status == LXP_OK && header.protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) status = lxp_state_root(kernel, root);
    if (status == LXP_OK && (kernel->state->next_sequence == 0U ||
        kernel->state->next_sequence - 1U != header.last_sequence ||
        lxp_ct_memcmp(root, kernel->current_state_root, 32U) != 0 ||
        lxp_ct_memcmp(root, header.resulting_state_root, 32U) != 0 ||
        (selector_kind == 2U && selector_batch != header.batch_number)))
        status = LXP_ERR_PROJECTION_STALE;
    if (status == LXP_OK && with_checkpoint)
        status = module_read_checkpoint(store, signed_header, &header,
            selector_checkpoint_id, arena, &checkpoint);
    if (status != LXP_OK) return status;
    witness = malloc(sizeof(*witness));
    wire = malloc(LXP_STATE_WITNESS_MAX_BYTES);
    if (witness == NULL || wire == NULL) {
        free(witness);
        free(wire);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    status = lxp_state_proof_build(kernel, module_id, key, witness);
    if (status == LXP_OK) status = lxp_state_proof_verify(witness, root);
    if (status == LXP_OK)
        status = lxp_state_proof_encode(witness, wire, LXP_STATE_WITNESS_MAX_BYTES, &wire_length);
    proof_length = 3U + (selector_kind == 1U ? 1U : selector_kind == 2U ? 9U : 33U) +
        4U + wire_length + signed_header_length(signed_header) + 1U;
    if (with_checkpoint)
        proof_length += 8U + checkpoint.checkpoint_payload.length + checkpoint.finality_proof.length;
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, witness->value_length == 0U ? 1U : witness->value_length,
            _Alignof(uint64_t), &allocation);
    value = status == LXP_OK ? allocation : NULL;
    if (status == LXP_OK) memcpy(value, witness->value, witness->value_length);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, proof_length, _Alignof(uint64_t), &allocation);
    if (status == LXP_OK) {
        writer = (evidence_writer){allocation, proof_length, 0U};
        status = writer_u16(&writer, 1U);
        if (status == LXP_OK) status = writer_u8(&writer, 4U);
        if (status == LXP_OK) status = writer_u8(&writer, selector_kind);
        if (status == LXP_OK && selector_kind == 2U) status = writer_u64(&writer, selector_batch);
        if (status == LXP_OK && selector_kind == 3U) status = writer_bytes(&writer, selector_checkpoint_id, 32U);
        if (status == LXP_OK) status = writer_u32(&writer, (uint32_t)wire_length);
        if (status == LXP_OK) status = writer_bytes(&writer, wire, wire_length);
        if (status == LXP_OK) status = write_signed_header(&writer, signed_header);
        if (status == LXP_OK) status = writer_u8(&writer, with_checkpoint ? 1U : 0U);
        if (status == LXP_OK && with_checkpoint) status = writer_u32(&writer, (uint32_t)checkpoint.checkpoint_payload.length);
        if (status == LXP_OK && with_checkpoint) status = writer_bytes(&writer, checkpoint.checkpoint_payload.bytes, checkpoint.checkpoint_payload.length);
        if (status == LXP_OK && with_checkpoint) status = writer_u32(&writer, (uint32_t)checkpoint.finality_proof.length);
        if (status == LXP_OK && with_checkpoint) status = writer_bytes(&writer, checkpoint.finality_proof.bytes, checkpoint.finality_proof.length);
        if (status == LXP_OK && writer.cursor != writer.capacity) status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK) {
            *canonical_value = (lxp_byte_span){value, witness->value_length};
            *proof_material = (lxp_byte_span){allocation, proof_length};
        }
    }
    free(wire);
    free(witness);
    return status;
}

#endif
