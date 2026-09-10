#define _POSIX_C_SOURCE 200809L

#include "support/lxp_real_replay.h"

#include "layerx/lxp_replica.h"

#include <unistd.h>

#define CHECK(value) do { if (!(value)) { \
    (void)fprintf(stderr, "replica ingest check failed at %d\n", __LINE__); \
    return 1; \
} } while (0)

static int public_key_for(const uint8_t private_key[32],
                          uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                 private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

int main(void)
{
    static uint8_t history_storage[16U * 1024U * 1024U];
    static uint8_t ingest_storage[16U * 1024U * 1024U];
    static uint8_t bodies[3][65536];
    static uint8_t stored[65536];
    lxp_real_replay_fixture *builder = calloc(1U, sizeof(*builder));
    lxp_real_replay_fixture *follower = calloc(1U, sizeof(*follower));
    lxp_arena history_arena;
    lxp_arena ingest_arena;
    lxp_byte_span activities[6];
    lxp_batch_body batches[3];
    lxp_sequencer_authorization authorization;
    lxp_replica replica;
    lxp_log log;
    lxp_log_record_header record;
    size_t lengths[3];
    uint8_t private_key[32] = { 9U };
    uint8_t replica_ids[3][32] = { { 1U }, { 2U }, { 3U } };
    uint8_t genesis[32];
    bool ack = true;
    bool eligible = true;
    size_t i;
    char directory[] = "/tmp/lxp-replica-ingest-XXXXXX";
    char path[128];
    CHECK(builder != NULL && follower != NULL);
    CHECK(lxp_real_replay_init(builder) == 0);
    CHECK(lxp_real_replay_init(follower) == 0);
    CHECK(lxp_arena_init(&history_arena, history_storage,
                         sizeof(history_storage)) == LXP_OK);
    CHECK(lxp_arena_init(&ingest_arena, ingest_storage,
                         sizeof(ingest_storage)) == LXP_OK);
    (void)memcpy(genesis, follower->kernel.current_state_root, 32U);
    CHECK(memcmp(genesis, builder->kernel.current_state_root, 32U) == 0);
    (void)memset(&authorization, 0, sizeof(authorization));
    CHECK(public_key_for(private_key, authorization.public_key) == 0);
    authorization.first_batch_number = 0U;
    authorization.last_batch_number = 10U;
    authorization.authorized = 1U;
    for (i = 0U; i < 6U; ++i)
        CHECK(lxp_real_replay_activity(builder, i, &history_arena,
                                       &activities[i]) == 0);
    for (i = 0U; i < 3U; ++i) {
        lxp_byte_span encoded;
        CHECK(lxp_real_replay_build(builder, i + 1U, activities + i * 2U, 2U,
                                    NULL, 0U, &history_arena,
                                    &batches[i]) == 0);
        CHECK(lxp_batch_sign(&batches[i].header, private_key, &authorization,
                             batches[i].sequencer_signature,
                             &history_arena) == LXP_OK);
        if (i == 2U) {
            lxp_byte_span section = batches[i].receipts;
            CHECK(section.length != 0U);
            ((uint8_t *)section.bytes)[section.length - 1U] ^= 1U;
        }
        CHECK(lxp_batch_body_encode(&batches[i], &history_arena,
                                    &encoded) == LXP_OK);
        CHECK(encoded.length <= sizeof(bodies[i]));
        (void)memcpy(bodies[i], encoded.bytes, encoded.length);
        lengths[i] = encoded.length;
    }
    CHECK(mkdtemp(directory) != NULL);
    CHECK(snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) > 0);
    CHECK(lxp_log_segment_create(&log, directory, 0U,
                                 1024U * 1024U) == LXP_OK);
    CHECK(lxp_replica_init(&replica, &log) == LXP_OK);
    CHECK(lxp_replica_ingest_batch(&replica, bodies[0], lengths[0], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_ERR_MODULE_DISABLED);
    CHECK(!ack && !replica.has_head && replica.durable_batch_count == 0U);
    CHECK(lxp_replica_bind_execution(&replica, &follower->engine, genesis,
                                     batches[0].header.batch_number,
                                     batches[0].header.first_sequence) ==
          LXP_OK);
    CHECK(lxp_replica_bind_eligibility(&replica, replica_ids[0],
              (const uint8_t (*)[32])replica_ids, 3U, 2U) == LXP_OK);
    follower->execution.batch_number = batches[1].header.batch_number;
    CHECK(lxp_replica_ingest_batch(&replica, bodies[1], lengths[1], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_ERR_BATCH_GAP);
    CHECK(!ack && !replica.has_head && !replica.halted);
    CHECK(lxp_replica_ingest_batch(&replica, bodies[0], lengths[0] - 1U, 7U,
                                   &authorization, &ingest_arena, &ack) !=
          LXP_OK);
    CHECK(!ack && !replica.has_head && !replica.halted);
    CHECK(memcmp(follower->kernel.current_state_root, genesis, 32U) == 0);
    follower->execution.batch_number = batches[0].header.batch_number;
    CHECK(lxp_replica_ingest_batch(&replica, bodies[0], lengths[0], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_OK);
    CHECK(ack && replica.has_head && replica.durable_batch_count == 1U);
    CHECK(replica.executed_batch_count == 1U &&
          replica.acknowledged_batch_count == 1U);
    CHECK(memcmp(replica.state_root, batches[0].header.resulting_state_root,
                 32U) == 0);
    CHECK(memcmp(follower->kernel.current_state_root,
                 batches[0].header.resulting_state_root, 32U) == 0);
    CHECK(replica.eligibility.batch_number ==
              batches[0].header.batch_number &&
          replica.eligibility.acknowledgement_count == 1U);
    CHECK(lxp_replica_batch_eligible(&replica, &eligible) ==
          LXP_ERR_ATTESTATION_THRESHOLD && !eligible);
    CHECK(lxp_replica_ack(&replica.eligibility, replica_ids[1], &log) ==
          LXP_OK);
    CHECK(lxp_replica_batch_eligible(&replica, &eligible) == LXP_OK &&
          eligible);
    CHECK(lxp_replica_ingest_batch(&replica, bodies[0], lengths[0], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_ERR_BATCH_GAP);
    CHECK(!ack && replica.durable_batch_count == 1U && !replica.halted);
    follower->execution.batch_number = batches[1].header.batch_number;
    CHECK(lxp_replica_ingest_batch(&replica, bodies[1], lengths[1], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_OK);
    CHECK(ack && replica.durable_batch_count == 2U &&
          replica.acknowledged_batch_count == 2U);
    CHECK(memcmp(replica.state_root, batches[1].header.resulting_state_root,
                 32U) == 0);
    CHECK(replica.eligibility.batch_number ==
              batches[1].header.batch_number &&
          replica.eligibility.acknowledgement_count == 1U);
    CHECK(lxp_log_read(&log, 0U, &record, stored, sizeof(stored)) == LXP_OK);
    CHECK(record.record_kind == (uint8_t)LXP_LOG_BATCH_BODY);
    CHECK(record.body_length == lengths[0]);
    CHECK(memcmp(stored, bodies[0], lengths[0]) == 0);
    follower->execution.batch_number = batches[2].header.batch_number;
    CHECK(lxp_replica_ingest_batch(&replica, bodies[2], lengths[2], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_FATAL_REPLAY_DIVERGENCE);
    CHECK(!ack && replica.halted && replica.durable_batch_count == 2U &&
          replica.acknowledged_batch_count == 2U);
    CHECK(lxp_replica_ingest_batch(&replica, bodies[2], lengths[2], 7U,
                                   &authorization, &ingest_arena, &ack) ==
          LXP_FATAL_REPLAY_DIVERGENCE);
    CHECK(!ack);
    CHECK(lxp_log_close(&log) == LXP_OK);
    CHECK(unlink(path) == 0 && rmdir(directory) == 0);
    CHECK(lxp_state_store_destroy(&builder->state) == LXP_OK);
    CHECK(lxp_state_store_destroy(&follower->state) == LXP_OK);
    free(builder);
    free(follower);
    return 0;
}
