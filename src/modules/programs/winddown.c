#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_kernel.h"

#include <limits.h>
#include <string.h>

enum {
    WIND_DOWN_ROUTE = 1,
    WIND_DOWN_DEPRECATE = 2,
    WIND_DOWN_TOMBSTONE = 3,
    WIND_DOWN_EXIT = 4,
    PROGRAM_RECORD_BYTES = 71,
    PROGRAM_OWNER_RECORD_BYTES = 33,
    WIND_DOWN_STATUS_BYTES = 54,
    WIND_DOWN_ROUTE_FIXED_BYTES = 67,
    WIND_DOWN_HISTORY_BYTES = 119
};

static const uint8_t program_prefix[] = "program\0";
static const uint8_t program_owner_prefix[] = "program-owner\0";
static const uint8_t status_prefix[] = "wind-down\0s";
static const uint8_t route_prefix[] = "wind-down\0r";
static const uint8_t history_prefix[] = "wind-down\0h";

typedef struct programs_wind_down_activity {
    uint8_t program_id[32];
    uint8_t operation;
    uint8_t account_id[32];
    uint8_t asset_id[32];
    uint8_t destination[32];
    uint8_t exit_program[32];
    uint64_t deadline;
    const uint8_t *seed;
    uint16_t seed_length;
} programs_wind_down_activity;

typedef struct wind_down_inventory {
    lxp_module_ctx *ctx;
    uint16_t account_count;
    uint16_t live_count;
} wind_down_inventory;

typedef struct wind_down_settlement {
    lxp_module_ctx *ctx;
    const lxp_authority_resolved *authority;
    lx_programs_exit_route_view route;
    lx_account *source;
    lx_account *destination;
    uint64_t account_sequence;
    uint64_t program_spend_token;
    uint16_t seed_written;
    bool begun;
    bool applied;
    lxp_receipt receipt;
} wind_down_settlement;

static uint16_t read_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint64_t read_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index) value = (value << 8U) | bytes[index];
    return value;
}

static void write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static void program_key(const uint8_t program_id[32],
                        uint8_t key[sizeof(program_prefix) - 1U + 32U])
{
    (void)memcpy(key, program_prefix, sizeof(program_prefix) - 1U);
    (void)memcpy(key + sizeof(program_prefix) - 1U, program_id, 32U);
}

static void owner_key(const uint8_t program_id[32],
                      uint8_t key[sizeof(program_owner_prefix) - 1U + 32U])
{
    (void)memcpy(key, program_owner_prefix,
                 sizeof(program_owner_prefix) - 1U);
    (void)memcpy(key + sizeof(program_owner_prefix) - 1U, program_id, 32U);
}

static void status_key(const uint8_t program_id[32],
                       uint8_t key[sizeof(status_prefix) - 1U + 32U])
{
    (void)memcpy(key, status_prefix, sizeof(status_prefix) - 1U);
    (void)memcpy(key + sizeof(status_prefix) - 1U, program_id, 32U);
}

static void route_key(const uint8_t program_id[32],
                      const uint8_t account_id[32],
                      uint8_t key[sizeof(route_prefix) - 1U + 64U])
{
    (void)memcpy(key, route_prefix, sizeof(route_prefix) - 1U);
    (void)memcpy(key + sizeof(route_prefix) - 1U, program_id, 32U);
    (void)memcpy(key + sizeof(route_prefix) - 1U + 32U, account_id, 32U);
}

static void history_key(const uint8_t program_id[32], uint64_t sequence,
                        uint8_t key[sizeof(history_prefix) - 1U + 40U])
{
    (void)memcpy(key, history_prefix, sizeof(history_prefix) - 1U);
    (void)memcpy(key + sizeof(history_prefix) - 1U, program_id, 32U);
    write_u64(key + sizeof(history_prefix) - 1U + 32U, sequence);
}

lxp_result lxp_programs_program_abi(lxp_module_ctx *ctx,
                                    const uint8_t program_id[32],
                                    uint16_t *abi_version)
{
    uint8_t key[sizeof(program_prefix) - 1U + 32U];
    const uint8_t *record;
    size_t record_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || abi_version == NULL ||
        lxp_ct_is_zero(program_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    program_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    if (status != LXP_OK) return status;
    if (record_length != PROGRAM_RECORD_BYTES ||
        (record[0] != 0U && record[0] != 1U))
        return LXP_FATAL_INVARIANT;
    *abi_version = read_u16(record + 65U);
    return LXP_OK;
}

static lxp_result owner_authorized(lxp_module_ctx *ctx,
                                   const uint8_t program_id[32],
                                   const uint8_t principal[32])
{
    uint8_t key[sizeof(program_owner_prefix) - 1U + 32U];
    const uint8_t *record;
    size_t record_length;
    lxp_result status;
    if (principal == NULL || lxp_ct_is_zero(principal, 32U))
        return LXP_ERR_AUTH_SCOPE;
    owner_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    if (status != LXP_OK) return status;
    if (record_length != PROGRAM_OWNER_RECORD_BYTES || record[0] != 1U ||
        lxp_ct_is_zero(record + 1U, 32U))
        return LXP_FATAL_INVARIANT;
    return lxp_ct_memcmp(record + 1U, principal, 32U) == 0 ?
        LXP_OK : LXP_ERR_AUTH_SCOPE;
}

static lxp_result status_decode(const uint8_t program_id[32],
                                const uint8_t *record, size_t record_length,
                                lx_programs_wind_down_view *view)
{
    if (program_id == NULL || record == NULL || view == NULL ||
        record_length != WIND_DOWN_STATUS_BYTES || record[0] != 1U ||
        (record[1] != LX_PROGRAMS_LIFECYCLE_DEPRECATED &&
         record[1] != LX_PROGRAMS_LIFECYCLE_TOMBSTONED) ||
        lxp_ct_memcmp(record + 2U, program_id, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    (void)memset(view, 0, sizeof(*view));
    (void)memcpy(view->program_id, program_id, 32U);
    view->status = (lx_programs_lifecycle_status)record[1];
    (void)memcpy(view->exit_program, record + 2U, 32U);
    view->deadline = read_u64(record + 34U);
    view->effective_sequence = read_u64(record + 42U);
    view->value_account_count = read_u16(record + 50U);
    view->live_value_account_count = read_u16(record + 52U);
    return view->deadline != 0U && view->effective_sequence != 0U ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_programs_wind_down_read(lxp_module_ctx *ctx,
                                       const uint8_t program_id[32],
                                       lx_programs_wind_down_view *view)
{
    uint8_t key[sizeof(status_prefix) - 1U + 32U];
    const uint8_t *record;
    size_t record_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || view == NULL)
        return LXP_ERR_NON_CANONICAL;
    status_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    return status == LXP_OK ?
        status_decode(program_id, record, record_length, view) : status;
}

lxp_result lxp_programs_program_active(lxp_module_ctx *ctx,
                                       const uint8_t program_id[32])
{
    lx_programs_wind_down_view view;
    lxp_result status = lxp_programs_wind_down_read(ctx, program_id, &view);
    return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK :
        status == LXP_OK ? LXP_ERR_PROGRAM_REFUSED : status;
}

static lxp_result route_decode(const uint8_t program_id[32],
                               const uint8_t account_id[32],
                               const uint8_t *record, size_t record_length,
                               lx_programs_exit_route_view *route)
{
    uint16_t seed_length;
    uint8_t derived[32];
    lxp_result status;
    if (program_id == NULL || account_id == NULL || record == NULL ||
        route == NULL || record_length < WIND_DOWN_ROUTE_FIXED_BYTES ||
        record[0] != 1U)
        return LXP_FATAL_INVARIANT;
    seed_length = read_u16(record + 65U);
    if (seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        record_length != (size_t)WIND_DOWN_ROUTE_FIXED_BYTES +
                         (size_t)seed_length)
        return LXP_FATAL_INVARIANT;
    (void)memset(route, 0, sizeof(*route));
    (void)memcpy(route->program_id, program_id, 32U);
    (void)memcpy(route->account_id, account_id, 32U);
    (void)memcpy(route->asset_id, record + 1U, 32U);
    (void)memcpy(route->destination, record + 33U, 32U);
    route->seed_length = seed_length;
    (void)memcpy(route->seed, record + 67U, seed_length);
    status = lxp_programs_account_derive(program_id, route->seed,
                                         route->seed_length, derived);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(derived, account_id, 32U) == 0 ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_programs_exit_route_read(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t account_id[32], lx_programs_exit_route_view *route)
{
    uint8_t key[sizeof(route_prefix) - 1U + 64U];
    const uint8_t *record;
    size_t record_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || account_id == NULL ||
        route == NULL)
        return LXP_ERR_NON_CANONICAL;
    route_key(program_id, account_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    return status == LXP_OK ? route_decode(program_id, account_id, record,
                                             record_length, route) : status;
}

typedef struct exit_route_iter_state {
    const uint8_t *program_id;
    lx_programs_exit_route_visit_fn visit;
    void *user;
} exit_route_iter_state;

static lxp_result visit_route(const uint8_t *key, size_t key_length,
                              const uint8_t *value, size_t value_length,
                              void *user)
{
    exit_route_iter_state *state = (exit_route_iter_state *)user;
    lx_programs_exit_route_view route;
    size_t expected = sizeof(route_prefix) - 1U + 64U;
    lxp_result status;
    if (key_length != expected ||
        lxp_ct_memcmp(key + sizeof(route_prefix) - 1U,
                      state->program_id, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    status = route_decode(state->program_id,
                          key + sizeof(route_prefix) - 1U + 32U,
                          value, value_length, &route);
    return status == LXP_OK ? state->visit(&route, state->user) : status;
}

lxp_result lxp_programs_exit_route_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_exit_route_visit_fn visit, void *user)
{
    uint8_t prefix[sizeof(route_prefix) - 1U + 32U];
    exit_route_iter_state state;
    if (ctx == NULL || program_id == NULL || visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(prefix, route_prefix, sizeof(route_prefix) - 1U);
    (void)memcpy(prefix + sizeof(route_prefix) - 1U, program_id, 32U);
    state.program_id = program_id;
    state.visit = visit;
    state.user = user;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_route, &state);
}

typedef struct wind_down_history_iter_state {
    const uint8_t *program_id;
    lx_programs_wind_down_history_visit_fn visit;
    void *user;
} wind_down_history_iter_state;

static lxp_result visit_history(const uint8_t *key, size_t key_length,
                                const uint8_t *value, size_t value_length,
                                void *user)
{
    wind_down_history_iter_state *state =
        (wind_down_history_iter_state *)user;
    lx_programs_wind_down_history_view history;
    size_t prefix_length = sizeof(history_prefix) - 1U;
    if (state == NULL || key == NULL || value == NULL ||
        key_length != prefix_length + 40U ||
        value_length != WIND_DOWN_HISTORY_BYTES || value[0] != 1U ||
        lxp_ct_memcmp(key + prefix_length, state->program_id, 32U) != 0 ||
        read_u64(key + prefix_length + 32U) != read_u64(value + 75U) ||
        (value[1] != LX_PROGRAMS_LIFECYCLE_ACTIVE &&
         value[1] != LX_PROGRAMS_LIFECYCLE_DEPRECATED) ||
        (value[2] != LX_PROGRAMS_LIFECYCLE_DEPRECATED &&
         value[2] != LX_PROGRAMS_LIFECYCLE_TOMBSTONED) ||
        lxp_ct_memcmp(value + 35U, state->program_id, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    (void)memset(&history, 0, sizeof(history));
    (void)memcpy(history.program_id, state->program_id, 32U);
    history.prior = (lx_programs_lifecycle_status)value[1];
    history.current = (lx_programs_lifecycle_status)value[2];
    (void)memcpy(history.authority, value + 3U, 32U);
    (void)memcpy(history.exit_program, value + 35U, 32U);
    history.deadline = read_u64(value + 67U);
    history.effective_sequence = read_u64(value + 75U);
    history.value_account_count = read_u16(value + 83U);
    history.live_value_account_count = read_u16(value + 85U);
    (void)memcpy(history.account_root, value + 87U, 32U);
    return state->visit(&history, state->user);
}

lxp_result lxp_programs_wind_down_history_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_wind_down_history_visit_fn visit, void *user)
{
    uint8_t prefix[sizeof(history_prefix) - 1U + 32U];
    wind_down_history_iter_state state;
    if (ctx == NULL || program_id == NULL || visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(prefix, history_prefix, sizeof(history_prefix) - 1U);
    (void)memcpy(prefix + sizeof(history_prefix) - 1U, program_id, 32U);
    state.program_id = program_id;
    state.visit = visit;
    state.user = user;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_history, &state);
}

static lxp_result route_live_validate(lxp_module_ctx *ctx,
                                      const lx_programs_account_binding *binding,
                                      const lx_programs_exit_route_view *route,
                                      lx_account **source, lx_account **destination)
{
    lx_programs_account_binding indexed;
    lx_account *account;
    lx_account *target;
    lxp_result status;
    if (ctx == NULL || binding == NULL || route == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_account_lookup_id(ctx, binding->account_id,
                                            &indexed, &account);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, route->destination, &target);
    if (status != LXP_OK) return status;
    if (binding->record_version != 2U ||
        binding->registered_sequence == 0U ||
        lxp_ct_is_zero(binding->registration_event_digest, 32U) ||
        indexed.record_version != binding->record_version ||
        indexed.registered_sequence != binding->registered_sequence ||
        lxp_ct_memcmp(indexed.registration_event_digest,
                      binding->registration_event_digest, 32U) != 0 ||
        lxp_ct_memcmp(binding->program_id, route->program_id, 32U) != 0 ||
        lxp_ct_memcmp(binding->account_id, route->account_id, 32U) != 0 ||
        lxp_ct_memcmp(binding->asset_id, route->asset_id, 32U) != 0 ||
        binding->seed_length != route->seed_length ||
        memcmp(binding->seed, route->seed, binding->seed_length) != 0 ||
        lxp_ct_memcmp(account->asset_id, binding->asset_id, 32U) != 0 ||
        !account->has_asset || account->kind != LX_ACCOUNT_MODULE_VALUE ||
        account->has_authority_key ||
        account->created_at_sequence != binding->registered_sequence ||
        lxp_ct_memcmp(account->id, target->id, 32U) == 0 ||
        !target->has_asset ||
        lxp_ct_memcmp(target->asset_id, binding->asset_id, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if ((!lxp_u128_is_zero(account->balance) && account->frozen) ||
        target->frozen)
        return LXP_ERR_ACCOUNT_FROZEN;
    if (source != NULL) *source = account;
    if (destination != NULL) *destination = target;
    return LXP_OK;
}

static bool words_match(const uint8_t bytes[32], uint64_t w0, uint64_t w1,
                        uint64_t w2, uint64_t w3)
{
    return read_u64(bytes) == w0 && read_u64(bytes + 8U) == w1 &&
           read_u64(bytes + 16U) == w2 && read_u64(bytes + 24U) == w3;
}

static lxp_result inventory_visit(const lx_programs_account_binding *binding,
                                  void *user)
{
    wind_down_inventory *inventory = (wind_down_inventory *)user;
    lx_programs_exit_route_view route;
    lx_account *source;
    lxp_result status;
    if (inventory->account_count == UINT16_MAX) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_programs_exit_route_read(inventory->ctx,
                                          binding->program_id,
                                          binding->account_id, &route);
    if (status == LXP_OK)
        status = route_live_validate(inventory->ctx, binding, &route,
                                     &source, NULL);
    if (status != LXP_OK) return status;
    ++inventory->account_count;
    if (!lxp_u128_is_zero(source->balance)) {
        if (inventory->live_count == UINT16_MAX) return LXP_ERR_LENGTH_LIMIT;
        ++inventory->live_count;
    }
    return LXP_OK;
}

static lxp_result inventory_read(lxp_module_ctx *ctx,
                                 const uint8_t program_id[32],
                                 wind_down_inventory *inventory)
{
    if (ctx == NULL || program_id == NULL || inventory == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(inventory, 0, sizeof(*inventory));
    inventory->ctx = ctx;
    return lxp_programs_account_iter(ctx, program_id, inventory_visit,
                                     inventory);
}

lxp_result lxp_programs_wind_down_decode(lxp_module_ctx *ctx,
                                         const uint8_t *payload,
                                         size_t payload_length,
                                         void **decoded)
{
    programs_wind_down_activity *value;
    void *allocation;
    uint16_t seed_length;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL ||
        payload_length < 33U)
        return LXP_ERR_TRUNCATED;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_wind_down_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (programs_wind_down_activity *)allocation;
    (void)memset(value, 0, sizeof(*value));
    (void)memcpy(value->program_id, payload, 32U);
    value->operation = payload[32U];
    if (lxp_ct_is_zero(value->program_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (value->operation == WIND_DOWN_ROUTE) {
        if (payload_length < 131U) return LXP_ERR_TRUNCATED;
        seed_length = read_u16(payload + 129U);
        if (seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
            payload_length != 131U + seed_length)
            return LXP_ERR_LENGTH_LIMIT;
        (void)memcpy(value->account_id, payload + 33U, 32U);
        (void)memcpy(value->asset_id, payload + 65U, 32U);
        (void)memcpy(value->destination, payload + 97U, 32U);
        value->seed_length = seed_length;
        value->seed = payload + 131U;
    } else if (value->operation == WIND_DOWN_DEPRECATE) {
        if (payload_length != 73U) return LXP_ERR_NON_CANONICAL;
        (void)memcpy(value->exit_program, payload + 33U, 32U);
        value->deadline = read_u64(payload + 65U);
    } else if (value->operation == WIND_DOWN_TOMBSTONE) {
        if (payload_length != 33U) return LXP_ERR_NON_CANONICAL;
    } else if (value->operation == WIND_DOWN_EXIT) {
        if (payload_length != 65U) return LXP_ERR_NON_CANONICAL;
        (void)memcpy(value->account_id, payload + 33U, 32U);
    } else {
        return LXP_ERR_UNKNOWN_FIELD;
    }
    *decoded = value;
    return LXP_OK;
}

static lxp_result route_request_validate(
    lxp_module_ctx *ctx, const programs_wind_down_activity *value)
{
    lx_programs_account_binding binding;
    lx_programs_exit_route_view route;
    lx_account *account;
    lxp_result status;
    status = lxp_programs_account_lookup_id(ctx, value->account_id,
                                            &binding, &account);
    if (status != LXP_OK) return status;
    (void)memset(&route, 0, sizeof(route));
    (void)memcpy(route.program_id, value->program_id, 32U);
    (void)memcpy(route.account_id, value->account_id, 32U);
    (void)memcpy(route.asset_id, value->asset_id, 32U);
    (void)memcpy(route.destination, value->destination, 32U);
    route.seed_length = value->seed_length;
    (void)memcpy(route.seed, value->seed, value->seed_length);
    return route_live_validate(ctx, &binding, &route, NULL, NULL);
}

static lxp_result exit_accounts(
    lxp_module_ctx *ctx, const programs_wind_down_activity *value,
    lx_programs_exit_route_view *route, lx_account **source,
    lx_account **destination)
{
    lx_programs_account_binding binding;
    lxp_result status = lxp_programs_exit_route_read(
        ctx, value->program_id, value->account_id, route);
    if (status == LXP_OK)
        status = lxp_programs_account_lookup_id(ctx, value->account_id,
                                                &binding, source);
    if (status == LXP_OK)
        status = route_live_validate(ctx, &binding, route, source,
                                     destination);
    if (status != LXP_OK) return status;
    return lxp_u128_is_zero((*source)->balance) ? LXP_ERR_ZERO_AMOUNT : LXP_OK;
}

lxp_result lxp_programs_wind_down_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_wind_down_activity *value =
        (const programs_wind_down_activity *)decoded;
    lx_programs_wind_down_view current;
    wind_down_inventory inventory;
    uint16_t abi_version;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        !lxp_protocol_version_uses_occupancy(ctx->protocol_version))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_program_abi(ctx, value->program_id, &abi_version);
    if (status != LXP_OK) return status;
    if (abi_version != LX_PROGRAMS_ACCOUNT_ABI_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_programs_wind_down_read(ctx, value->program_id, &current);
    if (value->operation == WIND_DOWN_ROUTE) {
        if (status != LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_PROGRAM_REFUSED;
        status = owner_authorized(ctx, value->program_id, authority->principal);
        if (status == LXP_OK) status = route_request_validate(ctx, value);
    } else if (value->operation == WIND_DOWN_DEPRECATE) {
        if (status != LXP_ERR_UNKNOWN_FIELD ||
            lxp_ct_memcmp(value->exit_program, value->program_id, 32U) != 0 ||
            value->deadline <= lxp_ctx_global_sequence(ctx))
            return LXP_ERR_PROGRAM_REFUSED;
        status = owner_authorized(ctx, value->program_id, authority->principal);
        if (status == LXP_OK)
            status = inventory_read(ctx, value->program_id, &inventory);
    } else if (value->operation == WIND_DOWN_TOMBSTONE) {
        if (status != LXP_OK ||
            current.status != LX_PROGRAMS_LIFECYCLE_DEPRECATED)
            return LXP_ERR_PROGRAM_REFUSED;
        status = owner_authorized(ctx, value->program_id, authority->principal);
        if (status == LXP_OK)
            status = inventory_read(ctx, value->program_id, &inventory);
    } else if (value->operation == WIND_DOWN_EXIT) {
        lx_programs_exit_route_view route;
        lx_account *source;
        lx_account *destination;
        if (status != LXP_OK ||
            (current.status != LX_PROGRAMS_LIFECYCLE_DEPRECATED &&
             current.status != LX_PROGRAMS_LIFECYCLE_TOMBSTONED))
            return LXP_ERR_PROGRAM_REFUSED;
        status = owner_authorized(ctx, value->program_id,
                                  authority->principal);
        if (status == LXP_OK)
            status = exit_accounts(ctx, value, &route, &source,
                                   &destination);
    } else {
        status = LXP_ERR_UNKNOWN_FIELD;
    }
    return status == LXP_OK ? lxp_ctx_charge_gas(ctx, 256U) : status;
}

static lxp_result route_store(lxp_module_ctx *ctx,
                              const programs_wind_down_activity *value)
{
    uint8_t key[sizeof(route_prefix) - 1U + 64U];
    uint8_t record[WIND_DOWN_ROUTE_FIXED_BYTES +
                   LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    uint8_t event[128];
    size_t record_length = WIND_DOWN_ROUTE_FIXED_BYTES + value->seed_length;
    route_key(value->program_id, value->account_id, key);
    record[0] = 1U;
    (void)memcpy(record + 1U, value->asset_id, 32U);
    (void)memcpy(record + 33U, value->destination, 32U);
    write_u16(record + 65U, value->seed_length);
    (void)memcpy(record + 67U, value->seed, value->seed_length);
    (void)memcpy(event, value->program_id, 32U);
    (void)memcpy(event + 32U, value->account_id, 32U);
    (void)memcpy(event + 64U, value->asset_id, 32U);
    (void)memcpy(event + 96U, value->destination, 32U);
    {
        lxp_result status = lxp_ctx_kv_put(ctx, key, sizeof(key), record,
                                           record_length);
        return status == LXP_OK ?
            lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_EXIT_ROUTE,
                               event, sizeof(event)) : status;
    }
}

static lxp_result transition_store(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const programs_wind_down_activity *value,
    lx_programs_lifecycle_status prior,
    lx_programs_lifecycle_status target, const uint8_t exit_program[32],
    uint64_t deadline)
{
    wind_down_inventory inventory;
    uint8_t status_record[WIND_DOWN_STATUS_BYTES];
    uint8_t history_record[WIND_DOWN_HISTORY_BYTES];
    uint8_t status_name[sizeof(status_prefix) - 1U + 32U];
    uint8_t history_name[sizeof(history_prefix) - 1U + 40U];
    uint8_t account_root[32];
    uint64_t sequence = lxp_ctx_global_sequence(ctx);
    lx_programs_transfer_runtime *runtime;
    lxp_result status = inventory_read(ctx, value->program_id, &inventory);
    if (status != LXP_OK) return status;
    runtime = (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL)
        return LXP_ERR_MODULE_DISABLED;
    status = lx_account_registry_root(runtime->accounts, account_root);
    if (status != LXP_OK) return status;
    status_record[0] = 1U;
    status_record[1] = (uint8_t)target;
    (void)memcpy(status_record + 2U, exit_program, 32U);
    write_u64(status_record + 34U, deadline);
    write_u64(status_record + 42U, sequence);
    write_u16(status_record + 50U, inventory.account_count);
    write_u16(status_record + 52U, inventory.live_count);
    history_record[0] = 1U;
    history_record[1] = (uint8_t)prior;
    history_record[2] = (uint8_t)target;
    (void)memcpy(history_record + 3U, authority->principal, 32U);
    (void)memcpy(history_record + 35U, exit_program, 32U);
    write_u64(history_record + 67U, deadline);
    write_u64(history_record + 75U, sequence);
    write_u16(history_record + 83U, inventory.account_count);
    write_u16(history_record + 85U, inventory.live_count);
    (void)memcpy(history_record + 87U, account_root, 32U);
    status_key(value->program_id, status_name);
    history_key(value->program_id, sequence, history_name);
    status = lxp_ctx_kv_put(ctx, history_name, sizeof(history_name),
                            history_record, sizeof(history_record));
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, status_name, sizeof(status_name),
                                status_record, sizeof(status_record));
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(
        ctx, target == LX_PROGRAMS_LIFECYCLE_DEPRECATED ?
            LX_PROGRAMS_EVENT_DEPRECATED : LX_PROGRAMS_EVENT_TOMBSTONED,
        history_record, sizeof(history_record));
}

static lx_account *account_by_id(lx_account_registry *accounts,
                                 const uint8_t account_id[32])
{
    size_t index;
    if (accounts == NULL) return NULL;
    for (index = 0U; index < accounts->count; ++index)
        if (lxp_ct_memcmp(accounts->accounts[index].id, account_id, 32U) == 0)
            return &accounts->accounts[index];
    return NULL;
}

static lxp_result status_live_refresh(lxp_module_ctx *ctx,
                                      const uint8_t program_id[32])
{
    wind_down_inventory inventory;
    uint8_t key[sizeof(status_prefix) - 1U + 32U];
    uint8_t updated[WIND_DOWN_STATUS_BYTES];
    const uint8_t *record;
    size_t record_length;
    lxp_result status = inventory_read(ctx, program_id, &inventory);
    if (status != LXP_OK) return status;
    status_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    if (status != LXP_OK) return status;
    if (record_length != sizeof(updated)) return LXP_FATAL_INVARIANT;
    (void)memcpy(updated, record, sizeof(updated));
    write_u16(updated + 50U, inventory.account_count);
    write_u16(updated + 52U, inventory.live_count);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), updated, sizeof(updated));
}

lxp_result layerx_programs_wind_down_transfer_begin(
    uint64_t token, uint64_t program_spend_token, uint8_t source_kind,
    uint64_t f0, uint64_t f1, uint64_t f2, uint64_t f3,
    uint64_t o0, uint64_t o1, uint64_t o2, uint64_t o3,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t frame_path, uint8_t frame_depth, uint16_t seed_length,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t amount_hi, uint64_t amount_lo)
{
    wind_down_settlement *settlement =
        (wind_down_settlement *)(uintptr_t)token;
    if (settlement == NULL || settlement->ctx == NULL ||
        settlement->authority == NULL || settlement->source == NULL ||
        settlement->destination == NULL || settlement->begun ||
        settlement->applied || program_spend_token == 0U ||
        source_kind != 2U || frame_path != 0U ||
        frame_depth != 0U || seed_length != settlement->route.seed_length ||
        !words_match(settlement->source->id, f0, f1, f2, f3) ||
        !words_match(settlement->route.program_id, o0, o1, o2, o3) ||
        !words_match(settlement->route.program_id, p0, p1, p2, p3) ||
        !words_match(settlement->destination->id, t0, t1, t2, t3) ||
        !words_match(settlement->route.asset_id, a0, a1, a2, a3) ||
        settlement->source->balance.hi != amount_hi ||
        settlement->source->balance.lo != amount_lo ||
        lxp_u128_is_zero(settlement->source->balance))
        return LXP_ERR_AUTH_SCOPE;
    settlement->program_spend_token = program_spend_token;
    settlement->begun = true;
    settlement->seed_written = 0U;
    return LXP_OK;
}

lxp_result layerx_programs_wind_down_transfer_seed_byte(
    uint64_t token, uint16_t offset, uint8_t byte)
{
    wind_down_settlement *settlement =
        (wind_down_settlement *)(uintptr_t)token;
    if (settlement == NULL || !settlement->begun || settlement->applied ||
        offset != settlement->seed_written ||
        offset >= settlement->route.seed_length ||
        settlement->route.seed[offset] != byte)
        return LXP_ERR_AUTH_SCOPE;
    ++settlement->seed_written;
    return LXP_OK;
}

lxp_result layerx_programs_wind_down_transfer_apply(uint64_t token)
{
    wind_down_settlement *settlement =
        (wind_down_settlement *)(uintptr_t)token;
    lx_programs_transfer_runtime *runtime;
    lxp_transfer_source_authority source_authority;
    lxp_transfer_set set;
    lx_account *sequence_account;
    lxp_result status;
    if (settlement == NULL || !settlement->begun || settlement->applied ||
        settlement->seed_written != settlement->route.seed_length)
        return LXP_ERR_AUTH_SCOPE;
    runtime = (lx_programs_transfer_runtime *)
        lxp_ctx_module_runtime(settlement->ctx);
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL)
        return LXP_ERR_MODULE_DISABLED;
    sequence_account = account_by_id(runtime->accounts,
                                     settlement->authority->principal);
    if (sequence_account == NULL ||
        sequence_account->kind != LX_ACCOUNT_AGENT_MAIN ||
        settlement->account_sequence != sequence_account->next_sequence)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = settlement->source;
    set.legs[0].to = settlement->destination;
    (void)memcpy(set.legs[0].asset_id, settlement->route.asset_id, 32U);
    set.legs[0].amount = settlement->source->balance;
    set.legs[0].reason = LXP_REASON_PAYMENT;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    (void)memset(&source_authority, 0, sizeof(source_authority));
    (void)memcpy(source_authority.authorized_from,
                 settlement->source->id, 32U);
    source_authority.debit_authority_kind = LXP_AUTH_PROGRAM_SPEND;
    source_authority.protocol_system_capability = false;
    set.context.assets = runtime->assets;
    set.context.asset_count = runtime->asset_count;
    (void)memcpy(set.context.authorized_from,
                 settlement->authority->principal, 32U);
    set.context.actor_sequence = settlement->account_sequence;
    set.context.batch_timestamp =
        lxp_ctx_batch_timestamp_ms(settlement->ctx);
    set.context.sequence_account = sequence_account;
    set.context.origin_module_id = LXP_MODULE_PROGRAMS;
    set.context.debit_authority_kind = LXP_AUTH_PROGRAM_SPEND;
    set.context.source_authorities = &source_authority;
    set.context.source_authority_count = 1U;
    set.context.program_spend_token = settlement->program_spend_token;
    (void)memset(&settlement->receipt, 0, sizeof(settlement->receipt));
    status = lxp_ctx_emit_transfer_set(settlement->ctx, &set,
                                       &settlement->receipt);
    if (status == LXP_OK)
        status = status_live_refresh(settlement->ctx,
                                     settlement->route.program_id);
    if (status == LXP_OK) settlement->applied = true;
    return status;
}

lxp_result layerx_programs_wind_down_transfer_root_byte(uint64_t token,
                                                         uint32_t offset)
{
    const wind_down_settlement *settlement =
        (const wind_down_settlement *)(uintptr_t)token;
    if (settlement == NULL || !settlement->applied || offset >= 32U ||
        lxp_ct_is_zero(settlement->receipt.transfer_set_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    return settlement->receipt.transfer_set_root[offset];
}

static lxp_result exit_execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                               const lxp_authority_resolved *authority,
                               const programs_wind_down_activity *value)
{
    lx_programs_exit_route_view route;
    wind_down_settlement settlement;
    lx_account *source;
    lx_account *destination;
    uint64_t program[4], principal[4], authority_hash[4];
    uint64_t account[4], asset[4], target[4];
    uint8_t event[96];
    size_t index;
    lxp_result status = exit_accounts(ctx, value, &route, &source,
                                      &destination);
    if (status != LXP_OK) return status;
    (void)memset(&settlement, 0, sizeof(settlement));
    settlement.ctx = ctx;
    settlement.authority = authority;
    settlement.route = route;
    settlement.source = source;
    settlement.destination = destination;
    settlement.account_sequence = activity->account_sequence;
    for (index = 0U; index < 4U; ++index) {
        program[index] = read_u64(route.program_id + index * 8U);
        principal[index] = read_u64(authority->principal + index * 8U);
        authority_hash[index] = read_u64(authority->authority_hash + index * 8U);
        account[index] = read_u64(route.account_id + index * 8U);
        asset[index] = read_u64(route.asset_id + index * 8U);
        target[index] = read_u64(route.destination + index * 8U);
    }
    status = layerx_programs_settle_wind_down_402lxp_leg(
        (uint64_t)(uintptr_t)&settlement,
        program[0], program[1], program[2], program[3],
        principal[0], principal[1], principal[2], principal[3],
        authority_hash[0], authority_hash[1], authority_hash[2],
        authority_hash[3], route.seed, route.seed_length,
        account[0], account[1], account[2], account[3],
        asset[0], asset[1], asset[2], asset[3],
        target[0], target[1], target[2], target[3],
        source->balance.hi, source->balance.lo);
    if (status != LXP_OK) return status;
    (void)memcpy(event, value->program_id, 32U);
    (void)memcpy(event + 32U, value->account_id, 32U);
    (void)memcpy(event + 64U, settlement.receipt.transfer_set_root, 32U);
    return lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_VALUE_EXITED,
                              event, sizeof(event));
}

lxp_result lxp_programs_wind_down_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    const programs_wind_down_activity *value =
        (const programs_wind_down_activity *)decoded;
    (void)activity;
    (void)effects;
    if (ctx == NULL || authority == NULL || value == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (value->operation == WIND_DOWN_ROUTE)
        return route_store(ctx, value);
    if (value->operation == WIND_DOWN_DEPRECATE)
        return transition_store(ctx, authority, value,
                                LX_PROGRAMS_LIFECYCLE_ACTIVE,
                                LX_PROGRAMS_LIFECYCLE_DEPRECATED,
                                value->exit_program, value->deadline);
    if (value->operation == WIND_DOWN_TOMBSTONE) {
        lx_programs_wind_down_view current;
        lxp_result status = lxp_programs_wind_down_read(
            ctx, value->program_id, &current);
        return status == LXP_OK ?
            transition_store(ctx, authority, value,
                             LX_PROGRAMS_LIFECYCLE_DEPRECATED,
                             LX_PROGRAMS_LIFECYCLE_TOMBSTONED,
                             current.exit_program, current.deadline) : status;
    }
    if (value->operation == WIND_DOWN_EXIT)
        return exit_execute(ctx, activity, authority, value);
    return LXP_ERR_UNKNOWN_FIELD;
}
