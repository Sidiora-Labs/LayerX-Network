#include "layerx/lx_stream.h"

#include <stdio.h>
#include <string.h>

#define CHECK(x) do { \
    if (!(x)) { (void)fprintf(stderr, "line %d\n", __LINE__); return 1; } \
} while (0)

static void record_time(lx_stream_record *record)
{
    (void)memset(record, 0, sizeof(*record));
    record->stream_id[0] = 4U;
    record->payer[0] = 1U;
    record->stream_account[0] = 2U;
    record->recipient[0] = 3U;
    record->asset_id[0] = 5U;
    record->mode = LX_STREAM_MODE_TIME;
    record->rate = (lxp_u128){ 0U, 3U };
    record->rate_unit = 1000U;
    record->start_timestamp = 100U;
    record->last_accrual_timestamp = 100U;
    record->end_timestamp = 1100U;
    record->total_cap = (lxp_u128){ 0U, 100U };
}

static void record_metered(lx_stream_record *record)
{
    size_t i;
    record_time(record);
    record->mode = LX_STREAM_MODE_METERED;
    record->end_timestamp = 0U;
    record->rate_unit = 2U;
    for (i = 0U; i < LX_STREAM_MAX_METER_AUTHORITIES; ++i)
        record->meter_authorities[i][0] = (uint8_t)(i + 1U);
    record->meter_authority_count = LX_STREAM_MAX_METER_AUTHORITIES;
}

static int replay(lx_stream_record *record)
{
    static const uint64_t timestamps[] = { 250U, 500U, 1000U, 1400U };
    size_t i;
    for (i = 0U; i < sizeof(timestamps) / sizeof(timestamps[0]); ++i) {
        lxp_u128 accrued;
        if (lx_stream_accrue(record, timestamps[i], &accrued) != LXP_OK)
            return 1;
    }
    return 0;
}

static int accrual_boundaries(void)
{
    lx_stream_record first;
    lx_stream_record second;
    lx_stream_record capped;
    lx_stream_record overflow;
    lx_stream_record guarded;
    lxp_u128 accrued;
    uint64_t elapsed;

    record_time(&first);
    second = first;
    CHECK(replay(&first) == 0);
    CHECK(replay(&second) == 0);
    CHECK(memcmp(&first, &second, sizeof(first)) == 0);
    CHECK(first.accrued_total.lo == 3U && first.accrued_total.hi == 0U);
    CHECK(first.remainder_carry.lo == 0U);
    CHECK(first.last_accrual_timestamp == 1100U);
    CHECK(lx_stream_elapsed_ms(&first, 1099U, &elapsed) ==
          LXP_ERR_NON_MONOTONIC_TIME);
    /* Past the end timestamp the stream is inert but not an error. */
    CHECK(lx_stream_elapsed_ms(&first, 4000U, &elapsed) == LXP_OK &&
          elapsed == 0U);
    CHECK(lx_stream_accrue(&first, 4000U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && first.accrued_total.lo == 3U &&
          first.last_accrual_timestamp == 1100U);

    /* Carry is exposed step by step, never rounded away. */
    record_time(&capped);
    CHECK(lx_stream_accrue(&capped, 250U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && capped.remainder_carry.lo == 450U &&
          capped.last_accrual_timestamp == 250U);
    CHECK(lx_stream_accrue(&capped, 500U, &accrued) == LXP_OK &&
          accrued.lo == 1U && capped.remainder_carry.lo == 200U);
    CHECK(lx_stream_carry_apply(&capped,
                                (lxp_u128){ 0U, capped.rate_unit }) ==
          LXP_FATAL_INVARIANT);
    CHECK(capped.remainder_carry.lo == 200U);
    CHECK(lx_stream_carry_apply(&capped,
                                (lxp_u128){ 0U, capped.rate_unit - 1U }) ==
          LXP_OK && capped.remainder_carry.lo == capped.rate_unit - 1U);

    /* The cap clamps the quotient and clears the residue with it. */
    record_time(&capped);
    capped.end_timestamp = 0U;
    capped.total_cap = (lxp_u128){ 0U, 2U };
    CHECK(lx_stream_accrue(&capped, 1100U, &accrued) == LXP_OK &&
          accrued.lo == 2U && capped.accrued_total.lo == 2U &&
          lxp_u128_is_zero(capped.remainder_carry) &&
          capped.last_accrual_timestamp == 1100U);
    CHECK(lx_stream_accrue(&capped, 9000U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && capped.accrued_total.lo == 2U &&
          capped.last_accrual_timestamp == 1100U);

    (void)memset(&overflow, 0, sizeof(overflow));
    overflow.mode = LX_STREAM_MODE_TIME;
    overflow.rate = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    overflow.rate_unit = 1U;
    overflow.last_accrual_timestamp = 1U;
    overflow.total_cap = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    CHECK(lx_stream_accrue(&overflow, UINT64_MAX, &accrued) ==
          LXP_ERR_ACCRUAL_OVERFLOW);
    CHECK(overflow.last_accrual_timestamp == 1U &&
          lxp_u128_is_zero(overflow.accrued_total));

    /* Suspended states hold the clock and the totals still. */
    record_time(&guarded);
    guarded.paused = true;
    CHECK(lx_stream_accrue(&guarded, 900U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && guarded.last_accrual_timestamp == 100U);
    guarded.paused = false;
    guarded.underfunded = true;
    CHECK(lx_stream_accrue(&guarded, 900U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && guarded.last_accrual_timestamp == 100U);
    guarded.underfunded = false;
    guarded.closed = true;
    CHECK(lx_stream_accrue(&guarded, 900U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && guarded.last_accrual_timestamp == 100U);
    guarded.closed = false;
    CHECK(lx_stream_accrue(&guarded, 100U, &accrued) == LXP_OK &&
          lxp_u128_is_zero(accrued) && guarded.last_accrual_timestamp == 100U);
    guarded.rate_unit = 0U;
    CHECK(lx_stream_accrue(&guarded, 900U, &accrued) == LXP_ERR_NON_CANONICAL);
    record_metered(&guarded);
    CHECK(lx_stream_accrue(&guarded, 900U, &accrued) == LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_accrue(NULL, 900U, &accrued) == LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_elapsed_ms(&guarded, 900U, NULL) ==
          LXP_ERR_NON_CANONICAL);
    return 0;
}

static int record_validation(void)
{
    lx_stream_record record;

    record_time(&record);
    CHECK(lx_stream_record_validate(&record) == LXP_OK);
    CHECK(lx_stream_record_validate(NULL) == LXP_ERR_NON_CANONICAL);

    record_time(&record);
    (void)memset(record.stream_id, 0, 32U);
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    (void)memcpy(record.stream_account, record.payer, 32U);
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    (void)memcpy(record.stream_account, record.recipient, 32U);
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    (void)memset(record.asset_id, 0, 32U);
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.rate = (lxp_u128){ 0U, 0U };
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.rate_unit = 0U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.start_timestamp = 0U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.last_accrual_timestamp = record.start_timestamp - 1U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.end_timestamp = record.start_timestamp;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.end_timestamp = 0U;
    CHECK(lx_stream_record_validate(&record) == LXP_OK);
    record_time(&record);
    record.total_cap = (lxp_u128){ 0U, 0U };
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.accrued_total = (lxp_u128){ 0U, 101U };
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.accrued_total = (lxp_u128){ 0U, 100U };
    CHECK(lx_stream_record_validate(&record) == LXP_OK);
    record.settled_total = (lxp_u128){ 0U, 101U };
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_time(&record);
    record.remainder_carry = (lxp_u128){ 0U, record.rate_unit };
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record.remainder_carry = (lxp_u128){ 0U, record.rate_unit - 1U };
    CHECK(lx_stream_record_validate(&record) == LXP_OK);
    record_time(&record);
    record.meter_authority_count = 1U;
    record.meter_authorities[0][0] = 9U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);

    record_metered(&record);
    CHECK(lx_stream_record_validate(&record) == LXP_OK);
    record.meter_authority_count = 0U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_metered(&record);
    record.meter_authority_count = LX_STREAM_MAX_METER_AUTHORITIES + 1U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_metered(&record);
    (void)memset(record.meter_authorities[3], 0, 32U);
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_metered(&record);
    record.meter_authorities[2][0] = record.meter_authorities[3][0];
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    record_metered(&record);
    record.meter_authority_count = 2U;
    CHECK(lx_stream_record_validate(&record) == LXP_ERR_NON_CANONICAL);
    return 0;
}

static int record_codec(void)
{
    lx_stream_record record;
    lx_stream_record decoded;
    uint8_t bytes[LX_STREAM_RECORD_BYTES];

    record_time(&record);
    record.accrued_total = (lxp_u128){ 0U, 40U };
    record.settled_total = (lxp_u128){ 0U, 25U };
    record.remainder_carry = (lxp_u128){ 0U, 999U };
    record.underfunded = true;
    record.paused = true;
    record.closed = true;
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_OK);
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) == LXP_OK);
    CHECK(memcmp(&record, &decoded, sizeof(record)) == 0);
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes) - 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes) + 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_record_decode(NULL, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_record_encode(&record, NULL) == LXP_ERR_NON_CANONICAL);

    bytes[160] = 0U;
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);
    bytes[160] = 3U;
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_OK);
    bytes[LX_STREAM_RECORD_BYTES - 3] = 2U;
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_OK);
    bytes[LX_STREAM_RECORD_BYTES - 1] = 2U;
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);

    record_metered(&record);
    record.cumulative_meter = 0x0102030405060708ULL;
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_OK);
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) == LXP_OK);
    CHECK(memcmp(&record, &decoded, sizeof(record)) == 0);
    CHECK(decoded.meter_authority_count == LX_STREAM_MAX_METER_AUTHORITIES);
    bytes[281] = (uint8_t)(LX_STREAM_MAX_METER_AUTHORITIES + 1U);
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);
    /* A shortened count leaves a non-zero tail the decoder must refuse. */
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_OK);
    bytes[281] = 4U;
    CHECK(lx_stream_record_decode(bytes, sizeof(bytes), &decoded) ==
          LXP_ERR_NON_CANONICAL);

    /* An invalid record never reaches the wire. */
    record_time(&record);
    record.rate = (lxp_u128){ 0U, 0U };
    CHECK(lx_stream_record_encode(&record, bytes) == LXP_ERR_NON_CANONICAL);
    return 0;
}

int main(void)
{
    if (accrual_boundaries() != 0) return 1;
    if (record_validation() != 0) return 1;
    if (record_codec() != 0) return 1;
    return 0;
}
