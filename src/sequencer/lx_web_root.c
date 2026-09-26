#include "layerx/lx_web.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_merkle.h"

#include <string.h>

static void put_u32(uint8_t bytes[4], uint32_t value)
{
    size_t i;
    for (i = 0U; i < 4U; ++i)
        bytes[i] = (uint8_t)(value >> ((3U - i) * 8U));
}

static void put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

lxp_result lx_web_leaf_encode(const lx_web_committed *committed,
                              uint8_t *bytes, size_t capacity,
                              size_t *length)
{
    const lx_web_observation *observation;
    lxp_result status;
    if (committed == NULL || bytes == NULL || length == NULL ||
        capacity < LX_WEB_LEAF_BYTES || committed->signer_count == 0U ||
        committed->signer_count > LX_WEB_MAX_ATTESTORS ||
        committed->signer_count != committed->observation.signature_count ||
        committed->observation.response_length > LX_WEB_MAX_RESPONSE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    observation = &committed->observation;
    (void)memcpy(bytes, committed->attestation_digest, 32U);
    (void)memcpy(bytes + 32U, observation->program_id, 32U);
    put_u64(bytes + 64U, observation->request_id);
    bytes[72] = observation->kind;
    (void)memcpy(bytes + 73U, observation->payload_hash, 32U);
    (void)memcpy(bytes + 105U, observation->content_digest, 32U);
    status = lxp_keccak256(observation->response,
                           observation->response_length, bytes + 137U);
    if (status != LXP_OK) return status;
    put_u32(bytes + 169U, observation->full_length);
    status = lxp_keccak256(&committed->signers[0][0],
                           committed->signer_count * LX_WEB_SIGNER_BYTES,
                           bytes + 173U);
    if (status != LXP_OK) return status;
    put_u64(bytes + 205U, committed->global_sequence);
    *length = LX_WEB_LEAF_BYTES;
    return LXP_OK;
}

static void leaves_sort(
    uint8_t leaves[LX_WEB_STORE_CAPACITY][LX_WEB_LEAF_BYTES], size_t count)
{
    size_t i;
    for (i = 1U; i < count; ++i) {
        uint8_t current[LX_WEB_LEAF_BYTES];
        size_t position = i;
        (void)memcpy(current, leaves[i], sizeof(current));
        while (position != 0U &&
               memcmp(current, leaves[position - 1U], sizeof(current)) < 0) {
            (void)memcpy(leaves[position], leaves[position - 1U],
                         sizeof(current));
            --position;
        }
        (void)memcpy(leaves[position], current, sizeof(current));
    }
}

lxp_result lx_web_availability_bundle_build(
    const lx_web_store *store, lx_web_availability_bundle *bundle)
{
    size_t i;
    if (store == NULL || bundle == NULL ||
        store->committed_count > LX_WEB_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bundle, 0, sizeof(*bundle));
    for (i = 0U; i < store->committed_count; ++i) {
        size_t length;
        lxp_result status = lx_web_leaf_encode(
            &store->committed[i], bundle->leaves[i], LX_WEB_LEAF_BYTES,
            &length);
        if (status != LXP_OK || length != LX_WEB_LEAF_BYTES)
            return status != LXP_OK ? status : LXP_FATAL_INVARIANT;
    }
    bundle->count = store->committed_count;
    leaves_sort(bundle->leaves, bundle->count);
    return LXP_OK;
}

lxp_result lx_web_root_from_availability(
    const lx_web_availability_bundle *bundle, lxp_arena *arena,
    uint8_t root[32])
{
    uint8_t hashes[LX_WEB_STORE_CAPACITY][32];
    size_t i;
    lxp_result status;
    if (bundle == NULL || arena == NULL || root == NULL ||
        bundle->count > LX_WEB_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < bundle->count; ++i) {
        if (i != 0U && memcmp(bundle->leaves[i - 1U], bundle->leaves[i],
                              LX_WEB_LEAF_BYTES) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
        status = lxp_merkle_leaf_hash(bundle->leaves[i], LX_WEB_LEAF_BYTES,
                                      hashes[i]);
        if (status != LXP_OK) return status;
    }
    return lxp_merkle_build((const uint8_t (*)[32])hashes, bundle->count,
                            arena, root);
}

lxp_result lx_web_root(const lx_web_store *store, lxp_arena *arena,
                       uint8_t root[32])
{
    lx_web_availability_bundle bundle;
    lxp_result status = lx_web_availability_bundle_build(store, &bundle);
    if (status != LXP_OK) return status;
    return lx_web_root_from_availability(&bundle, arena, root);
}
