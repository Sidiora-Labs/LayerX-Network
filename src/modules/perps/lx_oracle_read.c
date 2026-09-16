#include "layerx/lx_oracle.h"

#include "layerx/lx_perps.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t committed_market_prefix[] = "market:";
static const uint8_t committed_oracle_prefix[] = "oracle:";
static const uint8_t source_set_domain[] = "LXP:ORACLE:SOURCE-SET:v1";

static uint64_t committed_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void committed_u64_le(uint8_t *out, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[i] = (uint8_t)(value >> (8U * i));
}

static void committed_key(uint8_t key[LX_PERPS_MARKET_KEY_BYTES],
                          const uint8_t *prefix, size_t prefix_length,
                          const uint8_t market_id[32])
{
    (void)memcpy(key, prefix, prefix_length);
    (void)memcpy(key + prefix_length, market_id, 32U);
}

static lxp_result committed_entry(const lxp_kernel *kernel,
                                  const uint8_t *key, size_t key_length,
                                  const uint8_t **bytes, size_t *length)
{
    size_t i;
    if (kernel == NULL || kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_PERPS) continue;
        if ((size_t)entry->key_length != key_length) continue;
        if (memcmp(entry->key, key, key_length) != 0) continue;
        *bytes = entry->value;
        *length = entry->value_length;
        return LXP_OK;
    }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_oracle_source_set_digest(const uint8_t *permitted_keys,
                                       size_t permitted_key_count,
                                       uint8_t digest[32])
{
    lxp_hash_context context;
    uint8_t count;
    lxp_result status;
    if (permitted_keys == NULL || digest == NULL ||
        permitted_key_count == 0U || permitted_key_count > LX_ORACLE_MAX_KEYS)
        return LXP_ERR_NON_CANONICAL;
    count = (uint8_t)permitted_key_count;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, source_set_domain,
                             sizeof(source_set_domain) - 1U);
    if (status == LXP_OK) status = lxp_hash_update(&context, &count, 1U);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, permitted_keys,
                                 permitted_key_count * 32U);
    if (status != LXP_OK) return status;
    return lxp_hash_final(&context, digest);
}

lxp_result lx_oracle_committed_encode(
    const lx_oracle_committed *committed,
    uint8_t bytes[LX_ORACLE_COMMITTED_BYTES])
{
    uint8_t price[16];
    lxp_result status;
    size_t i;
    if (committed == NULL || bytes == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_to_be(committed->price, price);
    if (status != LXP_OK) return status;
    for (i = 0U; i < 16U; ++i) bytes[i] = price[15U - i];
    committed_u64_le(bytes + 16U, committed->observed_at);
    committed_u64_le(bytes + 24U, committed->observation_sequence);
    (void)memcpy(bytes + 32U, committed->source_set_digest, 32U);
    return LXP_OK;
}

lxp_result lx_oracle_committed_read(lxp_module_ctx *ctx,
                                    const uint8_t market_id[32],
                                    lx_oracle_committed *committed)
{
    uint8_t key[LX_PERPS_MARKET_KEY_BYTES];
    lx_perps_market market;
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || market_id == NULL || committed == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(committed, 0, sizeof(*committed));
    committed_key(key, committed_market_prefix,
                  sizeof(committed_market_prefix) - 1U, market_id);
    status = committed_entry(ctx->kernel, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    status = lx_perps_market_decode(bytes, length, &market);
    if (status != LXP_OK) return status;
    if (market.halted) return LXP_ERR_MARKET_HALTED;
    committed_key(key, committed_oracle_prefix,
                  sizeof(committed_oracle_prefix) - 1U, market_id);
    status = committed_entry(ctx->kernel, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    if (length != LX_PERPS_ORACLE_BYTES) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(committed->market_id, market_id, 32U);
    committed->observation_sequence = committed_u64(bytes);
    status = lxp_u128_from_be(bytes + 8U, &committed->price);
    if (status != LXP_OK) return status;
    committed->observed_at = committed_u64(bytes + 24U);
    return lx_oracle_source_set_digest(&market.permitted_oracle_keys[0][0],
                                       market.permitted_oracle_key_count,
                                       committed->source_set_digest);
}
