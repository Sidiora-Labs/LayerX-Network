#include "layerx/lxp_arena.h"

#include <stdint.h>
#include <string.h>

static int exercise(lxp_arena *arena, size_t offsets[3])
{
    void *first = NULL;
    void *second = NULL;
    void *third = NULL;
    size_t mark;

    if (lxp_arena_alloc(arena, 7U, 1U, &first) != LXP_OK) return 1;
    if (lxp_arena_alloc(arena, 16U, 8U, &second) != LXP_OK) return 1;
    mark = lxp_arena_mark(arena);
    if (lxp_arena_alloc(arena, 9U, 16U, &third) != LXP_OK) return 1;
    offsets[0] = (size_t)((uint8_t *)first - arena->buffer);
    offsets[1] = (size_t)((uint8_t *)second - arena->buffer);
    offsets[2] = (size_t)((uint8_t *)third - arena->buffer);
    (void)memset(first, 0x11, 7U);
    (void)memset(second, 0x22, 16U);
    (void)memset(third, 0x33, 9U);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return 1;
    if (lxp_arena_alloc(arena, 9U, 16U, &third) != LXP_OK) return 1;
    {
        size_t i;
        const uint8_t *bytes = (const uint8_t *)third;
        for (i = 0U; i < 9U; ++i) if (bytes[i] != 0U) return 1;
    }
    (void)memset(third, 0x33, 9U);
    return 0;
}

int main(void)
{
    uint8_t storage_a[127];
    struct {
        uint8_t pad[13];
        uint8_t storage[127];
    } shifted;
    lxp_arena a;
    lxp_arena b;
    lxp_arena empty;
    size_t offsets_a[3];
    size_t offsets_b[3];
    void *allocation = NULL;

    if (lxp_arena_init(&a, storage_a, sizeof(storage_a)) != LXP_OK ||
        lxp_arena_init(&b, shifted.storage, sizeof(shifted.storage)) != LXP_OK ||
        lxp_arena_init(&empty, NULL, 0U) != LXP_OK) {
        return 1;
    }
    if (lxp_arena_alloc(&empty, 0U, 1U, &allocation) != LXP_OK ||
        allocation != NULL ||
        lxp_arena_alloc(&empty, 1U, 1U, &allocation) !=
            LXP_ERR_ARENA_EXHAUSTED || allocation != NULL)
        return 1;
    if (exercise(&a, offsets_a) != 0 || exercise(&b, offsets_b) != 0) return 1;
    if (memcmp(offsets_a, offsets_b, sizeof(offsets_a)) != 0 ||
        memcmp(storage_a, shifted.storage, sizeof(storage_a)) != 0) return 1;
    if (lxp_arena_alloc(&a, sizeof(storage_a), 1U, &allocation) !=
        LXP_ERR_ARENA_EXHAUSTED || allocation != NULL) return 1;
    if (lxp_arena_alloc(&a, 1U, 3U, &allocation) != LXP_ERR_NON_CANONICAL)
        return 1;
    if (lxp_arena_reset(&a, sizeof(storage_a) + 1U) != LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}
