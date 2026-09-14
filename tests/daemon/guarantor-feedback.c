#define _POSIX_C_SOURCE 200809L
#include "../../cmd/layerx-guarantor/lni.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static lxp_byte_span read_file(const char *path, lxp_arena *arena)
{
    FILE *file = fopen(path, "rb");
    long size;
    void *bytes;
    assert(file != NULL && fseek(file, 0L, SEEK_END) == 0);
    size = ftell(file);
    assert(size > 0L && (unsigned long)size <= LXP_GUARANTOR_FRAME_MAX);
    assert(fseek(file, 0L, SEEK_SET) == 0);
    assert(lxp_arena_alloc(arena, (size_t)size, 1U, &bytes) == LXP_OK);
    assert(fread(bytes, 1U, (size_t)size, file) == (size_t)size);
    assert(fclose(file) == 0);
    return (lxp_byte_span){bytes, (size_t)size};
}

int main(int argc, char **argv)
{
    const size_t capacity = 3U * LXP_GUARANTOR_FRAME_MAX;
    uint8_t *memory = malloc(capacity), *original;
    lxp_arena arena;
    lxp_guarantor_lni client = {-1, 0U, 0U};
    lxp_byte_span certificate, proof;
    lxp_result expected, status;
    size_t mark;
    unsigned long timeout;
    unsigned long long batch;
    assert(argc == 7 && memory != NULL);
    batch = strtoull(argv[2], NULL, 10);
    timeout = strtoul(argv[5], NULL, 10);
    assert(batch > 0U && timeout > 0U && timeout <= 60000U);
    assert(lxp_arena_init(&arena, memory, capacity) == LXP_OK);
    certificate = read_file(argv[3], &arena);
    proof = read_file(argv[4], &arena);
    mark = lxp_arena_mark(&arena);
    original = malloc(mark);
    assert(original != NULL);
    memcpy(original, memory, mark);
    if (strcmp(argv[6], "accepted") == 0)
        expected = LXP_OK;
    else if (strcmp(argv[6], "conflict") == 0)
        expected = LXP_ERR_LOG_CORRUPT;
    else {
        assert(strcmp(argv[6], "deadline") == 0);
        expected = LXP_ERR_IO;
    }
    status = lxp_guarantor_lni_feedback_confirmed(&client, argv[1], (uint64_t)batch,
        certificate, proof, &arena, (uint32_t)timeout);
    printf("feedback result=%d expected=%d arena=%zu/%zu\n", (int)status, (int)expected,
        lxp_arena_mark(&arena), mark);
    fflush(stdout);
    assert(status == expected);
    assert(lxp_arena_mark(&arena) == mark && memcmp(original, memory, mark) == 0);
    lxp_guarantor_lni_close(&client);
    free(original);
    free(memory);
    return 0;
}
