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

static lxp_result kernel_state_validate(const lxp_kernel *kernel)
{
    size_t blob_total = 0U;
    size_t i;
    uint16_t last_module_id = LXP_LEGACY_LAST_MODULE_ID;
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    if (kernel->state == NULL) return LXP_FATAL_INVARIANT;
    if (kernel->state->count > LXP_STATE_MAX_CELLS ||
        kernel->state->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        kernel->module_count > LXP_KERNEL_MAX_MODULE_REGISTRATIONS ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV ||
        kernel->blob_count > LXP_KERNEL_MAX_BLOBS ||
        kernel->blob_total_bytes > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES ||
        (kernel->state->accounts != NULL &&
         kernel->state->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY))
        return LXP_ERR_LENGTH_LIMIT;
    if (kernel->state->account_root_required &&
        kernel->state->accounts == NULL)
        return LXP_FATAL_INVARIANT;
    for (i = 0U; i < kernel->state->idempotency_count; ++i)
        if (kernel->state->idempotency[i].receipt_length >
            LXP_STATE_MAX_RECEIPT_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < kernel->module_count; ++i)
        if (kernel->modules[i].activity_type_count >
            LXP_MODULE_MAX_ACTIVITY_TYPES)
            return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < kernel->module_count; ++i)
        if (kernel->modules[i].module_id != 0U &&
            kernel->modules[i].module_id <= LXP_MODULE_RESERVED_COUNT &&
            kernel->modules[i].module_id > last_module_id)
            last_module_id = kernel->modules[i].module_id;
    for (i = 0U; i < kernel->module_kv_count; ++i) {
        if (kernel->module_kv[i].module_id == 0U ||
            kernel->module_kv[i].module_id > last_module_id)
            return LXP_ERR_UNKNOWN_MODULE;
        if (kernel->module_kv[i].key_length == 0U ||
            kernel->module_kv[i].key_length > LXP_MODULE_MAX_KEY_BYTES ||
            kernel->module_kv[i].value_length > LXP_MODULE_MAX_VALUE_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
    }
    for (i = 0U; i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        if (blob->length > LXP_KERNEL_MAX_BLOB_BYTES ||
            blob->length > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES - blob_total)
            return LXP_ERR_LENGTH_LIMIT;
        if (blob->length != 0U && blob->bytes == NULL)
            return LXP_FATAL_INVARIANT;
        blob_total += blob->length;
    }
    return blob_total == kernel->blob_total_bytes ? LXP_OK :
           LXP_FATAL_INVARIANT;
}

lxp_result lxp_state_module_root_count(const lxp_kernel *kernel,
                                       size_t *count)
{
    uint16_t last_module_id = LXP_LEGACY_LAST_MODULE_ID;
    size_t i;
    if (count == NULL) return LXP_ERR_NON_CANONICAL;
    {
        lxp_result validation = kernel_state_validate(kernel);
        if (validation != LXP_OK) return validation;
    }
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

static lxp_result leaves_proof(state_leaf *leaves, size_t count,
                               const uint8_t *key, size_t key_length,
                               uint8_t root[32], lxp_state_proof *proof)
{
    size_t index;
    size_t level_count;
    size_t depth = 0U;
    lxp_result status;
    if (leaves == NULL || key == NULL || root == NULL || proof == NULL ||
        count == 0U || count > UINT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    leaves_sort(leaves, count);
    for (index = 0U; index < count; ++index)
        if (bytes_compare(leaves[index].key, leaves[index].key_length,
                          key, key_length) == 0)
            break;
    if (index == count) return LXP_ERR_UNKNOWN_FIELD;
    (void)memset(proof, 0, sizeof(*proof));
    proof->leaf_index = (uint32_t)index;
    proof->leaf_count = (uint32_t)count;
    level_count = count;
    while (level_count > 1U) {
        size_t sibling = index ^ 1U;
        size_t next_count = (level_count + 1U) / 2U;
        size_t node;
        if (depth == LXP_STATE_PROOF_MAX_DEPTH)
            return LXP_ERR_LENGTH_LIMIT;
        if (sibling >= level_count) sibling = index;
        (void)memcpy(proof->siblings[depth], leaves[sibling].hash, 32U);
        for (node = 0U; node < next_count; ++node) {
            size_t right = node * 2U + 1U;
            if (right >= level_count) right = node * 2U;
            status = state_node_hash(leaves[node * 2U].hash,
                                     leaves[right].hash, leaves[node].hash);
            if (status != LXP_OK) return status;
        }
        index /= 2U;
        level_count = next_count;
        ++depth;
    }
    proof->depth = (uint8_t)depth;
    (void)memcpy(root, leaves[0].hash, 32U);
    return LXP_OK;
}

static lxp_result leaf_set(state_leaf *leaf, const uint8_t *key,
                           size_t key_length, const uint8_t *value,
                           size_t value_length);

static void account_write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static lxp_result account_leaf_material(
    const lx_account *account, uint8_t key[33], uint8_t value[615],
    size_t *value_length)
{
    size_t offset = 0U;
    lxp_result status;
    if (account == NULL || key == NULL || value == NULL ||
        value_length == NULL || account->name_length == 0U ||
        account->name_length > LX_ACCOUNT_NAME_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_validate_canonical(account);
    if (status != LXP_OK) return status;
    key[0] = 4U;
    (void)memcpy(key + 1U, account->id, 32U);
    value[offset++] = (uint8_t)(account->name_length >> 8U);
    value[offset++] = (uint8_t)account->name_length;
    (void)memcpy(value + offset, account->name, account->name_length);
    offset += account->name_length;
    value[offset++] = (uint8_t)account->kind;
    status = lxp_u128_to_be(account->balance, value + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    (void)memcpy(value + offset, account->asset_id, 32U);
    offset += 32U;
    value[offset++] = account->has_asset ? 1U : 0U;
    account_write_u64(value + offset, account->next_sequence);
    offset += 8U;
    account_write_u64(value + offset, account->created_at_sequence);
    offset += 8U;
    value[offset++] = account->frozen ? 1U : 0U;
    value[offset++] = account->has_open_reference ? 1U : 0U;
    (void)memcpy(value + offset, account->authority_key, 32U);
    offset += 32U;
    value[offset++] = account->has_authority_key ? 1U : 0U;
    *value_length = offset;
    return LXP_OK;
}

lxp_result lx_account_registry_root(const lx_account_registry *registry,
                                    uint8_t root[32])
{
    state_leaf leaves[LX_ACCOUNT_REGISTRY_CAPACITY];
    size_t count;
    size_t i;
    size_t j;
    lxp_result status;
    if (registry == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    if (registry->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    count = registry->count;
    for (i = 0U; i < count; ++i) {
        uint8_t key[33];
        uint8_t value[615];
        size_t value_length;
        for (j = 0U; j < i; ++j)
            if (memcmp(registry->accounts[j].id,
                       registry->accounts[i].id, 32U) == 0)
                return LXP_ERR_NON_CANONICAL;
        status = account_leaf_material(&registry->accounts[i], key, value,
                                       &value_length);
        if (status == LXP_OK)
            status = leaf_set(&leaves[i], key, sizeof(key), value,
                              value_length);
        if (status != LXP_OK) return status;
    }
    return leaves_root(leaves, count, root);
}

lxp_result lx_account_registry_proof(
    const lx_account_registry *registry, const uint8_t account_id[32],
    uint8_t root[32], lxp_state_proof *proof)
{
    state_leaf leaves[LX_ACCOUNT_REGISTRY_CAPACITY];
    uint8_t target[33];
    size_t count;
    size_t index;
    size_t prior;
    lxp_result status;
    if (registry == NULL || account_id == NULL || root == NULL || proof == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (registry->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    count = registry->count;
    for (index = 0U; index < count; ++index) {
        uint8_t key[33];
        uint8_t value[615];
        size_t value_length;
        for (prior = 0U; prior < index; ++prior)
            if (memcmp(registry->accounts[prior].id,
                       registry->accounts[index].id, 32U) == 0)
                return LXP_ERR_NON_CANONICAL;
        status = account_leaf_material(&registry->accounts[index], key, value,
                                       &value_length);
        if (status == LXP_OK)
            status = leaf_set(&leaves[index], key, sizeof(key), value,
                              value_length);
        if (status != LXP_OK) return status;
    }
    target[0] = 4U;
    (void)memcpy(target + 1U, account_id, 32U);
    return leaves_proof(leaves, count, target, sizeof(target), root, proof);
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
    if (kernel->state->account_root_required) {
        uint8_t account_root[32];
        static const uint8_t account_key[] = "account-tree";
        if (kernel->state->accounts == NULL) return LXP_FATAL_INVARIANT;
        status = lx_account_registry_root(kernel->state->accounts,
                                          account_root);
        if (status != LXP_OK) return status;
        status = leaf_set(&leaves[(*count)++], account_key,
                          sizeof(account_key) - 1U, account_root,
                          sizeof(account_root));
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
    status = kernel_state_validate(kernel);
    if (status != LXP_OK) return status;
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

lxp_result lxp_state_subtree_proof(
    const lxp_kernel *kernel, uint16_t module_id, const uint8_t *key,
    size_t key_length, uint8_t root[32], lxp_state_proof *proof)
{
    state_leaf leaves[LXP_STATE_MAX_LEAVES];
    size_t count = 0U;
    size_t index;
    lxp_result status;
    if (kernel == NULL || key == NULL || root == NULL || proof == NULL ||
        module_id > LXP_MODULE_RESERVED_COUNT)
        return LXP_ERR_NON_CANONICAL;
    status = kernel_state_validate(kernel);
    if (status != LXP_OK) return status;
    if (module_id == 0U) {
        status = universal_leaves(kernel, leaves, &count);
        return status == LXP_OK ?
            leaves_proof(leaves, count, key, key_length, root, proof) : status;
    }
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id != module_id) continue;
        status = leaf_set(&leaves[count++], entry->key, entry->key_length,
                          entry->value, entry->value_length);
        if (status != LXP_OK) return status;
    }
    for (index = 0U; index < kernel->blob_count; ++index) {
        const lxp_module_blob *blob = &kernel->blobs[index];
        uint8_t blob_key[LXP_MODULE_MAX_KEY_BYTES + 1U] = { 0 };
        if (blob->module_id != module_id) continue;
        blob_key[0] = 0xffU;
        (void)memcpy(blob_key + sizeof(blob_key) - 32U, blob->key, 32U);
        status = leaf_set(&leaves[count++], blob_key, sizeof(blob_key),
                          blob->bytes, blob->length);
        if (status != LXP_OK) return status;
    }
    return leaves_proof(leaves, count, key, key_length, root, proof);
}

lxp_result lxp_state_supply_check(const lxp_kernel *kernel)
{
    lxp_result status;
    status = kernel_state_validate(kernel);
    if (status != LXP_OK) return status;
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
    status = kernel_state_validate(kernel);
    if (status != LXP_OK) return status;
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

lxp_result lxp_state_root_proof(const lxp_kernel *kernel, uint16_t module_id,
                                uint8_t root[32], lxp_state_proof *proof)
{
    state_leaf leaves[LXP_MODULE_RESERVED_COUNT + 1U];
    size_t current;
    size_t module_root_count;
    size_t count = 0U;
    lxp_result status;
    uint8_t target[2];
    if (kernel == NULL || root == NULL || proof == NULL ||
        module_id > LXP_MODULE_RESERVED_COUNT)
        return LXP_ERR_NON_CANONICAL;
    status = kernel_state_validate(kernel);
    if (status != LXP_OK) return status;
    status = lxp_state_supply_check(kernel);
    if (status != LXP_OK) return status;
    status = lxp_state_module_root_count(kernel, &module_root_count);
    if (status != LXP_OK) return status;
    if ((size_t)module_id >= module_root_count) return LXP_ERR_UNKNOWN_MODULE;
    for (current = 0U; current < module_root_count; ++current) {
        uint8_t subtree[32];
        uint8_t key[2];
        status = lxp_state_subtree_root(kernel, (uint16_t)current, subtree);
        if (status != LXP_OK) return status;
        key[0] = (uint8_t)(current >> 8U);
        key[1] = (uint8_t)current;
        status = leaf_set(&leaves[count++], key, sizeof(key), subtree, 32U);
        if (status != LXP_OK) return status;
    }
    target[0] = (uint8_t)(module_id >> 8U);
    target[1] = (uint8_t)module_id;
    return leaves_proof(leaves, count, target, sizeof(target), root, proof);
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
