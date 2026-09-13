#include "layerx/lxp_replica.h"
#include "layerx/lxp_crypto.h"

#include <openssl/evp.h>
#include <string.h>

static void put_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

lxp_result lxp_divergence_detect(lxp_divergence_state *state,
                                 uint64_t batch_number,
                                 uint64_t global_sequence,
                                 lxp_divergence_component component,
                                 lxp_byte_span expected,
                                 lxp_byte_span produced)
{
    int equal;
    if (state == NULL || component < LXP_DIVERGENCE_RECEIPT ||
        component > LXP_DIVERGENCE_STATE_ROOT ||
        (expected.bytes == NULL && expected.length != 0U) ||
        (produced.bytes == NULL && produced.length != 0U))
        return LXP_ERR_NON_CANONICAL;
    if (state->detected) return LXP_FATAL_REPLAY_DIVERGENCE;
    if (expected.length > LXP_MAX_DIVERGENCE_VALUE_BYTES ||
        produced.length > LXP_MAX_DIVERGENCE_VALUE_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    equal = expected.length == produced.length &&
            lxp_ct_memcmp(expected.bytes, produced.bytes,
                          expected.length) == 0;
    if (equal) return LXP_OK;
    if (expected.length > LXP_MAX_DIVERGENCE_VALUE_BYTES ||
        produced.length > LXP_MAX_DIVERGENCE_VALUE_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    state->batch_number = batch_number;
    state->global_sequence = global_sequence;
    state->component = component;
    state->expected_length = expected.length;
    state->produced_length = produced.length;
    if (expected.length != 0U)
        (void)memcpy(state->expected, expected.bytes, expected.length);
    if (produced.length != 0U)
        (void)memcpy(state->produced, produced.bytes, produced.length);
    state->detected = true;
    return LXP_FATAL_REPLAY_DIVERGENCE;
}

static lxp_result report_preimage(const lxp_divergence_report_record *report,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length)
{
    const lxp_divergence_state *state = &report->divergence;
    size_t required;
    size_t offset = 0U;
    if (!state->detected ||
        state->component < LXP_DIVERGENCE_RECEIPT ||
        state->component > LXP_DIVERGENCE_STATE_ROOT ||
        state->expected_length > sizeof(state->expected) ||
        state->produced_length > sizeof(state->produced))
        return LXP_ERR_LENGTH_LIMIT;
    required = 8U + 8U + 1U + 4U + state->expected_length + 4U +
               state->produced_length + 32U;
    if (required > capacity) return LXP_ERR_LENGTH_LIMIT;
    put_u64(bytes + offset, state->batch_number); offset += 8U;
    put_u64(bytes + offset, state->global_sequence); offset += 8U;
    bytes[offset++] = (uint8_t)state->component;
    put_u32(bytes + offset, (uint32_t)state->expected_length); offset += 4U;
    (void)memcpy(bytes + offset, state->expected, state->expected_length);
    offset += state->expected_length;
    put_u32(bytes + offset, (uint32_t)state->produced_length); offset += 4U;
    (void)memcpy(bytes + offset, state->produced, state->produced_length);
    offset += state->produced_length;
    (void)memcpy(bytes + offset, report->replica_id, 32U); offset += 32U;
    *length = offset;
    return LXP_OK;
}

lxp_result lxp_divergence_report(
    const lxp_divergence_state *state, const uint8_t replica_id[32],
    const uint8_t private_key[32], lxp_divergence_report_record *report)
{
    uint8_t preimage[2100];
    uint8_t digest[32];
    size_t preimage_length;
    size_t signature_length = 64U;
    EVP_PKEY *key;
    EVP_MD_CTX *signing;
    lxp_result status;
    int signed_ok;
    if (state == NULL || replica_id == NULL || private_key == NULL ||
        report == NULL || !state->detected)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(report, 0, sizeof(*report));
    report->divergence = *state;
    (void)memcpy(report->replica_id, replica_id, 32U);
    status = report_preimage(report, preimage, sizeof(preimage),
                             &preimage_length);
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, preimage,
                                 preimage_length, digest);
    key = status == LXP_OK ? EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U) : NULL;
    signing = key == NULL ? NULL : EVP_MD_CTX_new();
    signed_ok = signing != NULL &&
        EVP_DigestSignInit(signing, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(signing, report->signature, &signature_length,
                       digest, sizeof(digest)) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(signing);
    EVP_PKEY_free(key);
    lxp_secure_zero(preimage, sizeof(preimage));
    lxp_secure_zero(digest, sizeof(digest));
    return status != LXP_OK ? status : signed_ok ? LXP_OK :
           LXP_ERR_BAD_SIGNATURE;
}

lxp_result lxp_divergence_report_verify(
    const lxp_divergence_report_record *report, const uint8_t public_key[32])
{
    uint8_t preimage[2100];
    size_t preimage_length;
    lxp_result status;
    if (report == NULL || public_key == NULL) return LXP_ERR_NON_CANONICAL;
    status = report_preimage(report, preimage, sizeof(preimage),
                             &preimage_length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify(public_key, report->signature,
                                    LXP_DOMAIN_SIGNATURE_PREIMAGE, preimage,
                                    preimage_length);
    lxp_secure_zero(preimage, sizeof(preimage));
    return status;
}

lxp_result lxp_replica_halt(lxp_replica *replica)
{
    if (replica == NULL) return LXP_ERR_NON_CANONICAL;
    replica->halted = true;
    replica->execution_enabled = false;
    replica->acknowledgements_enabled = false;
    replica->serving_current_state = false;
    replica->serving_finalised_history = true;
    return LXP_OK;
}
