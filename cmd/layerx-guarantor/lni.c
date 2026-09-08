#define _POSIX_C_SOURCE 200809L
#include "lni.h"
#include "layerx/lxp_crypto.h"
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

static uint64_t get(const uint8_t *p, size_t n)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < n; ++i)
        value = (value << 8U) | p[i];
    return value;
}
static void put(uint8_t *p, uint64_t value, size_t n)
{
    for (size_t i = 0U; i < n; ++i)
        p[n - i - 1U] = (uint8_t)(value >> (8U * i));
}
static int64_t now(void)
{
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC, &t) != 0)
        return -1;
    return (int64_t)t.tv_sec * 1000 + t.tv_nsec / 1000000;
}
static lxp_result transfer(int fd, uint8_t *p, size_t n, bool writing, int64_t deadline)
{
    while (n != 0U) {
        struct pollfd poller = {fd, writing ? POLLOUT : POLLIN, 0};
        int64_t remaining = deadline - now();
        ssize_t amount;
        int ready;
        if (remaining <= 0 || remaining > INT32_MAX)
            return LXP_ERR_IO;
        ready = poll(&poller, 1U, (int)remaining);
        if (ready < 0 && errno == EINTR)
            continue;
        if (ready <= 0)
            return LXP_ERR_IO;
        amount = writing ? send(fd, p, n, MSG_NOSIGNAL) : recv(fd, p, n, 0);
        if (amount < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK))
            continue;
        if (amount <= 0)
            return LXP_ERR_IO;
        p += (size_t)amount;
        n -= (size_t)amount;
    }
    return LXP_OK;
}
static lxp_result handshake(lxp_guarantor_lni *);
lxp_result lxp_guarantor_lni_open(lxp_guarantor_lni *c, const char *path, uint32_t timeout_ms)
{
    struct sockaddr_un address;
    int flags;
    if (c == NULL || path == NULL || timeout_ms == 0U || timeout_ms > 60000U ||
        strlen(path) >= sizeof(address.sun_path))
        return LXP_ERR_NON_CANONICAL;
    *c = (lxp_guarantor_lni){-1, 0U, timeout_ms};
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, path, strlen(path) + 1U);
    c->fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (c->fd < 0)
        return LXP_ERR_IO;
    flags = connect(c->fd, (const struct sockaddr *)&address, sizeof(address));
    if (flags != 0) {
        lxp_guarantor_lni_close(c);
        return LXP_ERR_IO;
    }
    lxp_result status = handshake(c);
    if (status != LXP_OK)
        lxp_guarantor_lni_close(c);
    return status;
}
void lxp_guarantor_lni_close(lxp_guarantor_lni *c)
{
    if (c != NULL && c->fd >= 0) {
        (void)close(c->fd);
        c->fd = -1;
    }
}
static lxp_result request(lxp_guarantor_lni *c, uint16_t tag, lxp_byte_span payload,
                          lxp_byte_span proof, int64_t deadline)
{
    size_t size = 22U + payload.length + proof.length;
    uint8_t *bytes;
    lxp_result status;
    if (c == NULL || c->fd < 0 || c->correlation == UINT64_MAX ||
        payload.length > LXP_GUARANTOR_FRAME_MAX || proof.length > LXP_GUARANTOR_FRAME_MAX ||
        size > LXP_GUARANTOR_FRAME_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    bytes = malloc(size + 4U);
    if (bytes == NULL)
        return LXP_ERR_IO;
    put(bytes, size, 4U);
    put(bytes + 4U, 1U, 2U);
    put(bytes + 6U, 4U, 2U);
    put(bytes + 8U, tag, 2U);
    put(bytes + 10U, tag == 1U ? c->correlation : ++c->correlation, 8U);
    put(bytes + 18U, payload.length, 4U);
    if (payload.length != 0U)
        memcpy(bytes + 22U, payload.bytes, payload.length);
    put(bytes + 22U + payload.length, proof.length, 4U);
    if (proof.length != 0U)
        memcpy(bytes + 26U + payload.length, proof.bytes, proof.length);
    status = transfer(c->fd, bytes, size + 4U, true, deadline);
    free(bytes);
    return status;
}
static lxp_result response(lxp_guarantor_lni *c, lxp_arena *arena, uint16_t *tag,
                           lxp_byte_span *payload, lxp_byte_span *proof, int64_t deadline)
{
    uint8_t prefix[4], *bytes;
    void *memory;
    size_t size, plen, qlen;
    lxp_result status = transfer(c->fd, prefix, 4U, false, deadline);
    if (status != LXP_OK)
        return status;
    size = (size_t)get(prefix, 4U);
    if (size < 22U || size > LXP_GUARANTOR_FRAME_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    status = lxp_arena_alloc(arena, size, 1U, &memory);
    if (status != LXP_OK)
        return status;
    bytes = memory;
    status = transfer(c->fd, bytes, size, false, deadline);
    if (status != LXP_OK)
        return status;
    if (get(bytes, 2U) != 1U || get(bytes + 2U, 2U) != 4U || get(bytes + 6U, 8U) != c->correlation)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *tag = (uint16_t)get(bytes + 4U, 2U);
    plen = (size_t)get(bytes + 14U, 4U);
    if (plen > size - 22U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    qlen = (size_t)get(bytes + 18U + plen, 4U);
    if (qlen != size - 22U - plen)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *payload = (lxp_byte_span){bytes + 18U, plen};
    *proof = (lxp_byte_span){bytes + 22U + plen, qlen};
    if (*tag == 25U) {
        uint32_t code;
        int32_t signed_code;
        if (plen != 5U || qlen != 0U)
            return LXP_ERR_MALFORMED_ENVELOPE;
        code = (uint32_t)get(payload->bytes + 1U, 4U);
        memcpy(&signed_code, &code, sizeof(code));
        return signed_code < 0 ? (lxp_result)signed_code : LXP_ERR_MALFORMED_ENVELOPE;
    }
    return LXP_OK;
}
static lxp_result handshake(lxp_guarantor_lni *c)
{
    void *memory = malloc(LXP_GUARANTOR_FRAME_MAX);
    lxp_arena arena;
    uint16_t tag;
    lxp_byte_span payload, proof;
    int64_t deadline = now() + c->timeout_ms;
    lxp_result status;
    if (memory == NULL)
        return LXP_ERR_IO;
    status = lxp_arena_init(&arena, memory, LXP_GUARANTOR_FRAME_MAX);
    if (status == LXP_OK)
        status = request(c, 1U, (lxp_byte_span){NULL, 0U}, (lxp_byte_span){NULL, 0U}, deadline);
    if (status == LXP_OK)
        status = response(c, &arena, &tag, &payload, &proof, deadline);
    if (status == LXP_OK && (tag != 2U || payload.length < 93U || proof.length != 0U))
        status = LXP_ERR_MALFORMED_ENVELOPE;
    free(memory);
    return status;
}
lxp_result lxp_guarantor_lni_header(lxp_guarantor_lni *c, uint64_t batch,
                                    const lxp_sequencer_authorization *authority, uint32_t network,
                                    lxp_arena *arena, lxp_batch_header *header,
                                    uint8_t signature[64])
{
    uint8_t query[10] = {0U, 1U};
    uint16_t tag;
    lxp_byte_span payload, proof;
    int64_t deadline;
    lxp_result status;
    if (c == NULL || authority == NULL || header == NULL || signature == NULL || arena == NULL ||
        batch == 0U)
        return LXP_ERR_NON_CANONICAL;
    deadline = now() + c->timeout_ms;
    put(query + 2U, batch, 8U);
    status =
        request(c, 12U, (lxp_byte_span){query, sizeof(query)}, (lxp_byte_span){NULL, 0U}, deadline);
    if (status == LXP_OK)
        status = response(c, arena, &tag, &payload, &proof, deadline);
    if (status != LXP_OK)
        return status;
    if (tag != 13U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    if (payload.length == 0U && proof.length == 0U)
        return LXP_ERR_DA_MISSING;
    if (proof.length != 146U || get(proof.bytes, 2U) != 1U ||
        memcmp(proof.bytes + 2U, authority->sequencer_id, 32U) != 0 ||
        memcmp(proof.bytes + 34U, authority->public_key, 32U) != 0 ||
        get(proof.bytes + 66U, 8U) != authority->first_batch_number ||
        get(proof.bytes + 74U, 8U) != authority->last_batch_number)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_batch_header_decode(payload.bytes, payload.length, header);
    if (status == LXP_OK && (header->batch_number != batch || header->network_id != network))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(header, proof.bytes + 82U, 64U, authority, arena);
    if (status == LXP_OK)
        memcpy(signature, proof.bytes + 82U, 64U);
    return status;
}
lxp_result lxp_guarantor_chunk_verify(lxp_byte_span bytes, lxp_byte_span metadata,
                                      const lxp_batch_header *header, uint32_t index,
                                      lxp_da_chunk *chunk, uint32_t *count)
{
    lxp_merkle_proof proof;
    uint8_t claimed[32];
    lxp_result status;
    const uint8_t *p = metadata.bytes;
    if (header == NULL || chunk == NULL || count == NULL || p == NULL || metadata.length < 62U ||
        bytes.length > LXP_DA_MAX_CHUNK_BYTES || (bytes.length != 0U && bytes.bytes == NULL))
        return LXP_ERR_NON_CANONICAL;
    memset(chunk, 0, sizeof(*chunk));
    memset(&proof, 0, sizeof(proof));
    chunk->batch_number = get(p, 8U);
    chunk->chunk_index = (uint32_t)get(p + 8U, 4U);
    chunk->availability_class = (lxp_da_class)p[12];
    chunk->class_offset = get(p + 13U, 8U);
    memcpy(claimed, p + 21U, 32U);
    proof.leaf_index = (uint32_t)get(p + 53U, 4U);
    proof.leaf_count = (uint32_t)get(p + 57U, 4U);
    proof.depth = p[61];
    if (proof.depth > LXP_MERKLE_MAX_DEPTH || metadata.length != 62U + (size_t)proof.depth * 32U ||
        proof.leaf_count < LXP_DA_CLASS_COUNT || proof.leaf_count > LXP_DA_MAX_CHUNKS ||
        proof.leaf_index != index || chunk->chunk_index != index ||
        chunk->batch_number != header->batch_number)
        return LXP_ERR_DA_MISSING;
    memcpy(proof.siblings, p + 62U, (size_t)proof.depth * 32U);
    chunk->bytes = bytes;
    chunk->length = (uint32_t)bytes.length;
    status = lxp_da_chunk_hash(chunk);
    if (status == LXP_OK && lxp_ct_memcmp(claimed, chunk->chunk_hash, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(chunk->chunk_hash, &proof, header->data_availability_root);
    if (status == LXP_OK)
        *count = proof.leaf_count;
    return status;
}
lxp_result lxp_guarantor_lni_fetch(lxp_guarantor_lni *c, const lxp_batch_header *header,
                                   lxp_arena *arena, lxp_da_bundle *bundle)
{
    uint8_t query[9] = {LXP_GUARANTOR_CANDIDATE_SELECTOR}, root[32];
    uint32_t expected = 0U;
    uint16_t tag;
    void *memory;
    int64_t deadline;
    lxp_result status;
    if (c == NULL || header == NULL || arena == NULL || bundle == NULL)
        return LXP_ERR_NON_CANONICAL;
    deadline = now() + c->timeout_ms;
    memset(bundle, 0, sizeof(*bundle));
    bundle->batch_number = header->batch_number;
    status = lxp_arena_alloc(arena, LXP_DA_MAX_CHUNKS * sizeof(lxp_da_chunk),
                             _Alignof(lxp_da_chunk), &memory);
    if (status != LXP_OK)
        return status;
    bundle->chunks = memory;
    put(query + 1U, header->batch_number, 8U);
    status =
        request(c, 18U, (lxp_byte_span){query, sizeof(query)}, (lxp_byte_span){NULL, 0U}, deadline);
    while (status == LXP_OK) {
        lxp_byte_span payload, proof;
        uint32_t count;
        status = response(c, arena, &tag, &payload, &proof, deadline);
        if (status != LXP_OK)
            break;
        if (tag == 20U) {
            if (payload.length != 0U || proof.length != 0U || bundle->chunk_count != expected ||
                expected == 0U)
                return LXP_ERR_DA_MISSING;
            status = lxp_da_bundle_root(bundle, arena, root);
            if (status == LXP_OK && lxp_ct_memcmp(root, header->data_availability_root, 32U) != 0)
                status = LXP_ERR_ROOT_MISMATCH;
            return status;
        }
        if (tag != 19U || bundle->chunk_count >= LXP_DA_MAX_CHUNKS)
            return LXP_ERR_DA_MISSING;
        status = lxp_guarantor_chunk_verify(payload, proof, header, (uint32_t)bundle->chunk_count,
                                            &bundle->chunks[bundle->chunk_count], &count);
        if (status != LXP_OK)
            break;
        if (expected != 0U && count != expected)
            return LXP_ERR_DA_MISSING;
        expected = count;
        ++bundle->chunk_count;
        bundle->total_bytes += payload.length;
    }
    return status;
}
lxp_result lxp_guarantor_lni_feedback(lxp_guarantor_lni *c, lxp_byte_span certificate,
                                      lxp_byte_span proof, lxp_arena *arena)
{
    uint16_t tag;
    lxp_byte_span payload, evidence;
    int64_t deadline;
    lxp_result status;
    if (c == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    deadline = now() + c->timeout_ms;
    status = request(c, 28U, certificate, proof, deadline);
    if (status == LXP_OK)
        status = response(c, arena, &tag, &payload, &evidence, deadline);
    if (status == LXP_OK && (tag != 29U || evidence.length != 0U || payload.length != 74U ||
                             get(payload.bytes, 2U) != 1U))
        status = LXP_ERR_MALFORMED_ENVELOPE;
    return status;
}

lxp_result lxp_guarantor_lni_checkpoint(lxp_guarantor_lni *c, uint64_t batch, lxp_arena *arena,
                                        lxp_byte_span *certificate, lxp_byte_span *proof)
{
    uint8_t query[11] = {0U, 1U, 2U};
    uint16_t tag;
    lxp_result status;
    if (c == NULL || arena == NULL || certificate == NULL || proof == NULL || batch == 0U)
        return LXP_ERR_NON_CANONICAL;
    int64_t deadline = now() + c->timeout_ms;
    put(query + 3U, batch, 8U);
    status =
        request(c, 14U, (lxp_byte_span){query, sizeof(query)}, (lxp_byte_span){NULL, 0U}, deadline);
    if (status == LXP_OK)
        status = response(c, arena, &tag, certificate, proof, deadline);
    if (status == LXP_OK && (tag != 15U || certificate->length == 0U || proof->length == 0U))
        status = LXP_ERR_MALFORMED_ENVELOPE;
    return status;
}
