#include "layerx/lxp_ledger.h"

#include "layerx/lxp_hash.h"

#include <string.h>

static bool span_equal(const uint8_t *bytes, size_t length, const char *text)
{
    size_t text_length = strlen(text);
    return length == text_length && memcmp(bytes, text, length) == 0;
}

static bool canonical_bytes(const uint8_t *name, size_t length)
{
    size_t i;
    bool previous_colon = true;
    if (name == NULL || length == 0U || length > LX_ACCOUNT_NAME_MAX)
        return false;
    for (i = 0U; i < length; ++i) {
        uint8_t byte = name[i];
        bool valid = (byte >= (uint8_t)'a' && byte <= (uint8_t)'z') ||
                     (byte >= (uint8_t)'0' && byte <= (uint8_t)'9') ||
                     byte == (uint8_t)'.' || byte == (uint8_t)'_' ||
                     byte == (uint8_t)'-' || byte == (uint8_t)':';
        if (!valid || (byte == (uint8_t)':' && previous_colon)) return false;
        previous_colon = byte == (uint8_t)':';
    }
    return !previous_colon;
}

static bool has_agent_shape(const uint8_t *name, size_t length,
                            const char *marker)
{
    size_t marker_length = strlen(marker);
    size_t i;
    if (length <= 6U + marker_length || memcmp(name, "agent:", 6U) != 0)
        return false;
    for (i = 6U; i + marker_length < length; ++i) {
        if (memcmp(name + i, marker, marker_length) == 0 && i > 6U &&
            i + marker_length < length) {
            size_t tail;
            for (tail = i + marker_length; tail < length; ++tail)
                if (name[tail] == (uint8_t)':') return false;
            return true;
        }
    }
    return false;
}

static bool system_funding(const uint8_t *name, size_t length,
                           const char *suffix)
{
    static const char prefix[] = "system:funding:";
    size_t prefix_length = sizeof(prefix) - 1U;
    size_t suffix_length = strlen(suffix);
    size_t i;
    if (length <= prefix_length + suffix_length ||
        memcmp(name, prefix, prefix_length) != 0 ||
        memcmp(name + length - suffix_length, suffix, suffix_length) != 0)
        return false;
    for (i = prefix_length; i < length - suffix_length; ++i)
        if (name[i] == (uint8_t)':') return false;
    return true;
}

static bool system_tail(const uint8_t *name, size_t length,
                        const char *prefix)
{
    size_t prefix_length = strlen(prefix);
    size_t i;
    if (length <= prefix_length || memcmp(name, prefix, prefix_length) != 0)
        return false;
    for (i = prefix_length; i < length; ++i)
        if (name[i] == (uint8_t)':') return false;
    return true;
}

static bool module_value(const uint8_t *name, size_t length)
{
    static const uint8_t prefix[] = "module:";
    static const uint8_t marker[] = ":value:";
    const size_t prefix_length = sizeof(prefix) - 1U;
    const size_t marker_length = sizeof(marker) - 1U;
    const size_t identifier_length = 64U;
    size_t module_length;
    size_t i;
    if (length <= prefix_length + marker_length + identifier_length ||
        memcmp(name, prefix, prefix_length) != 0 ||
        memcmp(name + length - identifier_length - marker_length,
               marker, marker_length) != 0)
        return false;
    module_length = length - prefix_length - marker_length - identifier_length;
    if (module_length == 0U || module_length > 31U) return false;
    for (i = prefix_length; i < prefix_length + module_length; ++i) {
        uint8_t byte = name[i];
        if (!((byte >= (uint8_t)'a' && byte <= (uint8_t)'z') ||
              (byte >= (uint8_t)'0' && byte <= (uint8_t)'9') ||
              byte == (uint8_t)'-'))
            return false;
    }
    for (i = length - identifier_length; i < length; ++i) {
        uint8_t byte = name[i];
        if (!((byte >= (uint8_t)'0' && byte <= (uint8_t)'9') ||
              (byte >= (uint8_t)'a' && byte <= (uint8_t)'f')))
            return false;
    }
    return true;
}

static uint8_t hex_nibble(uint8_t byte)
{
    return byte <= (uint8_t)'9' ? (uint8_t)(byte - (uint8_t)'0') :
                                  (uint8_t)(byte - (uint8_t)'a' + 10U);
}

lxp_result lx_account_name_parse(const uint8_t *name, size_t name_length,
                                 lx_account_name *parsed)
{
    lx_account_kind kind;
    if (parsed == NULL || !canonical_bytes(name, name_length))
        return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (span_equal(name, name_length, "system:insurance"))
        kind = LX_ACCOUNT_SYSTEM_INSURANCE;
    else if (span_equal(name, name_length, "system:fees"))
        kind = LX_ACCOUNT_SYSTEM_FEES;
    else if (span_equal(name, name_length, "system:paxeer-reserve"))
        kind = LX_ACCOUNT_SYSTEM_PAXEER_RESERVE;
    else if (span_equal(name, name_length, "system:paxeer-withdrawals"))
        kind = LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS;
    else if (system_tail(name, name_length, "system:liquidity:"))
        kind = LX_ACCOUNT_SYSTEM_LIQUIDITY;
    else if (system_funding(name, name_length, ":long"))
        kind = LX_ACCOUNT_SYSTEM_FUNDING_LONG;
    else if (system_funding(name, name_length, ":short"))
        kind = LX_ACCOUNT_SYSTEM_FUNDING_SHORT;
    else if (name_length > 11U && memcmp(name, "agent:", 6U) == 0 &&
             memcmp(name + name_length - 5U, ":main", 5U) == 0)
        kind = LX_ACCOUNT_AGENT_MAIN;
    else if (has_agent_shape(name, name_length, ":budget:"))
        kind = LX_ACCOUNT_AGENT_BUDGET;
    else if (has_agent_shape(name, name_length, ":escrow:"))
        kind = LX_ACCOUNT_AGENT_ESCROW;
    else if (has_agent_shape(name, name_length, ":stream:"))
        kind = LX_ACCOUNT_AGENT_STREAM;
    else if (has_agent_shape(name, name_length, ":margin:"))
        kind = LX_ACCOUNT_AGENT_MARGIN;
    else if (module_value(name, name_length))
        kind = LX_ACCOUNT_MODULE_VALUE;
    else return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    parsed->bytes = name;
    parsed->length = name_length;
    parsed->kind = kind;
    return LXP_OK;
}

lxp_result lx_account_kind_of(const uint8_t *name, size_t name_length,
                              lx_account_kind *kind)
{
    lx_account_name parsed;
    lxp_result status;
    if (kind == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_account_name_parse(name, name_length, &parsed);
    if (status == LXP_OK) *kind = parsed.kind;
    return status;
}

lxp_result lx_account_id_from_string(const uint8_t *name, size_t name_length,
                                     uint8_t account_id[32])
{
    static const uint8_t tag[] = "LX:ACCOUNT:v1";
    uint8_t length_be[4];
    lxp_hash_context context;
    lx_account_name parsed;
    lxp_result status;
    if (account_id == NULL || name_length > UINT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_name_parse(name, name_length, &parsed);
    if (status != LXP_OK) return status;
    if (parsed.kind == LX_ACCOUNT_MODULE_VALUE) {
        const uint8_t *encoded = name + name_length - 64U;
        size_t i;
        for (i = 0U; i < 32U; ++i)
            account_id[i] = (uint8_t)((hex_nibble(encoded[i * 2U]) << 4U) |
                                      hex_nibble(encoded[i * 2U + 1U]));
        return LXP_OK;
    }
    length_be[0] = (uint8_t)(name_length >> 24U);
    length_be[1] = (uint8_t)(name_length >> 16U);
    length_be[2] = (uint8_t)(name_length >> 8U);
    length_be[3] = (uint8_t)name_length;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, tag, sizeof(tag) - 1U);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, length_be, sizeof(length_be));
    if (status == LXP_OK) status = lxp_hash_update(&context, name, name_length);
    return status == LXP_OK ? lxp_hash_final(&context, account_id) : status;
}
