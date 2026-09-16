#ifndef LAYERX_LX_ORACLE_H
#define LAYERX_LX_ORACLE_H

#include "layerx/lxp_activity.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_result.h"
#include "layerx/lxp_u128.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LX_ORACLE_PUSH_ACTIVITY = 0x00060003,
    LX_ORACLE_OBSERVATION_BYTES = 72,
    LX_ORACLE_MAX_MARKETS = 128,
    LX_ORACLE_MAX_KEYS = 8,
    LX_ORACLE_STORE_CAPACITY = 512,
    LX_ORACLE_COMMITTED_BYTES = 64
};

typedef struct lx_oracle_observation {
    uint8_t market_id[32];
    uint64_t observation_sequence;
    lxp_u128 price;
    uint64_t observed_at;
    uint64_t source_identifier;
    uint8_t oracle_public_key[32];
    uint8_t signature[64];
} lx_oracle_observation;

typedef lxp_result (*lx_oracle_poll_fn)(void *context,
                                       lx_oracle_observation *observation,
                                       bool *available);
typedef lxp_result (*lx_oracle_submit_fn)(void *context,
                                         const uint8_t *activity,
                                         size_t activity_length);

typedef struct lx_oracle_adapter_config {
    lx_oracle_poll_fn poll_crossverse;
    void *poll_context;
    lx_oracle_submit_fn submit_activity;
    void *submit_context;
    uint8_t oracle_private_key[32];
    uint32_t network_id;
    const uint8_t *actor_did;
    size_t actor_did_length;
    uint64_t next_account_sequence;
    lxp_u128 fee_limit;
    size_t maximum_observations;
} lx_oracle_adapter_config;

typedef struct lx_oracle_market {
    uint8_t market_id[32];
    uint8_t permitted_keys[LX_ORACLE_MAX_KEYS][32];
    size_t permitted_key_count;
    uint64_t maximum_staleness;
    lxp_u128 minimum_price;
    lxp_u128 maximum_price;
    uint32_t maximum_deviation_basis_points;
    bool halted;
} lx_oracle_market;

typedef struct lx_oracle_market_store {
    lx_oracle_market markets[LX_ORACLE_MAX_MARKETS];
    size_t count;
} lx_oracle_market_store;

typedef struct lx_oracle_accepted {
    lx_oracle_observation observation;
    uint8_t payload[LX_ORACLE_OBSERVATION_BYTES];
    size_t payload_length;
    uint64_t global_sequence;
} lx_oracle_accepted;

typedef struct lx_oracle_store {
    lx_oracle_accepted accepted[LX_ORACLE_STORE_CAPACITY];
    size_t count;
} lx_oracle_store;

typedef struct lx_oracle_committed {
    uint8_t market_id[32];
    lxp_u128 price;
    uint64_t observed_at;
    uint64_t observation_sequence;
    uint8_t source_set_digest[32];
} lx_oracle_committed;

typedef struct lx_oracle_push_request {
    lx_oracle_store *store;
    lx_oracle_market_store *markets;
    const uint8_t *payload;
    size_t payload_length;
    const uint8_t *oracle_public_key;
    const uint8_t *signature;
    bool attempts_balance_mutation;
} lx_oracle_push_request;

typedef enum lx_oracle_market_action {
    LX_ORACLE_ACTION_ORDER_PLACE = 1,
    LX_ORACLE_ACTION_ORDER_CANCEL = 2,
    LX_ORACLE_ACTION_POSITION_INCREASE = 3,
    LX_ORACLE_ACTION_LIQUIDATE = 4,
    LX_ORACLE_ACTION_FUNDING_TICK = 5,
    LX_ORACLE_ACTION_MARGIN_ADD = 6
} lx_oracle_market_action;

lxp_result lx_oracle_observation_encode(
    const lx_oracle_observation *observation, uint8_t *bytes,
    size_t capacity, size_t *length);
lxp_result lx_oracle_observation_sign(lx_oracle_observation *observation,
                                      const uint8_t private_key[32]);
lxp_result lx_oracle_activity_encode(
    const lx_oracle_observation *observation, uint32_t network_id,
    const uint8_t *actor_did, size_t actor_did_length,
    uint64_t account_sequence, lxp_u128 fee_limit, lxp_arena *arena,
    lxp_byte_span *encoded);
lxp_result lx_oracle_adapter_run(lx_oracle_adapter_config *config,
                                 size_t *submitted);
lxp_result lx_oracle_adapter_isolation_check(void);
lxp_result lx_oracle_market_lookup(const lx_oracle_market_store *store,
                                   const uint8_t market_id[32],
                                   const lx_oracle_market **market);
lxp_result lx_oracle_observation_decode(
    const uint8_t *bytes, size_t length, const uint8_t public_key[32],
    const uint8_t signature[64], const lx_oracle_market_store *markets,
    lx_oracle_observation *observation);
lxp_result lx_oracle_key_set_check(const lx_oracle_market *market,
                                   const lx_oracle_observation *observation,
                                   const uint8_t *canonical_payload,
                                   size_t payload_length);
lxp_result lx_oracle_store_put(lx_oracle_store *store,
                               const lx_oracle_observation *observation,
                               const uint8_t *payload, size_t payload_length,
                               uint64_t global_sequence);
lxp_result lx_oracle_push_execute(lxp_module_ctx *ctx,
                                  const lx_oracle_push_request *request,
                                  lx_oracle_accepted *accepted);
lxp_result lx_oracle_staleness_check(const lx_oracle_market *market,
                                     const lx_oracle_observation *observation,
                                     uint64_t batch_timestamp);
lxp_result lx_oracle_bounds_check(const lx_oracle_market *market,
                                  const lx_oracle_observation *observation);
lxp_result lx_oracle_deviation_check(
    const lx_oracle_market *market, const lx_oracle_observation *latest,
    const lx_oracle_observation *observation);
lxp_result lx_oracle_store_latest(const lx_oracle_store *store,
                                  const uint8_t market_id[32],
                                  const lx_oracle_accepted **latest);
lxp_result lx_oracle_market_halt(lx_oracle_market *market);
bool lx_oracle_market_halted(const lx_oracle_market *market);
lxp_result lx_oracle_market_action_check(
    const lx_oracle_market *market, lx_oracle_market_action action);
lxp_result lx_oracle_fail_closed_eval(lx_oracle_market *market,
                                      const lx_oracle_store *store,
                                      uint64_t batch_timestamp);
lxp_result lx_oracle_source_set_digest(const uint8_t *permitted_keys,
                                       size_t permitted_key_count,
                                       uint8_t digest[32]);
lxp_result lx_oracle_committed_read(lxp_module_ctx *ctx,
                                    const uint8_t market_id[32],
                                    lx_oracle_committed *committed);
lxp_result lx_oracle_committed_encode(
    const lx_oracle_committed *committed,
    uint8_t bytes[LX_ORACLE_COMMITTED_BYTES]);

#endif
