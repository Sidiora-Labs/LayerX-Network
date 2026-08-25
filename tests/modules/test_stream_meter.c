#include "layerx/lx_stream.h"
#include "layerx/lxp_hash.h"

#include <openssl/evp.h>
#include <string.h>

static int sign_attestation(lx_stream_meter_attestation *attestation,
                            const uint8_t seed[32])
{
    uint8_t message[128];
    uint8_t digest[32];
    size_t message_length;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int failed = key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, attestation->authority_key,
                                    &public_length) != 1 ||
        lx_stream_meter_attestation_bytes(attestation, message,
                                          sizeof(message),
                                          &message_length) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message,
                        message_length, digest) != LXP_OK ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, attestation->signature, &signature_length,
                       digest, sizeof(digest)) != 1 || signature_length != 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return failed;
}

int main(void)
{
    static const uint8_t seed[32] = { 1U };
    static const uint8_t other_seed[32] = { 2U };
    lx_stream_record record;
    lx_stream_meter_attestation attestation;
    lxp_u128 accrued;
    uint8_t authorized[32];

    (void)memset(&record, 0, sizeof(record));
    (void)memset(&attestation, 0, sizeof(attestation));
    record.stream_id[0] = 3U;
    record.mode = LX_STREAM_MODE_METERED;
    record.rate = (lxp_u128){ 0U, 3U };
    record.rate_unit = 2U;
    record.total_cap = (lxp_u128){ 0U, 1000U };
    (void)memcpy(attestation.stream_id, record.stream_id, 32U);
    attestation.cumulative_reading = 3U;
    if (sign_attestation(&attestation, seed) != 0) return 1;
    (void)memcpy(authorized, attestation.authority_key, 32U);
    (void)memcpy(record.meter_authorities[0], authorized, 32U);
    record.meter_authority_count = 1U;
    if (lx_stream_meter_execute(&record, &attestation, &accrued) != LXP_OK ||
        accrued.lo != 4U || record.remainder_carry.lo != 1U ||
        record.cumulative_meter != 3U)
        return 1;
    if (lx_stream_meter_execute(&record, &attestation, &accrued) != LXP_OK ||
        !lxp_u128_is_zero(accrued) || record.accrued_total.lo != 4U)
        return 1;
    attestation.cumulative_reading = 2U;
    if (sign_attestation(&attestation, seed) != 0 ||
        lx_stream_meter_execute(&record, &attestation, &accrued) !=
            LXP_ERR_METER_REGRESSION || record.accrued_total.lo != 4U)
        return 1;
    attestation.cumulative_reading = 4U;
    if (sign_attestation(&attestation, other_seed) != 0 ||
        lx_stream_meter_execute(&record, &attestation, &accrued) !=
            LXP_ERR_UNAUTHORIZED_METER || record.accrued_total.lo != 4U)
        return 1;
    (void)memcpy(attestation.authority_key, authorized, 32U);
    record.rate = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    record.rate_unit = 1U;
    attestation.cumulative_reading = UINT64_MAX;
    if (sign_attestation(&attestation, seed) != 0 ||
        lx_stream_meter_execute(&record, &attestation, &accrued) !=
            LXP_ERR_ACCRUAL_OVERFLOW || record.cumulative_meter != 3U)
        return 1;
    record.meter_authority_count = LX_STREAM_MAX_METER_AUTHORITIES + 1U;
    if (lx_stream_meter_execute(&record, &attestation, &accrued) !=
        LXP_ERR_UNAUTHORIZED_METER)
        return 1;
    return 0;
}
