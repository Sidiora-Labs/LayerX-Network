#define _GNU_SOURCE

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_storage.h"

#include "layerx/programs.h"
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
            (void)fprintf(stderr, "test_daemon_lni_admission:%d: %s\n", \
                          __LINE__, #condition); \
            return 1; \
        } \
    } while (0)

enum {
    NETWORK_ID = 77,
    LNI_MAJOR = 1,
    LNI_MINOR = 4,
    NODE_INFO_REQUEST = 1,
    NODE_INFO_RESPONSE = 2,
    SUBMIT_REQUEST = 3,
    SUBMIT_RESPONSE = 4,
    ERROR_RESPONSE = 25,
    ENVELOPE_FIXED_BYTES = 22,
    JOURNAL_SUPERBLOCK_BYTES = 32,
    JOURNAL_RECORD_BYTES = 64,
    ACTIVITY_CAPACITY = 4096,
    OWNER_SCRATCH_BYTES = 2 * 1024 * 1024,
    INVALID_FLOOD = LXP_DAEMON_QUEUE_CAPACITY + 16,
    WAIT_POLLS = 12000,
    IO_DEADLINE_MILLISECONDS = 10000
};

static uint8_t REGISTERED_DID[76];
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
    uint8_t preimage[32];
    uint8_t signature[64];
    size_t index;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.network_id = NETWORK_ID;
    activity.activity_type = activity_type;
    activity.actor_did = (lxp_byte_span){
        REGISTERED_DID, sizeof(REGISTERED_DID) - 1U};
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


typedef lxp_result (*simulate_function)(lxp_daemon_protocol_owner *,
    const uint8_t *, const uint8_t *, size_t, uint8_t *, size_t, size_t *,
    uint8_t *, size_t, size_t *);
_Static_assert(_Generic(&lxp_daemon_lni_simulate,
                        simulate_function: 1, default: 0),
               "simulation must remain available through the public header");

static int simulate_call(int descriptor, const signer *key)
{
    static const uint8_t access[] = "LayerX/programs/access-declaration/v1\0";
    static const uint8_t domain[] = "LayerX/agent/program-simulation-evidence/v1";
    static const uint64_t budgets[] = {
        1000000U, 16777216U, 1048576U, 1048576U, 64U, 1048576U, 4096U
    };
    uint8_t payload[118U + sizeof(access)] = {1U};
    uint8_t encoded[ACTIVITY_CAPACITY], activity_id[32], query[33] = {1U};
    uint8_t digest_input[sizeof(domain) + 145U], digest[32];
    wire_envelope response;
    lxp_receipt receipt;
    signer sequencer;
    size_t length;
    uint32_t receipt_length;
    uint64_t timestamp;
    uint8_t preparation[79];
    store_u16(preparation, 1U);
    store_u16(preparation + 2U, 75U);
    memcpy(preparation + 4U, REGISTERED_DID, 75U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 19U, preparation, sizeof(preparation)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 27U && response.payload_length >= 99U);
    timestamp = load_u64(response.payload + 91U);
    REQUIRE(timestamp != 0U);
    release_envelope(&response);
    store_u16(payload + 32U, LX_PROGRAMS_ABI_VERSION);
    store_u16(payload + 34U, 10U);
    store_u16(payload + 40U, 2U);
    store_u32(payload + 42U, (uint32_t)sizeof(access));
    store_u32(payload + 46U, 16U);
    for (size_t i = 0U; i < 7U; ++i)
        store_u64(payload + 50U + i * 8U, budgets[i]);
    memcpy(payload + 106U, "layerx_call", 10U);
    memcpy(payload + 118U, access, sizeof(access));
    REQUIRE(build_activity(key, 0U, LX_PROGRAMS_CALL, timestamp, payload, sizeof(payload),
                           encoded, sizeof(encoded), &length) == 0);
    REQUIRE(lxp_activity_id(encoded, length, activity_id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, 30U, 20U, encoded, length) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    if (response.tag == ERROR_RESPONSE && response.payload_length == 5U)
        fprintf(stderr, "simulation refusal class=%u result=%d\n",
                response.payload[0], (int32_t)load_u32(response.payload + 1U));
    REQUIRE(response.tag == 31U && response.correlation_id == 20U);
    REQUIRE(response.payload_length >= 46U && response.proof_length == 242U);
    REQUIRE(load_u16(response.payload) == 1U && load_u16(response.proof) == 1U);
    REQUIRE(memcmp(response.payload + 2U, activity_id, 32U) == 0);
    REQUIRE(memcmp(response.proof + 34U, activity_id, 32U) == 0);
    REQUIRE(load_u64(response.proof + 130U) == 0U);
    receipt_length = load_u32(response.payload + 34U);
    REQUIRE(receipt_length <= response.payload_length - 46U);
    REQUIRE(lxp_receipt_decode(response.payload + 38U, receipt_length,
                               true, &receipt) == LXP_OK);
    REQUIRE(receipt.global_sequence == 1U);
    REQUIRE(memcmp(receipt.activity_id, activity_id, 32U) == 0);
    REQUIRE(signer_init(&sequencer, 0x22U) == 0);
    REQUIRE(memcmp(response.proof + 146U, sequencer.public_key, 32U) == 0);
    memcpy(digest_input, domain, sizeof(domain));
    memcpy(digest_input + sizeof(domain), response.proof + 2U, 144U);
    digest_input[sizeof(digest_input) - 1U] = 0U;
    REQUIRE(lxp_hash_sha256(digest_input, sizeof(digest_input), digest) == LXP_OK);
    REQUIRE(lxp_ed25519_verify_raw(sequencer.public_key, response.proof + 178U,
                                   digest, sizeof(digest)) == LXP_OK);
    release_envelope(&response);
    memcpy(query + 1U, activity_id, 32U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 21U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 6U && response.correlation_id == 21U &&
            response.payload_length == 0U);
    release_envelope(&response);
    puts("simulation route returned signed evidence without publishing a receipt");
    return 0;
}


int main(int argc, char **argv)
{
    signer key;
    struct sockaddr_un address = {0};
    uint8_t malformed[32] = {1U};
    uint8_t deploy[112] = {1U};
    uint8_t encoded[ACTIVITY_CAPACITY], activity_id[32], query[33] = {1U};
    size_t length;
    int descriptor;
    static const char digits[] = "0123456789abcdef";
    static const uint32_t types[] = {LX_PROGRAMS_CALL, LX_PROGRAMS_DEPLOY, LX_PROGRAMS_UPGRADE};
    REQUIRE((argc == 2 || argc == 3) && strlen(argv[1]) < sizeof(address.sun_path));
    REQUIRE(signer_init(&key, 0x11U) == 0);
    memcpy(REGISTERED_DID, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        REGISTERED_DID[11U + i * 2U] = (uint8_t)digits[key.public_key[i] >> 4U];
        REGISTERED_DID[12U + i * 2U] = (uint8_t)digits[key.public_key[i] & 15U];
    }
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    REQUIRE(handshake(descriptor) == 0);
    if (argc == 3) REQUIRE(simulate_call(descriptor, &key) == 0);
    for (size_t i = 0U; i < sizeof(types) / sizeof(types[0]); ++i) {
        REQUIRE(build_activity(&key, 0U, types[i], 0U, malformed, sizeof(malformed), encoded,
                               sizeof(encoded), &length) == 0);
        REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, i + 1U, encoded, length) == 0);
        REQUIRE(expect_error(descriptor, i + 1U, 4U, LXP_ERR_TRUNCATED) == 0);
    }
    store_u16(deploy + 32U, 1U);
    store_u32(deploy + 100U, 8U);
    memcpy(deploy + 104U, "\0asm\1\0\0\0", 8U);
    REQUIRE(lxp_hash_sha256(deploy + 104U, 8U, deploy + 68U) == LXP_OK);
    REQUIRE(build_activity(&key, 0U, LX_PROGRAMS_DEPLOY, 0U, deploy, sizeof(deploy), encoded,
                           sizeof(encoded), &length) == 0);
    REQUIRE(lxp_activity_id(encoded, length, activity_id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 4U, encoded, length) == 0);
    REQUIRE(expect_ack(descriptor, 4U, encoded, length, activity_id) == 0);
    memcpy(query + 1U, activity_id, 32U);
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 5U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 5U);
        if (response.payload_length != 0U) {
            lxp_receipt receipt;
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, &receipt) == LXP_OK);
            REQUIRE(receipt.global_sequence == 1U && memcmp(receipt.activity_id, activity_id, 32U) == 0);
            release_envelope(&response);
            REQUIRE(close(descriptor) == 0);
            descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
            REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
            REQUIRE(handshake(descriptor) == 0);
            REQUIRE(close(descriptor) == 0);
            puts("malformed CALL/DEPLOY/UPGRADE refused; canonical deployment acknowledged and receipted at sequence 1");
            return 0;
        }
        release_envelope(&response);
        { const struct timespec delay = {0, 50000000}; REQUIRE(nanosleep(&delay, NULL) == 0); }
    }
    return 1;
}
