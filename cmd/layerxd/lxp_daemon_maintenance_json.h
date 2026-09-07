#ifndef LXP_DAEMON_MAINTENANCE_JSON_H
#define LXP_DAEMON_MAINTENANCE_JSON_H

#include "layerx/lxp_daemon.h"
#include <stdio.h>
#include <stdlib.h>

static void maintenance_hex(const uint8_t *bytes, size_t length, char *text)
{
    static const char digits[] = "0123456789abcdef";
    size_t index;
    for (index = 0U; index < length; ++index) {
        text[index * 2U] = digits[bytes[index] >> 4U];
        text[index * 2U + 1U] = digits[bytes[index] & 15U];
    }
    text[length * 2U] = '\0';
}

static lxp_result maintenance_identity_json(
    const lxp_daemon_receipt_authority_store *store,
    const lxp_daemon_receipt_evidence *evidence, lxp_arena *arena,
    char **body, size_t *body_length)
{
    lxp_daemon_receipt_evidence maintenance;
    lxp_codec_writer maintenance_writer;
    bool maintained = false;
    char *identity_json = NULL;
    size_t identity_length = 0U;
    size_t mark = lxp_arena_mark(arena);
    int length;
    lxp_result status = LXP_OK;
    if (status == LXP_OK)
        status = lxp_daemon_receipt_authority_batch_maintenance(
            store, evidence, arena, &maintenance, &maintained);
    if (status == LXP_OK && maintained)
        status = lxp_codec_writer_init(&maintenance_writer, arena,
            16U + LXP_MERKLE_MAX_DEPTH * 32U);
    if (status == LXP_OK && maintained)
        status = lxp_merkle_proof_encode(&maintenance_writer, &maintenance.receipt_proof);
    if (status == LXP_OK && maintained) {
        char *receipt_hex = malloc(maintenance.canonical_receipt.length * 2U + 1U);
        char *maintenance_proof_hex = malloc(maintenance_writer.length * 2U + 1U);
        size_t identity_capacity = sizeof(",\"batch_identity\":{\"kind\":\"occupancy_maintenance_v2\","
            "\"receipt_hex\":\"\",\"receipt_proof_hex\":\"\"}") +
            maintenance.canonical_receipt.length * 2U + maintenance_writer.length * 2U;
        identity_json = malloc(identity_capacity);
        if (receipt_hex == NULL || maintenance_proof_hex == NULL || identity_json == NULL)
            status = LXP_ERR_IO;
        else {
            maintenance_hex(maintenance.canonical_receipt.bytes,
                maintenance.canonical_receipt.length, receipt_hex);
            maintenance_hex(maintenance_writer.bytes, maintenance_writer.length, maintenance_proof_hex);
            length = snprintf(identity_json, identity_capacity,
                ",\"batch_identity\":{\"kind\":\"occupancy_maintenance_v2\","
                "\"receipt_hex\":\"%s\",\"receipt_proof_hex\":\"%s\"}", receipt_hex, maintenance_proof_hex);
            if (length < 0 || (size_t)length >= identity_capacity)
                status = LXP_ERR_LENGTH_LIMIT;
            else identity_length = (size_t)length;
        }
        free(receipt_hex);
        free(maintenance_proof_hex);
    }
    (void)lxp_arena_reset(arena, mark);
    if (status != LXP_OK) {
        free(identity_json);
        return status;
    }
    *body = identity_json;
    *body_length = identity_length;
    return LXP_OK;
}

#endif
