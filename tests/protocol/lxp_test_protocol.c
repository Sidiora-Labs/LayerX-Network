#include "layerx/lxp_arena.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_protocol.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define SETTLEMENT_PATH "contracts/config/checkpoint-settlement.json"
#define VECTOR_DIRECTORY "tests/vectors/checkpoint/"
#define MAX_DOCUMENT_BYTES 65536U
#define VECTOR_GUARANTORS 3U

typedef struct json_view {
    const char *begin;
    const char *end;
} json_view;

typedef struct declared_guarantor {
    uint8_t guarantor_id[32];
    uint8_t signer[20];
    uint8_t public_key[33];
} declared_guarantor;

typedef struct declared_settlement {
    uint64_t maximum_attestation_delay_seconds;
    size_t certificate_threshold;
    uint64_t paxeer_chain_id;
    uint32_t network_id;
    uint8_t settlement_contract[20];
    uint8_t header_prefix[8];
    size_t header_prefix_length;
    declared_guarantor guarantors[VECTOR_GUARANTORS];
} declared_settlement;

static const char *const vector_cases[] = {
    "fresh", "too_early", "too_late", "boundary_low", "boundary_high",
};

static int fail(const char *what)
{
    fprintf(stderr, "lxp_test_protocol: %s\n", what);
    return 1;
}

static int check_tags(void)
{
    lxp_domain_tag_id i;
    lxp_domain_tag_id j;

    for (i = 0U; i < (lxp_domain_tag_id)LXP_DOMAIN_TAG_COUNT; ++i) {
        size_t i_length = 0U;
        const uint8_t *i_tag = lxp_domain_tag(i, &i_length);
        if (i_tag == NULL || i_length == 0U) return 1;
        for (j = 0U; j < (lxp_domain_tag_id)LXP_DOMAIN_TAG_COUNT; ++j) {
            size_t j_length = 0U;
            size_t common;
            const uint8_t *j_tag;
            if (i == j) continue;
            j_tag = lxp_domain_tag(j, &j_length);
            if (j_tag == NULL || j_length == 0U) return 1;
            common = i_length < j_length ? i_length : j_length;
            if (memcmp(i_tag, j_tag, common) == 0) {
                fprintf(stderr, "domain tags %u and %u collide by prefix\n",
                        (unsigned)i, (unsigned)j);
                return 1;
            }
        }
    }
    return 0;
}

static int read_document(const char *path, char *buffer, size_t capacity,
                         json_view *document)
{
    FILE *file = fopen(path, "rb");
    size_t length;
    if (file == NULL) return fail(path);
    length = fread(buffer, 1U, capacity, file);
    if (ferror(file) != 0 || length == 0U || length == capacity) {
        (void)fclose(file);
        return fail(path);
    }
    (void)fclose(file);
    document->begin = buffer;
    document->end = buffer + length;
    return 0;
}

static const char *json_skip_ws(const char *p, const char *end)
{
    while (p < end && (*p == ' ' || *p == '\n' || *p == '\r' || *p == '\t'))
        ++p;
    return p;
}

static const char *json_skip_string(const char *p, const char *end)
{
    if (p >= end || *p != '"') return NULL;
    ++p;
    while (p < end) {
        if (*p == '\\') {
            p += 2;
            continue;
        }
        if (*p == '"') return p + 1;
        ++p;
    }
    return NULL;
}

static const char *json_skip_value(const char *p, const char *end);

static const char *json_skip_container(const char *p, const char *end,
                                       char open, char close)
{
    if (p >= end || *p != open) return NULL;
    p = json_skip_ws(p + 1, end);
    if (p < end && *p == close) return p + 1;
    for (;;) {
        if (open == '{') {
            p = json_skip_string(p, end);
            if (p == NULL) return NULL;
            p = json_skip_ws(p, end);
            if (p >= end || *p != ':') return NULL;
            p = json_skip_ws(p + 1, end);
        }
        p = json_skip_value(p, end);
        if (p == NULL) return NULL;
        p = json_skip_ws(p, end);
        if (p >= end) return NULL;
        if (*p == close) return p + 1;
        if (*p != ',') return NULL;
        p = json_skip_ws(p + 1, end);
    }
}

static const char *json_skip_value(const char *p, const char *end)
{
    if (p >= end) return NULL;
    switch (*p) {
    case '"':
        return json_skip_string(p, end);
    case '{':
        return json_skip_container(p, end, '{', '}');
    case '[':
        return json_skip_container(p, end, '[', ']');
    default:
        break;
    }
    while (p < end && *p != ',' && *p != '}' && *p != ']' && *p != ' ' &&
           *p != '\n' && *p != '\r' && *p != '\t')
        ++p;
    return p;
}

static int json_find(json_view document, const char *path, json_view *out)
{
    const char *p = json_skip_ws(document.begin, document.end);
    const char *end = document.end;
    const char *segment = path;
    while (*segment != '\0') {
        const char *segment_end = strchr(segment, '.');
        size_t segment_length = segment_end == NULL ? strlen(segment)
                                                    : (size_t)(segment_end - segment);
        int found = 0;
        if (p >= end) return 1;
        if (*p == '{') {
            p = json_skip_ws(p + 1, end);
            while (p < end && *p != '}') {
                const char *key_begin = p + 1;
                const char *key_end;
                const char *after = json_skip_string(p, end);
                if (after == NULL) return 1;
                key_end = after - 1;
                p = json_skip_ws(after, end);
                if (p >= end || *p != ':') return 1;
                p = json_skip_ws(p + 1, end);
                if ((size_t)(key_end - key_begin) == segment_length &&
                    memcmp(key_begin, segment, segment_length) == 0) {
                    found = 1;
                    break;
                }
                p = json_skip_value(p, end);
                if (p == NULL) return 1;
                p = json_skip_ws(p, end);
                if (p < end && *p == ',') p = json_skip_ws(p + 1, end);
            }
        } else if (*p == '[') {
            size_t wanted = 0U;
            size_t index = 0U;
            size_t k;
            for (k = 0U; k < segment_length; ++k) {
                if (segment[k] < '0' || segment[k] > '9') return 1;
                wanted = wanted * 10U + (size_t)(segment[k] - '0');
            }
            p = json_skip_ws(p + 1, end);
            while (p < end && *p != ']') {
                if (index == wanted) {
                    found = 1;
                    break;
                }
                p = json_skip_value(p, end);
                if (p == NULL) return 1;
                p = json_skip_ws(p, end);
                if (p < end && *p == ',') p = json_skip_ws(p + 1, end);
                ++index;
            }
        }
        if (!found) return 1;
        segment = segment_end == NULL ? segment + segment_length
                                      : segment_end + 1;
    }
    out->begin = p;
    out->end = json_skip_value(p, end);
    return out->end == NULL ? 1 : 0;
}

static int json_u64(json_view document, const char *path, uint64_t *value)
{
    json_view view;
    const char *p;
    uint64_t accumulated = 0U;
    if (json_find(document, path, &view) != 0 || view.begin == view.end)
        return 1;
    for (p = view.begin; p < view.end; ++p) {
        if (*p < '0' || *p > '9') return 1;
        if (accumulated > (UINT64_MAX - (uint64_t)(*p - '0')) / 10U) return 1;
        accumulated = accumulated * 10U + (uint64_t)(*p - '0');
    }
    *value = accumulated;
    return 0;
}

static int json_bool(json_view document, const char *path, bool *value)
{
    json_view view;
    size_t length;
    if (json_find(document, path, &view) != 0) return 1;
    length = (size_t)(view.end - view.begin);
    if (length == 4U && memcmp(view.begin, "true", 4U) == 0) {
        *value = true;
        return 0;
    }
    if (length == 5U && memcmp(view.begin, "false", 5U) == 0) {
        *value = false;
        return 0;
    }
    return 1;
}

static int json_string(json_view document, const char *path, json_view *text)
{
    json_view view;
    if (json_find(document, path, &view) != 0 ||
        view.end - view.begin < 2 || *view.begin != '"' ||
        view.end[-1] != '"')
        return 1;
    text->begin = view.begin + 1;
    text->end = view.end - 1;
    return 0;
}

static int json_string_equals(json_view document, const char *path,
                              const char *expected)
{
    json_view text;
    if (json_string(document, path, &text) != 0) return 0;
    return (size_t)(text.end - text.begin) == strlen(expected) &&
           memcmp(text.begin, expected, strlen(expected)) == 0;
}

static int hex_nibble(char value, uint8_t *nibble)
{
    if (value >= '0' && value <= '9') *nibble = (uint8_t)(value - '0');
    else if (value >= 'a' && value <= 'f') *nibble = (uint8_t)(value - 'a' + 10);
    else if (value >= 'A' && value <= 'F') *nibble = (uint8_t)(value - 'A' + 10);
    else return 1;
    return 0;
}

static int json_hex(json_view document, const char *path, uint8_t *out,
                    size_t capacity, size_t *length)
{
    json_view text;
    size_t digits;
    size_t i;
    if (json_string(document, path, &text) != 0) return 1;
    if (text.end - text.begin < 2 || text.begin[0] != '0' ||
        text.begin[1] != 'x')
        return 1;
    text.begin += 2;
    digits = (size_t)(text.end - text.begin);
    if (digits % 2U != 0U || digits / 2U > capacity) return 1;
    for (i = 0U; i < digits / 2U; ++i) {
        uint8_t high;
        uint8_t low;
        if (hex_nibble(text.begin[2U * i], &high) != 0 ||
            hex_nibble(text.begin[2U * i + 1U], &low) != 0)
            return 1;
        out[i] = (uint8_t)((high << 4U) | low);
    }
    *length = digits / 2U;
    return 0;
}

static int json_hex_exact(json_view document, const char *path, uint8_t *out,
                          size_t expected)
{
    size_t length = 0U;
    if (json_hex(document, path, out, expected, &length) != 0 ||
        length != expected)
        return 1;
    return 0;
}

static int json_u64_field(json_view document, const char *path,
                          uint64_t *value)
{
    return json_u64(document, path, value);
}

static int check_declared_tag(json_view settlement, const char *path,
                              lxp_domain_tag_id id)
{
    json_view text;
    size_t tag_length = 0U;
    const uint8_t *tag = lxp_domain_tag(id, &tag_length);
    if (tag == NULL || json_string(settlement, path, &text) != 0) return 1;
    if (tag_length != (size_t)(text.end - text.begin) + 1U ||
        memcmp(tag, text.begin, tag_length - 1U) != 0 ||
        tag[tag_length - 1U] != 0U)
        return 1;
    return 0;
}

static int load_settlement(json_view document, declared_settlement *settlement)
{
    uint64_t value;
    size_t i;
    (void)memset(settlement, 0, sizeof(*settlement));
    if (!json_string_equals(document, "schema",
                            "layerx/checkpoint-settlement/1"))
        return fail("settlement schema");
    if (json_u64(document, "protocol_version", &value) != 0 ||
        value != (uint64_t)LXP_PROTOCOL_VERSION)
        return fail("settlement protocol version");
    if (check_declared_tag(document, "checkpoint_certificate_domain",
                           LXP_DOMAIN_CHECKPOINT_CERTIFICATE) != 0)
        return fail("checkpoint certificate domain differs from the native tag");
    if (check_declared_tag(document, "guarantor_attestation_domain",
                           LXP_DOMAIN_GUARANTOR_ATTESTATION) != 0)
        return fail("guarantor attestation domain differs from the native tag");
    if (json_hex(document, "header_encoding_prefix", settlement->header_prefix,
                 sizeof(settlement->header_prefix),
                 &settlement->header_prefix_length) != 0 ||
        settlement->header_prefix_length == 0U)
        return fail("header encoding prefix");
    if (json_u64(document, "header_length", &value) != 0 || value != 354U)
        return fail("header length");
    if (json_u64(document, "finality_policy.maximum_attestation_delay_seconds",
                 &settlement->maximum_attestation_delay_seconds) != 0 ||
        settlement->maximum_attestation_delay_seconds == 0U)
        return fail("maximum attestation delay");
    if (lxp_checkpoint_maximum_attestation_delay_ms() !=
        settlement->maximum_attestation_delay_seconds * UINT64_C(1000))
        return fail("native maximum attestation delay differs from the declared value");
    if (json_u64(document, "finality_policy.certificate_threshold", &value) != 0 ||
        value == 0U || value > VECTOR_GUARANTORS)
        return fail("certificate threshold");
    settlement->certificate_threshold = (size_t)value;
    if (json_u64(document, "settlement_domains.vectors.paxeer_chain_id",
                 &settlement->paxeer_chain_id) != 0 ||
        settlement->paxeer_chain_id == 0U)
        return fail("vector chain id");
    if (json_u64(document, "settlement_domains.vectors.network_id", &value) != 0 ||
        value == 0U || value > UINT32_MAX)
        return fail("vector network id");
    settlement->network_id = (uint32_t)value;
    if (json_hex_exact(document, "settlement_domains.vectors.settlement_contract",
                       settlement->settlement_contract, 20U) != 0)
        return fail("vector settlement contract");
    for (i = 0U; i < VECTOR_GUARANTORS; ++i) {
        char path[96];
        declared_guarantor *guarantor = &settlement->guarantors[i];
        (void)snprintf(path, sizeof(path),
                       "settlement_domains.vectors.guarantor_set.%u.guarantor_id",
                       (unsigned)i);
        if (json_hex_exact(document, path, guarantor->guarantor_id, 32U) != 0)
            return fail("vector guarantor id");
        (void)snprintf(path, sizeof(path),
                       "settlement_domains.vectors.guarantor_set.%u.signer",
                       (unsigned)i);
        if (json_hex_exact(document, path, guarantor->signer, 20U) != 0)
            return fail("vector guarantor signer");
        (void)snprintf(path, sizeof(path),
                       "settlement_domains.vectors.guarantor_set.%u.public_key",
                       (unsigned)i);
        if (json_hex_exact(document, path, guarantor->public_key, 33U) != 0)
            return fail("vector guarantor public key");
        if (i != 0U &&
            memcmp(settlement->guarantors[i - 1U].guarantor_id,
                   guarantor->guarantor_id, 32U) >= 0)
            return fail("vector guarantor set is not sorted");
    }
    {
        json_view extra;
        if (json_find(document, "settlement_domains.vectors.guarantor_set.3",
                      &extra) == 0)
            return fail("vector guarantor set size");
    }
    return 0;
}

static int load_header(json_view vector, lxp_batch_header *header)
{
    uint64_t value;
    (void)memset(header, 0, sizeof(*header));
    if (json_u64_field(vector, "header.protocol_version", &value) != 0 ||
        value > UINT16_MAX)
        return 1;
    header->protocol_version = (uint16_t)value;
    if (json_u64_field(vector, "header.network_id", &value) != 0 ||
        value > UINT32_MAX)
        return 1;
    header->network_id = (uint32_t)value;
    if (json_u64_field(vector, "header.epoch", &header->epoch) != 0 ||
        json_u64_field(vector, "header.batch_number", &header->batch_number) != 0 ||
        json_u64_field(vector, "header.first_sequence", &header->first_sequence) != 0 ||
        json_u64_field(vector, "header.last_sequence", &header->last_sequence) != 0 ||
        json_hex_exact(vector, "header.previous_state_root",
                       header->previous_state_root, 32U) != 0 ||
        json_hex_exact(vector, "header.resulting_state_root",
                       header->resulting_state_root, 32U) != 0 ||
        json_hex_exact(vector, "header.activity_merkle_root",
                       header->activity_merkle_root, 32U) != 0 ||
        json_hex_exact(vector, "header.receipt_merkle_root",
                       header->receipt_merkle_root, 32U) != 0 ||
        json_hex_exact(vector, "header.event_merkle_root",
                       header->event_merkle_root, 32U) != 0 ||
        json_hex_exact(vector, "header.data_availability_root",
                       header->data_availability_root, 32U) != 0 ||
        json_hex_exact(vector, "header.oracle_root", header->oracle_root,
                       32U) != 0 ||
        json_u64_field(vector, "header.timestamp_ms", &header->timestamp_ms) != 0 ||
        json_hex_exact(vector, "header.sequencer_id", header->sequencer_id,
                       32U) != 0)
        return 1;
    return 0;
}

static int load_attestation(json_view vector, size_t index,
                            const declared_settlement *settlement,
                            const lxp_batch_header *header,
                            const uint8_t checkpoint_id[32],
                            lxp_guarantor_attestation *attestation)
{
    char path[64];
    uint64_t value;
    bool flag;
    uint8_t message[189];
    (void)memset(attestation, 0, sizeof(*attestation));
    attestation->protocol_version = header->protocol_version;
    attestation->network_id = header->network_id;
    attestation->paxeer_chain_id = settlement->paxeer_chain_id;
    (void)memcpy(attestation->paxeer_settlement_contract,
                 settlement->settlement_contract, 20U);
    attestation->epoch = header->epoch;
    (void)memcpy(attestation->checkpoint_id, checkpoint_id, 32U);
    (void)memcpy(attestation->checkpoint_hash, checkpoint_id, 32U);
    (void)snprintf(path, sizeof(path), "attestations.%u.guarantor_id",
                   (unsigned)index);
    if (json_hex_exact(vector, path, attestation->guarantor_id, 32U) != 0)
        return 1;
    attestation->batch_number = header->batch_number;
    (void)memcpy(attestation->data_availability_root,
                 header->data_availability_root, 32U);
    (void)snprintf(path, sizeof(path), "attestations.%u.replayed",
                   (unsigned)index);
    if (json_bool(vector, path, &flag) != 0) return 1;
    attestation->replayed = flag;
    (void)snprintf(path, sizeof(path), "attestations.%u.data_possessed",
                   (unsigned)index);
    if (json_bool(vector, path, &flag) != 0) return 1;
    attestation->da_possessed = flag;
    (void)snprintf(path, sizeof(path),
                   "attestations.%u.availability_class_mask", (unsigned)index);
    if (json_u64(vector, path, &value) != 0 || value > UINT8_MAX) return 1;
    attestation->availability_class_mask = (uint8_t)value;
    (void)snprintf(path, sizeof(path), "attestations.%u.attested_at_ms",
                   (unsigned)index);
    if (json_u64(vector, path, &attestation->attested_at_ms) != 0) return 1;
    (void)snprintf(path, sizeof(path), "attestations.%u.signer",
                   (unsigned)index);
    if (json_hex_exact(vector, path, attestation->signer, 20U) != 0) return 1;
    (void)snprintf(path, sizeof(path), "attestations.%u.signature",
                   (unsigned)index);
    if (json_hex_exact(vector, path, attestation->signature, 64U) != 0)
        return 1;
    (void)snprintf(path, sizeof(path), "attestations.%u.signature_v",
                   (unsigned)index);
    if (json_u64(vector, path, &value) != 0 || (value != 27U && value != 28U))
        return 1;
    attestation->signature_v = (uint8_t)value;
    (void)snprintf(path, sizeof(path), "attestations.%u.message",
                   (unsigned)index);
    if (json_hex_exact(vector, path, message, sizeof(message)) != 0) return 1;
    if (memcmp(message + 42U, checkpoint_id, 32U) != 0 ||
        memcmp(message + 74U, checkpoint_id, 32U) != 0 ||
        memcmp(message + 106U, attestation->guarantor_id, 32U) != 0 ||
        memcmp(message + 14U, settlement->settlement_contract, 20U) != 0)
        return 1;
    if (memcmp(attestation->guarantor_id,
               settlement->guarantors[index].guarantor_id, 32U) != 0 ||
        memcmp(attestation->signer, settlement->guarantors[index].signer,
               20U) != 0)
        return 1;
    return 0;
}

static int run_vector(const char *name, const declared_settlement *settlement,
                      lxp_arena *arena)
{
    static char buffer[MAX_DOCUMENT_BYTES];
    static uint8_t validity_proof[LXP_MAX_VALIDITY_PROOF_BYTES];
    char path[128];
    json_view vector;
    lxp_batch_header header;
    lxp_batch_header decoded;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation attestations[VECTOR_GUARANTORS];
    lxp_guarantor_key_record keys[VECTOR_GUARANTORS];
    lxp_guarantor_cert certificate;
    lxp_guarantor_set set;
    lxp_finalisation_state state;
    lxp_finalisation_requirements requirements;
    lxp_byte_span encoded;
    uint8_t header_bytes[354];
    uint8_t expected_digest[32];
    uint8_t checkpoint_id[32];
    size_t proof_length = 0U;
    size_t valid = 0U;
    size_t mark;
    size_t i;
    bool accept;
    bool finalisable = false;
    lxp_result status;
    uint64_t threshold;

    (void)snprintf(path, sizeof(path), VECTOR_DIRECTORY "%s.json", name);
    if (read_document(path, buffer, sizeof(buffer), &vector) != 0) return 1;
    if (!json_string_equals(vector, "schema", "layerx/checkpoint-vector/1") ||
        !json_string_equals(vector, "case", name) ||
        !json_string_equals(vector, "settlement_domain", "vectors"))
        return fail("vector identity");
    if (json_string_equals(vector, "expected_outcome", "accept")) accept = true;
    else if (json_string_equals(vector, "expected_outcome", "reject"))
        accept = false;
    else return fail("vector expected outcome");
    if (load_header(vector, &header) != 0) return fail("vector header fields");
    if (header.network_id != settlement->network_id)
        return fail("vector header network id differs from the declared domain");
    if (json_hex_exact(vector, "header.bytes", header_bytes,
                       sizeof(header_bytes)) != 0)
        return fail("vector header bytes");
    if (json_hex(vector, "certificate.validity_proof", validity_proof,
                 sizeof(validity_proof), &proof_length) != 0)
        return fail("vector validity proof");
    if (json_u64(vector, "certificate.threshold", &threshold) != 0 ||
        threshold != settlement->certificate_threshold)
        return fail("vector threshold differs from the declared policy");
    if (json_hex_exact(vector, "expected_digest", expected_digest, 32U) != 0)
        return fail("vector expected digest");

    mark = lxp_arena_mark(arena);
    if (lxp_batch_header_encode(&header, arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(header_bytes) ||
        memcmp(encoded.bytes, header_bytes, sizeof(header_bytes)) != 0)
        return fail("native header encoding differs from the vector bytes");
    if (memcmp(header_bytes, settlement->header_prefix,
               settlement->header_prefix_length) != 0)
        return fail("native header prefix differs from the declared prefix");
    if (lxp_batch_header_decode(header_bytes, sizeof(header_bytes),
                                &decoded) != LXP_OK ||
        memcmp(&decoded, &header, sizeof(header)) != 0)
        return fail("native header decode differs from the vector fields");
    (void)lxp_arena_reset(arena, mark);

    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header = header;
    checkpoint.validity_proof = (lxp_byte_span){validity_proof, proof_length};
    if (lxp_checkpoint_certificate_hash(&checkpoint, arena, checkpoint_id) !=
            LXP_OK ||
        memcmp(checkpoint_id, expected_digest, 32U) != 0)
        return fail("native checkpoint identity differs from the expected digest");

    for (i = 0U; i < VECTOR_GUARANTORS; ++i) {
        if (load_attestation(vector, i, settlement, &header, checkpoint_id,
                             &attestations[i]) != 0)
            return fail("vector attestation fields");
        if (lxp_guarantor_attestation_verify(
                &attestations[i], settlement->guarantors[i].public_key) !=
            LXP_OK)
            return fail("vector attestation signature does not verify natively");
        (void)memcpy(keys[i].guarantor_id,
                     settlement->guarantors[i].guarantor_id, 32U);
        (void)memcpy(keys[i].public_key,
                     settlement->guarantors[i].public_key, 33U);
        keys[i].bonded = true;
    }
    {
        json_view extra;
        if (json_find(vector, "attestations.3", &extra) == 0)
            return fail("vector attestation count");
    }
    if (lxp_guarantor_cert_assemble(&checkpoint, attestations,
                                    VECTOR_GUARANTORS,
                                    settlement->certificate_threshold,
                                    &certificate) != LXP_OK)
        return fail("native certificate assembly");
    status = lxp_guarantor_cert_verify(&certificate, keys, VECTOR_GUARANTORS,
                                       arena, &valid);
    if (accept) {
        if (!json_string_equals(vector, "expected_rejection", "none"))
            return fail("accept vector names a rejection");
        if (status != LXP_OK || valid != VECTOR_GUARANTORS)
            return fail("native verifier rejected an expected-accept vector");
    } else {
        lxp_result expected;
        if (json_string_equals(vector, "expected_rejection", "not_yet_valid"))
            expected = LXP_ERR_NOT_YET_VALID;
        else if (json_string_equals(vector, "expected_rejection", "expired"))
            expected = LXP_ERR_EXPIRED;
        else return fail("vector expected rejection");
        if (status != expected)
            return fail("native verifier status differs from the expected rejection");
    }

    if (lxp_guarantor_set_init(&set) != LXP_OK) return fail("guarantor set");
    for (i = 0U; i < VECTOR_GUARANTORS; ++i) {
        lxp_guarantor_bond_state bond;
        (void)memset(&bond, 0, sizeof(bond));
        (void)memcpy(bond.guarantor_id,
                     settlement->guarantors[i].guarantor_id, 32U);
        (void)memcpy(bond.public_key, settlement->guarantors[i].public_key,
                     33U);
        bond.bond_amount = (lxp_u128){0U, 1000U};
        bond.joined_epoch = 1U;
        bond.active = true;
        if (lxp_guarantor_set_apply(&set, i + 1U, true, &bond) != LXP_OK)
            return fail("guarantor set application");
    }
    (void)memset(&state, 0, sizeof(state));
    (void)memcpy(state.settlement_anchor, header.previous_state_root, 32U);
    (void)memset(&requirements, 0, sizeof(requirements));
    requirements.checkpoint_epoch = header.epoch;
    requirements.challenge_window_end_ms = header.timestamp_ms;
    requirements.checkpoint_deadline_ms =
        header.timestamp_ms + lxp_checkpoint_maximum_attestation_delay_ms();
    requirements.now_ms = requirements.checkpoint_deadline_ms + 1U;
    requirements.threshold = settlement->certificate_threshold;
    requirements.minimum_bond = (lxp_u128){0U, 500U};
    requirements.availability_challenges_answered = true;
    status = lxp_checkpoint_finalisable(&state, &certificate, &set,
                                        &requirements, arena, &finalisable);
    if (accept) {
        if (status != LXP_OK || !finalisable ||
            memcmp(state.settlement_anchor, header.resulting_state_root,
                   32U) != 0)
            return fail("native finalisation rejected an expected-accept vector");
    } else if (status == LXP_OK || finalisable) {
        return fail("native finalisation accepted an expected-reject vector");
    }
    return 0;
}

static int check_checkpoint_vectors(void)
{
    static char buffer[MAX_DOCUMENT_BYTES];
    static uint8_t arena_buffer[65536];
    json_view document;
    declared_settlement settlement;
    lxp_arena arena;
    size_t i;
    if (read_document(SETTLEMENT_PATH, buffer, sizeof(buffer), &document) != 0)
        return 1;
    if (load_settlement(document, &settlement) != 0) return 1;
    if (lxp_arena_init(&arena, arena_buffer, sizeof(arena_buffer)) != LXP_OK)
        return fail("arena");
    for (i = 0U; i < sizeof(vector_cases) / sizeof(vector_cases[0]); ++i)
        if (run_vector(vector_cases[i], &settlement, &arena) != 0) return 1;
    return 0;
}

int main(void)
{
    size_t ignored = 0U;
    if (!lxp_protocol_version_supported((uint16_t)LXP_PROTOCOL_VERSION) ||
        lxp_protocol_version_supported(UINT16_C(0)) ||
        lxp_protocol_version_supported(UINT16_MAX)) return 1;
    if (!lxp_network_id_matches(UINT32_C(17), UINT32_C(17)) ||
        lxp_network_id_matches(UINT32_C(17), UINT32_C(18)) ||
        lxp_network_id_matches(UINT32_C(0), UINT32_C(0))) return 1;
    if (LXP_MAX_ACTIVITY_BYTES == 0 || LXP_MAX_ACTIVITY_BYTES > UINT32_MAX ||
        LXP_MAX_PAYLOAD_BYTES == 0 || LXP_MAX_PAYLOAD_BYTES > UINT32_MAX ||
        LXP_MAX_DID_LENGTH == 0 || LXP_MAX_DID_LENGTH > UINT16_MAX ||
        LXP_MAX_AUTHORITY_CHAIN_DEPTH == 0 ||
        LXP_MAX_AUTHORITY_CHAIN_DEPTH > UINT8_MAX ||
        LXP_MAX_TRANSFER_SET_LEGS == 0 ||
        LXP_MAX_TRANSFER_SET_LEGS > UINT16_MAX ||
        LXP_MAX_EFFECTS == 0 || LXP_MAX_EFFECTS > UINT16_MAX ||
        LXP_MAX_BATCH_ACTIVITIES == 0 ||
        LXP_MAX_BATCH_ACTIVITIES > UINT32_MAX) return 1;
    if (lxp_domain_tag((lxp_domain_tag_id)LXP_DOMAIN_TAG_COUNT, &ignored) != NULL ||
        lxp_domain_tag(LXP_DOMAIN_ACTIVITY_ID, NULL) != NULL) return 1;
    if (check_tags() != 0) return 1;
    return check_checkpoint_vectors();
}
