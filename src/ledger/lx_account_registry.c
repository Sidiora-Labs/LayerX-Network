#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_ledger.h"
#include "layerx/lxp_crypto.h"

#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>

static bool system_kind(lx_account_kind kind)
{
    switch (kind) {
    case LX_ACCOUNT_SYSTEM_LIQUIDITY:
    case LX_ACCOUNT_SYSTEM_FUNDING_LONG:
    case LX_ACCOUNT_SYSTEM_FUNDING_SHORT:
    case LX_ACCOUNT_SYSTEM_INSURANCE:
    case LX_ACCOUNT_SYSTEM_FEES:
    case LX_ACCOUNT_SYSTEM_PAXEER_RESERVE:
    case LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS:
        return true;
    default:
        return false;
    }
}

static bool bytes_zero(const uint8_t *bytes, size_t length)
{
    size_t i;
    for (i = 0U; i < length; ++i)
        if (bytes[i] != 0U) return false;
    return true;
}

static bool module_name_valid(const uint8_t *name, size_t length)
{
    size_t i;
    if (name == NULL || length == 0U || length > 31U) return false;
    for (i = 0U; i < length; ++i) {
        uint8_t byte = name[i];
        if (!((byte >= (uint8_t)'a' && byte <= (uint8_t)'z') ||
              (byte >= (uint8_t)'0' && byte <= (uint8_t)'9') ||
              byte == (uint8_t)'-'))
            return false;
    }
    return true;
}

static lxp_result module_value_name(
    const uint8_t *module_name, size_t module_name_length,
    const uint8_t account_id[LX_ACCOUNT_ID_BYTES], uint8_t *name,
    uint16_t *name_length)
{
    static const uint8_t prefix[] = "module:";
    static const uint8_t marker[] = ":value:";
    static const uint8_t hex[] = "0123456789abcdef";
    size_t offset = 0U;
    size_t i;
    if (!module_name_valid(module_name, module_name_length) ||
        account_id == NULL || name == NULL || name_length == NULL ||
        bytes_zero(account_id, LX_ACCOUNT_ID_BYTES))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(name + offset, prefix, sizeof(prefix) - 1U);
    offset += sizeof(prefix) - 1U;
    (void)memcpy(name + offset, module_name, module_name_length);
    offset += module_name_length;
    (void)memcpy(name + offset, marker, sizeof(marker) - 1U);
    offset += sizeof(marker) - 1U;
    for (i = 0U; i < LX_ACCOUNT_ID_BYTES; ++i) {
        name[offset++] = hex[account_id[i] >> 4U];
        name[offset++] = hex[account_id[i] & 0x0fU];
    }
    if (offset > UINT16_MAX || offset > LX_ACCOUNT_NAME_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    *name_length = (uint16_t)offset;
    return LXP_OK;
}

static lxp_result append_creation(lxp_log *log, const lx_account *account)
{
    uint8_t body[1U + 8U + 2U + LX_ACCOUNT_NAME_MAX + 32U];
    size_t cursor = 0U;
    uint64_t sequence = account->created_at_sequence;
    size_t i;
    if (log == NULL) return LXP_OK;
    body[cursor++] = UINT8_C(0xa1);
    for (i = 0U; i < 8U; ++i)
        body[cursor++] = (uint8_t)(sequence >> ((7U - i) * 8U));
    body[cursor++] = (uint8_t)(account->name_length >> 8U);
    body[cursor++] = (uint8_t)account->name_length;
    (void)memcpy(body + cursor, account->name, account->name_length);
    cursor += account->name_length;
    (void)memcpy(body + cursor, account->id, sizeof(account->id));
    cursor += sizeof(account->id);
    return lxp_log_append(log, LXP_LOG_ACTIVITY, sequence, body,
                          (uint32_t)cursor, NULL);
}

static size_t index_position(const lx_account_registry *registry,
                             const uint8_t id[LX_ACCOUNT_ID_BYTES],
                             bool *found)
{
    size_t low = 0U;
    size_t high = registry->count;
    *found = false;
    while (low < high) {
        size_t middle = low + (high - low) / 2U;
        int order = memcmp(registry->index[middle].id, id,
                           LX_ACCOUNT_ID_BYTES);
        if (order == 0) {
            *found = true;
            return middle;
        }
        if (order < 0) low = middle + 1U;
        else high = middle;
    }
    return low;
}

static lxp_result index_insert(lx_account_registry *registry,
                               const uint8_t id[LX_ACCOUNT_ID_BYTES],
                               size_t slot)
{
    bool found = false;
    size_t position;
    if (registry->index == NULL || registry->count >= registry->index_capacity)
        return LXP_FATAL_INVARIANT;
    position = index_position(registry, id, &found);
    if (found) return LXP_FATAL_INVARIANT;
    if (position < registry->count)
        (void)memmove(&registry->index[position + 1U],
                      &registry->index[position],
                      (registry->count - position) *
                          sizeof(registry->index[0]));
    (void)memcpy(registry->index[position].id, id, LX_ACCOUNT_ID_BYTES);
    registry->index[position].slot = slot;
    return LXP_OK;
}

static lxp_result index_remove(lx_account_registry *registry, size_t slot)
{
    bool found = false;
    size_t position;
    size_t i;
    if (registry->index == NULL || slot >= registry->count ||
        registry->count > registry->index_capacity)
        return LXP_FATAL_INVARIANT;
    position = index_position(registry, registry->accounts[slot].id, &found);
    if (!found || registry->index[position].slot != slot)
        return LXP_FATAL_INVARIANT;
    if (position + 1U < registry->count)
        (void)memmove(&registry->index[position],
                      &registry->index[position + 1U],
                      (registry->count - position - 1U) *
                          sizeof(registry->index[0]));
    for (i = 0U; i + 1U < registry->count; ++i)
        if (registry->index[i].slot > slot) --registry->index[i].slot;
    return LXP_OK;
}

static size_t registry_page_bytes(void)
{
    long page = sysconf(_SC_PAGESIZE);
    return page > 0L ? (size_t)page : (size_t)4096;
}

static size_t registry_round_up(size_t bytes, size_t page)
{
    size_t remainder = bytes % page;
    if (remainder == 0U) return bytes;
    if (bytes > SIZE_MAX - (page - remainder)) return 0U;
    return bytes + (page - remainder);
}

static size_t registry_account_span(void)
{
    return (size_t)LX_ACCOUNT_REGISTRY_CAPACITY * sizeof(lx_account);
}

static size_t registry_index_span(void)
{
    return (size_t)LX_ACCOUNT_REGISTRY_CAPACITY *
           sizeof(lx_account_index_entry);
}

static lxp_result registry_map(lx_account_registry *registry)
{
    void *accounts;
    void *index;
    if (registry->accounts != NULL && registry->index != NULL) return LXP_OK;
    if (registry->accounts != NULL || registry->index != NULL)
        return LXP_FATAL_INVARIANT;
    accounts = mmap(NULL, registry_account_span(), PROT_NONE,
                    MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (accounts == MAP_FAILED) return LXP_ERR_ARENA_EXHAUSTED;
    index = mmap(NULL, registry_index_span(), PROT_NONE,
                 MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, -1, 0);
    if (index == MAP_FAILED) {
        (void)munmap(accounts, registry_account_span());
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    registry->accounts = (lx_account *)accounts;
    registry->index = (lx_account_index_entry *)index;
    registry->capacity = 0U;
    registry->index_capacity = 0U;
    return LXP_OK;
}

static lxp_result registry_commit(void *base, size_t used_bytes,
                                  size_t span_bytes, size_t page)
{
    size_t bytes = registry_round_up(used_bytes, page);
    if (bytes == 0U && used_bytes != 0U) return LXP_ERR_LENGTH_LIMIT;
    if (bytes > span_bytes) bytes = registry_round_up(span_bytes, page);
    if (bytes == 0U) return LXP_OK;
    return mprotect(base, bytes, PROT_READ | PROT_WRITE) == 0 ?
        LXP_OK : LXP_ERR_ARENA_EXHAUSTED;
}

lxp_result lx_account_registry_reserve(lx_account_registry *registry,
                                       size_t slots)
{
    size_t target;
    size_t page;
    lxp_result status;
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    if (slots > (size_t)LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    if (registry->accounts == NULL && slots == 0U) return LXP_OK;
    if (registry->accounts != NULL && slots <= registry->capacity &&
        registry->capacity == registry->index_capacity)
        return LXP_OK;
    if (registry->borrowed) return LXP_ERR_ARENA_EXHAUSTED;
    status = registry_map(registry);
    if (status != LXP_OK) return status;
    target = registry->capacity != 0U ? registry->capacity :
             (size_t)LX_ACCOUNT_REGISTRY_INITIAL_SLOTS;
    while (target < slots) {
        if (target > (size_t)LX_ACCOUNT_REGISTRY_CAPACITY / 2U) {
            target = (size_t)LX_ACCOUNT_REGISTRY_CAPACITY;
            break;
        }
        target *= 2U;
    }
    if (target < slots) return LXP_ERR_LENGTH_LIMIT;
    page = registry_page_bytes();
    status = registry_commit(registry->accounts, target * sizeof(lx_account),
                             registry_account_span(), page);
    if (status == LXP_OK)
        status = registry_commit(registry->index,
                                 target * sizeof(lx_account_index_entry),
                                 registry_index_span(), page);
    if (status != LXP_OK) return status;
    registry->capacity = target;
    registry->index_capacity = target;
    return LXP_OK;
}

void lx_account_registry_release(lx_account_registry *registry)
{
    if (registry == NULL) return;
    if (registry->borrowed) {
        registry->accounts = NULL;
        registry->index = NULL;
        registry->count = 0U;
        registry->capacity = 0U;
        registry->index_capacity = 0U;
        registry->borrowed = false;
        return;
    }
    if (registry->accounts != NULL)
        (void)munmap(registry->accounts, registry_account_span());
    if (registry->index != NULL)
        (void)munmap(registry->index, registry_index_span());
    registry->accounts = NULL;
    registry->index = NULL;
    registry->count = 0U;
    registry->capacity = 0U;
    registry->index_capacity = 0U;
}

lxp_result lx_account_registry_index_lookup(
    const lx_account_registry *registry,
    const uint8_t account_id[LX_ACCOUNT_ID_BYTES], size_t *slot)
{
    bool found = false;
    size_t position;
    if (registry == NULL || account_id == NULL || slot == NULL ||
        registry->count > registry->index_capacity)
        return LXP_ERR_NON_CANONICAL;
    if (registry->count == 0U) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    position = index_position(registry, account_id, &found);
    if (!found) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (registry->index[position].slot >= registry->count)
        return LXP_FATAL_INVARIANT;
    *slot = registry->index[position].slot;
    return LXP_OK;
}

lxp_result lx_account_registry_index_slot(const lx_account_registry *registry,
                                          size_t position, size_t *slot)
{
    if (registry == NULL || slot == NULL || position >= registry->count ||
        registry->count > registry->index_capacity)
        return LXP_ERR_NON_CANONICAL;
    if (registry->index[position].slot >= registry->count)
        return LXP_FATAL_INVARIANT;
    *slot = registry->index[position].slot;
    return LXP_OK;
}

lxp_result lx_account_registry_index_validate(
    const lx_account_registry *registry)
{
    size_t i;
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    if (registry->count > (size_t)LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    if (registry->count == 0U) return LXP_OK;
    if (registry->accounts == NULL || registry->index == NULL ||
        registry->count > registry->capacity ||
        registry->count > registry->index_capacity)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < registry->count; ++i) {
        size_t slot = registry->index[i].slot;
        if (slot >= registry->count) return LXP_FATAL_INVARIANT;
        if (memcmp(registry->index[i].id, registry->accounts[slot].id,
                   LX_ACCOUNT_ID_BYTES) != 0)
            return LXP_FATAL_INVARIANT;
        if (i != 0U && memcmp(registry->index[i - 1U].id,
                              registry->index[i].id,
                              LX_ACCOUNT_ID_BYTES) >= 0)
            return LXP_ERR_NON_CANONICAL;
    }
    return LXP_OK;
}

static int index_entry_compare(const void *left, const void *right)
{
    return memcmp(((const lx_account_index_entry *)left)->id,
                  ((const lx_account_index_entry *)right)->id,
                  LX_ACCOUNT_ID_BYTES);
}

lxp_result lx_account_registry_index_rebuild(lx_account_registry *registry)
{
    size_t i;
    lxp_result status;
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    if (registry->count == 0U) return LXP_OK;
    status = lx_account_registry_reserve(registry, registry->count);
    if (status != LXP_OK) return status;
    for (i = 0U; i < registry->count; ++i) {
        (void)memcpy(registry->index[i].id, registry->accounts[i].id,
                     LX_ACCOUNT_ID_BYTES);
        registry->index[i].slot = i;
    }
    qsort(registry->index, registry->count, sizeof(registry->index[0]),
          index_entry_compare);
    return lx_account_registry_index_validate(registry);
}

lxp_result lx_account_registry_slot_insert(lx_account_registry *registry,
                                           const lx_account *account,
                                           size_t *slot)
{
    lxp_result status;
    if (registry == NULL || account == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_account_registry_reserve(registry, registry->count + 1U);
    if (status != LXP_OK) return status;
    status = index_insert(registry, account->id, registry->count);
    if (status != LXP_OK) return status;
    registry->accounts[registry->count] = *account;
    if (slot != NULL) *slot = registry->count;
    ++registry->count;
    return LXP_OK;
}

lxp_result lx_account_registry_slot_remove(lx_account_registry *registry,
                                           size_t slot)
{
    lxp_result status;
    if (registry == NULL || slot >= registry->count)
        return LXP_ERR_NON_CANONICAL;
    status = index_remove(registry, slot);
    if (status != LXP_OK) return status;
    if (slot + 1U < registry->count)
        (void)memmove(&registry->accounts[slot],
                      &registry->accounts[slot + 1U],
                      (registry->count - slot - 1U) * sizeof(lx_account));
    --registry->count;
    return LXP_OK;
}

lxp_result lx_account_registry_copy(const lx_account_registry *source,
                                    lx_account_registry *target)
{
    lxp_result status;
    if (source == NULL || target == NULL || source == target)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(target, 0, sizeof(*target));
    atomic_init(&target->gateway_owner, NULL);
    atomic_init(&target->gateway_acquirers, 0U);
    atomic_init(&target->gateway_transition, false);
    status = lx_account_registry_index_validate(source);
    if (status != LXP_OK) return status;
    if (source->count == 0U) return LXP_OK;
    status = lx_account_registry_reserve(target, source->count);
    if (status != LXP_OK) return status;
    (void)memcpy(target->accounts, source->accounts,
                 source->count * sizeof(source->accounts[0]));
    (void)memcpy(target->index, source->index,
                 source->count * sizeof(source->index[0]));
    target->count = source->count;
    return LXP_OK;
}

lxp_result lx_account_registry_borrow(const lx_account_registry *source,
                                      lx_account *slots,
                                      lx_account_index_entry *index,
                                      size_t capacity,
                                      lx_account_registry *target)
{
    lxp_result status;
    if (source == NULL || target == NULL || source == target)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(target, 0, sizeof(*target));
    atomic_init(&target->gateway_owner, NULL);
    atomic_init(&target->gateway_acquirers, 0U);
    atomic_init(&target->gateway_transition, false);
    if (capacity < source->count ||
        (capacity != 0U && (slots == NULL || index == NULL)))
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_registry_index_validate(source);
    if (status != LXP_OK) return status;
    if (capacity == 0U) return LXP_OK;
    target->accounts = slots;
    target->index = index;
    target->capacity = capacity;
    target->index_capacity = capacity;
    target->borrowed = true;
    if (source->count != 0U) {
        (void)memcpy(target->accounts, source->accounts,
                     source->count * sizeof(source->accounts[0]));
        (void)memcpy(target->index, source->index,
                     source->count * sizeof(source->index[0]));
        target->count = source->count;
    }
    return LXP_OK;
}

lxp_result lx_account_registry_init(lx_account_registry *registry)
{
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(registry, 0, sizeof(*registry));
    atomic_init(&registry->gateway_owner, NULL);
    atomic_init(&registry->gateway_acquirers, 0U);
    atomic_init(&registry->gateway_transition, false);
    return LXP_OK;
}

lxp_result lx_account_validate_canonical(const lx_account *account)
{
    lx_account_name parsed;
    uint8_t derived[LX_ACCOUNT_ID_BYTES];
    lxp_result status;
    if (account == NULL || account->name_length == 0U ||
        account->name_length > LX_ACCOUNT_NAME_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_name_parse(account->name, account->name_length,
                                   &parsed);
    if (status == LXP_OK)
        status = lx_account_id_from_string(account->name,
                                           account->name_length, derived);
    if (status != LXP_OK || parsed.kind != account->kind ||
        memcmp(derived, account->id, LX_ACCOUNT_ID_BYTES) != 0 ||
        (!account->has_asset &&
         (!lxp_u128_is_zero(account->balance) ||
          !bytes_zero(account->asset_id, sizeof(account->asset_id)))) ||
        (account->has_asset &&
         bytes_zero(account->asset_id, sizeof(account->asset_id))) ||
        (!account->has_authority_key &&
         !bytes_zero(account->authority_key,
                     sizeof(account->authority_key))) ||
        (account->has_authority_key &&
         bytes_zero(account->authority_key,
                    sizeof(account->authority_key))))
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    if (account->name_length > 77U &&
        memcmp(account->name, "agent:", 6U) == 0 &&
        memcmp(account->name + account->name_length - 71U, ":asset:", 7U) == 0 &&
        account->has_asset) {
        static const uint8_t hex[] = "0123456789abcdef";
        size_t i;
        const uint8_t *encoded = account->name + account->name_length - 64U;
        for (i = 0U; i < 32U; ++i)
            if (encoded[i * 2U] != hex[account->asset_id[i] >> 4U] ||
                encoded[i * 2U + 1U] != hex[account->asset_id[i] & 15U])
                return LXP_ERR_ASSET_MISMATCH;
    }
    if (account->kind == LX_ACCOUNT_MODULE_VALUE && account->name_length == 79U &&
        memcmp(account->name, "asset:", 6U) == 0) {
        static const uint8_t hex[] = "0123456789abcdef";
        if (!account->has_asset) return LXP_ERR_ASSET_MISMATCH;
        for (size_t i = 0U; i < 32U; ++i)
            if (account->name[6U + i * 2U] != hex[account->asset_id[i] >> 4U] ||
                account->name[7U + i * 2U] != hex[account->asset_id[i] & 15U])
                return LXP_ERR_ASSET_MISMATCH;
    }
    return LXP_OK;
}

lxp_result lx_account_registry_snapshot(lx_account_registry *source,
                                        lx_account_registry *snapshot)
{
    bool expected = false;
    lxp_result status;
    if (source == NULL || snapshot == NULL || source == snapshot ||
        source->count > (size_t)LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (!atomic_compare_exchange_strong_explicit(
            &source->gateway_transition, &expected, true,
            memory_order_acq_rel, memory_order_acquire))
        return LXP_ERR_CONTEXT_MISMATCH;
    if (atomic_load_explicit(&source->gateway_acquirers,
                             memory_order_acquire) != 0U) {
        atomic_store_explicit(&source->gateway_transition, false,
                              memory_order_release);
        return LXP_ERR_CONTEXT_MISMATCH;
    }
    (void)memset(snapshot, 0, sizeof(*snapshot));
    atomic_init(&snapshot->gateway_owner, NULL);
    atomic_init(&snapshot->gateway_acquirers, 0U);
    atomic_init(&snapshot->gateway_transition, false);
    status = lx_account_registry_reserve(snapshot, source->count);
    if (status == LXP_OK && source->count != 0U) {
        (void)memcpy(snapshot->accounts, source->accounts,
                     source->count * sizeof(source->accounts[0]));
        (void)memcpy(snapshot->index, source->index,
                     source->count * sizeof(source->index[0]));
        snapshot->count = source->count;
    }
    atomic_store_explicit(&source->gateway_transition, false,
                          memory_order_release);
    if (status != LXP_OK) lx_account_registry_release(snapshot);
    return status;
}

lxp_result lx_account_lookup(lx_account_registry *registry,
                             const uint8_t *name, size_t name_length,
                             const uint8_t presented_id[32],
                             lx_account **account)
{
    uint8_t derived[32];
    size_t slot = 0U;
    lxp_result status;
    if (registry == NULL || presented_id == NULL || account == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_id_from_string(name, name_length, derived);
    if (status != LXP_OK) return status;
    if (memcmp(derived, presented_id, sizeof(derived)) != 0)
        return LXP_ERR_ACCOUNT_ID_MISMATCH;
    status = lx_account_registry_index_lookup(registry, derived, &slot);
    if (status != LXP_OK) return status;
    if ((size_t)registry->accounts[slot].name_length != name_length ||
        memcmp(registry->accounts[slot].name, name, name_length) != 0)
        return LXP_ERR_ACCOUNT_ID_MISMATCH;
    *account = &registry->accounts[slot];
    return LXP_OK;
}

lxp_result lx_account_open(lx_account_registry *registry,
                           const uint8_t *name, size_t name_length,
                           const uint8_t presented_id[32],
                           uint64_t global_sequence,
                           lx_account_open_authority authority,
                           lxp_log *activity_log, lx_account **account)
{
    uint8_t derived[32];
    lx_account_name parsed;
    lx_account *created;
    size_t slot = 0U;
    lxp_result status;
    if (registry == NULL || presented_id == NULL || account == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_name_parse(name, name_length, &parsed);
    if (status != LXP_OK) return status;
    status = lx_account_id_from_string(name, name_length, derived);
    if (status != LXP_OK) return status;
    if (memcmp(derived, presented_id, sizeof(derived)) != 0)
        return LXP_ERR_ACCOUNT_ID_MISMATCH;
    if (parsed.kind == LX_ACCOUNT_MODULE_VALUE)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_account_registry_index_lookup(registry, derived, &slot);
    if (status == LXP_OK) {
        if ((size_t)registry->accounts[slot].name_length != name_length ||
            memcmp(registry->accounts[slot].name, name, name_length) != 0)
            return LXP_ERR_ACCOUNT_ID_MISMATCH;
        *account = &registry->accounts[slot];
        return LXP_OK;
    }
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status;
    if (system_kind(parsed.kind) && authority == LX_ACCOUNT_OPEN_CREDIT)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_account_registry_reserve(registry, registry->count + 1U);
    if (status != LXP_OK) return status;
    created = &registry->accounts[registry->count];
    (void)memset(created, 0, sizeof(*created));
    (void)memcpy(created->id, derived, sizeof(created->id));
    (void)memcpy(created->name, name, name_length);
    created->name_length = (uint16_t)name_length;
    created->kind = parsed.kind;
    created->created_at_sequence = global_sequence;
    status = append_creation(activity_log, created);
    if (status != LXP_OK) return status;
    status = index_insert(registry, created->id, registry->count);
    if (status != LXP_OK) return status;
    ++registry->count;
    *account = created;
    return LXP_OK;
}

lxp_result lx_account_module_value_prepare(
    lx_account_registry *registry, const uint8_t *module_name,
    size_t module_name_length, const uint8_t account_id[LX_ACCOUNT_ID_BYTES],
    const uint8_t asset_id[32], uint64_t global_sequence,
    lx_account_registration *registration, lx_account **account,
    bool *created)
{
    lx_account candidate;
    uint8_t derived[LX_ACCOUNT_ID_BYTES];
    size_t slot = 0U;
    lxp_result status;
    if (registry == NULL || account_id == NULL || asset_id == NULL ||
        registration == NULL || account == NULL || created == NULL ||
        bytes_zero(asset_id, 32U) ||
        registry->count > (size_t)LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&candidate, 0, sizeof(candidate));
    status = module_value_name(module_name, module_name_length, account_id,
                               candidate.name, &candidate.name_length);
    if (status != LXP_OK) return status;
    status = lx_account_id_from_string(candidate.name, candidate.name_length,
                                       derived);
    if (status != LXP_OK || memcmp(derived, account_id, sizeof(derived)) != 0)
        return status != LXP_OK ? status : LXP_FATAL_INVARIANT;
    status = lx_account_registry_index_lookup(registry, account_id, &slot);
    if (status == LXP_OK) {
        lx_account *existing = &registry->accounts[slot];
        if (existing->kind != LX_ACCOUNT_MODULE_VALUE ||
            existing->name_length != candidate.name_length ||
            memcmp(existing->name, candidate.name,
                   candidate.name_length) != 0)
            return LXP_ERR_ACCOUNT_ID_MISMATCH;
        if (!existing->has_asset ||
            memcmp(existing->asset_id, asset_id, 32U) != 0)
            return LXP_ERR_ASSET_MISMATCH;
        (void)memset(registration, 0, sizeof(*registration));
        *account = existing;
        *created = false;
        return LXP_OK;
    }
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status;
    (void)memcpy(candidate.id, account_id, LX_ACCOUNT_ID_BYTES);
    candidate.kind = LX_ACCOUNT_MODULE_VALUE;
    (void)memcpy(candidate.asset_id, asset_id, 32U);
    candidate.has_asset = true;
    candidate.created_at_sequence = global_sequence;
    registration->account = candidate;
    registration->expected_count = registry->count;
    *account = &registration->account;
    *created = true;
    return LXP_OK;
}

lxp_result lx_account_registration_commit(
    lx_account_registry *registry, const lx_account_registration *registration,
    lx_account **account)
{
    uint8_t derived[LX_ACCOUNT_ID_BYTES];
    lxp_result status;
    if (registry == NULL || registration == NULL || account == NULL ||
        registry->count != registration->expected_count)
        return LXP_FATAL_INVARIANT;
    status = lx_account_id_from_string(registration->account.name,
                                       registration->account.name_length,
                                       derived);
    if (status != LXP_OK || registration->account.kind !=
            LX_ACCOUNT_MODULE_VALUE ||
        memcmp(derived, registration->account.id, sizeof(derived)) != 0 ||
        !registration->account.has_asset ||
        bytes_zero(registration->account.asset_id, 32U))
        return LXP_FATAL_INVARIANT;
    status = lx_account_registry_reserve(registry, registry->count + 1U);
    if (status != LXP_OK) return status;
    status = index_insert(registry, registration->account.id,
                          registry->count);
    if (status != LXP_OK) return status;
    registry->accounts[registry->count] = registration->account;
    *account = &registry->accounts[registry->count];
    ++registry->count;
    return LXP_OK;
}

lxp_result lx_account_credit_registration_commit(
    lx_account_registry *registry, const lx_account_registration *registration,
    lx_account **account)
{
    uint8_t derived[32];
    lx_account_name name;
    size_t existing = 0U;
    lxp_result status;
    if (registry == NULL || registration == NULL || account == NULL ||
        registry->count != registration->expected_count ||
        (registration->account.kind != LX_ACCOUNT_AGENT_MAIN &&
         registration->account.kind != LX_ACCOUNT_AGENT_ASSET) ||
        !registration->account.has_asset ||
        bytes_zero(registration->account.asset_id, 32U) ||
        !registration->account.has_authority_key ||
        !lxp_ed25519_pubkey_is_canonical(registration->account.authority_key) ||
        registration->account.frozen || registration->account.has_open_reference ||
        lx_account_name_parse(registration->account.name,
            registration->account.name_length, &name) != LXP_OK ||
        name.kind != registration->account.kind ||
        lx_account_id_from_string(registration->account.name,
            registration->account.name_length, derived) != LXP_OK ||
        memcmp(derived, registration->account.id, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    status = lx_account_registry_index_lookup(registry, derived, &existing);
    if (status == LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status;
    status = lx_account_registry_reserve(registry, registry->count + 1U);
    if (status != LXP_OK) return status;
    status = index_insert(registry, registration->account.id, registry->count);
    if (status != LXP_OK) return status;
    registry->accounts[registry->count] = registration->account;
    *account = &registry->accounts[registry->count++];
    return LXP_OK;
}

lxp_result lx_account_close(lx_account_registry *registry,
                            const uint8_t account_id[32])
{
    size_t slot = 0U;
    lxp_result status;
    if (registry == NULL || account_id == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_account_registry_index_lookup(registry, account_id, &slot);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(registry->accounts[slot].balance) ||
        registry->accounts[slot].has_open_reference)
        return LXP_ERR_ACCOUNT_NOT_EMPTY;
    return lx_account_registry_slot_remove(registry, slot);
}
