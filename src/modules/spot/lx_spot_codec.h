#ifndef LAYERX_LX_SPOT_CODEC_H
#define LAYERX_LX_SPOT_CODEC_H

#include "layerx/lx_spot.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

static inline void lx_spot_put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static inline uint64_t lx_spot_get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static inline bool lx_spot_side_valid(uint8_t byte)
{
    return byte == (uint8_t)LX_SPOT_SIDE_BID ||
           byte == (uint8_t)LX_SPOT_SIDE_ASK;
}

static inline bool lx_spot_zero_id(const uint8_t id[32])
{
    uint8_t accumulator = 0U;
    size_t i;
    for (i = 0U; i < 32U; ++i) accumulator |= id[i];
    return accumulator == 0U;
}

#endif
