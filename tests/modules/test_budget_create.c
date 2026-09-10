#include "layerx/lx_asset.h"
#include "layerx/lx_budget.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state_proof.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x) do { \
    if (!(x)) { fprintf(stderr, "%s line %d\n", __func__, __LINE__); \
                return 1; } \
} while (0)

static size_t legs;
static uint16_t reason;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    legs = set->leg_count;
    reason = set->legs[0].reason;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

static int store_path(void)
{
    lx_account owner;
    lx_account budget_account;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_budget_store store;
    lx_budget_fund_request request;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    (void)memset(&owner, 0, sizeof(owner));
    (void)memset(&budget_account, 0, sizeof(budget_account));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    owner.id[0] = 1U; owner.kind = LX_ACCOUNT_AGENT_MAIN;
    budget_account.id[0] = 2U; budget_account.kind = LX_ACCOUNT_AGENT_BUDGET;
    asset.asset_id[0] = 3U;
    if (lxp_ledger_bootstrap_balance(&owner, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&budget_account, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_budget_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.owner = &owner;
    request.budget_account = &budget_account;
    request.asset = &asset;
    request.amount = (lxp_u128){ 0U, 50U };
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = &owner;
    (void)memcpy(request.context.authorized_from, owner.id, 32U);
    request.record.budget_id[0] = 4U;
    (void)memcpy(request.record.owner, owner.id, 32U);
    (void)memcpy(request.record.budget_account, budget_account.id, 32U);
    (void)memcpy(request.record.asset_id, asset.asset_id, 32U);
    request.record.per_period_limit = (lxp_u128){ 0U, 100U };
    request.record.period_length = 1000U;
    request.record.period_start = 100U;
    request.record.rollover_policy = LX_BUDGET_ROLLOVER_CAPPED;
    request.record.carry_cap = (lxp_u128){ 0U, 20U };
    request.record.purpose_hash[0] = 5U;
    request.record.expiry = 10000U;
    request.record.revocation_sequence = 1U;
    store.count = LX_BUDGET_STORE_CAPACITY + 1U;
    if (lx_budget_state_put(&store, &request.record) !=
            LXP_ERR_NON_CANONICAL ||
        lx_budget_create_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NON_CANONICAL ||
        owner.balance.lo != 100U || !lxp_u128_is_zero(budget_account.balance))
        return 1;
    store.count = 0U;
    if (lx_budget_create_execute(&ctx, &request, &receipt) != LXP_OK ||
        legs != 1U || reason != LXP_REASON_BUDGET_FUND ||
        owner.balance.lo != 50U || budget_account.balance.lo != 50U ||
        store.count != 1U || store.records[0].per_period_limit.lo != 100U ||
        lx_budget_create_execute(&ctx, &request, &receipt) !=
            LXP_ERR_SEQUENCE_REUSED || owner.balance.lo != 50U)
        return 1;
    request.amount = (lxp_u128){ 0U, 10U };
    request.context.actor_sequence = owner.next_sequence;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 2U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    if (lx_budget_fund_execute(&ctx, &request, &receipt) != LXP_OK ||
        owner.balance.lo != 40U || budget_account.balance.lo != 60U ||
        store.records[0].per_period_limit.lo != 100U ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

static int sign_raw(const uint8_t seed[32], const uint8_t *message,
                    size_t message_length, uint8_t signature[64],
                    uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    size_t public_length = 32U;
    size_t signature_length = 64U;
    int ok = key != NULL && context != NULL &&
             EVP_PKEY_get_raw_public_key(key, public_key, &public_length) == 1 &&
             public_length == 32U &&
             EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
             EVP_DigestSign(context, signature, &signature_length,
                            message, message_length) == 1 &&
             signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

enum {
    BUDGET_TAMPER_NONE = 0,
    BUDGET_TAMPER_SIGNATURE = 1,
    BUDGET_TAMPER_PAYLOAD = 2,
    BUDGET_TAMPER_AUTHORITY_KIND = 3,
    BUDGET_TAMPER_VERIFIED_KEY = 4,
    BUDGET_TAMPER_ACTOR = 5,
    BUDGET_TAMPER_TRUNCATE = 6
};

static const char owner_did[] = "did:key:owner";
static const char delegate_did[] = "did:key:dele";
static const char owner_name[] = "agent:did:key:owner:main";
static const char budget_name[] = "agent:did:key:owner:budget:b1";
static const char recipient_name[] = "agent:did:key:recv:main";
static const char delegate_name[] = "agent:did:key:dele:main";
static const char foreign_name[] = "agent:did:key:recv:budget:x";
static const uint8_t owner_seed[32] = { 11U };
static const uint8_t delegate_seed[32] = { 22U };

typedef struct budget_env {
    lx_account_registry accounts;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_asset_runtime asset_runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    const lxp_module_registration *registration;
    lx_account *owner;
    lx_account *budget_account;
    lx_account *recipient;
    lx_account *delegate;
    lx_account *foreign;
    uint64_t parameters;
    uint64_t global_sequence;
} budget_env;

typedef struct budget_call {
    uint32_t activity_type;
    const char *did;
    const uint8_t *seed;
    const uint8_t *payload;
    size_t payload_length;
    uint64_t sequence;
    const uint8_t *principal;
    uint64_t timestamp;
    unsigned tamper;
} budget_call;

static void put_u64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static void put_u128(uint8_t *bytes, uint64_t high, uint64_t low)
{
    put_u64(bytes, high);
    put_u64(bytes + 8U, low);
}

static budget_env env;

static int open_account(const char *name, uint64_t balance,
                        const uint8_t *authority_key, lx_account **account)
{
    uint8_t id[32];
    size_t length = strlen(name);
    if (lx_account_id_from_string((const uint8_t *)name, length, id) != LXP_OK ||
        lx_account_open(&env.accounts, (const uint8_t *)name, length, id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, account) != LXP_OK ||
        lxp_ledger_bootstrap_balance(*account, env.asset.asset_id,
                                     (lxp_u128){ 0U, balance }, 0U) != LXP_OK)
        return 1;
    if (authority_key != NULL) {
        (void)memcpy((*account)->authority_key, authority_key, 32U);
        (*account)->has_authority_key = true;
    }
    return lx_account_validate_canonical(*account) == LXP_OK ? 0 : 1;
}

static int env_init(void)
{
    uint8_t owner_key[32];
    uint8_t delegate_key[32];
    uint8_t signature[64];
    (void)memset(&env, 0, sizeof(env));
    env.parameters = 1U;
    env.asset.asset_id[0] = 0x5AU;
    env.asset.asset_id[31] = 0xC3U;
    CHECK(sign_raw(owner_seed, env.asset.asset_id, 0U, signature,
                   owner_key) == 0);
    CHECK(sign_raw(delegate_seed, env.asset.asset_id, 0U, signature,
                   delegate_key) == 0);
    CHECK(lx_asset_transfer_state(&env.asset, &env.asset_state) == LXP_OK);
    CHECK(lx_account_registry_init(&env.accounts) == LXP_OK);
    CHECK(open_account(owner_name, 1000U, owner_key, &env.owner) == 0);
    CHECK(open_account(budget_name, 0U, NULL, &env.budget_account) == 0);
    CHECK(open_account(recipient_name, 0U, NULL, &env.recipient) == 0);
    CHECK(open_account(delegate_name, 0U, delegate_key, &env.delegate) == 0);
    CHECK(open_account(foreign_name, 0U, NULL, &env.foreign) == 0);
    CHECK(env.owner->kind == LX_ACCOUNT_AGENT_MAIN);
    CHECK(env.budget_account->kind == LX_ACCOUNT_AGENT_BUDGET);
    CHECK(env.foreign->kind == LX_ACCOUNT_AGENT_BUDGET);
    CHECK(lxp_state_store_init(&env.state, 0U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(&env.state, &env.accounts) == LXP_OK);
    CHECK(lxp_state_store_require_account_root(&env.state) == LXP_OK);
    CHECK(lxp_kernel_create(&env.kernel, &env.state, &env.journal,
                            &env.parameters, 0U) == LXP_OK);
    CHECK(lxp_kernel_register_module(&env.kernel,
                                     lx_budget_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(
              &env.kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    env.asset_runtime.accounts = &env.accounts;
    env.asset_runtime.assets = &env.asset;
    env.asset_runtime.asset_count = 1U;
    env.asset_runtime.transfer_assets = &env.asset_state;
    env.asset_runtime.transfer_asset_count = 1U;
    env.asset_runtime.network_id = 7U;
    env.asset_runtime.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    CHECK(lxp_kernel_bind_module_runtime(&env.kernel, LXP_MODULE_ASSET,
                                         &env.asset_runtime) == LXP_OK);
    CHECK(lxp_kernel_module_for_activity(&env.kernel, LX_BUDGET_CREATE, 0U,
                                         &env.registration) == LXP_OK);
    return 0;
}

static int submit(const budget_call *call, lxp_effect_buffer *effects,
                  lxp_result *result)
{
    static uint8_t arena_bytes[262144];
    static uint8_t wire_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t payload_copy[256];
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t principal[32];
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t digest[32];
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_arena wire_arena;
    lxp_byte_span wire;
    size_t did_length = strlen(call->did);
    size_t length = call->payload_length;
    CHECK(length <= sizeof(payload_copy));
    (void)memcpy(payload_copy, call->payload, length);
    if (call->tamper == BUDGET_TAMPER_TRUNCATE) {
        CHECK(length != 0U);
        --length;
    }
    if (call->principal != NULL) {
        (void)memcpy(principal, call->principal, 32U);
    } else {
        CHECK(11U + did_length <= sizeof(name));
        (void)memcpy(name, "agent:", 6U);
        (void)memcpy(name + 6U, call->did, did_length);
        (void)memcpy(name + 6U + did_length, ":main", 5U);
        CHECK(lx_account_id_from_string(name, 11U + did_length,
                                        principal) == LXP_OK);
    }
    CHECK(sign_raw(call->seed, payload_copy, 0U, signature, public_key) == 0);
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    activity.network_id = 7U;
    activity.activity_type = call->activity_type;
    activity.actor_did.bytes = (const uint8_t *)call->did;
    activity.actor_did.length = did_length;
    activity.authority.bytes = public_key;
    activity.authority.length = 32U;
    activity.signature.bytes = signature;
    activity.signature.length = 64U;
    activity.payload.bytes = payload_copy;
    activity.payload.length = length;
    activity.account_sequence = call->sequence;
    activity.idempotency_key[0] = (uint8_t)(env.global_sequence + 1U);
    activity.idempotency_key[1] = (uint8_t)call->activity_type;
    activity.fee_limit.lo = 1000000U;
    activity.timestamp_bound.not_after = call->timestamp + 1000U;
    CHECK(lxp_hash_payload(payload_copy, length,
                           activity.payload_hash) == LXP_OK);
    CHECK(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    CHECK(sign_raw(call->seed, digest, 32U, signature, public_key) == 0);
    if (call->tamper == BUDGET_TAMPER_SIGNATURE) signature[0] ^= 1U;
    if (call->tamper == BUDGET_TAMPER_PAYLOAD) {
        CHECK(length != 0U);
        payload_copy[length - 1U] ^= 1U;
    }
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = call->tamper == BUDGET_TAMPER_AUTHORITY_KIND ?
        LXP_AUTHORITY_SESSION_KEY : LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, public_key, 32U);
    if (call->tamper == BUDGET_TAMPER_VERIFIED_KEY)
        authority.verified_key[0] ^= 1U;
    CHECK(lxp_did_id_derive(activity.actor_did.bytes, did_length,
                            authority.actor) == LXP_OK);
    if (call->tamper == BUDGET_TAMPER_ACTOR) authority.actor[0] ^= 1U;
    (void)memcpy(authority.principal, principal, 32U);
    CHECK(lxp_arena_init(&wire_arena, wire_bytes,
                         sizeof(wire_bytes)) == LXP_OK);
    CHECK(lxp_activity_encode(&activity, &wire_arena, &wire) == LXP_OK);
    CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    CHECK(lxp_effect_buffer_init(effects) == LXP_OK);
    ++env.global_sequence;
    CHECK(lxp_module_ctx_init(&ctx, &env.kernel, LXP_MODULE_BUDGET,
                              call->timestamp, 0U, env.global_sequence,
                              1000000U, &arena, true) == LXP_OK);
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    ctx.batch_number = env.global_sequence;
    CHECK(lxp_activity_id(wire.bytes, wire.length, ctx.activity_id) == LXP_OK);
    CHECK(lxp_module_ctx_bind_effects(&ctx, effects) == LXP_OK);
    CHECK(lxp_kernel_dispatch(env.registration, &ctx, &activity, &authority,
                              effects, result) == LXP_OK);
    if (*result == LXP_OK) CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);
    return 0;
}

static int read_record(const uint8_t budget_id[32], lx_budget_record *record)
{
    uint8_t key[LX_BUDGET_STATE_KEY_BYTES];
    uint8_t root[32];
    lxp_byte_span span;
    lxp_state_witness *proof = (lxp_state_witness *)malloc(sizeof(*proof));
    int failed;
    CHECK(proof != NULL);
    span.bytes = key;
    span.length = sizeof(key);
    failed = lx_budget_state_key(budget_id, key) != LXP_OK ||
             lxp_state_root(&env.kernel, root) != LXP_OK ||
             lxp_state_proof_build(&env.kernel, LXP_MODULE_BUDGET, span,
                                   proof) != LXP_OK ||
             lxp_state_proof_verify(proof, root) != LXP_OK ||
             proof->key_length != (uint32_t)sizeof(key) ||
             memcmp(proof->key, key, sizeof(key)) != 0 ||
             lx_budget_record_decode(proof->value, proof->value_length,
                                     record) != LXP_OK;
    free(proof);
    CHECK(failed == 0);
    return 0;
}

static int expect_event(const lxp_effect_buffer *effects, uint16_t event_type,
                        const uint8_t *body, size_t body_length)
{
    CHECK(effects->count == 1U);
    CHECK(effects->effects[0].module_id == LXP_MODULE_BUDGET);
    CHECK(effects->effects[0].kind == LXP_EFFECT_EVENT);
    CHECK(effects->effects[0].event_type == event_type);
    CHECK((size_t)effects->effects[0].body_length == body_length);
    CHECK(memcmp(effects->effects[0].body, body, body_length) == 0);
    return 0;
}

static int dispatch_path(void)
{
    static const uint8_t budget_id[32] = { 0x0BU, 0x1DU, 0x9EU };
    static const uint8_t missing_id[32] = { 0xEEU };
    uint8_t create[LX_BUDGET_CREATE_PAYLOAD_BYTES];
    uint8_t fund[LX_BUDGET_FUND_PAYLOAD_BYTES];
    uint8_t amend[LX_BUDGET_AMEND_PAYLOAD_BYTES];
    uint8_t delegate[LX_BUDGET_DELEGATE_PAYLOAD_BYTES];
    uint8_t spend[LX_BUDGET_SPEND_PAYLOAD_BYTES];
    uint8_t close[LX_BUDGET_CLOSE_PAYLOAD_BYTES];
    uint8_t scratch[LX_BUDGET_CREATE_PAYLOAD_BYTES];
    uint8_t expected[80];
    uint8_t delegate_id[32];
    uint8_t before[32];
    uint8_t after[32];
    lx_budget_record record;
    lxp_effect_buffer effects;
    budget_call call;
    lxp_result result = LXP_OK;
    size_t i;

    CHECK(env_init() == 0);
    CHECK(lxp_did_id_derive((const uint8_t *)delegate_did,
                            strlen(delegate_did), delegate_id) == LXP_OK);

    (void)memset(create, 0, sizeof(create));
    create[1] = 1U;
    (void)memcpy(create + 2U, budget_id, 32U);
    (void)memcpy(create + 34U, env.budget_account->id, 32U);
    (void)memcpy(create + 66U, env.asset.asset_id, 32U);
    create[98] = 0x77U;
    put_u128(create + 130U, 0U, 200U);
    put_u128(create + 146U, 0U, 50U);
    put_u128(create + 162U, 0U, 300U);
    put_u64(create + 178U, 1000U);
    put_u64(create + 186U, 0U);
    put_u64(create + 194U, 1000000U);
    put_u64(create + 202U, 1U);
    create[210] = (uint8_t)LX_BUDGET_ROLLOVER_CAPPED;

    (void)memset(fund, 0, sizeof(fund));
    fund[1] = 1U;
    (void)memcpy(fund + 2U, budget_id, 32U);
    put_u128(fund + 34U, 0U, 100U);

    (void)memset(amend, 0, sizeof(amend));
    amend[1] = 1U;
    (void)memcpy(amend + 2U, budget_id, 32U);
    put_u128(amend + 34U, 0U, 500U);
    put_u128(amend + 50U, 0U, 50U);
    put_u64(amend + 66U, 1000000U);
    amend[74] = (uint8_t)LX_BUDGET_ROLLOVER_CAPPED;

    (void)memset(delegate, 0, sizeof(delegate));
    delegate[1] = 1U;
    (void)memcpy(delegate + 2U, budget_id, 32U);
    (void)memcpy(delegate + 34U, delegate_id, 32U);

    (void)memset(spend, 0, sizeof(spend));
    spend[1] = 1U;
    (void)memcpy(spend + 2U, budget_id, 32U);
    (void)memcpy(spend + 34U, env.recipient->id, 32U);
    put_u128(spend + 66U, 0U, 120U);

    (void)memset(close, 0, sizeof(close));
    close[1] = 1U;
    (void)memcpy(close + 2U, budget_id, 32U);
    put_u64(close + 34U, 2U);

    (void)memset(&call, 0, sizeof(call));
    call.did = owner_did;
    call.seed = owner_seed;
    call.timestamp = 500U;

    /* Create binds the record, funds the budget account and emits the
     * creation event. */
    call.activity_type = LX_BUDGET_CREATE;
    call.payload = create;
    call.payload_length = sizeof(create);
    call.sequence = 0U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.owner->balance.lo == 700U);
    CHECK(env.budget_account->balance.lo == 300U);
    CHECK(env.owner->next_sequence == 1U);
    (void)memcpy(expected, budget_id, 32U);
    (void)memcpy(expected + 32U, env.asset.asset_id, 32U);
    put_u128(expected + 64U, 0U, 300U);
    CHECK(expect_event(&effects, 1U, expected, 80U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(memcmp(record.owner, env.owner->id, 32U) == 0);
    CHECK(memcmp(record.budget_account, env.budget_account->id, 32U) == 0);
    CHECK(memcmp(record.asset_id, env.asset.asset_id, 32U) == 0);
    CHECK(record.per_period_limit.lo == 200U);
    CHECK(record.configured_period_limit.lo == 200U);
    CHECK(record.carry_cap.lo == 50U);
    CHECK(record.period_length == 1000U && record.period_start == 0U);
    CHECK(record.expiry == 1000000U && record.revocation_sequence == 1U);
    CHECK(record.rollover_policy == LX_BUDGET_ROLLOVER_CAPPED);
    CHECK(record.delegate_count == 0U && !record.closed && !record.revoked);
    CHECK(lxp_u128_is_zero(record.spent_this_period));

    /* Every envelope, authority and payload refusal leaves the ledger and
     * the module subtree untouched. */
    CHECK(lxp_state_root(&env.kernel, before) == LXP_OK);
    for (i = 0U; i < 18U; ++i) {
        budget_call refuse = call;
        lxp_result expected_result = LXP_OK;
        refuse.activity_type = LX_BUDGET_FUND;
        refuse.payload = fund;
        refuse.payload_length = sizeof(fund);
        refuse.sequence = 1U;
        switch (i) {
        case 0U:
            refuse.tamper = BUDGET_TAMPER_SIGNATURE;
            expected_result = LXP_ERR_BAD_SIGNATURE;
            break;
        case 1U:
            refuse.tamper = BUDGET_TAMPER_PAYLOAD;
            expected_result = LXP_ERR_PAYLOAD_HASH_MISMATCH;
            break;
        case 2U:
            refuse.tamper = BUDGET_TAMPER_AUTHORITY_KIND;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 3U:
            refuse.tamper = BUDGET_TAMPER_VERIFIED_KEY;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 4U:
            refuse.tamper = BUDGET_TAMPER_ACTOR;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 5U:
            refuse.tamper = BUDGET_TAMPER_TRUNCATE;
            expected_result = LXP_ERR_NON_CANONICAL;
            break;
        case 6U:
            refuse.sequence = 0U;
            expected_result = LXP_ERR_SEQUENCE_REUSED;
            break;
        case 7U:
            refuse.sequence = 99U;
            expected_result = LXP_ERR_SEQUENCE_GAP;
            break;
        case 8U:
            refuse.principal = env.recipient->id;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 9U:
            refuse.principal = env.budget_account->id;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 10U:
            refuse.did = delegate_did;
            refuse.seed = delegate_seed;
            refuse.sequence = 0U;
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 11U:
            (void)memcpy(scratch, fund, sizeof(fund));
            (void)memcpy(scratch + 2U, missing_id, 32U);
            refuse.payload = scratch;
            expected_result = LXP_ERR_UNKNOWN_FIELD;
            break;
        case 12U:
            (void)memcpy(scratch, fund, sizeof(fund));
            put_u128(scratch + 34U, 0U, 0U);
            refuse.payload = scratch;
            expected_result = LXP_ERR_INVALID_AMOUNT;
            break;
        case 13U:
            (void)memcpy(scratch, create, sizeof(create));
            refuse.activity_type = LX_BUDGET_CREATE;
            refuse.payload = scratch;
            refuse.payload_length = sizeof(create);
            expected_result = LXP_ERR_SEQUENCE_REUSED;
            break;
        case 14U:
            (void)memcpy(scratch, create, sizeof(create));
            scratch[2] ^= 0xFFU;
            (void)memcpy(scratch + 34U, env.foreign->id, 32U);
            refuse.activity_type = LX_BUDGET_CREATE;
            refuse.payload = scratch;
            refuse.payload_length = sizeof(create);
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 15U:
            (void)memcpy(scratch, create, sizeof(create));
            scratch[2] ^= 0xFFU;
            (void)memcpy(scratch + 34U, env.owner->id, 32U);
            refuse.activity_type = LX_BUDGET_CREATE;
            refuse.payload = scratch;
            refuse.payload_length = sizeof(create);
            expected_result = LXP_ERR_UNAUTHORIZED_DEBIT;
            break;
        case 16U:
            (void)memcpy(scratch, spend, sizeof(spend));
            (void)memcpy(scratch + 34U, env.budget_account->id, 32U);
            refuse.activity_type = LX_BUDGET_SPEND;
            refuse.payload = scratch;
            refuse.payload_length = sizeof(spend);
            expected_result = LXP_ERR_NON_CANONICAL;
            break;
        default:
            (void)memcpy(scratch, close, sizeof(close));
            put_u64(scratch + 34U, 0U);
            refuse.activity_type = LX_BUDGET_CLOSE;
            refuse.payload = scratch;
            refuse.payload_length = sizeof(close);
            expected_result = LXP_ERR_NON_CANONICAL;
            break;
        }
        if (submit(&refuse, &effects, &result) != 0) {
            (void)fprintf(stderr, "refusal %zu setup\n", i);
            return 1;
        }
        if (result != expected_result) {
            (void)fprintf(stderr, "refusal %zu result %d\n", i, (int)result);
            return 1;
        }
        CHECK(effects.count == 0U);
        CHECK(env.owner->balance.lo == 700U);
        CHECK(env.budget_account->balance.lo == 300U);
        CHECK(env.owner->next_sequence == 1U);
        CHECK(lxp_state_root(&env.kernel, after) == LXP_OK);
        CHECK(memcmp(before, after, 32U) == 0);
    }

    /* Fund adds to the budget account without touching the allowance. */
    call.activity_type = LX_BUDGET_FUND;
    call.payload = fund;
    call.payload_length = sizeof(fund);
    call.sequence = 1U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.owner->balance.lo == 600U);
    CHECK(env.budget_account->balance.lo == 400U);
    CHECK(env.owner->next_sequence == 2U);
    (void)memcpy(expected, budget_id, 32U);
    put_u128(expected + 32U, 0U, 100U);
    CHECK(expect_event(&effects, 2U, expected, 48U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.per_period_limit.lo == 200U);

    /* Amend raises the configured allowance without moving value. */
    call.activity_type = LX_BUDGET_AMEND;
    call.payload = amend;
    call.payload_length = sizeof(amend);
    call.sequence = 2U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.owner->balance.lo == 600U);
    CHECK(env.owner->next_sequence == 2U);
    (void)memcpy(expected, budget_id, 32U);
    put_u128(expected + 32U, 0U, 500U);
    put_u64(expected + 48U, 1000000U);
    CHECK(expect_event(&effects, 3U, expected, 56U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.per_period_limit.lo == 500U);
    CHECK(record.configured_period_limit.lo == 500U);

    /* Delegate add records the delegate did id in the canonical record. */
    call.activity_type = LX_BUDGET_DELEGATE_ADD;
    call.payload = delegate;
    call.payload_length = sizeof(delegate);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    (void)memcpy(expected, budget_id, 32U);
    (void)memcpy(expected + 32U, delegate_id, 32U);
    CHECK(expect_event(&effects, 4U, expected, 64U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.delegate_count == 1U);
    CHECK(memcmp(record.delegates[0], delegate_id, 32U) == 0);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_SEQUENCE_REUSED);

    /* The owner spends against the allowance. */
    call.activity_type = LX_BUDGET_SPEND;
    call.payload = spend;
    call.payload_length = sizeof(spend);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.budget_account->balance.lo == 280U);
    CHECK(env.recipient->balance.lo == 120U);
    CHECK(env.owner->next_sequence == 3U);
    (void)memcpy(expected, budget_id, 32U);
    (void)memcpy(expected + 32U, env.recipient->id, 32U);
    put_u128(expected + 64U, 0U, 120U);
    CHECK(expect_event(&effects, 6U, expected, 80U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.spent_this_period.lo == 120U);

    /* The registered delegate spends on its own account sequence. */
    (void)memcpy(scratch, spend, sizeof(spend));
    put_u128(scratch + 66U, 0U, 80U);
    call.did = delegate_did;
    call.seed = delegate_seed;
    call.payload = scratch;
    call.sequence = 0U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.budget_account->balance.lo == 200U);
    CHECK(env.recipient->balance.lo == 200U);
    CHECK(env.delegate->next_sequence == 1U);
    CHECK(env.owner->next_sequence == 3U);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.spent_this_period.lo == 200U);

    /* A capped rollover at the period boundary carries the unspent
     * allowance, bounded by the carry cap. */
    call.did = owner_did;
    call.seed = owner_seed;
    call.sequence = 3U;
    call.timestamp = 1500U;
    put_u128(scratch + 66U, 0U, 50U);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.budget_account->balance.lo == 150U);
    CHECK(env.recipient->balance.lo == 250U);
    CHECK(env.owner->next_sequence == 4U);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.period_start == 1000U);
    CHECK(record.carried.lo == 50U);
    CHECK(record.per_period_limit.lo == 550U);
    CHECK(record.spent_this_period.lo == 50U);

    /* Allowance and funding refusals are distinct and non-destructive. */
    call.sequence = 4U;
    put_u128(scratch + 66U, 0U, 600U);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_BUDGET_ALLOWANCE_EXCEEDED);
    CHECK(env.budget_account->balance.lo == 150U);
    put_u128(scratch + 66U, 0U, 200U);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_INSUFFICIENT_BUDGET_FUNDS);
    CHECK(env.budget_account->balance.lo == 150U);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.spent_this_period.lo == 50U);

    /* Removing the delegate revokes its ability to spend. */
    call.activity_type = LX_BUDGET_DELEGATE_REMOVE;
    call.payload = delegate;
    call.payload_length = sizeof(delegate);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    (void)memcpy(expected, budget_id, 32U);
    (void)memcpy(expected + 32U, delegate_id, 32U);
    CHECK(expect_event(&effects, 5U, expected, 64U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.delegate_count == 0U);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNAUTHORIZED_DELEGATE);
    (void)memcpy(scratch, spend, sizeof(spend));
    put_u128(scratch + 66U, 0U, 10U);
    call.activity_type = LX_BUDGET_SPEND;
    call.payload = scratch;
    call.payload_length = sizeof(spend);
    call.did = delegate_did;
    call.seed = delegate_seed;
    call.sequence = 1U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNAUTHORIZED_DELEGATE);
    CHECK(env.budget_account->balance.lo == 150U);

    /* Close refuses a stale revocation sequence and then drains the
     * remaining balance back to the owner. */
    call.did = owner_did;
    call.seed = owner_seed;
    call.sequence = 4U;
    call.activity_type = LX_BUDGET_CLOSE;
    (void)memcpy(scratch, close, sizeof(close));
    put_u64(scratch + 34U, 1U);
    call.payload = scratch;
    call.payload_length = sizeof(close);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_STALE_REVOCATION);
    CHECK(env.budget_account->balance.lo == 150U);
    call.payload = close;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_OK);
    CHECK(env.budget_account->balance.lo == 0U);
    CHECK(env.owner->balance.lo == 750U);
    CHECK(env.owner->next_sequence == 5U);
    (void)memcpy(expected, budget_id, 32U);
    put_u128(expected + 32U, 0U, 150U);
    put_u64(expected + 48U, 2U);
    CHECK(expect_event(&effects, 7U, expected, 56U) == 0);
    CHECK(read_record(budget_id, &record) == 0);
    CHECK(record.closed && record.revocation_sequence == 2U);

    /* A closed budget refuses every further ordinal. */
    call.sequence = 5U;
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNKNOWN_FIELD);
    call.activity_type = LX_BUDGET_FUND;
    call.payload = fund;
    call.payload_length = sizeof(fund);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNKNOWN_FIELD);
    call.activity_type = LX_BUDGET_AMEND;
    call.payload = amend;
    call.payload_length = sizeof(amend);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNKNOWN_FIELD);
    call.activity_type = LX_BUDGET_DELEGATE_ADD;
    call.payload = delegate;
    call.payload_length = sizeof(delegate);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNKNOWN_FIELD);
    call.activity_type = LX_BUDGET_SPEND;
    call.payload = spend;
    call.payload_length = sizeof(spend);
    CHECK(submit(&call, &effects, &result) == 0);
    CHECK(result == LXP_ERR_UNKNOWN_FIELD);
    CHECK(env.owner->balance.lo == 750U);
    CHECK(env.recipient->balance.lo == 250U);
    CHECK(env.budget_account->balance.lo == 0U);
    CHECK(lxp_state_store_destroy(&env.state) == LXP_OK);
    return 0;
}

int main(void)
{
    if (store_path() != 0) return 1;
    return dispatch_path();
}
