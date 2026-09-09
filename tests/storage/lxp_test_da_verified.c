#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_da.h"
#include "layerx/lxp_merkle.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define REQUIRE(expression) do { \
    if (!(expression)) { \
        (void)fprintf(stderr, "line %d: %s\n", __LINE__, #expression); \
        return 1; \
    } \
} while (0)

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

int main(void)
{
    uint8_t build_memory[65536];
    uint8_t read_memory[65536];
    uint8_t sections[5][7] = {
        {1U, 2U, 3U, 4U, 5U, 6U, 7U},
        {8U, 9U, 10U, 11U, 12U, 13U, 14U},
        {15U, 16U, 17U, 18U, 19U, 20U, 21U},
        {22U, 23U, 24U, 25U, 26U, 27U, 28U},
        {29U, 30U, 31U, 32U, 33U, 34U, 35U}
    };
    lxp_batch_body body = {0};
    lxp_da_bundle bundle;
    lxp_da_bundle read_bundle;
    lxp_da_store store;
    lxp_da_store reopened;
    lxp_arena build_arena;
    lxp_arena read_arena;
    uint8_t root[32];
    uint8_t wrong_root[32];
    uint8_t byte;
    char directory[] = "qual-logs/dan1/da-store-XXXXXX";
    char path[LXP_DA_STORE_PATH_BYTES];
    lxp_byte_span bytes;
    lxp_byte_span metadata;
    int descriptor;
    size_t i;
    REQUIRE(mkdtemp(directory) != NULL);
    REQUIRE(lxp_arena_init(&build_arena, build_memory, sizeof(build_memory)) == LXP_OK);
    REQUIRE(lxp_arena_init(&read_arena, read_memory, sizeof(read_memory)) == LXP_OK);
    REQUIRE(lxp_da_store_init(&store, directory) == LXP_OK);
    body.header.batch_number = 23U;
    body.activities = (lxp_byte_span){sections[0], 7U};
    body.receipts = (lxp_byte_span){sections[1], 7U};
    body.oracle_inputs = (lxp_byte_span){sections[2], 7U};
    body.state_diff = (lxp_byte_span){sections[3], 7U};
    body.recovery_metadata = (lxp_byte_span){sections[4], 7U};
    REQUIRE(lxp_da_bundle_build(&body, 3U, &build_arena, &bundle) == LXP_OK);
    REQUIRE(lxp_da_bundle_root(&bundle, &build_arena, root) == LXP_OK);
    REQUIRE(lxp_da_store_bundle(&store, &bundle, &build_arena) == LXP_OK);
    (void)memset(&store, 0, sizeof(store));
    REQUIRE(lxp_da_store_init(&reopened, directory) == LXP_OK);
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, root, &read_arena, &read_bundle) == LXP_OK);
    REQUIRE(read_bundle.chunk_count == bundle.chunk_count);
    REQUIRE(read_bundle.total_bytes == bundle.total_bytes);
    for (i = 0U; i < bundle.chunk_count; ++i) {
        lxp_merkle_proof proof = {0};
        size_t sibling;
        REQUIRE(read_bundle.chunks[i].length == bundle.chunks[i].length);
        REQUIRE(memcmp(read_bundle.chunks[i].bytes.bytes, bundle.chunks[i].bytes.bytes,
                       bundle.chunks[i].length) == 0);
        REQUIRE(lxp_da_serve_chunk_proof(&reopened, 23U, (uint32_t)i, root,
                    &read_arena, &bytes, &metadata) == LXP_OK);
        REQUIRE(bytes.length == bundle.chunks[i].length);
        REQUIRE(memcmp(bytes.bytes, bundle.chunks[i].bytes.bytes, bytes.length) == 0);
        REQUIRE(metadata.length >= 62U);
        REQUIRE(read_u32(metadata.bytes + 8U) == i);
        REQUIRE(metadata.bytes[12] == (uint8_t)bundle.chunks[i].availability_class);
        REQUIRE(memcmp(metadata.bytes + 21U, bundle.chunks[i].chunk_hash, 32U) == 0);
        proof.leaf_index = read_u32(metadata.bytes + 53U);
        proof.leaf_count = read_u32(metadata.bytes + 57U);
        proof.depth = metadata.bytes[61];
        REQUIRE(proof.depth <= LXP_MERKLE_MAX_DEPTH);
        REQUIRE(metadata.length == 62U + (size_t)proof.depth * 32U);
        for (sibling = 0U; sibling < proof.depth; ++sibling)
            (void)memcpy(proof.siblings[sibling], metadata.bytes + 62U + sibling * 32U, 32U);
        REQUIRE(lxp_merkle_proof_verify(bundle.chunks[i].chunk_hash, &proof, root) == LXP_OK);
    }
    REQUIRE(lxp_arena_reset(&read_arena, 0U) == LXP_OK);
    (void)memcpy(wrong_root, root, 32U);
    wrong_root[0] ^= 1U;
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, wrong_root, &read_arena,
                                      &read_bundle) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(read_arena.offset == 0U && read_bundle.chunks == NULL);
    REQUIRE(lxp_da_serve_chunk_proof(&reopened, 23U, (uint32_t)bundle.chunk_count,
                                    root, &read_arena, &bytes, &metadata) == LXP_ERR_DA_MISSING);
    REQUIRE(read_arena.offset == 0U && bytes.bytes == NULL && metadata.bytes == NULL);
    REQUIRE(lxp_da_store_read_verified(&reopened, 24U, root, &read_arena,
                                      &read_bundle) == LXP_ERR_DA_MISSING);
    REQUIRE(read_arena.offset == 0U);
    REQUIRE(snprintf(path, sizeof(path), "%s/%020llu.lxda", directory, 23ULL) > 0);
    descriptor = open(path, O_RDWR);
    REQUIRE(descriptor >= 0);
    REQUIRE(pread(descriptor, &byte, 1U, 105) == 1);
    byte ^= 1U;
    REQUIRE(pwrite(descriptor, &byte, 1U, 105) == 1);
    REQUIRE(close(descriptor) == 0);
    REQUIRE(lxp_da_store_init(&reopened, directory) == LXP_OK);
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, root, &read_arena,
                                      &read_bundle) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(lxp_da_serve_chunk(&reopened, 23U, 0U, &read_arena, &bytes) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(lxp_da_serve_chunk_proof(&reopened, 23U, 0U, root, &read_arena,
                                    &bytes, &metadata) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(read_arena.offset == 0U);
    REQUIRE(lxp_da_store_bundle(&reopened, &bundle, &build_arena) == LXP_OK);
    descriptor = open(path, O_RDWR);
    REQUIRE(descriptor >= 0);
    REQUIRE(pwrite(descriptor, wrong_root, 32U, 12) == 32);
    REQUIRE(close(descriptor) == 0);
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, root, &read_arena,
                                      &read_bundle) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(lxp_da_serve_chunk(&reopened, 23U, 0U, &read_arena, &bytes) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(truncate(path, 57) == 0);
    REQUIRE(lxp_da_store_init(&reopened, directory) == LXP_OK);
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, root, &read_arena,
                                      &read_bundle) == LXP_ERR_DA_MISSING);
    REQUIRE(read_arena.offset == 0U);
    REQUIRE(unlink(path) == 0);
    REQUIRE(lxp_da_store_init(&reopened, directory) == LXP_OK);
    REQUIRE(lxp_da_store_read_verified(&reopened, 23U, root, &read_arena,
                                      &read_bundle) == LXP_ERR_DA_MISSING);
    REQUIRE(rmdir(directory) == 0);
    return 0;
}
