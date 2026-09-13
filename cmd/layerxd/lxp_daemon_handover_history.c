#include "lxp_daemon_handover_history.h"

#include "layerx/lxp_da.h"

#include <stdlib.h>
#include <string.h>

lxp_result lxp_daemon_handover_wal_verify(const lxp_handover_trust_chain *chain,
    const lxp_log *log, const lxp_daemon_batch_wal_input *input,
    lxp_handover_trust_finality_fn verify, void *context, lxp_arena *arena)
{
    lxp_handover_trust_chain *candidate;
    lxp_sequencer_authorization expected;
    lxp_batch_body body, original;
    lxp_byte_span expected_header;
    const lxp_batch_header *known_header;
    const uint8_t *known_signature;
    uint64_t epoch;
    size_t mark;
    lxp_result status;
    if (chain == NULL || log == NULL || input == NULL || verify == NULL || context == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    candidate = malloc(sizeof(*candidate));
    if (candidate == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *candidate = *chain;
    status = lxp_daemon_batch_wal_body(input, arena, &body);
    if (status == LXP_OK && candidate->predecessor.batch_number != UINT64_MAX &&
        input->batch_number == candidate->predecessor.batch_number + 1U)
        status = lxp_handover_trust_accept(candidate, &body, verify, context, arena);
    else if (status == LXP_OK && input->batch_number > candidate->predecessor.batch_number)
        status = LXP_ERR_BATCH_GAP;
    else if (status == LXP_OK) {
        known_header = &candidate->predecessor;
        known_signature = candidate->predecessor_signature;
        if (input->batch_number != candidate->predecessor.batch_number) {
            status = lxp_da_log_read_body(log, input->batch_number, arena, &original);
            known_header = &original.header;
            known_signature = original.sequencer_signature;
        }
        if (status == LXP_OK) status = lxp_batch_header_encode(known_header, arena, &expected_header);
        if (status == LXP_OK && (expected_header.length != input->canonical_header.length ||
            memcmp(expected_header.bytes, input->canonical_header.bytes, expected_header.length) != 0 ||
            memcmp(known_signature, input->header_signature, 64U) != 0))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
    }
    if (status == LXP_OK) status = lxp_handover_trust_authorization(candidate,
        input->batch_number, &expected, &epoch);
    if (status == LXP_OK && (epoch != input->epoch || input->authorization.authorized != 1U ||
        input->authorization.first_batch_number != expected.first_batch_number ||
        (input->authorization.last_batch_number != expected.last_batch_number &&
            input->authorization.last_batch_number != UINT64_MAX) ||
        memcmp(input->authorization.public_key, expected.public_key, 32U) != 0 ||
        memcmp(input->authorization.sequencer_id, expected.sequencer_id, 32U) != 0))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK) status = lxp_batch_verify_signature(&body.header,
        body.sequencer_signature, 64U, &expected, arena);
    free(candidate);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    return status;
}

typedef struct handover_wal_context {
    const lxp_handover_trust_chain *chain;
    const lxp_log *log;
    lxp_handover_trust_finality_fn verify;
    void *context;
    lxp_arena *arena;
} handover_wal_context;

static lxp_result authorize_wal(void *context, const lxp_daemon_batch_wal_input *input)
{
    const handover_wal_context *authorization = context;
    return lxp_daemon_handover_wal_verify(authorization->chain, authorization->log,
        input, authorization->verify, authorization->context, authorization->arena);
}

lxp_result lxp_daemon_handover_history_load(lxp_handover_trust_chain *chain,
    lxp_log *log, const char *checkpoint_directory,
    lxp_handover_trust_finality_fn verify, void *context, lxp_arena *arena)
{
    lxp_log recovered;
    lxp_daemon_batch_wal_record *record = NULL;
    lxp_handover_trust_chain *candidate;
    bool present = false, fallback = false;
    uint64_t offset = 0U;
    size_t mark;
    lxp_result status;
    if (chain == NULL || log == NULL || checkpoint_directory == NULL || verify == NULL ||
        context == NULL || arena == NULL || !log->has_durable_marker)
        return LXP_ERR_NON_CANONICAL;
    recovered = *log;
    recovered.allow_fallback_durable_marker = false;
    status = lxp_log_recover_complete_records(&recovered, NULL, NULL);
    if ((status == LXP_ERR_LOG_CORRUPT || status == LXP_ERR_LOG_TRUNCATED) &&
        log->has_fallback_durable_marker) {
        recovered = *log;
        recovered.durable_offset = log->fallback_durable_offset;
        recovered.durable_previous_record_offset = log->fallback_durable_previous_record_offset;
        recovered.durable_next_sequence = log->fallback_durable_next_sequence;
        recovered.durable_generation = log->fallback_durable_generation;
        recovered.has_fallback_durable_marker = false;
        recovered.allow_fallback_durable_marker = false;
        status = lxp_log_recover_complete_records(&recovered, NULL, NULL);
        fallback = status == LXP_OK;
    }
    if (status != LXP_OK) return status;
    candidate = malloc(sizeof(*candidate));
    if (candidate == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *candidate = *chain;
    mark = lxp_arena_mark(arena);
    status = lxp_handover_trust_scan_log(candidate, &recovered, &offset, verify, context, arena);
    if (status == LXP_OK && fallback) {
        handover_wal_context authorization = {candidate, &recovered, verify, context, arena};
        const lxp_daemon_batch_wal_input *input;
        lxp_batch_body body;
        lxp_byte_span encoded;
        status = lxp_daemon_batch_wal_read_authorized(checkpoint_directory,
            authorize_wal, &authorization, &record, &present);
        input = status == LXP_OK && present ? lxp_daemon_batch_wal_view(record) : NULL;
        if (status == LXP_OK && (input == NULL ||
            lxp_daemon_batch_wal_record_state(record) != LXP_DAEMON_BATCH_WAL_PREPARED ||
            input->first_sequence != recovered.durable_next_sequence ||
            input->last_sequence == UINT64_MAX ||
            input->last_sequence + 1U != log->durable_next_sequence ||
            candidate->predecessor.batch_number == UINT64_MAX ||
            input->batch_number != candidate->predecessor.batch_number + 1U ||
            log->durable_previous_record_offset != recovered.durable_offset))
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK) status = lxp_daemon_batch_wal_body(input, arena, &body);
        if (status == LXP_OK) status = lxp_batch_body_encode(&body, arena, &encoded);
        if (status == LXP_OK && (log->durable_offset < recovered.durable_offset ||
            log->durable_offset - recovered.durable_offset != LXP_LOG_HEADER_BYTES + encoded.length))
            status = LXP_ERR_LOG_CORRUPT;
    }
    if (status == LXP_OK) {
        *chain = *candidate;
        *log = recovered;
    }
    lxp_daemon_batch_wal_destroy(record);
    free(candidate);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    return status;
}
