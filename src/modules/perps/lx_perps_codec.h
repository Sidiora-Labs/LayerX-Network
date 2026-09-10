#ifndef LAYERX_LX_PERPS_CODEC_H
#define LAYERX_LX_PERPS_CODEC_H

#include "layerx/lxp_result.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

static inline void lx_perps_put_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static inline uint32_t lx_perps_get_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static inline void lx_perps_put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static inline uint64_t lx_perps_get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static inline lxp_result lx_perps_put_i128(uint8_t bytes[17], lxp_i128 value)
{
    bytes[0] = value.negative ? 1U : 0U;
    if (lxp_u128_is_zero(value.magnitude) && value.negative)
        return LXP_ERR_NON_CANONICAL;
    return lxp_u128_to_be(value.magnitude, bytes + 1U);
}

static inline lxp_result lx_perps_get_i128(const uint8_t bytes[17],
                                           lxp_i128 *value)
{
    lxp_result status;
    if (bytes[0] > 1U) return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_from_be(bytes + 1U, &value->magnitude);
    if (status != LXP_OK) return status;
    value->negative = bytes[0] != 0U;
    if (value->negative && lxp_u128_is_zero(value->magnitude))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static inline lxp_result lx_perps_put_side(uint8_t *byte, int side)
{
    if (side != 1 && side != 2) return LXP_ERR_NON_CANONICAL;
    *byte = (uint8_t)side;
    return LXP_OK;
}

static inline bool lx_perps_side_valid(uint8_t byte)
{
    return byte == 1U || byte == 2U;
}

#endif
