#define _POSIX_C_SOURCE 200809L
#include "../../cmd/layerx-guarantor/runtime.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_maintenance.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/resource.h>
#include <signal.h>
#include <unistd.h>
#include <openssl/evp.h>

static void write_u32(FILE *output, uint32_t value)
{
    uint8_t bytes[4] = {(uint8_t)(value >> 24U), (uint8_t)(value >> 16U),
                        (uint8_t)(value >> 8U), (uint8_t)value};
    assert(fwrite(bytes, sizeof(bytes), 1U, output) == 1U);
}

static FILE *public_export(const char *name)
{
    char path[4096];
    const char *directory = getenv("LAYERX_TEST_PUBLICATION_EXPORT_DIR");
    assert(directory != NULL);
    int length = snprintf(path, sizeof(path), "%s/%s", directory, name);
    assert(length > 0 && (size_t)length < sizeof(path));
    FILE *output = fopen(path, "wx");
    assert(output != NULL);
    return output;
}

static void export_genesis(gp_runtime *runtime)
{
    const lxp_kernel *kernel = gp_runtime_engine(runtime)->kernel;
    if (!kernel->handover.enabled || getenv("LAYERX_TEST_PUBLICATION_EXPORT_DIR") == NULL)
        return;
    uint8_t key[32] = "handover-authority";
    uint8_t state_root[32], receipt_root[32];
    lxp_state_witness *proof = malloc(sizeof(*proof));
    uint8_t *encoded = malloc(LXP_STATE_WITNESS_MAX_BYTES);
    size_t length = 0U;
    assert(proof != NULL && encoded != NULL);
    assert(lxp_state_root(kernel, state_root) == LXP_OK);
    assert(lxp_genesis_receipt_state_root(kernel->handover.network_id, state_root,
        receipt_root) == LXP_OK);
    assert(memcmp(receipt_root, kernel->current_state_root, 32U) == 0);
    assert(gp_runtime_state_proof(runtime, LXP_MODULE_GOVERNANCE,
        (lxp_byte_span){key, sizeof(key)}, proof) == LXP_OK);
    assert(lxp_state_proof_verify(proof, state_root) == LXP_OK);
    assert(lxp_state_proof_encode(proof, encoded, LXP_STATE_WITNESS_MAX_BYTES, &length) == LXP_OK);
    assert(length <= UINT32_MAX && kernel->module_count <= UINT32_MAX);
    FILE *output = public_export("handover-genesis.bin");
    write_u32(output, kernel->handover.network_id);
    assert(fwrite(state_root, 32U, 1U, output) == 1U);
    assert(fwrite(kernel->handover.genesis_authorization.public_key, 32U, 1U, output) == 1U);
    write_u32(output, (uint32_t)length);
    assert(fwrite(encoded, length, 1U, output) == 1U);
    write_u32(output, (uint32_t)kernel->module_count);
    for (size_t i = 0U; i < kernel->module_count; ++i) {
        const lxp_module_registration *module = &kernel->modules[i];
        write_u32(output, module->module_id);
        assert(module->activity_type_count <= UINT32_MAX);
        write_u32(output, (uint32_t)module->activity_type_count);
        for (size_t j = 0U; j < module->activity_type_count; ++j)
            write_u32(output, module->activity_types[j]);
    }
    assert(fclose(output) == 0);
    free(encoded);
    free(proof);
}

static void export_forgery(const lxp_batch_body *body, const char *kind, lxp_arena *arena)
{
    if (getenv("LAYERX_TEST_PUBLICATION_EXPORT_DIR") == NULL)
        return;
    char name[128];
    int length = snprintf(name, sizeof(name), "%s-%llu-%llu.bin", kind,
        (unsigned long long)body->header.batch_number, (unsigned long long)body->header.epoch);
    assert(length > 0 && (size_t)length < sizeof(name));
    lxp_byte_span header;
    assert(lxp_batch_header_encode(&body->header, arena, &header) == LXP_OK && header.length <= UINT32_MAX);
    FILE *output = public_export(name);
    write_u32(output, (uint32_t)header.length);
    assert(fwrite(header.bytes, header.length, 1U, output) == 1U);
    assert(fwrite(body->sequencer_signature, 64U, 1U, output) == 1U);
    assert(fclose(output) == 0);
}

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
    {
        char directory[4096], path[4096];
        int length = snprintf(directory, sizeof(directory), "%s/halt-input-XXXXXX", argv[2]);
        assert(length > 0 && (size_t)length < sizeof(directory));
        assert(mkdtemp(directory) != NULL);
        length = snprintf(path, sizeof(path), "%s/replay-halt", directory);
        assert(length > 0 && (size_t)length < sizeof(path));
        assert(mkfifo(path, 0600) == 0);
        (void)alarm(5U);
        assert(gp_runtime_open(&runtime, argv[1], directory) == LXP_ERR_AUTH_SCOPE);
        (void)alarm(0U);
        assert(runtime == NULL && unlink(path) == 0 && rmdir(directory) == 0);
    }
    assert(lxp_arena_init(&arena, memory, 128U * 1024U * 1024U) == LXP_OK);
    status = gp_runtime_open(&runtime, argv[1], argv[2]);
    if (status != LXP_OK) {
        fprintf(stderr, "runtime open refused: %d\n", (int)status);
        free(memory);
        return 1;
    }
    assert(gp_runtime_engine(runtime)->kernel != NULL);
    export_genesis(runtime);
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
        if (engine->kernel->handover.enabled) {
            lxp_batch_body forged = body;
            lxp_sequencer_authorization claimed = {0};
            uint8_t unauthorized_key[32];
            uint64_t sequence_before = engine->kernel->state->next_sequence;
            memset(unauthorized_key, 0x66U, sizeof(unauthorized_key));
            EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                unauthorized_key, sizeof(unauthorized_key));
            size_t key_length = sizeof(claimed.public_key);
            assert(key != NULL && EVP_PKEY_get_raw_public_key(key, claimed.public_key, &key_length) == 1 &&
                key_length == sizeof(claimed.public_key));
            EVP_PKEY_free(key);
            assert(lxp_handover_sequencer_id(claimed.public_key, claimed.sequencer_id) == LXP_OK);
            claimed.first_batch_number = 1U;
            claimed.last_batch_number = UINT64_MAX;
            claimed.authorized = 1U;
            memcpy(forged.header.sequencer_id, claimed.sequencer_id, 32U);
            assert(lxp_batch_availability_root(&forged, &arena,
                forged.header.data_availability_root) == LXP_OK);
            assert(lxp_batch_sign(&forged.header, unauthorized_key, &claimed,
                forged.sequencer_signature, &arena) == LXP_OK);
            assert(lxp_batch_verify_signature(&forged.header, forged.sequencer_signature,
                sizeof(forged.sequencer_signature), &claimed, &arena) == LXP_OK);
            assert(gp_runtime_prepare(runtime, &forged) != LXP_OK);
            export_forgery(&forged, "unauthorized", &arena);
            assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
            assert(engine->kernel->state->next_sequence == sequence_before);
            lxp_secure_zero(unauthorized_key, sizeof(unauthorized_key));
        }
        if (engine->kernel->handover.enabled && engine->kernel->epoch > 1U) {
            lxp_sequencer_authorization retired = engine->kernel->handover.genesis_authorization;
            uint8_t retired_key[32];
            uint64_t sequence_before = engine->kernel->state->next_sequence;
            assert(retired.first_batch_number == 1U && retired.last_batch_number == UINT64_MAX);
            memset(retired_key, 0x22U, sizeof(retired_key));
            for (uint64_t claimed_epoch = 1U; claimed_epoch <= body.header.epoch; ++claimed_epoch) {
                lxp_batch_body forged = body;
                forged.header.epoch = claimed_epoch;
                memcpy(forged.header.sequencer_id, retired.sequencer_id, 32U);
                assert(lxp_batch_availability_root(&forged, &arena,
                    forged.header.data_availability_root) == LXP_OK);
                assert(lxp_batch_sign(&forged.header, retired_key, &retired,
                    forged.sequencer_signature, &arena) == LXP_OK);
                assert(lxp_batch_verify_signature(&forged.header, forged.sequencer_signature,
                    sizeof(forged.sequencer_signature), &retired, &arena) == LXP_OK);
                assert(gp_runtime_prepare(runtime, &forged) != LXP_OK);
                export_forgery(&forged, "retired", &arena);
                assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
                assert(engine->kernel->state->next_sequence == sequence_before);
            }
            lxp_secure_zero(retired_key, sizeof(retired_key));
        }
        if (batch == count && getenv("LAYERX_TEST_HANDOVER_DIVERGENCE") != NULL) {
            lxp_batch_body divergent = body;
            lxp_sequencer_authorization authorization;
            uint64_t epoch;
            uint8_t replacement_key[32];
            assert(engine->kernel->handover.enabled && body.header.epoch == 2U);
            memset(replacement_key, 0x44U, sizeof(replacement_key));
            assert(lxp_handover_history_resolve(engine->kernel, body.header.batch_number,
                &authorization, &epoch, &arena) == LXP_OK && epoch == 2U);
            divergent.header.resulting_state_root[0] ^= 1U;
            assert(lxp_batch_availability_root(&divergent, &arena,
                divergent.header.data_availability_root) == LXP_OK);
            assert(lxp_batch_sign(&divergent.header, replacement_key, &authorization,
                divergent.sequencer_signature, &arena) == LXP_OK);
            lxp_secure_zero(replacement_key, sizeof(replacement_key));
            assert(gp_runtime_prepare(runtime, &divergent) == LXP_OK);
            assert(engine->transaction_begin(engine->context) == LXP_OK);
            status = lxp_replay_batch_publication(engine, &divergent, initial_root, &arena, &replay);
            assert(status == LXP_ERR_ROOT_MISMATCH || status == LXP_FATAL_REPLAY_DIVERGENCE);
            assert(engine->transaction_finish(engine->context, false) == LXP_OK);
            assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
            assert(gp_runtime_note_divergence(runtime, &divergent.header, status) == LXP_OK);
            assert(gp_runtime_prepare(runtime, &body) == LXP_ERR_DA_MISSING);
            assert(lxp_log_close(&log) == LXP_OK);
            gp_runtime_close(runtime);
            runtime = NULL;
            assert(gp_runtime_open(&runtime, argv[1], argv[2]) != LXP_OK);
            if (runtime != NULL) gp_runtime_close(runtime);
            free(memory);
            puts("authenticated conflicting root refused and local divergence halt survives restart");
            return 0;
        }
        status = gp_runtime_prepare(runtime, &body);
        if (status != LXP_OK) {
            fprintf(stderr, "prepare batch=%lu refused: %d\n", batch, (int)status);
            break;
        }
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
        {
            char feed_path[4096];
            struct stat before, staged, restored;
            uint64_t sequence_before = engine->kernel->state->next_sequence;
            uint8_t replayed_root[32];
            int written = snprintf(feed_path, sizeof(feed_path), "%s/replay-feed.log", argv[2]);
            assert(written > 0 && (size_t)written < sizeof(feed_path));
            assert(stat(feed_path, &before) == 0);
            assert(engine->transaction_begin != NULL && engine->transaction_finish != NULL);
            assert(engine->transaction_begin(engine->context) == LXP_OK);
            status = lxp_replay_batch_publication(engine, &body, initial_root, &arena, &replay);
            if (status != LXP_OK) fprintf(stderr, "transaction replay batch=%lu refused: %d\n", batch, (int)status);
            assert(status == LXP_OK);
            memcpy(replayed_root, engine->kernel->current_state_root, 32U);
            assert(stat(feed_path, &staged) == 0 && staged.st_size == before.st_size);
            assert(engine->transaction_finish(engine->context, false) == LXP_OK);
            assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
            assert(engine->kernel->state->next_sequence == sequence_before);
            assert(stat(feed_path, &restored) == 0 && restored.st_size == before.st_size);
            assert(gp_runtime_prepare(runtime, &body) == LXP_OK);
            assert(engine->transaction_begin(engine->context) == LXP_OK);
            status = lxp_replay_batch_publication(engine, &body, initial_root, &arena, &replay);
            assert(status == LXP_OK);
            assert(!memcmp(replayed_root, engine->kernel->current_state_root, 32U));
            if (batch == 1U) {
                struct rlimit original, limited;
                struct sigaction previous, ignore = {0};
                assert(getrlimit(RLIMIT_FSIZE, &original) == 0);
                limited = original;
                limited.rlim_cur = (rlim_t)before.st_size + 1U;
                assert(limited.rlim_cur <= original.rlim_max);
                ignore.sa_handler = SIG_IGN;
                assert(sigemptyset(&ignore.sa_mask) == 0);
                assert(sigaction(SIGXFSZ, &ignore, &previous) == 0);
                assert(setrlimit(RLIMIT_FSIZE, &limited) == 0);
                assert(engine->transaction_finish(engine->context, true) == LXP_ERR_IO);
                assert(setrlimit(RLIMIT_FSIZE, &original) == 0);
                assert(sigaction(SIGXFSZ, &previous, NULL) == 0);
                assert(stat(feed_path, &staged) == 0 && staged.st_size == before.st_size + 1);
                assert(engine->transaction_finish(engine->context, false) == LXP_OK);
                assert(stat(feed_path, &restored) == 0 && restored.st_size == before.st_size);
                assert(!memcmp(initial_root, engine->kernel->current_state_root, 32U));
                assert(engine->kernel->state->next_sequence == sequence_before);
                assert(gp_runtime_prepare(runtime, &body) == LXP_OK);
                assert(engine->transaction_begin(engine->context) == LXP_OK);
                status = lxp_replay_batch_publication(engine, &body, initial_root, &arena, &replay);
                assert(status == LXP_OK);
                assert(!memcmp(replayed_root, engine->kernel->current_state_root, 32U));
            }
            assert(engine->transaction_finish(engine->context, true) == LXP_OK);
            assert(stat(feed_path, &restored) == 0 && restored.st_size >= before.st_size);
        }
        if (status != LXP_OK)
            break;
        status = lxp_guarantor_recompute_roots(&body, &replay, &arena, &roots);
        if (status != LXP_OK) {
            fprintf(stderr, "independent roots batch=%lu refused: %d\n", batch, (int)status);
            break;
        }
        if (lxp_batch_maintenance_is_envelope(replay.encoded_batch_maintenance_receipt)) {
            lxp_replay_batch_result incomplete = replay;
            lxp_batch_roots rejected;
            assert(incomplete.event_count == incomplete.activity_count + 1U);
            incomplete.event_count--;
            assert(lxp_guarantor_recompute_roots(&body, &incomplete, &arena, &rejected) != LXP_OK);
        }
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
