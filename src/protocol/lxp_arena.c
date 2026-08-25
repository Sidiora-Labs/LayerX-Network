#include "layerx/lxp_arena.h"

#include <stdint.h>
#include <string.h>

static int alignment_is_valid(size_t alignment)
{
    return alignment != 0U && (alignment & (alignment - 1U)) == 0U;
}

lxp_result lxp_arena_init(lxp_arena *arena, void *buffer, size_t capacity)
{
    if (arena == NULL || (buffer == NULL && capacity != 0U)) {
        return LXP_ERR_NON_CANONICAL;
    }
    arena->buffer = (uint8_t *)buffer;
    arena->capacity = capacity;
    arena->offset = 0U;
    if (capacity != 0U) {
        (void)memset(buffer, 0, capacity);
    }
    return LXP_OK;
}

lxp_result lxp_arena_alloc(lxp_arena *arena, size_t size, size_t alignment,
                           void **allocation)
{
    size_t mask;
    size_t aligned;

    if (arena == NULL || allocation == NULL || !alignment_is_valid(alignment)) {
        return LXP_ERR_NON_CANONICAL;
    }
    *allocation = NULL;
    if (arena->buffer == NULL) {
        return size == 0U && arena->capacity == 0U ? LXP_OK :
               LXP_ERR_ARENA_EXHAUSTED;
    }
    mask = alignment - 1U;
    if (arena->offset > SIZE_MAX - mask) {
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    aligned = (arena->offset + mask) & ~mask;
    if (aligned > arena->capacity || size > arena->capacity - aligned) {
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    *allocation = arena->buffer + aligned;
    arena->offset = aligned + size;
    return LXP_OK;
}

size_t lxp_arena_mark(const lxp_arena *arena)
{
    return arena == NULL ? 0U : arena->offset;
}

lxp_result lxp_arena_reset(lxp_arena *arena, size_t mark)
{
    if (arena == NULL || mark > arena->offset) {
        return LXP_ERR_NON_CANONICAL;
    }
    if (arena->offset > mark) {
        (void)memset(arena->buffer + mark, 0, arena->offset - mark);
    }
    arena->offset = mark;
    return LXP_OK;
}
