#ifndef LXP_DAEMON_LNI_MODULE_H
#define LXP_DAEMON_LNI_MODULE_H

static lxp_result send_module_read(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    uint16_t module_id;
    uint16_t key_length;
    lxp_byte_span key, value, proof;
    uint8_t selector_kind;
    uint8_t rank;
    uint64_t batch = 0U;
    const uint8_t *checkpoint = NULL;
    size_t cursor = 7U;
    size_t mark;
    lxp_daemon_receipt_evidence head;
    lxp_daemon_signed_header_evidence signed_header;
    lxp_result status;
    if (request->minor < 5U || request->correlation_id == 0U ||
        request->proof_length != 0U || request->payload_length < 10U ||
        load_u16(request->payload) != 1U || request->payload[2] != 4U)
        return send_refusal(descriptor, server->frame_bytes, request->correlation_id, 1U,
            request->minor < 5U ? LXP_ERR_VERSION_UNSUPPORTED : LXP_ERR_MALFORMED_ENVELOPE, deadline);
    module_id = load_u16(request->payload + 3U);
    key_length = load_u16(request->payload + 5U);
    if (module_id > LXP_MODULE_RESERVED_COUNT || key_length == 0U ||
        key_length > LXP_MODULE_MAX_KEY_BYTES + 1U || key_length > request->payload_length - cursor - 2U)
        return send_refusal(descriptor, server->frame_bytes, request->correlation_id, 1U,
            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    key = (lxp_byte_span){request->payload + cursor, key_length};
    cursor += key_length;
    selector_kind = request->payload[cursor++];
    if (selector_kind == 2U && request->payload_length - cursor == 9U) {
        batch = load_u64(request->payload + cursor);
        cursor += 8U;
        if (batch == 0U) selector_kind = 0U;
    } else if (selector_kind == 3U && request->payload_length - cursor == 33U) {
        checkpoint = request->payload + cursor;
        cursor += 32U;
        if (lxp_ct_is_zero(checkpoint, 32U)) selector_kind = 0U;
    } else if (selector_kind != 1U) {
        selector_kind = 0U;
    }
    if (selector_kind == 0U || request->payload_length - cursor != 1U || request->payload[cursor] > 5U)
        return send_refusal(descriptor, server->frame_bytes, request->correlation_id, 1U,
            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    rank = request->payload[cursor];
    if (server->owner->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
        server->owner->evidence_store == NULL || rank > 4U)
        return evidence_refusal(server, descriptor, request->correlation_id, LXP_ERR_MODULE_DISABLED, deadline);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    status = latest_receipt_evidence(server->owner, server->owner->scratch, &head);
    if (status == LXP_OK) {
        signed_header.authorization = server->owner->evidence_store->authorization;
        signed_header.canonical_header = head.canonical_header;
        memcpy(signed_header.signature, head.header_signature, sizeof(signed_header.signature));
        status = lxp_daemon_module_evidence_wire_encode(
            server->owner->evidence_store, server->owner->kernel, &signed_header,
            module_id, key, selector_kind, batch, checkpoint, rank,
            server->owner->scratch, &value, &proof);
    }
    if (status == LXP_OK)
        status = send_envelope(descriptor, server->frame_bytes, LNI_ACCOUNT_READ_RESPONSE,
            request->correlation_id, value.bytes, value.length, proof.bytes, proof.length, deadline);
    else
        status = evidence_refusal(server, descriptor, request->correlation_id, status, deadline);
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0) return LXP_FATAL_INVARIANT;
    return status;
}

#endif
