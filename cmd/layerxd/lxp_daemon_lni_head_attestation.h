#ifndef LXP_DAEMON_LNI_HEAD_ATTESTATION_H
#define LXP_DAEMON_LNI_HEAD_ATTESTATION_H

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_result.h"

#include <openssl/evp.h>

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

enum {
    LXP_DAEMON_HEAD_ATTESTATION_VERSION = 1,
    LXP_DAEMON_HEAD_ATTESTATION_DOMAIN_BYTES = 34,
    LXP_DAEMON_HEAD_ATTESTATION_PREIMAGE_BYTES =
        LXP_DAEMON_HEAD_ATTESTATION_DOMAIN_BYTES + 32 + 1 + 4 + 32 + 2 + 8 + 8 + 8 + 32,
    LXP_DAEMON_HEAD_ATTESTATION_PAYLOAD_BYTES =
        2 + 32 + 4 + 32 + 2 + 8 + 8 + 8 + 32 + 32,
    LXP_DAEMON_HEAD_ATTESTATION_PROOF_BYTES = 32 + 64
};

static const uint8_t LXP_DAEMON_HEAD_ATTESTATION_DOMAIN[] =
    "LayerX/program-discovery-proof/v1";

typedef struct lxp_daemon_head_attestation {
    uint8_t program_id[32];
    uint32_t version;
    uint8_t code_hash[32];
    uint16_t abi_version;
    uint64_t observed_sequence;
    uint64_t observed_at;
    uint64_t valid_through;
    uint8_t state_root[32];
    uint8_t head_receipt_digest[32];
} lxp_daemon_head_attestation;

static void head_attestation_store_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void head_attestation_store_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void head_attestation_store_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static lxp_result lxp_daemon_head_attestation_digest(
    const lxp_daemon_head_attestation *attestation, uint8_t digest[32])
{
    uint8_t preimage[LXP_DAEMON_HEAD_ATTESTATION_PREIMAGE_BYTES];
    size_t cursor = 0U;
    if (attestation == NULL || digest == NULL) return LXP_ERR_NON_CANONICAL;
    if (sizeof(LXP_DAEMON_HEAD_ATTESTATION_DOMAIN) !=
        LXP_DAEMON_HEAD_ATTESTATION_DOMAIN_BYTES)
        return LXP_FATAL_INVARIANT;
    (void)memcpy(preimage + cursor, LXP_DAEMON_HEAD_ATTESTATION_DOMAIN,
                 sizeof(LXP_DAEMON_HEAD_ATTESTATION_DOMAIN));
    cursor += sizeof(LXP_DAEMON_HEAD_ATTESTATION_DOMAIN);
    (void)memcpy(preimage + cursor, attestation->program_id, 32U); cursor += 32U;
    preimage[cursor++] = 1U;
    head_attestation_store_u32(preimage + cursor, attestation->version); cursor += 4U;
    (void)memcpy(preimage + cursor, attestation->code_hash, 32U); cursor += 32U;
    head_attestation_store_u16(preimage + cursor, attestation->abi_version); cursor += 2U;
    head_attestation_store_u64(preimage + cursor, attestation->observed_sequence);
    cursor += 8U;
    head_attestation_store_u64(preimage + cursor, attestation->observed_at); cursor += 8U;
    head_attestation_store_u64(preimage + cursor, attestation->valid_through);
    cursor += 8U;
    (void)memcpy(preimage + cursor, attestation->state_root, 32U); cursor += 32U;
    if (cursor != LXP_DAEMON_HEAD_ATTESTATION_PREIMAGE_BYTES) return LXP_FATAL_INVARIANT;
    return lxp_hash_sha256(preimage, cursor, digest);
}

static lxp_result lxp_daemon_head_attestation_sign(
    const uint8_t private_key[32], const uint8_t public_key[32],
    const lxp_daemon_head_attestation *attestation, uint8_t signature[64])
{
    uint8_t digest[32];
    size_t signature_length = 64U;
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    bool signed_ok;
    lxp_result status;
    if (private_key == NULL || public_key == NULL || signature == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_head_attestation_digest(attestation, digest);
    if (status != LXP_OK) return status;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key, 32U);
    context = key == NULL ? NULL : EVP_MD_CTX_new();
    signed_ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length, digest,
                       sizeof(digest)) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    status = signed_ok ? lxp_ed25519_verify_raw(public_key, signature, digest,
                                                sizeof(digest)) :
        LXP_ERR_BAD_SIGNATURE;
    lxp_secure_zero(digest, sizeof(digest));
    return status;
}

static lxp_result lxp_daemon_head_attestation_encode(
    const lxp_daemon_head_attestation *attestation,
    uint8_t payload[LXP_DAEMON_HEAD_ATTESTATION_PAYLOAD_BYTES])
{
    size_t cursor = 0U;
    if (attestation == NULL || payload == NULL) return LXP_ERR_NON_CANONICAL;
    head_attestation_store_u16(payload + cursor, LXP_DAEMON_HEAD_ATTESTATION_VERSION);
    cursor += 2U;
    (void)memcpy(payload + cursor, attestation->program_id, 32U); cursor += 32U;
    head_attestation_store_u32(payload + cursor, attestation->version); cursor += 4U;
    (void)memcpy(payload + cursor, attestation->code_hash, 32U); cursor += 32U;
    head_attestation_store_u16(payload + cursor, attestation->abi_version); cursor += 2U;
    head_attestation_store_u64(payload + cursor, attestation->observed_sequence);
    cursor += 8U;
    head_attestation_store_u64(payload + cursor, attestation->observed_at); cursor += 8U;
    head_attestation_store_u64(payload + cursor, attestation->valid_through);
    cursor += 8U;
    (void)memcpy(payload + cursor, attestation->state_root, 32U); cursor += 32U;
    (void)memcpy(payload + cursor, attestation->head_receipt_digest, 32U);
    cursor += 32U;
    return cursor == LXP_DAEMON_HEAD_ATTESTATION_PAYLOAD_BYTES ? LXP_OK :
        LXP_FATAL_INVARIANT;
}

#endif
