#define _POSIX_C_SOURCE 200809L
#include "lxp_daemon_finality_authority.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_paxeer.h"
#include <arpa/inet.h>
#include <errno.h>
#include <limits.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

enum { RPC_CAPACITY = 262144, TOKEN_CAPACITY = 16384, RPC_TIMEOUT_MS = 5000 };
enum { HEADER_LIMIT = 8192, CHUNK_SIZE_DIGITS = 16, TRAILER_LINE_LIMIT = 1024 };
typedef struct json_token { const char *text; size_t length; size_t end; char kind; } json_token;
typedef struct json_document { json_token *tokens; size_t count; const char *cursor; const char *end; } json_document;

static void whitespace(json_document *doc)
{
    while (doc->cursor < doc->end && (*doc->cursor == ' ' || *doc->cursor == '\r' || *doc->cursor == '\n' || *doc->cursor == '\t')) ++doc->cursor;
}
static int json_number(const char *text, size_t length)
{
    size_t index = 0U;
    if (index < length && text[index] == '-') ++index;
    if (index == length) return 0;
    if (text[index] == '0') ++index;
    else {
        if (text[index] < '1' || text[index] > '9') return 0;
        do { ++index; } while (index < length && text[index] >= '0' && text[index] <= '9');
    }
    if (index < length && text[index] == '.') {
        ++index;
        if (index == length || text[index] < '0' || text[index] > '9') return 0;
        do { ++index; } while (index < length && text[index] >= '0' && text[index] <= '9');
    }
    if (index < length && (text[index] == 'e' || text[index] == 'E')) {
        ++index;
        if (index < length && (text[index] == '+' || text[index] == '-')) ++index;
        if (index == length || text[index] < '0' || text[index] > '9') return 0;
        do { ++index; } while (index < length && text[index] >= '0' && text[index] <= '9');
    }
    return index == length;
}
static int parse_value(json_document *doc, unsigned depth)
{
    size_t index;
    char kind;
    whitespace(doc);
    if (depth > 32U || doc->cursor == doc->end || doc->count == TOKEN_CAPACITY) return -1;
    index = doc->count++;
    kind = *doc->cursor++;
    doc->tokens[index].kind = kind;
    doc->tokens[index].text = doc->cursor;
    if (kind == '{' || kind == '[') {
        char close = kind == '{' ? '}' : ']';
        whitespace(doc);
        if (doc->cursor < doc->end && *doc->cursor != close) {
            for (;;) {
                if (kind == '{') {
                    size_t key_index = doc->count;
                    size_t previous = index + 1U;
                    if (doc->cursor == doc->end || *doc->cursor != '"' || parse_value(doc, depth + 1U) != 0) return -1;
                    while (previous < key_index) {
                        const json_token *old = &doc->tokens[previous];
                        const json_token *key = &doc->tokens[key_index];
                        if (old->length == key->length && memcmp(old->text, key->text, key->length) == 0) return -1;
                        previous = doc->tokens[previous + 1U].end;
                    }
                    whitespace(doc);
                    if (doc->cursor == doc->end || *doc->cursor++ != ':') return -1;
                }
                if (parse_value(doc, depth + 1U) != 0) return -1;
                whitespace(doc);
                if (doc->cursor == doc->end) return -1;
                if (*doc->cursor != ',') break;
                ++doc->cursor;
                whitespace(doc);
            }
        }
        if (doc->cursor == doc->end || *doc->cursor++ != close) return -1;
    } else if (kind == '"') {
        while (doc->cursor < doc->end && *doc->cursor != '"') {
            unsigned char ch = (unsigned char)*doc->cursor++;
            if (ch < 32U || ch == '\\') return -1;
        }
        if (doc->cursor == doc->end) return -1;
        doc->tokens[index].length = (size_t)(doc->cursor - doc->tokens[index].text);
        ++doc->cursor;
    } else {
        const char *start = doc->cursor - 1;
        while (doc->cursor < doc->end && *doc->cursor != ',' && *doc->cursor != '}' && *doc->cursor != ']' && *doc->cursor != ' ' && *doc->cursor != '\n' && *doc->cursor != '\r' && *doc->cursor != '\t') ++doc->cursor;
        doc->tokens[index].text = start;
        doc->tokens[index].length = (size_t)(doc->cursor - start);
        if (!((doc->tokens[index].length == 4U && (memcmp(start, "null", 4U) == 0 || memcmp(start, "true", 4U) == 0)) || (doc->tokens[index].length == 5U && memcmp(start, "false", 5U) == 0) || json_number(start, doc->tokens[index].length))) return -1;
    }
    doc->tokens[index].end = doc->count;
    return 0;
}
static int equal(const json_token *token, const char *text)
{
    return token != NULL && token->length == strlen(text) && memcmp(token->text, text, token->length) == 0;
}
static const json_token *field(const json_document *doc, const json_token *object, const char *name)
{
    const json_token *found = NULL;
    size_t i;
    if (object == NULL || object->kind != '{') return NULL;
    i = (size_t)(object - doc->tokens) + 1U;
    while (i < object->end) {
        const json_token *key = &doc->tokens[i++];
        const json_token *value = &doc->tokens[i];
        if (equal(key, name)) { if (found != NULL) return NULL; found = value; }
        i = value->end;
    }
    return found;
}
static int nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}
static int hex_bytes(const char *text, size_t length, uint8_t *out, size_t bytes)
{
    size_t i;
    if (text == NULL || length != bytes * 2U + 2U || text[0] != '0' || text[1] != 'x') return -1;
    for (i = 0U; i < bytes; ++i) {
        int hi = nibble(text[2U + i * 2U]); int lo = nibble(text[3U + i * 2U]);
        if (hi < 0 || lo < 0) return -1;
        out[i] = (uint8_t)((unsigned)hi * 16U + (unsigned)lo);
    }
    return 0;
}
static int token_bytes(const json_token *token, const uint8_t *expected, size_t bytes)
{
    uint8_t decoded[192];
    return token != NULL && token->kind == '"' && bytes <= sizeof(decoded) && hex_bytes(token->text, token->length, decoded, bytes) == 0 && lxp_ct_memcmp(decoded, expected, bytes) == 0;
}
static int quantity(const json_token *token, uint64_t *out)
{
    size_t i;
    uint64_t value = 0U;
    if (token == NULL || token->kind != '"' || token->length < 3U || token->length > 18U || token->text[0] != '0' || token->text[1] != 'x' || (token->length > 3U && token->text[2] == '0')) return -1;
    for (i = 2U; i < token->length; ++i) { int digit = nibble(token->text[i]); if (digit < 0) return -1; value = value * 16U + (unsigned)digit; }
    *out = value;
    return 0;
}
static void encode_hex(const uint8_t *bytes, size_t length, char *text)
{
    static const char digits[] = "0123456789abcdef";
    size_t i;
    text[0] = '0'; text[1] = 'x';
    for (i = 0U; i < length; ++i) { text[2U + i * 2U] = digits[bytes[i] >> 4U]; text[3U + i * 2U] = digits[bytes[i] & 15U]; }
    text[2U + length * 2U] = '\0';
}
static int64_t milliseconds(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}
static int ready(int fd, short events, int64_t deadline)
{
    struct pollfd item = {fd, events, 0};
    for (;;) {
        int64_t left = deadline - milliseconds();
        int status;
        if (left <= 0 || left > INT_MAX) return -1;
        status = poll(&item, 1U, (int)left);
        if (status > 0) return (item.revents & events) != 0 ? 0 : -1;
        if (status == 0 || errno != EINTR) return -1;
    }
}
static const char *crlf(const char *data, size_t length)
{
    size_t i;
    for (i = 0U; i + 1U < length; ++i) { if (data[i] == '\r' && data[i + 1U] == '\n') return data + i; }
    return NULL;
}
static lxp_daemon_http_parse chunked_body(char *data, size_t length, size_t limit, char *out, size_t *consumed, size_t *decoded)
{
    size_t offset = 0U, total = 0U;
    for (;;) {
        size_t digits = 0U, size = 0U;
        while (offset + digits < length && nibble(data[offset + digits]) >= 0) {
            if (digits == CHUNK_SIZE_DIGITS) return LXP_DAEMON_HTTP_MALFORMED;
            size = size * 16U + (size_t)nibble(data[offset + digits]);
            if (size > limit) return LXP_DAEMON_HTTP_MALFORMED;
            ++digits;
        }
        if (offset + digits == length) return LXP_DAEMON_HTTP_INCOMPLETE;
        if (digits == 0U || data[offset + digits] != '\r') return LXP_DAEMON_HTTP_MALFORMED;
        if (offset + digits + 1U == length) return LXP_DAEMON_HTTP_INCOMPLETE;
        if (data[offset + digits + 1U] != '\n') return LXP_DAEMON_HTTP_MALFORMED;
        offset += digits + 2U;
        if (size == 0U) break;
        if (total > limit - size) return LXP_DAEMON_HTTP_MALFORMED;
        if (length - offset < size + 2U) return LXP_DAEMON_HTTP_INCOMPLETE;
        if (data[offset + size] != '\r' || data[offset + size + 1U] != '\n') return LXP_DAEMON_HTTP_MALFORMED;
        if (out != NULL) (void)memmove(out + total, data + offset, size);
        total += size;
        offset += size + 2U;
    }
    for (;;) {
        const char *line = crlf(data + offset, length - offset);
        size_t used;
        if (line == NULL) return length - offset > TRAILER_LINE_LIMIT ? LXP_DAEMON_HTTP_MALFORMED : LXP_DAEMON_HTTP_INCOMPLETE;
        used = (size_t)(line - (data + offset));
        if (used == 0U) { offset += 2U; break; }
        if (used > TRAILER_LINE_LIMIT || memchr(data + offset, ':', used) == NULL) return LXP_DAEMON_HTTP_MALFORMED;
        offset += used + 2U;
    }
    *consumed = offset;
    *decoded = total;
    return LXP_DAEMON_HTTP_COMPLETE;
}
lxp_daemon_http_parse lxp_daemon_http_response_parse(char *buffer, size_t received, size_t capacity, lxp_daemon_http_response *response)
{
    const char *end; const char *line;
    size_t header_length = 0U, content_length = 0U, consumed = 0U, decoded = 0U, i;
    bool has_length = false, chunked = false;
    lxp_daemon_http_parse status;
    if (buffer == NULL || response == NULL || capacity <= HEADER_LIMIT || received > capacity) return LXP_DAEMON_HTTP_MALFORMED;
    for (i = 0U; i + 3U < received; ++i) {
        if (buffer[i] == '\r' && buffer[i + 1U] == '\n' && buffer[i + 2U] == '\r' && buffer[i + 3U] == '\n') { header_length = i + 4U; break; }
    }
    if (header_length == 0U) return received > (size_t)HEADER_LIMIT ? LXP_DAEMON_HTTP_MALFORMED : LXP_DAEMON_HTTP_INCOMPLETE;
    if (header_length > (size_t)HEADER_LIMIT || header_length < 17U) return LXP_DAEMON_HTTP_MALFORMED;
    if (memcmp(buffer, "HTTP/1.1 200 ", 13U) != 0 && memcmp(buffer, "HTTP/1.0 200 ", 13U) != 0) return LXP_DAEMON_HTTP_MALFORMED;
    end = buffer + header_length - 4U;
    line = crlf(buffer, header_length - 2U);
    if (line == NULL) return LXP_DAEMON_HTTP_MALFORMED;
    line += 2U;
    while (line < end) {
        const char *next = crlf(line, (size_t)(end - line) + 2U);
        const char *colon; const char *value;
        size_t name;
        if (next == NULL) return LXP_DAEMON_HTTP_MALFORMED;
        colon = memchr(line, ':', (size_t)(next - line));
        if (colon == NULL) return LXP_DAEMON_HTTP_MALFORMED;
        name = (size_t)(colon - line);
        value = colon + 1;
        while (value < next && (*value == ' ' || *value == '\t')) ++value;
        if (name == 14U && strncasecmp(line, "Content-Length", 14U) == 0) {
            if (has_length || value == next) return LXP_DAEMON_HTTP_MALFORMED;
            has_length = true;
            while (value < next && *value >= '0' && *value <= '9') {
                if (content_length > capacity / 10U) return LXP_DAEMON_HTTP_MALFORMED;
                content_length = content_length * 10U + (size_t)(*value++ - '0');
            }
            while (value < next && (*value == ' ' || *value == '\t')) ++value;
            if (value != next || content_length == 0U || content_length >= capacity - header_length) return LXP_DAEMON_HTTP_MALFORMED;
        }
        if (name == 17U && strncasecmp(line, "Transfer-Encoding", 17U) == 0) {
            if (chunked || (size_t)(next - value) < 7U || strncasecmp(value, "chunked", 7U) != 0) return LXP_DAEMON_HTTP_MALFORMED;
            chunked = true;
            value += 7U;
            while (value < next && (*value == ' ' || *value == '\t')) ++value;
            if (value != next) return LXP_DAEMON_HTTP_MALFORMED;
        }
        line = next + 2U;
    }
    if (has_length == chunked) return LXP_DAEMON_HTTP_MALFORMED;
    response->header_length = header_length;
    response->chunked = chunked;
    response->body_offset = header_length;
    response->body_length = 0U;
    if (!chunked) {
        if (received < header_length + content_length) return LXP_DAEMON_HTTP_INCOMPLETE;
        if (received != header_length + content_length) return LXP_DAEMON_HTTP_MALFORMED;
        response->body_length = content_length;
        return LXP_DAEMON_HTTP_COMPLETE;
    }
    status = chunked_body(buffer + header_length, received - header_length, capacity - header_length, NULL, &consumed, &decoded);
    if (status != LXP_DAEMON_HTTP_COMPLETE) return status;
    if (consumed != received - header_length || decoded == 0U) return LXP_DAEMON_HTTP_MALFORMED;
    if (chunked_body(buffer + header_length, received - header_length, capacity - header_length, buffer + header_length, &consumed, &decoded) != LXP_DAEMON_HTTP_COMPLETE) return LXP_DAEMON_HTTP_MALFORMED;
    response->body_length = decoded;
    return LXP_DAEMON_HTTP_COMPLETE;
}
static lxp_result rpc(const lxp_daemon_finality_authority *authority, const char *method, const char *params, char *response, json_document *doc, const json_token **result)
{
    char body[512]; char request[1024];
    struct sockaddr_in address;
    lxp_daemon_http_response message;
    size_t sent = 0U, received = 0U;
    int length, fd, error = 0;
    bool complete = false;
    socklen_t error_length = sizeof(error);
    int64_t deadline = milliseconds() + RPC_TIMEOUT_MS;
    lxp_result status = LXP_ERR_IO;
    length = snprintf(body, sizeof(body), "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"%s\",\"params\":%s}", method, params);
    if (length < 0 || (size_t)length >= sizeof(body)) return LXP_ERR_LENGTH_LIMIT;
    length = snprintf(request, sizeof(request), "POST / HTTP/1.1\r\nHost: 127.0.0.1:%u\r\nContent-Type: application/json\r\nContent-Length: %zu\r\nConnection: close\r\n\r\n%s", authority->rpc_port, strlen(body), body);
    if (length < 0 || (size_t)length >= sizeof(request)) return LXP_ERR_LENGTH_LIMIT;
    fd = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0) return LXP_ERR_IO;
    (void)memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET; address.sin_port = htons(authority->rpc_port); address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(fd, (const struct sockaddr *)&address, sizeof(address)) != 0 && errno != EINPROGRESS) goto cleanup;
    if (ready(fd, POLLOUT, deadline) != 0 || getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &error_length) != 0 || error != 0) goto cleanup;
    while (sent < (size_t)length) {
        ssize_t count;
        if (ready(fd, POLLOUT, deadline) != 0) goto cleanup;
        count = send(fd, request + sent, (size_t)length - sent, MSG_NOSIGNAL);
        if (count < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        if (count <= 0) goto cleanup;
        sent += (size_t)count;
    }
    while (received < RPC_CAPACITY - 1U) {
        ssize_t count;
        lxp_daemon_http_parse parsed;
        if (ready(fd, POLLIN, deadline) != 0) goto cleanup;
        count = recv(fd, response + received, RPC_CAPACITY - 1U - received, 0);
        if (count < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        if (count <= 0) goto cleanup;
        received += (size_t)count;
        parsed = lxp_daemon_http_response_parse(response, received, RPC_CAPACITY - 1U, &message);
        if (parsed == LXP_DAEMON_HTTP_MALFORMED) goto cleanup;
        if (parsed == LXP_DAEMON_HTTP_COMPLETE) { complete = true; break; }
    }
    if (!complete) goto cleanup;
    doc->count = 0U; doc->cursor = response + message.body_offset; doc->end = doc->cursor + message.body_length;
    status = LXP_ERR_CONTEXT_MISMATCH;
    if (parse_value(doc, 0U) != 0) goto cleanup;
    whitespace(doc);
    if (doc->cursor != doc->end || !equal(field(doc, doc->tokens, "jsonrpc"), "2.0") || !equal(field(doc, doc->tokens, "id"), "1") || field(doc, doc->tokens, "id")->kind != '1' || field(doc, doc->tokens, "error") != NULL) goto cleanup;
    *result = field(doc, doc->tokens, "result");
    if (*result != NULL) status = LXP_OK;
cleanup:
    (void)close(fd);
    return status;
}
static int decimal_environment(const char *name, uint64_t maximum, uint64_t *out)
{
    const char *text = getenv(name);
    uint64_t value = 0U;
    if (text == NULL || *text == '\0') return -1;
    while (*text != '\0') {
        unsigned digit;
        if (*text < '0' || *text > '9') return -1;
        digit = (unsigned)(*text++ - '0');
        if (value > (maximum - digit) / 10U) return -1;
        value = value * 10U + digit;
    }
    if (value == 0U) return -1;
    *out = value;
    return 0;
}
lxp_result lxp_daemon_finality_authority_init_pins(lxp_daemon_finality_authority *authority)
{
    const char *address = getenv("LAYERX_NODE_PAXEER_RPC_ADDRESS");
    const char *settlement = getenv("LAYERX_NODE_SETTLEMENT_CONTRACT");
    const char *registry = getenv("LAYERX_NODE_CHECKPOINT_REGISTRY");
    uint64_t port;
    if (authority == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(authority, 0, sizeof(*authority));
    if (address == NULL || strcmp(address, "127.0.0.1") != 0 || settlement == NULL || registry == NULL ||
        decimal_environment("LAYERX_NODE_PAXEER_CHAIN_ID", UINT64_MAX, &authority->paxeer_chain_id) != 0 ||
        decimal_environment("LAYERX_NODE_PAXEER_RPC_PORT", UINT16_MAX, &port) != 0 ||
        hex_bytes(settlement, strlen(settlement), authority->settlement_contract, 20U) != 0 ||
        hex_bytes(registry, strlen(registry), authority->checkpoint_registry, 20U) != 0 ||
        lxp_ct_memcmp(authority->settlement_contract, lxp_paxeer_anchor_address, 20U) != 0 ||
        lxp_ct_memcmp(authority->checkpoint_registry, lxp_paxeer_anchor_address, 20U) != 0) return LXP_ERR_NON_CANONICAL;
    authority->rpc_port = (uint16_t)port;
    return LXP_OK;
}
lxp_result lxp_daemon_finality_authority_init(lxp_daemon_finality_authority *authority,
    lxp_daemon_evidence_store *store)
{
    lxp_result status;
    if (authority == NULL || store == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_finality_authority_init_pins(authority);
    if (status == LXP_OK) authority->store = store;
    return status;
}
static void abi_u64(uint8_t *word, uint64_t value)
{
    size_t i;
    (void)memset(word, 0, 32U);
    for (i = 0U; i < 8U; ++i) { word[31U - i] = (uint8_t)value; value >>= 8U; }
}
enum { ANCHOR_CHECKPOINT_WORDS = 18, ANCHOR_CHECKPOINT_BYTES = 576 };
static int abi_word_u64(const uint8_t *word, uint64_t *out)
{
    size_t i;
    uint64_t value = 0U;
    if (!lxp_ct_is_zero(word, 24U)) return -1;
    for (i = 24U; i < 32U; ++i) value = (value << 8U) | word[i];
    *out = value;
    return 0;
}
static int anchor_ladder_decode(const json_token *result, lxp_daemon_anchor_ladder *ladder)
{
    uint8_t word[32];
    uint64_t value;
    if (result == NULL || ladder == NULL || result->kind != '"' || hex_bytes(result->text, result->length, word, 32U) != 0 ||
        abi_word_u64(word, &value) != 0 || value > (uint64_t)LXP_DAEMON_ANCHOR_FINAL) return -1;
    *ladder = (lxp_daemon_anchor_ladder)value;
    return 0;
}
static int anchor_checkpoint_matches(const json_token *result, const lxp_guarantor_cert *certificate, const uint8_t checkpoint_id[32], lxp_daemon_anchor_ladder ladder, uint64_t *submitted_height, uint64_t *finalized_height)
{
    static uint8_t record[ANCHOR_CHECKPOINT_BYTES];
    const lxp_batch_header *header = &certificate->checkpoint.header;
    uint64_t batch, epoch, first, last, timestamp, status, signers, mask, challenges;
    if (result == NULL || result->kind != '"' || hex_bytes(result->text, result->length, record, sizeof(record)) != 0 ||
        abi_word_u64(record, &batch) != 0 || abi_word_u64(record + 96U, &epoch) != 0 ||
        abi_word_u64(record + 128U, &first) != 0 || abi_word_u64(record + 160U, &last) != 0 ||
        abi_word_u64(record + 352U, &timestamp) != 0 || abi_word_u64(record + 384U, &status) != 0 ||
        abi_word_u64(record + 416U, &signers) != 0 || abi_word_u64(record + 448U, &mask) != 0 ||
        abi_word_u64(record + 480U, &challenges) != 0 || abi_word_u64(record + 512U, submitted_height) != 0 ||
        abi_word_u64(record + 544U, finalized_height) != 0) return 0;
    return batch == header->batch_number && lxp_ct_memcmp(record + 32U, checkpoint_id, 32U) == 0 &&
        !lxp_ct_is_zero(record + 64U, 32U) && epoch == header->epoch && first == header->first_sequence &&
        last == header->last_sequence && lxp_ct_memcmp(record + 192U, header->previous_state_root, 32U) == 0 &&
        lxp_ct_memcmp(record + 224U, header->resulting_state_root, 32U) == 0 &&
        lxp_ct_memcmp(record + 256U, header->receipt_merkle_root, 32U) == 0 &&
        lxp_ct_memcmp(record + 288U, header->data_availability_root, 32U) == 0 &&
        lxp_ct_memcmp(record + 320U, header->sequencer_id, 32U) == 0 && timestamp == header->timestamp_ms &&
        status == (uint64_t)ladder && signers == certificate->attestation_count && *submitted_height != 0U &&
        (ladder == LXP_DAEMON_ANCHOR_FINAL ? (challenges == 0U && *finalized_height >= *submitted_height) : *finalized_height == 0U);
}
static lxp_result event_topic(const char *signature, char topic[67])
{
    uint8_t hash[32];
    lxp_result status = lxp_keccak256((const uint8_t *)signature, strlen(signature), hash);
    if (status == LXP_OK) encode_hex(hash, 32U, topic);
    return status;
}
static int submitted_event(const json_document *doc, const json_token *receipt, const lxp_guarantor_cert *certificate, const lxp_daemon_settlement_registration_evidence *registration)
{
    const json_token *logs = field(doc, receipt, "logs");
    const json_token *block_hash = field(doc, receipt, "blockHash");
    const lxp_batch_header *header = &certificate->checkpoint.header;
    char topic[67];
    uint8_t data[96], batch[32], hash[32];
    size_t i, matches = 0U;
    if (logs == NULL || logs->kind != '[' || block_hash == NULL || block_hash->kind != '"' || hex_bytes(block_hash->text, block_hash->length, hash, 32U) != 0 || lxp_ct_is_zero(hash, 32U) ||
        event_topic("CheckpointSubmitted(uint64,bytes32,bytes32,bytes32,uint8)", topic) != LXP_OK) return 0;
    abi_u64(batch, header->batch_number);
    (void)memcpy(data, header->resulting_state_root, 32U);
    (void)memcpy(data + 32U, header->receipt_merkle_root, 32U);
    abi_u64(data + 64U, certificate->attestation_count);
    i = (size_t)(logs - doc->tokens) + 1U;
    while (i < logs->end) {
        const json_token *log = &doc->tokens[i];
        const json_token *topics = field(doc, log, "topics");
        const json_token *removed = field(doc, log, "removed");
        uint64_t block;
        if (topics != NULL && topics->kind == '[' && topics->end == (size_t)(topics - doc->tokens) + 4U &&
            equal(topics + 1U, topic) && token_bytes(topics + 2U, batch, 32U) &&
            token_bytes(topics + 3U, registration->checkpoint_id, 32U) &&
            token_bytes(field(doc, log, "address"), lxp_paxeer_anchor_address, 20U) &&
            token_bytes(field(doc, log, "transactionHash"), registration->transaction_id, 32U) &&
            token_bytes(field(doc, log, "blockHash"), hash, 32U) &&
            quantity(field(doc, log, "blockNumber"), &block) == 0 && block == registration->observed_block_number &&
            removed != NULL && removed->kind == 'f' && equal(removed, "false") && token_bytes(field(doc, log, "data"), data, sizeof(data))) ++matches;
        i = log->end;
    }
    return matches == 1U;
}
static lxp_result anchor_call(const lxp_daemon_finality_authority *authority, const char *signature, uint64_t batch_number, char *response, json_document *doc, const json_token **result)
{
    uint8_t calldata[36];
    char address[43], data[75], params[192];
    lxp_result status = lxp_paxeer_abi_selector(signature, calldata);
    if (status != LXP_OK) return status;
    abi_u64(calldata + 4U, batch_number);
    encode_hex(lxp_paxeer_anchor_address, 20U, address);
    encode_hex(calldata, sizeof(calldata), data);
    (void)snprintf(params, sizeof(params), "[{\"to\":\"%s\",\"data\":\"%s\"},\"latest\"]", address, data);
    return rpc(authority, "eth_call", params, response, doc, result);
}
lxp_result lxp_daemon_finality_authority_ladder(const lxp_daemon_finality_authority *authority, uint64_t batch_number, lxp_daemon_anchor_ladder *ladder)
{
    char *response;
    json_document doc;
    const json_token *result = NULL;
    lxp_result status;
    if (authority == NULL || ladder == NULL || authority->rpc_port == 0U || batch_number == 0U) return LXP_ERR_NON_CANONICAL;
    response = malloc(RPC_CAPACITY);
    doc.tokens = calloc(TOKEN_CAPACITY, sizeof(*doc.tokens));
    if (response == NULL || doc.tokens == NULL) { free(response); free(doc.tokens); return LXP_ERR_IO; }
    status = anchor_call(authority, LXP_DAEMON_ANCHOR_STATUS_OF, batch_number, response, &doc, &result);
    if (status == LXP_OK && anchor_ladder_decode(result, ladder) != 0) status = LXP_ERR_CONTEXT_MISMATCH;
    free(doc.tokens); free(response);
    return status;
}
lxp_result lxp_daemon_finality_authority_verify_explicit(
    const lxp_daemon_finality_authority *authority,
    const lxp_finalisation_state *trusted_finalisation,
    const lxp_guarantor_cert *certificate, const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration)
{
    uint8_t *memory;
    char *response;
    json_document doc;
    const json_token *result = NULL;
    lxp_arena arena;
    lxp_finalisation_state finalisation;
    uint8_t checkpoint_id[32];
    char transaction[67], params[192];
    uint64_t value, submitted_height = 0U, finalized_height = 0U;
    lxp_daemon_anchor_ladder ladder = LXP_DAEMON_ANCHOR_INSTANT;
    bool finalisable = false;
    lxp_result status;
    size_t i;
    if (authority == NULL || trusted_finalisation == NULL || authority->rpc_port == 0U || certificate == NULL || bonded_set == NULL || requirements == NULL || registration == NULL || certificate->attestation_count == 0U || certificate->attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS) return LXP_ERR_NON_CANONICAL;
    if (registration->paxeer_chain_id != authority->paxeer_chain_id || lxp_ct_memcmp(registration->settlement_contract, authority->settlement_contract, 20U) != 0 || lxp_ct_is_zero(registration->transaction_id, 32U) || registration->observed_block_number == 0U) return LXP_ERR_CONTEXT_MISMATCH;
    for (i = 0U; i < certificate->attestation_count; ++i) {
        if (certificate->attestations[i].paxeer_chain_id != authority->paxeer_chain_id || lxp_ct_memcmp(certificate->attestations[i].paxeer_settlement_contract, authority->settlement_contract, 20U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    }
    memory = malloc(LXP_MAX_VALIDITY_PROOF_BYTES + 1024U * 1024U);
    response = malloc(RPC_CAPACITY);
    doc.tokens = calloc(TOKEN_CAPACITY, sizeof(*doc.tokens));
    if (memory == NULL || response == NULL || doc.tokens == NULL) { free(memory); free(response); free(doc.tokens); return LXP_ERR_IO; }
    status = lxp_arena_init(&arena, memory, LXP_MAX_VALIDITY_PROOF_BYTES + 1024U * 1024U);
    if (status == LXP_OK) status = lxp_checkpoint_certificate_hash(&certificate->checkpoint, &arena, checkpoint_id);
    if (status == LXP_OK && lxp_ct_memcmp(checkpoint_id, registration->checkpoint_id, 32U) != 0) status = LXP_ERR_CONTEXT_MISMATCH;
    finalisation = *trusted_finalisation;
    if (status == LXP_OK) status = lxp_checkpoint_finalisable(&finalisation, certificate, bonded_set, requirements, &arena, &finalisable);
    if (status == LXP_OK && !finalisable) status = LXP_ERR_ATTESTATION_THRESHOLD;
    if (status == LXP_OK) status = rpc(authority, "eth_chainId", "[]", response, &doc, &result);
    if (status == LXP_OK && (quantity(result, &value) != 0 || value != authority->paxeer_chain_id)) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = anchor_call(authority, LXP_DAEMON_ANCHOR_STATUS_OF, certificate->checkpoint.header.batch_number, response, &doc, &result);
    if (status == LXP_OK && anchor_ladder_decode(result, &ladder) != 0) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK && ladder != LXP_DAEMON_ANCHOR_FINAL) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = anchor_call(authority, LXP_DAEMON_ANCHOR_CHECKPOINT, certificate->checkpoint.header.batch_number, response, &doc, &result);
    if (status == LXP_OK && !anchor_checkpoint_matches(result, certificate, registration->checkpoint_id, ladder, &submitted_height, &finalized_height)) status = LXP_ERR_CONTEXT_MISMATCH;
    encode_hex(registration->transaction_id, 32U, transaction);
    (void)snprintf(params, sizeof(params), "[\"%s\"]", transaction);
    if (status == LXP_OK) status = rpc(authority, "eth_getTransactionReceipt", params, response, &doc, &result);
    if (status == LXP_OK && (result->kind != '{' ||
        quantity(field(&doc, result, "status"), &value) != 0 || value != 1U ||
        quantity(field(&doc, result, "blockNumber"), &value) != 0 || value != registration->observed_block_number ||
        !token_bytes(field(&doc, result, "transactionHash"), registration->transaction_id, 32U) ||
        !token_bytes(field(&doc, result, "to"), lxp_paxeer_anchor_address, 20U) ||
        !submitted_event(&doc, result, certificate, registration))) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = rpc(authority, "eth_blockNumber", "[]", response, &doc, &result);
    if (status == LXP_OK && (quantity(result, &value) != 0 || value < registration->observed_block_number || value < finalized_height)) status = LXP_ERR_CONTEXT_MISMATCH;
    free(doc.tokens); free(response); free(memory);
    return status;
}

lxp_result lxp_daemon_finality_authority_verify(void *context,
    const lxp_guarantor_cert *certificate, const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration)
{
    lxp_daemon_finality_authority *authority = context;
    if (authority == NULL || authority->store == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_daemon_finality_authority_verify_explicit(authority,
        &authority->store->registry.finalisation, certificate, bonded_set,
        requirements, registration);
}

typedef struct handover_finality_context {
    const lxp_daemon_finality_authority *authority;
    lxp_finalisation_state finalisation;
} handover_finality_context;

static lxp_result handover_finality_authority_verify(void *context,
    const lxp_guarantor_cert *certificate, const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration)
{
    const handover_finality_context *verification = context;
    if (verification == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_daemon_finality_authority_verify_explicit(verification->authority,
        &verification->finalisation, certificate, bonded_set, requirements, registration);
}

lxp_result lxp_daemon_handover_finality_verify(
    const lxp_daemon_finality_authority *authority,
    const lxp_finalisation_state *known_finalisation,
    const lxp_batch_header *authenticated_predecessor,
    const uint8_t predecessor_signature[64],
    const lxp_handover_evidence *evidence, lxp_arena *arena)
{
    handover_finality_context verification;
    lxp_byte_span header;
    size_t mark;
    lxp_result status;
    if (authority == NULL || known_finalisation == NULL || authenticated_predecessor == NULL ||
        predecessor_signature == NULL || evidence == NULL || arena == NULL ||
        authenticated_predecessor->batch_number != evidence->certificate.predecessor_batch ||
        authenticated_predecessor->network_id != evidence->certificate.network_id ||
        known_finalisation->finalisation_halted ||
        lxp_ct_memcmp(predecessor_signature, evidence->predecessor_signature, 64U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_header_encode(authenticated_predecessor, arena, &header);
    if (status == LXP_OK && (header.length != evidence->predecessor_header.length ||
        lxp_ct_memcmp(header.bytes, evidence->predecessor_header.bytes, header.length) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    verification.authority = authority;
    verification.finalisation = *known_finalisation;
    (void)memcpy(verification.finalisation.settlement_anchor,
                 authenticated_predecessor->previous_state_root, 32U);
    if (status == LXP_OK)
        status = lxp_daemon_finality_contents_verify(authenticated_predecessor->network_id,
            evidence->checkpoint_payload, evidence->finality_proof, header,
            evidence->certificate.predecessor_checkpoint_id,
            handover_finality_authority_verify, &verification, arena);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    return status;
}
