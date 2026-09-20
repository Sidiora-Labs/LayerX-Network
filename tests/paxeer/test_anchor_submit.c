#include "layerx/lxp_arena.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_paxeer.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { HEADER_BYTES = 354, ATTESTATION_BYTES = 274, REFERENCE_BYTES = 110 };

static uint8_t *file_bytes(const char *path, size_t *length)
{
    FILE *file = fopen(path, "rb");
    uint8_t *bytes;
    long size;
    if (file == NULL || fseek(file, 0L, SEEK_END) != 0 || (size = ftell(file)) <= 0 ||
        fseek(file, 0L, SEEK_SET) != 0)
        return NULL;
    bytes = malloc((size_t)size + 1U);
    if (bytes == NULL || fread(bytes, 1U, (size_t)size, file) != (size_t)size) return NULL;
    bytes[size] = 0U;
    (void)fclose(file);
    *length = (size_t)size;
    return bytes;
}

static int nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    return -1;
}

static uint8_t *hex_field(const char *from, const char *key, size_t *length)
{
    char pattern[64];
    const char *begin;
    const char *end;
    uint8_t *out;
    size_t i;
    (void)snprintf(pattern, sizeof(pattern), "\"%s\":\"", key);
    begin = strstr(from, pattern);
    if (begin == NULL) return NULL;
    begin += strlen(pattern);
    end = strchr(begin, '"');
    if (end == NULL || ((size_t)(end - begin) & 1U) != 0U) return NULL;
    *length = (size_t)(end - begin) / 2U;
    out = malloc(*length + 1U);
    if (out == NULL) return NULL;
    for (i = 0U; i < *length; ++i) {
        int hi = nibble(begin[i * 2U]);
        int lo = nibble(begin[i * 2U + 1U]);
        if (hi < 0 || lo < 0) return NULL;
        out[i] = (uint8_t)((unsigned)hi * 16U + (unsigned)lo);
    }
    return out;
}

static uint64_t be(const uint8_t *bytes, size_t width)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < width; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static int abi_signature(const char *abi, const char *name, char *out, size_t capacity)
{
    char pattern[96];
    const char *entry;
    const char *outputs;
    const char *cursor;
    size_t used;
    int first = 1;
    (void)snprintf(pattern, sizeof(pattern), "\"name\": \"%s\",\n    \"inputs\"", name);
    entry = strstr(abi, pattern);
    if (entry == NULL) return -1;
    outputs = strstr(entry, "\"outputs\"");
    if (outputs == NULL) return -1;
    used = (size_t)snprintf(out, capacity, "%s(", name);
    cursor = entry;
    while ((cursor = strstr(cursor, "\"type\": \"")) != NULL && cursor < outputs) {
        const char *end;
        cursor += 9U;
        end = strchr(cursor, '"');
        if (end == NULL || used + (size_t)(end - cursor) + 3U >= capacity) return -1;
        if (!first) out[used++] = ',';
        (void)memcpy(out + used, cursor, (size_t)(end - cursor));
        used += (size_t)(end - cursor);
        first = 0;
    }
    out[used++] = ')';
    out[used] = '\0';
    return 0;
}

int main(int argc, char **argv)
{
    static const uint8_t pinned[4] = {0x55U, 0x98U, 0xbaU, 0x4dU};
    static const uint8_t anchor[20] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x14};
    static uint8_t memory[1U << 20];
    static lxp_guarantor_cert certificate;
    lxp_arena arena;
    lxp_byte_span encoded, calldata, arguments[2];
    uint8_t *vectors, *abi, *header, *signature, *payload, selector[4];
    const char *checkpoints;
    char text[128];
    size_t length, header_length, signature_length, payload_length, cursor, proof_length, i;
    size_t offsets[3], lengths[3], tail;
    if (argc != 3) return 2;
    vectors = file_bytes(argv[1], &length);
    abi = file_bytes(argv[2], &length);
    if (vectors == NULL || abi == NULL) return 3;
    if (abi_signature((const char *)abi, "submitCheckpoint", text, sizeof(text)) != 0 ||
        strcmp(text, LXP_PAXEER_ANCHOR_SUBMIT_CHECKPOINT) != 0 ||
        lxp_paxeer_abi_selector(text, selector) != LXP_OK || memcmp(selector, pinned, 4U) != 0 ||
        memcmp(lxp_paxeer_anchor_address, anchor, 20U) != 0)
        return 4;
    checkpoints = strstr((const char *)vectors, "\"checkpoints\"");
    if (checkpoints == NULL || strstr(checkpoints, "\"name\":\"batch_1_quorum\"") == NULL) return 5;
    checkpoints = strstr(checkpoints, "\"name\":\"batch_1_quorum\"");
    header = hex_field(checkpoints, "header", &header_length);
    signature = hex_field(checkpoints, "header_signature", &signature_length);
    payload = hex_field(checkpoints, "certificate", &payload_length);
    if (header == NULL || signature == NULL || payload == NULL || header_length != HEADER_BYTES ||
        signature_length != 64U || payload_length < 6U + HEADER_BYTES + 4U + 1U + REFERENCE_BYTES)
        return 6;
    if (be(payload, 2U) != LXP_PAXEER_CERTIFICATE_WIRE_VERSION || be(payload + 2U, 4U) != HEADER_BYTES ||
        memcmp(payload + 6U, header, HEADER_BYTES) != 0 ||
        lxp_batch_header_decode(header, HEADER_BYTES, &certificate.checkpoint.header) != LXP_OK)
        return 7;
    cursor = 6U + HEADER_BYTES;
    proof_length = (size_t)be(payload + cursor, 4U);
    cursor += 4U;
    certificate.checkpoint.validity_proof = (lxp_byte_span){proof_length == 0U ? NULL : payload + cursor, proof_length};
    cursor += proof_length;
    certificate.attestation_count = payload[cursor++];
    if (certificate.attestation_count == 0U || certificate.attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        payload_length != cursor + certificate.attestation_count * ATTESTATION_BYTES + 3U + REFERENCE_BYTES)
        return 8;
    for (i = 0U; i < certificate.attestation_count; ++i) {
        const uint8_t *w = payload + cursor;
        lxp_guarantor_attestation *a = &certificate.attestations[i];
        a->protocol_version = (uint16_t)be(w, 2U);
        a->network_id = (uint32_t)be(w + 2U, 4U);
        a->paxeer_chain_id = be(w + 6U, 8U);
        (void)memcpy(a->paxeer_settlement_contract, w + 14U, 20U);
        a->epoch = be(w + 34U, 8U);
        (void)memcpy(a->checkpoint_id, w + 42U, 32U);
        (void)memcpy(a->checkpoint_hash, w + 74U, 32U);
        (void)memcpy(a->guarantor_id, w + 106U, 32U);
        a->batch_number = be(w + 138U, 8U);
        (void)memcpy(a->data_availability_root, w + 146U, 32U);
        a->replayed = w[178] == 1U;
        a->da_possessed = w[179] == 1U;
        a->availability_class_mask = w[180];
        a->attested_at_ms = be(w + 181U, 8U);
        (void)memcpy(a->signer, w + 189U, 20U);
        (void)memcpy(a->signature, w + 209U, 64U);
        a->signature_v = w[273];
        if (memcmp(a->paxeer_settlement_contract, anchor, 20U) != 0) return 9;
        cursor += ATTESTATION_BYTES;
    }
    certificate.threshold = payload[cursor++];
    if (be(payload + cursor, 2U) != REFERENCE_BYTES) return 10;
    if (lxp_arena_init(&arena, memory, sizeof(memory)) != LXP_OK ||
        lxp_paxeer_checkpoint_certificate_encode(&certificate, &arena, &encoded) != LXP_OK)
        return 11;
    if (encoded.length != cursor + 2U || memcmp(encoded.bytes, payload, cursor) != 0 ||
        encoded.bytes[cursor] != 0U || encoded.bytes[cursor + 1U] != 0U)
        return 12;
    if (lxp_checkpoint_submit_calldata(&certificate, signature, &arena, &calldata) != LXP_OK ||
        memcmp(calldata.bytes, pinned, 4U) != 0)
        return 13;
    lengths[0] = HEADER_BYTES; lengths[1] = 64U; lengths[2] = encoded.length;
    tail = 96U;
    for (i = 0U; i < 3U; ++i) {
        const uint8_t *word = calldata.bytes + 4U + 32U * i;
        size_t padded = (lengths[i] + 31U) / 32U * 32U, pad;
        const uint8_t *content = i == 0U ? header : i == 1U ? signature : encoded.bytes;
        offsets[i] = tail;
        if (be(word, 24U) != 0U || be(word + 24U, 8U) != offsets[i] ||
            4U + tail + 32U + padded > calldata.length ||
            be(calldata.bytes + 4U + tail, 24U) != 0U ||
            be(calldata.bytes + 4U + tail + 24U, 8U) != lengths[i] ||
            memcmp(calldata.bytes + 4U + tail + 32U, content, lengths[i]) != 0)
            return 14;
        for (pad = lengths[i]; pad < padded; ++pad)
            if (calldata.bytes[4U + tail + 32U + pad] != 0U) return 15;
        tail += 32U + padded;
    }
    if (calldata.length != 4U + tail) return 16;
    {
        uint8_t zero[64] = {0U};
        lxp_byte_span refused;
        lxp_guarantor_attestation swapped;
        if (lxp_checkpoint_submit_calldata(&certificate, zero, &arena, &refused) == LXP_OK) return 17;
        if (certificate.attestation_count >= 2U) {
            swapped = certificate.attestations[0];
            certificate.attestations[0] = certificate.attestations[1];
            certificate.attestations[1] = swapped;
            if (lxp_paxeer_checkpoint_certificate_encode(&certificate, &arena, &refused) == LXP_OK) return 18;
            certificate.attestations[1] = certificate.attestations[0];
            certificate.attestations[0] = swapped;
        }
        certificate.attestations[0].signature_v = 29U;
        if (lxp_paxeer_checkpoint_certificate_encode(&certificate, &arena, &refused) == LXP_OK) return 19;
        certificate.attestations[0].signature_v = 27U;
        certificate.threshold = certificate.attestation_count + 1U;
        if (lxp_paxeer_checkpoint_certificate_encode(&certificate, &arena, &refused) == LXP_OK) return 20;
    }
    arguments[0] = (lxp_byte_span){NULL, 0U};
    arguments[1] = (lxp_byte_span){header, 33U};
    if (lxp_paxeer_abi_encode_bytes(pinned, arguments, 2U, &arena, &calldata) != LXP_OK ||
        calldata.length != 4U + 64U + 32U + 32U + 64U || be(calldata.bytes + 4U + 24U, 8U) != 64U ||
        be(calldata.bytes + 36U + 24U, 8U) != 96U || be(calldata.bytes + 68U + 24U, 8U) != 0U ||
        be(calldata.bytes + 100U + 24U, 8U) != 33U || memcmp(calldata.bytes + 132U, header, 33U) != 0 ||
        calldata.bytes[165] != 0U)
        return 21;
    (void)puts("anchor submitCheckpoint selector, certificate wire and ABI calldata match the C vectors");
    return 0;
}
