#include "layerx/lxp_bridge_light.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

#define REFUSED LXP_ERR_DEPOSIT_PROOF_NOT_FINAL

enum {
    VALIDATOR_BYTES = 40,
    FLAG_ABSENT = 1,
    FLAG_COMMIT = 2,
    FLAG_NIL = 3,
    HEADER_LEAVES = 14,
    MAX_TOTAL_POWER_SHIFT = 3
};

typedef struct reader {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
    bool failed;
} reader;

typedef struct span {
    const uint8_t *bytes;
    size_t length;
} span;

typedef struct header_fields {
    uint64_t version_block;
    uint64_t version_app;
    const uint8_t *chain_id;
    size_t chain_id_length;
    uint64_t height;
    int64_t time_seconds;
    uint32_t time_nanos;
    span last_block_hash;
    uint32_t last_parts_total;
    span last_parts_hash;
    span hashes[8];
    span proposer;
} header_fields;

typedef struct validator_set {
    const uint8_t *entries;
    size_t count;
} validator_set;

typedef lxp_result (*leaf_function)(const void *context, size_t index, uint8_t out[32]);

static const uint8_t *take(reader *input, size_t count)
{
    const uint8_t *start;
    if (input->failed || count > input->length - input->offset) {
        input->failed = true;
        return NULL;
    }
    start = input->bytes + input->offset;
    input->offset += count;
    return start;
}

static uint64_t take_uint(reader *input, size_t width)
{
    const uint8_t *bytes = take(input, width);
    uint64_t value = 0U;
    if (bytes == NULL) return 0U;
    for (size_t index = 0U; index < width; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static span take_span(reader *input, size_t width)
{
    span value = {NULL, 0U};
    size_t length = (size_t)take_uint(input, width);
    const uint8_t *bytes = take(input, length);
    if (bytes != NULL) {
        value.bytes = bytes;
        value.length = length;
    }
    return value;
}

static span take_hash(reader *input)
{
    span value = take_span(input, 1U);
    if (!input->failed && value.length != 0U && value.length != 32U)
        input->failed = true;
    return value;
}

static size_t put_varint(uint8_t *out, uint64_t value)
{
    size_t length = 0U;
    while (value >= 0x80U) {
        out[length++] = (uint8_t)(value | 0x80U);
        value >>= 7U;
    }
    out[length++] = (uint8_t)value;
    return length;
}

static size_t put_bytes(uint8_t *out, uint8_t tag, const uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    out[offset++] = tag;
    offset += put_varint(out + offset, length);
    if (length != 0U) (void)memcpy(out + offset, bytes, length);
    return offset + length;
}

static size_t put_little(uint8_t *out, uint8_t tag, uint64_t value)
{
    out[0] = tag;
    for (size_t index = 0U; index < 8U; ++index)
        out[1U + index] = (uint8_t)(value >> (8U * index));
    return 9U;
}

static size_t put_timestamp(uint8_t *out, int64_t seconds, uint32_t nanos)
{
    size_t length = 0U;
    if (seconds != 0) {
        out[length++] = 0x08U;
        length += put_varint(out + length, (uint64_t)seconds);
    }
    if (nanos != 0U) {
        out[length++] = 0x10U;
        length += put_varint(out + length, nanos);
    }
    return length;
}

static size_t put_parts(uint8_t *out, uint32_t total, const uint8_t *hash, size_t hash_length)
{
    size_t length = 0U;
    if (total != 0U) {
        out[length++] = 0x08U;
        length += put_varint(out + length, total);
    }
    if (hash_length != 0U) length += put_bytes(out + length, 0x12U, hash, hash_length);
    return length;
}

static size_t put_block_id(uint8_t *out, const uint8_t *hash, size_t hash_length,
                           uint32_t total, const uint8_t *parts_hash, size_t parts_hash_length)
{
    uint8_t parts[48];
    size_t parts_length = put_parts(parts, total, parts_hash, parts_hash_length);
    size_t length = 0U;
    if (hash_length != 0U) length += put_bytes(out + length, 0x0aU, hash, hash_length);
    length += put_bytes(out + length, 0x12U, parts, parts_length);
    return length;
}

static lxp_result leaf_hash(const uint8_t *bytes, size_t length, uint8_t out[32])
{
    static const uint8_t prefix = 0U;
    lxp_hash_context context;
    lxp_result status;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, &prefix, 1U);
    if (status == LXP_OK && length != 0U) status = lxp_hash_update(&context, bytes, length);
    if (status == LXP_OK) status = lxp_hash_final(&context, out);
    return status;
}

static lxp_result merkle(leaf_function leaf, const void *context, size_t start,
                         size_t count, uint8_t out[32])
{
    uint8_t node[65] = {0};
    size_t split = 1U;
    lxp_result status;
    if (count == 0U) return lxp_hash_sha256(node, 0U, out);
    if (count == 1U) return leaf(context, start, out);
    while (split * 2U < count) split *= 2U;
    node[0] = 1U;
    status = merkle(leaf, context, start, split, node + 1U);
    if (status == LXP_OK)
        status = merkle(leaf, context, start + split, count - split, node + 33U);
    if (status == LXP_OK) status = lxp_hash_sha256(node, sizeof(node), out);
    return status;
}

static lxp_result header_leaf(const void *context, size_t index, uint8_t out[32])
{
    const header_fields *header = context;
    uint8_t bytes[128];
    size_t length = 0U;
    if (index == 0U) {
        if (header->version_block != 0U) {
            bytes[length++] = 0x08U;
            length += put_varint(bytes + length, header->version_block);
        }
        if (header->version_app != 0U) {
            bytes[length++] = 0x10U;
            length += put_varint(bytes + length, header->version_app);
        }
    } else if (index == 1U) {
        length = put_bytes(bytes, 0x0aU, header->chain_id, header->chain_id_length);
    } else if (index == 2U) {
        bytes[length++] = 0x08U;
        length += put_varint(bytes + length, header->height);
    } else if (index == 3U) {
        length = put_timestamp(bytes, header->time_seconds, header->time_nanos);
    } else if (index == 4U) {
        length = put_block_id(bytes, header->last_block_hash.bytes, header->last_block_hash.length,
                              header->last_parts_total, header->last_parts_hash.bytes,
                              header->last_parts_hash.length);
    } else {
        const span *field = index == 13U ? &header->proposer : &header->hashes[index - 5U];
        if (field->length != 0U) length = put_bytes(bytes, 0x0aU, field->bytes, field->length);
    }
    return leaf_hash(bytes, length, out);
}

static uint64_t validator_power(const validator_set *set, size_t index)
{
    const uint8_t *bytes = set->entries + index * VALIDATOR_BYTES + 32U;
    uint64_t value = 0U;
    for (size_t offset = 0U; offset < 8U; ++offset)
        value = (value << 8U) | bytes[offset];
    return value;
}

static lxp_result validator_leaf(const void *context, size_t index, uint8_t out[32])
{
    const validator_set *set = context;
    uint8_t bytes[48] = {0x0aU, 0x22U, 0x0aU, 0x20U};
    size_t length = 36U;
    (void)memcpy(bytes + 4U, set->entries + index * VALIDATOR_BYTES, 32U);
    bytes[length++] = 0x10U;
    length += put_varint(bytes + length, validator_power(set, index));
    return leaf_hash(bytes, length, out);
}

static lxp_result take_validators(reader *input, validator_set *set, bool required,
                                  uint64_t *total, uint8_t hash[32])
{
    size_t count = (size_t)take_uint(input, 2U);
    const uint8_t *entries = take(input, count * VALIDATOR_BYTES);
    if (input->failed || count > LXP_BRIDGE_LIGHT_MAX_VALIDATORS || (required && count == 0U))
        return REFUSED;
    set->entries = entries;
    set->count = count;
    *total = 0U;
    for (size_t index = 0U; index < count; ++index) {
        uint64_t power = validator_power(set, index);
        if (!lxp_ed25519_pubkey_is_canonical(entries + index * VALIDATOR_BYTES) ||
            power == 0U || power > ((uint64_t)INT64_MAX >> MAX_TOTAL_POWER_SHIFT) - *total)
            return REFUSED;
        *total += power;
    }
    return count == 0U ? LXP_OK : merkle(validator_leaf, set, 0U, count, hash);
}

static size_t vote_sign_bytes(uint8_t *out, const header_fields *header, const uint8_t header_hash[32],
                              uint32_t round, uint32_t parts_total, const uint8_t *parts_hash,
                              int64_t seconds, uint32_t nanos)
{
    uint8_t body[224];
    uint8_t block_id[96];
    uint8_t timestamp[24];
    size_t length = 0U;
    size_t prefix;
    size_t block_id_length = put_block_id(block_id, header_hash, 32U, parts_total, parts_hash, 32U);
    size_t timestamp_length = put_timestamp(timestamp, seconds, nanos);
    body[length++] = 0x08U;
    body[length++] = 0x02U;
    length += put_little(body + length, 0x11U, header->height);
    if (round != 0U) length += put_little(body + length, 0x19U, round);
    length += put_bytes(body + length, 0x22U, block_id, block_id_length);
    length += put_bytes(body + length, 0x2aU, timestamp, timestamp_length);
    length += put_bytes(body + length, 0x32U, header->chain_id, header->chain_id_length);
    prefix = put_varint(out, length);
    (void)memcpy(out + prefix, body, length);
    return prefix + length;
}

static bool take_varint(reader *input, uint64_t *value)
{
    uint64_t result = 0U;
    for (unsigned shift = 0U; shift < 70U; shift += 7U) {
        const uint8_t *byte = take(input, 1U);
        if (byte == NULL || (shift == 63U && *byte > 1U)) break;
        result |= (uint64_t)(*byte & 0x7fU) << shift;
        if ((*byte & 0x80U) == 0U) {
            if (*byte == 0U && shift != 0U) break;
            *value = result;
            return true;
        }
    }
    input->failed = true;
    return false;
}

static lxp_result existence_root(const uint8_t *leaf_prefix, size_t leaf_prefix_length,
                                 const uint8_t *key, size_t key_length,
                                 const uint8_t *value, size_t value_length, uint8_t out[32])
{
    uint8_t digest[32];
    uint8_t framing[10];
    lxp_hash_context context;
    lxp_result status = lxp_hash_sha256(value, value_length, digest);
    if (status != LXP_OK) return status;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, leaf_prefix, leaf_prefix_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, framing, put_varint(framing, key_length));
    if (status == LXP_OK) status = lxp_hash_update(&context, key, key_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, framing, put_varint(framing, sizeof(digest)));
    if (status == LXP_OK) status = lxp_hash_update(&context, digest, sizeof(digest));
    if (status == LXP_OK) status = lxp_hash_final(&context, out);
    return status;
}

static lxp_result inner_step(span prefix, span suffix, uint8_t node[32])
{
    lxp_hash_context context;
    lxp_result status;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, prefix.bytes, prefix.length);
    if (status == LXP_OK) status = lxp_hash_update(&context, node, 32U);
    if (status == LXP_OK && suffix.length != 0U)
        status = lxp_hash_update(&context, suffix.bytes, suffix.length);
    if (status == LXP_OK) status = lxp_hash_final(&context, node);
    return status;
}

static bool iavl_node_prefix(span prefix, bool leaf, size_t *remaining)
{
    reader input = {prefix.bytes, prefix.length, 0U, false};
    uint64_t height;
    uint64_t size;
    uint64_t version;
    if (!take_varint(&input, &height) || !take_varint(&input, &size) ||
        !take_varint(&input, &version) || (height == 0U) != leaf)
        return false;
    *remaining = input.length - input.offset;
    return true;
}

static lxp_result iavl_root(reader *input, lxp_bridge_light_result *result, uint8_t root[32])
{
    span key = take_span(input, 2U);
    span value = take_span(input, 2U);
    span leaf = take_span(input, 1U);
    size_t steps = (size_t)take_uint(input, 1U);
    size_t remaining;
    lxp_result status;
    if (input->failed || key.length == 0U || value.length == 0U ||
        value.length > LXP_BRIDGE_LIGHT_MAX_RECORD || steps > LXP_BRIDGE_LIGHT_MAX_IAVL_STEPS ||
        !iavl_node_prefix(leaf, true, &remaining) || remaining != 0U)
        return REFUSED;
    status = existence_root(leaf.bytes, leaf.length, key.bytes, key.length,
                            value.bytes, value.length, root);
    for (size_t index = 0U; status == LXP_OK && index < steps; ++index) {
        span prefix = take_span(input, 1U);
        span suffix = take_span(input, 1U);
        if (input->failed || !iavl_node_prefix(prefix, false, &remaining)) return REFUSED;
        if (remaining == 1U) {
            if (prefix.bytes[prefix.length - 1U] != 0x20U || suffix.length != 33U ||
                suffix.bytes[0] != 0x20U)
                return REFUSED;
        } else if (remaining != 34U || prefix.bytes[prefix.length - 34U] != 0x20U ||
                   prefix.bytes[prefix.length - 1U] != 0x20U || suffix.length != 0U) {
            return REFUSED;
        }
        status = inner_step(prefix, suffix, root);
    }
    result->key = key.bytes;
    result->key_length = key.length;
    result->value = value.bytes;
    result->value_length = value.length;
    return status;
}

static lxp_result store_root(reader *input, const uint8_t *store, size_t store_length,
                             const uint8_t commitment[32], uint8_t root[32])
{
    span name = take_span(input, 1U);
    span leaf = take_span(input, 1U);
    size_t steps = (size_t)take_uint(input, 1U);
    lxp_result status;
    if (input->failed || name.length != store_length ||
        memcmp(name.bytes, store, store_length) != 0 ||
        leaf.length != 1U || leaf.bytes[0] != 0U || steps > LXP_BRIDGE_LIGHT_MAX_STORE_STEPS)
        return REFUSED;
    status = existence_root(leaf.bytes, leaf.length, name.bytes, name.length, commitment, 32U, root);
    for (size_t index = 0U; status == LXP_OK && index < steps; ++index) {
        span prefix = take_span(input, 1U);
        span suffix = take_span(input, 1U);
        if (input->failed || prefix.length == 0U || prefix.bytes[0] != 1U ||
            !((prefix.length == 1U && suffix.length == 32U) ||
              (prefix.length == 33U && suffix.length == 0U)))
            return REFUSED;
        status = inner_step(prefix, suffix, root);
    }
    return status;
}

static bool later(int64_t seconds, uint32_t nanos, const lxp_bridge_light_trust *trusted)
{
    return seconds > trusted->time_seconds ||
        (seconds == trusted->time_seconds && nanos > trusted->time_nanos);
}

lxp_result lxp_bridge_light_verify(const uint8_t *chain_id, size_t chain_id_length,
                                   const uint8_t *store, size_t store_length,
                                   const lxp_bridge_light_trust *trusted,
                                   const uint8_t *bundle, size_t bundle_length,
                                   lxp_bridge_light_result *result)
{
    reader input = {bundle, bundle_length, 0U, false};
    header_fields header;
    validator_set validators;
    validator_set previous;
    uint8_t signed_by[LXP_BRIDGE_LIGHT_MAX_VALIDATORS] = {0};
    uint8_t message[256];
    uint8_t hash[32];
    uint8_t root[32];
    uint8_t commitment[32];
    const uint8_t *magic;
    const uint8_t *parts_hash;
    uint64_t round;
    uint64_t parts_total;
    uint64_t total;
    uint64_t previous_total;
    uint64_t tallied = 0U;
    lxp_result status;
    if (chain_id == NULL || chain_id_length == 0U || chain_id_length > LXP_BRIDGE_LIGHT_MAX_CHAIN_ID ||
        store == NULL || store_length == 0U || store_length > UINT8_MAX || trusted == NULL ||
        bundle == NULL || result == NULL || trusted->height == 0U ||
        trusted->height >= (uint64_t)INT64_MAX ||
        lxp_ct_is_zero(trusted->next_validators_hash, 32U))
        return REFUSED;
    (void)memset(result, 0, sizeof(*result));
    (void)memset(&header, 0, sizeof(header));
    magic = take(&input, 5U);
    if (magic == NULL || memcmp(magic, "LXLB1", 5U) != 0) return REFUSED;
    header.chain_id = chain_id;
    header.chain_id_length = chain_id_length;
    header.version_block = take_uint(&input, 8U);
    header.version_app = take_uint(&input, 8U);
    header.height = take_uint(&input, 8U);
    header.time_seconds = (int64_t)take_uint(&input, 8U);
    header.time_nanos = (uint32_t)take_uint(&input, 4U);
    header.last_block_hash = take_hash(&input);
    header.last_parts_total = (uint32_t)take_uint(&input, 4U);
    header.last_parts_hash = take_hash(&input);
    for (size_t index = 0U; index < 8U; ++index) header.hashes[index] = take_hash(&input);
    header.proposer = take_span(&input, 1U);
    if (input.failed || header.height == 0U || header.height >= (uint64_t)INT64_MAX ||
        header.time_seconds <= 0 || header.time_nanos >= 1000000000U ||
        header.hashes[2].length != 32U || header.hashes[3].length != 32U ||
        header.hashes[5].length != 32U || header.proposer.length != 20U)
        return REFUSED;
    status = merkle(header_leaf, &header, 0U, HEADER_LEAVES, hash);
    if (status != LXP_OK) return status;
    round = take_uint(&input, 4U);
    parts_total = take_uint(&input, 4U);
    parts_hash = take(&input, 32U);
    if (input.failed || round > (uint64_t)INT32_MAX) return REFUSED;
    status = take_validators(&input, &validators, true, &total, root);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(root, header.hashes[2].bytes, 32U) != 0) return REFUSED;
    for (size_t index = 0U; index < validators.count; ++index) {
        uint64_t flag = take_uint(&input, 1U);
        int64_t seconds;
        uint32_t nanos;
        const uint8_t *signature;
        size_t length;
        if (input.failed || flag < FLAG_ABSENT || flag > FLAG_NIL) return REFUSED;
        if (flag == FLAG_ABSENT) continue;
        seconds = (int64_t)take_uint(&input, 8U);
        nanos = (uint32_t)take_uint(&input, 4U);
        signature = take(&input, 64U);
        if (input.failed || nanos >= 1000000000U) return REFUSED;
        if (flag != FLAG_COMMIT) continue;
        length = vote_sign_bytes(message, &header, hash, (uint32_t)round, (uint32_t)parts_total,
                                 parts_hash, seconds, nanos);
        if (lxp_ed25519_verify_raw(validators.entries + index * VALIDATOR_BYTES, signature,
                                   message, length) != LXP_OK)
            return REFUSED;
        signed_by[index] = 1U;
        tallied += validator_power(&validators, index);
    }
    if (tallied * 3U <= total * 2U) return REFUSED;
    status = take_validators(&input, &previous, false, &previous_total, root);
    if (status != LXP_OK) return status;
    if (header.height < trusted->height) return REFUSED;
    if (header.height == trusted->height) {
        if (previous.count != 0U || lxp_ct_is_zero(trusted->header_hash, 32U) ||
            lxp_ct_memcmp(hash, trusted->header_hash, 32U) != 0)
            return REFUSED;
    } else {
        bool same = lxp_ct_memcmp(header.hashes[2].bytes, trusted->next_validators_hash, 32U) == 0;
        if (!lxp_ct_is_zero(trusted->header_hash, 32U) &&
            !later(header.time_seconds, header.time_nanos, trusted))
            return REFUSED;
        if (same || header.height == trusted->height + 1U) {
            if (!same || previous.count != 0U) return REFUSED;
        } else {
            uint64_t overlap = 0U;
            if (previous.count == 0U ||
                lxp_ct_memcmp(root, trusted->next_validators_hash, 32U) != 0)
                return REFUSED;
            for (size_t known = 0U; known < previous.count; ++known)
                for (size_t index = 0U; index < validators.count; ++index)
                    if (signed_by[index] != 0U &&
                        memcmp(previous.entries + known * VALIDATOR_BYTES,
                               validators.entries + index * VALIDATOR_BYTES, 32U) == 0) {
                        overlap += validator_power(&previous, known);
                        break;
                    }
            if (overlap * 3U <= previous_total) return REFUSED;
        }
    }
    status = iavl_root(&input, result, commitment);
    if (status == LXP_OK) status = store_root(&input, store, store_length, commitment, root);
    if (status != LXP_OK) return status;
    if (input.failed || input.offset != input.length ||
        lxp_ct_memcmp(root, header.hashes[5].bytes, 32U) != 0)
        return REFUSED;
    result->height = header.height;
    (void)memcpy(result->header_hash, hash, 32U);
    (void)memcpy(result->app_hash, header.hashes[5].bytes, 32U);
    (void)memcpy(result->validators_hash, header.hashes[2].bytes, 32U);
    if (header.height == trusted->height) {
        result->advanced = *trusted;
    } else {
        result->advanced.height = header.height;
        (void)memcpy(result->advanced.header_hash, hash, 32U);
        (void)memcpy(result->advanced.next_validators_hash, header.hashes[3].bytes, 32U);
        result->advanced.time_seconds = header.time_seconds;
        result->advanced.time_nanos = header.time_nanos;
    }
    return LXP_OK;
}

static int hex_digit(uint8_t value, bool lower_only)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (!lower_only && value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

static bool hex_decode(const uint8_t *text, size_t length, bool lower_only, uint8_t *out)
{
    for (size_t index = 0U; index < length; ++index) {
        int high = hex_digit(text[2U * index], lower_only);
        int low = hex_digit(text[2U * index + 1U], lower_only);
        if (high < 0 || low < 0) return false;
        out[index] = (uint8_t)((high << 4) | low);
    }
    return true;
}

static bool decimal_u128(span text, uint8_t out[16])
{
    uint32_t limbs[4] = {0U, 0U, 0U, 0U};
    if (text.length == 0U || text.length > 39U || (text.length > 1U && text.bytes[0] == '0'))
        return false;
    for (size_t index = 0U; index < text.length; ++index) {
        uint64_t carry;
        if (text.bytes[index] < '0' || text.bytes[index] > '9') return false;
        carry = (uint64_t)(text.bytes[index] - '0');
        for (size_t limb = 4U; limb-- > 0U;) {
            uint64_t product = (uint64_t)limbs[limb] * 10U + carry;
            limbs[limb] = (uint32_t)product;
            carry = product >> 32U;
        }
        if (carry != 0U) return false;
    }
    for (size_t limb = 0U; limb < 4U; ++limb) {
        out[4U * limb] = (uint8_t)(limbs[limb] >> 24U);
        out[4U * limb + 1U] = (uint8_t)(limbs[limb] >> 16U);
        out[4U * limb + 2U] = (uint8_t)(limbs[limb] >> 8U);
        out[4U * limb + 3U] = (uint8_t)limbs[limb];
    }
    return true;
}

lxp_result lxp_bridge_light_deposit_decode(const uint8_t *value, size_t length,
                                           lxp_bridge_light_deposit *deposit)
{
    reader input = {value, length, 0U, false};
    span text[10];
    uint64_t number[10] = {0U};
    bool present[10] = {false};
    uint64_t last = 0U;
    if (value == NULL || deposit == NULL || length == 0U || length > LXP_BRIDGE_LIGHT_MAX_RECORD)
        return REFUSED;
    (void)memset(text, 0, sizeof(text));
    (void)memset(deposit, 0, sizeof(*deposit));
    while (input.offset < input.length) {
        uint64_t tag;
        uint64_t field;
        if (!take_varint(&input, &tag)) return REFUSED;
        field = tag >> 3U;
        if (field <= last || field > 9U) return REFUSED;
        last = field;
        present[field] = true;
        if (field == 2U || field == 8U || field == 9U) {
            if ((tag & 7U) != 0U || !take_varint(&input, &number[field]) || number[field] == 0U)
                return REFUSED;
        } else {
            uint64_t size;
            if ((tag & 7U) != 2U || !take_varint(&input, &size) || size == 0U ||
                size > input.length - input.offset)
                return REFUSED;
            text[field].bytes = take(&input, (size_t)size);
            text[field].length = (size_t)size;
        }
    }
    if (input.failed || !present[1] || !present[3] || !present[4] || !present[5] ||
        !present[7] || !present[8] || !present[9] ||
        text[1].length != 64U || text[4].length != 64U || text[5].length != 64U ||
        text[3].length != 42U || text[3].bytes[0] != '0' || text[3].bytes[1] != 'x' ||
        number[9] >= (uint64_t)INT64_MAX ||
        !hex_decode(text[1].bytes, 32U, true, deposit->deposit_id) ||
        !hex_decode(text[3].bytes + 2U, 20U, false, deposit->depositor) ||
        !hex_decode(text[4].bytes, 32U, true, deposit->beneficiary) ||
        !hex_decode(text[5].bytes, 32U, true, deposit->asset_id) ||
        !decimal_u128(text[7], deposit->amount))
        return REFUSED;
    deposit->nonce = number[8];
    deposit->height = number[9];
    return LXP_OK;
}

static void put_u64(uint8_t *out, uint64_t value)
{
    for (size_t index = 0U; index < 8U; ++index)
        out[index] = (uint8_t)(value >> (56U - 8U * index));
}

lxp_result lxp_bridge_light_trust_encode(const lxp_bridge_light_trust *trust,
                                         uint8_t bytes[LXP_BRIDGE_LIGHT_TRUST_BYTES])
{
    if (trust == NULL || bytes == NULL || trust->height == 0U ||
        trust->height >= (uint64_t)INT64_MAX || trust->time_seconds <= 0 ||
        trust->time_nanos >= 1000000000U || lxp_ct_is_zero(trust->header_hash, 32U) ||
        lxp_ct_is_zero(trust->next_validators_hash, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, "LXLT1", 5U);
    put_u64(bytes + 5U, trust->height);
    (void)memcpy(bytes + 13U, trust->header_hash, 32U);
    (void)memcpy(bytes + 45U, trust->next_validators_hash, 32U);
    put_u64(bytes + 77U, (uint64_t)trust->time_seconds);
    bytes[85] = (uint8_t)(trust->time_nanos >> 24U);
    bytes[86] = (uint8_t)(trust->time_nanos >> 16U);
    bytes[87] = (uint8_t)(trust->time_nanos >> 8U);
    bytes[88] = (uint8_t)trust->time_nanos;
    return LXP_OK;
}

lxp_result lxp_bridge_light_trust_decode(const uint8_t *bytes, size_t length,
                                         lxp_bridge_light_trust *trust)
{
    uint8_t canonical[LXP_BRIDGE_LIGHT_TRUST_BYTES];
    reader input = {bytes, length, 5U, false};
    if (bytes == NULL || trust == NULL || length != LXP_BRIDGE_LIGHT_TRUST_BYTES ||
        memcmp(bytes, "LXLT1", 5U) != 0)
        return LXP_ERR_NON_CANONICAL;
    trust->height = take_uint(&input, 8U);
    (void)memcpy(trust->header_hash, take(&input, 32U), 32U);
    (void)memcpy(trust->next_validators_hash, take(&input, 32U), 32U);
    trust->time_seconds = (int64_t)take_uint(&input, 8U);
    trust->time_nanos = (uint32_t)take_uint(&input, 4U);
    if (lxp_bridge_light_trust_encode(trust, canonical) != LXP_OK ||
        memcmp(canonical, bytes, sizeof(canonical)) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}
