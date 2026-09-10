#include "layerx/lx_stream.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <stdio.h>
#include <string.h>

#define CHECK(x) do { \
    if (!(x)) { (void)fprintf(stderr, "line %d\n", __LINE__); return 1; } \
} while (0)

static const char payer_name[] = "agent:did:key:payer:main";
static const char provider_name[] = "agent:did:key:provider:main";
static const char stream_one_name[] = "agent:did:key:payer:stream:s1";
static const char stream_two_name[] = "agent:did:key:payer:stream:s2";

static struct {
    size_t calls;
    size_t leg_count;
    uint16_t reasons[2];
    size_t authority_count;
    lxp_authorization_kind authority_kind;
    uint8_t authorized_from[32];
    uint16_t origin_module_id;
    uint64_t actor_sequence;
    uint8_t transfer_set_root[32];
} observed;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    size_t i;
    (void)kernel;
    ++observed.calls;
    observed.leg_count = set->leg_count;
    for (i = 0U; i < set->leg_count && i < 2U; ++i)
        observed.reasons[i] = set->legs[i].reason;
    observed.authority_count = set->context.source_authority_count;
    observed.origin_module_id = set->context.origin_module_id;
    observed.actor_sequence = set->context.actor_sequence;
    if (set->context.source_authorities != NULL &&
        set->context.source_authority_count != 0U) {
        observed.authority_kind =
            set->context.source_authorities[0].debit_authority_kind;
        (void)memcpy(observed.authorized_from,
                     set->context.source_authorities[0].authorized_from, 32U);
    }
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK) {
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root,
                     32U);
        (void)memcpy(observed.transfer_set_root, result.transfer_set_root,
                     32U);
    }
    return status;
}

static lx_account_registry accounts;
static lx_account *payer;
static lx_account *provider;
static lx_account *stream_one;
static lx_account *stream_two;
static lx_asset_record asset;
static lxp_transfer_asset_state asset_state;
static lx_stream_runtime runtime;
static lxp_state_store state;
static lxp_state_journal journal;
static lxp_kernel kernel;
static lxp_module_ctx ctx;
static lxp_effect_buffer effects;
static lxp_arena arena;
static uint8_t arena_bytes[131072];
static uint64_t parameters = 1U;

static int open_account(const char *name, lx_account **account)
{
    uint8_t id[32];
    size_t length = strlen(name);
    CHECK(lx_account_id_from_string((const uint8_t *)name, length, id) ==
          LXP_OK);
    CHECK(lx_account_open(&accounts, (const uint8_t *)name, length, id, 1U,
                          LX_ACCOUNT_OPEN_CREDIT, NULL, account) == LXP_OK);
    return 0;
}

static int fixture_init(void)
{
    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 6U;
    CHECK(lx_asset_transfer_state(&asset, &asset_state) == LXP_OK);
    runtime.assets = &asset_state;
    runtime.asset_count = 1U;
    CHECK(lx_account_registry_init(&accounts) == LXP_OK);
    if (open_account(payer_name, &payer) != 0) return 1;
    if (open_account(provider_name, &provider) != 0) return 1;
    if (open_account(stream_one_name, &stream_one) != 0) return 1;
    if (open_account(stream_two_name, &stream_two) != 0) return 1;
    CHECK(lxp_ledger_bootstrap_balance(payer, asset.asset_id,
                                       (lxp_u128){ 0U, 100U }, 0U) == LXP_OK);
    CHECK(lxp_state_store_init(&state, 0U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(&state, &accounts) == LXP_OK);
    CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) ==
          LXP_OK);
    CHECK(lxp_kernel_register_module(&kernel, lx_stream_module_iface()) ==
          LXP_OK);
    CHECK(lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) ==
          LXP_OK);
    CHECK(lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_STREAM,
                                         &runtime) == LXP_OK);
    return 0;
}

static lxp_result begin_ctx(uint64_t timestamp, uint64_t sequence)
{
    lxp_result status = lxp_arena_init(&arena, arena_bytes,
                                       sizeof(arena_bytes));
    if (status == LXP_OK) status = lxp_effect_buffer_init(&effects);
    if (status == LXP_OK)
        status = lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_STREAM,
                                     timestamp, 0U, sequence, 1000000U,
                                     &arena, true);
    if (status == LXP_OK) status = lxp_module_ctx_bind_effects(&ctx, &effects);
    return status;
}

static lxp_result run(uint32_t activity_type, const uint8_t *payload,
                      size_t length, const uint8_t principal[32],
                      uint64_t timestamp, uint64_t sequence,
                      lxp_result *module_result)
{
    const lxp_module_registration *registration;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_result status;
    status = lxp_kernel_module_for_activity(&kernel, LX_STREAM_PAUSE, 0U,
                                            &registration);
    if (status != LXP_OK) return status;
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&authority, 0, sizeof(authority));
    activity.activity_type = activity_type;
    activity.payload.bytes = payload;
    activity.payload.length = length;
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, principal, 32U);
    status = begin_ctx(timestamp, sequence);
    if (status != LXP_OK) return status;
    *module_result = LXP_OK;
    (void)memset(&observed, 0, sizeof(observed));
    return lxp_kernel_dispatch(registration, &ctx, &activity, &authority,
                               &effects, module_result);
}

static int lifecycle(uint32_t activity_type, uint8_t marker,
                     uint64_t timestamp, uint64_t sequence,
                     const uint8_t principal[32], lxp_result *module_result)
{
    lx_stream_id_payload payload;
    uint8_t bytes[LX_STREAM_ID_PAYLOAD_BYTES];
    size_t length = 0U;
    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = marker;
    CHECK(lx_stream_id_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(activity_type, bytes, length, principal, timestamp, sequence,
              module_result) == LXP_OK);
    return 0;
}

static int keyed(uint32_t activity_type, uint8_t marker, uint8_t key_marker,
                 uint64_t timestamp, uint64_t sequence,
                 const uint8_t principal[32], lxp_result *module_result)
{
    lx_stream_keyed_payload payload;
    uint8_t bytes[LX_STREAM_KEYED_PAYLOAD_BYTES];
    size_t length = 0U;
    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = marker;
    payload.idempotency_key[0] = key_marker;
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(activity_type, bytes, length, principal, timestamp, sequence,
              module_result) == LXP_OK);
    return 0;
}

static int open_stream(uint8_t marker, lx_account *custody, uint64_t funding,
                       uint64_t start, uint64_t timestamp, uint64_t sequence,
                       lxp_result *module_result)
{
    lx_stream_open_payload payload;
    uint8_t bytes[LX_STREAM_OPEN_PAYLOAD_MAX];
    size_t length = 0U;
    (void)memset(&payload, 0, sizeof(payload));
    payload.record.stream_id[0] = marker;
    (void)memcpy(payload.record.stream_account, custody->id, 32U);
    (void)memcpy(payload.record.recipient, provider->id, 32U);
    (void)memcpy(payload.record.asset_id, asset.asset_id, 32U);
    payload.record.mode = LX_STREAM_MODE_TIME;
    payload.record.rate = (lxp_u128){ 0U, 10U };
    payload.record.rate_unit = 1000U;
    payload.record.start_timestamp = start;
    payload.record.total_cap = (lxp_u128){ 0U, 500U };
    payload.initial_funding = (lxp_u128){ 0U, funding };
    CHECK(lx_stream_open_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(LX_STREAM_OPEN, bytes, length, payer->id, timestamp, sequence,
              module_result) == LXP_OK);
    return 0;
}

static int id_payload_codec(void)
{
    lx_stream_id_payload payload;
    lx_stream_id_payload decoded;
    uint8_t bytes[LX_STREAM_ID_PAYLOAD_BYTES];
    size_t length = 0U;

    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = 1U;
    CHECK(lx_stream_id_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(length == (size_t)LX_STREAM_ID_PAYLOAD_BYTES);
    CHECK(lx_stream_id_decode(bytes, length, &decoded) == LXP_OK);
    CHECK(memcmp(&payload, &decoded, sizeof(payload)) == 0);
    CHECK(lx_stream_id_encode(&payload, bytes, length - 1U, &length) ==
          LXP_ERR_LENGTH_LIMIT);
    CHECK(lx_stream_id_encode(NULL, bytes, sizeof(bytes), &length) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_id_encode(&payload, bytes, sizeof(bytes), NULL) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_id_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(lx_stream_id_decode(bytes, length - 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_id_decode(bytes, length + 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_id_decode(NULL, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_id_decode(bytes, length, NULL) == LXP_ERR_NON_CANONICAL);
    bytes[1] = 7U;
    CHECK(lx_stream_id_decode(bytes, length, &decoded) ==
          LXP_ERR_VERSION_UNSUPPORTED);
    bytes[1] = (uint8_t)LX_STREAM_PAYLOAD_VERSION;
    (void)memset(bytes + 2U, 0, 32U);
    CHECK(lx_stream_id_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    return 0;
}

static int pause_and_resume(void)
{
    lx_stream_record record;
    uint8_t id[32] = { 0U };
    uint8_t stamp[8];
    uint8_t amount[16];
    lxp_result module_result = LXP_OK;
    size_t i;
    id[0] = 1U;

    if (open_stream(1U, stream_one, 40U, 1000U, 1000U, 1U,
                    &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 60U && stream_one->balance.lo == 40U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Pausing settles the clock up to the moment it stops. */
    if (lifecycle(LX_STREAM_PAUSE, 1U, 3000U, 2U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 5U);
    CHECK(effects.effects[0].body_length == 56U);
    CHECK(memcmp(effects.effects[0].body, id, 32U) == 0);
    for (i = 0U; i < 8U; ++i)
        stamp[i] = (uint8_t)(3000U >> ((7U - i) * 8U));
    CHECK(memcmp(effects.effects[0].body + 32U, stamp, 8U) == 0);
    CHECK(lxp_u128_to_be((lxp_u128){ 0U, 20U }, amount) == LXP_OK);
    CHECK(memcmp(effects.effects[0].body + 40U, amount, 16U) == 0);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.paused && record.accrued_total.lo == 20U);
    CHECK(record.last_accrual_timestamp == 3000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Pausing an already paused stream writes nothing. */
    if (lifecycle(LX_STREAM_PAUSE, 1U, 4000U, 3U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(ctx.staged_count == 0U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.paused && record.last_accrual_timestamp == 3000U);

    /* A paused stream still pays out what it already accrued. */
    if (keyed(LX_STREAM_SETTLE, 1U, 0x30U, 4000U, 4U, provider->id,
              &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 1U);
    CHECK(observed.reasons[0] == (uint16_t)LXP_REASON_STREAM_DRAW);
    CHECK(stream_one->balance.lo == 20U && provider->balance.lo == 20U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.paused && record.accrued_total.lo == 20U);
    CHECK(record.settled_total.lo == 20U);
    CHECK(record.last_accrual_timestamp == 3000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Resuming restarts the clock from the resume, never from the pause. */
    if (lifecycle(LX_STREAM_RESUME, 1U, 5000U, 5U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 6U);
    CHECK(effects.effects[0].body_length == 56U);
    for (i = 0U; i < 8U; ++i)
        stamp[i] = (uint8_t)(5000U >> ((7U - i) * 8U));
    CHECK(memcmp(effects.effects[0].body + 32U, stamp, 8U) == 0);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(!record.paused && record.last_accrual_timestamp == 5000U);
    CHECK(record.accrued_total.lo == 20U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Resuming a running stream writes nothing. */
    if (lifecycle(LX_STREAM_RESUME, 1U, 6000U, 6U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(ctx.staged_count == 0U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.last_accrual_timestamp == 5000U);

    /* Only the payer controls the lifecycle. */
    if (lifecycle(LX_STREAM_PAUSE, 1U, 6000U, 7U, provider->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_UNAUTHORIZED_DEBIT);
    if (lifecycle(LX_STREAM_RESUME, 1U, 6000U, 7U, stream_one->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_UNAUTHORIZED_DEBIT);

    /* A pause can not reach back before the last accrual. */
    if (lifecycle(LX_STREAM_PAUSE, 1U, 4500U, 7U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_NON_MONOTONIC_TIME);

    /* An unknown stream has no lifecycle. */
    if (lifecycle(LX_STREAM_PAUSE, 9U, 6000U, 7U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_UNKNOWN_FIELD);
    return 0;
}

static int close_stream(void)
{
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_receipt replayed;
    uint8_t close_root[32];
    uint8_t id[32] = { 0U };
    uint8_t paid[16];
    uint8_t refunded[16];
    lxp_result module_result = LXP_OK;
    bool found = false;
    id[0] = 1U;

    {
        lx_stream_amount_payload amount;
        uint8_t bytes[LX_STREAM_TOP_UP_PAYLOAD_BYTES];
        size_t length = 0U;
        (void)memset(&amount, 0, sizeof(amount));
        amount.stream_id[0] = 1U;
        amount.amount = (lxp_u128){ 0U, 30U };
        CHECK(lx_stream_amount_encode(&amount, bytes, sizeof(bytes),
                                      &length) == LXP_OK);
        CHECK(run(LX_STREAM_TOP_UP, bytes, length, payer->id, 6000U, 8U,
                  &module_result) == LXP_OK);
    }
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 30U && stream_one->balance.lo == 50U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.last_accrual_timestamp == 5000U && !record.underfunded);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Closing pays the recipient what is owed and returns the rest of
     * custody to the payer in one atomic set. */
    if (keyed(LX_STREAM_CLOSE, 1U, 0x31U, 7000U, 9U, payer->id,
              &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 2U);
    CHECK(observed.reasons[0] == (uint16_t)LXP_REASON_STREAM_DRAW);
    CHECK(observed.reasons[1] == (uint16_t)LXP_REASON_STREAM_REFUND);
    CHECK(observed.authority_count == 1U);
    CHECK(observed.authority_kind == LXP_AUTH_PROTOCOL_MODULE);
    CHECK(memcmp(observed.authorized_from, stream_one->id, 32U) == 0);
    CHECK(observed.origin_module_id == (uint16_t)LXP_MODULE_STREAM);
    CHECK(observed.actor_sequence == 1U);
    CHECK(stream_one->balance.lo == 0U);
    CHECK(provider->balance.lo == 40U);
    CHECK(payer->balance.lo == 60U);
    CHECK(stream_one->next_sequence == 2U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 7U);
    CHECK(effects.effects[0].body_length == 64U);
    CHECK(memcmp(effects.effects[0].body, id, 32U) == 0);
    CHECK(lxp_u128_to_be((lxp_u128){ 0U, 20U }, paid) == LXP_OK);
    CHECK(lxp_u128_to_be((lxp_u128){ 0U, 30U }, refunded) == LXP_OK);
    CHECK(memcmp(effects.effects[0].body + 32U, paid, 16U) == 0);
    CHECK(memcmp(effects.effects[0].body + 48U, refunded, 16U) == 0);
    (void)memcpy(close_root, observed.transfer_set_root, 32U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.closed && !record.paused && !record.underfunded);
    CHECK(record.accrued_total.lo == 40U && record.settled_total.lo == 40U);
    CHECK(record.last_accrual_timestamp == 7000U);
    CHECK(lx_stream_result_load(&ctx, (const uint8_t[32]){ 0x31U }, &result,
                                &found) == LXP_OK);
    CHECK(found && result.ordinal == 7U && result.leg_count == 2U);
    CHECK(result.paid.lo == 20U && result.refunded.lo == 30U);
    CHECK(memcmp(result.transfer_set_root, close_root, 32U) == 0);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* A replayed close moves nothing and reproduces the original receipt. */
    if (keyed(LX_STREAM_CLOSE, 1U, 0x31U, 8000U, 10U, payer->id,
              &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(ctx.staged_count == 0U);
    CHECK(stream_one->balance.lo == 0U && provider->balance.lo == 40U);
    CHECK(payer->balance.lo == 60U);
    CHECK(stream_one->next_sequence == 2U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 7U);
    CHECK(lx_stream_result_load(&ctx, (const uint8_t[32]){ 0x31U }, &result,
                                &found) == LXP_OK);
    CHECK(found);
    CHECK(lx_stream_result_receipt(&result, &replayed) == LXP_OK);
    CHECK(memcmp(replayed.transfer_set_root, close_root, 32U) == 0);

    /* Nothing runs on a closed stream under a fresh key. */
    if (keyed(LX_STREAM_CLOSE, 1U, 0x32U, 8000U, 10U, payer->id,
              &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_STREAM_CLOSED);
    if (keyed(LX_STREAM_SETTLE, 1U, 0x33U, 8000U, 10U, payer->id,
              &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_STREAM_CLOSED);
    if (lifecycle(LX_STREAM_PAUSE, 1U, 8000U, 10U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_STREAM_CLOSED);
    if (lifecycle(LX_STREAM_RESUME, 1U, 8000U, 10U, payer->id,
                  &module_result) != 0) return 1;
    CHECK(module_result == LXP_ERR_STREAM_CLOSED);
    {
        lx_stream_amount_payload amount;
        uint8_t bytes[LX_STREAM_TOP_UP_PAYLOAD_BYTES];
        size_t length = 0U;
        (void)memset(&amount, 0, sizeof(amount));
        amount.stream_id[0] = 1U;
        amount.amount = (lxp_u128){ 0U, 5U };
        CHECK(lx_stream_amount_encode(&amount, bytes, sizeof(bytes),
                                      &length) == LXP_OK);
        CHECK(run(LX_STREAM_TOP_UP, bytes, length, payer->id, 8000U, 10U,
                  &module_result) == LXP_OK);
    }
    CHECK(module_result == LXP_ERR_STREAM_CLOSED);
    CHECK(payer->balance.lo == 60U);
    return 0;
}

static int close_atomicity(void)
{
    lx_stream_lifecycle_request request;
    lxp_authority_resolved authority;
    lxp_transfer_asset_state permitted;
    lxp_receipt receipt;
    lx_stream_record record;
    lx_stream_economic_result result;
    uint8_t id[32] = { 0U };
    lxp_result module_result = LXP_OK;
    bool found = false;
    size_t attempt;
    id[0] = 2U;

    if (open_stream(2U, stream_two, 40U, 9000U, 9000U, 11U,
                    &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 20U && stream_two->balance.lo == 40U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    permitted = asset_state;
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, payer->id, 32U);

    /* A ledger failure on either leg of a two leg close leaves the whole
     * set unapplied: no partial payment, no partial refund, no sequence
     * consumed and no state written. */
    for (attempt = 0U; attempt < 2U; ++attempt) {
        CHECK(begin_ctx(11000U, 12U) == LXP_OK);
        (void)memset(&observed, 0, sizeof(observed));
        (void)memset(&request, 0, sizeof(request));
        (void)memset(&receipt, 0, sizeof(receipt));
        request.stream_id = id;
        request.stream_account = stream_two;
        request.payer = payer;
        request.recipient = provider;
        (void)memcpy(request.asset_id, asset.asset_id, 32U);
        request.authority = &authority;
        request.idempotency_key[0] = (uint8_t)(0x40U + attempt);
        request.context.assets = &permitted;
        request.context.asset_count = 1U;
        request.context.inject_failure = true;
        request.context.failure_after_leg = attempt;
        CHECK(lx_stream_close_execute(&ctx, &request, &receipt) ==
              LXP_ERR_IO);
        CHECK(observed.calls == 1U && observed.leg_count == 2U);
        CHECK(stream_two->balance.lo == 40U);
        CHECK(provider->balance.lo == 40U);
        CHECK(payer->balance.lo == 20U);
        CHECK(stream_two->next_sequence == 0U);
        CHECK(ctx.staged_count == 0U);
        CHECK(lx_stream_result_load(&ctx, request.idempotency_key, &result,
                                    &found) == LXP_OK);
        CHECK(!found);
        CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
        CHECK(!record.closed && lxp_u128_is_zero(record.settled_total));
    }

    /* The same close without the injected failure commits in full. */
    CHECK(begin_ctx(11000U, 12U) == LXP_OK);
    (void)memset(&observed, 0, sizeof(observed));
    (void)memset(&receipt, 0, sizeof(receipt));
    request.idempotency_key[0] = 0x42U;
    request.context.inject_failure = false;
    request.context.failure_after_leg = 0U;
    CHECK(lx_stream_close_execute(&ctx, &request, &receipt) == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 2U);
    CHECK(observed.reasons[0] == (uint16_t)LXP_REASON_STREAM_DRAW);
    CHECK(observed.reasons[1] == (uint16_t)LXP_REASON_STREAM_REFUND);
    CHECK(stream_two->balance.lo == 0U);
    CHECK(provider->balance.lo == 60U);
    CHECK(payer->balance.lo == 40U);
    CHECK(stream_two->next_sequence == 1U);
    CHECK(memcmp(receipt.transfer_set_root, observed.transfer_set_root,
                 32U) == 0);
    CHECK(lx_stream_result_load(&ctx, request.idempotency_key, &result,
                                &found) == LXP_OK);
    CHECK(found && result.leg_count == 2U);
    CHECK(result.paid.lo == 20U && result.refunded.lo == 20U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.closed && record.settled_total.lo == 20U);
    CHECK(record.accrued_total.lo == 20U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);
    return 0;
}

static lxp_result count_streams(const lx_stream_record *record, void *user)
{
    size_t *count = (size_t *)user;
    if (record == NULL || !record->closed) return LXP_ERR_NON_CANONICAL;
    ++(*count);
    return LXP_OK;
}

static int final_state(void)
{
    size_t count = 0U;
    lxp_u128 total;
    CHECK(lx_stream_iter(&ctx, count_streams, &count) == LXP_OK);
    CHECK(count == 2U);
    CHECK(lxp_u128_add(payer->balance, provider->balance, &total) == LXP_OK);
    CHECK(lxp_u128_add(total, stream_one->balance, &total) == LXP_OK);
    CHECK(lxp_u128_add(total, stream_two->balance, &total) == LXP_OK);
    CHECK(total.hi == 0U && total.lo == 100U);
    CHECK(lxp_u128_is_zero(stream_one->balance));
    CHECK(lxp_u128_is_zero(stream_two->balance));
    return 0;
}

int main(void)
{
    if (fixture_init() != 0) return 1;
    if (id_payload_codec() != 0) return 1;
    if (pause_and_resume() != 0) return 1;
    if (close_stream() != 0) return 1;
    if (close_atomicity() != 0) return 1;
    if (final_state() != 0) return 1;
    if (lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
}
