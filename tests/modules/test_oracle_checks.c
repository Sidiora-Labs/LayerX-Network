#include "layerx/lx_oracle.h"

#include <string.h>

int main(void)
{
    lx_oracle_market market;
    lx_oracle_observation latest;
    lx_oracle_observation observation;
    lx_oracle_store store;
    const lx_oracle_accepted *found;
    uint8_t payload[LX_ORACLE_OBSERVATION_BYTES];
    size_t payload_length;

    (void)memset(&market, 0, sizeof(market));
    (void)memset(&latest, 0, sizeof(latest));
    (void)memset(&observation, 0, sizeof(observation));
    (void)memset(&store, 0, sizeof(store));
    market.market_id[0] = 1U;
    market.maximum_staleness = 100U;
    market.minimum_price = (lxp_u128){ 0U, 50U };
    market.maximum_price = (lxp_u128){ 0U, 200U };
    market.maximum_deviation_basis_points = 500U;
    latest.market_id[0] = 1U;
    latest.observation_sequence = 5U;
    latest.price = (lxp_u128){ 0U, 100U };
    latest.observed_at = 900U;
    latest.source_identifier = 1U;
    observation = latest;
    observation.observation_sequence = 6U;
    observation.observed_at = 950U;
    observation.price = (lxp_u128){ 0U, 105U };
    if (lx_oracle_staleness_check(&market, &observation, 1051U) !=
            LXP_ERR_ORACLE_STALE ||
        lx_oracle_staleness_check(&market, &observation, 1050U) != LXP_OK ||
        lx_oracle_bounds_check(&market, &observation) != LXP_OK ||
        lx_oracle_deviation_check(&market, &latest, &observation) != LXP_OK)
        return 1;
    observation.price = (lxp_u128){ 0U, 106U };
    if (lx_oracle_deviation_check(&market, &latest, &observation) !=
            LXP_ERR_ORACLE_DEVIATION)
        return 1;
    observation.price = (lxp_u128){ 0U, 201U };
    if (lx_oracle_bounds_check(&market, &observation) !=
            LXP_ERR_ORACLE_BOUNDS)
        return 1;
    latest.oracle_public_key[0] = 2U;
    if (lx_oracle_observation_encode(&latest, payload, sizeof(payload),
                                     &payload_length) != LXP_OK ||
        lx_oracle_store_put(&store, &latest, payload, payload_length, 10U) !=
            LXP_OK ||
        lx_oracle_store_latest(&store, latest.market_id, &found) != LXP_OK ||
        found->observation.observation_sequence != 5U)
        return 1;
    observation = latest;
    observation.observation_sequence = 5U;
    if (observation.observation_sequence >
        found->observation.observation_sequence)
        return 1;
    store.count = LX_ORACLE_STORE_CAPACITY + 1U;
    if (lx_oracle_store_latest(&store, latest.market_id, &found) !=
            LXP_ERR_NON_CANONICAL ||
        lx_oracle_store_put(&store, &latest, payload, payload_length, 11U) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    market.permitted_key_count = LX_ORACLE_MAX_KEYS + 1U;
    if (lx_oracle_key_set_check(&market, &latest, payload,
                                payload_length) != LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}
