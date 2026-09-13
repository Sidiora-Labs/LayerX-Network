#ifndef LXP_DAEMON_MAINTENANCE_JSON_H
#define LXP_DAEMON_MAINTENANCE_JSON_H

#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"
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

static lxp_result maintenance_activity_receipts_json(
    const lxp_daemon_receipt_authority_store *store,
    const lxp_daemon_receipt_evidence *maintenance, lxp_arena *arena,
    char **output, size_t *output_length)
{
    uint64_t offset = 0U;
    uint32_t count = 0U;
    size_t length = 1U;
    char *json = malloc(3U);
    lxp_result status = LXP_OK;
    if (json == NULL) return LXP_ERR_IO;
    json[0] = '[';
    if (maintenance->receipt_proof.leaf_index == 0U ||
        maintenance->receipt_proof.leaf_index > LXP_DAEMON_MAX_BATCH_ACTIVITIES) {
        free(json);
        return LXP_ERR_LENGTH_LIMIT;
    }
    for (;;) {
        lxp_daemon_receipt_evidence item;
        bool found = false;
        size_t mark = lxp_arena_mark(arena);
        status = lxp_daemon_receipt_authority_scan(store, &offset, arena, &item, &found);
        if (status == LXP_OK && found && item.format_version != 3U &&
            item.canonical_header.length == maintenance->canonical_header.length &&
            lxp_ct_memcmp(item.canonical_header.bytes, maintenance->canonical_header.bytes,
                item.canonical_header.length) == 0) {
            size_t additional;
            char *grown;
            if (count >= maintenance->receipt_proof.leaf_index ||
                item.receipt_proof.leaf_index != count ||
                item.receipt_proof.leaf_count != maintenance->receipt_proof.leaf_count ||
                lxp_ct_memcmp(item.header_signature, maintenance->header_signature, 64U) != 0 ||
                item.canonical_receipt.length > (SIZE_MAX - 5U) / 2U) {
                status = LXP_ERR_CONTEXT_MISMATCH;
            } else {
                additional = item.canonical_receipt.length * 2U + 3U;
                if (length > SIZE_MAX - additional - 2U) status = LXP_ERR_LENGTH_LIMIT;
                else {
                    grown = realloc(json, length + additional + 2U);
                    if (grown == NULL) status = LXP_ERR_IO;
                    else {
                        json = grown;
                        if (count != 0U) json[length++] = ',';
                        json[length++] = '"';
                        maintenance_hex(item.canonical_receipt.bytes,
                            item.canonical_receipt.length, json + length);
                        length += item.canonical_receipt.length * 2U;
                        json[length++] = '"';
                        ++count;
                    }
                }
            }
        }
        if (lxp_arena_reset(arena, mark) != LXP_OK) status = LXP_FATAL_INVARIANT;
        if (status != LXP_OK || !found) break;
    }
    if (status == LXP_OK && count != maintenance->receipt_proof.leaf_index)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status != LXP_OK) { free(json); return status; }
    json[length++] = ']';
    json[length] = '\0';
    *output = json;
    *output_length = length;
    return LXP_OK;
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
    char *activity_receipts = NULL;
    size_t activity_receipts_length = 0U;
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
    if (status == LXP_OK && maintained)
        status = maintenance_activity_receipts_json(store, &maintenance, arena,
            &activity_receipts, &activity_receipts_length);
    if (status == LXP_OK && maintained) {
        char *receipt_hex = malloc(maintenance.canonical_receipt.length * 2U + 1U);
        char *maintenance_proof_hex = malloc(maintenance_writer.length * 2U + 1U);
        size_t identity_capacity = sizeof(",\"batch_identity\":{\"kind\":\"occupancy_maintenance_v2\","
            "\"receipt_hex\":\"\",\"receipt_proof_hex\":\"\",\"activity_receipts_hex\":}") +
            maintenance.canonical_receipt.length * 2U + maintenance_writer.length * 2U + activity_receipts_length;
        identity_json = malloc(identity_capacity);
        if (receipt_hex == NULL || maintenance_proof_hex == NULL || identity_json == NULL)
            status = LXP_ERR_IO;
        else {
            maintenance_hex(maintenance.canonical_receipt.bytes,
                maintenance.canonical_receipt.length, receipt_hex);
            maintenance_hex(maintenance_writer.bytes, maintenance_writer.length, maintenance_proof_hex);
            length = snprintf(identity_json, identity_capacity,
                ",\"batch_identity\":{\"kind\":\"occupancy_maintenance_v2\","
                "\"receipt_hex\":\"%s\",\"receipt_proof_hex\":\"%s\",\"activity_receipts_hex\":%s}",
                receipt_hex, maintenance_proof_hex, activity_receipts);
            if (length < 0 || (size_t)length >= identity_capacity)
                status = LXP_ERR_LENGTH_LIMIT;
            else identity_length = (size_t)length;
        }
        free(receipt_hex);
        free(maintenance_proof_hex);
    }
    free(activity_receipts);
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
