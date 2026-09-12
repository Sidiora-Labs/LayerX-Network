#include "layerx/lx_stream.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <openssl/evp.h>
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
    uint16_t reason;
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
    (void)kernel;
    ++observed.calls;
    observed.leg_count = set->leg_count;
    observed.reason = set->legs[0].reason;
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
static uint8_t meter_key[32];

static int sign_attestation(lx_stream_meter_attestation *attestation,
                            const uint8_t seed[32])
{
    uint8_t message[128];
    uint8_t digest[32];
    size_t message_length;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int failed = key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, attestation->authority_key,
                                    &public_length) != 1 ||
        lx_stream_meter_attestation_bytes(attestation, message,
                                          sizeof(message),
                                          &message_length) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message,
                        message_length, digest) != LXP_OK ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, attestation->signature, &signature_length,
                       digest, sizeof(digest)) != 1 || signature_length != 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return failed;
}

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
    lx_stream_meter_attestation probe;
    (void)memset(&probe, 0, sizeof(probe));
    probe.stream_id[0] = 2U;
    probe.cumulative_reading = 1U;
    {
        static const uint8_t seed[32] = { 11U };
        CHECK(sign_attestation(&probe, seed) == 0);
    }
    (void)memcpy(meter_key, probe.authority_key, 32U);
    CHECK(!lxp_ct_is_zero(meter_key, 32U));

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
    CHECK(stream_one->kind == LX_ACCOUNT_AGENT_STREAM);
    CHECK(stream_two->kind == LX_ACCOUNT_AGENT_STREAM);
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

static lxp_result run(uint32_t activity_type, const uint8_t *payload,
                      size_t length, const uint8_t principal[32],
                      uint64_t timestamp, uint64_t sequence,
                      lxp_result *module_result)
{
    const lxp_module_registration *registration;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_result status;
    status = lxp_kernel_module_for_activity(&kernel, LX_STREAM_SETTLE, 0U,
                                            &registration);
    if (status != LXP_OK) return status;
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&authority, 0, sizeof(authority));
    activity.activity_type = activity_type;
    activity.payload.bytes = payload;
    activity.payload.length = length;
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, principal, 32U);
    status = lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes));
    if (status == LXP_OK) status = lxp_effect_buffer_init(&effects);
    if (status == LXP_OK)
        status = lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_STREAM,
                                     timestamp, 0U, sequence, 1000000U,
                                     &arena, true);
    if (status == LXP_OK) status = lxp_module_ctx_bind_effects(&ctx, &effects);
    if (status != LXP_OK) return status;
    *module_result = LXP_OK;
    (void)memset(&observed, 0, sizeof(observed));
    return lxp_kernel_dispatch(registration, &ctx, &activity, &authority,
                               &effects, module_result);
}

static int settle(uint8_t stream_marker, uint8_t key_marker,
                  uint64_t timestamp, uint64_t sequence,
                  const uint8_t principal[32], lxp_result *module_result)
{
    lx_stream_keyed_payload payload;
    uint8_t bytes[LX_STREAM_KEYED_PAYLOAD_BYTES];
    size_t length = 0U;
    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = stream_marker;
    payload.idempotency_key[0] = key_marker;
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(LX_STREAM_SETTLE, bytes, length, principal, timestamp, sequence,
              module_result) == LXP_OK);
    return 0;
}

static int open_stream(uint8_t marker, lx_account *custody, uint64_t funding,
                       uint64_t rate, uint64_t rate_unit, uint64_t cap,
                       bool metered, uint64_t timestamp, uint64_t sequence,
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
    payload.record.mode = metered ? LX_STREAM_MODE_METERED :
                                    LX_STREAM_MODE_TIME;
    payload.record.rate = (lxp_u128){ 0U, rate };
    payload.record.rate_unit = rate_unit;
    payload.record.start_timestamp = 1000U;
    payload.record.total_cap = (lxp_u128){ 0U, cap };
    payload.initial_funding = (lxp_u128){ 0U, funding };
    if (metered) {
        (void)memcpy(payload.record.meter_authorities[0], meter_key, 32U);
        payload.record.meter_authority_count = 1U;
    }
    CHECK(lx_stream_open_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(LX_STREAM_OPEN, bytes, length, payer->id, timestamp, sequence,
              module_result) == LXP_OK);
    return 0;
}

static int keyed_payload_codec(void)
{
    lx_stream_keyed_payload payload;
    lx_stream_keyed_payload decoded;
    uint8_t bytes[LX_STREAM_KEYED_PAYLOAD_BYTES];
    size_t length = 0U;

    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = 1U;
    payload.idempotency_key[0] = 0xA1U;
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(length == (size_t)LX_STREAM_KEYED_PAYLOAD_BYTES);
    CHECK(lx_stream_keyed_decode(bytes, length, &decoded) == LXP_OK);
    CHECK(memcmp(&payload, &decoded, sizeof(payload)) == 0);
    CHECK(lx_stream_keyed_encode(&payload, bytes, length - 1U, &length) ==
          LXP_ERR_LENGTH_LIMIT);
    CHECK(lx_stream_keyed_encode(NULL, bytes, sizeof(bytes), &length) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), NULL) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(lx_stream_keyed_decode(bytes, length - 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_decode(bytes, length + 1U, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_decode(NULL, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_decode(bytes, length, NULL) ==
          LXP_ERR_NON_CANONICAL);
    bytes[1] = 2U;
    CHECK(lx_stream_keyed_decode(bytes, length, &decoded) ==
          LXP_ERR_VERSION_UNSUPPORTED);
    bytes[1] = (uint8_t)LX_STREAM_PAYLOAD_VERSION;
    (void)memset(bytes + 34U, 0, 32U);
    CHECK(lx_stream_keyed_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    (void)memset(bytes + 2U, 0, 32U);
    CHECK(lx_stream_keyed_decode(bytes, length, &decoded) ==
          LXP_ERR_NON_CANONICAL);

    /* The idempotency key is the only thing that maps a replay onto a
     * stored economic result, so it can never be the state key of a
     * stream record. */
    {
        uint8_t stream_key[LX_STREAM_KEY_BYTES];
        uint8_t result_key[LX_STREAM_KEY_BYTES];
        uint8_t id[32] = { 0U };
        id[0] = 1U;
        CHECK(lx_stream_state_key(id, stream_key) == LXP_OK);
        CHECK(lx_stream_result_key(id, result_key) == LXP_OK);
        CHECK(memcmp(stream_key, result_key, sizeof(stream_key)) != 0);
        CHECK(memcmp(result_key, "result:", 7U) == 0);
        (void)memset(id, 0, sizeof(id));
        CHECK(lx_stream_result_key(id, result_key) == LXP_ERR_NON_CANONICAL);
        CHECK(lx_stream_state_key(id, stream_key) == LXP_ERR_NON_CANONICAL);
    }
    return 0;
}

static int settle_flow(void)
{
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_receipt replayed;
    uint8_t first_root[32];
    uint8_t id[32] = { 0U };
    lxp_result module_result = LXP_OK;
    bool found = false;
    id[0] = 1U;

    if (open_stream(1U, stream_one, 40U, 10U, 1000U, 500U, false, 1000U, 1U,
                    &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 60U && stream_one->balance.lo == 40U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* A settle with no elapsed time draws nothing and refuses no one. */
    if (settle(1U, 0x10U, 1000U, 2U, provider->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(stream_one->balance.lo == 40U && lxp_u128_is_zero(provider->balance));
    CHECK(lx_stream_result_load(&ctx, (const uint8_t[32]){ 0x10U }, &result,
                                &found) == LXP_OK);
    CHECK(found && result.ordinal == 4U && result.leg_count == 0U);
    CHECK(lxp_u128_is_zero(result.paid));
    CHECK(lxp_ct_is_zero(result.transfer_set_root, 32U));
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(!record.underfunded && record.last_accrual_timestamp == 1000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Two seconds of a ten-per-second stream pays twenty. */
    if (settle(1U, 0x11U, 3000U, 3U, provider->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 1U);
    CHECK(observed.reason == (uint16_t)LXP_REASON_STREAM_DRAW);
    CHECK(observed.authority_count == 1U);
    CHECK(observed.authority_kind == LXP_AUTH_PROTOCOL_MODULE);
    CHECK(memcmp(observed.authorized_from, stream_one->id, 32U) == 0);
    CHECK(observed.origin_module_id == (uint16_t)LXP_MODULE_STREAM);
    CHECK(observed.actor_sequence == 0U);
    CHECK(stream_one->balance.lo == 20U && provider->balance.lo == 20U);
    CHECK(stream_one->next_sequence == 1U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 4U);
    CHECK(effects.effects[0].body_length == 65U);
    CHECK(effects.effects[0].body[64] == 0U);
    (void)memcpy(first_root, observed.transfer_set_root, 32U);
    CHECK(lx_stream_result_load(&ctx, (const uint8_t[32]){ 0x11U }, &result,
                                &found) == LXP_OK);
    CHECK(found && result.ordinal == 4U && result.leg_count == 1U);
    CHECK(result.paid.lo == 20U && lxp_u128_is_zero(result.refunded));
    CHECK(memcmp(result.transfer_set_root, first_root, 32U) == 0);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 20U && record.settled_total.lo == 20U);
    CHECK(record.last_accrual_timestamp == 3000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Replaying the same idempotency key moves no value and reproduces the
     * original receipt byte for byte. */
    if (settle(1U, 0x11U, 4000U, 4U, provider->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(stream_one->balance.lo == 20U && provider->balance.lo == 20U);
    CHECK(stream_one->next_sequence == 1U);
    CHECK(ctx.staged_count == 0U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 4U);
    CHECK(lx_stream_result_load(&ctx, (const uint8_t[32]){ 0x11U }, &result,
                                &found) == LXP_OK);
    CHECK(found);
    CHECK(lx_stream_result_receipt(&result, &replayed) == LXP_OK);
    CHECK(memcmp(replayed.transfer_set_root, first_root, 32U) == 0);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 20U && record.last_accrual_timestamp ==
          3000U);

    if (settle(2U, 0x11U, 4000U, 4U, provider->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_ERR_CONTEXT_MISMATCH);
    CHECK(observed.calls == 0U);
    CHECK(stream_one->balance.lo == 20U && provider->balance.lo == 20U);

    /* The second settle exhausts custody exactly. */
    if (settle(1U, 0x12U, 5000U, 5U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.actor_sequence == 1U);
    CHECK(stream_one->balance.lo == 0U && provider->balance.lo == 40U);
    CHECK(stream_one->next_sequence == 2U);
    CHECK(effects.effects[0].body[64] == 0U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 40U && record.settled_total.lo == 40U);
    CHECK(!record.underfunded);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* With nothing left to draw the stream records the shortfall instead of
     * accruing a debt it can not pay. */
    if (settle(1U, 0x13U, 7000U, 6U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(stream_one->balance.lo == 0U && provider->balance.lo == 40U);
    CHECK(effects.effects[0].body[64] == 1U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.underfunded);
    CHECK(record.accrued_total.lo == 40U && record.settled_total.lo == 40U);
    CHECK(record.last_accrual_timestamp == 7000U);
    CHECK(lxp_u128_is_zero(record.remainder_carry));
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* An underfunded stream holds its clock until it is funded again. */
    if (settle(1U, 0x14U, 7500U, 7U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 40U && record.underfunded);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    {
        lx_stream_amount_payload amount;
        uint8_t bytes[LX_STREAM_TOP_UP_PAYLOAD_BYTES];
        size_t length = 0U;
        (void)memset(&amount, 0, sizeof(amount));
        amount.stream_id[0] = 1U;
        amount.amount = (lxp_u128){ 0U, 30U };
        CHECK(lx_stream_amount_encode(&amount, bytes, sizeof(bytes),
                                      &length) == LXP_OK);
        CHECK(run(LX_STREAM_TOP_UP, bytes, length, payer->id, 8000U, 8U,
                  &module_result) == LXP_OK);
    }
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 30U && stream_one->balance.lo == 30U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 2U);
    CHECK(effects.effects[0].body[48] == 1U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(!record.underfunded && record.last_accrual_timestamp == 8000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Funding restarts the clock from the moment of the top up, never from
     * the moment the stream ran dry. */
    if (settle(1U, 0x15U, 9000U, 9U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U);
    CHECK(stream_one->balance.lo == 20U && provider->balance.lo == 50U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 50U && record.settled_total.lo == 50U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* A partial draw pays what custody holds and marks the rest unfunded
     * rather than leaving an unpayable accrual on the record. */
    if (settle(1U, 0x16U, 12000U, 10U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 1U);
    CHECK(stream_one->balance.lo == 0U && provider->balance.lo == 70U);
    CHECK(effects.effects[0].body[64] == 1U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.underfunded);
    CHECK(record.accrued_total.lo == 70U && record.settled_total.lo == 70U);
    CHECK(record.last_accrual_timestamp == 12000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);
    CHECK(payer->balance.lo == 30U);
    return 0;
}

static int settle_refusals(void)
{
    lx_stream_keyed_payload payload;
    uint8_t bytes[LX_STREAM_KEYED_PAYLOAD_BYTES];
    size_t length = 0U;
    lxp_result module_result = LXP_OK;

    /* Neither party to the stream is a bystander. */
    if (settle(1U, 0x17U, 13000U, 11U, stream_one->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_ERR_UNAUTHORIZED_DEBIT);
    CHECK(observed.calls == 0U);

    /* Settling an unknown stream refuses before any ledger contact. */
    if (settle(9U, 0x18U, 13000U, 11U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_ERR_UNKNOWN_FIELD);

    /* A settle can not walk the stream clock backwards. */
    if (settle(1U, 0x19U, 11000U, 11U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_ERR_NON_MONOTONIC_TIME);
    CHECK(observed.calls == 0U);

    /* An asset the host withdrew stops the stream. */
    asset_state.paused = true;
    if (settle(1U, 0x1AU, 13000U, 11U, payer->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_ERR_ASSET_PAUSED);
    asset_state.paused = false;

    /* A settle payload carrying the wrong ordinal never reaches execute. */
    (void)memset(&payload, 0, sizeof(payload));
    payload.stream_id[0] = 1U;
    payload.idempotency_key[0] = 0x1BU;
    CHECK(lx_stream_keyed_encode(&payload, bytes, sizeof(bytes), &length) ==
          LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 13000U, 11U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_ERR_NON_CANONICAL);
    CHECK(observed.calls == 0U);
    return 0;
}

static int metered_settle(void)
{
    static const uint8_t seed[32] = { 11U };
    static const uint8_t other[32] = { 12U };
    lx_stream_meter_attestation attestation;
    lx_stream_record record;
    uint8_t bytes[LX_STREAM_METER_PAYLOAD_BYTES];
    uint8_t id[32] = { 0U };
    uint8_t reading[8];
    uint8_t amount[16];
    size_t length = 0U;
    size_t i;
    lxp_result module_result = LXP_OK;
    id[0] = 2U;

    if (open_stream(2U, stream_two, 20U, 5U, 100U, 100U, true, 13000U, 12U,
                    &module_result) != 0) return 1;
    CHECK(module_result == LXP_OK);
    CHECK(payer->balance.lo == 10U && stream_two->balance.lo == 20U);
    CHECK(effects.effects[0].body[160] == (uint8_t)LX_STREAM_MODE_METERED);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* A signed reading is the only thing that moves a metered stream. */
    (void)memset(&attestation, 0, sizeof(attestation));
    attestation.stream_id[0] = 2U;
    attestation.cumulative_reading = 300U;
    CHECK(sign_attestation(&attestation, seed) == 0);
    CHECK(memcmp(attestation.authority_key, meter_key, 32U) == 0);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 13500U, 13U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 0U);
    CHECK(effects.count == 1U);
    CHECK(effects.effects[0].event_type == 3U);
    CHECK(effects.effects[0].body_length == 72U);
    CHECK(memcmp(effects.effects[0].body, attestation.stream_id, 32U) == 0);
    for (i = 0U; i < 8U; ++i)
        reading[i] = (uint8_t)(attestation.cumulative_reading >>
                               ((7U - i) * 8U));
    CHECK(memcmp(effects.effects[0].body + 32U, reading, 8U) == 0);
    CHECK(lxp_u128_to_be((lxp_u128){ 0U, 15U }, amount) == LXP_OK);
    CHECK(memcmp(effects.effects[0].body + 40U, amount, 16U) == 0);
    CHECK(memcmp(effects.effects[0].body + 56U, amount, 16U) == 0);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.cumulative_meter == 300U && record.accrued_total.lo == 15U);
    CHECK(lxp_u128_is_zero(record.remainder_carry));
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Replaying the same reading accrues nothing. */
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 13600U, 14U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_OK);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.cumulative_meter == 300U && record.accrued_total.lo == 15U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* Time never accrues on a metered stream. */
    if (settle(2U, 0x20U, 20000U, 15U, provider->id, &module_result) != 0)
        return 1;
    CHECK(module_result == LXP_OK);
    CHECK(observed.calls == 1U && observed.leg_count == 1U);
    CHECK(observed.reason == (uint16_t)LXP_REASON_STREAM_DRAW);
    CHECK(memcmp(observed.authorized_from, stream_two->id, 32U) == 0);
    CHECK(stream_two->balance.lo == 5U && provider->balance.lo == 85U);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.accrued_total.lo == 15U && record.settled_total.lo == 15U);
    CHECK(record.last_accrual_timestamp == 1000U);
    CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);

    /* A reading below the recorded meter is a refusal, not a credit. */
    attestation.cumulative_reading = 200U;
    CHECK(sign_attestation(&attestation, seed) == 0);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 21000U, 16U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_ERR_METER_REGRESSION);
    CHECK(lx_stream_load(&ctx, id, &record) == LXP_OK);
    CHECK(record.cumulative_meter == 300U);

    /* A key the record does not list can not meter the stream. */
    attestation.cumulative_reading = 400U;
    CHECK(sign_attestation(&attestation, other) == 0);
    CHECK(memcmp(attestation.authority_key, meter_key, 32U) != 0);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 21000U, 16U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_ERR_UNAUTHORIZED_METER);

    /* A listed key with a broken signature is refused as well. */
    attestation.cumulative_reading = 400U;
    CHECK(sign_attestation(&attestation, seed) == 0);
    attestation.signature[0] = (uint8_t)(attestation.signature[0] ^ 1U);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 21000U, 16U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_ERR_UNAUTHORIZED_METER);

    /* A meter attestation for a time stream is refused outright. */
    attestation.stream_id[0] = 1U;
    attestation.cumulative_reading = 400U;
    CHECK(sign_attestation(&attestation, seed) == 0);
    CHECK(lx_stream_meter_encode(&attestation, bytes, sizeof(bytes),
                                 &length) == LXP_OK);
    CHECK(run(LX_STREAM_METER, bytes, length, payer->id, 21000U, 16U,
              &module_result) == LXP_OK);
    CHECK(module_result == LXP_ERR_NON_CANONICAL);
    CHECK(observed.calls == 0U);
    return 0;
}

static lxp_result count_streams(const lx_stream_record *record, void *user)
{
    size_t *count = (size_t *)user;
    if (record == NULL) return LXP_ERR_NON_CANONICAL;
    ++(*count);
    return LXP_OK;
}

static int state_shape(void)
{
    size_t count = 0U;
    lxp_u128 total;
    CHECK(lx_stream_iter(&ctx, count_streams, &count) == LXP_OK);
    CHECK(count == 2U);
    /* Every unit funded into the module is still accounted for. */
    CHECK(lxp_u128_add(payer->balance, provider->balance, &total) == LXP_OK);
    CHECK(lxp_u128_add(total, stream_one->balance, &total) == LXP_OK);
    CHECK(lxp_u128_add(total, stream_two->balance, &total) == LXP_OK);
    CHECK(total.hi == 0U && total.lo == 100U);
    return 0;
}

int main(void)
{
    if (fixture_init() != 0) return 1;
    if (keyed_payload_codec() != 0) return 1;
    if (settle_flow() != 0) return 1;
    if (settle_refusals() != 0) return 1;
    if (metered_settle() != 0) return 1;
    if (state_shape() != 0) return 1;
    if (lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
}
