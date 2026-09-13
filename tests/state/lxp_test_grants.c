#include "layerx/lxp_authority.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#define FEE_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "fee grant check failed at line %d\n", __LINE__); return 1; \
} } while (0)

static int fee_grants(void)
{
    uint8_t storage[2048], encoded_v1[1024], encoded_v2[1024], first_id[32], second_id[32];
    uint8_t owner[32] = {1U}, key[32] = {2U};
    lxp_authority_grant grant, decoded, narrower;
    lxp_authority_fee_budget original;
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t legacy_length, length;
    FEE_CHECK(lxp_session_key_bind(&grant, owner, key, 2U, 1U, 9U, 10U, 100U, 1U) == LXP_OK);
    grant.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    grant.scope.asset_id[0] = 3U;
    grant.scope.purpose_hash[0] = 4U;
    grant.scope.maximum_per_activity.lo = 2U;
    grant.scope.maximum_total.lo = 10U;
    FEE_CHECK(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    FEE_CHECK(lxp_grant_encode(&grant, &arena, &encoded) == LXP_OK);
    legacy_length = encoded.length;
    FEE_CHECK(legacy_length <= sizeof(encoded_v1) && encoded.bytes[4] == 1U);
    (void)memcpy(encoded_v1, encoded.bytes, legacy_length);
    FEE_CHECK(lxp_grant_id_compute(&grant, first_id) == LXP_OK);
    FEE_CHECK(lxp_grant_decode(encoded_v1, legacy_length, &decoded) == LXP_OK && !decoded.fee_budget.present);
    FEE_CHECK(lxp_grant_id_compute(&decoded, second_id) == LXP_OK && memcmp(first_id, second_id, 32U) == 0);
    grant.fee_budget.present = true;
    grant.fee_budget.asset_id[0] = 5U;
    grant.fee_budget.maximum_per_activity.lo = 4U;
    grant.fee_budget.maximum_total.lo = 12U;
    grant.fee_budget.period_length = 10U;
    grant.fee_budget.maximum_per_period.lo = 8U;
    grant.fee_budget.period_start = 10U;
    FEE_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
    FEE_CHECK(lxp_grant_encode(&grant, &arena, &encoded) == LXP_OK);
    length = encoded.length;
    FEE_CHECK(length == legacy_length + 132U && length <= sizeof(encoded_v2));
    (void)memcpy(encoded_v2, encoded.bytes, length);
    FEE_CHECK(encoded_v2[4] == 2U && memcmp(encoded_v1, encoded_v2, 4U) == 0 &&
        memcmp(encoded_v1 + 5U, encoded_v2 + 5U, legacy_length - 5U) == 0);
    FEE_CHECK(lxp_grant_decode(encoded_v2, length, &decoded) == LXP_OK && decoded.fee_budget.present);
    FEE_CHECK(lxp_grant_id_compute(&decoded, second_id) == LXP_OK && memcmp(first_id, second_id, 32U) != 0);
    for (size_t truncated = 0U; truncated < length; ++truncated)
        FEE_CHECK(lxp_grant_decode(encoded_v2, truncated, &decoded) != LXP_OK);
    encoded_v2[4] = 1U;
    FEE_CHECK(lxp_grant_decode(encoded_v2, length, &decoded) != LXP_OK);
    encoded_v2[4] = 3U;
    FEE_CHECK(lxp_grant_decode(encoded_v2, length, &decoded) == LXP_ERR_VERSION_UNSUPPORTED);
    encoded_v2[4] = 2U;
    narrower = grant;
    narrower.grantor_revocation_sequence++;
    narrower.fee_budget.maximum_total.lo++;
    FEE_CHECK(lxp_authority_amend(&grant, &narrower) == LXP_ERR_AUTH_SCOPE);
    original = grant.fee_budget;
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 5U}, 10U) == LXP_ERR_GRANT_EXHAUSTED);
    FEE_CHECK(memcmp(&grant.fee_budget, &original, sizeof(original)) == 0);
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 4U}, 10U) == LXP_OK);
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 4U}, 19U) == LXP_OK);
    original = grant.fee_budget;
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 1U}, 19U) == LXP_ERR_GRANT_EXHAUSTED);
    FEE_CHECK(memcmp(&grant.fee_budget, &original, sizeof(original)) == 0);
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 4U}, 20U) == LXP_OK);
    FEE_CHECK(grant.fee_budget.spent_total.lo == 12U && grant.fee_budget.spent_this_period.lo == 4U &&
        grant.fee_budget.period_start == 20U);
    original = grant.fee_budget;
    FEE_CHECK(lxp_authority_fee_charge(&grant.fee_budget, (lxp_u128){0U, 1U}, 21U) == LXP_ERR_GRANT_EXHAUSTED);
    FEE_CHECK(memcmp(&grant.fee_budget, &original, sizeof(original)) == 0);
    return 0;
}

int main(void)
{
    if (fee_grants() != 0) return 1;
    uint8_t storage[1024];
    uint8_t second_storage[1024];
    uint8_t grantor[32] = { 1U };
    uint8_t session_key[32] = { 2U };
    lxp_authority_grant grant;
    lxp_arena arena;
    lxp_arena second_arena;
    lxp_byte_span first;
    lxp_byte_span second;
    uint8_t first_id[32];
    uint8_t amended_id[32];
    if (lxp_session_key_bind(&grant, grantor, session_key, UINT64_C(3), 1U,
                             9U, 10U, 20U, 0U) != LXP_OK ||
        lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_grant_encode(&grant, &arena, &first) != LXP_OK ||
        lxp_grant_id_compute(&grant, first_id) != LXP_OK ||
        memcmp(first_id, grant.grant_id, 32U) != 0) return 1;
    if (lxp_arena_init(&second_arena, second_storage, sizeof(second_storage)) !=
        LXP_OK || lxp_grant_encode(&grant, &second_arena, &second) != LXP_OK ||
        first.length != second.length ||
        memcmp(first.bytes, second.bytes, first.length) != 0) return 1;
    grant.not_after = 19U;
    if (lxp_grant_id_compute(&grant, amended_id) != LXP_OK ||
        memcmp(first_id, amended_id, 32U) == 0) return 1;
    if (lxp_session_key_bind(&grant, grantor, session_key, 0U, 1U, 9U,
                             10U, 20U, 0U) != LXP_ERR_MALFORMED_GRANT ||
        lxp_session_key_bind(&grant, grantor, session_key, 1U, 9U, 1U,
                             10U, 20U, 0U) != LXP_ERR_MALFORMED_GRANT ||
        lxp_session_key_bind(&grant, grantor, session_key, 1U, 1U, 9U,
                             10U, 0U, 0U) != LXP_ERR_MALFORMED_GRANT)
        return 1;
    (void)memset(&grant, 0, sizeof(grant));
    grant.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    grant.not_after = 20U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_grant_encode(&grant, &arena, &first) != LXP_ERR_MALFORMED_GRANT)
        return 1;
    return 0;
}
