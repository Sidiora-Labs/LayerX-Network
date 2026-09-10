#define _POSIX_C_SOURCE 200809L

#include "lxp_verify_cli.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_verify.h"

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

enum {
    VERIFY_ARENA_BYTES = 4 * 1024 * 1024,
    VERIFY_EXIT_OK = 0,
    VERIFY_EXIT_REFUSED = 1,
    VERIFY_EXIT_USAGE = 2
};

typedef struct verify_reader {
    const uint8_t *bytes;
    size_t length;
    size_t cursor;
} verify_reader;

typedef struct verify_signed_header {
    uint8_t sequencer_id[32];
    uint8_t public_key[32];
    uint64_t first_batch_number;
    uint64_t last_batch_number;
    uint8_t canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t signature[64];
} verify_signed_header;

static lxp_result reader_take(verify_reader *reader, size_t length,
                              const uint8_t **bytes)
{
    if (reader == NULL || bytes == NULL || reader->cursor > reader->length ||
        length > reader->length - reader->cursor)
        return LXP_ERR_TRUNCATED;
    *bytes = reader->bytes + reader->cursor;
    reader->cursor += length;
    return LXP_OK;
}

static lxp_result reader_u8(verify_reader *reader, uint8_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 1U, &bytes);
    if (status == LXP_OK) *value = bytes[0];
    return status;
}

static lxp_result reader_u16(verify_reader *reader, uint16_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 2U, &bytes);
    if (status == LXP_OK)
        *value = (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
    return status;
}

static lxp_result reader_u32(verify_reader *reader, uint32_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 4U, &bytes);
    size_t index;
    if (status != LXP_OK) return status;
    *value = 0U;
    for (index = 0U; index < 4U; ++index)
        *value = (*value << 8U) | (uint32_t)bytes[index];
    return LXP_OK;
}

static lxp_result reader_u64(verify_reader *reader, uint64_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 8U, &bytes);
    size_t index;
    if (status != LXP_OK) return status;
    *value = 0U;
    for (index = 0U; index < 8U; ++index)
        *value = (*value << 8U) | (uint64_t)bytes[index];
    return LXP_OK;
}

static lxp_result reader_copy(verify_reader *reader, uint8_t *output,
                              size_t length)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, length, &bytes);
    if (status == LXP_OK && length != 0U)
        (void)memcpy(output, bytes, length);
    return status;
}

static lxp_result reader_finish(const verify_reader *reader)
{
    return reader != NULL && reader->cursor == reader->length ?
        LXP_OK : LXP_ERR_TRAILING_BYTES;
}

/* Mirrors the depth the daemon writes in cmd/layerxd/lxp_daemon_evidence.c. */
static uint8_t proof_depth(uint32_t count)
{
    uint8_t depth = 0U;
    while (count > 1U) {
        count = (count + 1U) / 2U;
        ++depth;
    }
    return depth;
}

static lxp_result read_merkle_proof(verify_reader *reader,
                                    lxp_merkle_proof *proof)
{
    lxp_result status;
    (void)memset(proof, 0, sizeof(*proof));
    status = reader_u32(reader, &proof->leaf_index);
    if (status == LXP_OK) status = reader_u32(reader, &proof->leaf_count);
    if (status == LXP_OK) status = reader_u8(reader, &proof->depth);
    if (status == LXP_OK &&
        (proof->leaf_count == 0U || proof->leaf_index >= proof->leaf_count ||
         proof->depth > LXP_MERKLE_MAX_DEPTH ||
         proof->depth != proof_depth(proof->leaf_count)))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = reader_copy(reader, proof->siblings[0],
                             (size_t)proof->depth * 32U);
    return status;
}

static lxp_result read_signed_header(verify_reader *reader,
                                     verify_signed_header *signed_header)
{
    uint16_t version = 0U;
    uint32_t header_length = 0U;
    lxp_result status;
    (void)memset(signed_header, 0, sizeof(*signed_header));
    status = reader_u16(reader, &version);
    if (status == LXP_OK && version != (uint16_t)LXP_VERIFY_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_copy(reader, signed_header->sequencer_id, 32U);
    if (status == LXP_OK)
        status = reader_copy(reader, signed_header->public_key, 32U);
    if (status == LXP_OK)
        status = reader_u64(reader, &signed_header->first_batch_number);
    if (status == LXP_OK)
        status = reader_u64(reader, &signed_header->last_batch_number);
    if (status == LXP_OK) status = reader_u32(reader, &header_length);
    if (status == LXP_OK &&
        header_length != (uint32_t)LXP_BATCH_HEADER_ENCODED_SIZE)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = reader_copy(reader, signed_header->canonical_header,
                             (size_t)LXP_BATCH_HEADER_ENCODED_SIZE);
    if (status == LXP_OK)
        status = reader_copy(reader, signed_header->signature, 64U);
    if (status == LXP_OK &&
        (signed_header->first_batch_number == 0U ||
         signed_header->last_batch_number <
             signed_header->first_batch_number ||
         lxp_ct_is_zero(signed_header->sequencer_id, 32U) ||
         lxp_ct_is_zero(signed_header->public_key, 32U)))
        status = LXP_ERR_NON_CANONICAL;
    return status;
}

static void fill_receipt_report(const lxp_receipt *receipt,
                                lxp_verify_receipt_report *report)
{
    (void)memcpy(report->activity_id, receipt->activity_id, 32U);
    (void)memcpy(report->asset, receipt->asset, 32U);
    (void)memcpy(report->previous_state_root,
                 receipt->previous_state_root, 32U);
    (void)memcpy(report->resulting_state_root,
                 receipt->resulting_state_root, 32U);
    (void)memcpy(report->batch_id, receipt->batch_id, 32U);
    report->amount = receipt->amount;
    report->fee_charged = receipt->fee_charged;
    report->global_sequence = receipt->global_sequence;
    report->timestamp = receipt->timestamp;
    report->result_code = receipt->result_code;
    report->protocol_version = receipt->protocol_version;
    report->module_id = receipt->module_id;
    report->operation = receipt->operation;
}

static lxp_result canonical_receipt_bytes(const lxp_receipt *receipt,
                                          const uint8_t *bytes,
                                          size_t length, lxp_arena *arena)
{
    lxp_byte_span encoded;
    size_t mark = lxp_arena_mark(arena);
    lxp_result status = lxp_receipt_encode(receipt, true, arena, &encoded);
    if (status == LXP_OK &&
        (encoded.length != length ||
         lxp_ct_memcmp(encoded.bytes, bytes, length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_verify_trust_root_load(
    const uint8_t *manifest_bytes, size_t manifest_length, lxp_arena *arena,
    lxp_verify_trust_root *trust_root, const char **stage)
{
    lxp_genesis_manifest *manifest;
    void *allocation;
    size_t index;
    size_t mark;
    lxp_result status;
    if (manifest_bytes == NULL || manifest_length == 0U || arena == NULL ||
        trust_root == NULL || stage == NULL)
        return LXP_ERR_NON_CANONICAL;
    *stage = "trust-root-allocate";
    mark = lxp_arena_mark(arena);
    status = lxp_arena_alloc(arena, sizeof(*manifest), _Alignof(uint64_t),
                             &allocation);
    if (status != LXP_OK) return status;
    manifest = (lxp_genesis_manifest *)allocation;
    *stage = "trust-root-parse";
    status = lxp_genesis_parse(manifest_bytes, manifest_length,
                               LXP_GENESIS_INPUT_MANIFEST, manifest);
    if (status == LXP_OK) {
        *stage = "trust-root-signature";
        status = lxp_genesis_verify_signature(manifest, arena);
    }
    if (status == LXP_OK) {
        *stage = "trust-root-bounds";
        if (manifest->network_id == 0U ||
            !lxp_protocol_version_supported(manifest->protocol_version) ||
            manifest->guarantor_count == 0U ||
            manifest->guarantor_count > (size_t)LXP_GENESIS_MAX_GUARANTORS ||
            lxp_ct_is_zero(manifest->signer_public_key, 32U) ||
            lxp_ct_is_zero(manifest->genesis_state_root, 32U))
            status = LXP_ERR_NON_CANONICAL;
    }
    (void)memset(trust_root, 0, sizeof(*trust_root));
    if (status == LXP_OK) {
        *stage = "trust-root-commitment";
        status = lxp_genesis_manifest_commitment(
            manifest, arena, trust_root->manifest_commitment);
    }
    if (status == LXP_OK) {
        trust_root->protocol_version = manifest->protocol_version;
        trust_root->network_id = manifest->network_id;
        trust_root->genesis_timestamp_ms = manifest->genesis_timestamp_ms;
        (void)memcpy(trust_root->authority_public_key,
                     manifest->signer_public_key, 32U);
        (void)memcpy(trust_root->genesis_state_root,
                     manifest->genesis_state_root, 32U);
        for (index = 0U; index < manifest->guarantor_count; ++index) {
            (void)memcpy(trust_root->guarantors[index].guarantor_id,
                         manifest->guarantors[index].guarantor_id, 32U);
            (void)memcpy(trust_root->guarantors[index].public_key,
                         manifest->guarantors[index].public_key, 33U);
            trust_root->guarantors[index].bonded =
                !lxp_u128_is_zero(manifest->guarantors[index].bond);
        }
        trust_root->guarantor_count = manifest->guarantor_count;
        *stage = NULL;
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_verify_receipt_offline_pinned(
    const lxp_verify_trust_root *trust_root, const uint8_t *receipt_bytes,
    size_t receipt_length, lxp_arena *arena,
    lxp_verify_receipt_report *report, const char **stage)
{
    lxp_receipt receipt;
    lxp_result status;
    if (trust_root == NULL || receipt_bytes == NULL || receipt_length == 0U ||
        arena == NULL || report == NULL || stage == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(report, 0, sizeof(*report));
    *stage = "receipt-decode";
    status = lxp_receipt_decode(receipt_bytes, receipt_length, true, &receipt);
    if (status == LXP_OK) {
        *stage = "receipt-canonical";
        status = canonical_receipt_bytes(&receipt, receipt_bytes,
                                         receipt_length, arena);
    }
    if (status == LXP_OK) {
        *stage = "receipt-authority";
        status = lxp_receipt_verify_offline(
            &receipt, trust_root->authority_public_key, arena);
    }
    if (status == LXP_OK) {
        *stage = "receipt-digest";
        status = lxp_receipt_digest(&receipt, arena, report->receipt_digest);
    }
    if (status != LXP_OK) return status;
    fill_receipt_report(&receipt, report);
    *stage = NULL;
    return LXP_OK;
}

lxp_result lxp_verify_bundle_offline(
    const lxp_verify_trust_root *trust_root, const uint8_t *value_bytes,
    size_t value_length, const uint8_t *proof_bytes, size_t proof_length,
    lxp_arena *arena, lxp_verify_bundle_report *report, const char **stage)
{
    verify_reader reader;
    verify_signed_header signed_header;
    lxp_sequencer_authorization authorization;
    lxp_merkle_proof inclusion;
    lxp_batch_header header;
    lxp_activity activity;
    lxp_receipt receipt;
    uint8_t asserted_activity_id[32];
    uint8_t computed_activity_id[32];
    uint16_t version = 0U;
    uint8_t kind = 0U;
    lxp_result status;
    if (trust_root == NULL || value_bytes == NULL || value_length == 0U ||
        proof_bytes == NULL || proof_length == 0U || arena == NULL ||
        report == NULL || stage == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(report, 0, sizeof(*report));
    (void)memset(&authorization, 0, sizeof(authorization));
    (void)memset(&inclusion, 0, sizeof(inclusion));
    (void)memset(&signed_header, 0, sizeof(signed_header));
    (void)memset(&header, 0, sizeof(header));
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&receipt, 0, sizeof(receipt));
    (void)memset(asserted_activity_id, 0, sizeof(asserted_activity_id));
    (void)memset(computed_activity_id, 0, sizeof(computed_activity_id));
    reader = (verify_reader){proof_bytes, proof_length, 0U};
    *stage = "bundle-version";
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != (uint16_t)LXP_VERIFY_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) {
        *stage = "bundle-kind";
        status = reader_u8(&reader, &kind);
    }
    if (status == LXP_OK && kind != (uint8_t)LXP_VERIFY_BUNDLE_ACTIVITY &&
        kind != (uint8_t)LXP_VERIFY_BUNDLE_RECEIPT)
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK) {
        *stage = "bundle-target";
        status = reader_copy(&reader, asserted_activity_id, 32U);
    }
    if (status == LXP_OK && lxp_ct_is_zero(asserted_activity_id, 32U))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        *stage = "bundle-proof";
        status = read_merkle_proof(&reader, &inclusion);
    }
    if (status == LXP_OK) {
        *stage = "bundle-signed-header";
        status = read_signed_header(&reader, &signed_header);
    }
    if (status == LXP_OK) {
        *stage = "bundle-trailing";
        status = reader_finish(&reader);
    }
    if (status == LXP_OK) {
        /*
         * The pin: only the genesis authority key may authorize evidence.
         * A responder-supplied key can never widen the trusted set.
         */
        *stage = "bundle-authority";
        if (lxp_ct_memcmp(signed_header.public_key,
                          trust_root->authority_public_key, 32U) != 0)
            status = LXP_ERR_AUTH_SCOPE;
    }
    if (status == LXP_OK) {
        *stage = "bundle-header";
        status = lxp_batch_header_decode(
            signed_header.canonical_header,
            (size_t)LXP_BATCH_HEADER_ENCODED_SIZE, &header);
    }
    if (status == LXP_OK) {
        *stage = "bundle-network";
        if (header.network_id != trust_root->network_id)
            status = LXP_ERR_WRONG_NETWORK;
        else if (!lxp_protocol_version_supported(header.protocol_version))
            status = LXP_ERR_VERSION_UNSUPPORTED;
        else if (lxp_ct_memcmp(header.sequencer_id,
                               signed_header.sequencer_id, 32U) != 0 ||
                 header.batch_number < signed_header.first_batch_number ||
                 header.batch_number > signed_header.last_batch_number ||
                 header.first_sequence == 0U ||
                 header.last_sequence < header.first_sequence)
            status = LXP_ERR_AUTH_SCOPE;
    }
    if (status == LXP_OK) {
        /*
         * The response's own batch range can never grant broader trust than
         * the header it presents, so the locally built authority is limited
         * to that one batch and carries the pinned key.
         */
        (void)memcpy(authorization.sequencer_id, header.sequencer_id, 32U);
        (void)memcpy(authorization.public_key,
                     trust_root->authority_public_key, 32U);
        authorization.first_batch_number = header.batch_number;
        authorization.last_batch_number = header.batch_number;
        authorization.authorized = 1U;
        *stage = "bundle-header-signature";
        status = lxp_batch_verify_signature(
            &header, signed_header.signature, 64U, &authorization, arena);
    }
    if (status == LXP_OK) {
        *stage = "bundle-header-hash";
        status = lxp_batch_header_hash(&header, arena, report->header_hash);
    }
    if (status == LXP_OK) {
        *stage = "bundle-leaf";
        status = lxp_merkle_leaf_hash(value_bytes, value_length,
                                      report->leaf_hash);
    }
    if (status == LXP_OK) {
        *stage = "bundle-inclusion";
        status = lxp_merkle_proof_verify(
            report->leaf_hash, &inclusion,
            kind == (uint8_t)LXP_VERIFY_BUNDLE_ACTIVITY ?
                header.activity_merkle_root : header.receipt_merkle_root);
    }
    if (status == LXP_OK && kind == (uint8_t)LXP_VERIFY_BUNDLE_ACTIVITY) {
        *stage = "bundle-activity";
        status = lxp_activity_decode(value_bytes, value_length, &activity);
        if (status == LXP_OK)
            status = lxp_activity_check_envelope(&activity,
                                                 trust_root->network_id);
        if (status == LXP_OK)
            status = lxp_activity_verify_payload_hash(&activity);
        if (status == LXP_OK)
            status = lxp_activity_verify_signature(&activity);
        if (status == LXP_OK)
            status = lxp_activity_id(value_bytes, value_length,
                                     computed_activity_id);
        if (status == LXP_OK &&
            lxp_ct_memcmp(computed_activity_id, asserted_activity_id,
                          32U) != 0)
            status = LXP_ERR_ROOT_MISMATCH;
    }
    if (status == LXP_OK && kind == (uint8_t)LXP_VERIFY_BUNDLE_RECEIPT) {
        *stage = "bundle-receipt";
        status = lxp_receipt_decode(value_bytes, value_length, true,
                                    &receipt);
        if (status == LXP_OK)
            status = canonical_receipt_bytes(&receipt, value_bytes,
                                             value_length, arena);
        if (status == LXP_OK)
            status = lxp_receipt_verify(
                &receipt, trust_root->authority_public_key, arena);
        if (status == LXP_OK)
            status = lxp_receipt_digest(&receipt, arena,
                                        report->receipt.receipt_digest);
        if (status == LXP_OK &&
            (lxp_ct_memcmp(receipt.activity_id, asserted_activity_id,
                           32U) != 0 ||
             receipt.global_sequence < header.first_sequence ||
             receipt.global_sequence > header.last_sequence ||
             receipt.timestamp != header.timestamp_ms))
            status = LXP_ERR_ROOT_MISMATCH;
        if (status == LXP_OK) {
            fill_receipt_report(&receipt, &report->receipt);
            report->receipt_present = true;
        }
    }
    if (status != LXP_OK) return status;
    report->kind = kind;
    (void)memcpy(report->activity_id, asserted_activity_id, 32U);
    (void)memcpy(report->sequencer_id, header.sequencer_id, 32U);
    (void)memcpy(report->previous_state_root, header.previous_state_root,
                 32U);
    (void)memcpy(report->resulting_state_root, header.resulting_state_root,
                 32U);
    report->leaf_index = inclusion.leaf_index;
    report->leaf_count = inclusion.leaf_count;
    report->network_id = header.network_id;
    report->protocol_version = header.protocol_version;
    report->batch_number = header.batch_number;
    report->first_sequence = header.first_sequence;
    report->last_sequence = header.last_sequence;
    report->timestamp_ms = header.timestamp_ms;
    *stage = NULL;
    return LXP_OK;
}

static lxp_result read_stream(int descriptor, uint8_t *buffer,
                              size_t capacity, size_t *length)
{
    size_t offset = 0U;
    for (;;) {
        ssize_t taken;
        if (offset == capacity) {
            uint8_t excess;
            taken = read(descriptor, &excess, 1U);
            if (taken < 0 && errno == EINTR) continue;
            if (taken < 0) return LXP_ERR_IO;
            if (taken == 0) break;
            return LXP_ERR_LENGTH_LIMIT;
        }
        taken = read(descriptor, buffer + offset, capacity - offset);
        if (taken < 0) {
            if (errno == EINTR) continue;
            return LXP_ERR_IO;
        }
        if (taken == 0) break;
        offset += (size_t)taken;
    }
    *length = offset;
    return offset == 0U ? LXP_ERR_TRUNCATED : LXP_OK;
}

static lxp_result read_input(const char *path, uint8_t *buffer,
                             size_t capacity, size_t *length)
{
    int descriptor;
    lxp_result status;
    if (path == NULL || buffer == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (strcmp(path, "-") == 0)
        return read_stream(STDIN_FILENO, buffer, capacity, length);
    descriptor = open(path, O_RDONLY | O_NOFOLLOW);
    if (descriptor < 0) return LXP_ERR_IO;
    status = read_stream(descriptor, buffer, capacity, length);
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static void print_hex(const char *label, const uint8_t *bytes, size_t length)
{
    static const char digits[] = "0123456789abcdef";
    char text[67];
    size_t index;
    if (length * 2U + 1U > sizeof(text)) return;
    for (index = 0U; index < length; ++index) {
        text[index * 2U] = digits[(size_t)(bytes[index] >> 4U)];
        text[index * 2U + 1U] = digits[(size_t)(bytes[index] & 15U)];
    }
    text[length * 2U] = '\0';
    (void)printf("%s %s\n", label, text);
}

static void print_u128(const char *label, lxp_u128 value)
{
    char reversed[39];
    char text[40];
    size_t count = 0U;
    size_t index;
    lxp_u128 current = value;
    do {
        uint64_t high = current.hi / 10U;
        uint64_t upper = ((current.hi % 10U) << 32U) | (current.lo >> 32U);
        uint64_t upper_quotient = upper / 10U;
        uint64_t lower = ((upper % 10U) << 32U) |
            (current.lo & UINT64_C(0xffffffff));
        reversed[count] = (char)('0' + (int)(lower % 10U));
        ++count;
        current.hi = high;
        current.lo = (upper_quotient << 32U) | (lower / 10U);
    } while (current.hi != 0U || current.lo != 0U);
    for (index = 0U; index < count; ++index)
        text[index] = reversed[count - index - 1U];
    text[count] = '\0';
    (void)printf("%s %s\n", label, text);
}

static void print_receipt(const lxp_verify_receipt_report *report,
                          const char *prefix)
{
    char label[48];
    (void)snprintf(label, sizeof(label), "%sactivity-id", prefix);
    print_hex(label, report->activity_id, 32U);
    (void)snprintf(label, sizeof(label), "%sreceipt-digest", prefix);
    print_hex(label, report->receipt_digest, 32U);
    (void)printf("%sprotocol-version %" PRIu16 "\n", prefix,
                 report->protocol_version);
    (void)printf("%sglobal-sequence %" PRIu64 "\n", prefix,
                 report->global_sequence);
    (void)printf("%stimestamp-ms %" PRIu64 "\n", prefix, report->timestamp);
    (void)printf("%smodule-id %" PRIu16 "\n", prefix, report->module_id);
    (void)printf("%soperation %" PRIu8 "\n", prefix, report->operation);
    (void)printf("%sresult-code %" PRId32 " %s\n", prefix,
                 report->result_code, lxp_result_name(report->result_code));
    (void)snprintf(label, sizeof(label), "%sasset", prefix);
    print_hex(label, report->asset, 32U);
    (void)snprintf(label, sizeof(label), "%samount", prefix);
    print_u128(label, report->amount);
    (void)snprintf(label, sizeof(label), "%sfee-charged", prefix);
    print_u128(label, report->fee_charged);
    (void)snprintf(label, sizeof(label), "%sprevious-state-root", prefix);
    print_hex(label, report->previous_state_root, 32U);
    (void)snprintf(label, sizeof(label), "%sresulting-state-root", prefix);
    print_hex(label, report->resulting_state_root, 32U);
    (void)snprintf(label, sizeof(label), "%sbatch-id", prefix);
    print_hex(label, report->batch_id, 32U);
}

static void print_trust_root(const lxp_verify_trust_root *trust_root)
{
    size_t index;
    (void)printf("trust-root-network-id %" PRIu32 "\n",
                 trust_root->network_id);
    (void)printf("trust-root-protocol-version %" PRIu16 "\n",
                 trust_root->protocol_version);
    (void)printf("trust-root-genesis-timestamp-ms %" PRIu64 "\n",
                 trust_root->genesis_timestamp_ms);
    print_hex("trust-root-authority-public-key",
              trust_root->authority_public_key, 32U);
    print_hex("trust-root-manifest-commitment",
              trust_root->manifest_commitment, 32U);
    print_hex("trust-root-genesis-state-root",
              trust_root->genesis_state_root, 32U);
    (void)printf("trust-root-guarantor-count %zu\n",
                 trust_root->guarantor_count);
    for (index = 0U; index < trust_root->guarantor_count; ++index) {
        print_hex("trust-root-guarantor-id",
                  trust_root->guarantors[index].guarantor_id, 32U);
        print_hex("trust-root-guarantor-public-key",
                  trust_root->guarantors[index].public_key, 33U);
        (void)printf("trust-root-guarantor-bonded %s\n",
                     trust_root->guarantors[index].bonded ? "yes" : "no");
    }
}

static void print_bundle(const lxp_verify_bundle_report *report)
{
    (void)printf("bundle-kind %s\n",
                 report->kind == (uint8_t)LXP_VERIFY_BUNDLE_ACTIVITY ?
                     "activity" : "receipt");
    print_hex("bundle-activity-id", report->activity_id, 32U);
    print_hex("bundle-leaf-hash", report->leaf_hash, 32U);
    (void)printf("bundle-leaf-index %" PRIu32 "\n", report->leaf_index);
    (void)printf("bundle-leaf-count %" PRIu32 "\n", report->leaf_count);
    print_hex("header-hash", report->header_hash, 32U);
    print_hex("header-sequencer-id", report->sequencer_id, 32U);
    (void)printf("header-network-id %" PRIu32 "\n", report->network_id);
    (void)printf("header-protocol-version %" PRIu16 "\n",
                 report->protocol_version);
    (void)printf("header-batch-number %" PRIu64 "\n", report->batch_number);
    (void)printf("header-first-sequence %" PRIu64 "\n",
                 report->first_sequence);
    (void)printf("header-last-sequence %" PRIu64 "\n",
                 report->last_sequence);
    (void)printf("header-timestamp-ms %" PRIu64 "\n", report->timestamp_ms);
    print_hex("header-previous-state-root", report->previous_state_root, 32U);
    print_hex("header-resulting-state-root", report->resulting_state_root,
              32U);
    if (report->receipt_present) print_receipt(&report->receipt, "receipt-");
}

static void usage(void)
{
    (void)fprintf(stderr,
        "usage: layerx-verify trust-root --trust-root FILE\n"
        "       layerx-verify receipt --trust-root FILE --receipt FILE\n"
        "       layerx-verify bundle --trust-root FILE --value FILE"
        " --proof FILE\n"
        "FILE is a path, or - to read standard input (at most one per"
        " invocation).\n"
        "--trust-root takes the signed genesis manifest"
        " (genesis/genesis.manifest).\n");
}

static int refuse(const char *stage, lxp_result status)
{
    (void)fprintf(stderr, "layerx-verify: refused %s %" PRId32 " %s\n",
                  stage == NULL ? "unknown" : stage, status,
                  lxp_result_name(status));
    return VERIFY_EXIT_REFUSED;
}

int lxp_verify_cli_main(int argc, char **argv)
{
    static uint8_t arena_bytes[VERIFY_ARENA_BYTES];
    static uint8_t trust_root_bytes[LXP_VERIFY_TRUST_ROOT_MAX_BYTES];
    static uint8_t value_bytes[LXP_VERIFY_VALUE_MAX_BYTES];
    static uint8_t proof_bytes[LXP_VERIFY_PROOF_MAX_BYTES];
    static lxp_verify_trust_root trust_root;
    static lxp_verify_receipt_report receipt_report;
    static lxp_verify_bundle_report bundle_report;
    const char *trust_root_path = NULL;
    const char *value_path = NULL;
    const char *proof_path = NULL;
    const char *stage = "argument";
    size_t trust_root_length = 0U;
    size_t value_length = 0U;
    size_t proof_length = 0U;
    unsigned seen = 0U;
    unsigned required;
    unsigned streams = 0U;
    bool trust_root_only;
    bool bundle;
    lxp_arena arena;
    lxp_result status;
    int index;
    if (argc < 2) {
        usage();
        return VERIFY_EXIT_USAGE;
    }
    trust_root_only = strcmp(argv[1], "trust-root") == 0;
    bundle = strcmp(argv[1], "bundle") == 0;
    if (!trust_root_only && !bundle && strcmp(argv[1], "receipt") != 0) {
        usage();
        return VERIFY_EXIT_USAGE;
    }
    for (index = 2; index < argc; index += 2) {
        unsigned bit;
        const char *key = argv[index];
        const char *value;
        if (index + 1 == argc) {
            usage();
            return VERIFY_EXIT_USAGE;
        }
        value = argv[index + 1];
        /* "receipt" names its input --receipt; "bundle" names it --value. */
        if (strcmp(key, "--trust-root") == 0) {
            bit = 1U;
            trust_root_path = value;
        } else if (!trust_root_only && !bundle &&
                   strcmp(key, "--receipt") == 0) {
            bit = 2U;
            value_path = value;
        } else if (bundle && strcmp(key, "--value") == 0) {
            bit = 2U;
            value_path = value;
        } else if (bundle && strcmp(key, "--proof") == 0) {
            bit = 4U;
            proof_path = value;
        } else {
            usage();
            return VERIFY_EXIT_USAGE;
        }
        if ((seen & bit) != 0U) {
            usage();
            return VERIFY_EXIT_USAGE;
        }
        seen |= bit;
        if (strcmp(value, "-") == 0) ++streams;
    }
    required = trust_root_only ? 1U : bundle ? 7U : 3U;
    if (seen != required || streams > 1U) {
        usage();
        return VERIFY_EXIT_USAGE;
    }
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return refuse("arena", LXP_ERR_ARENA_EXHAUSTED);
    stage = "trust-root-read";
    status = read_input(trust_root_path, trust_root_bytes,
                        sizeof(trust_root_bytes), &trust_root_length);
    if (status == LXP_OK)
        status = lxp_verify_trust_root_load(
            trust_root_bytes, trust_root_length, &arena, &trust_root, &stage);
    if (status != LXP_OK) return refuse(stage, status);
    if (trust_root_only) {
        (void)printf("layerx-verify trust-root ok\n");
        print_trust_root(&trust_root);
        return fflush(stdout) == 0 ? VERIFY_EXIT_OK :
            refuse("output", LXP_ERR_IO);
    }
    stage = "value-read";
    status = read_input(value_path, value_bytes, sizeof(value_bytes),
                        &value_length);
    if (status == LXP_OK && bundle) {
        stage = "proof-read";
        status = read_input(proof_path, proof_bytes, sizeof(proof_bytes),
                            &proof_length);
    }
    if (status != LXP_OK) return refuse(stage, status);
    if (bundle)
        status = lxp_verify_bundle_offline(
            &trust_root, value_bytes, value_length, proof_bytes,
            proof_length, &arena, &bundle_report, &stage);
    else
        status = lxp_verify_receipt_offline_pinned(
            &trust_root, value_bytes, value_length, &arena, &receipt_report,
            &stage);
    if (status != LXP_OK) return refuse(stage, status);
    (void)printf("layerx-verify %s ok\n", bundle ? "bundle" : "receipt");
    print_trust_root(&trust_root);
    if (bundle) print_bundle(&bundle_report);
    else print_receipt(&receipt_report, "receipt-");
    return fflush(stdout) == 0 ? VERIFY_EXIT_OK :
        refuse("output", LXP_ERR_IO);
}
