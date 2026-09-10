#include "layerx/lx_service.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <openssl/evp.h>
#include <string.h>

enum { WORK_ARENA_BYTES = 16384, GAS_LIMIT = 100000 };

static const uint8_t execution_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'e', 'x', 'e', 'c', 'u', 't', ':', '1'
};

static lxp_state_store store_state;
static lxp_state_journal state_journal;
static lxp_kernel service_kernel;
static lxp_effect_buffer event_buffer;
static lxp_arena work_arena;
static uint8_t work_bytes[WORK_ARENA_BYTES];
static uint64_t parameter_set = 1U;

static void be64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static void id32(uint8_t out[32], uint8_t marker)
{
    (void)memset(out, 0, 32U);
    out[0] = marker;
}

static int sign_execution(lx_service_execution *execution,
                          const uint8_t seed[32])
{
    uint8_t message[384];
    uint8_t digest[32];
    size_t message_length = 0U;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                 seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int failed = key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, execution->public_key,
                                    &public_length) != 1 ||
        public_length != 32U;
    if (!failed)
        failed = lx_service_attestation_bytes(execution, message,
                                              sizeof(message),
                                              &message_length) != LXP_OK ||
            lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message,
                            message_length, digest) != LXP_OK ||
            EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
            EVP_DigestSign(context, execution->signature, &signature_length,
                           digest, sizeof(digest)) != 1 ||
            signature_length != 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return failed;
}

static size_t offer_payload(uint8_t *out)
{
    size_t offset = 0U;
    (void)memset(out, 0, (size_t)LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES);
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    id32(out + offset, 10U); offset += 32U;
    id32(out + offset, 11U); offset += 32U;
    be64(out + offset + 8U, 25U); offset += 16U;
    id32(out + offset, 12U); offset += 32U;
    id32(out + offset, 13U); offset += 32U;
    be64(out + offset, 1000U); offset += 8U;
    be64(out + offset, 200U); offset += 8U;
    be64(out + offset, 300U); offset += 8U;
    out[offset++] = (uint8_t)LX_SERVICE_DEFAULT_ACCEPT;
    be64(out + offset, 900U); offset += 8U;
    return offset;
}

static size_t identifier_payload(uint8_t *out, const uint8_t identifier[32])
{
    out[0] = 0U;
    out[1] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + LX_SERVICE_PAYLOAD_VERSION_BYTES, identifier, 32U);
    return (size_t)LX_SERVICE_IDENTIFIER_PAYLOAD_BYTES;
}

static size_t propose_payload(uint8_t *out, const uint8_t agreement_id[32],
                              const uint8_t offer_id[32],
                              const uint8_t terms_hash[32],
                              const uint8_t escrow_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, offer_id, 32U); offset += 32U;
    (void)memcpy(out + offset, terms_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, escrow_id, 32U); offset += 32U;
    return offset;
}

static size_t commit_payload(uint8_t *out, const uint8_t commitment_id[32],
                             const uint8_t agreement_id[32],
                             const uint8_t task_hash[32],
                             const uint8_t escrow_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, commitment_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, task_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, escrow_id, 32U); offset += 32U;
    be64(out + offset, 900U); offset += 8U;
    be64(out + offset, 100U); offset += 8U;
    return offset;
}

static size_t abandon_payload(uint8_t *out, const uint8_t commitment_id[32])
{
    out[0] = 0U;
    out[1] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + LX_SERVICE_PAYLOAD_VERSION_BYTES, commitment_id, 32U);
    out[LX_SERVICE_PAYLOAD_VERSION_BYTES + 32U] = 0U;
    out[LX_SERVICE_PAYLOAD_VERSION_BYTES + 33U] = 5U;
    return (size_t)LX_SERVICE_COMMIT_ABANDON_PAYLOAD_BYTES;
}

static int open_ctx(lxp_module_ctx *ctx, uint64_t timestamp,
                    uint64_t sequence)
{
    if (lxp_arena_init(&work_arena, work_bytes, sizeof(work_bytes)) !=
            LXP_OK ||
        lxp_effect_buffer_init(&event_buffer) != LXP_OK ||
        lxp_module_ctx_init(ctx, &service_kernel, LXP_MODULE_SERVICE,
                            timestamp, 0U, sequence, (uint64_t)GAS_LIMIT,
                            &work_arena, true) != LXP_OK)
        return 1;
    return lxp_module_ctx_bind_effects(ctx, &event_buffer) == LXP_OK ? 0 : 1;
}

static lxp_result dispatch(uint32_t activity_type, const uint8_t *payload,
                           size_t length,
                           const lxp_authority_resolved *authority,
                           uint64_t timestamp, uint64_t sequence,
                           uint8_t activity_marker,
                           lxp_result *module_result)
{
    const lxp_module_registration *registration = NULL;
    lxp_activity activity;
    lxp_module_ctx ctx;
    lxp_result status;

    *module_result = LXP_FATAL_INVARIANT;
    status = lxp_kernel_module_for_activity(&service_kernel, activity_type,
                                            0U, &registration);
    if (status != LXP_OK) return status;
    if (open_ctx(&ctx, timestamp, sequence) != 0) return LXP_FATAL_INVARIANT;
    ctx.activity_id[0] = activity_marker;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = activity_type;
    activity.payload.bytes = payload;
    activity.payload.length = length;
    status = lxp_kernel_dispatch(registration, &ctx, &activity, authority,
                                 &event_buffer, module_result);
    if (status != LXP_OK) return status;
    if (*module_result == LXP_OK) return lxp_module_ctx_commit(&ctx);
    lxp_module_ctx_rollback(&ctx);
    return LXP_OK;
}

static int event_is(uint16_t event_type, const uint8_t primary[32],
                    const uint8_t secondary[32], uint8_t code,
                    uint64_t sequence)
{
    uint8_t body[LX_SERVICE_EVENT_BODY_BYTES];
    const lxp_effect *effect = &event_buffer.effects[0];
    if (event_buffer.count != 1U || effect->kind != LXP_EFFECT_EVENT ||
        effect->monetary || effect->module_id != LXP_MODULE_SERVICE ||
        effect->event_type != event_type ||
        effect->body_length != (uint16_t)LX_SERVICE_EVENT_BODY_BYTES)
        return 1;
    (void)memcpy(body, primary, 32U);
    (void)memcpy(body + 32U, secondary, 32U);
    body[64] = code;
    be64(body + 65U, sequence);
    return memcmp(effect->body, body, sizeof(body)) == 0 ? 0 : 1;
}

static int record_present(const uint8_t identifier[32], size_t expected)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t length = 0U;
    lxp_module_ctx ctx;
    if (open_ctx(&ctx, 1U, 1U) != 0) return 1;
    (void)memcpy(key, execution_prefix, (size_t)LX_SERVICE_KEY_PREFIX_BYTES);
    (void)memcpy(key + LX_SERVICE_KEY_PREFIX_BYTES, identifier, 32U);
    if (lxp_ctx_kv_get(&ctx, key, sizeof(key), &value, &length) != LXP_OK)
        return 1;
    return length == expected ? 0 : 1;
}

static int grant_refusals(const lx_service_execution *execution,
                          const uint8_t public_key[32])
{
    lx_service_attestor_grant grant;
    lx_service_attest_request request;
    lx_service_execution accepted;
    lxp_module_ctx ctx;
    unsigned int variant;
    if (open_ctx(&ctx, 100U, 1U) != 0) return 1;
    for (variant = 0U; variant < 7U; ++variant) {
        (void)memset(&grant, 0, sizeof(grant));
        (void)memcpy(grant.principal, execution->attestor_identity, 32U);
        (void)memcpy(grant.public_key, public_key, 32U);
        grant.module_id = LXP_MODULE_SERVICE;
        grant.activity_type = LX_SERVICE_TOOL_EXEC_ATTEST;
        grant.not_before = 10U;
        grant.not_after = 900U;
        switch (variant) {
        case 0U: grant.revoked = true; break;
        case 1U: grant.module_id = LXP_MODULE_ESCROW; break;
        case 2U: grant.activity_type = LX_SERVICE_DELIVER; break;
        case 3U: grant.not_before = 101U; break;
        case 4U: grant.not_after = 99U; break;
        case 5U: grant.public_key[0] ^= 1U; break;
        case 6U: grant.principal[0] ^= 1U; break;
        default: break;
        }
        (void)memset(&request, 0, sizeof(request));
        request.execution = *execution;
        request.grant = &grant;
        if (lx_service_attestor_verify(&ctx, execution, &grant, 100U) !=
                LXP_ERR_INVALID_ATTESTATION ||
            lx_service_tool_exec_attest_execute(&ctx, &request, &accepted) !=
                LXP_ERR_INVALID_ATTESTATION)
            return 1;
    }
    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.principal, execution->attestor_identity, 32U);
    (void)memcpy(grant.public_key, public_key, 32U);
    grant.module_id = LXP_MODULE_SERVICE;
    grant.activity_type = LX_SERVICE_TOOL_EXEC_ATTEST;
    (void)memset(&request, 0, sizeof(request));
    request.execution = *execution;
    request.grant = &grant;
    request.attempts_balance_mutation = true;
    if (lx_service_tool_exec_attest_execute(&ctx, &request, &accepted) !=
        LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE)
        return 1;
    return 0;
}

int main(void)
{
    static const uint8_t seed[32] = { 7U };
    uint8_t payload[LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES];
    uint8_t encoded[LX_SERVICE_EXECUTION_BYTES];
    uint8_t reencoded[LX_SERVICE_EXECUTION_BYTES];
    uint8_t offer_id[32];
    uint8_t agreement_id[32];
    uint8_t escrow_id[32];
    uint8_t terms_hash[32];
    uint8_t commitment_id[32];
    uint8_t task_hash[32];
    uint8_t attestation_id[32];
    lx_service_execution execution;
    lx_service_execution stored;
    lx_service_execution decoded;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_authority_resolved wrong_key;
    lxp_module_ctx ctx;
    lxp_result outcome = LXP_OK;
    size_t length;
    size_t encoded_length = 0U;
    size_t reencoded_length = 0U;

    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&outsider, 0, sizeof(outsider));
    (void)memset(&wrong_key, 0, sizeof(wrong_key));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    outsider.principal[0] = 3U;
    id32(offer_id, 10U);
    id32(agreement_id, 40U);
    id32(escrow_id, 41U);
    id32(terms_hash, 12U);
    id32(commitment_id, 50U);
    id32(task_hash, 51U);
    id32(attestation_id, 80U);

    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK)
        return 1;

    length = offer_payload(payload);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 1U, 4U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = propose_payload(payload, agreement_id, offer_id, terms_hash,
                             escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer,
                 100U, 2U, 5U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = identifier_payload(payload, agreement_id);
    if (dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &provider,
                 100U, 3U, 6U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = commit_payload(payload, commitment_id, agreement_id, task_hash,
                            escrow_id);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 4U, 7U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;

    (void)memset(&execution, 0, sizeof(execution));
    (void)memcpy(execution.attestation_id, attestation_id, 32U);
    id32(execution.activity_id, 8U);
    (void)memcpy(execution.agreement_id, agreement_id, 32U);
    (void)memcpy(execution.commitment_id, commitment_id, 32U);
    id32(execution.tool_id, 81U);
    id32(execution.input_commitment_hash, 82U);
    id32(execution.output_commitment_hash, 83U);
    execution.execution_start = 10U;
    execution.execution_end = 90U;
    execution.resource_units = 80U;
    (void)memcpy(execution.attestor_identity, provider.principal, 32U);
    id32(execution.availability_reference, 84U);
    if (sign_execution(&execution, seed) != 0) return 1;
    (void)memcpy(provider.verified_key, execution.public_key, 32U);
    (void)memcpy(outsider.verified_key, execution.public_key, 32U);
    (void)memcpy(wrong_key.principal, provider.principal, 32U);
    (void)memcpy(wrong_key.verified_key, execution.public_key, 32U);
    wrong_key.verified_key[0] ^= 1U;

    if (grant_refusals(&execution, execution.public_key) != 0) return 1;

    if (lx_service_execution_encode(&execution, encoded, sizeof(encoded),
                                    &encoded_length) != LXP_OK ||
        encoded_length != (size_t)LX_SERVICE_EXECUTION_BYTES ||
        lx_service_execution_decode(encoded, encoded_length, &decoded) !=
            LXP_OK ||
        lx_service_execution_encode(&decoded, reencoded, sizeof(reencoded),
                                    &reencoded_length) != LXP_OK ||
        encoded_length != reencoded_length ||
        memcmp(encoded, reencoded, encoded_length) != 0 ||
        lx_service_execution_decode(encoded, encoded_length - 1U,
                                    &decoded) != LXP_ERR_NON_CANONICAL)
        return 1;

    if (dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 5U, 9U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &outsider, 100U, 5U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &wrong_key, 100U, 5U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    encoded[LX_SERVICE_ATTESTATION_BYTES] ^= 1U;
    if (dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 5U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    encoded[LX_SERVICE_ATTESTATION_BYTES] ^= 1U;
    if (dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 6U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_EXECUTION_ATTESTED,
                 attestation_id, commitment_id, 0U, 6U) != 0 ||
        record_present(attestation_id,
                       (size_t)LX_SERVICE_EXECUTION_RECORD_BYTES) != 0 ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 7U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;
    if (open_ctx(&ctx, 100U, 8U) != 0 ||
        lx_service_execution_lookup(&ctx, attestation_id, &stored) !=
            LXP_OK ||
        stored.global_sequence != 6U ||
        stored.canonical_payload_length == 0U ||
        stored.execution_start != 10U || stored.execution_end != 90U ||
        stored.resource_units != 80U ||
        memcmp(stored.public_key, execution.public_key, 32U) != 0 ||
        memcmp(stored.signature, execution.signature, 64U) != 0 ||
        memcmp(stored.commitment_id, commitment_id, 32U) != 0)
        return 1;

    (void)memset(&execution, 0, sizeof(execution));
    (void)memcpy(execution.attestation_id, attestation_id, 32U);
    execution.attestation_id[0] = 85U;
    id32(execution.activity_id, 8U);
    (void)memcpy(execution.agreement_id, agreement_id, 32U);
    (void)memcpy(execution.commitment_id, commitment_id, 32U);
    execution.commitment_id[0] = 86U;
    id32(execution.tool_id, 81U);
    id32(execution.input_commitment_hash, 82U);
    id32(execution.output_commitment_hash, 83U);
    execution.execution_start = 10U;
    execution.execution_end = 90U;
    execution.resource_units = 80U;
    (void)memcpy(execution.attestor_identity, provider.principal, 32U);
    id32(execution.availability_reference, 84U);
    if (sign_execution(&execution, seed) != 0 ||
        lx_service_execution_encode(&execution, encoded, sizeof(encoded),
                                    &encoded_length) != LXP_OK ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 9U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    (void)memcpy(execution.commitment_id, commitment_id, 32U);
    execution.resource_units = 0U;
    if (sign_execution(&execution, seed) != 0 ||
        lx_service_execution_encode(&execution, encoded, sizeof(encoded),
                                    &encoded_length) != LXP_OK ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 10U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    execution.resource_units = 80U;
    execution.execution_end = 5U;
    if (sign_execution(&execution, seed) != 0 ||
        lx_service_execution_encode(&execution, encoded, sizeof(encoded),
                                    &encoded_length) != LXP_OK ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 11U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;

    execution.execution_end = 90U;
    if (sign_execution(&execution, seed) != 0 ||
        lx_service_execution_encode(&execution, encoded, sizeof(encoded),
                                    &encoded_length) != LXP_OK)
        return 1;
    length = abandon_payload(payload, commitment_id);
    if (dispatch(LX_SERVICE_COMMIT_ABANDON, payload, length, &provider, 100U,
                 12U, 13U, &outcome) != LXP_OK || outcome != LXP_OK ||
        dispatch(LX_SERVICE_TOOL_EXEC_ATTEST, encoded, encoded_length,
                 &provider, 100U, 13U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_INVALID_ATTESTATION)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
