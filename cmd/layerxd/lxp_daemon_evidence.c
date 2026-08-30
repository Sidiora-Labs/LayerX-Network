#include "layerx/lxp_daemon.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_receipt.h"

#include <stdlib.h>
#include <string.h>

enum {
    EVIDENCE_RECORD_VERSION = 1,
    EVIDENCE_RECORD_FIXED_BYTES = 60,
    EVIDENCE_RECORD_DIGEST_BYTES = 32,
    EVIDENCE_WIRE_VERSION = 1,
    FINALITY_SETTLEMENT_REFERENCE_BYTES = 110,
    FINALITY_ATTESTATION_BYTES = 274
};

static const uint8_t evidence_magic[4] = {'L', 'X', 'E', '1'};
static const uint8_t account_tree_key[] = "account-tree";

typedef struct evidence_reader {
    const uint8_t *bytes;
    size_t length;
    size_t cursor;
} evidence_reader;

typedef struct evidence_writer {
    uint8_t *bytes;
    size_t capacity;
    size_t cursor;
} evidence_writer;

typedef struct decoded_finality {
    lxp_guarantor_cert certificate;
    lxp_guarantor_set bonded_set;
    lxp_finalisation_requirements requirements;
    lxp_daemon_settlement_registration_evidence settlement_registration;
    uint64_t expected_registration_count;
    uint8_t registered_checkpoint_id[32];
    uint8_t registered_resulting_root[32];
    uint64_t registered_batch_number;
    uint64_t registered_chain_id;
    uint8_t registered_contract[20];
    lxp_byte_span registered_reference;
} decoded_finality;

typedef struct decoded_record {
    lxp_daemon_evidence_kind kind;
    uint32_t network_id;
    uint64_t ordinal;
    uint8_t key[32];
    lxp_byte_span payload;
    lxp_byte_span proof;
    uint8_t digest[32];
} decoded_record;

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static lxp_result reader_take(evidence_reader *reader, size_t length,
                              const uint8_t **bytes)
{
    if (reader == NULL || bytes == NULL || reader->cursor > reader->length ||
        length > reader->length - reader->cursor)
        return LXP_ERR_TRUNCATED;
    *bytes = reader->bytes + reader->cursor;
    reader->cursor += length;
    return LXP_OK;
}

static lxp_result reader_u8(evidence_reader *reader, uint8_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 1U, &bytes);
    if (status == LXP_OK) *value = bytes[0];
    return status;
}

static lxp_result reader_u16(evidence_reader *reader, uint16_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 2U, &bytes);
    if (status == LXP_OK) *value = read_u16(bytes);
    return status;
}

static lxp_result reader_u32(evidence_reader *reader, uint32_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 4U, &bytes);
    if (status == LXP_OK) *value = read_u32(bytes);
    return status;
}

static lxp_result reader_u64(evidence_reader *reader, uint64_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 8U, &bytes);
    if (status == LXP_OK) *value = read_u64(bytes);
    return status;
}

static lxp_result reader_copy(evidence_reader *reader, uint8_t *output,
                              size_t length)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, length, &bytes);
    if (status == LXP_OK) (void)memcpy(output, bytes, length);
    return status;
}

static lxp_result reader_finish(const evidence_reader *reader)
{
    return reader != NULL && reader->cursor == reader->length ? LXP_OK :
           LXP_ERR_TRAILING_BYTES;
}

static lxp_result writer_take(evidence_writer *writer, size_t length,
                              uint8_t **bytes)
{
    if (writer == NULL || bytes == NULL || writer->cursor > writer->capacity ||
        length > writer->capacity - writer->cursor)
        return LXP_ERR_LENGTH_LIMIT;
    *bytes = writer->bytes + writer->cursor;
    writer->cursor += length;
    return LXP_OK;
}

static lxp_result writer_u8(evidence_writer *writer, uint8_t value)
{
    uint8_t *bytes;
    lxp_result status = writer_take(writer, 1U, &bytes);
    if (status == LXP_OK) bytes[0] = value;
    return status;
}

static lxp_result writer_u16(evidence_writer *writer, uint16_t value)
{
    uint8_t *bytes;
    lxp_result status = writer_take(writer, 2U, &bytes);
    if (status == LXP_OK) write_u16(bytes, value);
    return status;
}

static lxp_result writer_u32(evidence_writer *writer, uint32_t value)
{
    uint8_t *bytes;
    lxp_result status = writer_take(writer, 4U, &bytes);
    if (status == LXP_OK) write_u32(bytes, value);
    return status;
}

static lxp_result writer_u64(evidence_writer *writer, uint64_t value)
{
    uint8_t *bytes;
    lxp_result status = writer_take(writer, 8U, &bytes);
    if (status == LXP_OK) write_u64(bytes, value);
    return status;
}

static lxp_result writer_bytes(evidence_writer *writer, const uint8_t *input,
                               size_t length)
{
    uint8_t *bytes;
    lxp_result status;
    if (input == NULL && length != 0U) return LXP_ERR_NON_CANONICAL;
    status = writer_take(writer, length, &bytes);
    if (status == LXP_OK && length != 0U) (void)memcpy(bytes, input, length);
    return status;
}

static uint8_t proof_depth(uint32_t count)
{
    uint8_t depth = 0U;
    while (count > 1U) {
        count = (count + 1U) / 2U;
        ++depth;
    }
    return depth;
}

static size_t state_proof_length(const lxp_state_proof *proof)
{
    return proof == NULL ? 0U : 9U + (size_t)proof->depth * 32U;
}

static size_t merkle_proof_length(const lxp_merkle_proof *proof)
{
    return proof == NULL ? 0U : 9U + (size_t)proof->depth * 32U;
}

static lxp_result write_state_proof(evidence_writer *writer,
                                    const lxp_state_proof *proof)
{
    lxp_result status;
    if (proof == NULL || proof->leaf_count == 0U ||
        proof->leaf_index >= proof->leaf_count ||
        proof->depth > LXP_STATE_PROOF_MAX_DEPTH ||
        proof->depth != proof_depth(proof->leaf_count))
        return LXP_ERR_NON_CANONICAL;
    status = writer_u32(writer, proof->leaf_index);
    if (status == LXP_OK) status = writer_u32(writer, proof->leaf_count);
    if (status == LXP_OK) status = writer_u8(writer, proof->depth);
    if (status == LXP_OK)
        status = writer_bytes(writer, proof->siblings[0],
                              (size_t)proof->depth * 32U);
    return status;
}

static lxp_result read_state_proof(evidence_reader *reader,
                                   lxp_state_proof *proof)
{
    lxp_result status;
    if (reader == NULL || proof == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(proof, 0, sizeof(*proof));
    status = reader_u32(reader, &proof->leaf_index);
    if (status == LXP_OK) status = reader_u32(reader, &proof->leaf_count);
    if (status == LXP_OK) status = reader_u8(reader, &proof->depth);
    if (status == LXP_OK &&
        (proof->leaf_count == 0U || proof->leaf_index >= proof->leaf_count ||
         proof->depth > LXP_STATE_PROOF_MAX_DEPTH ||
         proof->depth != proof_depth(proof->leaf_count)))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = reader_copy(reader, proof->siblings[0],
                             (size_t)proof->depth * 32U);
    return status;
}

static lxp_result write_merkle_proof(evidence_writer *writer,
                                     const lxp_merkle_proof *proof)
{
    lxp_result status;
    if (proof == NULL || proof->leaf_count == 0U ||
        proof->leaf_index >= proof->leaf_count ||
        proof->depth > LXP_MERKLE_MAX_DEPTH ||
        proof->depth != proof_depth(proof->leaf_count))
        return LXP_ERR_NON_CANONICAL;
    status = writer_u32(writer, proof->leaf_index);
    if (status == LXP_OK) status = writer_u32(writer, proof->leaf_count);
    if (status == LXP_OK) status = writer_u8(writer, proof->depth);
    if (status == LXP_OK)
        status = writer_bytes(writer, proof->siblings[0],
                              (size_t)proof->depth * 32U);
    return status;
}

static lxp_result read_merkle_proof(evidence_reader *reader,
                                    lxp_merkle_proof *proof)
{
    lxp_result status;
    if (reader == NULL || proof == NULL) return LXP_ERR_NON_CANONICAL;
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

static size_t signed_header_length(
    const lxp_daemon_signed_header_evidence *signed_header)
{
    return signed_header == NULL ? 0U :
        2U + 32U + 32U + 8U + 8U + 4U +
        signed_header->canonical_header.length + 64U;
}

static lxp_result write_signed_header(
    evidence_writer *writer,
    const lxp_daemon_signed_header_evidence *signed_header)
{
    lxp_result status;
    if (signed_header == NULL ||
        signed_header->canonical_header.bytes == NULL ||
        signed_header->canonical_header.length !=
            LXP_BATCH_HEADER_ENCODED_SIZE ||
        !signed_header->authorization.authorized ||
        signed_header->authorization.first_batch_number == 0U ||
        signed_header->authorization.last_batch_number <
            signed_header->authorization.first_batch_number)
        return LXP_ERR_NON_CANONICAL;
    status = writer_u16(writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK)
        status = writer_bytes(writer,
            signed_header->authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = writer_bytes(writer,
            signed_header->authorization.public_key, 32U);
    if (status == LXP_OK)
        status = writer_u64(writer,
            signed_header->authorization.first_batch_number);
    if (status == LXP_OK)
        status = writer_u64(writer,
            signed_header->authorization.last_batch_number);
    if (status == LXP_OK)
        status = writer_u32(writer,
            (uint32_t)signed_header->canonical_header.length);
    if (status == LXP_OK)
        status = writer_bytes(writer, signed_header->canonical_header.bytes,
                              signed_header->canonical_header.length);
    if (status == LXP_OK)
        status = writer_bytes(writer, signed_header->signature, 64U);
    return status;
}

static lxp_result read_signed_header(
    evidence_reader *reader, lxp_arena *arena,
    lxp_daemon_signed_header_evidence *signed_header)
{
    uint16_t version;
    uint32_t header_length;
    const uint8_t *header;
    void *copy;
    lxp_result status;
    if (reader == NULL || arena == NULL || signed_header == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(signed_header, 0, sizeof(*signed_header));
    status = reader_u16(reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_copy(reader,
            signed_header->authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = reader_copy(reader,
            signed_header->authorization.public_key, 32U);
    if (status == LXP_OK)
        status = reader_u64(reader,
            &signed_header->authorization.first_batch_number);
    if (status == LXP_OK)
        status = reader_u64(reader,
            &signed_header->authorization.last_batch_number);
    if (status == LXP_OK) status = reader_u32(reader, &header_length);
    if (status == LXP_OK && header_length != LXP_BATCH_HEADER_ENCODED_SIZE)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = reader_take(reader, header_length, &header);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, header_length, _Alignof(uint64_t),
                                 &copy);
    if (status == LXP_OK) {
        (void)memcpy(copy, header, header_length);
        signed_header->canonical_header =
            (lxp_byte_span){(const uint8_t *)copy, header_length};
    }
    if (status == LXP_OK)
        status = reader_copy(reader, signed_header->signature, 64U);
    signed_header->authorization.authorized = status == LXP_OK ? 1U : 0U;
    return status;
}

static lxp_result verify_signed_header(
    const lxp_daemon_signed_header_evidence *signed_header,
    uint32_t network_id, lxp_arena *arena, lxp_batch_header *header)
{
    lxp_result status;
    if (signed_header == NULL || arena == NULL || header == NULL ||
        signed_header->canonical_header.bytes == NULL ||
        signed_header->canonical_header.length !=
            LXP_BATCH_HEADER_ENCODED_SIZE ||
        !signed_header->authorization.authorized)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_header_decode(
        signed_header->canonical_header.bytes,
        signed_header->canonical_header.length, header);
    if (status == LXP_OK &&
        (header->network_id != network_id ||
         header->batch_number <
             signed_header->authorization.first_batch_number ||
         header->batch_number >
             signed_header->authorization.last_batch_number ||
         lxp_ct_memcmp(header->sequencer_id,
             signed_header->authorization.sequencer_id, 32U) != 0))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(
            header, signed_header->signature, 64U,
            &signed_header->authorization, arena);
    return status;
}

static bool authorizations_equal(
    const lxp_sequencer_authorization *left,
    const lxp_sequencer_authorization *right)
{
    return left != NULL && right != NULL && left->authorized &&
        right->authorized &&
        left->first_batch_number == right->first_batch_number &&
        left->last_batch_number == right->last_batch_number &&
        lxp_ct_memcmp(left->sequencer_id, right->sequencer_id, 32U) == 0 &&
        lxp_ct_memcmp(left->public_key, right->public_key, 32U) == 0;
}

static lxp_result state_leaf_hash(const uint8_t *key, size_t key_length,
                                  const uint8_t *value, size_t value_length,
                                  uint8_t hash[32])
{
    lxp_hash_context context;
    const uint8_t *tag;
    size_t tag_length;
    uint8_t lengths[8];
    size_t index;
    lxp_result status;
    if (key == NULL || hash == NULL || key_length > UINT32_MAX ||
        value_length > UINT32_MAX || (value == NULL && value_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < 4U; ++index) {
        lengths[index] = (uint8_t)(key_length >> (24U - index * 8U));
        lengths[4U + index] =
            (uint8_t)(value_length >> (24U - index * 8U));
    }
    tag = lxp_domain_tag(LXP_DOMAIN_STATE_LEAF, &tag_length);
    if (tag == NULL) return LXP_FATAL_INVARIANT;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, tag, tag_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, lengths, sizeof(lengths));
    if (status == LXP_OK) status = lxp_hash_update(&context, key, key_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, value, value_length);
    if (status == LXP_OK) status = lxp_hash_final(&context, hash);
    return status;
}

static lxp_result state_node_hash(const uint8_t left[32],
                                  const uint8_t right[32], uint8_t hash[32])
{
    uint8_t pair[64];
    if (left == NULL || right == NULL || hash == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(pair, left, 32U);
    (void)memcpy(pair + 32U, right, 32U);
    return lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), hash);
}

static lxp_result state_proof_verify(const uint8_t leaf[32],
                                     const lxp_state_proof *proof,
                                     const uint8_t expected_root[32])
{
    uint8_t current[32];
    uint8_t next[32];
    uint32_t index;
    uint32_t count;
    size_t depth;
    lxp_result status = LXP_OK;
    if (leaf == NULL || proof == NULL || expected_root == NULL ||
        proof->leaf_count == 0U || proof->leaf_index >= proof->leaf_count ||
        proof->depth > LXP_STATE_PROOF_MAX_DEPTH ||
        proof->depth != proof_depth(proof->leaf_count))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(current, leaf, 32U);
    index = proof->leaf_index;
    count = proof->leaf_count;
    for (depth = 0U; status == LXP_OK && depth < proof->depth; ++depth) {
        uint32_t sibling = index ^ 1U;
        const uint8_t *left;
        const uint8_t *right;
        if (sibling >= count && lxp_ct_memcmp(
                proof->siblings[depth], current, 32U) != 0)
            return LXP_ERR_NON_CANONICAL;
        left = (index & 1U) == 0U ? current : proof->siblings[depth];
        right = (index & 1U) == 0U ? proof->siblings[depth] : current;
        status = state_node_hash(left, right, next);
        if (status == LXP_OK) (void)memcpy(current, next, 32U);
        index /= 2U;
        count = (count + 1U) / 2U;
    }
    if (status == LXP_OK && lxp_ct_memcmp(
            current, expected_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    lxp_secure_zero(current, sizeof(current));
    lxp_secure_zero(next, sizeof(next));
    return status;
}

static lxp_result verify_account_evidence(
    const lxp_daemon_account_evidence *evidence, uint32_t network_id,
    lxp_arena *arena)
{
    lxp_batch_header header;
    lxp_receipt receipt;
    uint8_t leaf[32];
    uint8_t receipt_leaf[32];
    uint8_t digest[32];
    uint8_t module_key[2] = {0U, 0U};
    lxp_result status;
    if (evidence == NULL || arena == NULL ||
        evidence->observed_sequence == 0U || evidence->observed_at_ms == 0U ||
        evidence->account_leaf_value_length == 0U ||
        evidence->account_leaf_value_length >
            LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES ||
        evidence->canonical_receipt.bytes == NULL ||
        evidence->canonical_receipt.length == 0U ||
        evidence->account_leaf_key[0] != 4U ||
        lxp_ct_memcmp(evidence->account_leaf_key + 1U,
                      evidence->account_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = verify_signed_header(&evidence->signed_header, network_id,
                                  arena, &header);
    if (status == LXP_OK)
        status = lxp_receipt_decode(evidence->canonical_receipt.bytes,
                                    evidence->canonical_receipt.length,
                                    true, &receipt);
    if (status == LXP_OK)
        status = lxp_receipt_verify(
            &receipt, evidence->signed_header.authorization.public_key,
            arena);
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, digest);
    if (status == LXP_OK &&
        (receipt.global_sequence != header.last_sequence ||
         receipt.global_sequence != evidence->observed_sequence ||
         receipt.timestamp != evidence->observed_at_ms ||
         receipt.timestamp != header.timestamp_ms ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       header.resulting_state_root, 32U) != 0 ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       evidence->resulting_state_root, 32U) != 0 ||
         lxp_ct_memcmp(digest, evidence->receipt_digest, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(evidence->canonical_receipt.bytes,
                                      evidence->canonical_receipt.length,
                                      receipt_leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            receipt_leaf, &evidence->receipt_proof,
            header.receipt_merkle_root);
    if (status == LXP_OK)
        status = state_leaf_hash(
            evidence->account_leaf_key, sizeof(evidence->account_leaf_key),
            evidence->account_leaf_value,
            evidence->account_leaf_value_length, leaf);
    if (status == LXP_OK)
        status = state_proof_verify(leaf, &evidence->account_proof,
                                    evidence->account_root);
    if (status == LXP_OK)
        status = state_leaf_hash(account_tree_key,
                                 sizeof(account_tree_key) - 1U,
                                 evidence->account_root, 32U, leaf);
    if (status == LXP_OK)
        status = state_proof_verify(leaf, &evidence->account_tree_proof,
                                    evidence->universal_root);
    if (status == LXP_OK)
        status = state_leaf_hash(module_key, sizeof(module_key),
                                 evidence->universal_root, 32U, leaf);
    if (status == LXP_OK)
        status = state_proof_verify(leaf, &evidence->universal_root_proof,
                                    evidence->resulting_state_root);
    return status;
}

static lxp_result verify_activity_evidence(
    const lxp_daemon_activity_evidence *evidence, uint32_t network_id,
    lxp_arena *arena)
{
    lxp_batch_header header;
    lxp_activity activity;
    lxp_receipt receipt;
    uint8_t activity_id[32];
    uint8_t receipt_digest[32];
    uint8_t leaf[32];
    lxp_result status;
    if (evidence == NULL || arena == NULL ||
        evidence->canonical_activity.bytes == NULL ||
        evidence->canonical_activity.length == 0U ||
        evidence->canonical_receipt.bytes == NULL ||
        evidence->canonical_receipt.length == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = verify_signed_header(&evidence->signed_header, network_id,
                                  arena, &header);
    if (status == LXP_OK)
        status = lxp_activity_decode(evidence->canonical_activity.bytes,
                                     evidence->canonical_activity.length,
                                     &activity);
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(&activity, network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK)
        status = lxp_activity_id(evidence->canonical_activity.bytes,
                                 evidence->canonical_activity.length,
                                 activity_id);
    if (status == LXP_OK)
        status = lxp_receipt_decode(evidence->canonical_receipt.bytes,
                                    evidence->canonical_receipt.length,
                                    true, &receipt);
    if (status == LXP_OK)
        status = lxp_receipt_verify(
            &receipt, evidence->signed_header.authorization.public_key,
            arena);
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, receipt_digest);
    if (status == LXP_OK &&
        (header.batch_number != evidence->batch_number ||
         receipt.global_sequence != evidence->global_sequence ||
         receipt.global_sequence < header.first_sequence ||
         receipt.global_sequence > header.last_sequence ||
         receipt.timestamp != header.timestamp_ms ||
         lxp_ct_memcmp(activity_id, evidence->activity_id, 32U) != 0 ||
         lxp_ct_memcmp(receipt.activity_id, activity_id, 32U) != 0 ||
         lxp_ct_memcmp(receipt_digest, evidence->receipt_digest, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(evidence->canonical_activity.bytes,
                                      evidence->canonical_activity.length,
                                      leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            leaf, &evidence->activity_proof, header.activity_merkle_root);
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(evidence->canonical_receipt.bytes,
                                      evidence->canonical_receipt.length,
                                      leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            leaf, &evidence->receipt_proof, header.receipt_merkle_root);
    return status;
}

static size_t account_payload_length(
    const lxp_daemon_account_evidence *evidence)
{
    if (evidence == NULL) return 0U;
    return 2U + 32U + 32U + 8U + 8U + 33U + 2U +
        evidence->account_leaf_value_length + 96U +
        state_proof_length(&evidence->account_proof) +
        state_proof_length(&evidence->account_tree_proof) +
        state_proof_length(&evidence->universal_root_proof) + 4U +
        evidence->canonical_receipt.length +
        merkle_proof_length(&evidence->receipt_proof) +
        signed_header_length(&evidence->signed_header);
}

static lxp_result encode_account_payload(
    const lxp_daemon_account_evidence *evidence, uint8_t *bytes,
    size_t capacity, size_t *length)
{
    evidence_writer writer = {bytes, capacity, 0U};
    lxp_result status;
    if (evidence == NULL || bytes == NULL || length == NULL ||
        evidence->account_leaf_value_length > UINT16_MAX ||
        evidence->canonical_receipt.length > UINT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = writer_u16(&writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->account_id, 32U);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->receipt_digest, 32U);
    if (status == LXP_OK) status = writer_u64(&writer, evidence->observed_sequence);
    if (status == LXP_OK) status = writer_u64(&writer, evidence->observed_at_ms);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->account_leaf_key, 33U);
    if (status == LXP_OK) status = writer_u16(&writer, (uint16_t)evidence->account_leaf_value_length);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->account_leaf_value, evidence->account_leaf_value_length);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->account_root, 32U);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->universal_root, 32U);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->resulting_state_root, 32U);
    if (status == LXP_OK) status = write_state_proof(&writer, &evidence->account_proof);
    if (status == LXP_OK) status = write_state_proof(&writer, &evidence->account_tree_proof);
    if (status == LXP_OK) status = write_state_proof(&writer, &evidence->universal_root_proof);
    if (status == LXP_OK) status = writer_u32(&writer, (uint32_t)evidence->canonical_receipt.length);
    if (status == LXP_OK) status = writer_bytes(&writer, evidence->canonical_receipt.bytes, evidence->canonical_receipt.length);
    if (status == LXP_OK) status = write_merkle_proof(&writer, &evidence->receipt_proof);
    if (status == LXP_OK) status = write_signed_header(&writer, &evidence->signed_header);
    if (status == LXP_OK) *length = writer.cursor;
    return status;
}

static lxp_result decode_account_payload(
    lxp_byte_span payload, lxp_arena *arena,
    lxp_daemon_account_evidence *evidence)
{
    evidence_reader reader = {payload.bytes, payload.length, 0U};
    uint16_t version;
    uint16_t value_length;
    uint32_t receipt_length;
    const uint8_t *receipt;
    void *copy;
    lxp_result status;
    if (payload.bytes == NULL || arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(evidence, 0, sizeof(*evidence));
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) status = reader_copy(&reader, evidence->account_id, 32U);
    if (status == LXP_OK) status = reader_copy(&reader, evidence->receipt_digest, 32U);
    if (status == LXP_OK) status = reader_u64(&reader, &evidence->observed_sequence);
    if (status == LXP_OK) status = reader_u64(&reader, &evidence->observed_at_ms);
    if (status == LXP_OK) status = reader_copy(&reader, evidence->account_leaf_key, 33U);
    if (status == LXP_OK) status = reader_u16(&reader, &value_length);
    if (status == LXP_OK && (value_length == 0U || value_length > LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES)) status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) status = reader_copy(&reader, evidence->account_leaf_value, value_length);
    evidence->account_leaf_value_length = status == LXP_OK ? value_length : 0U;
    if (status == LXP_OK) status = reader_copy(&reader, evidence->account_root, 32U);
    if (status == LXP_OK) status = reader_copy(&reader, evidence->universal_root, 32U);
    if (status == LXP_OK) status = reader_copy(&reader, evidence->resulting_state_root, 32U);
    if (status == LXP_OK) status = read_state_proof(&reader, &evidence->account_proof);
    if (status == LXP_OK) status = read_state_proof(&reader, &evidence->account_tree_proof);
    if (status == LXP_OK) status = read_state_proof(&reader, &evidence->universal_root_proof);
    if (status == LXP_OK) status = reader_u32(&reader, &receipt_length);
    if (status == LXP_OK && (receipt_length == 0U || receipt_length > LXP_STATE_MAX_RECEIPT_BYTES)) status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) status = reader_take(&reader, receipt_length, &receipt);
    if (status == LXP_OK) status = lxp_arena_alloc(arena, receipt_length, _Alignof(uint64_t), &copy);
    if (status == LXP_OK) {
        (void)memcpy(copy, receipt, receipt_length);
        evidence->canonical_receipt = (lxp_byte_span){copy, receipt_length};
    }
    if (status == LXP_OK) status = read_merkle_proof(&reader, &evidence->receipt_proof);
    if (status == LXP_OK) status = read_signed_header(&reader, arena, &evidence->signed_header);
    if (status == LXP_OK) status = reader_finish(&reader);
    return status;
}

static size_t activity_payload_length(
    const lxp_daemon_activity_evidence *evidence)
{
    if (evidence == NULL) return 0U;
    return 2U + 32U + 32U + 8U + 8U + 4U +
        evidence->canonical_activity.length +
        merkle_proof_length(&evidence->activity_proof) + 4U +
        evidence->canonical_receipt.length +
        merkle_proof_length(&evidence->receipt_proof) +
        signed_header_length(&evidence->signed_header);
}

static lxp_result encode_activity_payload(
    const lxp_daemon_activity_evidence *evidence, uint8_t *bytes,
    size_t capacity, size_t *length)
{
    evidence_writer writer = {bytes, capacity, 0U};
    lxp_result status;
    if (evidence == NULL || bytes == NULL || length == NULL ||
        evidence->canonical_activity.length > UINT32_MAX ||
        evidence->canonical_receipt.length > UINT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = writer_u16(&writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->activity_id, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->receipt_digest, 32U);
    if (status == LXP_OK)
        status = writer_u64(&writer, evidence->global_sequence);
    if (status == LXP_OK)
        status = writer_u64(&writer, evidence->batch_number);
    if (status == LXP_OK)
        status = writer_u32(&writer,
                            (uint32_t)evidence->canonical_activity.length);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->canonical_activity.bytes,
                              evidence->canonical_activity.length);
    if (status == LXP_OK)
        status = write_merkle_proof(&writer, &evidence->activity_proof);
    if (status == LXP_OK)
        status = writer_u32(&writer,
                            (uint32_t)evidence->canonical_receipt.length);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->canonical_receipt.bytes,
                              evidence->canonical_receipt.length);
    if (status == LXP_OK)
        status = write_merkle_proof(&writer, &evidence->receipt_proof);
    if (status == LXP_OK)
        status = write_signed_header(&writer, &evidence->signed_header);
    if (status == LXP_OK) *length = writer.cursor;
    return status;
}

static lxp_result decode_activity_payload(
    lxp_byte_span payload, lxp_arena *arena,
    lxp_daemon_activity_evidence *evidence)
{
    evidence_reader reader = {payload.bytes, payload.length, 0U};
    uint16_t version;
    uint32_t activity_length;
    uint32_t receipt_length;
    const uint8_t *activity;
    const uint8_t *receipt;
    void *copy;
    lxp_result status;
    if (payload.bytes == NULL || arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(evidence, 0, sizeof(*evidence));
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_copy(&reader, evidence->activity_id, 32U);
    if (status == LXP_OK)
        status = reader_copy(&reader, evidence->receipt_digest, 32U);
    if (status == LXP_OK)
        status = reader_u64(&reader, &evidence->global_sequence);
    if (status == LXP_OK)
        status = reader_u64(&reader, &evidence->batch_number);
    if (status == LXP_OK) status = reader_u32(&reader, &activity_length);
    if (status == LXP_OK &&
        (activity_length == 0U || activity_length > LXP_MAX_ACTIVITY_BYTES))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = reader_take(&reader, activity_length, &activity);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, activity_length,
                                 _Alignof(uint64_t), &copy);
    if (status == LXP_OK) {
        (void)memcpy(copy, activity, activity_length);
        evidence->canonical_activity =
            (lxp_byte_span){(const uint8_t *)copy, activity_length};
    }
    if (status == LXP_OK)
        status = read_merkle_proof(&reader, &evidence->activity_proof);
    if (status == LXP_OK) status = reader_u32(&reader, &receipt_length);
    if (status == LXP_OK &&
        (receipt_length == 0U || receipt_length > LXP_STATE_MAX_RECEIPT_BYTES))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = reader_take(&reader, receipt_length, &receipt);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, receipt_length,
                                 _Alignof(uint64_t), &copy);
    if (status == LXP_OK) {
        (void)memcpy(copy, receipt, receipt_length);
        evidence->canonical_receipt =
            (lxp_byte_span){(const uint8_t *)copy, receipt_length};
    }
    if (status == LXP_OK)
        status = read_merkle_proof(&reader, &evidence->receipt_proof);
    if (status == LXP_OK)
        status = read_signed_header(&reader, arena,
                                    &evidence->signed_header);
    if (status == LXP_OK) status = reader_finish(&reader);
    return status;
}

static lxp_result write_attestation(
    evidence_writer *writer, const lxp_guarantor_attestation *attestation)
{
    lxp_result status;
    if (writer == NULL || attestation == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = writer_u16(writer, attestation->protocol_version);
    if (status == LXP_OK) status = writer_u32(writer, attestation->network_id);
    if (status == LXP_OK) status = writer_u64(writer, attestation->paxeer_chain_id);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->paxeer_settlement_contract, 20U);
    if (status == LXP_OK) status = writer_u64(writer, attestation->epoch);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->checkpoint_id, 32U);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->checkpoint_hash, 32U);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->guarantor_id, 32U);
    if (status == LXP_OK) status = writer_u64(writer, attestation->batch_number);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->data_availability_root, 32U);
    if (status == LXP_OK) status = writer_u8(writer, attestation->replayed ? 1U : 0U);
    if (status == LXP_OK) status = writer_u8(writer, attestation->da_possessed ? 1U : 0U);
    if (status == LXP_OK) status = writer_u8(writer, attestation->availability_class_mask);
    if (status == LXP_OK) status = writer_u64(writer, attestation->attested_at_ms);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->signer, 20U);
    if (status == LXP_OK) status = writer_bytes(writer, attestation->signature, 64U);
    if (status == LXP_OK) status = writer_u8(writer, attestation->signature_v);
    return status;
}

static lxp_result read_attestation(
    evidence_reader *reader, lxp_guarantor_attestation *attestation)
{
    uint8_t replayed = 0U;
    uint8_t possessed = 0U;
    lxp_result status;
    if (reader == NULL || attestation == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(attestation, 0, sizeof(*attestation));
    status = reader_u16(reader, &attestation->protocol_version);
    if (status == LXP_OK) status = reader_u32(reader, &attestation->network_id);
    if (status == LXP_OK) status = reader_u64(reader, &attestation->paxeer_chain_id);
    if (status == LXP_OK) status = reader_copy(reader, attestation->paxeer_settlement_contract, 20U);
    if (status == LXP_OK) status = reader_u64(reader, &attestation->epoch);
    if (status == LXP_OK) status = reader_copy(reader, attestation->checkpoint_id, 32U);
    if (status == LXP_OK) status = reader_copy(reader, attestation->checkpoint_hash, 32U);
    if (status == LXP_OK) status = reader_copy(reader, attestation->guarantor_id, 32U);
    if (status == LXP_OK) status = reader_u64(reader, &attestation->batch_number);
    if (status == LXP_OK) status = reader_copy(reader, attestation->data_availability_root, 32U);
    if (status == LXP_OK) status = reader_u8(reader, &replayed);
    if (status == LXP_OK) status = reader_u8(reader, &possessed);
    if (status == LXP_OK && (replayed > 1U || possessed > 1U))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = reader_u8(reader, &attestation->availability_class_mask);
    if (status == LXP_OK) status = reader_u64(reader, &attestation->attested_at_ms);
    if (status == LXP_OK) status = reader_copy(reader, attestation->signer, 20U);
    if (status == LXP_OK) status = reader_copy(reader, attestation->signature, 64U);
    if (status == LXP_OK) status = reader_u8(reader, &attestation->signature_v);
    if (status == LXP_OK) {
        attestation->replayed = replayed == 1U;
        attestation->da_possessed = possessed == 1U;
    }
    return status;
}

static lxp_result settlement_reference_encode(
    const lxp_daemon_settlement_registration_evidence *registration,
    uint8_t output[FINALITY_SETTLEMENT_REFERENCE_BYTES])
{
    evidence_writer writer = {
        output, FINALITY_SETTLEMENT_REFERENCE_BYTES, 0U};
    lxp_result status;
    if (registration == NULL || output == NULL ||
        registration->paxeer_chain_id == 0U ||
        registration->observed_block_number == 0U ||
        registration->observed_at_ms == 0U ||
        lxp_ct_is_zero(registration->settlement_contract, 20U) ||
        lxp_ct_is_zero(registration->checkpoint_id, 32U) ||
        lxp_ct_is_zero(registration->transaction_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = writer_u16(&writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK)
        status = writer_u64(&writer, registration->paxeer_chain_id);
    if (status == LXP_OK)
        status = writer_bytes(&writer, registration->settlement_contract, 20U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, registration->checkpoint_id, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, registration->transaction_id, 32U);
    if (status == LXP_OK)
        status = writer_u64(&writer, registration->observed_block_number);
    if (status == LXP_OK)
        status = writer_u64(&writer, registration->observed_at_ms);
    return status == LXP_OK && writer.cursor == writer.capacity ? LXP_OK :
           status != LXP_OK ? status : LXP_FATAL_INVARIANT;
}

static lxp_result settlement_reference_decode(
    lxp_byte_span reference,
    lxp_daemon_settlement_registration_evidence *registration)
{
    evidence_reader reader = {reference.bytes, reference.length, 0U};
    uint16_t version;
    lxp_result status;
    if (reference.bytes == NULL ||
        reference.length != FINALITY_SETTLEMENT_REFERENCE_BYTES ||
        registration == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(registration, 0, sizeof(*registration));
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_u64(&reader, &registration->paxeer_chain_id);
    if (status == LXP_OK)
        status = reader_copy(&reader, registration->settlement_contract, 20U);
    if (status == LXP_OK)
        status = reader_copy(&reader, registration->checkpoint_id, 32U);
    if (status == LXP_OK)
        status = reader_copy(&reader, registration->transaction_id, 32U);
    if (status == LXP_OK)
        status = reader_u64(&reader, &registration->observed_block_number);
    if (status == LXP_OK)
        status = reader_u64(&reader, &registration->observed_at_ms);
    if (status == LXP_OK) status = reader_finish(&reader);
    if (status == LXP_OK &&
        (registration->paxeer_chain_id == 0U ||
         registration->observed_block_number == 0U ||
         registration->observed_at_ms == 0U ||
         lxp_ct_is_zero(registration->settlement_contract, 20U) ||
         lxp_ct_is_zero(registration->checkpoint_id, 32U) ||
         lxp_ct_is_zero(registration->transaction_id, 32U)))
        status = LXP_ERR_NON_CANONICAL;
    return status;
}

static lxp_result decode_checkpoint_payload(
    lxp_byte_span payload, decoded_finality *decoded)
{
    evidence_reader reader = {payload.bytes, payload.length, 0U};
    uint16_t version;
    uint32_t header_length;
    uint32_t validity_length;
    uint8_t attestation_count;
    uint8_t threshold;
    uint16_t reference_length;
    const uint8_t *header;
    const uint8_t *validity;
    const uint8_t *reference;
    size_t index;
    lxp_result status;
    if (payload.bytes == NULL || decoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded->certificate, 0, sizeof(decoded->certificate));
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) status = reader_u32(&reader, &header_length);
    if (status == LXP_OK && header_length != LXP_BATCH_HEADER_ENCODED_SIZE)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = reader_take(&reader, header_length, &header);
    if (status == LXP_OK)
        status = lxp_batch_header_decode(header, header_length,
                                         &decoded->certificate.checkpoint.header);
    if (status == LXP_OK) status = reader_u32(&reader, &validity_length);
    if (status == LXP_OK && validity_length > LXP_MAX_VALIDITY_PROOF_BYTES)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = reader_take(&reader, validity_length, &validity);
    if (status == LXP_OK)
        decoded->certificate.checkpoint.validity_proof =
            (lxp_byte_span){validity, validity_length};
    if (status == LXP_OK)
        status = reader_u8(&reader, &attestation_count);
    if (status == LXP_OK &&
        (attestation_count == 0U ||
         attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS))
        status = LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; status == LXP_OK && index < attestation_count; ++index) {
        status = read_attestation(&reader,
                                  &decoded->certificate.attestations[index]);
        if (status == LXP_OK && index != 0U &&
            memcmp(decoded->certificate.attestations[index - 1U].guarantor_id,
                   decoded->certificate.attestations[index].guarantor_id,
                   32U) >= 0)
            status = LXP_ERR_NON_CANONICAL;
    }
    if (status == LXP_OK) status = reader_u8(&reader, &threshold);
    if (status == LXP_OK &&
        (threshold == 0U || threshold > attestation_count))
        status = LXP_ERR_ATTESTATION_THRESHOLD;
    if (status == LXP_OK) status = reader_u16(&reader, &reference_length);
    if (status == LXP_OK &&
        reference_length != FINALITY_SETTLEMENT_REFERENCE_BYTES)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = reader_take(&reader, reference_length, &reference);
    if (status == LXP_OK)
        status = settlement_reference_decode(
            (lxp_byte_span){reference, reference_length},
            &decoded->settlement_registration);
    if (status == LXP_OK) status = reader_finish(&reader);
    if (status == LXP_OK) {
        decoded->certificate.attestation_count = attestation_count;
        decoded->certificate.threshold = threshold;
        decoded->certificate.bonded_economic_guarantee = true;
        decoded->certificate.validity_proof_present = validity_length != 0U;
    }
    return status;
}

static lxp_result write_bond_record(
    evidence_writer *writer, const lxp_guarantor_bond_state *record)
{
    uint8_t amount[16];
    uint8_t flags = 0U;
    size_t index;
    lxp_result status;
    if (record == NULL || record->signer_authorization_count == 0U ||
        record->signer_authorization_count > UINT8_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_to_be(record->bond_amount, amount);
    if (status == LXP_OK) status = writer_bytes(writer, record->guarantor_id, 32U);
    if (status == LXP_OK) status = writer_bytes(writer, record->public_key, 33U);
    if (status == LXP_OK) status = writer_bytes(writer, amount, sizeof(amount));
    if (status == LXP_OK) status = writer_u64(writer, record->joined_epoch);
    if (status == LXP_OK) status = writer_u64(writer, record->removed_epoch);
    if (status == LXP_OK) status = writer_u64(writer, record->ejected_at_version);
    if (status == LXP_OK) status = writer_u8(writer, (uint8_t)record->signer_authorization_count);
    for (index = 0U; status == LXP_OK && index < record->signer_authorization_count; ++index) {
        const lxp_guarantor_signer_authorization *authorization =
            &record->signer_authorizations[index];
        status = writer_bytes(writer, authorization->public_key, 33U);
        if (status == LXP_OK) status = writer_u64(writer, authorization->active_from_epoch);
        if (status == LXP_OK) status = writer_u64(writer, authorization->active_until_epoch);
        if (status == LXP_OK) status = writer_u64(writer, authorization->set_version);
    }
    if (record->jailed) flags |= 1U;
    if (record->unresolved_slashing) flags |= 2U;
    if (record->active) flags |= 4U;
    if (status == LXP_OK) status = writer_u8(writer, flags);
    lxp_secure_zero(amount, sizeof(amount));
    return status;
}

static lxp_result read_bond_record(
    evidence_reader *reader, lxp_guarantor_bond_state *record)
{
    uint8_t amount[16];
    uint8_t authorization_count = 0U;
    uint8_t flags = 0U;
    size_t index;
    lxp_result status;
    if (reader == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    status = reader_copy(reader, record->guarantor_id, 32U);
    if (status == LXP_OK) status = reader_copy(reader, record->public_key, 33U);
    if (status == LXP_OK) status = reader_copy(reader, amount, sizeof(amount));
    if (status == LXP_OK) status = lxp_u128_from_be(amount, &record->bond_amount);
    if (status == LXP_OK) status = reader_u64(reader, &record->joined_epoch);
    if (status == LXP_OK) status = reader_u64(reader, &record->removed_epoch);
    if (status == LXP_OK) status = reader_u64(reader, &record->ejected_at_version);
    if (status == LXP_OK) status = reader_u8(reader, &authorization_count);
    if (status == LXP_OK &&
        (authorization_count == 0U ||
         authorization_count > LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS))
        status = LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; status == LXP_OK && index < authorization_count; ++index) {
        lxp_guarantor_signer_authorization *authorization =
            &record->signer_authorizations[index];
        status = reader_copy(reader, authorization->public_key, 33U);
        if (status == LXP_OK) status = reader_u64(reader, &authorization->active_from_epoch);
        if (status == LXP_OK) status = reader_u64(reader, &authorization->active_until_epoch);
        if (status == LXP_OK) status = reader_u64(reader, &authorization->set_version);
    }
    record->signer_authorization_count =
        status == LXP_OK ? authorization_count : 0U;
    if (status == LXP_OK) status = reader_u8(reader, &flags);
    if (status == LXP_OK && (flags & UINT8_C(0xf8)) != 0U)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        record->jailed = (flags & 1U) != 0U;
        record->unresolved_slashing = (flags & 2U) != 0U;
        record->active = (flags & 4U) != 0U;
    }
    lxp_secure_zero(amount, sizeof(amount));
    return status;
}

static size_t bond_record_length(const lxp_guarantor_bond_state *record)
{
    return record == NULL ? 0U : 107U +
        record->signer_authorization_count * 57U;
}

static lxp_result decode_finality_proof(
    lxp_byte_span proof, decoded_finality *decoded)
{
    evidence_reader reader = {proof.bytes, proof.length, 0U};
    uint16_t version;
    uint8_t count = 0U;
    uint8_t flags = 0U;
    uint8_t amount[16];
    uint16_t reference_length;
    const uint8_t *reference;
    size_t index;
    lxp_result status;
    if (proof.bytes == NULL || decoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded->bonded_set, 0, sizeof(decoded->bonded_set));
    (void)memset(&decoded->requirements, 0, sizeof(decoded->requirements));
    status = reader_u16(&reader, &version);
    if (status == LXP_OK && version != EVIDENCE_WIRE_VERSION)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_u64(&reader, &decoded->expected_registration_count);
    if (status == LXP_OK)
        status = reader_u64(&reader, &decoded->bonded_set.version);
    if (status == LXP_OK)
        status = reader_u64(&reader,
                            &decoded->bonded_set.last_governance_sequence);
    if (status == LXP_OK) status = reader_u8(&reader, &count);
    if (status == LXP_OK &&
        (count == 0U || count > LXP_MAX_GUARANTOR_ATTESTATIONS))
        status = LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        status = read_bond_record(&reader, &decoded->bonded_set.records[index]);
        if (status == LXP_OK && index != 0U &&
            memcmp(decoded->bonded_set.records[index - 1U].guarantor_id,
                   decoded->bonded_set.records[index].guarantor_id, 32U) >= 0)
            status = LXP_ERR_NON_CANONICAL;
    }
    decoded->bonded_set.count = status == LXP_OK ? count : 0U;
    if (status == LXP_OK)
        status = lxp_guarantor_set_validate(&decoded->bonded_set);
    if (status == LXP_OK)
        status = reader_u64(&reader,
                            &decoded->requirements.checkpoint_epoch);
    if (status == LXP_OK)
        status = reader_u64(&reader,
                            &decoded->requirements.challenge_window_end_ms);
    if (status == LXP_OK)
        status = reader_u64(&reader,
                            &decoded->requirements.checkpoint_deadline_ms);
    if (status == LXP_OK)
        status = reader_u64(&reader, &decoded->requirements.now_ms);
    if (status == LXP_OK) status = reader_u8(&reader, &count);
    if (status == LXP_OK &&
        (count == 0U || count > LXP_MAX_GUARANTOR_ATTESTATIONS))
        status = LXP_ERR_ATTESTATION_THRESHOLD;
    if (status == LXP_OK) decoded->requirements.threshold = count;
    if (status == LXP_OK) status = reader_copy(&reader, amount, sizeof(amount));
    if (status == LXP_OK)
        status = lxp_u128_from_be(amount, &decoded->requirements.minimum_bond);
    if (status == LXP_OK) status = reader_u8(&reader, &flags);
    if (status == LXP_OK && (flags & UINT8_C(0xfc)) != 0U)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        decoded->requirements.availability_challenges_answered =
            (flags & 1U) != 0U;
        decoded->requirements.equivocation_detected = (flags & 2U) != 0U;
    }
    if (status == LXP_OK)
        status = reader_copy(&reader,
                             decoded->registered_checkpoint_id, 32U);
    if (status == LXP_OK)
        status = reader_copy(&reader,
                             decoded->registered_resulting_root, 32U);
    if (status == LXP_OK)
        status = reader_u64(&reader, &decoded->registered_batch_number);
    if (status == LXP_OK)
        status = reader_u64(&reader, &decoded->registered_chain_id);
    if (status == LXP_OK)
        status = reader_copy(&reader, decoded->registered_contract, 20U);
    if (status == LXP_OK) status = reader_u16(&reader, &reference_length);
    if (status == LXP_OK &&
        reference_length != FINALITY_SETTLEMENT_REFERENCE_BYTES)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = reader_take(&reader, reference_length, &reference);
    if (status == LXP_OK)
        decoded->registered_reference =
            (lxp_byte_span){reference, reference_length};
    if (status == LXP_OK) status = reader_finish(&reader);
    lxp_secure_zero(amount, sizeof(amount));
    return status;
}

static lxp_result verify_finality_bundle(
    const lxp_daemon_evidence_store *store, lxp_byte_span checkpoint_payload,
    lxp_byte_span finality_proof, lxp_arena *arena,
    decoded_finality *decoded, lxp_checkpoint_registry_state *candidate,
    lxp_checkpoint_registration *registration, uint8_t checkpoint_id[32])
{
    lxp_result status;
    if (store == NULL || arena == NULL || decoded == NULL ||
        candidate == NULL || registration == NULL || checkpoint_id == NULL ||
        !store->initialized)
        return LXP_ERR_NON_CANONICAL;
    if (store->verify_finality_authority == NULL)
        return LXP_ERR_MODULE_DISABLED;
    (void)memset(decoded, 0, sizeof(*decoded));
    status = decode_checkpoint_payload(checkpoint_payload, decoded);
    if (status == LXP_OK) status = decode_finality_proof(finality_proof, decoded);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(
            &decoded->certificate.checkpoint, arena, checkpoint_id);
    if (status == LXP_OK)
        status = store->verify_finality_authority(
            store->finality_authority_context, &decoded->certificate,
            &decoded->bonded_set, &decoded->requirements,
            &decoded->settlement_registration);
    if (status == LXP_OK &&
        (decoded->certificate.checkpoint.header.network_id !=
             store->network_id ||
         decoded->expected_registration_count !=
             store->registry.registration_count ||
         decoded->requirements.equivocation_detected ||
         !decoded->requirements.availability_challenges_answered ||
         decoded->requirements.threshold != decoded->certificate.threshold ||
         decoded->requirements.checkpoint_epoch !=
             decoded->certificate.checkpoint.header.epoch ||
         decoded->bonded_set.version < store->latest_bonded_set_version ||
         lxp_ct_memcmp(checkpoint_id,
                       decoded->settlement_registration.checkpoint_id,
                       32U) != 0 ||
         lxp_ct_memcmp(checkpoint_id,
                       decoded->registered_checkpoint_id, 32U) != 0 ||
         lxp_ct_memcmp(decoded->certificate.checkpoint.header.resulting_state_root,
                       decoded->registered_resulting_root, 32U) != 0 ||
         decoded->certificate.checkpoint.header.batch_number !=
             decoded->registered_batch_number ||
         decoded->settlement_registration.paxeer_chain_id !=
             decoded->registered_chain_id ||
         decoded->certificate.attestations[0].paxeer_chain_id !=
             decoded->registered_chain_id ||
         lxp_ct_memcmp(
                       decoded->settlement_registration.settlement_contract,
                       decoded->registered_contract, 20U) != 0 ||
         lxp_ct_memcmp(decoded->certificate.attestations[0]
                           .paxeer_settlement_contract,
                       decoded->registered_contract, 20U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) {
        uint8_t reference[FINALITY_SETTLEMENT_REFERENCE_BYTES];
        status = settlement_reference_encode(
            &decoded->settlement_registration, reference);
        if (status == LXP_OK &&
            (decoded->registered_reference.length != sizeof(reference) ||
             lxp_ct_memcmp(decoded->registered_reference.bytes,
                           reference, sizeof(reference)) != 0))
            status = LXP_ERR_CONTEXT_MISMATCH;
    }
    if (status == LXP_OK) {
        *candidate = store->registry;
        status = lxp_checkpoint_register(
            candidate, &decoded->certificate, &decoded->bonded_set,
            &decoded->requirements, arena, registration);
    }
    if (status == LXP_OK &&
        (lxp_ct_memcmp(registration->checkpoint_id,
                       decoded->registered_checkpoint_id, 32U) != 0 ||
         lxp_ct_memcmp(registration->resulting_state_root,
                       decoded->registered_resulting_root, 32U) != 0 ||
         registration->batch_number != decoded->registered_batch_number ||
         candidate->registration_count !=
             decoded->expected_registration_count + 1U))
        status = LXP_ERR_CONTEXT_MISMATCH;
    return status;
}

static size_t finality_proof_length(const lxp_guarantor_set *bonded_set)
{
    size_t length = 2U + 8U + 8U + 8U + 1U;
    size_t index;
    if (bonded_set == NULL) return 0U;
    for (index = 0U; index < bonded_set->count; ++index)
        length += bond_record_length(&bonded_set->records[index]);
    return length + 8U + 8U + 8U + 8U + 1U + 16U + 1U +
        32U + 32U + 8U + 8U + 20U + 2U +
        FINALITY_SETTLEMENT_REFERENCE_BYTES;
}

lxp_result lxp_daemon_finality_evidence_encode(
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    uint64_t expected_registration_count,
    const lxp_daemon_settlement_registration_evidence *registration,
    lxp_arena *arena, lxp_byte_span *checkpoint_payload,
    lxp_byte_span *finality_proof)
{
    lxp_byte_span header;
    evidence_writer payload_writer;
    evidence_writer proof_writer;
    uint8_t reference[FINALITY_SETTLEMENT_REFERENCE_BYTES];
    uint8_t checkpoint_id[32];
    uint8_t amount[16];
    size_t order[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t payload_length;
    size_t proof_length;
    size_t index;
    void *payload_memory = NULL;
    void *proof_memory = NULL;
    lxp_result status;
    if (certificate == NULL || bonded_set == NULL || requirements == NULL ||
        registration == NULL || arena == NULL || checkpoint_payload == NULL ||
        finality_proof == NULL || certificate->attestation_count == 0U ||
        certificate->attestation_count > UINT8_MAX ||
        certificate->threshold == 0U || certificate->threshold > UINT8_MAX ||
        bonded_set->count == 0U || bonded_set->count > UINT8_MAX ||
        requirements->threshold == 0U || requirements->threshold > UINT8_MAX ||
        certificate->checkpoint.validity_proof.length >
            LXP_MAX_VALIDITY_PROOF_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_guarantor_set_validate(bonded_set);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(
            &certificate->checkpoint, arena, checkpoint_id);
    if (status == LXP_OK &&
        lxp_ct_memcmp(checkpoint_id, registration->checkpoint_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = settlement_reference_encode(registration, reference);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(&certificate->checkpoint.header,
                                         arena, &header);
    payload_length = 2U + 4U + LXP_BATCH_HEADER_ENCODED_SIZE + 4U +
        certificate->checkpoint.validity_proof.length + 1U +
        certificate->attestation_count * FINALITY_ATTESTATION_BYTES +
        1U + 2U + sizeof(reference);
    proof_length = finality_proof_length(bonded_set);
    if (status == LXP_OK &&
        (payload_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
         proof_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
         payload_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES -
                              proof_length))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, payload_length,
                                 _Alignof(uint64_t), &payload_memory);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, proof_length,
                                 _Alignof(uint64_t), &proof_memory);
    payload_writer = (evidence_writer){payload_memory, payload_length, 0U};
    proof_writer = (evidence_writer){proof_memory, proof_length, 0U};
    if (status == LXP_OK) status = writer_u16(&payload_writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK) status = writer_u32(&payload_writer, (uint32_t)header.length);
    if (status == LXP_OK) status = writer_bytes(&payload_writer, header.bytes, header.length);
    if (status == LXP_OK) status = writer_u32(&payload_writer, (uint32_t)certificate->checkpoint.validity_proof.length);
    if (status == LXP_OK) status = writer_bytes(&payload_writer, certificate->checkpoint.validity_proof.bytes, certificate->checkpoint.validity_proof.length);
    if (status == LXP_OK) status = writer_u8(&payload_writer, (uint8_t)certificate->attestation_count);
    for (index = 0U; status == LXP_OK && index < certificate->attestation_count; ++index)
        status = write_attestation(&payload_writer, &certificate->attestations[index]);
    if (status == LXP_OK) status = writer_u8(&payload_writer, (uint8_t)certificate->threshold);
    if (status == LXP_OK) status = writer_u16(&payload_writer, sizeof(reference));
    if (status == LXP_OK) status = writer_bytes(&payload_writer, reference, sizeof(reference));
    for (index = 0U; index < bonded_set->count; ++index) order[index] = index;
    for (index = 1U; index < bonded_set->count; ++index) {
        size_t selected = order[index];
        size_t position = index;
        while (position != 0U && memcmp(
            bonded_set->records[order[position - 1U]].guarantor_id,
            bonded_set->records[selected].guarantor_id, 32U) > 0) {
            order[position] = order[position - 1U];
            --position;
        }
        order[position] = selected;
    }
    if (status == LXP_OK) status = writer_u16(&proof_writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK) status = writer_u64(&proof_writer, expected_registration_count);
    if (status == LXP_OK) status = writer_u64(&proof_writer, bonded_set->version);
    if (status == LXP_OK) status = writer_u64(&proof_writer, bonded_set->last_governance_sequence);
    if (status == LXP_OK) status = writer_u8(&proof_writer, (uint8_t)bonded_set->count);
    for (index = 0U; status == LXP_OK && index < bonded_set->count; ++index)
        status = write_bond_record(&proof_writer, &bonded_set->records[order[index]]);
    if (status == LXP_OK) status = writer_u64(&proof_writer, requirements->checkpoint_epoch);
    if (status == LXP_OK) status = writer_u64(&proof_writer, requirements->challenge_window_end_ms);
    if (status == LXP_OK) status = writer_u64(&proof_writer, requirements->checkpoint_deadline_ms);
    if (status == LXP_OK) status = writer_u64(&proof_writer, requirements->now_ms);
    if (status == LXP_OK) status = writer_u8(&proof_writer, (uint8_t)requirements->threshold);
    if (status == LXP_OK) status = lxp_u128_to_be(requirements->minimum_bond, amount);
    if (status == LXP_OK) status = writer_bytes(&proof_writer, amount, sizeof(amount));
    if (status == LXP_OK) status = writer_u8(&proof_writer,
        (requirements->availability_challenges_answered ? 1U : 0U) |
        (requirements->equivocation_detected ? 2U : 0U));
    if (status == LXP_OK) status = writer_bytes(&proof_writer, checkpoint_id, 32U);
    if (status == LXP_OK) status = writer_bytes(&proof_writer, certificate->checkpoint.header.resulting_state_root, 32U);
    if (status == LXP_OK) status = writer_u64(&proof_writer, certificate->checkpoint.header.batch_number);
    if (status == LXP_OK)
        status = writer_u64(&proof_writer, registration->paxeer_chain_id);
    if (status == LXP_OK)
        status = writer_bytes(&proof_writer,
                              registration->settlement_contract, 20U);
    if (status == LXP_OK) status = writer_u16(&proof_writer, sizeof(reference));
    if (status == LXP_OK) status = writer_bytes(&proof_writer, reference, sizeof(reference));
    if (status == LXP_OK &&
        (payload_writer.cursor != payload_length ||
         proof_writer.cursor != proof_length))
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        *checkpoint_payload = (lxp_byte_span){payload_memory, payload_length};
        *finality_proof = (lxp_byte_span){proof_memory, proof_length};
    }
    lxp_secure_zero(amount, sizeof(amount));
    lxp_secure_zero(reference, sizeof(reference));
    return status;
}

static lxp_result encode_record(
    lxp_daemon_evidence_kind kind, uint32_t network_id, uint64_t ordinal,
    const uint8_t key[32], lxp_byte_span payload, lxp_byte_span proof,
    uint8_t **body, uint32_t *body_length, uint8_t digest[32])
{
    size_t length;
    uint8_t *encoded;
    lxp_result status;
    if (key == NULL || body == NULL || body_length == NULL || digest == NULL ||
        network_id == 0U || ordinal == 0U ||
        kind < LXP_DAEMON_EVIDENCE_ACCOUNT ||
        kind > LXP_DAEMON_EVIDENCE_FINALITY ||
        (payload.bytes == NULL && payload.length != 0U) ||
        (proof.bytes == NULL && proof.length != 0U) ||
        payload.length > UINT32_MAX || proof.length > UINT32_MAX ||
        payload.length > SIZE_MAX - EVIDENCE_RECORD_FIXED_BYTES -
                             EVIDENCE_RECORD_DIGEST_BYTES ||
        proof.length > SIZE_MAX - EVIDENCE_RECORD_FIXED_BYTES -
                           EVIDENCE_RECORD_DIGEST_BYTES - payload.length)
        return LXP_ERR_NON_CANONICAL;
    length = EVIDENCE_RECORD_FIXED_BYTES + payload.length + proof.length +
             EVIDENCE_RECORD_DIGEST_BYTES;
    if (length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    encoded = (uint8_t *)malloc(length);
    if (encoded == NULL) return LXP_ERR_IO;
    (void)memcpy(encoded, evidence_magic, sizeof(evidence_magic));
    encoded[4] = EVIDENCE_RECORD_VERSION;
    encoded[5] = (uint8_t)kind;
    encoded[6] = 0U;
    encoded[7] = 0U;
    write_u32(encoded + 8U, network_id);
    write_u64(encoded + 12U, ordinal);
    (void)memcpy(encoded + 20U, key, 32U);
    write_u32(encoded + 52U, (uint32_t)payload.length);
    write_u32(encoded + 56U, (uint32_t)proof.length);
    if (payload.length != 0U)
        (void)memcpy(encoded + EVIDENCE_RECORD_FIXED_BYTES,
                     payload.bytes, payload.length);
    if (proof.length != 0U)
        (void)memcpy(encoded + EVIDENCE_RECORD_FIXED_BYTES + payload.length,
                     proof.bytes, proof.length);
    status = lxp_hash_sha256(encoded,
                             length - EVIDENCE_RECORD_DIGEST_BYTES, digest);
    if (status == LXP_OK) {
        (void)memcpy(encoded + length - EVIDENCE_RECORD_DIGEST_BYTES,
                     digest, EVIDENCE_RECORD_DIGEST_BYTES);
        *body = encoded;
        *body_length = (uint32_t)length;
    } else {
        free(encoded);
    }
    return status;
}

static lxp_result decode_record(const uint8_t *body, size_t body_length,
                                decoded_record *record)
{
    uint32_t payload_length;
    uint32_t proof_length;
    size_t expected;
    uint8_t digest[32];
    lxp_result status;
    if (body == NULL || record == NULL ||
        body_length < EVIDENCE_RECORD_FIXED_BYTES +
                          EVIDENCE_RECORD_DIGEST_BYTES)
        return LXP_ERR_LOG_CORRUPT;
    payload_length = read_u32(body + 52U);
    proof_length = read_u32(body + 56U);
    expected = EVIDENCE_RECORD_FIXED_BYTES + (size_t)payload_length +
               (size_t)proof_length + EVIDENCE_RECORD_DIGEST_BYTES;
    if (expected != body_length ||
        memcmp(body, evidence_magic, sizeof(evidence_magic)) != 0 ||
        body[4] != EVIDENCE_RECORD_VERSION || body[6] != 0U || body[7] != 0U ||
        body[5] < LXP_DAEMON_EVIDENCE_ACCOUNT ||
        body[5] > LXP_DAEMON_EVIDENCE_FINALITY || read_u32(body + 8U) == 0U ||
        read_u64(body + 12U) == 0U)
        return LXP_ERR_LOG_CORRUPT;
    status = lxp_hash_sha256(body,
                             body_length - EVIDENCE_RECORD_DIGEST_BYTES,
                             digest);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(digest,
                      body + body_length - EVIDENCE_RECORD_DIGEST_BYTES,
                      sizeof(digest)) != 0)
        return LXP_ERR_LOG_CORRUPT;
    (void)memset(record, 0, sizeof(*record));
    record->kind = (lxp_daemon_evidence_kind)body[5];
    record->network_id = read_u32(body + 8U);
    record->ordinal = read_u64(body + 12U);
    (void)memcpy(record->key, body + 20U, 32U);
    record->payload = (lxp_byte_span){
        body + EVIDENCE_RECORD_FIXED_BYTES, payload_length};
    record->proof = (lxp_byte_span){
        body + EVIDENCE_RECORD_FIXED_BYTES + payload_length, proof_length};
    (void)memcpy(record->digest, digest, sizeof(digest));
    return LXP_OK;
}

static lxp_result read_log_record(
    const lxp_daemon_evidence_store *store, uint64_t offset,
    lxp_log_record_header *header, uint8_t **body, decoded_record *record)
{
    lxp_result status;
    if (store == NULL || store->log == NULL || header == NULL || body == NULL ||
        record == NULL || offset >= store->log->write_offset)
        return LXP_ERR_NON_CANONICAL;
    *body = NULL;
    status = lxp_log_read(store->log, offset, header, NULL, 0U);
    if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) return status;
    if (header->record_kind != (uint8_t)LXP_LOG_CHECKPOINT ||
        header->body_length < EVIDENCE_RECORD_FIXED_BYTES +
                                  EVIDENCE_RECORD_DIGEST_BYTES)
        return LXP_ERR_LOG_CORRUPT;
    *body = (uint8_t *)malloc(header->body_length);
    if (*body == NULL) return LXP_ERR_IO;
    status = lxp_log_read(store->log, offset, header, *body,
                          header->body_length);
    if (status == LXP_OK) status = decode_record(*body, header->body_length, record);
    if (status != LXP_OK) {
        free(*body);
        *body = NULL;
    }
    return status;
}

static lxp_result find_record(
    const lxp_daemon_evidence_store *store, lxp_daemon_evidence_kind kind,
    const uint8_t key[32], lxp_byte_span expected_payload,
    lxp_byte_span expected_proof, uint8_t digest[32], bool *present,
    bool *exact)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    if (store == NULL || key == NULL || present == NULL || exact == NULL)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    *exact = false;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        decoded_record record;
        uint8_t *body = NULL;
        status = read_log_record(store, offset, &header, &body, &record);
        if (status == LXP_OK && record.kind == kind &&
            lxp_ct_memcmp(record.key, key, 32U) == 0) {
            *present = true;
            *exact = record.payload.length == expected_payload.length &&
                record.proof.length == expected_proof.length &&
                lxp_ct_memcmp(record.payload.bytes, expected_payload.bytes,
                              expected_payload.length) == 0 &&
                lxp_ct_memcmp(record.proof.bytes, expected_proof.bytes,
                              expected_proof.length) == 0;
            if (digest != NULL) (void)memcpy(digest, record.digest, 32U);
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
        free(body);
        if (*present) break;
    }
    return status;
}

static lxp_result append_record(
    lxp_daemon_evidence_store *store, lxp_daemon_evidence_kind kind,
    const uint8_t key[32], lxp_byte_span payload, lxp_byte_span proof,
    uint8_t record_digest[32], bool *appended)
{
    uint8_t *body = NULL;
    uint32_t body_length = 0U;
    uint8_t digest[32];
    uint64_t ordinal;
    bool present;
    bool exact;
    lxp_result status;
    if (store == NULL || !store->initialized || key == NULL || appended == NULL ||
        lxp_ct_is_zero(key, 32U))
        return LXP_ERR_NON_CANONICAL;
    *appended = false;
    status = find_record(store, kind, key, payload, proof, digest,
                         &present, &exact);
    if (status != LXP_OK) return status;
    if (present) {
        if (!exact) return LXP_ERR_LOG_CORRUPT;
        if (record_digest != NULL) (void)memcpy(record_digest, digest, 32U);
        return LXP_OK;
    }
    if (store->last_ordinal == UINT64_MAX || store->record_count == UINT64_MAX)
        return LXP_ERR_OVERFLOW;
    ordinal = store->last_ordinal + 1U;
    status = encode_record(kind, store->network_id, ordinal, key,
                           payload, proof, &body, &body_length, digest);
    if (status == LXP_OK)
        status = lxp_log_append(store->log, LXP_LOG_CHECKPOINT, ordinal,
                                body, body_length, NULL);
    if (status == LXP_OK) status = lxp_log_write_boundary(store->log);
    if (status == LXP_OK) {
        store->last_ordinal = ordinal;
        ++store->record_count;
        *appended = true;
        if (record_digest != NULL) (void)memcpy(record_digest, digest, 32U);
    }
    if (body != NULL) {
        lxp_secure_zero(body, body_length);
        free(body);
    }
    return status;
}

static lxp_result validate_recovered_record(
    lxp_daemon_evidence_store *store, const decoded_record *record,
    lxp_arena *arena)
{
    lxp_result status;
    if (store == NULL || record == NULL || arena == NULL ||
        record->network_id != store->network_id ||
        record->ordinal != store->last_ordinal + 1U)
        return LXP_ERR_LOG_CORRUPT;
    if (record->kind == LXP_DAEMON_EVIDENCE_ACCOUNT) {
        lxp_daemon_account_evidence evidence;
        status = decode_account_payload(record->payload, arena, &evidence);
        if (status == LXP_OK && !authorizations_equal(
                &evidence.signed_header.authorization,
                &store->authorization))
            status = LXP_ERR_AUTH_SCOPE;
        if (status == LXP_OK)
            status = verify_account_evidence(&evidence, store->network_id,
                                             arena);
        if (status == LXP_OK) {
            uint8_t key_material[64];
            uint8_t key[32];
            (void)memcpy(key_material, evidence.account_id, 32U);
            (void)memcpy(key_material + 32U,
                         evidence.resulting_state_root, 32U);
            status = lxp_hash_sha256(key_material, sizeof(key_material), key);
            if (status == LXP_OK &&
                lxp_ct_memcmp(key, record->key, 32U) != 0)
                status = LXP_ERR_CONTEXT_MISMATCH;
        }
    } else if (record->kind == LXP_DAEMON_EVIDENCE_ACTIVITY) {
        lxp_daemon_activity_evidence evidence;
        status = decode_activity_payload(record->payload, arena, &evidence);
        if (status == LXP_OK && !authorizations_equal(
                &evidence.signed_header.authorization,
                &store->authorization))
            status = LXP_ERR_AUTH_SCOPE;
        if (status == LXP_OK)
            status = verify_activity_evidence(&evidence, store->network_id,
                                              arena);
        if (status == LXP_OK && lxp_ct_memcmp(
                evidence.activity_id, record->key, 32U) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
    } else if (record->kind == LXP_DAEMON_EVIDENCE_FINALITY) {
        decoded_finality decoded;
        lxp_checkpoint_registry_state candidate;
        lxp_checkpoint_registration registration;
        uint8_t checkpoint_id[32];
        status = verify_finality_bundle(
            store, record->payload, record->proof, arena, &decoded,
            &candidate, &registration, checkpoint_id);
        if (status == LXP_OK && lxp_ct_memcmp(
                checkpoint_id, record->key, 32U) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK) {
            store->registry = candidate;
            store->latest_finalized_batch = registration.batch_number;
            store->latest_bonded_set_version = decoded.bonded_set.version;
            (void)memcpy(store->latest_checkpoint_id, checkpoint_id, 32U);
        }
    } else {
        status = LXP_ERR_INVALID_TAG;
    }
    return status == LXP_OK ? LXP_OK : LXP_ERR_LOG_CORRUPT;
}

lxp_result lxp_daemon_evidence_open(
    lxp_daemon_evidence_store *store, lxp_log *log, uint32_t network_id,
    const lxp_sequencer_authorization *authorization,
    const uint8_t initial_settlement_anchor[32], bool allow_initialize,
    lxp_daemon_finality_authority_verify_fn verify_finality_authority,
    void *finality_authority_context, lxp_arena *arena)
{
    uint64_t offset = 0U;
    uint8_t (*identities)[33] = NULL;
    lxp_result status;
    if (store == NULL || log == NULL || log->descriptor < 0 ||
        authorization == NULL || !authorization->authorized ||
        authorization->first_batch_number == 0U ||
        authorization->last_batch_number <
            authorization->first_batch_number ||
        lxp_ct_is_zero(authorization->sequencer_id, 32U) ||
        lxp_ct_is_zero(authorization->public_key, 32U) ||
        network_id == 0U || initial_settlement_anchor == NULL ||
        lxp_ct_is_zero(initial_settlement_anchor, 32U) || arena == NULL ||
        ((verify_finality_authority == NULL) !=
         (finality_authority_context == NULL)))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_log_recover_complete_records(log, NULL, NULL);
    if (status != LXP_OK) return status;
    (void)memset(store, 0, sizeof(*store));
    store->log = log;
    store->network_id = network_id;
    store->authorization = *authorization;
    store->verify_finality_authority = verify_finality_authority;
    store->finality_authority_context = finality_authority_context;
    (void)memcpy(store->registry.finalisation.settlement_anchor,
                 initial_settlement_anchor, 32U);
    store->initialized = true;
    while (status == LXP_OK && offset < log->write_offset) {
        lxp_log_record_header header;
        decoded_record record;
        uint8_t *body = NULL;
        size_t index;
        size_t mark = lxp_arena_mark(arena);
        status = read_log_record(store, offset, &header, &body, &record);
        for (index = 0U; status == LXP_OK &&
             index < store->record_count; ++index)
            if (identities[index][0] == (uint8_t)record.kind &&
                lxp_ct_memcmp(identities[index] + 1U,
                              record.key, 32U) == 0)
                status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK)
            status = validate_recovered_record(store, &record, arena);
        if (status == LXP_OK) {
            uint8_t (*resized)[33];
            if (store->record_count == SIZE_MAX / sizeof(*identities))
                status = LXP_ERR_OVERFLOW;
            else {
                resized = (uint8_t (*)[33])realloc(
                    identities,
                    (size_t)(store->record_count + 1U) *
                        sizeof(*identities));
                if (resized == NULL)
                    status = LXP_ERR_IO;
                else {
                    identities = resized;
                    identities[store->record_count][0] =
                        (uint8_t)record.kind;
                    (void)memcpy(identities[store->record_count] + 1U,
                                 record.key, 32U);
                }
            }
        }
        if (status == LXP_OK) {
            store->last_ordinal = record.ordinal;
            ++store->record_count;
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
        free(body);
        (void)lxp_arena_reset(arena, mark);
    }
    if (status == LXP_OK && !allow_initialize && store->record_count == 0U)
        status = LXP_ERR_PROJECTION_STALE;
    if (identities != NULL) {
        lxp_secure_zero(identities,
                        (size_t)store->record_count * sizeof(*identities));
        free(identities);
    }
    if (status != LXP_OK) (void)memset(store, 0, sizeof(*store));
    return status;
}

lxp_result lxp_daemon_evidence_bind_finality_authority(
    lxp_daemon_evidence_store *store,
    lxp_daemon_finality_authority_verify_fn verify, void *context)
{
    if (store == NULL || !store->initialized || verify == NULL ||
        context == NULL || store->verify_finality_authority != NULL)
        return LXP_ERR_NON_CANONICAL;
    store->verify_finality_authority = verify;
    store->finality_authority_context = context;
    return LXP_OK;
}

lxp_result lxp_daemon_account_evidence_build(
    const lxp_kernel *kernel, uint32_t network_id,
    const uint8_t account_id[32],
    const uint8_t receipt_digest[32], uint64_t observed_at_ms,
    lxp_byte_span canonical_receipt,
    const lxp_merkle_proof *receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena, lxp_daemon_account_evidence *evidence)
{
    const lx_account_registry *accounts;
    const lx_account *account = NULL;
    lxp_batch_header header;
    uint8_t candidate_root[32];
    size_t index;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || account_id == NULL ||
        network_id == 0U || receipt_digest == NULL || observed_at_ms == 0U ||
        canonical_receipt.bytes == NULL || canonical_receipt.length == 0U ||
        receipt_proof == NULL || authorization == NULL ||
        canonical_header.bytes == NULL || header_signature == NULL ||
        arena == NULL || evidence == NULL || kernel->state->accounts == NULL)
        return LXP_ERR_NON_CANONICAL;
    accounts = kernel->state->accounts;
    if (accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; index < accounts->count; ++index)
        if (lxp_ct_memcmp(accounts->accounts[index].id,
                          account_id, 32U) == 0) {
            if (account != NULL) return LXP_FATAL_INVARIANT;
            account = &accounts->accounts[index];
        }
    if (account == NULL) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    (void)memset(evidence, 0, sizeof(*evidence));
    (void)memcpy(evidence->account_id, account_id, 32U);
    (void)memcpy(evidence->receipt_digest, receipt_digest, 32U);
    evidence->observed_at_ms = observed_at_ms;
    evidence->canonical_receipt = canonical_receipt;
    evidence->receipt_proof = *receipt_proof;
    evidence->signed_header.authorization = *authorization;
    evidence->signed_header.canonical_header = canonical_header;
    (void)memcpy(evidence->signed_header.signature, header_signature, 64U);
    status = verify_signed_header(&evidence->signed_header,
                                  network_id, arena, &header);
    if (status == LXP_OK &&
        (header.network_id == 0U || header.last_sequence == UINT64_MAX ||
         kernel->state->next_sequence != header.last_sequence + 1U ||
         lxp_ct_memcmp(header.resulting_state_root,
                       kernel->current_state_root, 32U) != 0))
        status = LXP_ERR_PROJECTION_STALE;
    if (status == LXP_OK) {
        evidence->observed_sequence = header.last_sequence;
        status = lx_account_state_leaf_material(
            account, evidence->account_leaf_key,
            evidence->account_leaf_value,
            &evidence->account_leaf_value_length);
    }
    if (status == LXP_OK)
        status = lx_account_registry_proof(
            accounts, account_id, evidence->account_root,
            &evidence->account_proof);
    if (status == LXP_OK)
        status = lxp_state_subtree_proof(
            kernel, 0U, account_tree_key, sizeof(account_tree_key) - 1U,
            evidence->universal_root, &evidence->account_tree_proof);
    if (status == LXP_OK)
        status = lxp_state_root_proof(
            kernel, 0U, evidence->resulting_state_root,
            &evidence->universal_root_proof);
    if (status == LXP_OK)
        status = lxp_state_root(kernel, candidate_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(candidate_root, kernel->current_state_root, 32U) != 0 ||
         lxp_ct_memcmp(evidence->resulting_state_root,
                       kernel->current_state_root, 32U) != 0))
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = verify_account_evidence(evidence, header.network_id, arena);
    return status;
}

lxp_result lxp_daemon_account_evidence_publish(
    lxp_daemon_evidence_store *store,
    const lxp_daemon_account_evidence *evidence, lxp_arena *arena,
    uint8_t record_digest[32])
{
    uint8_t key_material[64];
    uint8_t key[32];
    uint8_t *payload;
    size_t payload_length;
    bool appended;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        evidence == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = authorizations_equal(&evidence->signed_header.authorization,
                                  &store->authorization) ?
        LXP_OK : LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = verify_account_evidence(evidence, store->network_id, arena);
    payload_length = account_payload_length(evidence);
    payload = status == LXP_OK ? (uint8_t *)malloc(payload_length) : NULL;
    if (status == LXP_OK && payload == NULL) status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = encode_account_payload(evidence, payload, payload_length,
                                        &payload_length);
    if (status == LXP_OK) {
        (void)memcpy(key_material, evidence->account_id, 32U);
        (void)memcpy(key_material + 32U, evidence->resulting_state_root, 32U);
        status = lxp_hash_sha256(key_material, sizeof(key_material), key);
    }
    if (status == LXP_OK)
        status = append_record(store, LXP_DAEMON_EVIDENCE_ACCOUNT, key,
            (lxp_byte_span){payload, payload_length},
            (lxp_byte_span){NULL, 0U}, record_digest, &appended);
    if (payload != NULL) {
        lxp_secure_zero(payload, payload_length);
        free(payload);
    }
    return status;
}

lxp_result lxp_daemon_account_evidence_publish_batch(
    lxp_daemon_evidence_store *store, const lxp_kernel *kernel,
    lxp_byte_span canonical_head_receipt,
    const lxp_merkle_proof *head_receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena)
{
    lxp_batch_header header;
    lxp_receipt receipt;
    const lx_account_registry *accounts;
    uint8_t receipt_digest[32];
    size_t index;
    size_t mark;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        kernel == NULL || kernel->state == NULL ||
        kernel->state->accounts == NULL ||
        canonical_head_receipt.bytes == NULL ||
        canonical_head_receipt.length == 0U || head_receipt_proof == NULL ||
        authorization == NULL || canonical_header.bytes == NULL ||
        header_signature == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!authorizations_equal(authorization, &store->authorization))
        return LXP_ERR_AUTH_SCOPE;
    accounts = kernel->state->accounts;
    if (accounts->count == 0U ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_PROJECTION_STALE;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_header_decode(canonical_header.bytes,
                                     canonical_header.length, &header);
    if (status == LXP_OK)
        status = lxp_receipt_decode(canonical_head_receipt.bytes,
                                    canonical_head_receipt.length,
                                    true, &receipt);
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, receipt_digest);
    if (status == LXP_OK &&
        (header.network_id != store->network_id ||
         receipt.global_sequence != header.last_sequence ||
         receipt.timestamp != header.timestamp_ms ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       header.resulting_state_root, 32U) != 0 ||
         lxp_ct_memcmp(kernel->current_state_root,
                       header.resulting_state_root, 32U) != 0 ||
         kernel->state->next_sequence == 0U ||
         kernel->state->next_sequence - 1U != header.last_sequence))
        status = LXP_ERR_PROJECTION_STALE;
    for (index = 0U; status == LXP_OK && index < accounts->count; ++index) {
        lxp_daemon_account_evidence evidence;
        size_t item_mark = lxp_arena_mark(arena);
        status = lxp_daemon_account_evidence_build(
            kernel, store->network_id, accounts->accounts[index].id,
            receipt_digest, receipt.timestamp, canonical_head_receipt,
            head_receipt_proof, authorization, canonical_header,
            header_signature, arena, &evidence);
        if (status == LXP_OK)
            status = lxp_daemon_account_evidence_publish(
                store, &evidence, arena, NULL);
        (void)lxp_arena_reset(arena, item_mark);
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_daemon_account_evidence_lookup(
    const lxp_daemon_evidence_store *store, const uint8_t account_id[32],
    const uint8_t resulting_state_root[32], lxp_arena *arena,
    lxp_daemon_account_evidence *evidence)
{
    uint8_t key_material[64];
    uint8_t key[32];
    uint64_t offset = 0U;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        account_id == NULL || resulting_state_root == NULL || arena == NULL ||
        evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key_material, account_id, 32U);
    (void)memcpy(key_material + 32U, resulting_state_root, 32U);
    status = lxp_hash_sha256(key_material, sizeof(key_material), key);
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        decoded_record record;
        uint8_t *body = NULL;
        status = read_log_record(store, offset, &header, &body, &record);
        if (status == LXP_OK &&
            record.kind == LXP_DAEMON_EVIDENCE_ACCOUNT &&
            lxp_ct_memcmp(record.key, key, 32U) == 0) {
            status = decode_account_payload(record.payload, arena, evidence);
            if (status == LXP_OK && !authorizations_equal(
                    &evidence->signed_header.authorization,
                    &store->authorization))
                status = LXP_ERR_AUTH_SCOPE;
            if (status == LXP_OK)
                status = verify_account_evidence(evidence, store->network_id,
                                                 arena);
            free(body);
            return status;
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
        free(body);
    }
    return status == LXP_OK ? LXP_ERR_UNKNOWN_FIELD : status;
}

lxp_result lxp_daemon_account_evidence_lookup_batch(
    const lxp_daemon_evidence_store *store, const uint8_t account_id[32],
    uint64_t batch_number, lxp_arena *arena,
    lxp_daemon_account_evidence *evidence)
{
    uint64_t offset = 0U;
    bool found = false;
    lxp_result status = LXP_OK;
    if (store == NULL || !store->initialized || store->log == NULL ||
        account_id == NULL || batch_number == 0U || arena == NULL ||
        evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header log_header;
        decoded_record record;
        uint8_t *body = NULL;
        status = read_log_record(store, offset, &log_header, &body, &record);
        if (status == LXP_OK &&
            record.kind == LXP_DAEMON_EVIDENCE_ACCOUNT) {
            lxp_daemon_account_evidence candidate;
            lxp_batch_header header;
            size_t candidate_mark = lxp_arena_mark(arena);
            status = decode_account_payload(record.payload, arena, &candidate);
            if (status == LXP_OK && !authorizations_equal(
                    &candidate.signed_header.authorization,
                    &store->authorization))
                status = LXP_ERR_AUTH_SCOPE;
            if (status == LXP_OK)
                status = lxp_batch_header_decode(
                    candidate.signed_header.canonical_header.bytes,
                    candidate.signed_header.canonical_header.length,
                    &header);
            if (status == LXP_OK && header.batch_number == batch_number &&
                lxp_ct_memcmp(candidate.account_id, account_id, 32U) == 0) {
                if (found)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    status = verify_account_evidence(
                        &candidate, store->network_id, arena);
                    if (status == LXP_OK) {
                        *evidence = candidate;
                        found = true;
                    }
                }
            }
            if (!found || status != LXP_OK)
                (void)lxp_arena_reset(arena, candidate_mark);
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)log_header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + log_header.body_length;
        }
        free(body);
    }
    return status != LXP_OK ? status :
        found ? LXP_OK : LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_daemon_account_evidence_wire_encode(
    const lxp_daemon_evidence_store *store,
    const lxp_daemon_account_evidence *latest_evidence,
    const lxp_kernel *latest_kernel, uint32_t network_id,
    const uint8_t account_id[32], uint8_t selector_kind,
    uint64_t selector_batch,
    const uint8_t selector_checkpoint_id[32],
    lxp_arena *arena, lxp_byte_span *canonical_value,
    lxp_byte_span *proof_material)
{
    lxp_daemon_account_evidence stored_evidence;
    lxp_daemon_finality_evidence stored_checkpoint;
    const lxp_daemon_account_evidence *evidence = latest_evidence;
    const lxp_daemon_finality_evidence *checkpoint = NULL;
    lxp_batch_header header;
    evidence_writer writer;
    uint8_t *value = NULL;
    uint8_t *proof = NULL;
    size_t selector_length;
    size_t checkpoint_length = 0U;
    size_t proof_length;
    void *allocation;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        network_id == 0U || network_id != store->network_id ||
        account_id == NULL || lxp_ct_is_zero(account_id, 32U) ||
        arena == NULL || canonical_value == NULL || proof_material == NULL ||
        (selector_kind != 1U && selector_kind != 2U && selector_kind != 3U))
        return LXP_ERR_NON_CANONICAL;
    if ((selector_kind == 1U &&
         (selector_batch != 0U || selector_checkpoint_id != NULL ||
          latest_evidence == NULL || latest_kernel == NULL ||
          latest_kernel->state == NULL)) ||
        (selector_kind == 2U &&
         (selector_batch == 0U || selector_checkpoint_id != NULL ||
          latest_evidence != NULL || latest_kernel != NULL)) ||
        (selector_kind == 3U &&
         (selector_batch != 0U || selector_checkpoint_id == NULL ||
          latest_evidence != NULL || latest_kernel != NULL ||
          lxp_ct_is_zero(selector_checkpoint_id, 32U))))
        return LXP_ERR_NON_CANONICAL;
    if (selector_kind == 2U) {
        status = lxp_daemon_account_evidence_lookup_batch(
            store, account_id, selector_batch, arena, &stored_evidence);
        if (status == LXP_OK) evidence = &stored_evidence;
    } else if (selector_kind == 3U) {
        status = lxp_daemon_finality_evidence_lookup(
            store, selector_checkpoint_id, 0U, arena, &stored_checkpoint);
        if (status == LXP_OK)
            status = lxp_daemon_account_evidence_lookup_batch(
                store, account_id, stored_checkpoint.batch_number, arena,
                &stored_evidence);
        if (status == LXP_OK) {
            evidence = &stored_evidence;
            checkpoint = &stored_checkpoint;
        }
    } else {
        status = LXP_OK;
    }
    if (status == LXP_OK &&
        (evidence == NULL ||
         lxp_ct_memcmp(evidence->account_id, account_id, 32U) != 0 ||
         !authorizations_equal(&evidence->signed_header.authorization,
                               &store->authorization)))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = verify_account_evidence(evidence, network_id, arena);
    if (status == LXP_OK)
        status = lxp_batch_header_decode(
            evidence->signed_header.canonical_header.bytes,
            evidence->signed_header.canonical_header.length, &header);
    if (status == LXP_OK && selector_kind == 1U &&
        (latest_kernel->state->next_sequence == 0U ||
         latest_kernel->state->next_sequence - 1U != header.last_sequence ||
         lxp_ct_memcmp(latest_kernel->current_state_root,
                       header.resulting_state_root, 32U) != 0))
        status = LXP_ERR_PROJECTION_STALE;
    if (status == LXP_OK && selector_kind == 2U &&
        header.batch_number != selector_batch)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK && selector_kind == 3U &&
        (lxp_ct_memcmp(selector_checkpoint_id,
                       checkpoint->checkpoint_id, 32U) != 0 ||
         checkpoint->batch_number != header.batch_number ||
         lxp_ct_memcmp(checkpoint->resulting_state_root,
                       header.resulting_state_root, 32U) != 0 ||
         checkpoint->checkpoint_payload.bytes == NULL ||
         checkpoint->checkpoint_payload.length == 0U ||
         checkpoint->checkpoint_payload.length > UINT32_MAX ||
         checkpoint->finality_proof.bytes == NULL ||
         checkpoint->finality_proof.length == 0U ||
         checkpoint->finality_proof.length > UINT32_MAX))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK && selector_kind == 3U &&
        (checkpoint->checkpoint_payload.length <
             6U + evidence->signed_header.canonical_header.length ||
         read_u16(checkpoint->checkpoint_payload.bytes) !=
             EVIDENCE_WIRE_VERSION ||
         read_u32(checkpoint->checkpoint_payload.bytes + 2U) !=
             evidence->signed_header.canonical_header.length ||
         lxp_ct_memcmp(
             checkpoint->checkpoint_payload.bytes + 6U,
             evidence->signed_header.canonical_header.bytes,
             evidence->signed_header.canonical_header.length) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    selector_length = selector_kind == 1U ? 1U :
        selector_kind == 2U ? 9U : 33U;
    if (status == LXP_OK && selector_kind == 3U)
        checkpoint_length = 4U + checkpoint->checkpoint_payload.length + 4U +
            checkpoint->finality_proof.length;
    proof_length = 3U + selector_length + 32U + 96U +
        state_proof_length(&evidence->account_proof) +
        state_proof_length(&evidence->account_tree_proof) +
        state_proof_length(&evidence->universal_root_proof) + 4U +
        evidence->canonical_receipt.length +
        merkle_proof_length(&evidence->receipt_proof) +
        signed_header_length(&evidence->signed_header) + 1U +
        checkpoint_length;
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, evidence->account_leaf_value_length,
                                 _Alignof(uint64_t), &allocation);
    if (status == LXP_OK) {
        value = (uint8_t *)allocation;
        (void)memcpy(value, evidence->account_leaf_value,
                     evidence->account_leaf_value_length);
    }
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, proof_length,
                                 _Alignof(uint64_t), &allocation);
    if (status != LXP_OK) return status;
    proof = (uint8_t *)allocation;
    writer = (evidence_writer){proof, proof_length, 0U};
    status = writer_u16(&writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK) status = writer_u8(&writer, 2U);
    if (status == LXP_OK) status = writer_u8(&writer, selector_kind);
    if (status == LXP_OK && selector_kind == 2U)
        status = writer_u64(&writer, selector_batch);
    if (status == LXP_OK && selector_kind == 3U)
        status = writer_bytes(&writer, selector_checkpoint_id, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->account_id, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->account_root, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->universal_root, 32U);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->resulting_state_root, 32U);
    if (status == LXP_OK)
        status = write_state_proof(&writer, &evidence->account_proof);
    if (status == LXP_OK)
        status = write_state_proof(&writer, &evidence->account_tree_proof);
    if (status == LXP_OK)
        status = write_state_proof(&writer, &evidence->universal_root_proof);
    if (status == LXP_OK)
        status = writer_u32(&writer,
                            (uint32_t)evidence->canonical_receipt.length);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->canonical_receipt.bytes,
                              evidence->canonical_receipt.length);
    if (status == LXP_OK)
        status = write_merkle_proof(&writer, &evidence->receipt_proof);
    if (status == LXP_OK)
        status = write_signed_header(&writer, &evidence->signed_header);
    if (status == LXP_OK)
        status = writer_u8(&writer, selector_kind == 3U ? 1U : 0U);
    if (status == LXP_OK && selector_kind == 3U)
        status = writer_u32(&writer,
            (uint32_t)checkpoint->checkpoint_payload.length);
    if (status == LXP_OK && selector_kind == 3U)
        status = writer_bytes(&writer, checkpoint->checkpoint_payload.bytes,
                              checkpoint->checkpoint_payload.length);
    if (status == LXP_OK && selector_kind == 3U)
        status = writer_u32(&writer,
            (uint32_t)checkpoint->finality_proof.length);
    if (status == LXP_OK && selector_kind == 3U)
        status = writer_bytes(&writer, checkpoint->finality_proof.bytes,
                              checkpoint->finality_proof.length);
    if (status == LXP_OK && writer.cursor != writer.capacity)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        *canonical_value = (lxp_byte_span){value,
            evidence->account_leaf_value_length};
        *proof_material = (lxp_byte_span){proof, proof_length};
    }
    return status;
}

lxp_result lxp_daemon_activity_evidence_publish(
    lxp_daemon_evidence_store *store, lxp_byte_span canonical_activity,
    const lxp_merkle_proof *activity_proof,
    lxp_byte_span canonical_receipt,
    const lxp_merkle_proof *receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena, uint8_t record_digest[32])
{
    lxp_daemon_activity_evidence evidence;
    lxp_batch_header header;
    lxp_receipt receipt;
    uint8_t *payload;
    size_t payload_length;
    bool appended;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        activity_proof == NULL || receipt_proof == NULL ||
        authorization == NULL || header_signature == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&evidence, 0, sizeof(evidence));
    evidence.canonical_activity = canonical_activity;
    evidence.activity_proof = *activity_proof;
    evidence.canonical_receipt = canonical_receipt;
    evidence.receipt_proof = *receipt_proof;
    evidence.signed_header.authorization = *authorization;
    evidence.signed_header.canonical_header = canonical_header;
    (void)memcpy(evidence.signed_header.signature, header_signature, 64U);
    status = authorizations_equal(authorization, &store->authorization) ?
        LXP_OK : LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = verify_signed_header(&evidence.signed_header,
                                      store->network_id, arena, &header);
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity.bytes,
                                 canonical_activity.length,
                                 evidence.activity_id);
    if (status == LXP_OK)
        status = lxp_receipt_decode(canonical_receipt.bytes,
                                    canonical_receipt.length, true, &receipt);
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, evidence.receipt_digest);
    if (status == LXP_OK) {
        evidence.global_sequence = receipt.global_sequence;
        evidence.batch_number = header.batch_number;
        status = verify_activity_evidence(&evidence, store->network_id, arena);
    }
    payload_length = activity_payload_length(&evidence);
    payload = status == LXP_OK ? (uint8_t *)malloc(payload_length) : NULL;
    if (status == LXP_OK && payload == NULL) status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = encode_activity_payload(&evidence, payload, payload_length,
                                         &payload_length);
    if (status == LXP_OK)
        status = append_record(store, LXP_DAEMON_EVIDENCE_ACTIVITY,
            evidence.activity_id, (lxp_byte_span){payload, payload_length},
            (lxp_byte_span){NULL, 0U}, record_digest, &appended);
    if (payload != NULL) {
        lxp_secure_zero(payload, payload_length);
        free(payload);
    }
    return status;
}

lxp_result lxp_daemon_activity_evidence_lookup(
    const lxp_daemon_evidence_store *store, const uint8_t activity_id[32],
    lxp_arena *arena, lxp_daemon_activity_evidence *evidence)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    if (store == NULL || !store->initialized || store->log == NULL ||
        activity_id == NULL || arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        decoded_record record;
        uint8_t *body = NULL;
        status = read_log_record(store, offset, &header, &body, &record);
        if (status == LXP_OK &&
            record.kind == LXP_DAEMON_EVIDENCE_ACTIVITY &&
            lxp_ct_memcmp(record.key, activity_id, 32U) == 0) {
            status = decode_activity_payload(record.payload, arena, evidence);
            if (status == LXP_OK && !authorizations_equal(
                    &evidence->signed_header.authorization,
                    &store->authorization))
                status = LXP_ERR_AUTH_SCOPE;
            if (status == LXP_OK)
                status = verify_activity_evidence(evidence, store->network_id,
                                                  arena);
            free(body);
            return status;
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
        free(body);
    }
    return status == LXP_OK ? LXP_ERR_UNKNOWN_ACTIVITY : status;
}

static bool merkle_proofs_equal(const lxp_merkle_proof *left,
                                const lxp_merkle_proof *right)
{
    if (left == NULL || right == NULL ||
        left->leaf_index != right->leaf_index ||
        left->leaf_count != right->leaf_count ||
        left->depth != right->depth)
        return false;
    return lxp_ct_memcmp(left->siblings, right->siblings,
                         (size_t)left->depth * 32U) == 0;
}

lxp_result lxp_daemon_activity_evidence_recover_batch(
    lxp_daemon_evidence_store *store, const lxp_log *canonical_log,
    const lxp_daemon_receipt_authority_store *receipt_authority,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena)
{
    lxp_byte_span activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    lxp_byte_span receipts[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    lxp_byte_span events[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    lxp_receipt decoded[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    uint8_t activity_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES][32];
    uint8_t receipt_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES][32];
    lxp_daemon_signed_header_evidence signed_header;
    lxp_batch_header header;
    lxp_batch_roots roots;
    uint64_t offset = 0U;
    size_t count = 0U;
    size_t index;
    size_t mark;
    lxp_result status;
    if (store == NULL || !store->initialized || canonical_log == NULL ||
        canonical_log->descriptor < 0 || receipt_authority == NULL ||
        receipt_authority->log == NULL || authorization == NULL ||
        canonical_header.bytes == NULL || header_signature == NULL ||
        arena == NULL || canonical_log == store->log ||
        receipt_authority->log == store->log ||
        receipt_authority->log == canonical_log)
        return LXP_ERR_NON_CANONICAL;
    if (!authorizations_equal(authorization, &store->authorization) ||
        !authorizations_equal(&receipt_authority->authorization,
                              &store->authorization))
        return LXP_ERR_AUTH_SCOPE;
    (void)memset(&signed_header, 0, sizeof(signed_header));
    signed_header.authorization = *authorization;
    signed_header.canonical_header = canonical_header;
    (void)memcpy(signed_header.signature, header_signature, 64U);
    mark = lxp_arena_mark(arena);
    status = verify_signed_header(&signed_header, store->network_id,
                                  arena, &header);
    if (status == LXP_OK &&
        (header.first_sequence == 0U ||
         header.last_sequence < header.first_sequence ||
         header.last_sequence - header.first_sequence >=
             LXP_DAEMON_MAX_BATCH_ACTIVITIES))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        count = (size_t)(header.last_sequence - header.first_sequence + 1U);
    while (status == LXP_OK && offset < canonical_log->write_offset) {
        lxp_log_record_header record;
        uint8_t *body = NULL;
        status = lxp_log_read(canonical_log, offset, &record, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (status == LXP_ERR_LENGTH_LIMIT) status = LXP_OK;
        if (record.global_sequence >= header.first_sequence &&
            record.global_sequence <= header.last_sequence &&
            (record.record_kind == (uint8_t)LXP_LOG_ACTIVITY ||
             record.record_kind == (uint8_t)LXP_LOG_RECEIPT)) {
            size_t position = (size_t)(record.global_sequence -
                                       header.first_sequence);
            if (record.body_length == 0U ||
                record.body_length > LXP_MAX_ACTIVITY_BYTES) {
                status = LXP_ERR_LOG_CORRUPT;
            } else {
                body = (uint8_t *)malloc(record.body_length);
                if (body == NULL)
                    status = LXP_ERR_IO;
                else
                    status = lxp_log_read(canonical_log, offset, &record,
                                          body, record.body_length);
            }
            if (status == LXP_OK &&
                record.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
                if (activities[position].bytes != NULL)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    activities[position] =
                        (lxp_byte_span){body, record.body_length};
                    body = NULL;
                }
            } else if (status == LXP_OK) {
                if (receipts[position].bytes != NULL)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    receipts[position] =
                        (lxp_byte_span){body, record.body_length};
                    body = NULL;
                }
            }
            free(body);
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)record.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + record.body_length;
        }
    }
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        lxp_activity activity;
        uint8_t activity_id[32];
        if (activities[index].bytes == NULL || receipts[index].bytes == NULL) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        status = lxp_activity_decode(activities[index].bytes,
                                     activities[index].length, &activity);
        if (status == LXP_OK)
            status = lxp_activity_check_envelope(&activity,
                                                 store->network_id);
        if (status == LXP_OK)
            status = lxp_activity_verify_payload_hash(&activity);
        if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
        if (status == LXP_OK)
            status = lxp_activity_id(activities[index].bytes,
                                     activities[index].length, activity_id);
        if (status == LXP_OK)
            status = lxp_receipt_decode(receipts[index].bytes,
                                        receipts[index].length, true,
                                        &decoded[index]);
        if (status == LXP_OK)
            status = lxp_receipt_verify(&decoded[index],
                                        authorization->public_key, arena);
        if (status == LXP_OK &&
            (decoded[index].global_sequence != header.first_sequence + index ||
             decoded[index].protocol_version != header.protocol_version ||
             decoded[index].timestamp != header.timestamp_ms ||
             lxp_ct_memcmp(decoded[index].activity_id,
                           activity_id, 32U) != 0 ||
             (index != 0U &&
              lxp_ct_memcmp(decoded[index - 1U].resulting_state_root,
                            decoded[index].previous_state_root, 32U) != 0)))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_merkle_leaf_hash(activities[index].bytes,
                                          activities[index].length,
                                          activity_hashes[index]);
        if (status == LXP_OK)
            status = lxp_merkle_leaf_hash(receipts[index].bytes,
                                          receipts[index].length,
                                          receipt_hashes[index]);
        if (status == LXP_OK)
            status = lxp_programs_project_receipt_events(
                &decoded[index], arena, &events[index]);
    }
    if (status == LXP_OK &&
        (lxp_ct_memcmp(decoded[0].previous_state_root,
                       header.previous_state_root, 32U) != 0 ||
         lxp_ct_memcmp(decoded[count - 1U].resulting_state_root,
                       header.resulting_state_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){activities, count, receipts, count,
                                     events, count, NULL, 0U, NULL, 0U},
            arena, &roots);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(roots.activity_merkle_root,
                       header.activity_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.receipt_merkle_root,
                       header.receipt_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.event_merkle_root,
                       header.event_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.data_availability_root,
                       header.data_availability_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.oracle_root, header.oracle_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        lxp_daemon_receipt_evidence authority_evidence;
        lxp_merkle_proof activity_proof;
        lxp_merkle_proof receipt_proof;
        uint8_t digest[32];
        uint8_t proof_root[32];
        size_t item_mark = lxp_arena_mark(arena);
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])activity_hashes, count, index,
            arena, &activity_proof, proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root,
                          header.activity_merkle_root, 32U) != 0)
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_merkle_proof_generate(
                (const uint8_t (*)[32])receipt_hashes, count, index,
                arena, &receipt_proof, proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root,
                          header.receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_receipt_digest(&decoded[index], arena, digest);
        if (status == LXP_OK)
            status = lxp_daemon_receipt_authority_lookup(
                receipt_authority, digest, arena, &authority_evidence);
        if (status == LXP_OK &&
            (authority_evidence.global_sequence !=
                 decoded[index].global_sequence ||
             authority_evidence.canonical_receipt.length !=
                 receipts[index].length ||
             lxp_ct_memcmp(authority_evidence.canonical_receipt.bytes,
                           receipts[index].bytes,
                           receipts[index].length) != 0 ||
             authority_evidence.canonical_header.length !=
                 canonical_header.length ||
             lxp_ct_memcmp(authority_evidence.canonical_header.bytes,
                           canonical_header.bytes,
                           canonical_header.length) != 0 ||
             lxp_ct_memcmp(authority_evidence.header_signature,
                           header_signature, 64U) != 0 ||
             !merkle_proofs_equal(&authority_evidence.receipt_proof,
                                  &receipt_proof)))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_daemon_activity_evidence_publish(
                store, activities[index], &activity_proof, receipts[index],
                &receipt_proof, authorization, canonical_header,
                header_signature, arena, NULL);
        (void)lxp_arena_reset(arena, item_mark);
    }
    for (index = 0U; index < count; ++index) {
        free((void *)activities[index].bytes);
        free((void *)receipts[index].bytes);
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_daemon_activity_evidence_wire_encode(
    const lxp_daemon_activity_evidence *evidence, uint32_t network_id,
    uint8_t response_kind, lxp_arena *arena,
    lxp_byte_span *canonical_value, lxp_byte_span *proof_material)
{
    const lxp_byte_span *value;
    const lxp_merkle_proof *inclusion;
    evidence_writer writer;
    uint8_t *value_copy = NULL;
    uint8_t *proof = NULL;
    size_t proof_length;
    void *allocation;
    lxp_result status;
    if (evidence == NULL || network_id == 0U || arena == NULL ||
        canonical_value == NULL || proof_material == NULL ||
        (response_kind != 1U && response_kind != 3U))
        return LXP_ERR_NON_CANONICAL;
    status = verify_activity_evidence(evidence, network_id, arena);
    if (status != LXP_OK) return status;
    value = response_kind == 1U ? &evidence->canonical_activity :
                                  &evidence->canonical_receipt;
    inclusion = response_kind == 1U ? &evidence->activity_proof :
                                      &evidence->receipt_proof;
    proof_length = 3U + 32U + merkle_proof_length(inclusion) +
        signed_header_length(&evidence->signed_header);
    status = lxp_arena_alloc(arena, value->length, _Alignof(uint64_t),
                             &allocation);
    if (status == LXP_OK) {
        value_copy = (uint8_t *)allocation;
        (void)memcpy(value_copy, value->bytes, value->length);
    }
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, proof_length,
                                 _Alignof(uint64_t), &allocation);
    if (status != LXP_OK) return status;
    proof = (uint8_t *)allocation;
    writer = (evidence_writer){proof, proof_length, 0U};
    status = writer_u16(&writer, EVIDENCE_WIRE_VERSION);
    if (status == LXP_OK) status = writer_u8(&writer, response_kind);
    if (status == LXP_OK)
        status = writer_bytes(&writer, evidence->activity_id, 32U);
    if (status == LXP_OK) status = write_merkle_proof(&writer, inclusion);
    if (status == LXP_OK)
        status = write_signed_header(&writer, &evidence->signed_header);
    if (status == LXP_OK && writer.cursor != writer.capacity)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        *canonical_value = (lxp_byte_span){value_copy, value->length};
        *proof_material = (lxp_byte_span){proof, proof_length};
    }
    return status;
}

lxp_result lxp_daemon_finality_evidence_register(
    lxp_daemon_evidence_store *store, lxp_byte_span checkpoint_payload,
    lxp_byte_span finality_proof, lxp_arena *arena,
    lxp_daemon_finality_evidence *evidence)
{
    decoded_finality decoded;
    lxp_checkpoint_registry_state candidate;
    lxp_checkpoint_registration registration;
    uint8_t checkpoint_id[32];
    uint8_t digest[32];
    bool present;
    bool exact;
    bool appended;
    lxp_result status;
    if (store == NULL || !store->initialized || store->log == NULL ||
        checkpoint_payload.bytes == NULL ||
        finality_proof.bytes == NULL || arena == NULL || evidence == NULL ||
        checkpoint_payload.length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
        finality_proof.length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
        checkpoint_payload.length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES -
                                        finality_proof.length)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded, 0, sizeof(decoded));
    status = decode_checkpoint_payload(checkpoint_payload, &decoded);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(
            &decoded.certificate.checkpoint, arena, checkpoint_id);
    if (status == LXP_OK)
        status = find_record(store, LXP_DAEMON_EVIDENCE_FINALITY,
            checkpoint_id, checkpoint_payload, finality_proof, digest,
            &present, &exact);
    if (status == LXP_OK && present && !exact) status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK && !present)
        status = verify_finality_bundle(
            store, checkpoint_payload, finality_proof, arena, &decoded,
            &candidate, &registration, checkpoint_id);
    if (status == LXP_OK && !present)
        status = append_record(store, LXP_DAEMON_EVIDENCE_FINALITY,
            checkpoint_id, checkpoint_payload, finality_proof, digest,
            &appended);
    if (status == LXP_OK && !present) {
        store->registry = candidate;
        store->latest_finalized_batch = registration.batch_number;
        store->latest_bonded_set_version = decoded.bonded_set.version;
        (void)memcpy(store->latest_checkpoint_id, checkpoint_id, 32U);
    }
    if (status == LXP_OK && present)
        return lxp_daemon_finality_evidence_lookup(
            store, checkpoint_id, 0U, arena, evidence);
    if (status == LXP_OK) {
        (void)memset(evidence, 0, sizeof(*evidence));
        (void)memcpy(evidence->checkpoint_id, checkpoint_id, 32U);
        (void)memcpy(evidence->resulting_state_root,
            registration.resulting_state_root, 32U);
        (void)memcpy(evidence->record_digest, digest, 32U);
        evidence->batch_number = registration.batch_number;
        evidence->bonded_set_version = decoded.bonded_set.version;
        evidence->resulting_registration_count =
            store->registry.registration_count;
        evidence->checkpoint_payload = checkpoint_payload;
        evidence->finality_proof = finality_proof;
    }
    return status;
}

lxp_result lxp_daemon_finality_evidence_lookup(
    const lxp_daemon_evidence_store *store,
    const uint8_t checkpoint_id[32], uint64_t batch_number,
    lxp_arena *arena, lxp_daemon_finality_evidence *evidence)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    if (store == NULL || !store->initialized || store->log == NULL ||
        checkpoint_id == NULL || arena == NULL || evidence == NULL ||
        (lxp_ct_is_zero(checkpoint_id, 32U) == (batch_number == 0U)))
        return LXP_ERR_NON_CANONICAL;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        decoded_record record;
        uint8_t *body = NULL;
        status = read_log_record(store, offset, &header, &body, &record);
        if (status == LXP_OK && record.kind == LXP_DAEMON_EVIDENCE_FINALITY) {
            decoded_finality decoded;
            lxp_checkpoint_registry_state prior = store->registry;
            lxp_checkpoint_registry_state candidate;
            lxp_checkpoint_registration registration;
            uint8_t derived[32];
            (void)memset(&decoded, 0, sizeof(decoded));
            status = decode_checkpoint_payload(record.payload, &decoded);
            if (status == LXP_OK)
                status = decode_finality_proof(record.proof, &decoded);
            if (status == LXP_OK)
                status = lxp_checkpoint_certificate_hash(
                    &decoded.certificate.checkpoint, arena, derived);
            if (status == LXP_OK &&
                ((batch_number != 0U &&
                  decoded.certificate.checkpoint.header.batch_number ==
                      batch_number) ||
                 (!lxp_ct_is_zero(checkpoint_id, 32U) &&
                  lxp_ct_memcmp(checkpoint_id, derived, 32U) == 0))) {
                void *payload_copy;
                void *proof_copy;
                (void)candidate;
                (void)registration;
                (void)prior;
                status = lxp_arena_alloc(arena, record.payload.length,
                                         _Alignof(uint64_t), &payload_copy);
                if (status == LXP_OK)
                    status = lxp_arena_alloc(arena, record.proof.length,
                                             _Alignof(uint64_t), &proof_copy);
                if (status == LXP_OK) {
                    (void)memcpy(payload_copy, record.payload.bytes,
                                 record.payload.length);
                    (void)memcpy(proof_copy, record.proof.bytes,
                                 record.proof.length);
                    (void)memset(evidence, 0, sizeof(*evidence));
                    (void)memcpy(evidence->checkpoint_id, derived, 32U);
                    (void)memcpy(evidence->resulting_state_root,
                        decoded.certificate.checkpoint.header.resulting_state_root,
                        32U);
                    (void)memcpy(evidence->record_digest, record.digest, 32U);
                    evidence->batch_number =
                        decoded.certificate.checkpoint.header.batch_number;
                    evidence->bonded_set_version = decoded.bonded_set.version;
                    evidence->resulting_registration_count =
                        decoded.expected_registration_count + 1U;
                    evidence->checkpoint_payload =
                        (lxp_byte_span){payload_copy, record.payload.length};
                    evidence->finality_proof =
                        (lxp_byte_span){proof_copy, record.proof.length};
                }
                free(body);
                return status;
            }
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)header.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
        free(body);
    }
    return status == LXP_OK ? LXP_ERR_UNKNOWN_FIELD : status;
}
