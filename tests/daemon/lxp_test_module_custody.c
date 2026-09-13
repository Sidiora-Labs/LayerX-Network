#include "lxp_test_epoch_modules.h"
#include "layerx/lx_perps.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "module custody check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

static void put_u64(uint8_t bytes[8], uint64_t value)
{
    for (size_t i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static int account_name(uint16_t module, unsigned index, const uint8_t object[32],
                         uint8_t name[LX_ACCOUNT_NAME_MAX], size_t *length, uint8_t id[32])
{
    static const char digits[] = "0123456789abcdef";
    const char *prefix = module == LXP_MODULE_ESCROW ? "agent:did:key:alice:escrow:" :
        module == LXP_MODULE_BUDGET ? "agent:did:key:alice:budget:" :
        module == LXP_MODULE_STREAM ? "agent:did:key:alice:stream:" :
        index == 0U ? "system:liquidity:" : "system:funding:";
    const char *suffix = module == LXP_MODULE_PERPS && index != 0U ?
        index == 1U ? ":long" : ":short" : "";
    size_t cursor = strlen(prefix);
    (void)memcpy(name, prefix, cursor);
    for (size_t i = 0U; i < 32U; ++i) {
        name[cursor++] = (uint8_t)digits[object[i] >> 4U];
        name[cursor++] = (uint8_t)digits[object[i] & 15U];
    }
    (void)memcpy(name + cursor, suffix, strlen(suffix));
    *length = cursor + strlen(suffix);
    REQUIRE(lx_account_id_from_string(name, *length, id) == LXP_OK);
    return 0;
}

static int run(uint16_t module, unsigned index)
{
    static epoch_fixture fixture;
    static lxp_module_ctx ctx;
    static lxp_effect_buffer effects;
    static uint8_t arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES + EPOCH_FIXTURE_ARENA_BYTES];
    static const uint8_t seed[32] = {1U};
    static const uint8_t did[] = "did:key:alice";
    static const lx_account_kind perps_kinds[] = {LX_ACCOUNT_SYSTEM_LIQUIDITY,
        LX_ACCOUNT_SYSTEM_FUNDING_LONG, LX_ACCOUNT_SYSTEM_FUNDING_SHORT};
    lxp_prepared_module_transition *prepared = NULL;
    lxp_activity activity = {0};
    const lxp_module_registration *registration;
    lx_account_registration valid;
    lx_account *owner, *account;
    uint8_t object[32] = {0x81U}, identifier[32], actor[32], public_key[32], signature[64];
    uint8_t digest[32], before[32], preview[32], committed[32], token[32] = {1U};
    uint8_t name[LX_ACCOUNT_NAME_MAX], payload[LX_PERPS_MARKET_BYTES] = {0};
    lxp_byte_span encoded;
    size_t name_length, payload_length = 0U, public_length = 32U, signature_length = 64U;
    void *decoded = NULL;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    EVP_MD_CTX *signer = EVP_MD_CTX_new();
    REQUIRE(key != NULL && signer != NULL);
    REQUIRE(EVP_PKEY_get_raw_public_key(key, public_key, &public_length) == 1 && public_length == 32U);
    REQUIRE(epoch_fixture_open(&fixture, &module, 1U) == LXP_OK);
    REQUIRE(lxp_arena_init(&fixture.arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    REQUIRE(epoch_fixture_account(&fixture, "agent:did:key:alice:main", 1U, 0U, &owner) == LXP_OK);
    REQUIRE(epoch_fixture_bind(&fixture) == LXP_OK);
    REQUIRE(lxp_did_id_derive(did, sizeof(did) - 1U, actor) == LXP_OK);
    REQUIRE(account_name(module, index, object, name, &name_length, identifier) == 0);
    if (module == LXP_MODULE_ESCROW) {
        (void)memcpy(payload, object, 32U);
        (void)memcpy(payload + 32U, owner->id, 32U);
        (void)memcpy(payload + 64U, identifier, 32U);
        (void)memcpy(payload + 96U, owner->id, 32U);
        (void)memcpy(payload + 128U, owner->id, 32U);
        (void)memcpy(payload + 160U, fixture.asset.asset_id, 32U);
        put_u64(payload + 200U, 100U); put_u64(payload + 208U, 2000U);
        put_u64(payload + 216U, 1000U); payload[224U] = 1U; payload[256U] = 1U;
        payload_length = LX_ESCROW_OPEN_PAYLOAD_BYTES;
    } else if (module == LXP_MODULE_BUDGET) {
        payload[1] = 1U;
        (void)memcpy(payload + 2U, object, 32U);
        (void)memcpy(payload + 34U, identifier, 32U);
        (void)memcpy(payload + 66U, fixture.asset.asset_id, 32U);
        payload[98U] = 1U;
        put_u64(payload + 138U, 10U); put_u64(payload + 170U, 20U);
        put_u64(payload + 178U, 1000U); put_u64(payload + 186U, 1000U);
        put_u64(payload + 194U, 10000U); payload[210U] = LX_BUDGET_ROLLOVER_NONE;
        payload_length = LX_BUDGET_CREATE_PAYLOAD_BYTES;
    } else if (module == LXP_MODULE_STREAM) {
        lx_stream_open_payload stream = {0};
        (void)memcpy(stream.record.stream_id, object, 32U);
        (void)memcpy(stream.record.stream_account, identifier, 32U);
        (void)memcpy(stream.record.recipient, owner->id, 32U);
        (void)memcpy(stream.record.asset_id, fixture.asset.asset_id, 32U);
        stream.record.mode = LX_STREAM_MODE_TIME;
        stream.record.rate = (lxp_u128){0U, 1U}; stream.record.rate_unit = 1000U;
        stream.record.start_timestamp = 1000U; stream.record.end_timestamp = 10000U;
        stream.record.total_cap = stream.initial_funding = (lxp_u128){0U, 100U};
        REQUIRE(lx_stream_open_encode(&stream, payload, sizeof(payload), &payload_length) == LXP_OK);
    } else {
        lx_perps_market market = {0};
        uint8_t unused[LX_ACCOUNT_NAME_MAX];
        size_t unused_length;
        (void)memcpy(market.market_id, object, 32U);
        (void)memcpy(market.quote_asset, fixture.asset.asset_id, 32U);
        (void)memcpy(market.administrator, actor, 32U);
        REQUIRE(account_name(module, 0U, object, unused, &unused_length, market.liquidity_account_id) == 0);
        REQUIRE(account_name(module, 1U, object, unused, &unused_length, market.long_funding_account_id) == 0);
        REQUIRE(account_name(module, 2U, object, unused, &unused_length, market.short_funding_account_id) == 0);
        REQUIRE(lx_account_id_from_string((const uint8_t *)"system:insurance", 16U, market.insurance_account_id) == LXP_OK);
        market.contract_size = market.tick_size = market.lot_size = market.price_scale = (lxp_u128){0U, 1U};
        market.initial_margin_ratio_bps = 1000U; market.maintenance_margin_ratio_bps = 500U;
        market.liquidation_fee_bps = 10U; market.liquidator_share_bps = 6000U;
        market.maximum_funding_rate_bps = 1000U; market.maximum_deviation_basis_points = 10000U;
        market.funding_interval_ms = 1000U; market.maximum_oracle_staleness_ms = 100000U;
        market.minimum_price = (lxp_u128){0U, 1U}; market.maximum_price = (lxp_u128){0U, 1000000U};
        market.permitted_oracle_key_count = 1U; market.parameter_version = 1U;
        (void)memcpy(market.permitted_oracle_keys[0], public_key, 32U);
        REQUIRE(lx_perps_market_encode(&market, payload) == LXP_OK);
        payload_length = sizeof(payload);
    }
    activity.protocol_version = 3U; activity.network_id = EPOCH_FIXTURE_NETWORK_ID;
    activity.activity_type = (uint32_t)module << 16U | 1U;
    activity.actor_did = (lxp_byte_span){did, sizeof(did) - 1U};
    activity.authority = (lxp_byte_span){public_key, 32U};
    activity.payload = (lxp_byte_span){payload, payload_length};
    activity.signature = (lxp_byte_span){signature, 64U};
    activity.timestamp_bound = (lxp_timestamp_bound){1000U, 2000U};
    activity.idempotency_key[0] = 1U;
    REQUIRE(lxp_hash_payload(payload, payload_length, activity.payload_hash) == LXP_OK);
    REQUIRE(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    REQUIRE(EVP_DigestSignInit(signer, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(signer, signature, &signature_length, digest, sizeof(digest)) == 1 && signature_length == 64U);
    EVP_MD_CTX_free(signer); EVP_PKEY_free(key);
    REQUIRE(lxp_state_root(&fixture.kernel, before) == LXP_OK);
    REQUIRE(lxp_state_journal_open(&fixture.state, 1U, &fixture.journal) == LXP_OK);
    REQUIRE(epoch_fixture_ctx(&fixture, &ctx, &effects, module, 1000U) == LXP_OK);
    ctx.protocol_version = 3U;
    REQUIRE(lxp_activity_encode(&activity, &fixture.arena, &encoded) == LXP_OK);
    REQUIRE(lxp_activity_id(encoded.bytes, encoded.length, ctx.activity_id) == LXP_OK);
    REQUIRE(lxp_kernel_module_for_activity(&fixture.kernel, activity.activity_type, 1U, &registration) == LXP_OK);
    REQUIRE(registration->iface->decode(&ctx, 1U, payload, payload_length, &decoded) == LXP_OK);
    if (module == LXP_MODULE_PERPS)
        REQUIRE(lxp_ctx_account_stage_perps_market(&ctx, &activity, object, actor,
            fixture.asset.asset_id, identifier, perps_kinds[index], true) == LXP_OK);
    else
        REQUIRE(lxp_ctx_account_stage_module_custody(&ctx, &activity, object,
            fixture.asset.asset_id, identifier, &account) == LXP_OK);
    REQUIRE(ctx.staged_account_count == 1U && fixture.accounts.count == 1U);
    valid = ctx.staged_accounts[0];
    REQUIRE(lx_account_registration_commit(&fixture.accounts, &valid, &account) == LXP_FATAL_INVARIANT);
    REQUIRE(lxp_module_ctx_prepare_commit(&ctx) == LXP_OK);
    ctx.commit_prepared = false;
    for (unsigned mutation = 0U; mutation < 7U; ++mutation) {
        ctx.staged_accounts[0] = valid;
        if (mutation == 0U) ctx.staged_accounts[0].account.asset_id[0] ^= 1U;
        if (mutation == 1U) ctx.staged_accounts[0].account.created_at_sequence++;
        if (mutation == 2U) ctx.staged_accounts[0].account.kind = LX_ACCOUNT_AGENT_MAIN;
        if (mutation == 3U) ctx.staged_accounts[0].account.id[0] ^= 1U;
        if (mutation == 4U) ctx.activity_id[0] ^= 1U;
        if (mutation == 5U || mutation == 6U) {
            size_t position = valid.account.name_length - (module == LXP_MODULE_PERPS && index != 0U ? index == 1U ? 6U : 7U : 1U);
            ctx.staged_accounts[0].account.name[position] = '1';
            if (mutation == 6U && module != LXP_MODULE_PERPS)
                ctx.staged_accounts[0].account.name[14U] = 'b';
            REQUIRE(lx_account_id_from_string(ctx.staged_accounts[0].account.name,
                ctx.staged_accounts[0].account.name_length, ctx.staged_accounts[0].account.id) == LXP_OK);
            REQUIRE(lx_account_validate_canonical(&ctx.staged_accounts[0].account) == LXP_OK);
        }
        REQUIRE(lxp_module_ctx_prepare_commit(&ctx) != LXP_OK);
        REQUIRE(!ctx.commit_prepared && fixture.accounts.count == 1U);
        if (mutation == 4U) ctx.activity_id[0] ^= 1U;
    }
    ctx.staged_accounts[0] = valid;
    REQUIRE(lxp_module_ctx_prepare_commit(&ctx) == LXP_OK);
    REQUIRE(lxp_module_ctx_preview_state_root(&ctx, &fixture.journal, preview) == LXP_OK);
    REQUIRE(lxp_module_ctx_export_prepared(&ctx, &effects, token, &prepared) == LXP_OK);
    lxp_module_ctx_rollback(&ctx);
    REQUIRE(lxp_state_journal_rollback(&fixture.journal) == LXP_OK);
    REQUIRE(lxp_state_root(&fixture.kernel, committed) == LXP_OK && memcmp(before, committed, 32U) == 0);
    REQUIRE(lxp_state_journal_open(&fixture.state, 1U, &fixture.journal) == LXP_OK);
    REQUIRE(epoch_fixture_ctx(&fixture, &ctx, &effects, module, 1000U) == LXP_OK);
    ctx.protocol_version = 3U;
    REQUIRE(lxp_activity_encode(&activity, &fixture.arena, &encoded) == LXP_OK);
    REQUIRE(lxp_activity_id(encoded.bytes, encoded.length, ctx.activity_id) == LXP_OK);
    REQUIRE(lxp_module_ctx_import_prepared(&ctx, prepared, token, &effects) == LXP_OK);
    REQUIRE(lxp_state_journal_commit(&fixture.journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_commit(&ctx) == LXP_OK);
    REQUIRE(lxp_state_root(&fixture.kernel, committed) == LXP_OK && memcmp(preview, committed, 32U) == 0);
    REQUIRE(fixture.accounts.count == 2U && lx_account_lookup(&fixture.accounts,
        name, name_length, identifier, &account) == LXP_OK);
    REQUIRE(account->name_length == name_length && memcmp(account->name, name, name_length) == 0);
    REQUIRE(memcmp(account->asset_id, fixture.asset.asset_id, 32U) == 0 && account->created_at_sequence == 1U);
    lxp_prepared_module_transition_destroy(prepared);
    return epoch_fixture_close(&fixture);
}

int main(void)
{
    REQUIRE(run(LXP_MODULE_ESCROW, 0U) == 0);
    REQUIRE(run(LXP_MODULE_BUDGET, 0U) == 0);
    REQUIRE(run(LXP_MODULE_STREAM, 0U) == 0);
    for (unsigned i = 0U; i < 3U; ++i) REQUIRE(run(LXP_MODULE_PERPS, i) == 0);
    return 0;
}
