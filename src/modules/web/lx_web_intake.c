#include "layerx/lx_web.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/programs.h"

#include <string.h>

static uint32_t get_u32(const uint8_t bytes[4])
{
    uint32_t value = 0U;
    size_t i;
    for (i = 0U; i < 4U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static uint64_t get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

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

static bool kind_known(uint8_t kind)
{
    return kind == LX_WEB_KIND_FETCH || kind == LX_WEB_KIND_SEARCH;
}

lxp_result lx_web_preimage_encode(uint8_t origin,
                                  const uint8_t network_id[32],
                                  const uint8_t requester[32],
                                  uint64_t request_id, uint8_t kind,
                                  const uint8_t payload_hash[32],
                                  const uint8_t content_digest[32],
                                  const uint8_t *response,
                                  size_t response_length,
                                  uint32_t full_length,
                                  uint8_t preimage[LX_WEB_PREIMAGE_BYTES])
{
    static const uint8_t domain[] = LX_WEB_PREIMAGE_DOMAIN;
    lxp_result status;
    if (network_id == NULL || requester == NULL || payload_hash == NULL ||
        content_digest == NULL || preimage == NULL ||
        (response == NULL && response_length != 0U) ||
        (origin != LX_WEB_ORIGIN_EVM && origin != LX_WEB_ORIGIN_PROGRAM) ||
        !kind_known(kind) || response_length > LX_WEB_MAX_RESPONSE_BYTES ||
        (size_t)full_length < response_length ||
        sizeof(domain) - 1U != LX_WEB_DOMAIN_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(preimage, domain, LX_WEB_DOMAIN_BYTES);
    preimage[14] = origin;
    (void)memcpy(preimage + 15U, network_id, 32U);
    (void)memcpy(preimage + 47U, requester, 32U);
    put_u64(preimage + 79U, request_id);
    preimage[87] = kind;
    (void)memcpy(preimage + 88U, payload_hash, 32U);
    (void)memcpy(preimage + 120U, content_digest, 32U);
    status = lxp_keccak256(response, response_length, preimage + 152U);
    if (status != LXP_OK) return status;
    put_u32(preimage + 184U, full_length);
    return LXP_OK;
}

lxp_result lx_web_observation_digest(const lx_web_observation *observation,
                                     uint8_t digest[32])
{
    uint8_t network_id[32];
    uint8_t preimage[LX_WEB_PREIMAGE_BYTES];
    lxp_result status;
    if (observation == NULL || digest == NULL ||
        observation->origin != LX_WEB_ORIGIN_PROGRAM ||
        observation->response_length > LX_WEB_MAX_RESPONSE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(network_id, 0, sizeof(network_id));
    put_u32(network_id + 28U, observation->network_id);
    status = lx_web_preimage_encode(
        observation->origin, network_id, observation->program_id,
        observation->request_id, observation->kind,
        observation->payload_hash, observation->content_digest,
        observation->response, observation->response_length,
        observation->full_length, preimage);
    if (status != LXP_OK) return status;
    return lxp_keccak256(preimage, sizeof(preimage), digest);
}

lxp_result lx_web_observation_decode(const uint8_t *bytes, size_t length,
                                     lx_web_observation *observation)
{
    uint8_t canonical[LX_WEB_OBSERVATION_MAX_BYTES];
    size_t canonical_length;
    size_t offset;
    size_t i;
    lxp_result status;
    if (bytes == NULL || observation == NULL ||
        length < LX_WEB_OBSERVATION_HEADER_BYTES + 1U ||
        length > LX_WEB_OBSERVATION_MAX_BYTES ||
        !lxp_ct_is_zero(bytes + 1U, 28U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(observation, 0, sizeof(*observation));
    observation->origin = bytes[0];
    observation->network_id = get_u32(bytes + 29U);
    (void)memcpy(observation->program_id, bytes + 33U, 32U);
    observation->request_id = get_u64(bytes + 65U);
    observation->kind = bytes[73];
    (void)memcpy(observation->payload_hash, bytes + 74U, 32U);
    (void)memcpy(observation->content_digest, bytes + 106U, 32U);
    observation->full_length = get_u32(bytes + 138U);
    observation->response_length = get_u32(bytes + 142U);
    if (observation->response_length > LX_WEB_MAX_RESPONSE_BYTES ||
        length < LX_WEB_OBSERVATION_HEADER_BYTES +
                     (size_t)observation->response_length + 1U)
        return LXP_ERR_NON_CANONICAL;
    offset = LX_WEB_OBSERVATION_HEADER_BYTES;
    (void)memcpy(observation->response, bytes + offset,
                 observation->response_length);
    offset += observation->response_length;
    observation->signature_count = bytes[offset++];
    if (observation->signature_count == 0U ||
        observation->signature_count > LX_WEB_MAX_ATTESTORS ||
        length != offset + observation->signature_count *
                               LX_WEB_SIGNATURE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < observation->signature_count; ++i) {
        (void)memcpy(observation->signatures[i], bytes + offset,
                     LX_WEB_SIGNATURE_BYTES);
        offset += LX_WEB_SIGNATURE_BYTES;
    }
    status = lx_web_observation_encode(observation, canonical,
                                       sizeof(canonical), &canonical_length);
    if (status != LXP_OK || canonical_length != length ||
        memcmp(canonical, bytes, length) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_web_request_record_decode(const uint8_t *data, size_t length,
                                        uint64_t *request_id, uint8_t *kind,
                                        lxp_byte_span *payload)
{
    uint32_t payload_length;
    if (data == NULL || request_id == NULL || kind == NULL ||
        payload == NULL || length < LX_WEB_REQUEST_RECORD_HEADER_BYTES)
        return LXP_ERR_NON_CANONICAL;
    payload_length = get_u32(data + 9U);
    if (!kind_known(data[8]) || payload_length == 0U ||
        length - LX_WEB_REQUEST_RECORD_HEADER_BYTES != (size_t)payload_length)
        return LXP_ERR_NON_CANONICAL;
    *request_id = get_u64(data);
    *kind = data[8];
    payload->bytes = data + LX_WEB_REQUEST_RECORD_HEADER_BYTES;
    payload->length = payload_length;
    return LXP_OK;
}

static lxp_result pending_find(lx_web_store *store,
                               const uint8_t program_id[32],
                               uint64_t request_id,
                               lx_web_pending_request **pending)
{
    size_t i;
    for (i = 0U; i < store->pending_count; ++i)
        if (store->pending[i].request_id == request_id &&
            memcmp(store->pending[i].program_id, program_id, 32U) == 0) {
            *pending = &store->pending[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_web_pending_add(lx_web_store *store,
                              const uint8_t program_id[32],
                              uint64_t request_id, uint8_t kind,
                              const uint8_t *payload, size_t payload_length,
                              uint64_t recorded_sequence)
{
    lx_web_pending_request *existing;
    lx_web_pending_request *entry;
    lxp_result status;
    if (store == NULL || program_id == NULL || payload == NULL ||
        payload_length == 0U || lxp_ct_is_zero(program_id, 32U) ||
        !kind_known(kind) || store->pending_count > LX_WEB_PENDING_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (pending_find(store, program_id, request_id, &existing) == LXP_OK)
        return LXP_ERR_DUPLICATE_ENTRY;
    if (store->pending_count == LX_WEB_PENDING_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    entry = &store->pending[store->pending_count];
    (void)memset(entry, 0, sizeof(*entry));
    status = lxp_keccak256(payload, payload_length, entry->payload_hash);
    if (status != LXP_OK) return status;
    (void)memcpy(entry->program_id, program_id, 32U);
    entry->request_id = request_id;
    entry->kind = kind;
    entry->recorded_sequence = recorded_sequence;
    ++store->pending_count;
    return LXP_OK;
}

static lxp_result signatures_verify(
    const lx_web_attestor_set *attestors,
    const lx_web_observation *observation, const uint8_t digest[32],
    uint8_t signers[LX_WEB_MAX_ATTESTORS][LX_WEB_SIGNER_BYTES])
{
    size_t i;
    if (observation->signature_count < attestors->threshold)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    for (i = 0U; i < observation->signature_count; ++i) {
        const uint8_t *signature = observation->signatures[i];
        const lx_web_attestor *attestor;
        uint8_t v = signature[64];
        int order;
        if ((v != 27U && v != 28U) || !lxp_secp256k1_sig_is_low_s(signature))
            return LXP_ERR_BAD_SIGNATURE;
        if (lxp_secp256k1_recover_address(signature, (uint8_t)(v - 27U),
                                          digest, signers[i]) != LXP_OK)
            return LXP_ERR_BAD_SIGNATURE;
        if (i != 0U) {
            order = memcmp(signers[i - 1U], signers[i], LX_WEB_SIGNER_BYTES);
            if (order == 0) return LXP_ERR_AUTH_DUPLICATE_SIGNER;
            if (order > 0) return LXP_ERR_UNSORTED_SEQUENCE;
        }
        if (lx_web_attestor_lookup(attestors, signers[i], &attestor) !=
            LXP_OK)
            return LXP_ERR_INVALID_ATTESTATION;
    }
    return LXP_OK;
}

_Static_assert((int)LX_WEB_ANSWER_CHUNK_BYTES <=
                   (int)LXP_MODULE_MAX_VALUE_BYTES &&
                   (int)LX_WEB_ANSWER_KEY_BYTES <=
                   (int)LXP_MODULE_MAX_KEY_BYTES &&
                   (int)LX_WEB_ANSWER_MAX_CHUNKS *
                   (int)LX_WEB_ANSWER_CHUNK_BYTES ==
                   (int)LX_WEB_MAX_RESPONSE_BYTES,
               "web answer parts fit module storage entries");

static const uint8_t answer_prefix[LX_WEB_ANSWER_PREFIX_BYTES] = {
    'w', 'e', 'b', '/', 'a', 'n', 's', 'w', 'e', 'r'
};

static void answer_key(uint8_t key[LX_WEB_ANSWER_KEY_BYTES],
                       const uint8_t program_id[32], uint64_t request_id,
                       uint8_t part)
{
    (void)memcpy(key, answer_prefix, LX_WEB_ANSWER_PREFIX_BYTES);
    (void)memcpy(key + LX_WEB_ANSWER_PREFIX_BYTES, program_id, 32U);
    put_u64(key + LX_WEB_ANSWER_PREFIX_BYTES + 32U, request_id);
    key[LX_WEB_ANSWER_KEY_BYTES - 1U] = part;
}

static size_t answer_chunks(uint32_t response_length)
{
    return ((size_t)response_length + LX_WEB_ANSWER_CHUNK_BYTES - 1U) /
           LX_WEB_ANSWER_CHUNK_BYTES;
}

lxp_result lx_web_committed_put(lxp_module_ctx *ctx,
                                const lx_web_observation *observation)
{
    uint8_t key[LX_WEB_ANSWER_KEY_BYTES];
    uint8_t header[LX_WEB_ANSWER_HEADER_BYTES];
    size_t chunks;
    size_t part;
    lxp_result status;
    if (ctx == NULL || observation == NULL ||
        observation->response_length > LX_WEB_MAX_RESPONSE_BYTES ||
        observation->response_length > observation->full_length ||
        ctx->staged_reserve > LXP_MODULE_MAX_STAGED_WRITES ||
        ctx->staged_count > LXP_MODULE_MAX_STAGED_WRITES - ctx->staged_reserve)
        return LXP_ERR_NON_CANONICAL;
    chunks = answer_chunks(observation->response_length);
    if (LXP_MODULE_MAX_STAGED_WRITES - ctx->staged_reserve -
            ctx->staged_count < chunks + 1U)
        return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(header, observation->content_digest, 32U);
    put_u32(header + 32U, observation->full_length);
    put_u32(header + 36U, observation->response_length);
    answer_key(key, observation->program_id, observation->request_id, 0U);
    status = lxp_ctx_kv_put(ctx, key, sizeof(key), header, sizeof(header));
    for (part = 0U; status == LXP_OK && part < chunks; ++part) {
        size_t offset = part * LX_WEB_ANSWER_CHUNK_BYTES;
        size_t length = observation->response_length - offset;
        if (length > LX_WEB_ANSWER_CHUNK_BYTES)
            length = LX_WEB_ANSWER_CHUNK_BYTES;
        answer_key(key, observation->program_id, observation->request_id,
                   (uint8_t)(part + 1U));
        status = lxp_ctx_kv_put(ctx, key, sizeof(key),
                                observation->response + offset, length);
    }
    return status;
}

static lxp_result committed_part(const lxp_module_ctx *ctx,
                                 const uint8_t key[LX_WEB_ANSWER_KEY_BYTES],
                                 const uint8_t **bytes, size_t *length)
{
    const lxp_kernel *kernel = ctx->kernel;
    size_t i;
    if (kernel == NULL || kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != ctx->module_id ||
            entry->key_length != LX_WEB_ANSWER_KEY_BYTES ||
            memcmp(entry->key, key, LX_WEB_ANSWER_KEY_BYTES) != 0)
            continue;
        *bytes = entry->value;
        *length = entry->value_length;
        return LXP_OK;
    }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_web_committed_read(lxp_module_ctx *ctx,
                                 const uint8_t program_id[32],
                                 uint64_t request_id, lx_web_answer *answer)
{
    uint8_t key[LX_WEB_ANSWER_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    size_t chunks;
    size_t part;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || answer == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(answer, 0, sizeof(*answer));
    answer_key(key, program_id, request_id, 0U);
    status = committed_part(ctx, key, &bytes, &length);
    if (status != LXP_OK) return status;
    if (length != LX_WEB_ANSWER_HEADER_BYTES) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(answer->program_id, program_id, 32U);
    answer->request_id = request_id;
    (void)memcpy(answer->content_digest, bytes, 32U);
    answer->full_length = get_u32(bytes + 32U);
    answer->response_length = get_u32(bytes + 36U);
    if (answer->response_length > LX_WEB_MAX_RESPONSE_BYTES ||
        answer->response_length > answer->full_length)
        return LXP_ERR_NON_CANONICAL;
    chunks = answer_chunks(answer->response_length);
    for (part = 0U; part < chunks; ++part) {
        size_t offset = part * LX_WEB_ANSWER_CHUNK_BYTES;
        size_t expected = answer->response_length - offset;
        if (expected > LX_WEB_ANSWER_CHUNK_BYTES)
            expected = LX_WEB_ANSWER_CHUNK_BYTES;
        answer_key(key, program_id, request_id, (uint8_t)(part + 1U));
        status = committed_part(ctx, key, &bytes, &length);
        if (status != LXP_OK || length != expected)
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(answer->response + offset, bytes, length);
    }
    return LXP_OK;
}

enum {
    PENDING_PREFIX_BYTES = 11,
    PENDING_KEY_BYTES = PENDING_PREFIX_BYTES + 32 + 8,
    PENDING_RECORD_VERSION = 1,
    PENDING_RECORD_BYTES = 123
};

/* The pending record a paid program request call staged: kind, payload hash,
 * the fee asset, account and amount, the recording sequence and the
 * fulfilled flag. */
typedef struct pending_record {
    bool present;
    uint8_t bytes[PENDING_RECORD_BYTES];
} pending_record;

static void pending_key(uint8_t key[PENDING_KEY_BYTES],
                        const uint8_t program_id[32], uint64_t request_id)
{
    static const uint8_t prefix[PENDING_PREFIX_BYTES] = {
        'w', 'e', 'b', '/', 'p', 'e', 'n', 'd', 'i', 'n', 'g'
    };
    (void)memcpy(key, prefix, sizeof(prefix));
    (void)memcpy(key + PENDING_PREFIX_BYTES, program_id, 32U);
    put_u64(key + PENDING_PREFIX_BYTES + 32U, request_id);
}

static lxp_result pending_record_load(lxp_module_ctx *ctx,
                                      const uint8_t program_id[32],
                                      uint64_t request_id,
                                      pending_record *record)
{
    uint8_t key[PENDING_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    (void)memset(record, 0, sizeof(*record));
    pending_key(key, program_id, request_id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (length != PENDING_RECORD_BYTES ||
        bytes[0] != PENDING_RECORD_VERSION || !kind_known(bytes[1]) ||
        bytes[122] > 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(record->bytes, bytes, PENDING_RECORD_BYTES);
    record->present = true;
    return LXP_OK;
}

/* Pays the request fee out of the web fee account to the kernel payout
 * accounts of the attestors whose signatures the answer carries, in equal
 * shares; the remainder of the division goes to the lowest signer, which is
 * the first in the ascending signer order. */
static lxp_result fee_split(lxp_module_ctx *ctx,
                            const lx_web_attestor_set *attestors,
                            const pending_record *record,
                            uint8_t signers[LX_WEB_MAX_ATTESTORS]
                                           [LX_WEB_SIGNER_BYTES],
                            size_t signer_count)
{
    lx_programs_transfer_runtime *runtime;
    lxp_transfer_set set;
    lxp_transfer_source_authority authority;
    lxp_receipt receipt;
    lx_account *source;
    lxp_u128 amount;
    lxp_u128 share;
    lxp_u128 remainder;
    size_t i;
    lxp_result status;
    if (signer_count == 0U || signer_count > LX_WEB_MAX_ATTESTORS ||
        signer_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->assets == NULL)
        return LXP_ERR_MODULE_DISABLED;
    status = lxp_u128_from_be(record->bytes + 98U, &amount);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(amount)) return LXP_OK;
    status = lxp_u128_mul_div_floor(amount, (lxp_u128){0U, 1U},
                                    (lxp_u128){0U, signer_count}, &share,
                                    &remainder);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, record->bytes + 66U, &source);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(&receipt, 0, sizeof(receipt));
    for (i = 0U; i < signer_count; ++i) {
        const lx_web_attestor *attestor;
        lxp_transfer_leg *leg = &set.legs[set.leg_count];
        lxp_u128 leg_amount = share;
        if (i == 0U) {
            status = lxp_u128_add(share, remainder, &leg_amount);
            if (status != LXP_OK) return status;
        }
        if (lxp_u128_is_zero(leg_amount)) continue;
        status = lx_web_attestor_lookup(attestors, signers[i], &attestor);
        if (status == LXP_OK)
            status = lxp_ctx_account_find(ctx, attestor->payout_account,
                                          &leg->to);
        if (status != LXP_OK) return status;
        leg->from = source;
        (void)memcpy(leg->asset_id, record->bytes + 34U, 32U);
        leg->amount = leg_amount;
        leg->reason = LXP_REASON_PAYMENT;
        leg->supply_mode = LXP_TRANSFER_CONSERVED;
        ++set.leg_count;
    }
    (void)memcpy(authority.authorized_from, source->id, 32U);
    authority.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    authority.protocol_system_capability = true;
    set.context.assets = runtime->assets;
    set.context.asset_count = runtime->asset_count;
    (void)memcpy(set.context.authorized_from, source->id, 32U);
    set.context.protocol_system_capability = true;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    set.context.source_authorities = &authority;
    set.context.source_authority_count = 1U;
    set.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    return lxp_ctx_emit_transfer_set(ctx, &set, &receipt);
}

static lxp_result pending_record_fulfil(lxp_module_ctx *ctx,
                                        const uint8_t program_id[32],
                                        uint64_t request_id,
                                        pending_record *record)
{
    uint8_t key[PENDING_KEY_BYTES];
    pending_key(key, program_id, request_id);
    record->bytes[122] = 1U;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), record->bytes,
                          PENDING_RECORD_BYTES);
}

/* Admits an observation for a request the store tracks or one a program call
 * recorded and paid for in the Programs module storage this context reads:
 * the recorded request supplies the kind and payload hash the observation
 * must match, the answer is committed beside it for web_read, and the fee is
 * split among the signers. */
lxp_result lx_web_intake(lxp_module_ctx *ctx,
                         const lx_web_intake_request *request,
                         lx_web_committed *committed)
{
    lx_web_observation observation;
    lx_web_pending_request admitted;
    lx_web_pending_request *pending;
    lx_web_committed *entry;
    pending_record record;
    uint8_t digest[32];
    uint8_t signers[LX_WEB_MAX_ATTESTORS][LX_WEB_SIGNER_BYTES];
    bool tracked;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->attestors == NULL || committed == NULL ||
        request->store->committed_count > LX_WEB_STORE_CAPACITY ||
        request->store->pending_count > LX_WEB_PENDING_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lx_web_observation_decode(request->payload,
                                       request->payload_length,
                                       &observation);
    if (status != LXP_OK) return status;
    if (observation.network_id != request->store->network_id)
        return LXP_ERR_WRONG_NETWORK;
    status = pending_record_load(ctx, observation.program_id,
                                 observation.request_id, &record);
    if (status != LXP_OK) return status;
    status = pending_find(request->store, observation.program_id,
                          observation.request_id, &pending);
    tracked = status == LXP_OK;
    if (!tracked && status != LXP_ERR_UNKNOWN_FIELD) return status;
    if (!tracked && !record.present) return LXP_ERR_UNKNOWN_FIELD;
    if ((tracked && pending->fulfilled) ||
        (record.present && record.bytes[122] != 0U))
        return LXP_ERR_SEQUENCE_REUSED;
    if (!tracked) {
        if (request->store->pending_count == LX_WEB_PENDING_CAPACITY)
            return LXP_ERR_ARENA_EXHAUSTED;
        (void)memset(&admitted, 0, sizeof(admitted));
        (void)memcpy(admitted.program_id, observation.program_id, 32U);
        admitted.request_id = observation.request_id;
        admitted.kind = record.bytes[1];
        (void)memcpy(admitted.payload_hash, record.bytes + 2U, 32U);
        admitted.recorded_sequence = get_u64(record.bytes + 114U);
        pending = &admitted;
    }
    if (pending->kind != observation.kind ||
        memcmp(pending->payload_hash, observation.payload_hash, 32U) != 0 ||
        (record.present &&
         (record.bytes[1] != observation.kind ||
          memcmp(record.bytes + 2U, observation.payload_hash, 32U) != 0)))
        return LXP_ERR_CONTEXT_MISMATCH;
    if (lx_web_attestor_set_validate(request->attestors) != LXP_OK)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    status = lx_web_observation_digest(&observation, digest);
    if (status == LXP_OK)
        status = signatures_verify(request->attestors, &observation, digest,
                                   signers);
    if (status != LXP_OK) return status;
    if (request->store->committed_count == LX_WEB_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = lx_web_committed_put(ctx, &observation);
    if (status == LXP_OK && record.present)
        status = fee_split(ctx, request->attestors, &record, signers,
                           observation.signature_count);
    if (status == LXP_OK && record.present)
        status = pending_record_fulfil(ctx, observation.program_id,
                                       observation.request_id, &record);
    if (status != LXP_OK) return status;
    if (!tracked) {
        admitted.fulfilled = true;
        request->store->pending[request->store->pending_count++] = admitted;
    } else {
        pending->fulfilled = true;
    }
    entry = &request->store->committed[request->store->committed_count++];
    (void)memset(entry, 0, sizeof(*entry));
    entry->observation = observation;
    (void)memcpy(entry->attestation_digest, digest, 32U);
    (void)memcpy(entry->signers, signers,
                 observation.signature_count * LX_WEB_SIGNER_BYTES);
    entry->signer_count = observation.signature_count;
    entry->global_sequence = lxp_ctx_global_sequence(ctx);
    *committed = *entry;
    return LXP_OK;
}

lxp_result lx_web_committed_lookup(const lx_web_store *store,
                                   const uint8_t program_id[32],
                                   uint64_t request_id,
                                   const lx_web_committed **committed)
{
    size_t i;
    if (store == NULL || program_id == NULL || committed == NULL ||
        store->committed_count > LX_WEB_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->committed_count; ++i)
        if (store->committed[i].observation.request_id == request_id &&
            memcmp(store->committed[i].observation.program_id, program_id,
                   32U) == 0) {
            *committed = &store->committed[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}
