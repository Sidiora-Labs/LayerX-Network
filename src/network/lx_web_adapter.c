#include "layerx/lx_web.h"

#include "layerx/lxp_crypto.h"

#include <openssl/evp.h>
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

lxp_result lx_web_observation_encode(const lx_web_observation *observation,
                                     uint8_t *bytes, size_t capacity,
                                     size_t *length)
{
    size_t required;
    size_t offset;
    size_t i;
    if (observation == NULL || bytes == NULL || length == NULL ||
        observation->origin != LX_WEB_ORIGIN_PROGRAM ||
        observation->network_id == 0U ||
        lxp_ct_is_zero(observation->program_id, 32U) ||
        (observation->kind != LX_WEB_KIND_FETCH &&
         observation->kind != LX_WEB_KIND_SEARCH) ||
        observation->response_length > LX_WEB_MAX_RESPONSE_BYTES ||
        observation->full_length < observation->response_length ||
        observation->signature_count == 0U ||
        observation->signature_count > LX_WEB_MAX_ATTESTORS)
        return LXP_ERR_NON_CANONICAL;
    required = LX_WEB_OBSERVATION_HEADER_BYTES +
               (size_t)observation->response_length + 1U +
               observation->signature_count * LX_WEB_SIGNATURE_BYTES;
    if (capacity < required) return LXP_ERR_LENGTH_LIMIT;
    bytes[0] = observation->origin;
    (void)memset(bytes + 1U, 0, 28U);
    put_u32(bytes + 29U, observation->network_id);
    (void)memcpy(bytes + 33U, observation->program_id, 32U);
    put_u64(bytes + 65U, observation->request_id);
    bytes[73] = observation->kind;
    (void)memcpy(bytes + 74U, observation->payload_hash, 32U);
    (void)memcpy(bytes + 106U, observation->content_digest, 32U);
    put_u32(bytes + 138U, observation->full_length);
    put_u32(bytes + 142U, observation->response_length);
    offset = LX_WEB_OBSERVATION_HEADER_BYTES;
    if (observation->response_length != 0U)
        (void)memcpy(bytes + offset, observation->response,
                     observation->response_length);
    offset += observation->response_length;
    bytes[offset++] = (uint8_t)observation->signature_count;
    for (i = 0U; i < observation->signature_count; ++i) {
        (void)memcpy(bytes + offset, observation->signatures[i],
                     LX_WEB_SIGNATURE_BYTES);
        offset += LX_WEB_SIGNATURE_BYTES;
    }
    *length = offset;
    return LXP_OK;
}

static lxp_result submitter_sign(lxp_activity *activity,
                                 const uint8_t private_key[32],
                                 uint8_t public_key[32],
                                 uint8_t signature[64])
{
    uint8_t preimage[32];
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    lxp_result status = LXP_OK;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                       private_key, 32U);
    context = EVP_MD_CTX_new();
    if (key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 ||
        public_length != 32U)
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_OK) {
        activity->authority.bytes = public_key;
        activity->authority.length = 32U;
        status = lxp_activity_signing_preimage(activity, preimage);
    }
    if (status == LXP_OK &&
        (EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
         EVP_DigestSign(context, signature, &signature_length, preimage,
                        sizeof(preimage)) != 1 ||
         signature_length != 64U))
        status = LXP_ERR_BAD_SIGNATURE;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    lxp_secure_zero(preimage, sizeof(preimage));
    return status;
}

lxp_result lx_web_activity_encode(const lx_web_observation *observation,
                                  const uint8_t submitter_private_key[32],
                                  uint32_t network_id,
                                  const uint8_t *actor_did,
                                  size_t actor_did_length,
                                  uint64_t account_sequence,
                                  lxp_u128 fee_limit,
                                  lxp_timestamp_bound timestamp_bound,
                                  lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_activity activity;
    uint8_t payload[LX_WEB_OBSERVATION_MAX_BYTES];
    uint8_t public_key[32];
    uint8_t signature[64];
    size_t payload_length;
    lxp_result status;
    if (observation == NULL || submitter_private_key == NULL ||
        actor_did == NULL || actor_did_length == 0U ||
        actor_did_length > LXP_MAX_DID_LENGTH || arena == NULL ||
        encoded == NULL || lxp_u128_is_zero(fee_limit) ||
        network_id == 0U || observation->network_id != network_id ||
        timestamp_bound.not_before > timestamp_bound.not_after)
        return LXP_ERR_NON_CANONICAL;
    status = lx_web_observation_encode(observation, payload,
                                       sizeof(payload), &payload_length);
    if (status != LXP_OK) return status;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = network_id;
    activity.activity_type = LX_WEB_OBSERVATION_ACTIVITY;
    activity.actor_did.bytes = actor_did;
    activity.actor_did.length = actor_did_length;
    activity.account_sequence = account_sequence;
    activity.timestamp_bound = timestamp_bound;
    status = lxp_hash_context_value(payload, payload_length,
                                    activity.idempotency_key);
    if (status == LXP_OK)
        status = lxp_hash_payload(payload, payload_length,
                                  activity.payload_hash);
    if (status != LXP_OK) return status;
    activity.fee_limit = fee_limit;
    activity.payload.bytes = payload;
    activity.payload.length = payload_length;
    status = submitter_sign(&activity, submitter_private_key, public_key,
                            signature);
    if (status != LXP_OK) return status;
    activity.signature.bytes = signature;
    activity.signature.length = 64U;
    return lxp_activity_encode(&activity, arena, encoded);
}

lxp_result lx_web_adapter_run(lx_web_adapter_config *config,
                              size_t *submitted)
{
    size_t count = 0U;
    if (config == NULL || submitted == NULL ||
        config->poll_observations == NULL ||
        config->submit_activity == NULL || config->actor_did == NULL ||
        config->actor_did_length == 0U || config->maximum_observations == 0U)
        return LXP_ERR_NON_CANONICAL;
    while (count < config->maximum_observations) {
        lx_web_observation observation;
        uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES];
        lxp_arena arena;
        lxp_byte_span activity;
        bool available = false;
        lxp_result status;
        (void)memset(&observation, 0, sizeof(observation));
        status = config->poll_observations(config->poll_context,
                                           &observation, &available);
        if (status != LXP_OK) return status;
        if (!available) break;
        status = lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes));
        if (status == LXP_OK)
            status = lx_web_activity_encode(
                &observation, config->submitter_private_key,
                config->network_id, config->actor_did,
                config->actor_did_length, config->next_account_sequence,
                config->fee_limit, config->timestamp_bound, &arena,
                &activity);
        if (status == LXP_OK)
            status = config->submit_activity(config->submit_context,
                                              activity.bytes,
                                              activity.length);
        if (status != LXP_OK) return status;
        ++config->next_account_sequence;
        ++count;
    }
    *submitted = count;
    return LXP_OK;
}
