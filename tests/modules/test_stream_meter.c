#include "layerx/lx_stream.h"
#include "layerx/lxp_hash.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define CHECK(x) do { \
    if (!(x)) { (void)fprintf(stderr, "line %d\n", __LINE__); return 1; } \
} while (0)

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

static void metered_record(lx_stream_record *record, lxp_u128 cap)
{
    (void)memset(record, 0, sizeof(*record));
    record->stream_id[0] = 3U;
    record->payer[0] = 1U;
    record->stream_account[0] = 2U;
    record->recipient[0] = 4U;
    record->asset_id[0] = 5U;
    record->mode = LX_STREAM_MODE_METERED;
    record->rate = (lxp_u128){ 0U, 3U };
    record->rate_unit = 2U;
    record->start_timestamp = 100U;
    record->last_accrual_timestamp = 100U;
    record->total_cap = cap;
    record->meter_authorities[0][0] = 7U;
    record->meter_authority_count = 1U;
}

static int attested_meter(void)
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
    CHECK(sign_attestation(&attestation, seed) == 0);
    (void)memcpy(authorized, attestation.authority_key, 32U);
    (void)memcpy(record.meter_authorities[0], authorized, 32U);
    record.meter_authority_count = 1U;
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) == LXP_OK);
    CHECK(accrued.lo == 4U && record.remainder_carry.lo == 1U &&
          record.cumulative_meter == 3U);
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) == LXP_OK);
    CHECK(lxp_u128_is_zero(accrued) && record.accrued_total.lo == 4U);

    attestation.cumulative_reading = 2U;
    CHECK(sign_attestation(&attestation, seed) == 0);
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_METER_REGRESSION);
    CHECK(record.accrued_total.lo == 4U && record.cumulative_meter == 3U);

    attestation.cumulative_reading = 4U;
    CHECK(sign_attestation(&attestation, other_seed) == 0);
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_UNAUTHORIZED_METER);
    CHECK(record.accrued_total.lo == 4U);

    /* An authorized key with a mutilated signature is still refused. */
    CHECK(sign_attestation(&attestation, seed) == 0);
    attestation.signature[0] = (uint8_t)(attestation.signature[0] ^ 1U);
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_UNAUTHORIZED_METER);
    CHECK(record.cumulative_meter == 3U);

    /* A signature bound to one stream cannot be replayed onto another. */
    CHECK(sign_attestation(&attestation, seed) == 0);
    attestation.stream_id[1] = 9U;
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_UNAUTHORIZED_METER);
    (void)memcpy(attestation.stream_id, record.stream_id, 32U);

    (void)memcpy(attestation.authority_key, authorized, 32U);
    record.rate = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    record.rate_unit = 1U;
    attestation.cumulative_reading = UINT64_MAX;
    CHECK(sign_attestation(&attestation, seed) == 0);
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_ACCRUAL_OVERFLOW);
    CHECK(record.cumulative_meter == 3U);
    record.meter_authority_count = LX_STREAM_MAX_METER_AUTHORITIES + 1U;
    CHECK(lx_stream_meter_execute(&record, &attestation, &accrued) ==
          LXP_ERR_UNAUTHORIZED_METER);
    CHECK(lx_stream_meter_authority_check(NULL, &attestation) ==
          LXP_ERR_UNAUTHORIZED_METER);
    CHECK(lx_stream_meter_authority_check(&record, NULL) ==
          LXP_ERR_UNAUTHORIZED_METER);
    return 0;
}

static int metered_boundaries(void)
{
    lx_stream_record record;
    lxp_u128 accrued;

    metered_record(&record, (lxp_u128){ 0U, 5U });
    CHECK(lx_stream_metered_accrue(&record, 4U, &accrued) == LXP_OK);
    CHECK(accrued.lo == 5U && record.accrued_total.lo == 5U &&
          record.cumulative_meter == 4U &&
          lxp_u128_is_zero(record.remainder_carry));
    /* At the cap the reading is not consumed, so no usage is lost. */
    CHECK(lx_stream_metered_accrue(&record, 8U, &accrued) == LXP_OK);
    CHECK(lxp_u128_is_zero(accrued) && record.cumulative_meter == 4U &&
          record.accrued_total.lo == 5U);

    metered_record(&record, (lxp_u128){ 0U, 100U });
    record.paused = true;
    CHECK(lx_stream_metered_accrue(&record, 6U, &accrued) == LXP_OK);
    CHECK(lxp_u128_is_zero(accrued) && record.cumulative_meter == 6U &&
          lxp_u128_is_zero(record.accrued_total));
    record.paused = false;
    record.underfunded = true;
    CHECK(lx_stream_metered_accrue(&record, 10U, &accrued) == LXP_OK);
    CHECK(lxp_u128_is_zero(accrued) && record.cumulative_meter == 10U &&
          lxp_u128_is_zero(record.accrued_total));
    record.underfunded = false;
    CHECK(lx_stream_metered_accrue(&record, 12U, &accrued) == LXP_OK);
    CHECK(accrued.lo == 3U && record.accrued_total.lo == 3U &&
          record.cumulative_meter == 12U);

    metered_record(&record, (lxp_u128){ 0U, 100U });
    record.closed = true;
    CHECK(lx_stream_metered_accrue(&record, 6U, &accrued) == LXP_OK);
    CHECK(lxp_u128_is_zero(accrued) && record.cumulative_meter == 0U);

    metered_record(&record, (lxp_u128){ 0U, 100U });
    record.mode = LX_STREAM_MODE_TIME;
    CHECK(lx_stream_metered_accrue(&record, 6U, &accrued) ==
          LXP_ERR_NON_CANONICAL);
    metered_record(&record, (lxp_u128){ 0U, 100U });
    record.rate_unit = 0U;
    CHECK(lx_stream_metered_accrue(&record, 6U, &accrued) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_metered_accrue(NULL, 6U, &accrued) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_metered_accrue(&record, 6U, NULL) ==
          LXP_ERR_NON_CANONICAL);
    return 0;
}

static int attestation_preimage(void)
{
    static const uint8_t tag[] = "LXP:STREAM:METER:v1";
    lx_stream_meter_attestation attestation;
    uint8_t message[128];
    size_t length = 0U;
    size_t i;

    (void)memset(&attestation, 0, sizeof(attestation));
    attestation.stream_id[0] = 3U;
    attestation.authority_key[0] = 7U;
    attestation.cumulative_reading = 0x0102030405060708ULL;
    CHECK(lx_stream_meter_attestation_bytes(&attestation, message,
                                            sizeof(message), &length) ==
          LXP_OK);
    CHECK(length == sizeof(tag) - 1U + 72U);
    CHECK(memcmp(message, tag, sizeof(tag) - 1U) == 0);
    CHECK(memcmp(message + sizeof(tag) - 1U, attestation.stream_id, 32U) == 0);
    for (i = 0U; i < 8U; ++i)
        CHECK(message[sizeof(tag) - 1U + 32U + i] == (uint8_t)(i + 1U));
    CHECK(memcmp(message + sizeof(tag) - 1U + 40U,
                 attestation.authority_key, 32U) == 0);
    CHECK(lx_stream_meter_attestation_bytes(&attestation, message,
                                            length - 1U, &length) ==
          LXP_ERR_LENGTH_LIMIT);
    return 0;
}

static int meter_payload(void)
{
    lx_stream_meter_attestation attestation;
    lx_stream_meter_attestation decoded;
    uint8_t bytes[LX_STREAM_METER_PAYLOAD_BYTES];
    size_t length = 0U;
    size_t i;

    (void)memset(&attestation, 0, sizeof(attestation));
    attestation.stream_id[0] = 3U;
    attestation.authority_key[0] = 7U;
    attestation.cumulative_reading = 0x0102030405060708ULL;
    for (i = 0U; i < 64U; ++i) attestation.signature[i] = (uint8_t)(i + 1U);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(length == (size_t)LX_STREAM_METER_PAYLOAD_BYTES);
    CHECK(bytes[0] == 0U && bytes[1] == (uint8_t)LX_STREAM_PAYLOAD_VERSION);
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) == LXP_OK);
    CHECK(memcmp(&attestation, &decoded, sizeof(attestation)) == 0);
    CHECK(lx_stream_meter_encode(&attestation, bytes, length - 1U,
                                 &length) == LXP_ERR_LENGTH_LIMIT);

    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(lx_stream_meter_decode(bytes, length - 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_meter_decode(bytes, length + 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    bytes[1] = (uint8_t)(LX_STREAM_PAYLOAD_VERSION + 1U);
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) ==
          LXP_ERR_VERSION_UNSUPPORTED);
    bytes[1] = (uint8_t)LX_STREAM_PAYLOAD_VERSION;
    bytes[0] = 1U;
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) ==
          LXP_ERR_VERSION_UNSUPPORTED);
    bytes[0] = 0U;
    (void)memset(bytes + 2U, 0, 32U);
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    (void)memset(bytes + 42U, 0, 32U);
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    (void)memset(bytes + 74U, 0, 64U);
    CHECK(lx_stream_meter_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_meter_decode(NULL, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes), NULL) ==
          LXP_ERR_NON_CANONICAL);
    return 0;
}

int main(void)
{
    if (attested_meter() != 0) return 1;
    if (metered_boundaries() != 0) return 1;
    if (attestation_preimage() != 0) return 1;
    if (meter_payload() != 0) return 1;
    return 0;
}
