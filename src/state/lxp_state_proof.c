#include "layerx/lxp_state_proof.h"
#include "layerx/lxp_hash.h"

#include <stdlib.h>
#include <string.h>

static void put(uint8_t *out, uint64_t value, size_t width)
{
    for (size_t i = 0U; i < width; ++i)
        out[i] = (uint8_t)(value >> (8U * (width - i - 1U)));
}

static uint32_t get32(const uint8_t *in)
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static lxp_result leaf(const uint8_t *key, uint32_t key_length,
                       const uint8_t *value, uint32_t value_length, uint8_t out[32])
{
    lxp_hash_context context;
    uint8_t lengths[8];
    size_t tag_length;
    const uint8_t *tag = lxp_domain_tag(LXP_DOMAIN_STATE_LEAF, &tag_length);
    lxp_result status;
    if (tag == NULL) return LXP_FATAL_INVARIANT;
    put(lengths, key_length, 4U);
    put(lengths + 4U, value_length, 4U);
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, tag, tag_length);
    if (status == LXP_OK) status = lxp_hash_update(&context, lengths, 8U);
    if (status == LXP_OK) status = lxp_hash_update(&context, key, key_length);
    if (status == LXP_OK) status = lxp_hash_update(&context, value, value_length);
    if (status == LXP_OK) status = lxp_hash_final(&context, out);
    return status;
}

static lxp_result fold(uint8_t node[32], const lxp_state_proof *path)
{
    uint32_t index = path->leaf_index;
    uint32_t count = path->leaf_count;
    size_t level = 0U;
    if (count == 0U || index >= count || path->depth > LXP_STATE_PROOF_MAX_DEPTH)
        return LXP_ERR_NON_CANONICAL;
    while (count > 1U) {
        uint8_t pair[64];
        lxp_result status;
        if (level >= path->depth) return LXP_ERR_NON_CANONICAL;
        if ((index ^ 1U) >= count && memcmp(node, path->siblings[level], 32U) != 0)
            return LXP_ERR_NON_CANONICAL;
        memcpy(pair + ((index & 1U) ? 32U : 0U), node, 32U);
        memcpy(pair + ((index & 1U) ? 0U : 32U), path->siblings[level], 32U);
        status = lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), node);
        if (status != LXP_OK) return status;
        index /= 2U;
        count = count / 2U + count % 2U;
        ++level;
    }
    return level == path->depth ? LXP_OK : LXP_ERR_NON_CANONICAL;
}

static bool account_witness(const lxp_state_witness *proof)
{
    return proof->module_id == 0U && proof->key_length == 33U && proof->key[0] == 4U;
}

static lxp_result root(const lxp_state_witness *proof, uint8_t out[32])
{
    uint8_t module[2];
    lxp_result status;
    if (proof == NULL || proof->version != LXP_STATE_WITNESS_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (proof->module_id > LXP_MODULE_RESERVED_COUNT ||
        proof->key_length == 0U || proof->key_length > LXP_STATE_WITNESS_MAX_KEY ||
        proof->value_length > LXP_STATE_WITNESS_MAX_VALUE ||
        proof->layer_b.leaf_index != proof->module_id ||
        proof->layer_b.leaf_count < 9U ||
        proof->layer_b.leaf_count > LXP_MODULE_RESERVED_COUNT + 1U)
        return LXP_ERR_NON_CANONICAL;
    status = leaf(proof->key, proof->key_length, proof->value, proof->value_length, out);
    if (status == LXP_OK && account_witness(proof)) {
        status = fold(out, &proof->account_path);
        if (status == LXP_OK)
            status = leaf((const uint8_t *)"account-tree", 12U, out, 32U, out);
    }
    if (status == LXP_OK) status = fold(out, &proof->layer_a);
    put(module, proof->module_id, 2U);
    if (status == LXP_OK) status = leaf(module, 2U, out, 32U, out);
    if (status == LXP_OK) status = fold(out, &proof->layer_b);
    return status;
}

lxp_result lxp_state_proof_verify(const lxp_state_witness *proof,
                                  const uint8_t state_root[32])
{
    uint8_t computed[32];
    lxp_result status;
    if (state_root == NULL) return LXP_ERR_NON_CANONICAL;
    status = root(proof, computed);
    if (status != LXP_OK) return status;
    return memcmp(computed, state_root, 32U) == 0 ? LXP_OK : LXP_ERR_ROOT_MISMATCH;
}

static bool key_is(lxp_byte_span key, const void *bytes, size_t length)
{
    return key.length == length && memcmp(key.bytes, bytes, length) == 0;
}

static lxp_result material(const lxp_kernel *state, lxp_byte_span key,
                            lxp_state_witness *proof)
{
    if (proof->module_id != 0U) {
        for (size_t i = 0U; i < state->module_kv_count; ++i) {
            const lxp_module_kv_entry *entry = &state->module_kv[i];
            if (entry->module_id == proof->module_id &&
                key_is(key, entry->key, entry->key_length)) {
                proof->value_length = (uint32_t)entry->value_length;
                memcpy(proof->value, entry->value, entry->value_length);
                return LXP_OK;
            }
        }
        for (size_t i = 0U; i < state->blob_count; ++i) {
            const lxp_module_blob *blob = &state->blobs[i];
            uint8_t blob_key[LXP_STATE_WITNESS_MAX_KEY] = {0xffU};
            memcpy(blob_key + sizeof(blob_key) - 32U, blob->key, 32U);
            if (blob->module_id == proof->module_id && key_is(key, blob_key, sizeof(blob_key))) {
                proof->value_length = (uint32_t)blob->length;
                if (blob->length != 0U) memcpy(proof->value, blob->bytes, blob->length);
                return LXP_OK;
            }
        }
    } else {
        if (key_is(key, "sequence", 8U)) {
            put(proof->value, state->state->next_sequence, 8U);
            proof->value_length = 8U;
            return LXP_OK;
        }
        if (state->state->account_root_required && key_is(key, "account-tree", 12U)) {
            proof->value_length = 32U;
            return lx_account_registry_root(state->state->accounts, proof->value);
        }
        if (key.length == 33U && key.bytes[0] == 1U) {
            for (size_t i = 0U; i < state->state->count; ++i) {
                const lxp_state_cell *cell = &state->state->cells[i];
                if (memcmp(key.bytes + 1U, cell->key, 32U) == 0) {
                    proof->value_length = 16U;
                    return lxp_u128_to_be(cell->value, proof->value);
                }
            }
        }
        if (key.length == 33U && key.bytes[0] == 2U) {
            for (size_t i = 0U; i < state->state->idempotency_count; ++i) {
                const lxp_idempotency_key_state *entry = &state->state->idempotency[i];
                if (memcmp(key.bytes + 1U, entry->key_hash, 32U) == 0) {
                    proof->value_length = (uint32_t)entry->receipt_length;
                    return lxp_kernel_idempotency_state_value(entry->receipt, entry->receipt_length,
                                                               proof->value, sizeof(proof->value));
                }
            }
        }
        if (key.length == 7U && key.bytes[0] == 3U) {
            bool expanded = false;
            for (size_t i = 0U; i < state->module_count; ++i)
                if (state->modules[i].module_id == LXP_MODULE_PROGRAMS) expanded = true;
            for (size_t i = 0U; i < state->module_count; ++i) {
                const lxp_module_registration *entry = &state->modules[i];
                uint8_t registration_key[7] = {3U};
                put(registration_key + 1U, entry->module_id, 2U);
                put(registration_key + 3U, entry->abi_version, 4U);
                if (!key_is(key, registration_key, sizeof(registration_key))) continue;
                proof->value[0] = entry->enabled ? 1U : 0U;
                proof->value[1] = (uint8_t)entry->activity_type_count;
                proof->value_length = 16U;
                if (expanded) {
                    put(proof->value + 2U, entry->enabled_epoch, 8U);
                    put(proof->value + 10U, entry->disabled_epoch, 8U);
                    for (size_t j = 0U; j < entry->activity_type_count; ++j)
                        put(proof->value + 18U + 4U * j, entry->activity_types[j], 4U);
                    proof->value_length = 18U + 4U * (uint32_t)entry->activity_type_count;
                }
                return LXP_OK;
            }
        }
    }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_state_proof_build(const lxp_kernel *state, uint16_t module_id,
                                 lxp_byte_span key, lxp_state_witness *proof)
{
    lxp_state_witness *candidate;
    uint8_t subtree[32], state_root[32];
    lxp_result status;
    if (state == NULL || proof == NULL || key.bytes == NULL || key.length == 0U ||
        key.length > LXP_STATE_WITNESS_MAX_KEY) return LXP_ERR_NON_CANONICAL;
    candidate = calloc(1U, sizeof(*candidate));
    if (candidate == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    candidate->version = LXP_STATE_WITNESS_VERSION;
    candidate->module_id = module_id;
    candidate->key_length = (uint32_t)key.length;
    memcpy(candidate->key, key.bytes, key.length);
    lxp_byte_span subtree_key = key;
    if (account_witness(candidate))
        subtree_key = (lxp_byte_span){(const uint8_t *)"account-tree", 12U};
    status = lxp_state_subtree_proof(state, module_id, subtree_key.bytes, subtree_key.length,
                                     subtree, &candidate->layer_a);
    if (status == LXP_OK)
        status = lxp_state_root_proof(state, module_id, state_root, &candidate->layer_b);
    if (status == LXP_OK && account_witness(candidate)) {
        status = LXP_ERR_UNKNOWN_FIELD;
        if (state->state->accounts != NULL && state->state->account_root_required) {
            const lx_account_registry *registry = state->state->accounts;
            for (size_t i = 0U; i < registry->count; ++i) {
                if (memcmp(registry->accounts[i].id, key.bytes + 1U, 32U) != 0) continue;
                size_t value_length = 0U;
                status = lx_account_state_leaf_material(&registry->accounts[i],
                    candidate->key, candidate->value, &value_length);
                candidate->value_length = (uint32_t)value_length;
                if (status == LXP_OK)
                    status = lx_account_registry_proof(registry, key.bytes + 1U,
                        subtree, &candidate->account_path);
                break;
            }
        }
    } else if (status == LXP_OK) status = material(state, key, candidate);
    if (status == LXP_OK) status = lxp_state_proof_verify(candidate, state_root);
    if (status == LXP_OK) memcpy(proof, candidate, sizeof(*proof));
    free(candidate);
    return status;
}

lxp_result lxp_state_proof_encode(const lxp_state_witness *proof,
                                  uint8_t *bytes, size_t capacity, size_t *length)
{
    uint8_t computed[32];
    size_t cursor = 0U;
    size_t required;
    lxp_result status;
    if (bytes == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = root(proof, computed);
    if (status != LXP_OK) return status;
    required = 26U + proof->key_length + proof->value_length +
               32U * ((size_t)proof->layer_a.depth + proof->layer_b.depth);
    if (account_witness(proof)) required += 9U + 32U * proof->account_path.depth;
    if (capacity < required) return LXP_ERR_LENGTH_LIMIT;
    put(bytes, proof->version, 2U);
    put(bytes + 2U, proof->module_id, 2U);
    put(bytes + 4U, proof->key_length, 4U);
    cursor = 8U;
    memcpy(bytes + cursor, proof->key, proof->key_length);
    cursor += proof->key_length;
    put(bytes + cursor, proof->value_length, 4U);
    cursor += 4U;
    memcpy(bytes + cursor, proof->value, proof->value_length);
    cursor += proof->value_length;
    if (account_witness(proof)) {
        put(bytes + cursor, proof->account_path.leaf_index, 4U);
        put(bytes + cursor + 4U, proof->account_path.leaf_count, 4U);
        cursor += 8U;
        bytes[cursor++] = proof->account_path.depth;
        memcpy(bytes + cursor, proof->account_path.siblings, 32U * proof->account_path.depth);
        cursor += 32U * proof->account_path.depth;
    }
    put(bytes + cursor, proof->layer_a.leaf_index, 4U);
    put(bytes + cursor + 4U, proof->layer_a.leaf_count, 4U);
    cursor += 8U;
    for (size_t i = 0U; i < 2U; ++i) {
        const lxp_state_proof *path = i == 0U ? &proof->layer_a : &proof->layer_b;
        if (i != 0U) { put(bytes + cursor, path->leaf_count, 4U); cursor += 4U; }
        bytes[cursor++] = path->depth;
        memcpy(bytes + cursor, path->siblings, 32U * path->depth);
        cursor += 32U * path->depth;
    }
    *length = cursor;
    return LXP_OK;
}

lxp_result lxp_state_proof_decode(const uint8_t *bytes, size_t length,
                                  lxp_state_witness *proof)
{
    lxp_state_witness *candidate;
    size_t cursor = 8U;
    uint8_t computed[32];
    lxp_result status = LXP_ERR_NON_CANONICAL;
    if (bytes == NULL || proof == NULL || length < 26U ||
        length > LXP_STATE_WITNESS_MAX_BYTES) return LXP_ERR_NON_CANONICAL;
    if (bytes[0] != 0U || bytes[1] != LXP_STATE_WITNESS_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    candidate = calloc(1U, sizeof(*candidate));
    if (candidate == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    candidate->version = LXP_STATE_WITNESS_VERSION;
    candidate->module_id = (uint16_t)(((uint16_t)bytes[2] << 8U) | bytes[3]);
    candidate->key_length = get32(bytes + 4U);
    if (candidate->key_length > LXP_STATE_WITNESS_MAX_KEY ||
        candidate->key_length > length - cursor) goto done;
    memcpy(candidate->key, bytes + cursor, candidate->key_length);
    cursor += candidate->key_length;
    if (length - cursor < 4U) goto done;
    candidate->value_length = get32(bytes + cursor);
    cursor += 4U;
    if (candidate->value_length > LXP_STATE_WITNESS_MAX_VALUE ||
        candidate->value_length > length - cursor) goto done;
    memcpy(candidate->value, bytes + cursor, candidate->value_length);
    cursor += candidate->value_length;
    if (account_witness(candidate)) {
        if (length - cursor < 9U) goto done;
        candidate->account_path.leaf_index = get32(bytes + cursor);
        candidate->account_path.leaf_count = get32(bytes + cursor + 4U);
        cursor += 8U;
        candidate->account_path.depth = bytes[cursor++];
        if (candidate->account_path.depth > LXP_STATE_PROOF_MAX_DEPTH ||
            32U * candidate->account_path.depth > length - cursor) goto done;
        memcpy(candidate->account_path.siblings, bytes + cursor, 32U * candidate->account_path.depth);
        cursor += 32U * candidate->account_path.depth;
    }
    if (length - cursor < 8U) goto done;
    candidate->layer_a.leaf_index = get32(bytes + cursor);
    candidate->layer_a.leaf_count = get32(bytes + cursor + 4U);
    candidate->layer_b.leaf_index = candidate->module_id;
    cursor += 8U;
    for (size_t i = 0U; i < 2U; ++i) {
        lxp_state_proof *path = i == 0U ? &candidate->layer_a : &candidate->layer_b;
        if (i != 0U) {
            if (length - cursor < 4U) goto done;
            path->leaf_count = get32(bytes + cursor);
            cursor += 4U;
        }
        if (cursor == length) goto done;
        path->depth = bytes[cursor++];
        if (path->depth > LXP_STATE_PROOF_MAX_DEPTH ||
            32U * path->depth > length - cursor) goto done;
        memcpy(path->siblings, bytes + cursor, 32U * path->depth);
        cursor += 32U * path->depth;
    }
    if (cursor != length) goto done;
    status = root(candidate, computed);
    if (status == LXP_OK) memcpy(proof, candidate, sizeof(*proof));
done:
    free(candidate);
    return status;
}
