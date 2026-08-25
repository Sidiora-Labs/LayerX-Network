#include "layerx/lx_oracle.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static uint64_t get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

lxp_result lx_oracle_market_lookup(const lx_oracle_market_store *store,
                                   const uint8_t market_id[32],
                                   const lx_oracle_market **market)
{
    size_t i;
    if (store == NULL || market_id == NULL || market == NULL ||
        store->count > LX_ORACLE_MAX_MARKETS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->markets[i].market_id, market_id, 32U) == 0) {
            *market = &store->markets[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_oracle_observation_decode(
    const uint8_t *bytes, size_t length, const uint8_t public_key[32],
    const uint8_t signature[64], const lx_oracle_market_store *markets,
    lx_oracle_observation *observation)
{
    const lx_oracle_market *market;
    uint8_t canonical[LX_ORACLE_OBSERVATION_BYTES];
    size_t canonical_length;
    lxp_result status;
    if (bytes == NULL || public_key == NULL || signature == NULL ||
        markets == NULL || observation == NULL ||
        length != LX_ORACLE_OBSERVATION_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(observation, 0, sizeof(*observation));
    (void)memcpy(observation->market_id, bytes, 32U);
    observation->observation_sequence = get_u64(bytes + 32U);
    status = lxp_u128_from_be(bytes + 40U, &observation->price);
    if (status != LXP_OK) return LXP_ERR_NON_CANONICAL;
    observation->observed_at = get_u64(bytes + 56U);
    observation->source_identifier = get_u64(bytes + 64U);
    (void)memcpy(observation->oracle_public_key, public_key, 32U);
    (void)memcpy(observation->signature, signature, 64U);
    status = lx_oracle_market_lookup(markets, observation->market_id, &market);
    if (status != LXP_OK) return status;
    status = lx_oracle_observation_encode(observation, canonical,
                                          sizeof(canonical),
                                          &canonical_length);
    if (status != LXP_OK || canonical_length != length ||
        memcmp(canonical, bytes, length) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_oracle_key_set_check(const lx_oracle_market *market,
                                   const lx_oracle_observation *observation,
                                   const uint8_t *canonical_payload,
                                   size_t payload_length)
{
    static const uint8_t tag[] = "LXP:ORACLE:OBSERVATION:v1";
    uint8_t message[sizeof(tag) - 1U + LX_ORACLE_OBSERVATION_BYTES];
    size_t i;
    bool permitted = false;
    if (market == NULL || observation == NULL || canonical_payload == NULL ||
        payload_length != LX_ORACLE_OBSERVATION_BYTES ||
        market->permitted_key_count > LX_ORACLE_MAX_KEYS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < market->permitted_key_count; ++i)
        if (memcmp(market->permitted_keys[i],
                   observation->oracle_public_key, 32U) == 0) {
            permitted = true;
            break;
        }
    if (!permitted) return LXP_ERR_UNAUTHORIZED_ORACLE;
    (void)memcpy(message, tag, sizeof(tag) - 1U);
    (void)memcpy(message + sizeof(tag) - 1U, canonical_payload,
                 payload_length);
    return lxp_ed25519_verify(observation->oracle_public_key,
                              observation->signature,
                              LXP_DOMAIN_SIGNATURE_PREIMAGE,
                              message, sizeof(message)) == LXP_OK ?
        LXP_OK : LXP_ERR_UNAUTHORIZED_ORACLE;
}

lxp_result lx_oracle_store_put(lx_oracle_store *store,
                               const lx_oracle_observation *observation,
                               const uint8_t *payload, size_t payload_length,
                               uint64_t global_sequence)
{
    lx_oracle_accepted *entry;
    if (store == NULL || observation == NULL || payload == NULL ||
        payload_length != LX_ORACLE_OBSERVATION_BYTES ||
        store->count > LX_ORACLE_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (store->count == LX_ORACLE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    entry = &store->accepted[store->count++];
    entry->observation = *observation;
    (void)memcpy(entry->payload, payload, payload_length);
    entry->payload_length = payload_length;
    entry->global_sequence = global_sequence;
    return LXP_OK;
}

lxp_result lx_oracle_push_execute(lxp_module_ctx *ctx,
                                  const lx_oracle_push_request *request,
                                  lx_oracle_accepted *accepted)
{
    lx_oracle_observation observation;
    const lx_oracle_market *market;
    lx_oracle_market *mutable_market;
    const lx_oracle_accepted *latest = NULL;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->markets == NULL || accepted == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_oracle_observation_decode(
        request->payload, request->payload_length,
        request->oracle_public_key, request->signature,
        request->markets, &observation);
    if (status != LXP_OK) return status;
    status = lx_oracle_market_lookup(request->markets,
                                     observation.market_id, &market);
    if (status != LXP_OK) return status;
    mutable_market = &request->markets->markets[
        (size_t)(market - request->markets->markets)];
    if (status == LXP_OK)
        status = lx_oracle_key_set_check(market, &observation,
                                         request->payload,
                                         request->payload_length);
    if (status == LXP_OK)
        status = lx_oracle_staleness_check(
            market, &observation, lxp_ctx_batch_timestamp_ms(ctx));
    if (status == LXP_OK)
        status = lx_oracle_bounds_check(market, &observation);
    if (status == LXP_OK &&
        lx_oracle_store_latest(request->store, observation.market_id,
                               &latest) == LXP_OK) {
        if (observation.observation_sequence <=
            latest->observation.observation_sequence)
            status = LXP_ERR_ORACLE_SEQUENCE;
        else
            status = lx_oracle_deviation_check(
                market, &latest->observation, &observation);
    }
    if (status != LXP_OK) {
        if (status == LXP_ERR_ORACLE_STALE ||
            status == LXP_ERR_ORACLE_SEQUENCE ||
            status == LXP_ERR_ORACLE_BOUNDS ||
            status == LXP_ERR_ORACLE_DEVIATION ||
            status == LXP_ERR_TIMESTAMP_REGRESSION)
            (void)lx_oracle_market_halt(mutable_market);
        return status;
    }
    status = lx_oracle_store_put(request->store, &observation,
                                 request->payload, request->payload_length,
                                 lxp_ctx_global_sequence(ctx));
    if (status != LXP_OK) return status;
    mutable_market->halted = false;
    *accepted = request->store->accepted[request->store->count - 1U];
    return LXP_OK;
}
