#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define REQUIRE(x) do { if (!(x)) { fprintf(stderr, "maintenance protocol line %d\n", __LINE__); return 1; } } while (0)
static uint8_t memory[16U * LXP_MAX_ACTIVITY_BYTES];
static char text[2U * LXP_MAX_ACTIVITY_BYTES + 2U];
static lxp_daemon_protocol_owner owner;
static lxp_daemon_receipt_authority_store store;
static lxp_kernel kernel;

static int read_span(lxp_arena *arena, lxp_byte_span *span)
{
    size_t length;
    void *bytes;
    REQUIRE(fgets(text, sizeof(text), stdin) != NULL);
    length = strlen(text);
    REQUIRE(length > 0U && text[length - 1U] == '\n');
    --length;
    REQUIRE(length % 2U == 0U);
    REQUIRE(lxp_arena_alloc(arena, length / 2U + 1U, 1U, &bytes) == LXP_OK);
    for (size_t i = 0U; i < length / 2U; ++i) {
        unsigned value;
        REQUIRE(sscanf(text + i * 2U, "%2x", &value) == 1);
        ((uint8_t *)bytes)[i] = (uint8_t)value;
    }
    *span = (lxp_byte_span){bytes, length / 2U};
    return 0;
}

static void hex32(const uint8_t *bytes, char *out)
{
    for (size_t i = 0U; i < 32U; ++i) (void)sprintf(out + i * 2U, "%02x", bytes[i]);
}

int main(void)
{
    const uint8_t bearer[] = "maintenance-protocol-fixture-bearer";
    lxp_arena arena;
    lxp_byte_span receipt, maintenance, header, signature, key;
    lxp_batch_header batch;
    lxp_sequencer_authorization authorization = {0};
    lxp_merkle_proof proofs[2];
    uint8_t leaves[2][32], root[32];
    lxp_log log = {.descriptor = -1};
    lxp_daemon_receipt_evidence evidence;
    lxp_daemon_protocol_response response;
    uint64_t offset = 0U;
    bool present;
    char path[] = "/tmp/lxp-maintenance-protocol-XXXXXX";
    char batch_hex[65], digest_hex[65], route[256];
    int fd;
    REQUIRE(lxp_arena_init(&arena, memory, sizeof(memory)) == LXP_OK);
    REQUIRE(read_span(&arena, &receipt) == 0 && read_span(&arena, &maintenance) == 0 &&
        read_span(&arena, &header) == 0 && read_span(&arena, &signature) == 0 && read_span(&arena, &key) == 0);
    REQUIRE(signature.length == 64U && key.length == 32U);
    REQUIRE(lxp_batch_header_decode(header.bytes, header.length, &batch) == LXP_OK);
    memcpy(authorization.public_key, key.bytes, 32U);
    memcpy(authorization.sequencer_id, batch.sequencer_id, 32U);
    authorization.authorized = 1U;
    authorization.first_batch_number = batch.batch_number;
    authorization.last_batch_number = batch.batch_number;
    REQUIRE(lxp_merkle_leaf_hash(receipt.bytes, receipt.length, leaves[0]) == LXP_OK);
    if (maintenance.length != 0U)
        REQUIRE(lxp_merkle_leaf_hash(maintenance.bytes, maintenance.length, leaves[1]) == LXP_OK);
    for (size_t i = 0U; i < (maintenance.length != 0U ? 2U : 1U); ++i) {
        REQUIRE(lxp_merkle_proof_generate((const uint8_t (*)[32])leaves,
            maintenance.length != 0U ? 2U : 1U, i, &arena, &proofs[i], root) == LXP_OK);
        REQUIRE(memcmp(root, batch.receipt_merkle_root, 32U) == 0);
    }
    fd = mkstemp(path);
    REQUIRE(fd >= 0 && close(fd) == 0);
    REQUIRE(lxp_log_open_or_create(&log, path, sizeof(memory)) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&store, &log, &authorization) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_append(&store, receipt.bytes, receipt.length,
        header.bytes, header.length, signature.bytes, &proofs[0], &arena) == LXP_OK);
    if (maintenance.length != 0U) {
        ((uint8_t *)maintenance.bytes)[maintenance.length - 1U] ^= 1U;
        REQUIRE(lxp_daemon_receipt_authority_append_maintenance(&store, maintenance.bytes,
            maintenance.length, header.bytes, header.length, signature.bytes, &proofs[1], &arena) != LXP_OK);
        ((uint8_t *)maintenance.bytes)[maintenance.length - 1U] ^= 1U;
        proofs[1].siblings[0][0] ^= 1U;
        REQUIRE(lxp_daemon_receipt_authority_append_maintenance(&store, maintenance.bytes,
            maintenance.length, header.bytes, header.length, signature.bytes, &proofs[1], &arena) != LXP_OK);
        proofs[1].siblings[0][0] ^= 1U;
        REQUIRE(lxp_daemon_receipt_authority_append_maintenance(&store, maintenance.bytes,
            maintenance.length, header.bytes, header.length, signature.bytes, &proofs[1], &arena) == LXP_OK);
    }
    REQUIRE(pthread_mutex_init(&owner.mutex, NULL) == 0);
    owner.attached = true;
    owner.receipt_authority = &store;
    owner.kernel = &kernel;
    memcpy(owner.bearer_token, bearer, sizeof(bearer) - 1U);
    owner.bearer_token_length = sizeof(bearer) - 1U;
    REQUIRE(lxp_daemon_receipt_authority_scan(&store, &offset, &arena, &evidence, &present) == LXP_OK && present);
    hex32(evidence.batch_id, batch_hex);
    hex32(evidence.receipt_digest, digest_hex);
    REQUIRE(snprintf(route, sizeof(route), "/v1/batches/%s/receipt-authority?receipt_digest=%s", batch_hex, digest_hex) > 0);
    REQUIRE(lxp_daemon_protocol_route(&owner, bearer, sizeof(bearer) - 1U, "GET", route, &arena, &response) == LXP_OK && response.status == 200U);
    REQUIRE(fwrite(response.body.bytes, 1U, response.body.length, stdout) == response.body.length && putchar('\n') != EOF);
    if (maintenance.length != 0U)
        REQUIRE(lxp_daemon_receipt_authority_scan(&store, &offset, &arena, &evidence, &present) == LXP_OK && present && evidence.format_version == 3U);
    memcpy(owner.feed_store.head_receipt_digest, evidence.receipt_digest, 32U);
    memcpy(owner.feed_store.head_state_root, batch.resulting_state_root, 32U);
    memcpy(kernel.current_state_root, batch.resulting_state_root, 32U);
    owner.feed_store.scanned_through_sequence = batch.last_sequence;
    REQUIRE(lxp_daemon_protocol_route(&owner, bearer, sizeof(bearer) - 1U, "GET",
        "/v1/protocol/account-state/head", &arena, &response) == LXP_OK && response.status == 200U);
    REQUIRE(fwrite(response.body.bytes, 1U, response.body.length, stdout) == response.body.length && putchar('\n') != EOF);
    owner.feed_store.head_state_root[0] ^= 1U;
    kernel.current_state_root[0] ^= 1U;
    REQUIRE(lxp_daemon_protocol_route(&owner, bearer, sizeof(bearer) - 1U, "GET",
        "/v1/protocol/account-state/head", &arena, &response) == LXP_OK && response.status == 503U);
    owner.feed_store.head_state_root[0] ^= 1U;
    kernel.current_state_root[0] ^= 1U;
    --owner.feed_store.scanned_through_sequence;
    REQUIRE(lxp_daemon_protocol_route(&owner, bearer, sizeof(bearer) - 1U, "GET",
        "/v1/protocol/account-state/head", &arena, &response) == LXP_OK && response.status == 503U);
    REQUIRE(pthread_mutex_destroy(&owner.mutex) == 0);
    REQUIRE(lxp_log_close(&log) == LXP_OK && unlink(path) == 0);
    return 0;
}
