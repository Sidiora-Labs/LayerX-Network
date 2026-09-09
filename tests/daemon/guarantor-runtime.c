#define _POSIX_C_SOURCE 200809L
#include "../../cmd/layerx-guarantor/runtime.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv)
{
    gp_runtime *runtime = NULL;
    lxp_log log;
    lxp_arena arena;
    uint8_t *memory = malloc(128U * 1024U * 1024U);
    lxp_result status;
    unsigned long count;
    char *end;
    assert(argc == 5 && memory);
    count = strtoul(argv[4], &end, 10);
    assert(*end == '\0' && count > 0U);
    assert(lxp_arena_init(&arena, memory, 128U * 1024U * 1024U) == LXP_OK);
    status = gp_runtime_open(&runtime, argv[1], argv[2]);
    if (status != LXP_OK) {
        fprintf(stderr, "runtime open refused: %d\n", (int)status);
        free(memory);
        return 1;
    }
    assert(gp_runtime_engine(runtime)->kernel != NULL);
    status = lxp_log_open(&log, argv[3]);
    if (status != LXP_OK) {
        gp_runtime_close(runtime);
        free(memory);
        return 1;
    }
    if (!log.has_durable_marker)
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK)
        status = lxp_log_recover_complete_records(&log, NULL, NULL);
    if (status != LXP_OK) {
        (void)lxp_log_close(&log);
        gp_runtime_close(runtime);
        free(memory);
        return 1;
    }
    for (unsigned long batch = 1; batch <= count; batch++) {
        lxp_batch_body body, mismatch;
        lxp_replay_batch_result replay;
        lxp_batch_roots roots;
        lxp_replay_engine *engine = gp_runtime_engine(runtime);
        uint8_t initial_root[32];
        (void)lxp_arena_reset(&arena, 0U);
        status = lxp_da_log_read_body(&log, batch, &arena, &body);
        if (status != LXP_OK)
            break;
        memcpy(initial_root, engine->kernel->current_state_root, 32U);
        mismatch = body;
        mismatch.header.previous_state_root[0] ^= 1U;
        assert(gp_runtime_prepare(runtime, &mismatch) != LXP_OK);
        assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
        status = gp_runtime_prepare(runtime, &body);
        if (status != LXP_OK)
            break;
        lxp_byte_span *activities;
        size_t activity_count;
        assert(lxp_replay_section_decode(&body.activities, &arena, &activities, &activity_count) ==
               LXP_OK);
        for (size_t index = 0; index < activity_count; index++) {
            lxp_activity activity, altered;
            lxp_guarantor_authority_verdict verdict;
            assert(lxp_activity_decode(activities[index].bytes, activities[index].length,
                                       &activity) == LXP_OK);
            assert(gp_runtime_authority(runtime, &activity, activities[index], &verdict) == LXP_OK);
            assert(verdict.actor_signature && verdict.session_key && verdict.capability_grant &&
                   verdict.delegated_authority);
            altered = activity;
            altered.network_id ^= 1U;
            assert(gp_runtime_authority(runtime, &altered, activities[index], &verdict) != LXP_OK);
            assert(!verdict.actor_signature);
        }
        bool valid = true;
        assert(gp_runtime_oracle(runtime, (lxp_byte_span){(const uint8_t *)"oracle", 6U}, &valid) !=
                   LXP_OK &&
               !valid);
        status = lxp_replay_batch_publication(engine, &body, initial_root, &arena, &replay);
        if (status != LXP_OK)
            break;
        status = lxp_guarantor_recompute_roots(&body, &replay, &arena, &roots);
        if (status != LXP_OK)
            break;
        assert(!memcmp(engine->kernel->current_state_root, body.header.resulting_state_root, 32U));
        {
            lxp_state_witness *proof = malloc(sizeof(*proof));
            uint8_t *wire = malloc(LXP_STATE_WITNESS_MAX_BYTES);
            size_t wire_length;
            assert(proof != NULL && wire != NULL);
            assert(gp_runtime_state_proof(runtime, 0U,
                       (lxp_byte_span){(const uint8_t *)"account-tree", 12U}, proof) == LXP_OK);
            assert(lxp_state_proof_verify(proof, body.header.resulting_state_root) == LXP_OK);
            assert(lxp_state_proof_encode(proof, wire, LXP_STATE_WITNESS_MAX_BYTES,
                                           &wire_length) == LXP_OK);
            assert(lxp_state_proof_decode(wire, wire_length, proof) == LXP_OK);
            assert(lxp_state_proof_verify(proof, body.header.resulting_state_root) == LXP_OK);
            for (size_t i = 0U; i < engine->kernel->state->accounts->count; ++i) {
                const lx_account *account = &engine->kernel->state->accounts->accounts[i];
                uint8_t key[33] = {4U};
                memcpy(key + 1U, account->id, 32U);
                assert(gp_runtime_state_proof(runtime, 0U,
                    (lxp_byte_span){key, sizeof(key)}, proof) == LXP_OK);
                assert(lxp_state_proof_verify(proof, body.header.resulting_state_root) == LXP_OK);
                assert(lxp_state_proof_encode(proof, wire, LXP_STATE_WITNESS_MAX_BYTES,
                    &wire_length) == LXP_OK);
                assert(lxp_state_proof_decode(wire, wire_length, proof) == LXP_OK);
                assert(lxp_state_proof_verify(proof, body.header.resulting_state_root) == LXP_OK);
            }
            for (size_t i = 0U; i < engine->kernel->module_kv_count; ++i) {
                const lxp_module_kv_entry *entry = &engine->kernel->module_kv[i];
                assert(gp_runtime_state_proof(runtime, entry->module_id,
                           (lxp_byte_span){entry->key, entry->key_length}, proof) == LXP_OK);
                assert(lxp_state_proof_verify(proof, body.header.resulting_state_root) == LXP_OK);
                assert(proof->value_length == entry->value_length);
                assert(memcmp(proof->value, entry->value, entry->value_length) == 0);
            }
            fprintf(stdout, "state witness v2 verified batch=%lu account-tree=1 account-proofs=all module-kv=%zu accounts=%zu\n",
                    batch, engine->kernel->module_kv_count, engine->kernel->state->accounts->count);
            free(wire);
            free(proof);
        }
        if (getenv("LAYERX_TEST_PUBLICATION_EXPORT_DIR") != NULL) {
            char path[4096];
            lxp_byte_span encoded;
            FILE *output;
            int written = snprintf(path, sizeof(path), "%s/%lu.json",
                getenv("LAYERX_TEST_PUBLICATION_EXPORT_DIR"), batch);
            assert(written > 0 && (size_t)written < sizeof(path));
            assert(lxp_batch_header_encode(&body.header, &arena, &encoded) == LXP_OK);
            output = fopen(path, "wx");
            assert(output != NULL);
            fputs("{\"canonical_header\":\"0x", output);
            for (size_t i = 0U; i < encoded.length; ++i) fprintf(output, "%02x", encoded.bytes[i]);
            fputs("\",\"native_facts\":", output);
            assert(gp_runtime_settlement_facts(runtime, output) == LXP_OK);
            fputs("}\n", output);
            assert(fclose(output) == 0);
        }
        fprintf(stdout,
                "guarantor runtime independently replayed batch=%lu activities=%zu receipts=%zu\n",
                batch, activity_count, replay.receipt_count);
    }
    if (status != LXP_OK)
        fprintf(stderr, "runtime qualification refused: %d\n", (int)status);
    assert(lxp_log_close(&log) == LXP_OK);
    gp_runtime_close(runtime);
    free(memory);
    return status == LXP_OK ? 0 : 1;
}
