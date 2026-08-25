#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/obj_mac.h>
#include <string.h>

enum { LXP_ATTESTATION_MESSAGE_BYTES = 189 };

static void store_u16(uint8_t out[2], uint16_t value)
{
    out[0] = (uint8_t)(value >> 8U);
    out[1] = (uint8_t)value;
}

static void store_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void store_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static void attestation_message(const lxp_guarantor_attestation *attestation,
                                uint8_t message[LXP_ATTESTATION_MESSAGE_BYTES])
{
    store_u16(message, attestation->protocol_version);
    store_u32(message + 2U, attestation->network_id);
    store_u64(message + 6U, attestation->paxeer_chain_id);
    (void)memcpy(message + 14U, attestation->paxeer_settlement_contract, 20U);
    store_u64(message + 34U, attestation->epoch);
    (void)memcpy(message + 42U, attestation->checkpoint_id, 32U);
    (void)memcpy(message + 74U, attestation->checkpoint_hash, 32U);
    (void)memcpy(message + 106U, attestation->guarantor_id, 32U);
    store_u64(message + 138U, attestation->batch_number);
    (void)memcpy(message + 146U, attestation->data_availability_root, 32U);
    message[178] = attestation->replayed ? 1U : 0U;
    message[179] = attestation->da_possessed ? 1U : 0U;
    message[180] = attestation->availability_class_mask;
    store_u64(message + 181U, attestation->attested_at_ms);
}

lxp_result lxp_secp256k1_sign(const uint8_t private_key[32],
                              lxp_domain_tag_id domain,
                              const void *message, size_t message_length,
                              uint8_t signature[64])
{
    EC_KEY *key = NULL;
    BIGNUM *private_value = NULL;
    EC_POINT *public_point = NULL;
    ECDSA_SIG *signed_digest = NULL;
    BIGNUM *order = NULL;
    BIGNUM *low = NULL;
    const EC_GROUP *group;
    const BIGNUM *r;
    const BIGNUM *s;
    uint8_t digest[32];
    lxp_result status = LXP_ERR_BAD_SIGNATURE;
    if (private_key == NULL || message == NULL || signature == NULL ||
        lxp_hash_domain(domain, message, message_length, digest) != LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    key = EC_KEY_new_by_curve_name(NID_secp256k1);
    private_value = BN_bin2bn(private_key, 32, NULL);
    group = key == NULL ? NULL : EC_KEY_get0_group(key);
    public_point = group == NULL ? NULL : EC_POINT_new(group);
    if (key == NULL || private_value == NULL || BN_is_zero(private_value) ||
        public_point == NULL ||
        EC_POINT_mul(group, public_point, private_value, NULL, NULL, NULL) != 1 ||
        EC_KEY_set_private_key(key, private_value) != 1 ||
        EC_KEY_set_public_key(key, public_point) != 1)
        goto cleanup;
    signed_digest = ECDSA_do_sign(digest, 32, key);
    if (signed_digest == NULL) goto cleanup;
    ECDSA_SIG_get0(signed_digest, &r, &s);
    if (BN_bn2binpad(r, signature, 32) != 32 ||
        BN_bn2binpad(s, signature + 32U, 32) != 32)
        goto cleanup;
    if (!lxp_secp256k1_sig_is_low_s(signature)) {
        order = BN_new();
        low = BN_new();
        if (order == NULL || low == NULL ||
            EC_GROUP_get_order(group, order, NULL) != 1 ||
            BN_sub(low, order, s) != 1 ||
            BN_bn2binpad(low, signature + 32U, 32) != 32)
            goto cleanup;
    }
    status = LXP_OK;
cleanup:
    BN_free(low);
    BN_free(order);
    ECDSA_SIG_free(signed_digest);
    EC_POINT_free(public_point);
    BN_free(private_value);
    EC_KEY_free(key);
    lxp_secure_zero(digest, sizeof(digest));
    return status;
}

lxp_result lxp_guarantor_attest(
    const lxp_guarantor_ctx *ctx,
    const lxp_checkpoint_certificate *checkpoint, bool replayed,
    bool da_possessed, uint64_t attested_at_ms, lxp_arena *arena,
    lxp_guarantor_attestation *attestation)
{
    uint8_t message[LXP_ATTESTATION_MESSAGE_BYTES];
    lxp_result status;
    if (ctx == NULL || checkpoint == NULL || arena == NULL ||
        attestation == NULL || !replayed || !da_possessed ||
        !ctx->ready_to_sign || !ctx->possesses_availability ||
        !ctx->bond_view.bonded || ctx->paxeer_chain_id == 0U ||
        lxp_ct_is_zero(ctx->paxeer_settlement_contract, 20U) ||
        ctx->protocol_version != checkpoint->header.protocol_version ||
        ctx->network_id == 0U ||
        ctx->network_id != checkpoint->header.network_id ||
        !lxp_protocol_version_supported(checkpoint->header.protocol_version) ||
        checkpoint->header.network_id == 0U || checkpoint->header.epoch == 0U ||
        checkpoint->header.batch_number == 0U || attested_at_ms == 0U)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    (void)memset(attestation, 0, sizeof(*attestation));
    status = lxp_checkpoint_certificate_hash(checkpoint, arena,
                                              attestation->checkpoint_hash);
    if (status != LXP_OK) return status;
    attestation->protocol_version = checkpoint->header.protocol_version;
    attestation->network_id = checkpoint->header.network_id;
    attestation->paxeer_chain_id = ctx->paxeer_chain_id;
    (void)memcpy(attestation->paxeer_settlement_contract,
                 ctx->paxeer_settlement_contract, 20U);
    attestation->epoch = checkpoint->header.epoch;
    (void)memcpy(attestation->checkpoint_id,
                 attestation->checkpoint_hash, 32U);
    (void)memcpy(attestation->guarantor_id, ctx->guarantor_id, 32U);
    attestation->batch_number = checkpoint->header.batch_number;
    (void)memcpy(attestation->data_availability_root,
                 checkpoint->header.data_availability_root, 32U);
    attestation->replayed = true;
    attestation->da_possessed = true;
    attestation->availability_class_mask = LXP_GUARANTOR_AVAILABILITY_ALL;
    attestation->attested_at_ms = attested_at_ms;
    attestation_message(attestation, message);
    return lxp_secp256k1_sign(ctx->paxeer_private_key,
                              LXP_DOMAIN_GUARANTOR_ATTESTATION,
                              message, sizeof(message),
                              attestation->signature);
}

lxp_result lxp_guarantor_attestation_verify(
    const lxp_guarantor_attestation *attestation,
    const uint8_t public_key[33])
{
    uint8_t message[LXP_ATTESTATION_MESSAGE_BYTES];
    if (attestation == NULL || public_key == NULL ||
        !lxp_protocol_version_supported(attestation->protocol_version) ||
        attestation->network_id == 0U || attestation->paxeer_chain_id == 0U ||
        lxp_ct_is_zero(attestation->paxeer_settlement_contract, 20U) ||
        attestation->epoch == 0U || attestation->batch_number == 0U ||
        memcmp(attestation->checkpoint_id,
               attestation->checkpoint_hash, 32U) != 0 ||
        !attestation->replayed ||
        !attestation->da_possessed ||
        attestation->availability_class_mask !=
            LXP_GUARANTOR_AVAILABILITY_ALL ||
        attestation->attested_at_ms == 0U)
        return LXP_ERR_BAD_SIGNATURE;
    attestation_message(attestation, message);
    return lxp_secp256k1_verify(public_key, 33U, attestation->signature,
                                LXP_DOMAIN_GUARANTOR_ATTESTATION,
                                message, sizeof(message));
}
