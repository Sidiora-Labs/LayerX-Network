#define _POSIX_C_SOURCE 200809L
#include "lxp_daemon_finality_authority.h"
#include "layerx/lxp_crypto.h"
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
static lxp_result rpc(const lxp_daemon_finality_authority *authority, const char *method, const char *params, char *response, json_document *doc, const json_token **result)
{
    char body[512]; char request[1024];
    struct sockaddr_in address;
    size_t sent = 0U, received = 0U, header_length = 0U, content_length = 0U;
    int length, fd, error = 0;
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
        if (ready(fd, POLLIN, deadline) != 0) goto cleanup;
        count = recv(fd, response + received, RPC_CAPACITY - 1U - received, 0);
        if (count < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        if (count <= 0) goto cleanup;
        received += (size_t)count; response[received] = '\0';
        if (header_length == 0U) {
            char *end = strstr(response, "\r\n\r\n"); char *line;
            bool has_length = false;
            if (end == NULL) { if (received > 8192U) goto cleanup; continue; }
            header_length = (size_t)(end - response) + 4U;
            if (strncmp(response, "HTTP/1.1 200 ", 13U) != 0 && strncmp(response, "HTTP/1.0 200 ", 13U) != 0) goto cleanup;
            line = strstr(response, "\r\n");
            if (line == NULL) goto cleanup;
            line += 2U;
            while (line < end) {
                char *next = strstr(line, "\r\n"); char *colon;
                if (next == NULL) goto cleanup;
                colon = memchr(line, ':', (size_t)(next - line));
                if (colon == NULL) goto cleanup;
                if ((size_t)(colon - line) == 14U && strncasecmp(line, "Content-Length", 14U) == 0) {
                    char *value = colon + 1;
                    if (has_length) goto cleanup;
                    has_length = true;
                    while (value < next && (*value == ' ' || *value == '\t')) ++value;
                    if (value == next) goto cleanup;
                    while (value < next && *value >= '0' && *value <= '9') {
                        if (content_length > RPC_CAPACITY / 10U) goto cleanup;
                        content_length = content_length * 10U + (unsigned)(*value++ - '0');
                    }
                    while (value < next && (*value == ' ' || *value == '\t')) ++value;
                    if (value != next || content_length == 0U || content_length >= RPC_CAPACITY - header_length) goto cleanup;
                }
                if ((size_t)(colon - line) == 17U && strncasecmp(line, "Transfer-Encoding", 17U) == 0) goto cleanup;
                line = next + 2U;
            }
            if (!has_length) goto cleanup;
        }
        if (received >= header_length + content_length) break;
    }
    if (header_length == 0U || received != header_length + content_length) goto cleanup;
    doc->count = 0U; doc->cursor = response + header_length; doc->end = response + received;
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
lxp_result lxp_daemon_finality_authority_init(lxp_daemon_finality_authority *authority, lxp_daemon_evidence_store *store)
{
    const char *address = getenv("LAYERX_NODE_PAXEER_RPC_ADDRESS");
    const char *settlement = getenv("LAYERX_NODE_SETTLEMENT_CONTRACT");
    const char *registry = getenv("LAYERX_NODE_CHECKPOINT_REGISTRY");
    uint64_t port;
    if (authority == NULL || store == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(authority, 0, sizeof(*authority));
    if (address == NULL || strcmp(address, "127.0.0.1") != 0 || settlement == NULL || registry == NULL ||
        decimal_environment("LAYERX_NODE_PAXEER_CHAIN_ID", UINT64_MAX, &authority->paxeer_chain_id) != 0 ||
        decimal_environment("LAYERX_NODE_PAXEER_RPC_PORT", UINT16_MAX, &port) != 0 ||
        hex_bytes(settlement, strlen(settlement), authority->settlement_contract, 20U) != 0 ||
        hex_bytes(registry, strlen(registry), authority->checkpoint_registry, 20U) != 0 ||
        lxp_ct_is_zero(authority->settlement_contract, 20U) || lxp_ct_is_zero(authority->checkpoint_registry, 20U)) return LXP_ERR_NON_CANONICAL;
    authority->store = store; authority->rpc_port = (uint16_t)port;
    return LXP_OK;
}
static void abi_u64(uint8_t *word, uint64_t value)
{
    size_t i;
    (void)memset(word, 0, 32U);
    for (i = 0U; i < 8U; ++i) { word[31U - i] = (uint8_t)value; value >>= 8U; }
}
static int registered_event(const json_document *doc, const json_token *receipt, const lxp_daemon_finality_authority *authority, const lxp_guarantor_cert *certificate, const lxp_guarantor_set *set, const lxp_daemon_settlement_registration_evidence *registration)
{
    static const char event_hash[] = "0x094d06132be90f1544eba63ff4d50ff3216950fca4912b3d469d482fbf88261c";
    const json_token *logs = field(doc, receipt, "logs");
    const json_token *block_hash = field(doc, receipt, "blockHash");
    const lxp_batch_header *header = &certificate->checkpoint.header;
    uint8_t data[192], epoch[32], batch[32], hash[32];
    size_t i, matches = 0U;
    if (logs == NULL || logs->kind != '[' || block_hash == NULL || block_hash->kind != '"' || hex_bytes(block_hash->text, block_hash->length, hash, 32U) != 0 || lxp_ct_is_zero(hash, 32U)) return 0;
    abi_u64(epoch, header->epoch); abi_u64(batch, header->batch_number);
    abi_u64(data, header->first_sequence); abi_u64(data + 32U, header->last_sequence);
    (void)memcpy(data + 64U, header->previous_state_root, 32U);
    (void)memcpy(data + 96U, header->resulting_state_root, 32U);
    (void)memcpy(data + 128U, header->data_availability_root, 32U);
    abi_u64(data + 160U, set->version);
    i = (size_t)(logs - doc->tokens) + 1U;
    while (i < logs->end) {
        const json_token *log = &doc->tokens[i];
        const json_token *topics = field(doc, log, "topics");
        const json_token *removed = field(doc, log, "removed");
        uint64_t block;
        if (topics != NULL && topics->kind == '[' && topics->end == (size_t)(topics - doc->tokens) + 5U &&
            equal(topics + 1U, event_hash) && token_bytes(topics + 2U, registration->checkpoint_id, 32U) &&
            token_bytes(topics + 3U, epoch, 32U) && token_bytes(topics + 4U, batch, 32U) &&
            token_bytes(field(doc, log, "address"), authority->checkpoint_registry, 20U) &&
            token_bytes(field(doc, log, "transactionHash"), registration->transaction_id, 32U) &&
            token_bytes(field(doc, log, "blockHash"), hash, 32U) &&
            quantity(field(doc, log, "blockNumber"), &block) == 0 && block == registration->observed_block_number &&
            removed != NULL && removed->kind == 'f' && equal(removed, "false") && token_bytes(field(doc, log, "data"), data, sizeof(data))) ++matches;
        i = log->end;
    }
    return matches == 1U;
}
lxp_result lxp_daemon_finality_authority_verify(void *context, const lxp_guarantor_cert *certificate, const lxp_guarantor_set *bonded_set, const lxp_finalisation_requirements *requirements, const lxp_daemon_settlement_registration_evidence *registration)
{
    lxp_daemon_finality_authority *authority = context;
    uint8_t *memory;
    char *response;
    json_document doc;
    const json_token *result = NULL;
    lxp_arena arena;
    lxp_finalisation_state finalisation;
    uint8_t checkpoint_id[32], binding[32];
    char address[43], transaction[67], params[192];
    uint64_t value;
    bool finalisable = false;
    lxp_result status;
    size_t i;
    if (authority == NULL || authority->store == NULL || authority->rpc_port == 0U || certificate == NULL || bonded_set == NULL || requirements == NULL || registration == NULL || certificate->attestation_count == 0U || certificate->attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS) return LXP_ERR_NON_CANONICAL;
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
    finalisation = authority->store->registry.finalisation;
    if (status == LXP_OK) status = lxp_checkpoint_finalisable(&finalisation, certificate, bonded_set, requirements, &arena, &finalisable);
    if (status == LXP_OK && !finalisable) status = LXP_ERR_ATTESTATION_THRESHOLD;
    if (status == LXP_OK) status = rpc(authority, "eth_chainId", "[]", response, &doc, &result);
    if (status == LXP_OK && (quantity(result, &value) != 0 || value != authority->paxeer_chain_id)) status = LXP_ERR_CONTEXT_MISMATCH;
    encode_hex(authority->checkpoint_registry, 20U, address);
    (void)snprintf(params, sizeof(params), "[{\"to\":\"%s\",\"data\":\"0x9e2b11f1\"},\"latest\"]", address);
    if (status == LXP_OK) status = rpc(authority, "eth_call", params, response, &doc, &result);
    (void)memset(binding, 0, sizeof(binding)); (void)memcpy(binding + 12U, authority->settlement_contract, 20U);
    if (status == LXP_OK && !token_bytes(result, binding, 32U)) status = LXP_ERR_CONTEXT_MISMATCH;
    encode_hex(registration->transaction_id, 32U, transaction);
    (void)snprintf(params, sizeof(params), "[\"%s\"]", transaction);
    if (status == LXP_OK) status = rpc(authority, "eth_getTransactionReceipt", params, response, &doc, &result);
    if (status == LXP_OK && (result->kind != '{' ||
        quantity(field(&doc, result, "status"), &value) != 0 || value != 1U ||
        quantity(field(&doc, result, "blockNumber"), &value) != 0 || value != registration->observed_block_number ||
        !token_bytes(field(&doc, result, "transactionHash"), registration->transaction_id, 32U) ||
        !token_bytes(field(&doc, result, "to"), authority->checkpoint_registry, 20U) ||
        !registered_event(&doc, result, authority, certificate, bonded_set, registration))) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = rpc(authority, "eth_blockNumber", "[]", response, &doc, &result);
    if (status == LXP_OK && (quantity(result, &value) != 0 || value < registration->observed_block_number)) status = LXP_ERR_CONTEXT_MISMATCH;
    free(doc.tokens); free(response); free(memory);
    return status;
}
