#include "../../cmd/layerx-guarantor/lni.h"
#include "../../cmd/layerx-guarantor/producer.h"
#include "../../cmd/layerx-guarantor/runtime.h"
#include "../../cmd/layerx-guarantor/settlement.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_tools.h"
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int hex32(const char *text, uint8_t output[32])
{
    if (strlen(text) != 64U)
        return 0;
    for (size_t index = 0U; index < 32U; ++index) {
        unsigned value;
        if (sscanf(text + index * 2U, "%2x", &value) != 1)
            return 0;
        output[index] = (uint8_t)value;
    }
    return 1;
}

static int same_file(const char *path, lxp_byte_span bytes)
{
    FILE *file = fopen(path, "rb");
    int result = 1;
    if (file == NULL)
        return 0;
    for (size_t index = 0U; index < bytes.length; ++index)
        if (fgetc(file) != bytes.bytes[index]) {
            result = 0;
            break;
        }
    if (result && fgetc(file) != EOF)
        result = 0;
    if (ferror(file))
        result = 0;
    if (fclose(file) != 0)
        result = 0;
    return result;
}

static lxp_result verify_certificate(lxp_guarantor_lni *client, const lxp_da_bundle *bundle,
                                     const lxp_batch_body *body, lxp_arena *arena, char **paths)
{
    gp_runtime *runtime = NULL;
    gp_settlement_config config;
    lxp_guarantor_set set;
    lxp_guarantor_attestation attestations[2];
    lxp_guarantor_cert certificate;
    lxp_checkpoint_certificate checkpoint = {body->header, {NULL, 0U}};
    lxp_guarantor_key_record keys[LXP_MAX_GUARANTOR_ATTESTATIONS];
    lxp_byte_span served, proof;
    uint8_t verified[LXP_VERIFY_OUTPUT_BYTES], starting_root[32];
    size_t threshold = 0U;
    uint64_t delay = 0U;
    lxp_u128 minimum_bond = {0U, 0U};
    const char *state = getenv("LAYERX_GUARANTOR_STATE_DIR");
    const char *node_config = getenv("LAYERX_GUARANTOR_NODE_CONFIG");
    lxp_result status;
    memset(keys, 0, sizeof(keys));
    for (size_t i = 0U; i < 2U; ++i) {
        uint8_t bytes[GP_ATTESTATION_BYTES + 1U];
        FILE *file = fopen(paths[i], "rb");
        size_t length;
        if (file == NULL)
            return LXP_ERR_IO;
        length = fread(bytes, 1U, sizeof(bytes), file);
        if (ferror(file) || fclose(file) != 0)
            return LXP_ERR_IO;
        status = gp_attestation_decode(bytes, length, &attestations[i]);
        if (status != LXP_OK)
            return status;
    }
    status =
        lxp_guarantor_lni_checkpoint(client, body->header.batch_number, arena, &served, &proof);
    if (status != LXP_OK)
        return status;
    if (!same_file(paths[2], served) || !same_file(paths[3], proof))
        return LXP_ERR_CONTEXT_MISMATCH;
    status = gp_settlement_config_from_env(&config, state);
    if (status == LXP_OK)
        status = gp_settlement_membership(&config, body->header.epoch, &set, &threshold, &delay,
                                          &minimum_bond);
    if (status != LXP_OK)
        return status;
    if (threshold != 2U || set.count != 2U ||
        delay != lxp_checkpoint_maximum_attestation_delay_ms())
        return LXP_ERR_ATTESTATION_THRESHOLD;
    for (size_t i = 0U; i < set.count; ++i) {
        memcpy(keys[i].guarantor_id, set.records[i].guarantor_id, 32U);
        memcpy(keys[i].public_key, set.records[i].public_key, 33U);
        status = lxp_guarantor_eligible(&set.records[i], body->header.epoch, minimum_bond,
                                        &keys[i].bonded);
        if (status != LXP_OK || !keys[i].bonded)
            return LXP_ERR_ATTESTATION_THRESHOLD;
    }
    status = lxp_guarantor_cert_assemble(&checkpoint, attestations, 2U, threshold, &certificate);
    if (status == LXP_OK)
        status = gp_runtime_open(&runtime, node_config, state);
    if (status == LXP_OK)
        status = gp_runtime_prepare(runtime, body);
    if (status == LXP_OK) {
        lxp_verify_run run = {bundle,        &body->header, &certificate,
                              keys,          set.count,     gp_runtime_engine(runtime),
                              starting_root, arena};
        memcpy(starting_root, run.engine->kernel->current_state_root, 32U);
        status = lxp_verify_main(&run, verified);
    }
    gp_runtime_close(runtime);
    if (status == LXP_OK)
        fprintf(stderr, "guarantor-integration: tag14 matches registered bundle; layerx-verify "
                        "accepted real certificate and replay\n");
    return status;
}

int main(int argc, char **argv)
{
    lxp_guarantor_lni client = {.fd = -1};
    lxp_sequencer_authorization authority = {0};
    lxp_batch_header header;
    lxp_da_bundle bundle;
    lxp_batch_body body;
    lxp_byte_span encoded;
    lxp_arena arena;
    uint8_t signature[64];
    uint8_t *memory = NULL;
    uint64_t batch;
    unsigned long network;
    lxp_result status;
    FILE *output;
    char *end;
    int result = 1;
    if (argc != 7 && argc != 11) {
        fprintf(stderr,
                "usage: %s SOCKET BATCH NETWORK SEQUENCER_ID SEQUENCER_PUBLIC_KEY BODY_OUTPUT "
                "[ATTESTATION1 ATTESTATION2 CHECKPOINT FINALITY]\n",
                argv[0]);
        return 2;
    }
    errno = 0;
    batch = strtoull(argv[2], &end, 10);
    if (errno != 0 || *end != '\0' || batch == 0U)
        return 2;
    network = strtoul(argv[3], &end, 10);
    if (errno != 0 || *end != '\0' || network == 0UL || network > UINT32_MAX)
        return 2;
    if (!hex32(argv[4], authority.sequencer_id) || !hex32(argv[5], authority.public_key))
        return 2;
    authority.first_batch_number = 1U;
    authority.last_batch_number = UINT64_MAX;
    authority.authorized = 1U;
    memory = malloc(128U * 1024U * 1024U);
    if (memory == NULL)
        return 1;
    status = lxp_arena_init(&arena, memory, 128U * 1024U * 1024U);
    if (status != LXP_OK)
        goto finished;
    status = lxp_guarantor_lni_open(&client, argv[1], 5000U);
    if (status != LXP_OK)
        goto finished;
    status = lxp_guarantor_lni_header(&client, batch, &authority, (uint32_t)network, &arena,
                                      &header, signature);
    if (status != LXP_OK) {
        fprintf(stderr, "guarantor-integration: tag12 header refused result=%d\n", (int)status);
        goto finished;
    }
    fprintf(stderr, "guarantor-integration: signed tag12 header verified batch=%" PRIu64 "\n",
            batch);
    status = lxp_guarantor_lni_fetch(&client, &header, &arena, &bundle);
    if (status != LXP_OK) {
        fprintf(stderr,
                "guarantor-integration: candidate selector=%02x refused result=%d; no signature or "
                "registration performed\n",
                LXP_GUARANTOR_CANDIDATE_SELECTOR, (int)status);
        goto finished;
    }
    status = lxp_da_bundle_body(&bundle, &header, &arena, &body);
    if (status != LXP_OK)
        goto finished;
    memcpy(body.sequencer_signature, signature, sizeof(signature));
    status = lxp_batch_body_encode(&body, &arena, &encoded);
    if (status != LXP_OK)
        goto finished;
    output = fopen(argv[6], "wb");
    if (output == NULL)
        goto finished;
    if (fwrite(encoded.bytes, 1U, encoded.length, output) != encoded.length) {
        (void)fclose(output);
        goto finished;
    }
    if (fclose(output) != 0)
        goto finished;
    fprintf(stderr,
            "guarantor-integration: candidate chunks and served bytes verified batch=%" PRIu64
            " chunks=%zu bytes=%zu\n",
            batch, bundle.chunk_count, bundle.total_bytes);
    if (argc == 11) {
        status = verify_certificate(&client, &bundle, &body, &arena, argv + 7);
        if (status != LXP_OK) {
            fprintf(stderr, "guarantor-integration: certificate verification refused result=%d\n",
                    (int)status);
            goto finished;
        }
    }
    result = 0;
finished:
    lxp_guarantor_lni_close(&client);
    free(memory);
    return result;
}
