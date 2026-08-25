#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static int authority_complete(const lxp_guarantor_authority_verdict *verdict)
{
    return verdict->actor_signature && verdict->session_key &&
           verdict->capability_grant && verdict->delegated_authority;
}

lxp_result lxp_guarantor_verify_signatures(lxp_guarantor_ctx *ctx,
                                           const lxp_batch_body *body,
                                           lxp_arena *arena)
{
    lxp_byte_span *activities = NULL;
    lxp_byte_span *oracles = NULL;
    size_t activity_count = 0U;
    size_t oracle_count = 0U;
    size_t i;
    lxp_result status;
    if (ctx == NULL || body == NULL || arena == NULL ||
        ctx->verify_authority == NULL || ctx->verify_oracle == NULL ||
        ctx->sequencer_authorization == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_replay_section_decode(&body->activities, arena, &activities,
                                       &activity_count);
    for (i = 0U; status == LXP_OK && i < activity_count; ++i) {
        lxp_activity activity;
        lxp_guarantor_authority_verdict verdict = {false, false, false, false};
        status = lxp_activity_decode(activities[i].bytes,
                                     activities[i].length, &activity);
        if (status == LXP_OK)
            status = ctx->verify_authority(ctx->authority_context, &activity,
                                           activities[i], &verdict);
        if (status == LXP_OK && !authority_complete(&verdict))
            status = LXP_ERR_BAD_SIGNATURE;
    }
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body->oracle_inputs, arena,
                                           &oracles, &oracle_count);
    for (i = 0U; status == LXP_OK && i < oracle_count; ++i) {
        bool valid = false;
        status = ctx->verify_oracle(ctx->oracle_context, oracles[i], &valid);
        if (status == LXP_OK && !valid) status = LXP_ERR_BAD_SIGNATURE;
    }
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(
            &body->header, body->sequencer_signature,
            sizeof(body->sequencer_signature), ctx->sequencer_authorization,
            arena);
    return status;
}

lxp_result lxp_guarantor_recompute_roots(
    const lxp_batch_body *body, const lxp_replay_batch_result *replay,
    lxp_arena *arena, lxp_batch_roots *roots)
{
    lxp_byte_span *activities = NULL;
    lxp_byte_span *oracles = NULL;
    lxp_byte_span availability[5];
    lxp_batch_root_inputs inputs;
    size_t activity_count = 0U;
    size_t oracle_count = 0U;
    lxp_result status;
    if (body == NULL || replay == NULL || arena == NULL || roots == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_replay_section_decode(&body->activities, arena, &activities,
                                       &activity_count);
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body->oracle_inputs, arena,
                                           &oracles, &oracle_count);
    if (status != LXP_OK || activity_count != replay->activity_count)
        return status == LXP_OK ? LXP_FATAL_REPLAY_DIVERGENCE : status;
    availability[0] = body->activities;
    availability[1] = replay->canonical_receipt_section;
    availability[2] = body->oracle_inputs;
    availability[3] = body->state_diff;
    availability[4] = body->recovery_metadata;
    inputs = (lxp_batch_root_inputs){
        activities, activity_count,
        replay->encoded_receipts, replay->activity_count,
        replay->encoded_events, replay->activity_count,
        oracles, oracle_count, availability, 5U
    };
    status = lxp_batch_roots_compute(&inputs, arena, roots);
    if (status != LXP_OK) return status;
#define ROOT_MATCH(field) \
    (lxp_ct_memcmp(roots->field, body->header.field, 32U) == 0)
    if (lxp_ct_memcmp(replay->resulting_state_root,
                      body->header.resulting_state_root, 32U) != 0 ||
        !ROOT_MATCH(activity_merkle_root) ||
        !ROOT_MATCH(receipt_merkle_root) ||
        !ROOT_MATCH(event_merkle_root) || !ROOT_MATCH(oracle_root) ||
        !ROOT_MATCH(data_availability_root))
        return LXP_FATAL_REPLAY_DIVERGENCE;
#undef ROOT_MATCH
    return LXP_OK;
}

lxp_result lxp_guarantor_process_batch(lxp_guarantor_ctx *ctx,
                                       uint64_t batch_number,
                                       lxp_arena *arena,
                                       bool *ready_to_sign)
{
    lxp_byte_span canonical;
    lxp_byte_span reencoded;
    lxp_batch_body body;
    lxp_replay_batch_result replay;
    lxp_batch_roots roots;
    size_t mark;
    lxp_result status;
    if (ctx == NULL || arena == NULL || ready_to_sign == NULL ||
        ctx->download == NULL || ctx->store_availability == NULL ||
        ctx->replay_engine == NULL || !ctx->bond_view.bonded ||
        !lxp_protocol_version_supported(ctx->protocol_version) ||
        ctx->network_id == 0U)
        return LXP_ERR_NON_CANONICAL;
    *ready_to_sign = false;
    ctx->ready_to_sign = false;
    ctx->possesses_availability = false;
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_NONE;
    mark = lxp_arena_mark(arena);
    status = ctx->download(ctx->download_context, batch_number, arena,
                           &canonical);
    if (status == LXP_OK)
        status = lxp_batch_body_decode(canonical.bytes, canonical.length,
                                       &body);
    if (status == LXP_OK &&
        (body.header.protocol_version != ctx->protocol_version ||
         body.header.network_id != ctx->network_id))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK && body.header.batch_number != batch_number)
        status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK)
        status = lxp_batch_body_encode(&body, arena, &reencoded);
    if (status == LXP_OK &&
        (reencoded.length != canonical.length ||
         lxp_ct_memcmp(reencoded.bytes, canonical.bytes, canonical.length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_DOWNLOADED;
    if (status == LXP_OK)
        status = lxp_guarantor_verify_signatures(ctx, &body, arena);
    if (status == LXP_OK)
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_SIGNATURES;
    if (status == LXP_OK)
        status = lxp_replay_batch(ctx->replay_engine, &body,
                                  ctx->independent_state_root, arena, &replay);
    if (status == LXP_OK)
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_REPLAYED;
    if (status == LXP_OK)
        status = lxp_guarantor_recompute_roots(&body, &replay, arena, &roots);
    if (status == LXP_OK)
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_ROOTS;
    if (status == LXP_OK)
        status = ctx->store_availability(ctx->storage_context, batch_number,
                                         canonical.bytes, canonical.length);
    if (status == LXP_OK) {
        ctx->possesses_availability = true;
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_STORED;
        (void)memcpy(ctx->independent_state_root,
                     replay.resulting_state_root, 32U);
        ctx->ready_to_sign = true;
        ctx->last_completed_duty = LXP_GUARANTOR_DUTY_READY_TO_SIGN;
        *ready_to_sign = true;
    }
    (void)lxp_arena_reset(arena, mark);
    if (status != LXP_OK && ctx->publish_dissent != NULL &&
        (status == LXP_ERR_BAD_SIGNATURE ||
         status == LXP_FATAL_REPLAY_DIVERGENCE ||
         status == LXP_ERR_ROOT_MISMATCH)) {
        uint8_t expected[4] = {0U, 0U, 0U, 0U};
        uint8_t produced[4];
        lxp_guarantor_divergence divergence;
        lxp_guarantor_dissent_record dissent;
        uint32_t code = (uint32_t)status;
        produced[0] = (uint8_t)(code >> 24U);
        produced[1] = (uint8_t)(code >> 16U);
        produced[2] = (uint8_t)(code >> 8U);
        produced[3] = (uint8_t)code;
        (void)memset(&divergence, 0, sizeof(divergence));
        divergence.batch_number = batch_number;
        divergence.global_sequence = body.header.first_sequence;
        divergence.component = status == LXP_ERR_BAD_SIGNATURE ?
            LXP_GUARANTOR_DIVERGENCE_SIGNATURE :
            LXP_GUARANTOR_DIVERGENCE_STATE_ROOT;
        if (lxp_hash_sha256(expected, sizeof(expected),
                            divergence.expected_hash) == LXP_OK &&
            lxp_hash_sha256(produced, sizeof(produced),
                            divergence.produced_hash) == LXP_OK)
            return lxp_guarantor_withhold(ctx, body.header.epoch,
                                          &divergence, &dissent);
    }
    return status;
}
