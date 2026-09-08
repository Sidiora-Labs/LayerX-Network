#include "layerx/lxp_state_diff.h"
#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_result sorted_accounts(const lx_account_registry *registry,
                                  const lx_account **sorted)
{
    size_t i;
    if (registry == NULL || registry->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < registry->count; ++i) {
        size_t at = i;
        const lx_account *account = &registry->accounts[i];
        lxp_result status = lx_account_validate_canonical(account);
        if (status != LXP_OK) return status;
        while (at != 0U && memcmp(sorted[at - 1U]->id, account->id, 32U) > 0) {
            sorted[at] = sorted[at - 1U];
            --at;
        }
        sorted[at] = account;
    }
    for (i = 1U; i < registry->count; ++i)
        if (memcmp(sorted[i - 1U]->id, sorted[i]->id, 32U) == 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
    return LXP_OK;
}

lxp_result lxp_state_diff_encode(const lx_account_registry *before,
                                 const lx_account_registry *after,
                                 lxp_arena *arena, lxp_byte_span *encoded)
{
    const lx_account *old[LX_ACCOUNT_REGISTRY_CAPACITY];
    const lx_account *current[LX_ACCOUNT_REGISTRY_CAPACITY];
    uint8_t old_value[LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES];
    uint8_t value[LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES];
    uint8_t key[LX_ACCOUNT_STATE_LEAF_KEY_BYTES];
    lxp_codec_writer writer;
    size_t i, previous = 0U, mark;
    uint32_t count = 0U;
    lxp_result status;
    if (arena == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    *encoded = (lxp_byte_span){NULL, 0U};
    status = sorted_accounts(before, old);
    if (status == LXP_OK) status = sorted_accounts(after, current);
    if (status != LXP_OK) return status;
    mark = lxp_arena_mark(arena);
    status = lxp_codec_writer_init(&writer, arena,
        4U + after->count * (40U + LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES));
    if (status == LXP_OK) status = lxp_codec_write_seq(&writer, 0U,
                                                       LX_ACCOUNT_REGISTRY_CAPACITY);
    for (i = 0U; status == LXP_OK && i < after->count; ++i) {
        size_t length = 0U, old_length = 0U;
        int order = previous < before->count ?
            memcmp(old[previous]->id, current[i]->id, 32U) : 1;
        if (order < 0) {
            status = LXP_FATAL_REPLAY_DIVERGENCE;
            break;
        }
        status = lx_account_state_leaf_material(current[i], key, value, &length);
        if (status == LXP_OK && order == 0) {
            status = lx_account_state_leaf_material(old[previous], key,
                                                     old_value, &old_length);
            ++previous;
            if (status == LXP_OK && length == old_length &&
                memcmp(value, old_value, length) == 0) continue;
        }
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, current[i]->id, 32U, 32U);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, value, length,
                                           LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES);
        if (status == LXP_OK) ++count;
    }
    if (status == LXP_OK && previous != before->count)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK) {
        writer.bytes[0] = (uint8_t)(count >> 24U);
        writer.bytes[1] = (uint8_t)(count >> 16U);
        writer.bytes[2] = (uint8_t)(count >> 8U);
        writer.bytes[3] = (uint8_t)count;
        *encoded = (lxp_byte_span){writer.bytes, writer.length};
    } else {
        (void)lxp_arena_reset(arena, mark);
    }
    return status;
}

static uint64_t read_u64(const uint8_t *bytes)
{
    size_t i;
    uint64_t value = 0U;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static lxp_result validate_leaf(const lxp_state_diff_entry *entry)
{
    lx_account account = {0};
    const uint8_t *bytes = entry->leaf.bytes;
    size_t offset, length;
    uint8_t key[LX_ACCOUNT_STATE_LEAF_KEY_BYTES];
    uint8_t canonical[LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES];
    lxp_result status;
    if (entry->leaf.length < 103U) return LXP_ERR_NON_CANONICAL;
    account.name_length = (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
    if (account.name_length == 0U || account.name_length > LX_ACCOUNT_NAME_MAX ||
        entry->leaf.length != 103U + account.name_length)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(account.id, entry->account_id, 32U);
    (void)memcpy(account.name, bytes + 2U, account.name_length);
    offset = 2U + account.name_length;
    account.kind = (lx_account_kind)bytes[offset++];
    status = lxp_u128_from_be(bytes + offset, &account.balance);
    offset += 16U;
    (void)memcpy(account.asset_id, bytes + offset, 32U); offset += 32U;
    if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
    account.has_asset = bytes[offset++] != 0U;
    account.next_sequence = read_u64(bytes + offset); offset += 8U;
    account.created_at_sequence = read_u64(bytes + offset); offset += 8U;
    if (bytes[offset] > 1U || bytes[offset + 1U] > 1U)
        return LXP_ERR_NON_CANONICAL;
    account.frozen = bytes[offset++] != 0U;
    account.has_open_reference = bytes[offset++] != 0U;
    (void)memcpy(account.authority_key, bytes + offset, 32U); offset += 32U;
    if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
    account.has_authority_key = bytes[offset] != 0U;
    if (status == LXP_OK)
        status = lx_account_state_leaf_material(&account, key, canonical, &length);
    if (status == LXP_OK && (length != entry->leaf.length ||
        lxp_ct_memcmp(canonical, bytes, length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    return status;
}

lxp_result lxp_state_diff_decode(lxp_byte_span encoded, lxp_arena *arena,
                                 lxp_state_diff_entry **entries, size_t *count)
{
    lxp_codec_reader reader;
    lxp_state_diff_entry *decoded = NULL;
    uint32_t length = 0U;
    size_t i, mark;
    void *memory;
    lxp_result status;
    if (arena == NULL || entries == NULL || count == NULL)
        return LXP_ERR_NON_CANONICAL;
    *entries = NULL;
    *count = 0U;
    mark = lxp_arena_mark(arena);
    status = lxp_codec_reader_init(&reader, encoded.bytes, encoded.length);
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &length);
    if (status == LXP_OK && length > LX_ACCOUNT_REGISTRY_CAPACITY)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK && length != 0U) {
        status = lxp_arena_alloc(arena, length * sizeof(*decoded),
                                 _Alignof(lxp_state_diff_entry), &memory);
        if (status == LXP_OK) decoded = memory;
    }
    for (i = 0U; status == LXP_OK && i < length; ++i) {
        lxp_byte_span id;
        status = lxp_codec_read_bytes(&reader, &id, 32U);
        if (status == LXP_OK && id.length != 32U) status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) (void)memcpy(decoded[i].account_id, id.bytes, 32U);
        if (status == LXP_OK)
            status = lxp_codec_read_bytes(&reader, &decoded[i].leaf,
                                          LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES);
        if (status == LXP_OK && i != 0U &&
            memcmp(decoded[i - 1U].account_id, decoded[i].account_id, 32U) >= 0)
            status = LXP_ERR_UNSORTED_SEQUENCE;
        if (status == LXP_OK) status = validate_leaf(&decoded[i]);
    }
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    *entries = decoded;
    *count = length;
    return LXP_OK;
}
