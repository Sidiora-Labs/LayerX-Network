#include "layerx/lxp_ledger.h"

#include <string.h>

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

lxp_result lx_account_registry_init(lx_account_registry *registry)
{
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(registry, 0, sizeof(*registry));
    atomic_init(&registry->gateway_owner, NULL);
    return LXP_OK;
}

lxp_result lx_account_lookup(lx_account_registry *registry,
                             const uint8_t *name, size_t name_length,
                             const uint8_t presented_id[32],
                             lx_account **account)
{
    uint8_t derived[32];
    size_t i;
    lxp_result status;
    if (registry == NULL || presented_id == NULL || account == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_id_from_string(name, name_length, derived);
    if (status != LXP_OK) return status;
    if (memcmp(derived, presented_id, sizeof(derived)) != 0)
        return LXP_ERR_ACCOUNT_ID_MISMATCH;
    for (i = 0U; i < registry->count; ++i) {
        if (memcmp(registry->accounts[i].id, derived, sizeof(derived)) == 0) {
            if ((size_t)registry->accounts[i].name_length != name_length ||
                memcmp(registry->accounts[i].name, name, name_length) != 0)
                return LXP_ERR_ACCOUNT_ID_MISMATCH;
            *account = &registry->accounts[i];
            return LXP_OK;
        }
    }
    return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
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
    size_t i;
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
    for (i = 0U; i < registry->count; ++i) {
        if (memcmp(registry->accounts[i].id, derived, sizeof(derived)) == 0) {
            if ((size_t)registry->accounts[i].name_length != name_length ||
                memcmp(registry->accounts[i].name, name, name_length) != 0)
                return LXP_ERR_ACCOUNT_ID_MISMATCH;
            *account = &registry->accounts[i];
            return LXP_OK;
        }
    }
    if (system_kind(parsed.kind) && authority == LX_ACCOUNT_OPEN_CREDIT)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (registry->count == LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    created = &registry->accounts[registry->count];
    (void)memset(created, 0, sizeof(*created));
    (void)memcpy(created->id, derived, sizeof(created->id));
    (void)memcpy(created->name, name, name_length);
    created->name_length = (uint16_t)name_length;
    created->kind = parsed.kind;
    created->created_at_sequence = global_sequence;
    status = append_creation(activity_log, created);
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
    size_t i;
    lxp_result status;
    if (registry == NULL || account_id == NULL || asset_id == NULL ||
        registration == NULL || account == NULL || created == NULL ||
        bytes_zero(asset_id, 32U) ||
        registry->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&candidate, 0, sizeof(candidate));
    status = module_value_name(module_name, module_name_length, account_id,
                               candidate.name, &candidate.name_length);
    if (status != LXP_OK) return status;
    status = lx_account_id_from_string(candidate.name, candidate.name_length,
                                       derived);
    if (status != LXP_OK || memcmp(derived, account_id, sizeof(derived)) != 0)
        return status != LXP_OK ? status : LXP_FATAL_INVARIANT;
    for (i = 0U; i < registry->count; ++i) {
        lx_account *existing = &registry->accounts[i];
        if (memcmp(existing->id, account_id, LX_ACCOUNT_ID_BYTES) != 0)
            continue;
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
    if (registry->count == LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
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
        registry->count != registration->expected_count ||
        registry->count == LX_ACCOUNT_REGISTRY_CAPACITY)
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
    registry->accounts[registry->count] = registration->account;
    *account = &registry->accounts[registry->count];
    ++registry->count;
    return LXP_OK;
}

lxp_result lx_account_close(lx_account_registry *registry,
                            const uint8_t account_id[32])
{
    size_t i;
    if (registry == NULL || account_id == NULL) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < registry->count; ++i) {
        if (memcmp(registry->accounts[i].id, account_id, 32U) == 0) {
            if (!lxp_u128_is_zero(registry->accounts[i].balance) ||
                registry->accounts[i].has_open_reference)
                return LXP_ERR_ACCOUNT_NOT_EMPTY;
            if (i + 1U < registry->count)
                (void)memmove(&registry->accounts[i], &registry->accounts[i + 1U],
                              (registry->count - i - 1U) * sizeof(lx_account));
            --registry->count;
            return LXP_OK;
        }
    }
    return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}
