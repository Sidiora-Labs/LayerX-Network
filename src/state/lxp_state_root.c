#include "layerx/lxp_kernel.h"

#include "layerx/lxp_hash.h"
#include "lxp_state_internal.h"

#include <string.h>

enum {
    LXP_LEGACY_LAST_MODULE_ID = 8,
    LXP_STATE_MAX_LEAVES = LXP_STATE_MAX_CELLS +
                           LXP_STATE_MAX_IDEMPOTENCY +
                           LXP_KERNEL_MAX_MODULE_REGISTRATIONS +
                           LXP_KERNEL_MAX_MODULE_KV +
                           LXP_KERNEL_MAX_BLOBS + 2
};

lxp_result lxp_state_module_root_count(const lxp_kernel *kernel,
                                       size_t *count)
{
    uint16_t last_module_id = LXP_LEGACY_LAST_MODULE_ID;
    size_t i;
    if (kernel == NULL || count == NULL) return LXP_ERR_NON_CANONICAL;
    if (kernel->module_count > LXP_KERNEL_MAX_MODULE_REGISTRATIONS ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < kernel->module_count; ++i) {
        const lxp_module_registration *registration = &kernel->modules[i];
        if (registration->module_id == 0U ||
            registration->module_id > LXP_MODULE_RESERVED_COUNT)
            return LXP_ERR_UNKNOWN_MODULE;
        if (registration->module_id > last_module_id)
            last_module_id = registration->module_id;
    }
    for (i = 0U; i < kernel->module_kv_count; ++i)
        if (kernel->module_kv[i].module_id == 0U ||
            kernel->module_kv[i].module_id > last_module_id)
            return LXP_ERR_UNKNOWN_MODULE;
    *count = (size_t)last_module_id + 1U;
    return LXP_OK;
}

typedef struct state_leaf {
    uint8_t key[LXP_MODULE_MAX_KEY_BYTES + 4U];
    size_t key_length;
    uint8_t hash[32];
} state_leaf;

static int bytes_compare(const uint8_t *left, size_t left_length,
                         const uint8_t *right, size_t right_length)
{
    size_t common = left_length < right_length ? left_length : right_length;
    int comparison = memcmp(left, right, common);
    if (comparison != 0) return comparison;
    return left_length < right_length ? -1 : left_length != right_length;
}

static void leaves_sort(state_leaf *leaves, size_t count)
{
    size_t i;
    for (i = 1U; i < count; ++i) {
        state_leaf value = leaves[i];
        size_t position = i;
        while (position != 0U &&
               bytes_compare(value.key, value.key_length,
                             leaves[position - 1U].key,
                             leaves[position - 1U].key_length) < 0) {
            leaves[position] = leaves[position - 1U];
            --position;
        }
        leaves[position] = value;
    }
}

static lxp_result state_node_hash(const uint8_t left[32],
                                  const uint8_t right[32], uint8_t root[32])
{
    uint8_t pair[64];
    (void)memcpy(pair, left, 32U);
    (void)memcpy(pair + 32U, right, 32U);
    return lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), root);
}

static lxp_result leaves_root(state_leaf *leaves, size_t count,
                              uint8_t root[32])
{
    size_t level_count = count;
    size_t i;
    lxp_result status;
    if (count == 0U)
        return lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, NULL, 0U, root);
    leaves_sort(leaves, count);
    while (level_count > 1U) {
        size_t next_count = (level_count + 1U) / 2U;
        for (i = 0U; i < next_count; ++i) {
            size_t right = i * 2U + 1U;
            if (right >= level_count) right = i * 2U;
            status = state_node_hash(leaves[i * 2U].hash,
                                     leaves[right].hash, leaves[i].hash);
            if (status != LXP_OK) return status;
        }
        level_count = next_count;
    }
    (void)memcpy(root, leaves[0].hash, 32U);
    return LXP_OK;
}

static lxp_result leaf_set(state_leaf *leaf, const uint8_t *key,
                           size_t key_length, const uint8_t *value,
                           size_t value_length)
{
    lxp_hash_context context;
    lxp_result status;
    size_t tag_length;
    const uint8_t *tag;
    uint8_t lengths[8];
    size_t i;
    if (key_length > sizeof(leaf->key) ||
        key_length > UINT32_MAX || value_length > UINT32_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(leaf->key, key, key_length);
    leaf->key_length = key_length;
    for (i = 0U; i < 4U; ++i) {
        lengths[i] = (uint8_t)(key_length >> (24U - i * 8U));
        lengths[4U + i] = (uint8_t)(value_length >> (24U - i * 8U));
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
    if (status == LXP_OK) status = lxp_hash_final(&context, leaf->hash);
    return status;
}

static lxp_result universal_leaves(const lxp_kernel *kernel,
                                   state_leaf *leaves, size_t *count)
{
    size_t i;
    uint8_t value[16];
    lxp_result status;
    bool expanded_registration_commitment = false;
    if (kernel->module_count > LXP_KERNEL_MAX_MODULE_REGISTRATIONS ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < kernel->module_count; ++i)
        if (kernel->modules[i].module_id == LXP_MODULE_PROGRAMS)
            expanded_registration_commitment = true;
    *count = 0U;
    for (i = 0U; i < kernel->state->count; ++i) {
        uint8_t key[33];
        key[0] = 1U;
        (void)memcpy(key + 1U, kernel->state->cells[i].key, 32U);
        lxp_u128_to_be(kernel->state->cells[i].value, value);
        status = leaf_set(&leaves[(*count)++], key, sizeof(key), value, 16U);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < kernel->state->idempotency_count; ++i) {
        const lxp_idempotency_key_state *entry = &kernel->state->idempotency[i];
        uint8_t key[33];
        key[0] = 2U;
        (void)memcpy(key + 1U, entry->key_hash, 32U);
        status = leaf_set(&leaves[(*count)++], key, sizeof(key),
                          entry->receipt, entry->receipt_length);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < kernel->module_count; ++i) {
        const lxp_module_registration *registration = &kernel->modules[i];
        uint8_t key[7];
        uint8_t body[18U + 4U * LXP_MODULE_MAX_ACTIVITY_TYPES] = { 0 };
        size_t body_length = 16U;
        size_t j;
        if (registration->activity_type_count >
            LXP_MODULE_MAX_ACTIVITY_TYPES)
            return LXP_ERR_LENGTH_LIMIT;
        key[0] = 3U;
        key[1] = (uint8_t)(registration->module_id >> 8U);
        key[2] = (uint8_t)registration->module_id;
        key[3] = (uint8_t)(registration->abi_version >> 24U);
        key[4] = (uint8_t)(registration->abi_version >> 16U);
        key[5] = (uint8_t)(registration->abi_version >> 8U);
        key[6] = (uint8_t)registration->abi_version;
        body[0] = registration->enabled ? 1U : 0U;
        body[1] = (uint8_t)registration->activity_type_count;
        if (expanded_registration_commitment) {
            for (j = 0U; j < 8U; ++j) {
                body[2U + j] = (uint8_t)(registration->enabled_epoch >>
                                         (56U - 8U * j));
                body[10U + j] = (uint8_t)(registration->disabled_epoch >>
                                          (56U - 8U * j));
            }
            for (j = 0U; j < registration->activity_type_count; ++j) {
                uint32_t type = registration->activity_types[j];
                size_t offset = 18U + 4U * j;
                body[offset] = (uint8_t)(type >> 24U);
                body[offset + 1U] = (uint8_t)(type >> 16U);
                body[offset + 2U] = (uint8_t)(type >> 8U);
                body[offset + 3U] = (uint8_t)type;
            }
            body_length = 18U + 4U * registration->activity_type_count;
        }
        status = leaf_set(&leaves[(*count)++], key, sizeof(key), body,
                          body_length);
        if (status != LXP_OK) return status;
    }
    value[0] = (uint8_t)(kernel->state->next_sequence >> 56U);
    value[1] = (uint8_t)(kernel->state->next_sequence >> 48U);
    value[2] = (uint8_t)(kernel->state->next_sequence >> 40U);
    value[3] = (uint8_t)(kernel->state->next_sequence >> 32U);
    value[4] = (uint8_t)(kernel->state->next_sequence >> 24U);
    value[5] = (uint8_t)(kernel->state->next_sequence >> 16U);
    value[6] = (uint8_t)(kernel->state->next_sequence >> 8U);
    value[7] = (uint8_t)kernel->state->next_sequence;
    return leaf_set(&leaves[(*count)++], (const uint8_t *)"sequence", 8U,
                    value, 8U);
}

lxp_result lxp_state_subtree_root(const lxp_kernel *kernel,
                                  uint16_t module_id, uint8_t root[32])
{
    state_leaf leaves[LXP_STATE_MAX_LEAVES];
    size_t count = 0U;
    size_t i;
    lxp_result status;
    if (kernel == NULL || root == NULL || module_id >
        LXP_MODULE_RESERVED_COUNT) return LXP_ERR_NON_CANONICAL;
    if (module_id == 0U) {
        status = universal_leaves(kernel, leaves, &count);
        return status == LXP_OK ? leaves_root(leaves, count, root) : status;
    }
    for (i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != module_id) continue;
        status = leaf_set(&leaves[count++], entry->key, entry->key_length,
                          entry->value, entry->value_length);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        uint8_t key[LXP_MODULE_MAX_KEY_BYTES + 1U] = { 0 };
        if (blob->module_id != module_id) continue;
        key[0] = 0xffU;
        (void)memcpy(key + sizeof(key) - 32U, blob->key, 32U);
        status = leaf_set(&leaves[count++], key, sizeof(key), blob->bytes,
                          blob->length);
        if (status != LXP_OK) return status;
    }
    return leaves_root(leaves, count, root);
}

lxp_result lxp_state_supply_check(const lxp_kernel *kernel)
{
    lxp_result status;
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    if (kernel->check_supply == NULL) return LXP_OK;
    status = kernel->check_supply(kernel);
    return status == LXP_OK ? LXP_OK : LXP_FATAL_SUPPLY_MISMATCH;
}

lxp_result lxp_state_root(const lxp_kernel *kernel, uint8_t root[32])
{
    state_leaf leaves[LXP_MODULE_RESERVED_COUNT + 1U];
    size_t module_id;
    size_t module_root_count;
    size_t count = 0U;
    lxp_result status;
    uint8_t key[2];
    if (kernel == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_supply_check(kernel);
    if (status != LXP_OK) return status;
    status = lxp_state_module_root_count(kernel, &module_root_count);
    if (status != LXP_OK) return status;
    for (module_id = 0U; module_id < module_root_count; ++module_id) {
        uint8_t subtree[32];
        status = lxp_state_subtree_root(kernel, (uint16_t)module_id, subtree);
        if (status != LXP_OK) return status;
        key[0] = (uint8_t)(module_id >> 8U);
        key[1] = (uint8_t)module_id;
        status = leaf_set(&leaves[count++], key, sizeof(key), subtree, 32U);
        if (status != LXP_OK) return status;
    }
    return leaves_root(leaves, count, root);
}

lxp_result lxp_state_root_chain(const uint8_t previous_root[32],
                                const uint8_t state_root[32],
                                uint64_t global_sequence, uint8_t root[32])
{
    uint8_t input[72];
    size_t i;
    if (previous_root == NULL || state_root == NULL || root == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(input, previous_root, 32U);
    (void)memcpy(input + 32U, state_root, 32U);
    for (i = 0U; i < 8U; ++i)
        input[64U + i] = (uint8_t)(global_sequence >> (56U - 8U * i));
    return lxp_hash_domain(LXP_DOMAIN_STATE_ROOT_CHAIN, input, sizeof(input),
                           root);
}
