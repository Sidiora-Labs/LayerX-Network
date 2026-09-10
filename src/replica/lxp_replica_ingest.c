#include "layerx/lxp_replica.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_sequencer.h"

#include <string.h>

lxp_result lxp_replica_init(lxp_replica *replica, lxp_log *log)
{
    if (replica == NULL || log == NULL || log->descriptor < 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(replica, 0, sizeof(*replica));
    replica->log = log;
    replica->execution_enabled = true;
    replica->acknowledgements_enabled = true;
    replica->serving_current_state = true;
    replica->serving_finalised_history = true;
    return LXP_OK;
}

lxp_result lxp_replica_bind_execution(lxp_replica *replica,
                                      lxp_replay_engine *engine,
                                      const uint8_t starting_state_root[32],
                                      uint64_t next_batch_number,
                                      uint64_t next_sequence)
{
    if (replica == NULL || replica->log == NULL || engine == NULL ||
        starting_state_root == NULL || engine->kernel == NULL ||
        replica->has_execution || replica->has_head || replica->halted)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_memcmp(engine->kernel->current_state_root,
                      starting_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    replica->engine = engine;
    (void)memcpy(replica->state_root, starting_state_root, 32U);
    replica->next_batch_number = next_batch_number;
    replica->next_sequence = next_sequence;
    replica->has_execution = true;
    return LXP_OK;
}

lxp_result lxp_replica_bind_eligibility(lxp_replica *replica,
                                        const uint8_t replica_id[32],
                                        const uint8_t (*replica_ids)[32],
                                        size_t replica_count,
                                        size_t threshold)
{
    size_t i;
    lxp_result status;
    if (replica == NULL || replica_id == NULL || replica_ids == NULL ||
        replica->has_eligibility || replica->halted)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_eligibility_init(&replica->eligibility,
                                        replica->next_batch_number,
                                        replica_ids, replica_count,
                                        threshold);
    if (status != LXP_OK) return status;
    for (i = 0U; i < replica_count; ++i)
        if (lxp_ct_memcmp(replica_id, replica_ids[i], 32U) == 0) break;
    if (i == replica_count) {
        (void)memset(&replica->eligibility, 0, sizeof(replica->eligibility));
        return LXP_ERR_AUTH_SCOPE;
    }
    (void)memcpy(replica->replica_id, replica_id, 32U);
    replica->has_eligibility = true;
    return LXP_OK;
}

lxp_result lxp_replica_batch_eligible(const lxp_replica *replica,
                                      bool *eligible)
{
    if (replica == NULL || eligible == NULL) return LXP_ERR_NON_CANONICAL;
    *eligible = false;
    if (!replica->has_eligibility) return LXP_ERR_MODULE_DISABLED;
    return lxp_batch_eligibility(&replica->eligibility, eligible);
}

lxp_result lxp_replica_validate_header(
    const lxp_batch_body *body, uint32_t configured_network_id,
    const lxp_sequencer_authorization *authorization, lxp_arena *arena)
{
    if (body == NULL || authorization == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_protocol_version_supported(body->header.protocol_version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (!lxp_network_id_matches(configured_network_id,
                                body->header.network_id))
        return LXP_ERR_WRONG_NETWORK;
    return lxp_batch_verify_signature(&body->header,
                                      body->sequencer_signature, 64U,
                                      authorization, arena);
}

lxp_result lxp_replica_chain_link(const lxp_batch_header *previous,
                                  const lxp_batch_header *candidate)
{
    return lxp_batch_range_check(previous, candidate);
}

lxp_result lxp_replica_ingest_batch(
    lxp_replica *replica, const uint8_t *canonical_body, size_t body_length,
    uint32_t configured_network_id,
    const lxp_sequencer_authorization *authorization, lxp_arena *arena,
    bool *acknowledge)
{
    lxp_batch_body decoded;
    lxp_replay_batch_result executed;
    lxp_byte_span reencoded;
    uint8_t resulting_state_root[32];
    size_t mark;
    lxp_result status;
    if (replica == NULL || replica->log == NULL ||
        (canonical_body == NULL && body_length != 0U) || arena == NULL ||
        acknowledge == NULL) return LXP_ERR_NON_CANONICAL;
    *acknowledge = false;
    if (replica->halted || !replica->execution_enabled ||
        !replica->acknowledgements_enabled)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (!replica->has_execution || replica->engine == NULL)
        return LXP_ERR_MODULE_DISABLED;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_body_decode(canonical_body, body_length, &decoded);
    if (status == LXP_OK)
        status = lxp_batch_body_encode(&decoded, arena, &reencoded);
    if (status == LXP_OK &&
        (reencoded.length != body_length ||
         lxp_ct_memcmp(reencoded.bytes, canonical_body, body_length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_replica_validate_header(&decoded,
                    configured_network_id, authorization, arena);
    if (status == LXP_OK && replica->has_head)
        status = lxp_replica_chain_link(&replica->head, &decoded.header);
    if (status == LXP_OK && !replica->has_head &&
        (decoded.header.batch_number != replica->next_batch_number ||
         decoded.header.first_sequence != replica->next_sequence ||
         decoded.header.last_sequence < decoded.header.first_sequence))
        status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK &&
        lxp_ct_memcmp(decoded.header.previous_state_root,
                      replica->state_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK && body_length > UINT32_MAX)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    status = lxp_replay_batch_publication(replica->engine, &decoded,
                                          replica->state_root, arena,
                                          &executed);
    if (status != LXP_OK) {
        (void)lxp_replica_halt(replica);
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    (void)memcpy(resulting_state_root, executed.resulting_state_root, 32U);
    status = lxp_log_append(replica->log, LXP_LOG_BATCH_BODY,
                            decoded.header.last_sequence, canonical_body,
                            (uint32_t)body_length, NULL);
    if (status == LXP_OK) status = lxp_log_write_boundary(replica->log);
    if (status == LXP_OK) {
        replica->head = decoded.header;
        replica->has_head = true;
        replica->durable_batch_count += 1U;
        replica->executed_batch_count += 1U;
        (void)memcpy(replica->state_root, resulting_state_root, 32U);
        replica->next_batch_number = decoded.header.batch_number + 1U;
        replica->next_sequence = decoded.header.last_sequence + 1U;
    }
    if (status == LXP_OK && replica->has_eligibility) {
        status = lxp_batch_eligibility_reset(&replica->eligibility,
                                             decoded.header.batch_number);
        if (status == LXP_OK)
            status = lxp_replica_ack(&replica->eligibility,
                                     replica->replica_id, replica->log);
    }
    if (status == LXP_OK) {
        replica->acknowledged_batch_count += 1U;
        *acknowledge = true;
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}
