#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum {
    ROUTE_OPERATION = 1,
    DEPRECATE_OPERATION = 2,
    TOMBSTONE_OPERATION = 3,
    EXIT_OPERATION = 4
};

static lx_account *account_by_id(lx_account_registry *accounts,
                                 const uint8_t account_id[32])
{
    size_t index;
    if (accounts == NULL) return NULL;
    for (index = 0U; index < accounts->count; ++index)
        if (memcmp(accounts->accounts[index].id, account_id, 32U) == 0)
            return &accounts->accounts[index];
    return NULL;
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

static int activity_dispatch(lxp_kernel *kernel,
                             const lxp_authority_resolved *authority,
                             const uint8_t *payload, size_t payload_length,
                             lxp_result expected, uint16_t expected_event)
{
    uint8_t arena_bytes[65536];
    lxp_arena arena;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    const lxp_module_registration *registration;
    lx_programs_transfer_runtime *runtime;
    lx_account *sequence_account;
    lxp_result module_result = LXP_OK;
    uint64_t sequence = kernel->state->next_sequence;
    if (lxp_state_journal_open(kernel->state, sequence, kernel->journal) !=
            LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_PROGRAMS, sequence, 1U,
                            sequence, 100000U, &arena, true) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK ||
        lxp_kernel_module_for_activity(kernel, LX_PROGRAMS_WIND_DOWN, 1U,
                                       &registration) != LXP_OK)
        return 1;
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    runtime = (lx_programs_transfer_runtime *)
        kernel->module_runtime[LXP_MODULE_PROGRAMS];
    sequence_account = runtime == NULL ? NULL :
        account_by_id(runtime->accounts, authority->principal);
    if (sequence_account == NULL) return 1;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = LX_PROGRAMS_WIND_DOWN;
    activity.account_sequence = sequence_account->next_sequence;
    activity.payload = (lxp_byte_span){payload, payload_length};
    if (lxp_kernel_dispatch(registration, &ctx, &activity, authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != expected)
        return 1;
    if (expected != LXP_OK) {
        if (effects.count != 0U) return 1;
        lxp_module_ctx_rollback(&ctx);
        return lxp_state_journal_rollback(kernel->journal) == LXP_OK ? 0 : 1;
    }
    if (effects.count != 1U ||
        effects.effects[0].event_type != expected_event ||
        lxp_module_ctx_prepare_commit(&ctx) != LXP_OK ||
        lxp_state_journal_commit(kernel->journal) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    return 0;
}

static size_t route_payload(uint8_t payload[259], const uint8_t program[32],
                            const uint8_t account[32], const uint8_t asset[32],
                            const uint8_t destination[32],
                            const uint8_t *seed, uint16_t seed_length)
{
    (void)memcpy(payload, program, 32U);
    payload[32] = ROUTE_OPERATION;
    (void)memcpy(payload + 33U, account, 32U);
    (void)memcpy(payload + 65U, asset, 32U);
    (void)memcpy(payload + 97U, destination, 32U);
    write_u16(payload + 129U, seed_length);
    (void)memcpy(payload + 131U, seed, seed_length);
    return 131U + seed_length;
}

static size_t transition_payload(uint8_t payload[73],
                                 const uint8_t program[32],
                                 uint8_t operation, uint64_t deadline)
{
    (void)memcpy(payload, program, 32U);
    payload[32] = operation;
    if (operation == DEPRECATE_OPERATION) {
        (void)memcpy(payload + 33U, program, 32U);
        write_u64(payload + 65U, deadline);
        return 73U;
    }
    return 33U;
}

static size_t exit_payload(uint8_t payload[65], const uint8_t program[32],
                           const uint8_t account[32])
{
    (void)memcpy(payload, program, 32U);
    payload[32] = EXIT_OPERATION;
    (void)memcpy(payload + 33U, account, 32U);
    return 65U;
}

static lxp_result count_route(const lx_programs_exit_route_view *route,
                              void *user)
{
    size_t *count = (size_t *)user;
    if (route == NULL || route->destination[0] == 0U)
        return LXP_ERR_CONTEXT_MISMATCH;
    ++*count;
    return LXP_OK;
}

static lxp_result count_history(
    const lx_programs_wind_down_history_view *history, void *user)
{
    size_t *count = (size_t *)user;
    if (history == NULL || history->effective_sequence == 0U ||
        lxp_ct_is_zero(history->account_root, 32U))
        return LXP_ERR_CONTEXT_MISMATCH;
    ++*count;
    return LXP_OK;
}

static int forged_program_spend_refused(
    lx_account *principal, lx_account *source, lx_account *destination,
    const lxp_transfer_asset_state *assets, size_t asset_count)
{
    lxp_transfer_leg leg;
    lxp_transfer_source_authority source_authority;
    lxp_transfer_context context;
    lxp_transfer_set_result result;
    lxp_u128 source_before = source->balance;
    lxp_u128 destination_before = destination->balance;
    uint64_t sequence_before = principal->next_sequence;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = source;
    leg.to = destination;
    (void)memcpy(leg.asset_id, source->asset_id, 32U);
    leg.amount = source->balance;
    leg.reason = LXP_REASON_PAYMENT;
    leg.supply_mode = LXP_TRANSFER_CONSERVED;
    (void)memset(&source_authority, 0, sizeof(source_authority));
    (void)memcpy(source_authority.authorized_from, source->id, 32U);
    source_authority.debit_authority_kind = LXP_AUTH_PROGRAM_SPEND;
    (void)memset(&context, 0, sizeof(context));
    context.assets = assets;
    context.asset_count = asset_count;
    context.actor_sequence = sequence_before;
    context.sequence_account = principal;
    context.origin_module_id = LXP_MODULE_PROGRAMS;
    context.debit_authority_kind = LXP_AUTH_PROGRAM_SPEND;
    context.source_authorities = &source_authority;
    context.source_authority_count = 1U;
    context.program_spend_token = UINT64_MAX;
    return lxp_apply_transfer_set(&leg, 1U, &context, &result) ==
               LXP_ERR_UNAUTHORIZED_DEBIT &&
           lxp_u128_cmp(source->balance, source_before) == 0 &&
           lxp_u128_cmp(destination->balance, destination_before) == 0 &&
           principal->next_sequence == sequence_before ? 0 : 1;
}

static int malformed_program_spend_tables_refused(
    lxp_kernel *kernel, lx_account *principal, lx_account *source,
    lx_account *destination, const lxp_transfer_asset_state *assets,
    size_t asset_count)
{
    lxp_transfer_leg legs[2];
    lxp_transfer_source_authority authorities[2];
    lxp_transfer_set set;
    lxp_receipt receipt;
    lxp_u128 source_before = source->balance;
    lxp_u128 destination_before = destination->balance;
    uint64_t sequence_before = principal->next_sequence;
    size_t index;
    (void)memset(legs, 0, sizeof(legs));
    for (index = 0U; index < 2U; ++index) {
        legs[index].from = source;
        legs[index].to = destination;
        (void)memcpy(legs[index].asset_id, source->asset_id, 32U);
        legs[index].amount = (lxp_u128){0U, 1U};
        legs[index].reason = LXP_REASON_PAYMENT;
        legs[index].supply_mode = LXP_TRANSFER_CONSERVED;
    }
    (void)memset(authorities, 0, sizeof(authorities));
    for (index = 0U; index < 2U; ++index) {
        (void)memcpy(authorities[index].authorized_from, source->id, 32U);
        authorities[index].debit_authority_kind = LXP_AUTH_PROGRAM_SPEND;
    }
    (void)memset(&set, 0, sizeof(set));
    (void)memcpy(set.legs, legs, sizeof(legs));
    set.leg_count = 1U;
    set.context.assets = assets;
    set.context.asset_count = asset_count;
    set.context.sequence_account = principal;
    set.context.actor_sequence = sequence_before;
    set.context.origin_module_id = LXP_MODULE_PROGRAMS;
    set.context.source_authorities = authorities;
    set.context.source_authority_count = LXP_MAX_TRANSFER_SET_LEGS + 1U;
    set.context.program_spend_token = UINT64_MAX;
    (void)memset(&receipt, 0, sizeof(receipt));
    if (lxp_kernel_apply_transfer_set(kernel, &set, &receipt) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    set.leg_count = 2U;
    set.context.source_authority_count = 2U;
    if (lxp_kernel_apply_transfer_set(kernel, &set, &receipt) !=
            LXP_ERR_UNAUTHORIZED_DEBIT)
        return 1;
    set.context.source_authorities = NULL;
    set.context.source_authority_count = 0U;
    if (lxp_kernel_apply_transfer_set(kernel, &set, &receipt) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    return lxp_u128_cmp(source->balance, source_before) == 0 &&
                   lxp_u128_cmp(destination->balance,
                                destination_before) == 0 &&
                   principal->next_sequence == sequence_before ?
               0 : 1;
}

int main(void)
{
    static const uint8_t program_prefix[] = "program\0";
    static const uint8_t owner_prefix[] = "program-owner\0";
    static const char *names[3] = {
        "agent:did:key:wind-owner:main",
        "agent:did:key:wind-one:main",
        "agent:did:key:wind-two:main"
    };
    static const uint8_t seeds[2][3] = {{'o','n','e'}, {'t','w','o'}};
    uint8_t program[32], ids[3][32], program_accounts[2][32];
    uint8_t program_key[sizeof(program_prefix) - 1U + 32U];
    uint8_t owner_key[sizeof(owner_prefix) - 1U + 32U];
    uint8_t program_record[71], owner_record[33];
    uint8_t route[259], transition[73], exit[65];
    lx_account_registry accounts;
    lx_account *opened[3], *program_account;
    lxp_transfer_asset_state assets[2];
    lx_programs_transfer_runtime runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[65536];
    lxp_authority_resolved authority;
    lx_programs_wind_down_view status_view;
    bool created;
    uint64_t parameters = 1U;
    uint64_t deadline;
    size_t routes = 0U, history = 0U, index;

    (void)memset(program, 0x31, sizeof(program));
    (void)memset(program_record, 0, sizeof(program_record));
    (void)memset(owner_record, 0, sizeof(owner_record));
    (void)memset(assets, 0, sizeof(assets));
    (void)memset(&runtime, 0, sizeof(runtime));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(assets[0].asset_id, 0x21, 32U);
    (void)memset(assets[1].asset_id, 0x22, 32U);
    assets[0].registered = true;
    assets[1].registered = true;
    if (lx_account_registry_init(&accounts) != LXP_OK) return 1;
    for (index = 0U; index < 3U; ++index)
        if (lx_account_id_from_string((const uint8_t *)names[index],
                                      strlen(names[index]), ids[index]) !=
                LXP_OK ||
            lx_account_open(&accounts, (const uint8_t *)names[index],
                            strlen(names[index]), ids[index], 7U,
                            LX_ACCOUNT_OPEN_CREDIT, NULL, &opened[index]) !=
                LXP_OK)
            return 1;
    if (lxp_ledger_bootstrap_balance(opened[0], assets[0].asset_id,
                                     (lxp_u128){0U, 0U}, 7U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened[1], assets[0].asset_id,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened[2], assets[1].asset_id,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK)
        return 1;
    runtime.accounts = &accounts;
    runtime.assets = assets;
    runtime.asset_count = 2U;
    if (lxp_state_store_init(&state, 7U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_set_epoch(&kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v2()) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS,
                                       &runtime) != LXP_OK ||
        lxp_kernel_set_capabilities(
            &kernel, NULL, lxp_kernel_canonical_ledger_apply) != LXP_OK)
        return 1;
    (void)memcpy(authority.principal, ids[0], 32U);
    (void)memset(authority.authority_hash, 0x51, 32U);
    (void)memcpy(program_key, program_prefix, sizeof(program_prefix) - 1U);
    (void)memcpy(program_key + sizeof(program_prefix) - 1U, program, 32U);
    (void)memcpy(owner_key, owner_prefix, sizeof(owner_prefix) - 1U);
    (void)memcpy(owner_key + sizeof(owner_prefix) - 1U, program, 32U);
    program_record[0] = 1U;
    (void)memcpy(program_record + 1U, ids[0], 32U);
    (void)memset(program_record + 33U, 0x61, 32U);
    program_record[66] = 2U;
    program_record[68] = 1U;
    program_record[70] = 1U;
    owner_record[0] = 1U;
    (void)memcpy(owner_record + 1U, ids[0], 32U);
    if (lxp_state_journal_open(&state, 7U, &journal) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS, 7U, 1U, 7U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&ctx, program_key, sizeof(program_key), program_record,
                       sizeof(program_record)) != LXP_OK ||
        lxp_ctx_kv_put(&ctx, owner_key, sizeof(owner_key), owner_record,
                       sizeof(owner_record)) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;

    if (lxp_state_journal_open(&state, 8U, &journal) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS, 8U, 1U, 7U,
                            100000U, &arena, true) != LXP_OK)
        return 1;
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    for (index = 0U; index < 2U; ++index)
        if (lxp_programs_account_register(
                &ctx, program, seeds[index], sizeof(seeds[index]),
                assets[index].asset_id, &program_account, &created) != LXP_OK ||
            !created ||
            lxp_programs_account_derive(program, seeds[index],
                                        sizeof(seeds[index]),
                                        program_accounts[index]) != LXP_OK)
            return 1;
    if (lxp_module_ctx_prepare_commit(&ctx) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    program_account = account_by_id(&accounts, program_accounts[0]);
    if (program_account == NULL ||
        lxp_ledger_bootstrap_balance(program_account, assets[0].asset_id,
                                     (lxp_u128){0U, 40U}, 0U) != LXP_OK ||
        (program_account = account_by_id(&accounts, program_accounts[1])) ==
            NULL ||
        lxp_ledger_bootstrap_balance(program_account, assets[1].asset_id,
                                     (lxp_u128){0U, 60U}, 0U) != LXP_OK)
        return 1;
    program_account = account_by_id(&accounts, program_accounts[0]);
    if (program_account == NULL ||
        forged_program_spend_refused(opened[0], program_account, opened[1],
                                     assets, 2U) != 0 ||
        malformed_program_spend_tables_refused(
            &kernel, opened[0], program_account, opened[1],
            assets, 2U) != 0)
        return 1;

    if (activity_dispatch(
            &kernel, &authority,
            route, route_payload(route, program, program_accounts[0],
                                 assets[0].asset_id, ids[1], seeds[0],
                                 (uint16_t)sizeof(seeds[0])),
            LXP_OK, LX_PROGRAMS_EVENT_EXIT_ROUTE) != 0)
        return 1;
    deadline = state.next_sequence + 1U;
    if (activity_dispatch(
            &kernel, &authority, transition,
            transition_payload(transition, program, DEPRECATE_OPERATION,
                               deadline),
            LXP_ERR_UNKNOWN_FIELD, 0U) != 0 ||
        lxp_programs_wind_down_read(&ctx, program, &status_view) == LXP_OK)
        return 1;
    if (activity_dispatch(
            &kernel, &authority,
            route, route_payload(route, program, program_accounts[1],
                                 assets[1].asset_id, ids[2], seeds[1],
                                 (uint16_t)sizeof(seeds[1])),
            LXP_OK, LX_PROGRAMS_EVENT_EXIT_ROUTE) != 0)
        return 1;
    deadline = state.next_sequence + 1U;
    if (activity_dispatch(
            &kernel, &authority, transition,
            transition_payload(transition, program, DEPRECATE_OPERATION,
                               deadline),
            LXP_OK, LX_PROGRAMS_EVENT_DEPRECATED) != 0)
        return 1;

    if (lxp_state_journal_open(&state, state.next_sequence, &journal) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS,
                            state.next_sequence, 1U, 7U, 100000U, &arena,
                            true) != LXP_OK ||
        lxp_programs_wind_down_read(&ctx, program, &status_view) != LXP_OK ||
        status_view.status != LX_PROGRAMS_LIFECYCLE_DEPRECATED ||
        status_view.value_account_count != 2U ||
        status_view.live_value_account_count != 2U ||
        lxp_programs_exit_route_iter(&ctx, program, count_route, &routes) !=
            LXP_OK ||
        routes != 2U)
        return 1;
    lxp_module_ctx_rollback(&ctx);
    if (lxp_state_journal_rollback(&journal) != LXP_OK) return 1;

    if (activity_dispatch(
            &kernel, &authority, exit,
            exit_payload(exit, program, program_accounts[0]), LXP_OK,
            LX_PROGRAMS_EVENT_VALUE_EXITED) != 0 ||
        opened[1]->balance.lo != 40U)
        return 1;
    if (activity_dispatch(
            &kernel, &authority, transition,
            transition_payload(transition, program, TOMBSTONE_OPERATION, 0U),
            LXP_OK, LX_PROGRAMS_EVENT_TOMBSTONED) != 0)
        return 1;
    if (state.next_sequence <= deadline ||
        activity_dispatch(
            &kernel, &authority, exit,
            exit_payload(exit, program, program_accounts[1]), LXP_OK,
            LX_PROGRAMS_EVENT_VALUE_EXITED) != 0 ||
        opened[2]->balance.lo != 60U)
        return 1;

    routes = 0U;
    if (lxp_state_journal_open(&state, state.next_sequence, &journal) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS,
                            state.next_sequence, 1U, 7U, 100000U, &arena,
                            true) != LXP_OK ||
        lxp_programs_wind_down_read(&ctx, program, &status_view) != LXP_OK ||
        status_view.status != LX_PROGRAMS_LIFECYCLE_TOMBSTONED ||
        lxp_programs_wind_down_history_iter(
            &ctx, program, count_history, &history) != LXP_OK ||
        history != 2U ||
        lxp_programs_exit_route_iter(&ctx, program, count_route, &routes) !=
            LXP_OK ||
        routes != 2U)
        return 1;
    lxp_module_ctx_rollback(&ctx);
    if (lxp_state_journal_rollback(&journal) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
