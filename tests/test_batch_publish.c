#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_batch.h"

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct sink {
    uint8_t bytes[3][4096];
    size_t lengths[3];
    size_t calls;
    size_t fail_call;
} sink;

static lxp_result receive_chunk(void *context, const uint8_t replica_id[32],
                                uint64_t batch_number, uint64_t offset,
                                const uint8_t *bytes, size_t length,
                                uint64_t total_length)
{
    sink *output = (sink *)context;
    size_t index = (size_t)(replica_id[0] - 1U);
    (void)batch_number;
    if (output->calls++ == output->fail_call) return LXP_ERR_IO;
    if (index >= 3U || offset != output->lengths[index] ||
        total_length > sizeof(output->bytes[index]) ||
        length > sizeof(output->bytes[index]) - output->lengths[index])
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(output->bytes[index] + output->lengths[index], bytes, length);
    output->lengths[index] += length;
    return LXP_OK;
}

int main(void)
{
    uint8_t arena_storage[16384];
    uint8_t sections[6][7] = {
        {1U,2U,3U}, {4U,5U}, {6U}, {7U,8U}, {9U,10U,11U}, {12U,13U}
    };
    uint8_t replica_ids[3][32] = {{1U},{2U},{3U}};
    lxp_batch_body body;
    lxp_batch_body decoded;
    lxp_batch_replica_target replicas[3];
    lxp_batch_eligibility_state eligibility;
    lxp_byte_span canonical;
    uint8_t canonical_copy[4096];
    size_t canonical_length;
    lxp_arena arena;
    lxp_log log;
    lxp_log_record_header record;
    uint8_t acknowledged_id[32];
    sink output;
    bool eligible = true;
    char directory[] = "/tmp/lxp-batch-publish-XXXXXX";
    char path[128];
    size_t i;
    (void)memset(&body, 0, sizeof(body));
    body.header.protocol_version = 1U;
    body.header.network_id = 9U;
    body.header.batch_number = 14U;
    body.header.timestamp_ms = 33U;
    body.sequencer_signature[0] = 1U;
    body.activities = (lxp_byte_span){ sections[0], 3U };
    body.receipts = (lxp_byte_span){ sections[1], 2U };
    body.events = (lxp_byte_span){ sections[2], 1U };
    body.oracle_inputs = (lxp_byte_span){ sections[3], 2U };
    body.state_diff = (lxp_byte_span){ sections[4], 3U };
    body.recovery_metadata = (lxp_byte_span){ sections[5], 2U };
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_batch_body_encode(&body, &arena, &canonical) != LXP_OK ||
        lxp_batch_body_decode(canonical.bytes, canonical.length, &decoded) !=
            LXP_OK || decoded.header.batch_number != 14U ||
        decoded.recovery_metadata.length != 2U) return 11;
    canonical_length = canonical.length;
    (void)memcpy(canonical_copy, canonical.bytes, canonical_length);
    (void)memset(replicas, 0, sizeof(replicas));
    for (i = 0U; i < 3U; ++i)
        (void)memcpy(replicas[i].replica_id, replica_ids[i], 32U);
    (void)memset(&output, 0, sizeof(output));
    output.fail_call = 2U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_batch_publish(&body, replicas, 3U, 128U, receive_chunk,
                          &output, &arena) != LXP_ERR_IO ||
        replicas[0].resume_offset == 0U) return 12;
    output.fail_call = SIZE_MAX;
    if (lxp_batch_publish(&body, replicas, 3U, 128U, receive_chunk,
                          &output, &arena) != LXP_OK) return 13;
    for (i = 0U; i < 3U; ++i) {
        if (replicas[i].complete != 1U) return (int)(20U + i);
        if (output.lengths[i] != canonical_length) return (int)(30U + i);
        if (memcmp(output.bytes[i], canonical_copy, canonical_length) != 0)
            return (int)(40U + i);
    }
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        lxp_batch_eligibility_init(&eligibility, 14U,
            (const uint8_t (*)[32])replica_ids, 3U, 2U) !=
            LXP_OK ||
        lxp_batch_eligibility(&eligibility, &eligible) !=
            LXP_ERR_ATTESTATION_THRESHOLD || eligible ||
        lxp_replica_ack(&eligibility, replica_ids[0], &log) != LXP_OK ||
        lxp_batch_eligibility(&eligibility, &eligible) !=
            LXP_ERR_ATTESTATION_THRESHOLD || eligible ||
        lxp_replica_ack(&eligibility, replica_ids[1], &log) != LXP_OK ||
        lxp_batch_eligibility(&eligibility, &eligible) != LXP_OK || !eligible)
        return 15;
    if (lxp_log_read(&log, 0U, &record, acknowledged_id,
                     sizeof(acknowledged_id)) != LXP_OK ||
        record.record_kind != (uint8_t)LXP_LOG_REPLICA_ACK ||
        record.global_sequence != 14U || record.body_length != 32U ||
        memcmp(acknowledged_id, replica_ids[0], 32U) != 0)
        return 16;
    if (lxp_batch_eligibility_reset(&eligibility, 13U) !=
            LXP_ERR_BATCH_GAP ||
        lxp_batch_eligibility_reset(&eligibility, 14U) != LXP_OK ||
        eligibility.acknowledgement_count != 2U ||
        lxp_batch_eligibility(&eligibility, &eligible) != LXP_OK ||
        !eligible ||
        lxp_batch_eligibility_reset(&eligibility, 15U) != LXP_OK ||
        eligibility.batch_number != 15U ||
        eligibility.acknowledgement_count != 0U ||
        eligibility.replica_count != 3U || eligibility.threshold != 2U ||
        lxp_batch_eligibility(&eligibility, &eligible) !=
            LXP_ERR_ATTESTATION_THRESHOLD || eligible)
        return 17;
    if (lxp_replica_ack(&eligibility, replica_ids[2], &log) != LXP_OK ||
        lxp_replica_ack(&eligibility, replica_ids[2], &log) != LXP_OK ||
        eligibility.acknowledgement_count != 1U ||
        lxp_batch_eligibility(&eligibility, &eligible) !=
            LXP_ERR_ATTESTATION_THRESHOLD || eligible ||
        lxp_replica_ack(&eligibility, replica_ids[0], &log) != LXP_OK ||
        eligibility.acknowledgement_count != 2U ||
        lxp_batch_eligibility(&eligibility, &eligible) != LXP_OK || !eligible)
        return 18;
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 19;
    return 0;
}
