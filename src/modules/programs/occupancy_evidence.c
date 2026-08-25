#include "occupancy_evidence.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

enum { OCC_MAX_POSITIONS = 256, OCC_NAMESPACE_MAX = 65 };

static const uint8_t evidence_domain[] =
    "LXP/storage-occupancy-settlement/v3\0";
static const uint8_t ledger_domain[] =
    "LXP/storage-occupancy-ledger/v2\0";
static const uint8_t legacy_ledger_domain[] =
    "LXP/storage-occupancy-ledger/v1\0";
static const uint8_t mandate_domain[] =
    "LXP/storage-occupancy-mandate/v1\0";

typedef struct occ_cursor {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
} occ_cursor;

typedef struct occ_position {
    uint8_t namespace_bytes[OCC_NAMESPACE_MAX];
    uint8_t namespace_length;
    uint8_t payer[32];
    uint8_t root_program[32];
    uint8_t activity_binding[32];
    uint64_t bytes;
    uint64_t batch;
    uint64_t maximum_bytes;
    uint64_t maximum_price;
    lxp_u128 remaining;
    uint8_t mandate[32];
    lxp_u128 arrears;
    bool frozen;
    bool legacy;
} occ_position;

typedef struct occ_ledger {
    uint64_t last_finalized_batch;
    occ_position *positions;
    uint32_t count;
} occ_ledger;

static lxp_result take(occ_cursor *cursor, size_t length,
                       const uint8_t **value)
{
    if (cursor == NULL || value == NULL || cursor->offset > cursor->length ||
        length > cursor->length - cursor->offset)
        return LXP_ERR_TRUNCATED;
    *value = cursor->bytes + cursor->offset;
    cursor->offset += length;
    return LXP_OK;
}

static lxp_result take_u8(occ_cursor *cursor, uint8_t *value)
{
    const uint8_t *bytes;
    lxp_result status = take(cursor, 1U, &bytes);
    if (status == LXP_OK) *value = bytes[0];
    return status;
}

static lxp_result take_u32(occ_cursor *cursor, uint32_t *value)
{
    const uint8_t *bytes;
    lxp_result status = take(cursor, 4U, &bytes);
    if (status == LXP_OK)
        *value = ((uint32_t)bytes[0] << 24U) |
                 ((uint32_t)bytes[1] << 16U) |
                 ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
    return status;
}

static lxp_result take_u64(occ_cursor *cursor, uint64_t *value)
{
    const uint8_t *bytes;
    size_t index;
    uint64_t result = 0U;
    lxp_result status = take(cursor, 8U, &bytes);
    if (status != LXP_OK) return status;
    for (index = 0U; index < 8U; ++index)
        result = (result << 8U) | bytes[index];
    *value = result;
    return LXP_OK;
}

static lxp_result take_u128(occ_cursor *cursor, lxp_u128 *value)
{
    const uint8_t *bytes;
    lxp_result status = take(cursor, 16U, &bytes);
    return status == LXP_OK ? lxp_u128_from_be(bytes, value) : status;
}

static int namespace_compare(const occ_position *left,
                             const occ_position *right)
{
    size_t minimum = left->namespace_length < right->namespace_length ?
                     left->namespace_length : right->namespace_length;
    int order = memcmp(left->namespace_bytes, right->namespace_bytes,
                       minimum);
    if (order != 0) return order;
    return left->namespace_length < right->namespace_length ? -1 :
           left->namespace_length > right->namespace_length ? 1 : 0;
}

static lxp_result take_namespace(occ_cursor *cursor, occ_position *position)
{
    const uint8_t *bytes;
    uint8_t length;
    lxp_result status = take_u8(cursor, &length);
    if (status != LXP_OK) return status;
    if (length != 33U && length != 65U) return LXP_ERR_NON_CANONICAL;
    status = take(cursor, length, &bytes);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(bytes, 32U) ||
        (length == 65U && (bytes[32] != 0U ||
                          lxp_ct_is_zero(bytes + 33U, 32U))) ||
        (length == 33U && bytes[32] != 1U))
        return LXP_ERR_NON_CANONICAL;
    position->namespace_length = length;
    (void)memcpy(position->namespace_bytes, bytes, length);
    return LXP_OK;
}

static lxp_result decode_position(occ_cursor *cursor, occ_position *position,
                                  bool legacy)
{
    const uint8_t *bytes;
    uint8_t flag = 0U;
    lxp_result status = take_namespace(cursor, position);
    if (status != LXP_OK) return status;
    status = take(cursor, 32U, &bytes);
    if (status != LXP_OK || lxp_ct_is_zero(bytes, 32U))
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    (void)memcpy(position->payer, bytes, 32U);
    if (position->namespace_length == 65U &&
        lxp_ct_memcmp(position->payer,
                      position->namespace_bytes + 33U, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (legacy) {
        (void)memcpy(position->root_program,
                     position->namespace_bytes, 32U);
        status = take_u64(cursor, &position->bytes);
        if (status == LXP_OK) status = take_u64(cursor, &position->batch);
        position->maximum_bytes = position->bytes;
        position->frozen = true;
        position->legacy = true;
        if (status != LXP_OK) return status;
        return position->bytes == 0U ? LXP_ERR_NON_CANONICAL : LXP_OK;
    }
    status = take(cursor, 32U, &bytes);
    if (status == LXP_OK) {
        if (lxp_ct_is_zero(bytes, 32U)) return LXP_ERR_NON_CANONICAL;
        (void)memcpy(position->root_program, bytes, 32U);
        status = take(cursor, 32U, &bytes);
    }
    if (status == LXP_OK)
        (void)memcpy(position->activity_binding, bytes, 32U);
    if (status == LXP_OK) status = take_u64(cursor, &position->bytes);
    if (status == LXP_OK) status = take_u64(cursor, &position->batch);
    if (status == LXP_OK)
        status = take_u64(cursor, &position->maximum_bytes);
    if (status == LXP_OK)
        status = take_u64(cursor, &position->maximum_price);
    if (status == LXP_OK) status = take_u128(cursor, &position->remaining);
    if (status == LXP_OK) status = take(cursor, 32U, &bytes);
    if (status == LXP_OK) (void)memcpy(position->mandate, bytes, 32U);
    if (status == LXP_OK) status = take_u128(cursor, &position->arrears);
    if (status == LXP_OK) status = take_u8(cursor, &flag);
    if (status != LXP_OK) return status;
    if (flag > 1U) return LXP_ERR_NON_CANONICAL;
    position->frozen = flag != 0U;
    status = take_u8(cursor, &flag);
    if (status != LXP_OK) return status;
    if (flag > 1U) return LXP_ERR_NON_CANONICAL;
    position->legacy = flag != 0U;
    if ((position->bytes == 0U && lxp_u128_is_zero(position->arrears)) ||
        position->bytes > position->maximum_bytes ||
        (position->legacy && (!position->frozen ||
                              !lxp_u128_is_zero(position->arrears))) ||
        (!position->legacy &&
         (lxp_ct_is_zero(position->mandate, 32U) ||
          lxp_ct_is_zero(position->activity_binding, 32U) ||
          position->frozen != !lxp_u128_is_zero(position->arrears))))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result decode_ledger(lxp_programs_occupancy_bridge *bridge,
                                const uint8_t *bytes, size_t length,
                                bool permit_legacy, occ_ledger *ledger)
{
    occ_cursor cursor = {bytes, length, 0U};
    const uint8_t *domain;
    uint64_t legacy_count = 0U;
    uint32_t count = 0U;
    uint32_t index;
    bool legacy = false;
    lxp_result status;
    if (bridge == NULL || ledger == NULL || bytes == NULL || length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (length >= sizeof(ledger_domain) - 1U &&
        memcmp(bytes, ledger_domain, sizeof(ledger_domain) - 1U) == 0) {
        status = take(&cursor, sizeof(ledger_domain) - 1U, &domain);
        if (status == LXP_OK)
            status = take_u64(&cursor, &ledger->last_finalized_batch);
        if (status == LXP_OK) status = take_u32(&cursor, &count);
    } else if (permit_legacy &&
               length >= sizeof(legacy_ledger_domain) - 1U &&
               memcmp(bytes, legacy_ledger_domain,
                      sizeof(legacy_ledger_domain) - 1U) == 0) {
        legacy = true;
        status = take(&cursor, sizeof(legacy_ledger_domain) - 1U, &domain);
        if (status == LXP_OK) status = take_u64(&cursor, &legacy_count);
        if (status == LXP_OK && legacy_count > UINT32_MAX)
            status = LXP_ERR_LENGTH_LIMIT;
        count = (uint32_t)legacy_count;
    } else return LXP_ERR_VERSION_UNSUPPORTED;
    if (status != LXP_OK || count > OCC_MAX_POSITIONS)
        return status != LXP_OK ? status : LXP_ERR_LENGTH_LIMIT;
    status = lxp_ctx_arena_alloc(bridge->ctx,
        count == 0U ? 1U : (size_t)count * sizeof(*ledger->positions),
        _Alignof(occ_position), (void **)&ledger->positions);
    if (status != LXP_OK) return status;
    (void)memset(ledger->positions, 0,
                 count == 0U ? 1U : (size_t)count * sizeof(*ledger->positions));
    ledger->count = count;
    for (index = 0U; index < count; ++index) {
        status = decode_position(&cursor, &ledger->positions[index], legacy);
        if (status != LXP_OK) return status;
        if (index != 0U && namespace_compare(
                &ledger->positions[index - 1U],
                &ledger->positions[index]) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
        if (legacy && ledger->positions[index].batch >
                      ledger->last_finalized_batch)
            ledger->last_finalized_batch = ledger->positions[index].batch;
    }
    if (cursor.offset != cursor.length) return LXP_ERR_TRAILING_BYTES;
    if (legacy)
        for (index = 0U; index < count; ++index)
            ledger->positions[index].batch = ledger->last_finalized_batch;
    return LXP_OK;
}

static const occ_position *find_position(const occ_ledger *ledger,
                                         const occ_position *key)
{
    size_t left = 0U;
    size_t right = ledger->count;
    while (left < right) {
        size_t middle = left + (right - left) / 2U;
        int order = namespace_compare(&ledger->positions[middle], key);
        if (order < 0) left = middle + 1U;
        else right = middle;
    }
    return left < ledger->count &&
           namespace_compare(&ledger->positions[left], key) == 0 ?
           &ledger->positions[left] : NULL;
}

static const lxp_programs_occupancy_activation_position *find_activation(
    const lxp_programs_occupancy_bridge *bridge, const occ_position *key)
{
    uint16_t index;
    for (index = 0U; index < bridge->activation_count; ++index) {
        const lxp_programs_occupancy_activation_position *position =
            &bridge->activation_positions[index];
        if (position->namespace_length == key->namespace_length &&
            memcmp(position->namespace_bytes, key->namespace_bytes,
                   key->namespace_length) == 0)
            return position;
    }
    return NULL;
}

static void activation_as_position(
    const lxp_programs_occupancy_activation_position *activation,
    uint64_t activation_batch, occ_position *position)
{
    (void)memset(position, 0, sizeof(*position));
    position->namespace_length = activation->namespace_length;
    (void)memcpy(position->namespace_bytes, activation->namespace_bytes,
                 activation->namespace_length);
    (void)memcpy(position->payer, activation->payer, 32U);
    (void)memcpy(position->root_program, activation->namespace_bytes, 32U);
    position->bytes = activation->persistent_bytes;
    position->batch = activation_batch;
    position->maximum_bytes = activation->persistent_bytes;
    position->frozen = true;
    position->legacy = true;
}

static lxp_result activation_payer_matches(
    lxp_programs_occupancy_bridge *bridge, const occ_position *position)
{
    uint8_t key[40] = {'p','r','o','g','r','a','m',0};
    const uint8_t *record;
    size_t record_length;
    if (position->namespace_length == 65U)
        return memcmp(position->payer,
                      position->namespace_bytes + 33U, 32U) == 0 ?
               LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
    (void)memcpy(key + 8U, position->namespace_bytes, 32U);
    {
        lxp_result status = lxp_ctx_kv_get(bridge->ctx, key, sizeof(key),
                                           &record, &record_length);
        if (status != LXP_OK) return status;
    }
    return record_length == 71U &&
           memcmp(record + 1U, position->payer, 32U) == 0 ?
           LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
}

static lxp_result multiply_u64(uint64_t left, uint64_t right,
                               lxp_u128 *value)
{
    lxp_u256 product;
    lxp_result status = lxp_u128_mul((lxp_u128){0U, left},
                                     (lxp_u128){0U, right}, &product);
    if (status != LXP_OK || product.words[2] != 0U || product.words[3] != 0U)
        return LXP_ERR_OVERFLOW;
    *value = (lxp_u128){product.words[1], product.words[0]};
    return LXP_OK;
}

static lxp_result multiply_u128_u64(lxp_u128 left, uint64_t right,
                                    lxp_u128 *value)
{
    lxp_u256 product;
    lxp_result status = lxp_u128_mul(left, (lxp_u128){0U, right}, &product);
    if (status != LXP_OK || product.words[2] != 0U || product.words[3] != 0U)
        return LXP_ERR_OVERFLOW;
    *value = (lxp_u128){product.words[1], product.words[0]};
    return LXP_OK;
}

static lxp_result mandate_matches(const occ_position *charge,
                                  uint64_t maximum_bytes,
                                  uint64_t maximum_price,
                                  lxp_u128 added,
                                  const uint8_t mandate[32])
{
    uint8_t material[sizeof(mandate_domain) - 1U + 32U + 32U + 32U +
                     1U + OCC_NAMESPACE_MAX + 8U + 8U + 16U];
    uint8_t amount[16];
    uint8_t digest[32];
    size_t offset = 0U;
    size_t index;
    lxp_result status;
    (void)memcpy(material + offset, mandate_domain,
                 sizeof(mandate_domain) - 1U);
    offset += sizeof(mandate_domain) - 1U;
    (void)memcpy(material + offset, charge->payer, 32U); offset += 32U;
    (void)memcpy(material + offset, charge->root_program, 32U); offset += 32U;
    (void)memcpy(material + offset, charge->activity_binding, 32U); offset += 32U;
    material[offset++] = charge->namespace_length;
    (void)memcpy(material + offset, charge->namespace_bytes,
                 charge->namespace_length);
    offset += charge->namespace_length;
    for (index = 0U; index < 8U; ++index)
        material[offset + index] = (uint8_t)(maximum_bytes >>
                                              (56U - 8U * index));
    offset += 8U;
    for (index = 0U; index < 8U; ++index)
        material[offset + index] = (uint8_t)(maximum_price >>
                                              (56U - 8U * index));
    offset += 8U;
    status = lxp_u128_to_be(added, amount);
    if (status != LXP_OK) return status;
    (void)memcpy(material + offset, amount, sizeof(amount));
    offset += sizeof(amount);
    status = lxp_hash_sha256(material, offset, digest);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(digest, mandate, 32U) == 0 ?
           LXP_OK : LXP_ERR_CONTEXT_MISMATCH;
}

static lxp_result add_to_payer(
    lxp_programs_occupancy_bridge *bridge, const uint8_t payer[32],
    lxp_u128 due, lxp_u128 paid, lxp_u128 arrears)
{
    uint16_t index;
    if (lxp_u128_is_zero(due) && lxp_u128_is_zero(arrears)) return LXP_OK;
    for (index = 0U; index < bridge->payer_count; ++index) {
        lxp_programs_occupancy_payer *target = &bridge->payers[index];
        if (memcmp(target->principal, payer, 32U) != 0) continue;
        if (lxp_u128_add(target->verified_due, due,
                         &target->verified_due) != LXP_OK ||
            lxp_u128_add(target->verified_paid, paid,
                         &target->verified_paid) != LXP_OK ||
            lxp_u128_add(target->verified_arrears, arrears,
                         &target->verified_arrears) != LXP_OK)
            return LXP_ERR_OVERFLOW;
        return LXP_OK;
    }
    return LXP_ERR_NON_CANONICAL;
}

static lxp_result validate_evidence(
    lxp_programs_occupancy_bridge *bridge, const occ_ledger *prior,
    const occ_ledger *next, bool transition)
{
    occ_cursor cursor = {bridge->evidence, bridge->evidence_length, 0U};
    const uint8_t *bytes;
    uint64_t batch;
    uint32_t schedule_version;
    uint64_t prices[7];
    lxp_u128 declared_units, declared_fee, declared_paid, declared_arrears;
    lxp_u128 units = {0U, 0U}, fee = {0U, 0U}, paid = {0U, 0U};
    lxp_u128 arrears_total = {0U, 0U};
    lxp_u128 authorized_added = {0U, 0U};
    uint32_t count;
    uint32_t index;
    uint32_t prior_seen = 0U;
    uint32_t next_seen = 0U;
    uint16_t activation_seen = 0U;
    occ_position previous = {0};
    lxp_result status = take(&cursor, sizeof(evidence_domain) - 1U, &bytes);
    if (status != LXP_OK || memcmp(bytes, evidence_domain,
                                   sizeof(evidence_domain) - 1U) != 0)
        return status == LXP_OK ? LXP_ERR_VERSION_UNSUPPORTED : status;
    status = take_u64(&cursor, &batch);
    if (status == LXP_OK) status = take_u32(&cursor, &schedule_version);
    for (index = 0U; status == LXP_OK && index < 7U; ++index)
        status = take_u64(&cursor, &prices[index]);
    if (status == LXP_OK) status = take_u128(&cursor, &declared_units);
    if (status == LXP_OK) status = take_u128(&cursor, &declared_fee);
    if (status == LXP_OK) status = take_u128(&cursor, &declared_paid);
    if (status == LXP_OK) status = take_u128(&cursor, &declared_arrears);
    if (status == LXP_OK) status = take_u32(&cursor, &count);
    if (status != LXP_OK || batch != bridge->batch_number ||
        schedule_version != bridge->schedule_version ||
        memcmp(prices, bridge->resolved_schedule_prices, sizeof(prices)) != 0 ||
        count > OCC_MAX_POSITIONS)
        return status != LXP_OK ? status : LXP_ERR_CONTEXT_MISMATCH;
    if (transition &&
        (count < prior->count || count > prior->count + OCC_MAX_POSITIONS))
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < count; ++index) {
        occ_position charge = {0};
        occ_position activation_before = {0};
        const occ_position *before;
        const occ_position *after;
        const lxp_programs_occupancy_activation_position *activation = NULL;
        uint64_t from_batch, to_batch, recorded_bytes, final_bytes;
        uint64_t interval, price, maximum_bytes, maximum_price;
        lxp_u128 byte_batches, accrued, prior_arrears, due, added;
        lxp_u128 arrears_after, remaining, computed, available;
        uint8_t disposition;
        uint8_t mandate[32];
        status = take_namespace(&cursor, &charge);
        if (status == LXP_OK) status = take(&cursor, 32U, &bytes);
        if (status == LXP_OK) (void)memcpy(charge.payer, bytes, 32U);
        if (status == LXP_OK) status = take(&cursor, 32U, &bytes);
        if (status == LXP_OK) (void)memcpy(charge.root_program, bytes, 32U);
        if (status == LXP_OK) status = take(&cursor, 32U, &bytes);
        if (status == LXP_OK) (void)memcpy(charge.activity_binding, bytes, 32U);
        if (status == LXP_OK) status = take_u64(&cursor, &from_batch);
        if (status == LXP_OK) status = take_u64(&cursor, &to_batch);
        if (status == LXP_OK) status = take_u64(&cursor, &recorded_bytes);
        if (status == LXP_OK) status = take_u64(&cursor, &final_bytes);
        if (status == LXP_OK) status = take_u128(&cursor, &byte_batches);
        if (status == LXP_OK) status = take_u64(&cursor, &price);
        if (status == LXP_OK) status = take_u128(&cursor, &accrued);
        if (status == LXP_OK) status = take_u128(&cursor, &prior_arrears);
        if (status == LXP_OK) status = take_u128(&cursor, &due);
        if (status == LXP_OK) status = take_u128(&cursor, &added);
        if (status == LXP_OK) status = take_u8(&cursor, &disposition);
        if (status == LXP_OK) status = take_u128(&cursor, &arrears_after);
        if (status == LXP_OK) status = take_u64(&cursor, &maximum_bytes);
        if (status == LXP_OK) status = take_u64(&cursor, &maximum_price);
        if (status == LXP_OK) status = take_u128(&cursor, &remaining);
        if (status == LXP_OK) status = take(&cursor, 32U, &bytes);
        if (status != LXP_OK) return status;
        (void)memcpy(mandate, bytes, 32U);
        if (lxp_ct_is_zero(charge.payer, 32U) ||
            lxp_ct_is_zero(charge.root_program, 32U) ||
            (disposition != 5U &&
             (lxp_ct_is_zero(charge.activity_binding, 32U) ||
              lxp_ct_is_zero(mandate, 32U))))
            return LXP_ERR_NON_CANONICAL;
        if (index != 0U && namespace_compare(&previous, &charge) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
        previous = charge;
        if (charge.namespace_length == 65U &&
            memcmp(charge.payer, charge.namespace_bytes + 33U, 32U) != 0)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        before = transition ? find_position(prior, &charge) : NULL;
        if (before != NULL) ++prior_seen;
        if (transition) activation = find_activation(bridge, &charge);
        if (before != NULL && activation != NULL)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        if (before == NULL && activation != NULL) {
            activation_as_position(activation, prior->last_finalized_batch,
                                   &activation_before);
            before = &activation_before;
            ++activation_seen;
        }
        after = transition ? find_position(next, &charge) : NULL;
        if (after != NULL) ++next_seen;
        if (transition && before == NULL && after == NULL)
            return LXP_ERR_CONTEXT_MISMATCH;
        if (transition && before != NULL &&
            (memcmp(before->payer, charge.payer, 32U) != 0 ||
             before->bytes != recorded_bytes || before->batch != from_batch ||
             lxp_u128_cmp(before->arrears, prior_arrears) != 0))
            return LXP_ERR_CONTEXT_MISMATCH;
        if (transition && before != NULL && before->legacy &&
            activation_payer_matches(bridge, &charge) != LXP_OK)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        if (transition && before == NULL) {
            if (disposition == 5U || recorded_bytes != 0U ||
                from_batch != batch || !lxp_u128_is_zero(prior_arrears) ||
                lxp_u128_is_zero(added)) {
                return LXP_ERR_CONTEXT_MISMATCH;
            }
        }
        if (to_batch != batch || from_batch > to_batch ||
            final_bytes > maximum_bytes)
            return LXP_ERR_NON_CANONICAL;
        interval = to_batch - from_batch;
        status = multiply_u64(recorded_bytes, interval, &computed);
        if (status != LXP_OK || lxp_u128_cmp(computed, byte_batches) != 0)
            return LXP_ERR_OVERFLOW;
        status = multiply_u128_u64(byte_batches, price, &computed);
        if (status != LXP_OK || lxp_u128_cmp(computed, accrued) != 0)
            return LXP_ERR_OVERFLOW;
        status = lxp_u128_add(prior_arrears, accrued, &computed);
        if (status != LXP_OK || lxp_u128_cmp(computed, due) != 0)
            return LXP_ERR_OVERFLOW;
        available = before == NULL ? (lxp_u128){0U, 0U} : before->remaining;
        if (transition)
            status = lxp_u128_add(available, added, &available);
        else
            status = LXP_OK;
        if (status != LXP_OK) return status;
        if (transition && !lxp_u128_is_zero(added)) {
            if (!bridge->call_authorized || bridge->finalizing ||
                memcmp(charge.payer, bridge->authorized_payer, 32U) != 0 ||
                memcmp(charge.root_program,
                       bridge->authorized_root_program, 32U) != 0 ||
                memcmp(charge.activity_binding,
                       bridge->authorized_activity_binding, 32U) != 0 ||
                (charge.namespace_length == 33U &&
                 activation_payer_matches(bridge, &charge) != LXP_OK))
                return LXP_ERR_UNAUTHORIZED_DEBIT;
            status = lxp_u128_add(authorized_added, added,
                                  &authorized_added);
            if (status != LXP_OK || lxp_u128_cmp(
                    authorized_added,
                    bridge->authorized_responsibility_ceiling) > 0)
                return status == LXP_OK ? LXP_ERR_UNAUTHORIZED_DEBIT : status;
        }
        if (!lxp_u128_is_zero(added)) {
            status = mandate_matches(&charge, maximum_bytes, maximum_price,
                                     added, mandate);
            if (status != LXP_OK) return status;
        } else if (transition && before != NULL &&
                   (memcmp(before->mandate, mandate, 32U) != 0 ||
                    before->maximum_bytes != maximum_bytes ||
                    before->maximum_price != maximum_price ||
                    memcmp(before->root_program, charge.root_program, 32U) != 0 ||
                    memcmp(before->activity_binding,
                           charge.activity_binding, 32U) != 0))
            return LXP_ERR_CONTEXT_MISMATCH;
        if (disposition == 1U) {
            if (price != prices[6] || price > maximum_price ||
                (transition && lxp_u128_cmp(due, available) > 0) ||
                !lxp_u128_is_zero(arrears_after) ||
                (transition && (lxp_u128_sub(available, due, &computed) != LXP_OK ||
                 lxp_u128_cmp(computed, remaining) != 0)))
                return LXP_ERR_NON_CANONICAL;
            status = lxp_u128_add(paid, due, &paid);
            if (status == LXP_OK)
                status = add_to_payer(bridge, charge.payer, due, due,
                                      (lxp_u128){0U, 0U});
        } else if (disposition == 2U) {
            if (price != prices[6] || price > maximum_price ||
                (transition && lxp_u128_cmp(due, available) > 0) ||
                lxp_u128_cmp(arrears_after, due) != 0 ||
                (transition && lxp_u128_cmp(remaining, available) != 0))
                return LXP_ERR_NON_CANONICAL;
            status = lxp_u128_add(arrears_total, due, &arrears_total);
            if (status == LXP_OK)
                status = add_to_payer(bridge, charge.payer, due,
                                      (lxp_u128){0U, 0U}, due);
        } else if (disposition == 3U) {
            if (price != prices[6] || price > maximum_price ||
                (transition && lxp_u128_cmp(due, available) <= 0) ||
                lxp_u128_cmp(arrears_after, due) != 0 ||
                (transition && lxp_u128_cmp(remaining, available) != 0))
                return LXP_ERR_NON_CANONICAL;
            status = lxp_u128_add(arrears_total, due, &arrears_total);
            if (status == LXP_OK)
                status = add_to_payer(bridge, charge.payer, due,
                                      (lxp_u128){0U, 0U}, due);
        } else if (disposition == 4U) {
            if (price <= maximum_price || price != prices[6] ||
                lxp_u128_cmp(arrears_after, due) != 0 ||
                (transition && lxp_u128_cmp(remaining, available) != 0))
                return LXP_ERR_NON_CANONICAL;
            status = lxp_u128_add(arrears_total, due, &arrears_total);
            if (status == LXP_OK)
                status = add_to_payer(bridge, charge.payer, due,
                                      (lxp_u128){0U, 0U}, due);
        } else if (disposition == 5U) {
            if ((transition && before != NULL && !before->legacy) || price != 0U ||
                !lxp_u128_is_zero(accrued) || !lxp_u128_is_zero(due) ||
                !lxp_u128_is_zero(added) || !lxp_u128_is_zero(arrears_after) ||
                !lxp_u128_is_zero(remaining) || !lxp_ct_is_zero(mandate, 32U) ||
                !lxp_ct_is_zero(charge.activity_binding, 32U))
                return LXP_ERR_NON_CANONICAL;
            if (memcmp(charge.root_program,
                       charge.namespace_bytes, 32U) != 0)
                return LXP_ERR_CONTEXT_MISMATCH;
            status = LXP_OK;
        } else return LXP_ERR_INVALID_TAG;
        if (status != LXP_OK) return status;
        if (transition && after == NULL) {
            if (final_bytes != 0U || !lxp_u128_is_zero(arrears_after))
                return LXP_ERR_CONTEXT_MISMATCH;
        } else if (transition && (after->bytes != final_bytes || after->batch != batch ||
                   after->maximum_bytes != maximum_bytes ||
                   after->maximum_price != maximum_price ||
                   lxp_u128_cmp(after->remaining, remaining) != 0 ||
                   lxp_u128_cmp(after->arrears, arrears_after) != 0 ||
                   memcmp(after->payer, charge.payer, 32U) != 0 ||
                   memcmp(after->root_program, charge.root_program, 32U) != 0 ||
                   memcmp(after->activity_binding,
                          charge.activity_binding, 32U) != 0 ||
                   memcmp(after->mandate, mandate, 32U) != 0 ||
                   after->frozen != (disposition != 1U) ||
                   after->legacy != (disposition == 5U)))
            return LXP_ERR_CONTEXT_MISMATCH;
        status = lxp_u128_add(units, byte_batches, &units);
        if (status == LXP_OK) status = lxp_u128_add(fee, accrued, &fee);
        if (status != LXP_OK) return status;
    }
    if ((transition &&
         (prior_seen != prior->count || next_seen != next->count ||
          activation_seen != bridge->activation_count)) ||
        cursor.offset != cursor.length ||
        lxp_u128_cmp(units, declared_units) != 0 ||
        lxp_u128_cmp(fee, declared_fee) != 0 ||
        lxp_u128_cmp(paid, declared_paid) != 0 ||
        lxp_u128_cmp(arrears_total, declared_arrears) != 0 ||
        lxp_u128_cmp(units, bridge->byte_batches) != 0 ||
        lxp_u128_cmp(fee, bridge->fee_units) != 0 ||
        lxp_u128_cmp(paid, bridge->paid_fee_units) != 0 ||
        lxp_u128_cmp(arrears_total, bridge->arrears_fee_units) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    for (index = 0U; index < bridge->payer_count; ++index) {
        const lxp_programs_occupancy_payer *payer = &bridge->payers[index];
        if (lxp_u128_cmp(payer->due, payer->verified_due) != 0 ||
            lxp_u128_cmp(payer->paid, payer->verified_paid) != 0 ||
            lxp_u128_cmp(payer->arrears, payer->verified_arrears) != 0 ||
            payer->frozen != !lxp_u128_is_zero(payer->verified_arrears))
            return LXP_ERR_CONTEXT_MISMATCH;
    }
    return LXP_OK;
}

lxp_result lxp_programs_occupancy_validate_output(
    lxp_programs_occupancy_bridge *bridge)
{
    occ_ledger prior = {0};
    occ_ledger next = {0};
    lxp_result status;
    if (bridge == NULL || bridge->ctx == NULL || !bridge->begun ||
        bridge->next_ledger == NULL || bridge->evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (bridge->current_ledger_length == 0U) {
        if (bridge->batch_number == 0U) return LXP_ERR_BATCH_GAP;
        prior.last_finalized_batch = bridge->batch_number - 1U;
    } else {
        status = decode_ledger(bridge, bridge->current_ledger,
                               bridge->current_ledger_length, true, &prior);
        if (status != LXP_OK) return status;
    }
    status = decode_ledger(bridge, bridge->next_ledger,
                           bridge->next_ledger_length, false, &next);
    if (status != LXP_OK) return status;
    if (prior.last_finalized_batch == UINT64_MAX ||
        prior.last_finalized_batch + 1U != bridge->batch_number ||
        next.last_finalized_batch != (bridge->finalizing ?
            bridge->batch_number : prior.last_finalized_batch))
        return LXP_ERR_BATCH_GAP;
    return validate_evidence(bridge, &prior, &next, true);
}

lxp_result lxp_programs_occupancy_validate_receipt_evidence(
    const lxp_programs_occupancy_receipt *receipt)
{
    lxp_programs_occupancy_bridge bridge;
    occ_ledger empty = {0};
    uint16_t index;
    if (receipt == NULL || receipt->settlement_evidence.bytes == NULL ||
        receipt->settlement_evidence.length == 0U ||
        receipt->payer_count > LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&bridge, 0, sizeof(bridge));
    bridge.batch_number = receipt->batch_number;
    bridge.schedule_version = receipt->schedule_version;
    bridge.evidence = (uint8_t *)receipt->settlement_evidence.bytes;
    bridge.evidence_length = (uint32_t)receipt->settlement_evidence.length;
    bridge.byte_batches = receipt->byte_batches;
    bridge.fee_units = receipt->fee_units;
    bridge.paid_fee_units = receipt->paid_fee_units;
    bridge.arrears_fee_units = receipt->arrears_fee_units;
    bridge.payer_count = receipt->payer_count;
    (void)memcpy(bridge.resolved_schedule_prices,
                 receipt->schedule_prices,
                 sizeof(bridge.resolved_schedule_prices));
    for (index = 0U; index < receipt->payer_count; ++index) {
        (void)memcpy(bridge.payers[index].principal,
                     receipt->payers[index].principal, 32U);
        bridge.payers[index].due = receipt->payers[index].due;
        bridge.payers[index].paid = receipt->payers[index].paid;
        bridge.payers[index].arrears = receipt->payers[index].arrears;
        bridge.payers[index].frozen = receipt->payers[index].frozen;
    }
    return validate_evidence(&bridge, &empty, &empty, false);
}
