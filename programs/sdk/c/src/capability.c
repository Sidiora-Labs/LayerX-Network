#include "layerx/program.h"

/*
 * The canonical capability encoding consumed by program_call: a big-endian
 * count followed by one record per grant, ordered by authority key exactly as
 * the runtime orders its own set. Two SDKs that both produce this ordering
 * produce identical bytes for identical authority.
 */

static void clear_grant(lxp_program_capability *grant)
{
    size_t index;
    grant->kind = 0U;
    grant->maximum_amount.hi = 0U;
    grant->maximum_amount.lo = 0U;
    for (index = 0U; index < (size_t)LXP_PROGRAM_ID_BYTES; ++index) {
        grant->program.bytes[index] = 0U;
        grant->asset.bytes[index] = 0U;
        grant->to.bytes[index] = 0U;
        grant->receipt_digest.bytes[index] = 0U;
    }
}

static int compare_authority(const lxp_program_capability *left,
                             const lxp_program_capability *right)
{
    int order;
    if (left->kind != right->kind) return left->kind < right->kind ? -1 : 1;
    switch (left->kind) {
    case LXP_PROGRAM_CAPABILITY_CALL:
        return lxp_program_bytes_compare(left->program.bytes,
                                         right->program.bytes,
                                         (size_t)LXP_PROGRAM_ID_BYTES);
    case LXP_PROGRAM_CAPABILITY_TRANSFER_402:
        order = lxp_program_bytes_compare(left->asset.bytes,
                                          right->asset.bytes,
                                          (size_t)LXP_PROGRAM_ID_BYTES);
        if (order != 0) return order;
        return lxp_program_bytes_compare(left->to.bytes, right->to.bytes,
                                         (size_t)LXP_PROGRAM_ID_BYTES);
    case LXP_PROGRAM_CAPABILITY_RECEIPT_READ:
        return lxp_program_bytes_compare(left->receipt_digest.bytes,
                                         right->receipt_digest.bytes,
                                         (size_t)LXP_PROGRAM_DIGEST_BYTES);
    default:
        return 0;
    }
}

static size_t grant_encoded_length(const lxp_program_capability *grant)
{
    switch (grant->kind) {
    case LXP_PROGRAM_CAPABILITY_STORAGE_READ:
    case LXP_PROGRAM_CAPABILITY_STORAGE_WRITE:
    case LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_READ:
    case LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_WRITE:
    case LXP_PROGRAM_CAPABILITY_EMIT_EVENT:
        return 1U;
    case LXP_PROGRAM_CAPABILITY_CALL:
    case LXP_PROGRAM_CAPABILITY_RECEIPT_READ:
        return 1U + (size_t)LXP_PROGRAM_ID_BYTES;
    case LXP_PROGRAM_CAPABILITY_TRANSFER_402:
        return 1U + (size_t)LXP_PROGRAM_ID_BYTES + (size_t)LXP_PROGRAM_ID_BYTES +
               (size_t)LXP_PROGRAM_AMOUNT_BYTES;
    default:
        return 0U;
    }
}

static lxp_program_status validate_grant(const lxp_program_capability *grant)
{
    switch (grant->kind) {
    case LXP_PROGRAM_CAPABILITY_STORAGE_READ:
    case LXP_PROGRAM_CAPABILITY_STORAGE_WRITE:
    case LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_READ:
    case LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_WRITE:
    case LXP_PROGRAM_CAPABILITY_EMIT_EVENT:
        return LXP_PROGRAM_OK;
    case LXP_PROGRAM_CAPABILITY_CALL:
        if (lxp_program_bytes32_is_zero(grant->program.bytes))
            return LXP_PROGRAM_ERR_RESERVED_IDENTIFIER;
        return LXP_PROGRAM_OK;
    case LXP_PROGRAM_CAPABILITY_TRANSFER_402:
        if (lxp_program_bytes32_is_zero(grant->asset.bytes) ||
            lxp_program_bytes32_is_zero(grant->to.bytes))
            return LXP_PROGRAM_ERR_RESERVED_IDENTIFIER;
        if (lxp_program_amount_is_zero(grant->maximum_amount))
            return LXP_PROGRAM_ERR_ZERO_AMOUNT;
        return LXP_PROGRAM_OK;
    case LXP_PROGRAM_CAPABILITY_RECEIPT_READ:
        if (lxp_program_bytes32_is_zero(grant->receipt_digest.bytes))
            return LXP_PROGRAM_ERR_RESERVED_IDENTIFIER;
        return LXP_PROGRAM_OK;
    default:
        return LXP_PROGRAM_ERR_INVALID;
    }
}

lxp_program_capability lxp_program_capability_storage_read(void)
{
    lxp_program_capability grant;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_STORAGE_READ;
    return grant;
}

lxp_program_capability lxp_program_capability_storage_write(void)
{
    lxp_program_capability grant;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_STORAGE_WRITE;
    return grant;
}

lxp_program_capability lxp_program_capability_shared_storage_read(void)
{
    lxp_program_capability grant;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_READ;
    return grant;
}

lxp_program_capability lxp_program_capability_shared_storage_write(void)
{
    lxp_program_capability grant;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_SHARED_STORAGE_WRITE;
    return grant;
}

lxp_program_capability lxp_program_capability_emit_event(void)
{
    lxp_program_capability grant;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_EMIT_EVENT;
    return grant;
}

lxp_program_status lxp_program_capability_call(lxp_program_id program,
                                               lxp_program_capability *out)
{
    lxp_program_capability grant;
    lxp_program_status status;
    if (out == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_CALL;
    lxp_program_copy(grant.program.bytes, program.bytes,
                     (size_t)LXP_PROGRAM_ID_BYTES);
    status = validate_grant(&grant);
    if (status != LXP_PROGRAM_OK) return status;
    *out = grant;
    return LXP_PROGRAM_OK;
}

lxp_program_status lxp_program_capability_transfer_402(
    lxp_program_asset asset, lxp_program_account to,
    lxp_program_amount maximum_amount, lxp_program_capability *out)
{
    lxp_program_capability grant;
    lxp_program_status status;
    if (out == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_TRANSFER_402;
    lxp_program_copy(grant.asset.bytes, asset.bytes,
                     (size_t)LXP_PROGRAM_ID_BYTES);
    lxp_program_copy(grant.to.bytes, to.bytes, (size_t)LXP_PROGRAM_ID_BYTES);
    grant.maximum_amount = maximum_amount;
    status = validate_grant(&grant);
    if (status != LXP_PROGRAM_OK) return status;
    *out = grant;
    return LXP_PROGRAM_OK;
}

lxp_program_status lxp_program_capability_receipt_read(
    lxp_program_digest receipt_digest, lxp_program_capability *out)
{
    lxp_program_capability grant;
    lxp_program_status status;
    if (out == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    clear_grant(&grant);
    grant.kind = (uint8_t)LXP_PROGRAM_CAPABILITY_RECEIPT_READ;
    lxp_program_copy(grant.receipt_digest.bytes, receipt_digest.bytes,
                     (size_t)LXP_PROGRAM_DIGEST_BYTES);
    status = validate_grant(&grant);
    if (status != LXP_PROGRAM_OK) return status;
    *out = grant;
    return LXP_PROGRAM_OK;
}

lxp_program_status lxp_program_capability_set_init(
    lxp_program_capability_set *set, lxp_program_capability *storage,
    uint16_t capacity)
{
    if (set == NULL || storage == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    if (capacity == 0U || capacity > (uint16_t)LXP_PROGRAM_MAX_CAPABILITIES)
        return LXP_PROGRAM_ERR_CAPABILITY_LIMIT;
    set->grants = storage;
    set->capacity = capacity;
    set->count = 0U;
    return LXP_PROGRAM_OK;
}

lxp_program_status lxp_program_capability_set_push(
    lxp_program_capability_set *set, lxp_program_capability grant)
{
    lxp_program_status status;
    uint16_t position;
    uint16_t index;
    if (set == NULL || set->grants == NULL)
        return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    status = validate_grant(&grant);
    if (status != LXP_PROGRAM_OK) return status;
    if (set->count >= set->capacity ||
        set->count >= (uint16_t)LXP_PROGRAM_MAX_CAPABILITIES)
        return LXP_PROGRAM_ERR_CAPABILITY_LIMIT;
    position = 0U;
    while (position < set->count) {
        int order = compare_authority(&set->grants[position], &grant);
        if (order == 0) return LXP_PROGRAM_ERR_DUPLICATE_CAPABILITY;
        if (order > 0) break;
        position = (uint16_t)(position + 1U);
    }
    index = set->count;
    while (index > position) {
        set->grants[index] = set->grants[index - 1U];
        index = (uint16_t)(index - 1U);
    }
    set->grants[position] = grant;
    set->count = (uint16_t)(set->count + 1U);
    return LXP_PROGRAM_OK;
}

size_t lxp_program_capability_set_encoded_length(
    const lxp_program_capability_set *set)
{
    size_t total = 2U;
    uint16_t index;
    if (set == NULL || set->grants == NULL) return 0U;
    for (index = 0U; index < set->count; ++index)
        total += grant_encoded_length(&set->grants[index]);
    return total;
}

lxp_program_status lxp_program_capability_set_encode(
    const lxp_program_capability_set *set, uint8_t *out, size_t capacity,
    size_t *length)
{
    size_t cursor;
    size_t required;
    uint16_t index;
    if (set == NULL || set->grants == NULL || out == NULL || length == NULL)
        return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    required = lxp_program_capability_set_encoded_length(set);
    if (required > (size_t)LXP_PROGRAM_MAX_CAPABILITY_BYTES)
        return LXP_PROGRAM_ERR_CAPABILITY_BYTES;
    if (required > capacity) return LXP_PROGRAM_ERR_BUFFER_TOO_SMALL;
    lxp_program_write_u16_be(out, set->count);
    cursor = 2U;
    for (index = 0U; index < set->count; ++index) {
        const lxp_program_capability *grant = &set->grants[index];
        out[cursor] = grant->kind;
        cursor += 1U;
        switch (grant->kind) {
        case LXP_PROGRAM_CAPABILITY_CALL:
            lxp_program_copy(out + cursor, grant->program.bytes,
                             (size_t)LXP_PROGRAM_ID_BYTES);
            cursor += (size_t)LXP_PROGRAM_ID_BYTES;
            break;
        case LXP_PROGRAM_CAPABILITY_TRANSFER_402:
            lxp_program_copy(out + cursor, grant->asset.bytes,
                             (size_t)LXP_PROGRAM_ID_BYTES);
            cursor += (size_t)LXP_PROGRAM_ID_BYTES;
            lxp_program_copy(out + cursor, grant->to.bytes,
                             (size_t)LXP_PROGRAM_ID_BYTES);
            cursor += (size_t)LXP_PROGRAM_ID_BYTES;
            lxp_program_amount_to_be(grant->maximum_amount, out + cursor);
            cursor += (size_t)LXP_PROGRAM_AMOUNT_BYTES;
            break;
        case LXP_PROGRAM_CAPABILITY_RECEIPT_READ:
            lxp_program_copy(out + cursor, grant->receipt_digest.bytes,
                             (size_t)LXP_PROGRAM_DIGEST_BYTES);
            cursor += (size_t)LXP_PROGRAM_DIGEST_BYTES;
            break;
        default:
            break;
        }
    }
    *length = cursor;
    return LXP_PROGRAM_OK;
}
