#define _GNU_SOURCE

#include "layerx/lxp_activity.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_storage.h"

#include "layerx/programs.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_maintenance.h"
#include "layerx/lx_escrow.h"
#include "layerx/lx_budget.h"
#include "layerx/lx_stream.h"
#include "layerx/lx_service.h"
#include "layerx/lx_perps.h"
#include <sys/un.h>

#include <openssl/evp.h>

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define REQUIRE(condition) \
    do { \
        if (!(condition)) { \
            (void)fprintf(stderr, "test_module_maintenance:%d: %s\n", \
                          __LINE__, #condition); \
            return 1; \
        } \
    } while (0)

enum {
    NETWORK_ID = 77,
    LNI_MAJOR = 1,
    LNI_MINOR = 5,
    NODE_INFO_REQUEST = 1,
    NODE_INFO_RESPONSE = 2,
    SUBMIT_REQUEST = 3,
    SUBMIT_RESPONSE = 4,
    ERROR_RESPONSE = 25,
    ASSET_READ_REQUEST = 32,
    ASSET_READ_RESPONSE = 33,
    FEE_ESTIMATE_REQUEST = 34,
    FEE_ESTIMATE_RESPONSE = 35,
    TYPED_READ_MINOR = 5,
    ENVELOPE_FIXED_BYTES = 22,
    JOURNAL_SUPERBLOCK_BYTES = 32,
    JOURNAL_RECORD_BYTES = 64,
    ACTIVITY_CAPACITY = 4096,
    OWNER_SCRATCH_BYTES = 2 * 1024 * 1024,
    INVALID_FLOOD = LXP_DAEMON_QUEUE_CAPACITY + 16,
    WAIT_POLLS = 12000,
    IO_DEADLINE_MILLISECONDS = 10000
};

typedef struct signer { uint8_t private_key[32]; uint8_t public_key[32]; } signer;
typedef struct wire_envelope {
    uint8_t *owned;
    size_t owned_length;
    uint16_t major, minor, tag;
    uint64_t correlation_id;
    const uint8_t *payload;
    size_t payload_length;
    const uint8_t *proof;
    size_t proof_length;
} wire_envelope;
static uint16_t load_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t load_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t load_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void store_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void store_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void store_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static int descriptor_write_all(int descriptor, const uint8_t *bytes,
                                size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written > 0) offset += (size_t)written;
        else if (written < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int64_t monotonic_milliseconds(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static int descriptor_read_all_deadline(int descriptor, uint8_t *bytes,
                                        size_t length, int timeout_milliseconds)
{
    size_t offset = 0U;
    int64_t start = monotonic_milliseconds();
    int64_t deadline;
    if (start < 0 || timeout_milliseconds <= 0 ||
        start > INT64_MAX - timeout_milliseconds)
        return 1;
    deadline = start + timeout_milliseconds;
    while (offset < length) {
        struct pollfd pending;
        int64_t now = monotonic_milliseconds();
        int remaining;
        int ready;
        if (now < 0 || now >= deadline) return 1;
        remaining = deadline - now > INT_MAX ?
            INT_MAX : (int)(deadline - now);
        pending.fd = descriptor;
        pending.events = POLLIN;
        pending.revents = 0;
        ready = poll(&pending, 1U, remaining);
        if (ready < 0 && errno == EINTR) continue;
        if (ready <= 0 ||
            (pending.revents & (POLLERR | POLLNVAL)) != 0)
            return 1;
        ssize_t received = read(descriptor, bytes + offset, length - offset);
        if (received > 0) offset += (size_t)received;
        else if (received < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int descriptor_read_all(int descriptor, uint8_t *bytes, size_t length)
{
    return descriptor_read_all_deadline(
        descriptor, bytes, length, IO_DEADLINE_MILLISECONDS);
}

static int send_request(int descriptor, uint16_t minor, uint16_t tag,
                        uint64_t correlation_id, const uint8_t *payload,
                        size_t payload_length)
{
    uint8_t prefix[4];
    uint8_t *frame;
    size_t length;
    size_t cursor = 0U;
    int result;
    if ((payload == NULL && payload_length != 0U) ||
        payload_length > UINT32_MAX ||
        payload_length > SIZE_MAX - ENVELOPE_FIXED_BYTES)
        return 1;
    length = ENVELOPE_FIXED_BYTES + payload_length;
    frame = (uint8_t *)malloc(length);
    if (frame == NULL) return 1;
    store_u16(frame + cursor, LNI_MAJOR); cursor += 2U;
    store_u16(frame + cursor, minor); cursor += 2U;
    store_u16(frame + cursor, tag); cursor += 2U;
    store_u64(frame + cursor, correlation_id); cursor += 8U;
    store_u32(frame + cursor, (uint32_t)payload_length); cursor += 4U;
    if (payload_length != 0U) {
        (void)memcpy(frame + cursor, payload, payload_length);
        cursor += payload_length;
    }
    store_u32(frame + cursor, 0U); cursor += 4U;
    store_u32(prefix, (uint32_t)cursor);
    result = descriptor_write_all(descriptor, prefix, sizeof(prefix));
    if (result == 0) result = descriptor_write_all(descriptor, frame, cursor);
    free(frame);
    return result;
}

static int receive_envelope(int descriptor, wire_envelope *envelope)
{
    uint8_t prefix[4];
    uint32_t length;
    uint32_t payload_length;
    uint32_t proof_length;
    size_t cursor = 0U;
    (void)memset(envelope, 0, sizeof(*envelope));
    if (descriptor_read_all(descriptor, prefix, sizeof(prefix)) != 0)
        return 1;
    length = load_u32(prefix);
    if (length < ENVELOPE_FIXED_BYTES ||
        length > LXP_DAEMON_LNI_MAX_FRAME_BYTES)
        return 1;
    envelope->owned = (uint8_t *)malloc(length);
    if (envelope->owned == NULL ||
        descriptor_read_all(descriptor, envelope->owned, length) != 0) {
        free(envelope->owned);
        envelope->owned = NULL;
        return 1;
    }
    envelope->owned_length = length;
    envelope->major = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->minor = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->tag = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->correlation_id = load_u64(envelope->owned + cursor); cursor += 8U;
    payload_length = load_u32(envelope->owned + cursor); cursor += 4U;
    if ((size_t)payload_length > length - cursor - 4U) return 1;
    envelope->payload = envelope->owned + cursor;
    envelope->payload_length = payload_length;
    cursor += payload_length;
    proof_length = load_u32(envelope->owned + cursor); cursor += 4U;
    if ((size_t)proof_length != length - cursor) return 1;
    envelope->proof = envelope->owned + cursor;
    envelope->proof_length = proof_length;
    return 0;
}

static void release_envelope(wire_envelope *envelope)
{
    free(envelope->owned);
    (void)memset(envelope, 0, sizeof(*envelope));
}

static int signer_init(signer *key, uint8_t seed)
{
    EVP_PKEY *pkey;
    size_t length = 32U;
    int ok;
    (void)memset(key->private_key, seed, sizeof(key->private_key));
    pkey = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                        key->private_key, 32U);
    ok = pkey != NULL && EVP_PKEY_get_raw_public_key(
        pkey, key->public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(pkey);
    return ok ? 0 : 1;
}

static int sign_raw(const signer *key, const uint8_t *message,
                    size_t message_length, uint8_t signature[64])
{
    EVP_PKEY *pkey = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                   key->private_key, 32U);
    EVP_MD_CTX *context = pkey == NULL ? NULL : EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, pkey) == 1 &&
        EVP_DigestSign(context, signature, &signature_length, message,
                       message_length) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(pkey);
    return ok ? 0 : 1;
}

static int build_activity(const signer *key, uint64_t account_sequence,
                          uint32_t activity_type, uint64_t timestamp, const uint8_t *payload, size_t payload_length, uint8_t *output,
                          size_t capacity, size_t *length)
{
    uint8_t *arena_storage;
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t did[75];
    static const uint8_t digits[] = "0123456789abcdef";
    uint8_t preimage[32];
    uint8_t signature[64];
    size_t index;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.network_id = NETWORK_ID;
    activity.activity_type = activity_type;
    (void)memcpy(did, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        did[11U + i * 2U] = digits[key->public_key[i] >> 4U];
        did[12U + i * 2U] = digits[key->public_key[i] & 15U];
    }
    activity.actor_did = (lxp_byte_span){did, sizeof(did)};
    activity.authority = (lxp_byte_span){key->public_key, 32U};
    activity.account_sequence = account_sequence;
    {
        struct timespec now;
        if (clock_gettime(CLOCK_REALTIME, &now) != 0) return 1;
        activity.timestamp_bound.not_before = timestamp != 0U ? timestamp : (uint64_t)now.tv_sec * 1000U;
        activity.timestamp_bound.not_after = activity.timestamp_bound.not_before + 300000U;
    }
    for (index = 0U; index < 8U; ++index)
        activity.idempotency_key[index] =
            (uint8_t)(account_sequence >> ((7U - index) * 8U));
    activity.idempotency_key[31] = 0xa5U;
    activity.fee_limit = (lxp_u128){0U, 0U};
    activity.payload = (lxp_byte_span){payload, payload_length};
    if (lxp_hash_payload(payload, payload_length, activity.payload_hash) !=
            LXP_OK ||
        lxp_activity_signing_preimage(&activity, preimage) != LXP_OK ||
        sign_raw(key, preimage, sizeof(preimage), signature) != 0)
        return 1;
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    arena_storage = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
    if (arena_storage == NULL) return 1;
    if (lxp_arena_init(&arena, arena_storage, LXP_MAX_ACTIVITY_BYTES) !=
            LXP_OK ||
        lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK ||
        encoded.length > capacity) {
        free(arena_storage);
        return 1;
    }
    (void)memcpy(output, encoded.bytes, encoded.length);
    *length = encoded.length;
    free(arena_storage);
    return 0;
}

static int handshake(int descriptor)
{
    wire_envelope response;
    size_t cursor = 93U;
    uint16_t capability_count;
    size_t index;
    bool durable = false;
    bool complete;
    if (send_request(descriptor, LNI_MINOR, NODE_INFO_REQUEST, 0U,
                     NULL, 0U) != 0 ||
        receive_envelope(descriptor, &response) != 0)
        return 1;
    if (response.major != LNI_MAJOR || response.minor != LNI_MINOR ||
        response.tag != NODE_INFO_RESPONSE || response.correlation_id != 0U ||
        response.proof_length != 0U || response.payload_length < cursor)
        return 1;
    capability_count = load_u16(response.payload + 91U);
    for (index = 0U; index < capability_count; ++index) {
        uint16_t length;
        if (cursor > response.payload_length - 2U) return 1;
        length = load_u16(response.payload + cursor); cursor += 2U;
        if ((size_t)length > response.payload_length - cursor) return 1;
        if (length == sizeof("authenticated_durable_submit") - 1U &&
            memcmp(response.payload + cursor,
                   "authenticated_durable_submit", length) == 0)
            durable = true;
        cursor += length;
    }
    complete = cursor == response.payload_length;
    release_envelope(&response);
    return durable && complete ? 0 : 1;
}

static int expect_error(int descriptor, uint64_t correlation_id,
                        uint8_t refusal_class, lxp_result result)
{
    wire_envelope response;
    if (receive_envelope(descriptor, &response) != 0) return 1;
    if (response.tag != ERROR_RESPONSE ||
        response.correlation_id != correlation_id ||
        response.payload_length != 5U || response.proof_length != 0U ||
        response.payload[0] != refusal_class ||
        (lxp_result)load_u32(response.payload + 1U) != result) {
        release_envelope(&response);
        return 1;
    }
    release_envelope(&response);
    return 0;
}

static int expect_ack(int descriptor, uint64_t correlation_id,
                      const uint8_t *activity, size_t activity_length,
                      const uint8_t activity_id[32])
{
    wire_envelope response;
    if (receive_envelope(descriptor, &response) != 0) return 1;
    if (response.tag == ERROR_RESPONSE && response.payload_length == 5U)
        (void)fprintf(stderr, "submission refusal class=%u result=%d\n",
                      response.payload[0], (int32_t)load_u32(response.payload + 1U));
    if (response.tag != SUBMIT_RESPONSE ||
        response.correlation_id != correlation_id ||
        response.payload_length != activity_length ||
        memcmp(response.payload, activity, activity_length) != 0 ||
        response.proof_length != 32U ||
        memcmp(response.proof, activity_id, 32U) != 0) {
        release_envelope(&response);
        return 1;
    }
    release_envelope(&response);
    return 0;
}

static uint64_t now_ms(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_REALTIME, &now) != 0 || now.tv_sec < 0) return 0U;
    return (uint64_t)now.tv_sec * 1000U + (uint64_t)now.tv_nsec / 1000000U;
}

static int actor_name(const signer *key, const char *suffix, uint8_t id[32])
{
    char name[LX_ACCOUNT_NAME_MAX];
    static const char digits[] = "0123456789abcdef";
    size_t length = 81U + strlen(suffix);
    REQUIRE(length < sizeof(name));
    memcpy(name, "agent:did:layerx:", 17U);
    for (size_t i = 0U; i < 32U; ++i) {
        name[17U + i * 2U] = digits[key->public_key[i] >> 4U];
        name[18U + i * 2U] = digits[key->public_key[i] & 15U];
    }
    memcpy(name + 81U, suffix, strlen(suffix));
    REQUIRE(lx_account_id_from_string((const uint8_t *)name, length, id) == LXP_OK);
    return 0;
}

static int custody_id(const signer *key, const char *module, const uint8_t object[32], uint8_t id[32])
{
    char suffix[80];
    static const char digits[] = "0123456789abcdef";
    size_t cursor = strlen(module) + 2U;
    REQUIRE(cursor + 64U < sizeof(suffix));
    suffix[0] = ':';
    memcpy(suffix + 1U, module, strlen(module));
    suffix[cursor - 1U] = ':';
    for (size_t i = 0U; i < 32U; ++i) {
        suffix[cursor++] = digits[object[i] >> 4U];
        suffix[cursor++] = digits[object[i] & 15U];
    }
    suffix[cursor] = '\0';
    return actor_name(key, suffix, id);
}

static int receipt_wait(int descriptor, const uint8_t id[32], lxp_result expected,
                        lxp_receipt *receipt)
{
    uint8_t query[34] = {1U};
    signer sequencer;
    uint8_t *storage = malloc(2U * LXP_MAX_ACTIVITY_BYTES);
    lxp_arena arena;
    REQUIRE(storage != NULL && signer_init(&sequencer, 0x22U) == 0);
    memcpy(query + 1U, id, 32U);
    query[33U] = 1U;
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 1000U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 1000U);
        if (response.payload_length != 0U) {
            REQUIRE(lxp_arena_init(&arena, storage, 2U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(receipt, sequencer.public_key, &arena) == LXP_OK);
            REQUIRE(memcmp(receipt->activity_id, id, 32U) == 0);
            if (receipt->result_code != expected)
                fprintf(stderr, "module %u result %d expected %d\n", receipt->module_id,
                    receipt->result_code, expected);
            REQUIRE(receipt->result_code == expected);
            release_envelope(&response);
            free(storage);
            return 0;
        }
        release_envelope(&response);
        const struct timespec delay = {0, 50000000};
        REQUIRE(nanosleep(&delay, NULL) == 0);
    }
    free(storage);
    return 1;
}

typedef struct batch_evidence {
    lxp_arena arena;
    uint8_t *storage;
    lxp_batch_body body;
    lxp_byte_span header;
    lxp_sequencer_authorization authorization;
    lxp_byte_span *receipts;
    size_t receipt_count;
    lxp_byte_span *events;
    size_t event_count;
    lxp_batch_maintenance maintenance;
    lxp_merkle_proof activity_receipt_proof;
    lxp_merkle_proof maintenance_proof;
} batch_evidence;

static int batch_fetch(int descriptor, uint64_t batch, const uint8_t activity_id[32], batch_evidence *evidence)
{
    uint8_t query[10] = {0U, 1U};
    wire_envelope response;
    lxp_batch_header header;
    lxp_da_bundle bundle = {0};
    lxp_da_chunk *chunks;
    uint8_t signature[64];
    uint8_t root[32];
    signer sequencer;
    uint8_t sequencer_name[81], sequencer_id[32];
    static const uint8_t digits[] = "0123456789abcdef";
    void *memory;
    memset(evidence, 0, sizeof(*evidence));
    evidence->storage = malloc(4U * LXP_MAX_BATCH_BODY_BYTES);
    REQUIRE(evidence->storage != NULL);
    REQUIRE(lxp_arena_init(&evidence->arena, evidence->storage, 4U * LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    store_u64(query + 2U, batch);
    for (unsigned attempt = 0U;; ++attempt) {
        REQUIRE(attempt < 200U);
        REQUIRE(send_request(descriptor, LNI_MINOR, 12U, 1001U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 13U && response.correlation_id == 1001U);
        if (response.payload_length != 0U) break;
        release_envelope(&response);
        const struct timespec delay = {0, 50000000};
        REQUIRE(nanosleep(&delay, NULL) == 0);
    }
    REQUIRE(response.proof_length == 146U && load_u16(response.proof) == 1U);
    REQUIRE(lxp_batch_header_decode(response.payload, response.payload_length, &header) == LXP_OK);
    REQUIRE(header.protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT && header.batch_number == batch && header.epoch == 1U);
    REQUIRE(signer_init(&sequencer, 0x22U) == 0);
    memcpy(sequencer_name, "layerx-sequencer:", 17U);
    for (size_t i = 0U; i < 32U; ++i) {
        sequencer_name[17U + i * 2U] = digits[sequencer.public_key[i] >> 4U];
        sequencer_name[18U + i * 2U] = digits[sequencer.public_key[i] & 15U];
    }
    REQUIRE(lxp_hash_sha256(sequencer_name, sizeof(sequencer_name), sequencer_id) == LXP_OK);
    REQUIRE(header.network_id == NETWORK_ID);
    REQUIRE(memcmp(response.proof + 2U, sequencer_id, 32U) == 0);
    REQUIRE(memcmp(response.proof + 34U, sequencer.public_key, 32U) == 0);
    REQUIRE(load_u64(response.proof + 66U) == 1U && load_u64(response.proof + 74U) == UINT64_MAX);
    memcpy(evidence->authorization.sequencer_id, response.proof + 2U, 32U);
    memcpy(evidence->authorization.public_key, response.proof + 34U, 32U);
    evidence->authorization.first_batch_number = load_u64(response.proof + 66U);
    evidence->authorization.last_batch_number = load_u64(response.proof + 74U);
    evidence->authorization.authorized = 1U;
    memcpy(signature, response.proof + 82U, 64U);
    REQUIRE(lxp_batch_verify_signature(&header, signature, 64U, &evidence->authorization, &evidence->arena) == LXP_OK);
    REQUIRE(lxp_arena_alloc(&evidence->arena, response.payload_length, 1U, &memory) == LXP_OK);
    memcpy(memory, response.payload, response.payload_length);
    evidence->header = (lxp_byte_span){memory, response.payload_length};
    release_envelope(&response);
    REQUIRE(lxp_arena_alloc(&evidence->arena, LXP_DA_MAX_CHUNKS * sizeof(*chunks), _Alignof(lxp_da_chunk), &memory) == LXP_OK);
    chunks = memory;
    query[0] = 5U;
    store_u64(query + 1U, batch);
    REQUIRE(send_request(descriptor, LNI_MINOR, 18U, 1002U, query, 9U) == 0);
    for (;;) {
        lxp_merkle_proof proof;
        lxp_da_chunk *chunk = &chunks[bundle.chunk_count];
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.correlation_id == 1002U);
        if (response.tag == 20U) {
            REQUIRE(response.payload_length == 0U && response.proof_length == 0U);
            release_envelope(&response);
            break;
        }
        REQUIRE(response.tag == 19U && response.proof_length >= 62U && bundle.chunk_count < LXP_DA_MAX_CHUNKS);
        memset(chunk, 0, sizeof(*chunk));
        memset(&proof, 0, sizeof(proof));
        chunk->batch_number = load_u64(response.proof);
        chunk->chunk_index = load_u32(response.proof + 8U);
        chunk->availability_class = (lxp_da_class)response.proof[12U];
        chunk->class_offset = load_u64(response.proof + 13U);
        chunk->length = (uint32_t)response.payload_length;
        REQUIRE(chunk->batch_number == batch && chunk->chunk_index == bundle.chunk_count);
        proof.leaf_index = load_u32(response.proof + 53U);
        proof.leaf_count = load_u32(response.proof + 57U);
        proof.depth = response.proof[61U];
        REQUIRE(proof.depth <= LXP_MERKLE_MAX_DEPTH && response.proof_length == 62U + (size_t)proof.depth * 32U);
        memcpy(proof.siblings, response.proof + 62U, (size_t)proof.depth * 32U);
        REQUIRE(lxp_arena_alloc(&evidence->arena, response.payload_length, 1U, &memory) == LXP_OK);
        memcpy(memory, response.payload, response.payload_length);
        chunk->bytes = (lxp_byte_span){memory, response.payload_length};
        REQUIRE(lxp_da_chunk_hash(chunk) == LXP_OK && memcmp(chunk->chunk_hash, response.proof + 21U, 32U) == 0);
        REQUIRE(lxp_merkle_proof_verify(chunk->chunk_hash, &proof, header.data_availability_root) == LXP_OK);
        ++bundle.chunk_count;
        bundle.total_bytes += chunk->length;
        release_envelope(&response);
    }
    bundle.chunks = chunks;
    bundle.batch_number = batch;
    REQUIRE(lxp_da_bundle_root(&bundle, &evidence->arena, root) == LXP_OK && memcmp(root, header.data_availability_root, 32U) == 0);
    REQUIRE(lxp_da_bundle_body(&bundle, &header, &evidence->arena, &evidence->body) == LXP_OK);
    memcpy(evidence->body.sequencer_signature, signature, 64U);
    REQUIRE(lxp_da_receipt_section_decode(evidence->body.receipts, &evidence->arena,
        &evidence->receipts, &evidence->receipt_count, &evidence->events, &evidence->event_count) == LXP_OK);
    REQUIRE(evidence->receipt_count == 2U && evidence->event_count == 2U);
    REQUIRE(lxp_batch_maintenance_decode(evidence->receipts[1U].bytes, evidence->receipts[1U].length, &evidence->maintenance) == LXP_OK);
    lxp_byte_span effects;
    REQUIRE(lxp_batch_maintenance_events(evidence->receipts[1U], &header, &effects) == LXP_OK);
    REQUIRE(effects.length == evidence->events[1U].length && memcmp(effects.bytes, evidence->events[1U].bytes, effects.length) == 0);
    uint8_t hashes[2][32];
    for (size_t i = 0U; i < 2U; ++i)
        REQUIRE(lxp_merkle_leaf_hash(evidence->receipts[i].bytes, evidence->receipts[i].length, hashes[i]) == LXP_OK);
    REQUIRE(lxp_merkle_proof_generate((const uint8_t (*)[32])hashes, 2U, 0U, &evidence->arena, &evidence->activity_receipt_proof, root) == LXP_OK && memcmp(root, header.receipt_merkle_root, 32U) == 0);
    REQUIRE(lxp_merkle_proof_generate((const uint8_t (*)[32])hashes, 2U, 1U, &evidence->arena, &evidence->maintenance_proof, root) == LXP_OK && memcmp(root, header.receipt_merkle_root, 32U) == 0);
    for (size_t i = 0U; i < 2U; ++i)
        REQUIRE(lxp_merkle_leaf_hash(evidence->events[i].bytes, evidence->events[i].length, hashes[i]) == LXP_OK);
    REQUIRE(lxp_merkle_build((const uint8_t (*)[32])hashes, 2U, &evidence->arena, root) == LXP_OK && memcmp(root, header.event_merkle_root, 32U) == 0);
    lxp_receipt receipt;
    REQUIRE(lxp_receipt_decode(evidence->receipts[0].bytes, evidence->receipts[0].length, true, &receipt) == LXP_OK);
    REQUIRE(memcmp(receipt.activity_id, activity_id, 32U) == 0 && receipt.global_sequence == header.first_sequence);
    REQUIRE(evidence->maintenance.global_sequence == header.last_sequence && header.last_sequence == header.first_sequence + 1U);
    return 0;
}

static void json_hex(FILE *file, const uint8_t *bytes, size_t length)
{
    for (size_t i = 0U; i < length; ++i) fprintf(file, "%02x", bytes[i]);
}

static void json_proof(FILE *file, const lxp_merkle_proof *proof)
{
    fprintf(file, "{\"leaf_index\":%u,\"leaf_count\":%u,\"siblings\":[", proof->leaf_index, proof->leaf_count);
    for (size_t i = 0U; i < proof->depth; ++i) {
        fprintf(file, "%s\"", i == 0U ? "" : ",");
        json_hex(file, proof->siblings[i], 32U);
        fputc('"', file);
    }
    fputs("]}", file);
}

static int fixture_write(const char *directory, const char *name, const batch_evidence *evidence)
{
    char path[4096];
    int length = snprintf(path, sizeof(path), "%s/%s.json", directory, name);
    REQUIRE(length > 0 && (size_t)length < sizeof(path));
    FILE *file = fopen(path, "wx");
    REQUIRE(file != NULL);
    fputs("{\"signed_header_hex\":\"", file); json_hex(file, evidence->header.bytes, evidence->header.length);
    fputs("\",\"sequencer_signature_hex\":\"", file); json_hex(file, evidence->body.sequencer_signature, 64U);
    fputs("\",\"sequencer_public_key_hex\":\"", file); json_hex(file, evidence->authorization.public_key, 32U);
    fputs("\",\"sequencer_id_hex\":\"", file); json_hex(file, evidence->authorization.sequencer_id, 32U);
    fputs("\",\"activity_receipt_hex\":\"", file); json_hex(file, evidence->receipts[0].bytes, evidence->receipts[0].length);
    fputs("\",\"activity_proof\":", file); json_proof(file, &evidence->activity_receipt_proof);
    fputs(",\"maintenance_hex\":\"", file); json_hex(file, evidence->receipts[1].bytes, evidence->receipts[1].length);
    fputs("\",\"maintenance_proof\":", file); json_proof(file, &evidence->maintenance_proof);
    fputs("}\n", file);
    REQUIRE(!ferror(file) && fclose(file) == 0);
    return 0;
}

enum { MAX_STEPS = 32 };
typedef struct saved_step {
    uint8_t activity_id[32];
    uint8_t header_hash[32];
    uint8_t maintenance_hash[32];
    uint64_t batch;
    int32_t result;
} saved_step;
typedef struct scenario_state {
    uint64_t owner_sequence;
    uint64_t provider_sequence;
    uint64_t deadline;
    uint64_t step_count;
    uint8_t artifact_hash[32];
    uint8_t artifact_reference[32];
    uint64_t artifact_size;
    saved_step steps[MAX_STEPS];
} scenario_state;

static const uint8_t asset_id[32] = {
    0xb5,0xa3,0x2b,0x12,0x02,0x9f,0x8d,0xdf,0xb9,0x05,0xf9,0x0f,0x28,0x0f,0x66,0x4b,
    0x46,0x39,0x0d,0xe0,0xfc,0x62,0x77,0x0f,0xc1,0x97,0xdd,0x87,0xb1,0x8c,0xd8,0x98
};
static const uint8_t escrow_id[32] = {0x81U};
static const uint8_t budget_id[32] = {0x82U};
static const uint8_t stream_id[32] = {0x83U};
static const uint8_t market_id[32] = {0x84U};
static const uint8_t offer_id[32] = {0x85U};
static const uint8_t agreement_id[32] = {0x86U};
static const uint8_t commitment_id[32] = {0x87U};
static const uint8_t delivery_id[32] = {0x88U};
static const uint8_t terms_hash[32] = {0x89U};

static int evidence_record(scenario_state *state, const batch_evidence *evidence,
                           const uint8_t id[32], lxp_result result)
{
    REQUIRE(state->step_count < MAX_STEPS);
    saved_step *step = &state->steps[state->step_count++];
    memcpy(step->activity_id, id, 32U);
    step->batch = evidence->body.header.batch_number;
    step->result = result;
    REQUIRE(lxp_hash_sha256(evidence->header.bytes, evidence->header.length, step->header_hash) == LXP_OK);
    REQUIRE(lxp_hash_sha256(evidence->receipts[1].bytes, evidence->receipts[1].length, step->maintenance_hash) == LXP_OK);
    return 0;
}

static int submit_encoded(int descriptor, scenario_state *state, const uint8_t *encoded,
                          size_t length, lxp_result expected, batch_evidence *evidence)
{
    uint8_t id[32];
    lxp_receipt receipt;
    REQUIRE(lxp_activity_id(encoded, length, id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 2000U, encoded, length) == 0);
    REQUIRE(expect_ack(descriptor, 2000U, encoded, length, id) == 0);
    REQUIRE(receipt_wait(descriptor, id, expected, &receipt) == 0);
    REQUIRE(receipt.global_sequence == state->step_count * 2U + 1U);
    REQUIRE(batch_fetch(descriptor, state->step_count + 1U, id, evidence) == 0);
    REQUIRE(evidence_record(state, evidence, id, expected) == 0);
    return 0;
}

static int submit(int descriptor, scenario_state *state, const signer *key,
                   uint64_t *sequence, uint32_t type, const uint8_t *payload,
                   size_t length, lxp_result expected, batch_evidence *evidence)
{
    uint8_t encoded[ACTIVITY_CAPACITY];
    size_t encoded_length;
    REQUIRE(build_activity(key, *sequence, type, 0U, payload, length,
        encoded, sizeof(encoded), &encoded_length) == 0);
    REQUIRE(submit_encoded(descriptor, state, encoded, encoded_length, expected, evidence) == 0);
    ++*sequence;
    return 0;
}

static int submit_release(int descriptor, scenario_state *state, const signer *key,
                           uint64_t *sequence, uint32_t type, const uint8_t *payload,
                           size_t length)
{
    batch_evidence evidence;
    REQUIRE(submit(descriptor, state, key, sequence, type, payload, length, LXP_OK, &evidence) == 0);
    free(evidence.storage);
    return 0;
}

static int market_account_id(const char *prefix, const char *suffix, uint8_t id[32])
{
    char name[LX_ACCOUNT_NAME_MAX];
    static const char digits[] = "0123456789abcdef";
    size_t cursor = strlen(prefix);
    memcpy(name, prefix, cursor);
    for (size_t i = 0U; i < 32U; ++i) {
        name[cursor++] = digits[market_id[i] >> 4U];
        name[cursor++] = digits[market_id[i] & 15U];
    }
    memcpy(name + cursor, suffix, strlen(suffix));
    cursor += strlen(suffix);
    REQUIRE(lx_account_id_from_string((const uint8_t *)name, cursor, id) == LXP_OK);
    return 0;
}

static int market_open(int descriptor, scenario_state *state, const signer *owner)
{
    lx_perps_market market = {0};
    uint8_t payload[LX_PERPS_MARKET_BYTES];
    uint8_t did[75];
    static const char digits[] = "0123456789abcdef";
    memcpy(did, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        did[11U + i * 2U] = (uint8_t)digits[owner->public_key[i] >> 4U];
        did[12U + i * 2U] = (uint8_t)digits[owner->public_key[i] & 15U];
    }
    memcpy(market.market_id, market_id, 32U);
    memcpy(market.quote_asset, asset_id, 32U);
    REQUIRE(lxp_did_id_derive(did, sizeof(did), market.administrator) == LXP_OK);
    REQUIRE(market_account_id("system:liquidity:", "", market.liquidity_account_id) == 0);
    REQUIRE(market_account_id("system:funding:", ":long", market.long_funding_account_id) == 0);
    REQUIRE(market_account_id("system:funding:", ":short", market.short_funding_account_id) == 0);
    REQUIRE(lx_account_id_from_string((const uint8_t *)"system:insurance", 16U, market.insurance_account_id) == LXP_OK);
    market.contract_size = market.tick_size = market.lot_size = market.price_scale = (lxp_u128){0U, 1U};
    market.initial_margin_ratio_bps = 1000U;
    market.maintenance_margin_ratio_bps = 500U;
    market.liquidation_fee_bps = 10U;
    market.liquidator_share_bps = 6000U;
    market.maximum_funding_rate_bps = 1000U;
    market.maximum_deviation_basis_points = 10000U;
    market.funding_interval_ms = 1000U;
    market.maximum_oracle_staleness_ms = 100000U;
    market.minimum_price = (lxp_u128){0U, 1U};
    market.maximum_price = (lxp_u128){0U, 1000000U};
    market.permitted_oracle_key_count = 1U;
    memcpy(market.permitted_oracle_keys[0], owner->public_key, 32U);
    market.parameter_version = 1U;
    REQUIRE(lx_perps_market_encode(&market, payload) == LXP_OK);
    return submit_release(descriptor, state, owner, &state->owner_sequence,
        LX_PERPS_MARKET_CREATE, payload, sizeof(payload));
}

static int maintenance_effects_check(const batch_evidence *evidence, bool expect_due)
{
    lxp_byte_span effects = evidence->maintenance.effects;
    size_t cursor = 35U;
    bool escrow = false, budget = false, service = false, transfer = false;
    REQUIRE(lxp_batch_maintenance_effects_validate(effects) == LXP_OK && effects.length >= cursor);
    uint16_t count = load_u16(effects.bytes + 33U);
    for (size_t frame = 0U; frame < count; ++frame) {
        uint16_t module = load_u16(effects.bytes + cursor);
        uint16_t effect_count = load_u16(effects.bytes + cursor + 6U);
        cursor += 8U;
        for (size_t i = 0U; i < effect_count; ++i) {
            uint16_t event = load_u16(effects.bytes + cursor + 2U);
            uint8_t kind = effects.bytes[cursor + 4U];
            uint16_t length = load_u16(effects.bytes + cursor + 38U);
            const uint8_t *body = effects.bytes + cursor + 40U;
            if (module == LXP_MODULE_ESCROW && kind == LXP_EFFECT_TRANSFER) transfer = true;
            if (module == LXP_MODULE_ESCROW && kind == LXP_EFFECT_EVENT && event == 5U) {
                REQUIRE(length == LX_ESCROW_EVENT_BYTES && memcmp(body, escrow_id, 32U) == 0);
                REQUIRE(load_u16(body + 32U) == 5U && body[34U] == LX_ESCROW_STATE_TIMED_OUT);
                REQUIRE(lxp_ct_is_zero(body + 51U, 16U));
                escrow = true;
            }
            if (module == LXP_MODULE_BUDGET && kind == LXP_EFFECT_STATE) budget = true;
            if (module == LXP_MODULE_SERVICE && kind == LXP_EFFECT_EVENT && event == LX_SERVICE_EVENT_DEFAULT_APPLIED) {
                REQUIRE(length == 50U && memcmp(body, agreement_id, 32U) == 0);
                REQUIRE(body[32U] == LX_SERVICE_AGREEMENT_ACCEPTED && body[33U] == 1U);
                REQUIRE(load_u64(body + 34U) == evidence->body.header.last_sequence && load_u64(body + 42U) == evidence->body.header.timestamp_ms);
                service = true;
            }
            cursor += 40U + length;
        }
    }
    REQUIRE(cursor == effects.length);
    if (expect_due) REQUIRE(escrow && budget && service && transfer);
    else REQUIRE(count == 0U);
    return 0;
}

static int maintenance_refusals(const batch_evidence *evidence)
{
    lxp_byte_span source = evidence->receipts[1U];
    uint8_t *bytes = malloc(source.length + 1U);
    lxp_batch_maintenance decoded;
    lxp_byte_span events;
    lxp_batch_header header = evidence->body.header;
    REQUIRE(bytes != NULL);
    memcpy(bytes, source.bytes, source.length);
    REQUIRE(lxp_batch_maintenance_decode(bytes, source.length - 1U, &decoded) != LXP_OK);
    bytes[source.length] = 0U;
    REQUIRE(lxp_batch_maintenance_decode(bytes, source.length + 1U, &decoded) != LXP_OK);
    bytes[26U] ^= 1U;
    REQUIRE(lxp_batch_maintenance_decode(bytes, source.length, &decoded) == LXP_ERR_VERSION_UNSUPPORTED);
    memcpy(bytes, source.bytes, source.length);
    ++header.epoch;
    REQUIRE(lxp_batch_maintenance_events(source, &header, &events) != LXP_OK);
    header = evidence->body.header;
    ++header.timestamp_ms;
    REQUIRE(lxp_batch_maintenance_events(source, &header, &events) != LXP_OK);
    lxp_programs_occupancy_receipt occupancy;
    REQUIRE(lxp_programs_occupancy_receipt_decode(source.bytes, source.length, &occupancy) != LXP_OK);
    size_t effects_offset = (size_t)(evidence->maintenance.effects.bytes - source.bytes);
    bytes[effects_offset + 33U] = 0xffU;
    REQUIRE(lxp_batch_maintenance_decode(bytes, source.length, &decoded) != LXP_OK);
    memcpy(bytes, source.bytes, source.length);
    bytes[source.length - 1U] ^= 1U;
    uint8_t leaf[32];
    REQUIRE(lxp_merkle_leaf_hash(bytes, source.length, leaf) == LXP_OK);
    REQUIRE(lxp_merkle_proof_verify(leaf, &evidence->maintenance_proof, evidence->body.header.receipt_merkle_root) != LXP_OK);
    free(bytes);
    return 0;
}

static int reconnect(int descriptor)
{
    struct sockaddr_un address = {0};
    socklen_t length = sizeof(address);
    REQUIRE(getpeername(descriptor, (struct sockaddr *)&address, &length) == 0);
    REQUIRE(address.sun_family == AF_UNIX && length <= sizeof(address));
    REQUIRE(close(descriptor) == 0);
    int connected = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(connected >= 0 && connect(connected, (struct sockaddr *)&address, length) == 0);
    if (connected != descriptor) {
        REQUIRE(dup2(connected, descriptor) == descriptor);
        REQUIRE(close(connected) == 0);
    }
    REQUIRE(handshake(descriptor) == 0);
    return 0;
}

static int scenario_start(int descriptor, const char *directory, scenario_state *state,
                           const signer *owner, const signer *provider)
{
    uint8_t encoded[ACTIVITY_CAPACITY];
    uint8_t payload[LX_PERPS_MARKET_BYTES] = {0};
    uint8_t owner_main[32], provider_main[32], custody[32];
    size_t length;
    batch_evidence evidence;
    const char *credit = getenv("LAYERX_TEST_WITHDRAW_CREDIT");
    REQUIRE(credit != NULL);
    FILE *file = fopen(credit, "rb");
    REQUIRE(file != NULL);
    length = fread(encoded, 1U, sizeof(encoded), file);
    REQUIRE(length > 0U && length < sizeof(encoded) && !ferror(file) && fclose(file) == 0);
    REQUIRE(submit_encoded(descriptor, state, encoded, length, LXP_OK, &evidence) == 0);
    state->owner_sequence = 1U;
    REQUIRE(maintenance_effects_check(&evidence, false) == 0);
    REQUIRE(maintenance_refusals(&evidence) == 0);
    REQUIRE(fixture_write(directory, "empty-effects", &evidence) == 0);
    REQUIRE(lxp_hash_sha256(evidence.body.receipts.bytes, evidence.body.receipts.length, state->artifact_hash) == LXP_OK);
    memcpy(state->artifact_reference, evidence.body.header.data_availability_root, 32U);
    state->artifact_size = evidence.body.receipts.length;
    free(evidence.storage);
    REQUIRE(actor_name(owner, ":main", owner_main) == 0 && actor_name(provider, ":main", provider_main) == 0);
    state->deadline = now_ms() + 30000U;
    REQUIRE(state->deadline > 30000U);
    memcpy(payload, escrow_id, 32U);
    memcpy(payload + 32U, owner_main, 32U);
    REQUIRE(custody_id(owner, "escrow", escrow_id, custody) == 0);
    memcpy(payload + 64U, custody, 32U);
    memcpy(payload + 96U, provider_main, 32U);
    memcpy(payload + 128U, owner_main, 32U);
    memcpy(payload + 160U, asset_id, 32U);
    store_u64(payload + 200U, 100U);
    store_u64(payload + 208U, state->deadline);
    store_u64(payload + 216U, 1000U);
    memcpy(payload + 224U, terms_hash, 32U);
    memcpy(payload + 256U, agreement_id, 32U);
    REQUIRE(build_activity(owner, state->owner_sequence, LX_ESCROW_OPEN, 0U, payload,
        LX_ESCROW_OPEN_PAYLOAD_BYTES, encoded, sizeof(encoded), &length) == 0);
    lxp_activity decoded;
    REQUIRE(lxp_activity_decode(encoded, length, &decoded) == LXP_OK);
    size_t signature_offset = (size_t)(decoded.signature.bytes - encoded);
    encoded[signature_offset] ^= 1U;
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 2999U, encoded, length) == 0);
    REQUIRE(expect_error(descriptor, 2999U, 6U, LXP_ERR_BAD_SIGNATURE) == 0);
    payload[64U] ^= 1U;
    REQUIRE(submit(descriptor, state, owner, &state->owner_sequence, LX_ESCROW_OPEN,
        payload, LX_ESCROW_OPEN_PAYLOAD_BYTES, LXP_ERR_ACCOUNT_ID_MISMATCH, &evidence) == 0);
    REQUIRE(maintenance_effects_check(&evidence, false) == 0);
    free(evidence.storage);
    payload[64U] ^= 1U;
    store_u64(payload + 200U, 1000001U);
    REQUIRE(submit(descriptor, state, owner, &state->owner_sequence, LX_ESCROW_OPEN,
        payload, LX_ESCROW_OPEN_PAYLOAD_BYTES, LXP_ERR_INSUFFICIENT_BALANCE, &evidence) == 0);
    REQUIRE(maintenance_effects_check(&evidence, false) == 0);
    free(evidence.storage);
    store_u64(payload + 200U, 100U);
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_ESCROW_OPEN,
        payload, LX_ESCROW_OPEN_PAYLOAD_BYTES) == 0);
    memset(payload, 0, sizeof(payload));
    store_u16(payload, 1U);
    memcpy(payload + 2U, budget_id, 32U);
    REQUIRE(custody_id(owner, "budget", budget_id, custody) == 0);
    memcpy(payload + 34U, custody, 32U);
    memcpy(payload + 66U, asset_id, 32U);
    memcpy(payload + 98U, terms_hash, 32U);
    store_u64(payload + 138U, 10U);
    store_u64(payload + 170U, 20U);
    store_u64(payload + 178U, 60000U);
    store_u64(payload + 186U, state->deadline - 60000U);
    store_u64(payload + 194U, state->deadline + 300000U);
    payload[210U] = LX_BUDGET_ROLLOVER_NONE;
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_BUDGET_CREATE,
        payload, LX_BUDGET_CREATE_PAYLOAD_BYTES) == 0);
    memset(payload, 0, sizeof(payload));
    store_u16(payload, 1U);
    memcpy(payload + 2U, budget_id, 32U);
    memcpy(payload + 34U, owner_main, 32U);
    store_u64(payload + 74U, 5U);
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_BUDGET_SPEND,
        payload, LX_BUDGET_SPEND_PAYLOAD_BYTES) == 0);
    lx_stream_open_payload stream = {0};
    memcpy(stream.record.stream_id, stream_id, 32U);
    REQUIRE(custody_id(owner, "stream", stream_id, stream.record.stream_account) == 0);
    memcpy(stream.record.recipient, owner_main, 32U);
    memcpy(stream.record.asset_id, asset_id, 32U);
    stream.record.mode = LX_STREAM_MODE_TIME;
    stream.record.rate = (lxp_u128){0U, 1U};
    stream.record.rate_unit = 1000U;
    stream.record.start_timestamp = now_ms();
    stream.record.end_timestamp = state->deadline + 300000U;
    stream.record.total_cap = stream.initial_funding = (lxp_u128){0U, 100U};
    REQUIRE(lx_stream_open_encode(&stream, payload, sizeof(payload), &length) == LXP_OK);
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_STREAM_OPEN, payload, length) == 0);
    REQUIRE(market_open(descriptor, state, owner) == 0);
    memset(payload, 0, sizeof(payload));
    store_u16(payload, 1U);
    memcpy(payload + 2U, offer_id, 32U);
    memcpy(payload + 34U, asset_id, 32U);
    store_u64(payload + 74U, 100U);
    memcpy(payload + 82U, terms_hash, 32U);
    memcpy(payload + 114U, state->artifact_hash, 32U);
    store_u64(payload + 146U, state->deadline - 1000U);
    store_u64(payload + 154U, 1000U);
    store_u64(payload + 162U, 60000U);
    payload[170U] = LX_SERVICE_DEFAULT_ACCEPT;
    store_u64(payload + 171U, state->deadline + 60000U);
    REQUIRE(submit_release(descriptor, state, provider, &state->provider_sequence, LX_SERVICE_OFFER_PUBLISH,
        payload, LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES) == 0);
    memset(payload, 0, sizeof(payload)); store_u16(payload, 1U);
    memcpy(payload + 2U, agreement_id, 32U); memcpy(payload + 34U, offer_id, 32U);
    memcpy(payload + 66U, terms_hash, 32U); memcpy(payload + 98U, escrow_id, 32U);
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_SERVICE_AGREEMENT_PROPOSE,
        payload, LX_SERVICE_AGREEMENT_PROPOSE_PAYLOAD_BYTES) == 0);
    REQUIRE(submit_release(descriptor, state, provider, &state->provider_sequence, LX_SERVICE_AGREEMENT_ACCEPT,
        payload, LX_SERVICE_IDENTIFIER_PAYLOAD_BYTES) == 0);
    memset(payload, 0, sizeof(payload)); store_u16(payload, 1U);
    memcpy(payload + 2U, commitment_id, 32U); memcpy(payload + 34U, agreement_id, 32U);
    memcpy(payload + 66U, terms_hash, 32U); memcpy(payload + 98U, escrow_id, 32U);
    store_u64(payload + 130U, state->deadline - 1000U); store_u64(payload + 138U, 100U);
    REQUIRE(submit_release(descriptor, state, provider, &state->provider_sequence, LX_SERVICE_COMMIT_TASK,
        payload, LX_SERVICE_COMMIT_TASK_PAYLOAD_BYTES) == 0);
    memset(payload, 0, sizeof(payload)); store_u16(payload, 1U);
    memcpy(payload + 2U, delivery_id, 32U); memcpy(payload + 34U, agreement_id, 32U);
    payload[66U] = 1U; memcpy(payload + 67U, state->artifact_hash, 32U);
    store_u64(payload + 99U, state->artifact_size); memcpy(payload + 107U, state->artifact_reference, 32U);
    REQUIRE(submit_release(descriptor, state, provider, &state->provider_sequence, LX_SERVICE_DELIVER,
        payload, LX_SERVICE_DELIVER_PAYLOAD_FIXED_BYTES + LX_SERVICE_DELIVER_PAYLOAD_ITEM_BYTES) == 0);
    REQUIRE(now_ms() < state->deadline);
    while (now_ms() <= state->deadline) {
        const struct timespec delay = {0, 10000000};
        REQUIRE(nanosleep(&delay, NULL) == 0);
    }
    REQUIRE(reconnect(descriptor) == 0);
    memcpy(payload, market_id, 32U); payload[32U] = 1U;
    REQUIRE(submit(descriptor, state, owner, &state->owner_sequence, LX_PERPS_MARKET_HALT,
        payload, LX_PERPS_HALT_PAYLOAD_BYTES, LXP_OK, &evidence) == 0);
    REQUIRE(maintenance_effects_check(&evidence, true) == 0);
    REQUIRE(maintenance_refusals(&evidence) == 0);
    REQUIRE(fixture_write(directory, "deadline-effects", &evidence) == 0);
    free(evidence.storage);
    return 0;
}

static int scenario_recovered(int descriptor, const char *directory, scenario_state *state,
                               const signer *owner)
{
    uint8_t digest[32];
    uint8_t payload[LX_BUDGET_SPEND_PAYLOAD_BYTES] = {0};
    batch_evidence evidence;
    for (size_t i = 0U; i < state->step_count; ++i) {
        lxp_receipt receipt;
        const saved_step *step = &state->steps[i];
        REQUIRE(receipt_wait(descriptor, step->activity_id, (lxp_result)step->result, &receipt) == 0);
        REQUIRE(batch_fetch(descriptor, step->batch, step->activity_id, &evidence) == 0);
        REQUIRE(lxp_hash_sha256(evidence.header.bytes, evidence.header.length, digest) == LXP_OK && memcmp(digest, step->header_hash, 32U) == 0);
        REQUIRE(lxp_hash_sha256(evidence.receipts[1].bytes, evidence.receipts[1].length, digest) == LXP_OK && memcmp(digest, step->maintenance_hash, 32U) == 0);
        if (i + 1U == state->step_count) REQUIRE(maintenance_effects_check(&evidence, true) == 0);
        free(evidence.storage);
    }
    store_u16(payload, 1U); memcpy(payload + 2U, budget_id, 32U);
    REQUIRE(actor_name(owner, ":main", payload + 34U) == 0);
    store_u64(payload + 74U, 8U);
    REQUIRE(submit(descriptor, state, owner, &state->owner_sequence, LX_BUDGET_SPEND,
        payload, sizeof(payload), LXP_OK, &evidence) == 0);
    REQUIRE(maintenance_effects_check(&evidence, false) == 0);
    REQUIRE(fixture_write(directory, "recovered-empty-effects", &evidence) == 0);
    free(evidence.storage);
    memset(payload, 0, sizeof(payload)); store_u16(payload, 1U); memcpy(payload + 2U, stream_id, 32U);
    payload[34U] = 0x91U;
    REQUIRE(submit_release(descriptor, state, owner, &state->owner_sequence, LX_STREAM_CLOSE,
        payload, LX_STREAM_KEYED_PAYLOAD_BYTES) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    signer owner, provider;
    struct sockaddr_un address = {0};
    scenario_state state = {0};
    char path[4096];
    REQUIRE(argc == 4 && strlen(argv[1]) < sizeof(address.sun_path));
    bool recovered = strcmp(argv[2], "--module-maintenance-recovered") == 0;
    REQUIRE(recovered || strcmp(argv[2], "--module-maintenance") == 0);
    REQUIRE(signer_init(&owner, 0x11U) == 0 && signer_init(&provider, 0x33U) == 0);
    int length = snprintf(path, sizeof(path), "%s/scenario.bin", argv[3]);
    REQUIRE(length > 0 && (size_t)length < sizeof(path));
    if (recovered) {
        FILE *file = fopen(path, "rb");
        REQUIRE(file != NULL && fread(&state, sizeof(state), 1U, file) == 1U);
        REQUIRE(fgetc(file) == EOF && !ferror(file) && fclose(file) == 0 && state.step_count <= MAX_STEPS);
    }
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    REQUIRE(handshake(descriptor) == 0);
    if (recovered) REQUIRE(scenario_recovered(descriptor, argv[3], &state, &owner) == 0);
    else {
        REQUIRE(scenario_start(descriptor, argv[3], &state, &owner, &provider) == 0);
        FILE *file = fopen(path, "wx");
        REQUIRE(file != NULL && fwrite(&state, sizeof(state), 1U, file) == 1U && fclose(file) == 0);
    }
    REQUIRE(close(descriptor) == 0);
    puts(recovered ? "module maintenance proofs and states survive daemon and guarantor restart" :
        "five enabled modules execute signed calls; deadline effects bind the signed receipt and event roots");
    return 0;
}
