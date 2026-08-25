#include "layerx/lx_oracle.h"

#include <string.h>

lxp_result lx_oracle_staleness_check(const lx_oracle_market *market,
                                     const lx_oracle_observation *observation,
                                     uint64_t batch_timestamp)
{
    if (market == NULL || observation == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (observation->observed_at > batch_timestamp)
        return LXP_ERR_TIMESTAMP_REGRESSION;
    if (batch_timestamp - observation->observed_at >
        market->maximum_staleness) return LXP_ERR_ORACLE_STALE;
    return LXP_OK;
}

lxp_result lx_oracle_bounds_check(const lx_oracle_market *market,
                                  const lx_oracle_observation *observation)
{
    if (market == NULL || observation == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_cmp(observation->price, market->minimum_price) < 0 ||
        lxp_u128_cmp(observation->price, market->maximum_price) > 0)
        return LXP_ERR_ORACLE_BOUNDS;
    return LXP_OK;
}

lxp_result lx_oracle_deviation_check(
    const lx_oracle_market *market, const lx_oracle_observation *latest,
    const lx_oracle_observation *observation)
{
    lxp_u128 difference;
    lxp_u128 allowed;
    lxp_result status;
    if (market == NULL || observation == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (latest == NULL) return LXP_OK;
    if (lxp_u128_cmp(observation->price, latest->price) >= 0)
        status = lxp_u128_sub(observation->price, latest->price, &difference);
    else
        status = lxp_u128_sub(latest->price, observation->price, &difference);
    if (status == LXP_OK)
        status = lxp_u128_mul_bps_floor(
            latest->price, market->maximum_deviation_basis_points, &allowed);
    if (status != LXP_OK) return status;
    return lxp_u128_cmp(difference, allowed) <= 0 ?
        LXP_OK : LXP_ERR_ORACLE_DEVIATION;
}

lxp_result lx_oracle_store_latest(const lx_oracle_store *store,
                                  const uint8_t market_id[32],
                                  const lx_oracle_accepted **latest)
{
    size_t i;
    if (store == NULL || market_id == NULL || latest == NULL ||
        store->count > LX_ORACLE_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = store->count; i != 0U; --i)
        if (memcmp(store->accepted[i - 1U].observation.market_id,
                   market_id, 32U) == 0) {
            *latest = &store->accepted[i - 1U];
            return LXP_OK;
        }
    *latest = NULL;
    return LXP_ERR_UNKNOWN_FIELD;
}
